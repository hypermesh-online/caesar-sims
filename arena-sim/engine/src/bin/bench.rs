// Arena Benchmark Runner — economist-grade whitepaper-aligned validation suite
// Writes results to benchmark-results/bench-{timestamp}.json

use arena_engine::*;
use serde::Serialize;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

// ─── Scenario Configuration ──────────────────────────────────────────────────

struct Scenario {
    name: &'static str,
    label: &'static str,
    category: &'static str,
    nodes: u32,
    ticks: u64,
    gold: f64,
    demand: f64,
    panic: f64,
    gold_curve: Option<fn(u64) -> f64>,
    demand_curve: Option<fn(u64) -> f64>,
    panic_curve: Option<fn(u64) -> f64>,
    criteria: PassCriteria,
}

struct PassCriteria {
    max_conservation_error: f64,
    min_settlement_rate: Option<f64>,
    max_fee_cap_breaches: Option<u32>,
    require_settlement_finality: bool,
    require_cost_certainty: bool,
    require_audit_trail: bool,
    require_zero_stuck: bool,
    max_held_at_end: Option<u32>,
}

impl Default for PassCriteria {
    fn default() -> Self {
        Self {
            max_conservation_error: 1.0,
            min_settlement_rate: None,
            max_fee_cap_breaches: None,
            require_settlement_finality: false,
            require_cost_certainty: false,
            require_audit_trail: false,
            require_zero_stuck: false,
            max_held_at_end: None,
        }
    }
}

// ─── Curve Functions ─────────────────────────────────────────────────────────

fn black_swan_gold(tick: u64) -> f64 {
    let t = tick as f64;
    if tick < 100 { 2600.0 - t * 11.0 }
    else if tick < 200 { 1500.0 + (t - 100.0) * 3.0 }
    else { 1800.0 }
}

fn governor_stress_gold(tick: u64) -> f64 {
    2600.0 + (tick as f64 / 10.0).sin() * 800.0
}

fn governor_stress_demand(tick: u64) -> f64 {
    0.5 + (tick as f64 / 15.0).sin() * 0.4
}

// ─── Real-World 2025-2026 Curves (per-gram pricing) ─────────────────────────

fn bull_2025_gold(tick: u64) -> f64 {
    let t = tick as f64;
    let progress = t / 600.0;
    let s_curve = 1.0 / (1.0 + (-12.0 * (progress - 0.4)).exp());
    83.5 + (141.5 - 83.5) * s_curve
}

fn bull_2025_demand(tick: u64) -> f64 {
    let t = tick as f64;
    let base = 0.3 + 0.5 * (t / 600.0).min(1.0);
    (base + 0.05 * (t / 20.0).sin()).clamp(0.1, 0.95)
}

fn flash_crash_oct25_gold(tick: u64) -> f64 {
    let t = tick as f64;
    if tick < 50 { 141.0 }
    else if tick < 60 { 141.0 - (t - 50.0) * 0.9 }
    else if tick < 100 { 132.0 + (t - 60.0) * 0.15 }
    else { 138.0 + 0.5 * ((t - 100.0) / 15.0).sin() }
}

fn flash_crash_oct25_demand(tick: u64) -> f64 {
    if tick < 50 { 0.5 }
    else if tick < 70 { 0.9 }
    else if tick < 120 { 0.7 }
    else { 0.4 }
}

fn flash_crash_oct25_panic(tick: u64) -> f64 {
    if tick < 50 { 0.0 }
    else if tick < 65 { 0.8 }
    else if tick < 100 { 0.3 }
    else { 0.05 }
}

fn fed_correction_26_gold(tick: u64) -> f64 {
    let t = tick as f64;
    if tick < 30 { 177.0 }
    else if tick < 80 { 177.0 - (t - 30.0) * 0.46 }
    else if tick < 150 { 154.0 + (t - 80.0) * 0.1 }
    else { 161.0 + 1.0 * ((t - 150.0) / 20.0).sin() }
}

fn fed_correction_26_demand(tick: u64) -> f64 {
    if tick < 30 { 0.6 }
    else if tick < 80 { 0.2 }
    else { 0.35 }
}

