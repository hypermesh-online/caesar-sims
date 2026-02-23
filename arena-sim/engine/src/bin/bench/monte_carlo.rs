// Monte Carlo Infrastructure — N runs per scenario with statistical aggregation
// Each scenario runs N=30 times with seeds 0..N-1, computing mean ± 95% CI

use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use arena_engine::*;

use crate::report::*;
use crate::scenarios::Scenario;
use crate::traffic::TrafficGenerator;
use crate::metrics::{PegTracker, ConservationTracker};
use crate::time_series::TimeSeriesRecorder;

use std::time::Instant;

/// Run a single scenario iteration with a specific seed.
pub fn run_single(
    scenario: &Scenario,
    seed: u64,
    time_series_dir: Option<&std::path::Path>,
) -> BenchResult {
    let start = Instant::now();
    let mut sim = ArenaSimulation::new(scenario.nodes);
    sim.set_gold_price(scenario.gold);
    sim.set_panic_level(scenario.panic);

    // Suppress engine traffic — bench injects via Poisson
    sim.set_demand_factor(0.0);

    // Set up Poisson traffic generator
    let ingress_nodes: Vec<u32> = (0..scenario.nodes)
        .filter(|i| i % 4 == 0) // Ingress nodes
        .collect();
    let rng = ChaCha8Rng::seed_from_u64(seed);
    let mut traffic = TrafficGenerator::new(rng, ingress_nodes);
    let demand_scale = (scenario.nodes as f64 / 24.0).sqrt();
    let _base_lambda = TrafficGenerator::compute_lambda(scenario.demand, scenario.nodes);

    // Metric trackers
    let mut peg = PegTracker::new();
    let mut conservation = ConservationTracker::new();
    let mut time_series = if time_series_dir.is_some() {
        Some(TimeSeriesRecorder::new())
    } else {
        None
    };

    let mut peak_fee: f64 = 0.0;
    let mut fee_cap_breaches: u32 = 0;
    let mut all_packets_settled_final = true;
    let mut cost_certainty_violations: u32 = 0;
    let mut audit_trail_violations: u32 = 0;
    let mut conservation_holds = true;
    let mut last_fee_rate = 0.0_f64;
    let mut last_state: Option<WorldState> = None;

    let caps = [0.05_f64, 0.02, 0.005, 0.001];

    // Pre-scenario setup (kill nodes, set liquidity, etc.)
    if let Some(setup) = &scenario.setup {
        setup(&mut sim);
    }

    for tick in 0..scenario.ticks {
        // Apply curves
        let gold = if let Some(curve) = scenario.gold_curve {
            curve(tick)
        } else {
            scenario.gold
        };
        sim.set_gold_price(gold);

        let demand = if let Some(curve) = scenario.demand_curve {
            curve(tick)
        } else {
            scenario.demand
        };
        // Modulate Poisson lambda via demand curve
        let current_lambda = demand * 5.0 * (scenario.nodes as f64 / 24.0).sqrt();

        if let Some(curve) = scenario.panic_curve {
            sim.set_panic_level(curve(tick));
        }

        // Mid-scenario events (e.g., kill nodes at tick 500)
        if let Some(event) = &scenario.mid_event {
            event(&mut sim, tick);
        }

        // Inject Poisson traffic (use last tick's fee rate for demand destruction)
        traffic.set_fee_rate(last_fee_rate);
        let spawns = traffic.generate_tick(current_lambda);
        for (node_id, amount) in spawns {
            sim.spawn_packet(node_id, amount);
        }

        // Tick the engine
        let result = sim.tick_core();
        last_fee_rate = result.state.current_fee_rate;
        peak_fee = peak_fee.max(result.state.current_fee_rate);

        // Track metrics
        peg.record_tick(&result.state);
        conservation.record_tick(&result.state);

        if let Some(ref mut ts) = time_series {
            ts.record(&result.state);
        }

        // Conservation check (raw)
        if result.state.total_value_leaked.abs() > scenario.criteria.max_conservation_error {
            conservation_holds = false;
        }

        // Fee cap breach check
        let tier_rates = result.state.tier_fee_rates;
        for t in 0..4 {
            if tier_rates[t] > caps[t] + 0.0001 {
                fee_cap_breaches += 1;
            }
        }

        // Fiduciary checks
        for p in &result.active_packets {
            if p.fee_budget > 0.0 && p.fees_consumed > p.fee_budget + 0.0001 {
                cost_certainty_violations += 1;
            }
            if p.route_history.is_empty() {
                audit_trail_violations += 1;
            }
            if p.status == PacketStatus::Settled {
                all_packets_settled_final = false;
            }
        }

        last_state = Some(result.state);
    }

    // Write time series if enabled
    if let (Some(ts), Some(dir)) = (&time_series, time_series_dir) {
        let path = dir.join(format!("seed-{}.jsonl", seed));
        if let Err(e) = ts.write_jsonl(&path) {
            eprintln!("  Warning: failed to write time series: {}", e);
        }
    }

    let elapsed = start.elapsed();
    let elapsed_ms = elapsed.as_millis();
    let elapsed_secs = elapsed.as_secs_f64().max(0.001);

    let state = last_state.as_ref().expect("No ticks executed");
    let settled = state.settlement_count;
    // Use bench-tracked spawn count (engine's spawn_count won't be incremented
    // since we use spawn_packet() which only increments total_input)
    let spawned = traffic.spawn_count.max(1);
    let settlement_rate = (settled as f64 / spawned as f64) * 100.0;

    let normalized_conservation = conservation.normalized_error();

    // Evaluate pass/fail
    let mut pass = state.total_value_leaked.abs() <= scenario.criteria.max_conservation_error;
    if let Some(min_rate) = scenario.criteria.min_settlement_rate {
        if settled > 0 && settlement_rate < min_rate {
            pass = false;
        }
    }
    if let Some(max_breaches) = scenario.criteria.max_fee_cap_breaches {
        if fee_cap_breaches > max_breaches {
            pass = false;
        }
    }
    if scenario.criteria.require_settlement_finality && !all_packets_settled_final {
        pass = false;
    }
    if scenario.criteria.require_cost_certainty && cost_certainty_violations > 0 {
        pass = false;
    }
    if scenario.criteria.require_audit_trail && audit_trail_violations > 0 {
        pass = false;
    }
    if scenario.criteria.require_zero_stuck && state.held_count > 0 {
        pass = false;
    }
    if let Some(max_held) = scenario.criteria.max_held_at_end {
        if state.held_count > max_held {
            pass = false;
        }
    }

    BenchResult {
        scenario: scenario.label.to_string(),
        name: scenario.name.to_string(),
        category: scenario.category.to_string(),
        seed,
        pass,
        settlement_count: settled,
        revert_count: state.revert_count,
        spawn_count: spawned,
        settlement_rate,
        conservation_error: state.total_value_leaked.abs(),
        normalized_conservation_error: normalized_conservation,
        avg_fee: state.current_fee_rate * 100.0,
        peak_fee: peak_fee * 100.0,
        dissolved_count: state.dissolved_count,
        held_count: state.held_count,
        fee_cap_breaches,
        settlement_finality: all_packets_settled_final,
        cost_certainty: cost_certainty_violations == 0,
        audit_trail: audit_trail_violations == 0,
        tier_breakdown: state.tier_distribution,
        ticks: scenario.ticks,
        elapsed_ms,
        packets_per_tick: spawned as f64 / scenario.ticks as f64,
        demand_scale_factor: demand_scale,
        egress_profit_total: state.total_rewards_egress,
        transit_profit_total: state.total_rewards_transit,
        demurrage_total: state.total_demurrage_burned,
        conservation_holds,
        final_held_count: state.held_count,
        final_orbit_count: state.orbit_count,
        throughput_per_sec: scenario.ticks as f64 / elapsed_secs,
        peg_elasticity_pct: peg.elasticity_pct(),
        max_normalized_conservation: normalized_conservation,
    }
}