// ─── Whitepaper Curve Functions ──────────────────────────────────────────────

fn peg_elasticity_gold(tick: u64) -> f64 {
    let t = tick as f64;
    // Oscillate gold price +/-50% around $163/g with varying frequency
    163.0 + 81.5 * (t / 100.0).sin() * (1.0 + 0.3 * (t / 300.0).sin())
}

// ─── Scenario Definitions ────────────────────────────────────────────────────

fn scenarios() -> Vec<Scenario> {
    vec![
        // ─── Market Conditions (5) ──────────────────────────────────────
        Scenario { name: "NORMAL_MARKET", label: "Normal Market", category: "market",
            gold: 2600.0, demand: 0.3, panic: 0.0, nodes: 24, ticks: 600,
            gold_curve: None, demand_curve: None, panic_curve: None,
            criteria: PassCriteria { min_settlement_rate: Some(50.0), ..Default::default() } },
        Scenario { name: "BULL_RUN", label: "Bull Run", category: "market",
            gold: 3200.0, demand: 0.8, panic: 0.05, nodes: 24, ticks: 200,
            gold_curve: None, demand_curve: None, panic_curve: None,
            criteria: PassCriteria { min_settlement_rate: Some(15.0), ..Default::default() } },
        Scenario { name: "BEAR_MARKET", label: "Bear Market", category: "market",
            gold: 1800.0, demand: 0.1, panic: 0.4, nodes: 24, ticks: 200,
            gold_curve: None, demand_curve: None, panic_curve: None,
            criteria: PassCriteria::default() },
        Scenario { name: "BLACK_SWAN", label: "Black Swan", category: "market",
            gold: 2600.0, demand: 0.9, panic: 0.95, nodes: 24, ticks: 300,
            gold_curve: Some(black_swan_gold), demand_curve: None, panic_curve: None,
            criteria: PassCriteria { max_conservation_error: 2.0, ..Default::default() } },
        Scenario { name: "STAGFLATION", label: "Stagflation", category: "market",
            gold: 2600.0, demand: 0.05, panic: 0.3, nodes: 24, ticks: 200,
            gold_curve: None, demand_curve: None, panic_curve: None,
            criteria: PassCriteria::default() },

        // ─── Stress Tests (8) ───────────────────────────────────────────
        Scenario { name: "SCALE_100", label: "Scale 100", category: "stress",
            gold: 2600.0, demand: 0.3, panic: 0.0, nodes: 100, ticks: 200,
            gold_curve: None, demand_curve: None, panic_curve: None,
            criteria: PassCriteria { max_conservation_error: 5.0, min_settlement_rate: Some(30.0), ..Default::default() } },
        Scenario { name: "SCALE_250", label: "Scale 250", category: "stress",
            gold: 2600.0, demand: 0.3, panic: 0.0, nodes: 250, ticks: 200,
            gold_curve: None, demand_curve: None, panic_curve: None,
            criteria: PassCriteria { max_conservation_error: 10.0, min_settlement_rate: Some(20.0), ..Default::default() } },
        Scenario { name: "SCALE_500", label: "Scale 500", category: "stress",
            gold: 2600.0, demand: 0.5, panic: 0.0, nodes: 500, ticks: 200,
            gold_curve: None, demand_curve: None, panic_curve: None,
            criteria: PassCriteria { max_conservation_error: 20.0, ..Default::default() } },
        Scenario { name: "TIER_ISOLATION", label: "Tier Isolation", category: "stress",
            gold: 2600.0, demand: 0.5, panic: 0.0, nodes: 24, ticks: 200,
            gold_curve: None, demand_curve: None, panic_curve: None,
            criteria: PassCriteria::default() },
        Scenario { name: "FEE_CAP_STRESS", label: "Fee Cap Stress", category: "stress",
            gold: 2600.0, demand: 0.95, panic: 0.8, nodes: 24, ticks: 300,
            gold_curve: None, demand_curve: None, panic_curve: None,
            criteria: PassCriteria { max_conservation_error: 2.0, max_fee_cap_breaches: Some(0), ..Default::default() } },
        Scenario { name: "GOVERNOR_STRESS", label: "Governor Stress", category: "stress",
            gold: 2600.0, demand: 0.5, panic: 0.0, nodes: 24, ticks: 200,
            gold_curve: Some(governor_stress_gold), demand_curve: Some(governor_stress_demand), panic_curve: None,
            criteria: PassCriteria { max_conservation_error: 2.0, ..Default::default() } },
        Scenario { name: "DISSOLUTION_TEST", label: "Dissolution", category: "stress",
            gold: 2600.0, demand: 0.3, panic: 0.0, nodes: 24, ticks: 8000,
            gold_curve: None, demand_curve: None, panic_curve: None,
            criteria: PassCriteria::default() },
        Scenario { name: "AML_DETECTION", label: "AML Detection", category: "stress",
            gold: 2600.0, demand: 0.9, panic: 0.0, nodes: 24, ticks: 200,
            gold_curve: None, demand_curve: None, panic_curve: None,
            criteria: PassCriteria::default() },

        // ─── Fiduciary Tests (3) ────────────────────────────────────────
        Scenario { name: "SETTLEMENT_FINALITY", label: "Settlement Finality", category: "fiduciary",
            gold: 2600.0, demand: 0.5, panic: 0.0, nodes: 24, ticks: 200,
            gold_curve: None, demand_curve: None, panic_curve: None,
            criteria: PassCriteria { max_conservation_error: 0.01, require_settlement_finality: true, ..Default::default() } },
        Scenario { name: "COST_CERTAINTY", label: "Cost Certainty", category: "fiduciary",
            gold: 2600.0, demand: 0.5, panic: 0.2, nodes: 24, ticks: 200,
            gold_curve: None, demand_curve: None, panic_curve: None,
            criteria: PassCriteria { max_conservation_error: 0.1, require_cost_certainty: true, ..Default::default() } },
        Scenario { name: "AUDIT_TRAIL", label: "Audit Trail", category: "fiduciary",
            gold: 2600.0, demand: 0.3, panic: 0.0, nodes: 24, ticks: 200,
            gold_curve: None, demand_curve: None, panic_curve: None,
            criteria: PassCriteria { max_conservation_error: 0.1, require_audit_trail: true, ..Default::default() } },

        // ─── Real-World 2025-2026 (per-gram, 4 scenarios) ───────────────
        Scenario { name: "RW_BASELINE_2026", label: "RW: Feb 2026 Baseline", category: "real-world",
            gold: 163.0, demand: 0.4, panic: 0.05, nodes: 24, ticks: 600,
            gold_curve: None, demand_curve: None, panic_curve: None,
            criteria: PassCriteria { min_settlement_rate: Some(40.0), ..Default::default() } },
        Scenario { name: "RW_BULL_2025", label: "RW: 2025 Bull Run", category: "real-world",
            gold: 83.5, demand: 0.3, panic: 0.0, nodes: 24, ticks: 600,
            gold_curve: Some(bull_2025_gold), demand_curve: Some(bull_2025_demand), panic_curve: None,
            criteria: PassCriteria { min_settlement_rate: Some(30.0), ..Default::default() } },
        Scenario { name: "RW_FLASH_CRASH_OCT25", label: "RW: Oct25 Flash Crash", category: "real-world",
            gold: 141.0, demand: 0.5, panic: 0.0, nodes: 24, ticks: 300,
            gold_curve: Some(flash_crash_oct25_gold), demand_curve: Some(flash_crash_oct25_demand),
            panic_curve: Some(flash_crash_oct25_panic),
            criteria: PassCriteria { max_conservation_error: 2.0, ..Default::default() } },
        Scenario { name: "RW_FED_CORRECTION_26", label: "RW: 2026 Fed Correction", category: "real-world",
            gold: 177.0, demand: 0.6, panic: 0.1, nodes: 24, ticks: 400,
            gold_curve: Some(fed_correction_26_gold), demand_curve: Some(fed_correction_26_demand), panic_curve: None,
            criteria: PassCriteria { ..Default::default() } },

        // ─── Whitepaper Invariant Tests (4) ─────────────────────────────
        Scenario { name: "WP_NO_FAIL_BANK_RUN", label: "WP: Bank Run No-Fail", category: "whitepaper",
            gold: 163.0, demand: 0.95, panic: 0.9, nodes: 100, ticks: 2000,
            gold_curve: None, demand_curve: None, panic_curve: None,
            criteria: PassCriteria { max_conservation_error: 10.0, max_held_at_end: Some(10000), ..Default::default() } },
        Scenario { name: "WP_PEG_ELASTICITY", label: "WP: Peg Elasticity", category: "whitepaper",
            gold: 163.0, demand: 0.5, panic: 0.0, nodes: 100, ticks: 2000,
            gold_curve: Some(peg_elasticity_gold), demand_curve: None, panic_curve: None,
            criteria: PassCriteria { max_conservation_error: 10.0, ..Default::default() } },
        Scenario { name: "WP_INCENTIVE_DROUGHT", label: "WP: Incentive Drought", category: "whitepaper",
            gold: 163.0, demand: 0.8, panic: 0.7, nodes: 100, ticks: 2000,
            gold_curve: None, demand_curve: None, panic_curve: None,
            criteria: PassCriteria { max_conservation_error: 20.0, ..Default::default() } },
        Scenario { name: "WP_DEMURRAGE_LOOP", label: "WP: Demurrage Loop Decay", category: "whitepaper",
            gold: 163.0, demand: 0.3, panic: 0.0, nodes: 24, ticks: 8000,
            gold_curve: None, demand_curve: None, panic_curve: None,
            criteria: PassCriteria { max_held_at_end: Some(2000), ..Default::default() } },

        // ─── Scale Validation (4) ───────────────────────────────────────
        Scenario { name: "SCALE_100_V2", label: "Scale: 100 Nodes", category: "scale",
            gold: 163.0, demand: 0.5, panic: 0.0, nodes: 100, ticks: 2000,
            gold_curve: None, demand_curve: None, panic_curve: None,
            criteria: PassCriteria { max_conservation_error: 5.0, min_settlement_rate: Some(40.0), ..Default::default() } },
        Scenario { name: "SCALE_1K", label: "Scale: 1K Nodes", category: "scale",
            gold: 163.0, demand: 0.5, panic: 0.0, nodes: 1000, ticks: 2000,
            gold_curve: None, demand_curve: None, panic_curve: None,
            criteria: PassCriteria { max_conservation_error: 50.0, min_settlement_rate: Some(30.0), ..Default::default() } },
        Scenario { name: "SCALE_5K", label: "Scale: 5K Nodes", category: "scale",
            gold: 163.0, demand: 0.4, panic: 0.0, nodes: 5000, ticks: 1000,
            gold_curve: None, demand_curve: None, panic_curve: None,
            criteria: PassCriteria { max_conservation_error: 200.0, ..Default::default() } },
        Scenario { name: "SCALE_10K", label: "Scale: 10K Nodes", category: "scale",
            gold: 163.0, demand: 0.3, panic: 0.0, nodes: 10000, ticks: 500,
            gold_curve: None, demand_curve: None, panic_curve: None,
            criteria: PassCriteria { max_conservation_error: 500.0, ..Default::default() } },

        // ─── Real-World at Scale (2) ────────────────────────────────────
        Scenario { name: "RW_1K_BULL_2025", label: "RW: 1K Bull Run 2025", category: "real-world",
            gold: 83.5, demand: 0.3, panic: 0.0, nodes: 1000, ticks: 2000,
            gold_curve: Some(bull_2025_gold), demand_curve: Some(bull_2025_demand), panic_curve: None,
            criteria: PassCriteria { max_conservation_error: 50.0, min_settlement_rate: Some(30.0), ..Default::default() } },
        Scenario { name: "RW_1K_SOVEREIGN", label: "RW: 1K Sovereign Crisis", category: "real-world",
            gold: 177.0, demand: 0.9, panic: 0.8, nodes: 1000, ticks: 2000,
            gold_curve: Some(black_swan_gold), demand_curve: None, panic_curve: None,
            criteria: PassCriteria { max_conservation_error: 200.0, ..Default::default() } },

        // ─── Stress Envelope (4) ────────────────────────────────────────
        Scenario { name: "STRESS_20K", label: "Stress: 20K Nodes", category: "stress-envelope",
            gold: 163.0, demand: 0.5, panic: 0.0, nodes: 20000, ticks: 500,
            gold_curve: None, demand_curve: None, panic_curve: None,
            criteria: PassCriteria { max_conservation_error: 1000.0, ..Default::default() } },
        Scenario { name: "STRESS_50K_TICKS", label: "Stress: 1K x 50K Ticks", category: "stress-envelope",
            gold: 163.0, demand: 0.5, panic: 0.0, nodes: 1000, ticks: 50000,
            gold_curve: Some(governor_stress_gold), demand_curve: Some(governor_stress_demand), panic_curve: None,
            criteria: PassCriteria { max_conservation_error: 500.0, ..Default::default() } },
        Scenario { name: "STRESS_FULL_PANIC", label: "Stress: 5K Full Panic", category: "stress-envelope",
            gold: 163.0, demand: 0.95, panic: 0.95, nodes: 5000, ticks: 1000,
            gold_curve: Some(black_swan_gold), demand_curve: None, panic_curve: None,
            criteria: PassCriteria { max_conservation_error: 1000.0, ..Default::default() } },
        Scenario { name: "STRESS_100K", label: "Stress: 100K Nodes", category: "stress-envelope",
            gold: 163.0, demand: 0.3, panic: 0.0, nodes: 100000, ticks: 100,
            gold_curve: None, demand_curve: None, panic_curve: None,
            criteria: PassCriteria { max_conservation_error: 10000.0, ..Default::default() } },
    ]
}

// ─── Result Types ────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct BenchReport {
    timestamp: String,
    version: &'static str,
    summary: Summary,
    benchmarks: Vec<BenchResult>,
}

#[derive(Serialize)]
struct Summary {
    total: usize,
    passed: usize,
    failed: usize,
}

#[derive(Serialize)]
struct BenchResult {
    scenario: String,
    name: String,
    category: String,
    pass: bool,
    settlement_count: u32,
    revert_count: u32,
    spawn_count: u32,
    settlement_rate: f64,
    conservation_error: f64,
    avg_fee: f64,
    peak_fee: f64,
    dissolved_count: u32,
    held_count: u32,
    fee_cap_breaches: u32,
    settlement_finality: bool,
    cost_certainty: bool,
    audit_trail: bool,
    tier_breakdown: [u32; 4],
    ticks: u64,
    elapsed_ms: u128,
    // v0.3 whitepaper-aligned fields
    packets_per_tick: f64,
    demand_scale_factor: f64,
    egress_profit_total: f64,
    transit_profit_total: f64,
    demurrage_total: f64,
    conservation_holds: bool,
    final_held_count: u32,
    final_orbit_count: u32,
    throughput_per_sec: f64,
}

// ─── Runner ──────────────────────────────────────────────────────────────────

fn run_scenario(scenario: &Scenario) -> BenchResult {
    let start = Instant::now();
    let mut sim = ArenaSimulation::new(scenario.nodes);
    sim.set_gold_price(scenario.gold);
    sim.set_panic_level(scenario.panic);

    // Scale demand with node count: sqrt(nodes/24)
    let demand_scale = (scenario.nodes as f64 / 24.0).sqrt();
    let effective_demand = scenario.demand * demand_scale;
    sim.set_demand_factor(effective_demand);

    let mut peak_fee: f64 = 0.0;
    let mut fee_cap_breaches: u32 = 0;
    let mut all_packets_settled_final = true;
    let mut cost_certainty_violations: u32 = 0;
    let mut audit_trail_violations: u32 = 0;
    let mut last_state: Option<WorldState> = None;
    let mut conservation_holds = true;

    let caps = [0.05_f64, 0.02, 0.005, 0.001];

    for tick in 0..scenario.ticks {
        if let Some(curve) = scenario.gold_curve {
            sim.set_gold_price(curve(tick));
        }
        if let Some(curve) = scenario.demand_curve {
            sim.set_demand_factor(curve(tick) * demand_scale);
        }
        if let Some(curve) = scenario.panic_curve {
            sim.set_panic_level(curve(tick));
        }

        let result = sim.tick_core();
        peak_fee = peak_fee.max(result.state.current_fee_rate);

        // Track conservation error across all ticks
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

        // Fiduciary checks on active packets
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

    let elapsed = start.elapsed();
    let elapsed_ms = elapsed.as_millis();
    let elapsed_secs = elapsed.as_secs_f64().max(0.001);

    let state = last_state.unwrap();
    let settled = state.settlement_count;
    let spawned = state.spawn_count.max(1);
    let error = state.total_value_leaked.abs();
    let settlement_rate = (settled as f64 / spawned as f64) * 100.0;

    // Collect new metrics from world state
    let egress_profit_total = state.total_rewards_egress;
    let transit_profit_total = state.total_rewards_transit;
    let demurrage_total = state.total_demurrage_burned;
    let final_held_count = state.held_count;
    let final_orbit_count = state.orbit_count;
    let packets_per_tick = spawned as f64 / scenario.ticks as f64;
    let throughput_per_sec = scenario.ticks as f64 / elapsed_secs;

    // Evaluate pass/fail
    let mut pass = error <= scenario.criteria.max_conservation_error;
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
        pass,
        settlement_count: settled,
        revert_count: state.revert_count,
        spawn_count: spawned,
        settlement_rate,
        conservation_error: error,
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
        packets_per_tick,
        demand_scale_factor: demand_scale,
        egress_profit_total,
        transit_profit_total,
        demurrage_total,
        conservation_holds,
        final_held_count,
        final_orbit_count,
        throughput_per_sec,
    }
}