/// Run Monte Carlo: N runs of a scenario, aggregate stats.
pub fn run_monte_carlo(
    scenario: &Scenario,
    n_runs: usize,
    base_seed: u64,
    time_series_base: Option<&std::path::Path>,
) -> MonteCarloReport {
    let ts_dir = time_series_base.map(|base| base.join(&scenario.name.to_lowercase()));

    let mut results = Vec::with_capacity(n_runs);
    for i in 0..n_runs {
        let seed = base_seed + i as u64;
        let result = run_single(scenario, seed, ts_dir.as_deref());
        results.push(result);
    }

    aggregate(scenario, results)
}

/// Aggregate individual runs into a MonteCarloReport.
fn aggregate(scenario: &Scenario, results: Vec<BenchResult>) -> MonteCarloReport {
    let n = results.len();
    let passed = results.iter().filter(|r| r.pass).count();
    let pass_rate = passed as f64 / n as f64;

    let conservation_error = Stats::from_samples(
        &results.iter().map(|r| r.conservation_error).collect::<Vec<_>>()
    );
    let normalized_conservation_error = Stats::from_samples(
        &results.iter().map(|r| r.normalized_conservation_error).collect::<Vec<_>>()
    );
    let settlement_rate = Stats::from_samples(
        &results.iter().map(|r| r.settlement_rate).collect::<Vec<_>>()
    );
    let peg_elasticity_pct = Stats::from_samples(
        &results.iter().map(|r| r.peg_elasticity_pct).collect::<Vec<_>>()
    );
    let egress_profit = Stats::from_samples(
        &results.iter().map(|r| r.egress_profit_total).collect::<Vec<_>>()
    );
    let transit_profit = Stats::from_samples(
        &results.iter().map(|r| r.transit_profit_total).collect::<Vec<_>>()
    );
    let demurrage_total = Stats::from_samples(
        &results.iter().map(|r| r.demurrage_total).collect::<Vec<_>>()
    );
    let held_count = Stats::from_samples(
        &results.iter().map(|r| r.held_count as f64).collect::<Vec<_>>()
    );
    let elapsed_ms = Stats::from_samples(
        &results.iter().map(|r| r.elapsed_ms as f64).collect::<Vec<_>>()
    );
    let throughput_per_sec = Stats::from_samples(
        &results.iter().map(|r| r.throughput_per_sec).collect::<Vec<_>>()
    );
    let packets_per_tick = Stats::from_samples(
        &results.iter().map(|r| r.packets_per_tick).collect::<Vec<_>>()
    );

    MonteCarloReport {
        scenario_name: scenario.name.to_string(),
        label: scenario.label.to_string(),
        category: scenario.category.to_string(),
        n_runs: n,
        pass_rate,
        conservation_error,
        normalized_conservation_error,
        settlement_rate,
        peg_elasticity_pct,
        egress_profit,
        transit_profit,
        demurrage_total,
        held_count,
        elapsed_ms,
        throughput_per_sec,
        packets_per_tick,
        individual_runs: results,
    }
}