// ─── Main ────────────────────────────────────────────────────────────────────

fn main() {
    let filter: Option<String> = std::env::args().nth(1);
    let all_scenarios = scenarios();

    let to_run: Vec<&Scenario> = match &filter {
        Some(f) => {
            let f_lower = f.to_lowercase();
            all_scenarios.iter()
                .filter(|s| s.name.to_lowercase().contains(&f_lower)
                          || s.label.to_lowercase().contains(&f_lower))
                .collect()
        }
        None => all_scenarios.iter().collect(),
    };

    if to_run.is_empty() {
        eprintln!("No scenarios match filter: {:?}", filter);
        std::process::exit(1);
    }

    println!("\n  Arena Benchmark Runner v0.2.0");
    println!("  Running {} scenario(s)...\n", to_run.len());
    println!("  {:<28} {:>6} {:>8} {:>10} {:>10} {:>6} {:>8}",
        "Scenario", "Pass", "Settle%", "Conserv", "PeakFee%", "Held", "Time");
    println!("  {}", "-".repeat(78));

    let mut results = Vec::new();
    for scenario in &to_run {
        let result = run_scenario(scenario);
        let status = if result.pass { " PASS" } else { " FAIL" };
        println!("  {:<28} {:>6} {:>7.1}% {:>10.4} {:>9.2}% {:>6} {:>5}ms",
            result.scenario, status, result.settlement_rate,
            result.conservation_error, result.peak_fee, result.final_held_count, result.elapsed_ms);
        results.push(result);
    }

    let passed = results.iter().filter(|r| r.pass).count();
    let failed = results.len() - passed;
    println!("  {}", "-".repeat(78));
    println!("  Total: {}  Passed: {}  Failed: {}\n", results.len(), passed, failed);

    // Write JSON report
    let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis();
    let timestamp = format!("{}", ts);

    let report = BenchReport {
        timestamp: timestamp.clone(),
        version: "0.2.0",
        summary: Summary { total: results.len(), passed, failed },
        benchmarks: results,
    };

    let dir = std::path::Path::new("benchmark-results");
    if !dir.exists() {
        std::fs::create_dir_all(dir).expect("Failed to create benchmark-results/");
    }
    let path = dir.join(format!("bench-{}.json", timestamp));
    let json = serde_json::to_string_pretty(&report).expect("Failed to serialize");
    std::fs::write(&path, &json).expect("Failed to write benchmark file");
    println!("  Results saved to: {}\n", path.display());

    if failed > 0 {
        std::process::exit(1);
    }
}
