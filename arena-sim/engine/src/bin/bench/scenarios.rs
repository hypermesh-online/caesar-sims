// Scenario Definitions — all 34 original + 3 whitepaper-exact additions
// Zero engine changes: all scenario logic is in curve functions and setup/event closures

use arena_engine::ArenaSimulation;

// ─── Scenario Configuration ─────────────────────────────────────────────────

pub struct Scenario {
    pub name: &'static str,
    pub label: &'static str,
    pub category: &'static str,
    pub nodes: u32,
    pub ticks: u64,
    pub gold: f64,
    pub demand: f64,
    pub panic: f64,
    pub gold_curve: Option<fn(u64) -> f64>,
    pub demand_curve: Option<fn(u64) -> f64>,
    pub panic_curve: Option<fn(u64) -> f64>,
    pub criteria: PassCriteria,
    /// Pre-run setup (e.g., set_node_crypto for liquidity control)
    pub setup: Option<Box<dyn Fn(&mut ArenaSimulation) + Send + Sync>>,
    /// Mid-simulation events (e.g., kill_node at specific tick)
    pub mid_event: Option<Box<dyn Fn(&mut ArenaSimulation, u64) + Send + Sync>>,
}

pub struct PassCriteria {
    pub max_conservation_error: f64,
    pub min_settlement_rate: Option<f64>,
    pub max_fee_cap_breaches: Option<u32>,
    pub require_settlement_finality: bool,
    pub require_cost_certainty: bool,
    pub require_audit_trail: bool,
    pub require_zero_stuck: bool,
    pub max_held_at_end: Option<u32>,
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

// ─── Curve Functions ────────────────────────────────────────────────────────

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

fn peg_elasticity_gold(tick: u64) -> f64 {
    let t = tick as f64;
    163.0 + 81.5 * (t / 100.0).sin() * (1.0 + 0.3 * (t / 300.0).sin())
}

// ─── Whitepaper-Exact Curve Functions ───────────────────────────────────────

/// Bank Run gold: σ=2.0 (100% swing amplitude over 20-tick period)
fn bank_run_exact_gold(tick: u64) -> f64 {
    let t = tick as f64;
    // Base price 163 g/oz, oscillate with amplitude producing σ≈2.0
    // 100% swing = ±163 over 20 ticks
    163.0 + 163.0 * (t * std::f64::consts::PI / 10.0).sin()
}

/// Bank Run demand: constant high (10:1 demand/liquidity ratio via setup)
fn bank_run_exact_demand(_tick: u64) -> f64 {
    0.95 // Maximum demand
}

/// Bank Run panic: ramp to simulate run dynamics
fn bank_run_exact_panic(tick: u64) -> f64 {
    let t = tick as f64;
    (t / 200.0).min(0.9) // Ramp from 0 to 0.9 over 200 ticks, hold
}

// ─── Scenario Definitions ───────────────────────────────────────────────────

pub fn scenarios() -> Vec<Scenario> {
    let mut all = vec![
        // ─── Market Conditions (5) ──────────────────────────────────────
        Scenario { name: "NORMAL_MARKET", label: "Normal Market", category: "market",
            gold: 2600.0, demand: 0.3, panic: 0.0, nodes: 24, ticks: 600,
            gold_curve: None, demand_curve: None, panic_curve: None,
            criteria: PassCriteria { min_settlement_rate: Some(50.0), ..Default::default() },
            setup: None, mid_event: None },
        Scenario { name: "BULL_RUN", label: "Bull Run", category: "market",
            gold: 3200.0, demand: 0.8, panic: 0.05, nodes: 24, ticks: 200,
            gold_curve: None, demand_curve: None, panic_curve: None,
            criteria: PassCriteria { min_settlement_rate: Some(15.0), ..Default::default() },
            setup: None, mid_event: None },
        Scenario { name: "BEAR_MARKET", label: "Bear Market", category: "market",
            gold: 1800.0, demand: 0.1, panic: 0.4, nodes: 24, ticks: 200,
            gold_curve: None, demand_curve: None, panic_curve: None,
            criteria: PassCriteria::default(),
            setup: None, mid_event: None },
        Scenario { name: "BLACK_SWAN", label: "Black Swan", category: "market",
            gold: 2600.0, demand: 0.9, panic: 0.95, nodes: 24, ticks: 300,
            gold_curve: Some(black_swan_gold), demand_curve: None, panic_curve: None,
            criteria: PassCriteria { max_conservation_error: 2.0, ..Default::default() },
            setup: None, mid_event: None },
        Scenario { name: "STAGFLATION", label: "Stagflation", category: "market",
            gold: 2600.0, demand: 0.05, panic: 0.3, nodes: 24, ticks: 200,
            gold_curve: None, demand_curve: None, panic_curve: None,
            criteria: PassCriteria::default(),
            setup: None, mid_event: None },

        // ─── Stress Tests (8) ───────────────────────────────────────────
        Scenario { name: "SCALE_100", label: "Scale 100", category: "stress",
            gold: 2600.0, demand: 0.3, panic: 0.0, nodes: 100, ticks: 200,
            gold_curve: None, demand_curve: None, panic_curve: None,
            criteria: PassCriteria { max_conservation_error: 5.0, min_settlement_rate: Some(30.0), ..Default::default() },
            setup: None, mid_event: None },
        Scenario { name: "SCALE_250", label: "Scale 250", category: "stress",
            gold: 2600.0, demand: 0.3, panic: 0.0, nodes: 250, ticks: 200,
            gold_curve: None, demand_curve: None, panic_curve: None,
            criteria: PassCriteria { max_conservation_error: 10.0, min_settlement_rate: Some(20.0), ..Default::default() },
            setup: None, mid_event: None },
        Scenario { name: "SCALE_500", label: "Scale 500", category: "stress",
            gold: 2600.0, demand: 0.5, panic: 0.0, nodes: 500, ticks: 200,
            gold_curve: None, demand_curve: None, panic_curve: None,
            criteria: PassCriteria { max_conservation_error: 20.0, ..Default::default() },
            setup: None, mid_event: None },
        Scenario { name: "TIER_ISOLATION", label: "Tier Isolation", category: "stress",
            gold: 2600.0, demand: 0.5, panic: 0.0, nodes: 24, ticks: 200,
            gold_curve: None, demand_curve: None, panic_curve: None,
            criteria: PassCriteria::default(),
            setup: None, mid_event: None },
        Scenario { name: "FEE_CAP_STRESS", label: "Fee Cap Stress", category: "stress",
            gold: 2600.0, demand: 0.95, panic: 0.8, nodes: 24, ticks: 300,
            gold_curve: None, demand_curve: None, panic_curve: None,
            criteria: PassCriteria { max_conservation_error: 2.0, max_fee_cap_breaches: Some(0), ..Default::default() },
            setup: None, mid_event: None },
        Scenario { name: "GOVERNOR_STRESS", label: "Governor Stress", category: "stress",
            gold: 2600.0, demand: 0.5, panic: 0.0, nodes: 24, ticks: 200,
            gold_curve: Some(governor_stress_gold), demand_curve: Some(governor_stress_demand), panic_curve: None,
            criteria: PassCriteria { max_conservation_error: 2.0, ..Default::default() },
            setup: None, mid_event: None },
        Scenario { name: "DISSOLUTION_TEST", label: "Dissolution", category: "stress",
            gold: 2600.0, demand: 0.3, panic: 0.0, nodes: 24, ticks: 8000,
            gold_curve: None, demand_curve: None, panic_curve: None,
            criteria: PassCriteria::default(),
            setup: None, mid_event: None },
        Scenario { name: "AML_DETECTION", label: "AML Detection", category: "stress",
            gold: 2600.0, demand: 0.9, panic: 0.0, nodes: 24, ticks: 200,
            gold_curve: None, demand_curve: None, panic_curve: None,
            criteria: PassCriteria::default(),
            setup: None, mid_event: None },

        // ─── Fiduciary Tests (3) ────────────────────────────────────────
        Scenario { name: "SETTLEMENT_FINALITY", label: "Settlement Finality", category: "fiduciary",
            gold: 2600.0, demand: 0.5, panic: 0.0, nodes: 24, ticks: 200,
            gold_curve: None, demand_curve: None, panic_curve: None,
            criteria: PassCriteria { max_conservation_error: 0.01, require_settlement_finality: true, ..Default::default() },
            setup: None, mid_event: None },
        Scenario { name: "COST_CERTAINTY", label: "Cost Certainty", category: "fiduciary",
            gold: 2600.0, demand: 0.5, panic: 0.2, nodes: 24, ticks: 200,
            gold_curve: None, demand_curve: None, panic_curve: None,
            criteria: PassCriteria { max_conservation_error: 0.1, require_cost_certainty: true, ..Default::default() },
            setup: None, mid_event: None },
        Scenario { name: "AUDIT_TRAIL", label: "Audit Trail", category: "fiduciary",
            gold: 2600.0, demand: 0.3, panic: 0.0, nodes: 24, ticks: 200,
            gold_curve: None, demand_curve: None, panic_curve: None,
            criteria: PassCriteria { max_conservation_error: 0.1, require_audit_trail: true, ..Default::default() },
            setup: None, mid_event: None },

        // ─── Real-World 2025-2026 (per-gram, 4 scenarios) ──────────────
        Scenario { name: "RW_BASELINE_2026", label: "RW: Feb 2026 Baseline", category: "real-world",
            gold: 163.0, demand: 0.4, panic: 0.05, nodes: 24, ticks: 600,
            gold_curve: None, demand_curve: None, panic_curve: None,
            criteria: PassCriteria { min_settlement_rate: Some(40.0), ..Default::default() },
            setup: None, mid_event: None },
        Scenario { name: "RW_BULL_2025", label: "RW: 2025 Bull Run", category: "real-world",
            gold: 83.5, demand: 0.3, panic: 0.0, nodes: 24, ticks: 600,
            gold_curve: Some(bull_2025_gold), demand_curve: Some(bull_2025_demand), panic_curve: None,
            criteria: PassCriteria { min_settlement_rate: Some(30.0), ..Default::default() },
            setup: None, mid_event: None },
        Scenario { name: "RW_FLASH_CRASH_OCT25", label: "RW: Oct25 Flash Crash", category: "real-world",
            gold: 141.0, demand: 0.5, panic: 0.0, nodes: 24, ticks: 300,
            gold_curve: Some(flash_crash_oct25_gold), demand_curve: Some(flash_crash_oct25_demand),
            panic_curve: Some(flash_crash_oct25_panic),
            criteria: PassCriteria { max_conservation_error: 2.0, ..Default::default() },
            setup: None, mid_event: None },
        Scenario { name: "RW_FED_CORRECTION_26", label: "RW: 2026 Fed Correction", category: "real-world",
            gold: 177.0, demand: 0.6, panic: 0.1, nodes: 24, ticks: 400,
            gold_curve: Some(fed_correction_26_gold), demand_curve: Some(fed_correction_26_demand), panic_curve: None,
            criteria: PassCriteria { ..Default::default() },
            setup: None, mid_event: None },

        // ─── Whitepaper Invariant Tests (4 original) ────────────────────
        Scenario { name: "WP_NO_FAIL_BANK_RUN", label: "WP: Bank Run No-Fail", category: "whitepaper",
            gold: 163.0, demand: 0.95, panic: 0.9, nodes: 100, ticks: 2000,
            gold_curve: None, demand_curve: None, panic_curve: None,
            criteria: PassCriteria { max_conservation_error: 10.0, max_held_at_end: Some(10000), ..Default::default() },
            setup: None, mid_event: None },
        Scenario { name: "WP_PEG_ELASTICITY", label: "WP: Peg Elasticity", category: "whitepaper",
            gold: 163.0, demand: 0.5, panic: 0.0, nodes: 100, ticks: 2000,
            gold_curve: Some(peg_elasticity_gold), demand_curve: None, panic_curve: None,
            criteria: PassCriteria { max_conservation_error: 10.0, ..Default::default() },
            setup: None, mid_event: None },
        Scenario { name: "WP_INCENTIVE_DROUGHT", label: "WP: Incentive Drought", category: "whitepaper",
            gold: 163.0, demand: 0.8, panic: 0.7, nodes: 100, ticks: 2000,
            gold_curve: None, demand_curve: None, panic_curve: None,
            criteria: PassCriteria { max_conservation_error: 20.0, ..Default::default() },
            setup: None, mid_event: None },
        Scenario { name: "WP_DEMURRAGE_LOOP", label: "WP: Demurrage Loop Decay", category: "whitepaper",
            gold: 163.0, demand: 0.3, panic: 0.0, nodes: 24, ticks: 8000,
            gold_curve: None, demand_curve: None, panic_curve: None,
            criteria: PassCriteria { max_held_at_end: Some(2000), ..Default::default() },
            setup: None, mid_event: None },

        // ─── Scale Validation (4) ───────────────────────────────────────
        Scenario { name: "SCALE_100_V2", label: "Scale: 100 Nodes", category: "scale",
            gold: 163.0, demand: 0.5, panic: 0.0, nodes: 100, ticks: 2000,
            gold_curve: None, demand_curve: None, panic_curve: None,
            criteria: PassCriteria { max_conservation_error: 5.0, min_settlement_rate: Some(40.0), ..Default::default() },
            setup: None, mid_event: None },
        Scenario { name: "SCALE_1K", label: "Scale: 1K Nodes", category: "scale",
            gold: 163.0, demand: 0.5, panic: 0.0, nodes: 1000, ticks: 2000,
            gold_curve: None, demand_curve: None, panic_curve: None,
            criteria: PassCriteria { max_conservation_error: 50.0, min_settlement_rate: Some(30.0), ..Default::default() },
            setup: None, mid_event: None },
        Scenario { name: "SCALE_5K", label: "Scale: 5K Nodes", category: "scale",
            gold: 163.0, demand: 0.4, panic: 0.0, nodes: 5000, ticks: 1000,
            gold_curve: None, demand_curve: None, panic_curve: None,
            criteria: PassCriteria { max_conservation_error: 200.0, ..Default::default() },
            setup: None, mid_event: None },
        Scenario { name: "SCALE_10K", label: "Scale: 10K Nodes", category: "scale",
            gold: 163.0, demand: 0.3, panic: 0.0, nodes: 10000, ticks: 500,
            gold_curve: None, demand_curve: None, panic_curve: None,
            criteria: PassCriteria { max_conservation_error: 500.0, ..Default::default() },
            setup: None, mid_event: None },

        // ─── Real-World at Scale (2) ────────────────────────────────────
        Scenario { name: "RW_1K_BULL_2025", label: "RW: 1K Bull Run 2025", category: "real-world",
            gold: 83.5, demand: 0.3, panic: 0.0, nodes: 1000, ticks: 2000,
            gold_curve: Some(bull_2025_gold), demand_curve: Some(bull_2025_demand), panic_curve: None,
            criteria: PassCriteria { max_conservation_error: 50.0, min_settlement_rate: Some(30.0), ..Default::default() },
            setup: None, mid_event: None },
        Scenario { name: "RW_1K_SOVEREIGN", label: "RW: 1K Sovereign Crisis", category: "real-world",
            gold: 177.0, demand: 0.9, panic: 0.8, nodes: 1000, ticks: 2000,
            gold_curve: Some(black_swan_gold), demand_curve: None, panic_curve: None,
            criteria: PassCriteria { max_conservation_error: 200.0, ..Default::default() },
            setup: None, mid_event: None },

        // ─── Stress Envelope (4) ────────────────────────────────────────
        Scenario { name: "STRESS_20K", label: "Stress: 20K Nodes", category: "stress-envelope",
            gold: 163.0, demand: 0.5, panic: 0.0, nodes: 20000, ticks: 500,
            gold_curve: None, demand_curve: None, panic_curve: None,
            criteria: PassCriteria { max_conservation_error: 1000.0, ..Default::default() },
            setup: None, mid_event: None },
        Scenario { name: "STRESS_50K_TICKS", label: "Stress: 1K x 50K Ticks", category: "stress-envelope",
            gold: 163.0, demand: 0.5, panic: 0.0, nodes: 1000, ticks: 50000,
            gold_curve: Some(governor_stress_gold), demand_curve: Some(governor_stress_demand), panic_curve: None,
            criteria: PassCriteria { max_conservation_error: 500.0, ..Default::default() },
            setup: None, mid_event: None },
        Scenario { name: "STRESS_FULL_PANIC", label: "Stress: 5K Full Panic", category: "stress-envelope",
            gold: 163.0, demand: 0.95, panic: 0.95, nodes: 5000, ticks: 1000,
            gold_curve: Some(black_swan_gold), demand_curve: None, panic_curve: None,
            criteria: PassCriteria { max_conservation_error: 1000.0, ..Default::default() },
            setup: None, mid_event: None },
        Scenario { name: "STRESS_100K", label: "Stress: 100K Nodes", category: "stress-envelope",
            gold: 163.0, demand: 0.3, panic: 0.0, nodes: 100000, ticks: 100,
            gold_curve: None, demand_curve: None, panic_curve: None,
            criteria: PassCriteria { max_conservation_error: 10000.0, ..Default::default() },
            setup: None, mid_event: None },
    ];

    // ─── NEW: Whitepaper-Exact Scenarios (Gap #6, #7, demurrage) ────────

    // Gap #6: Exact Bank Run (λ=0.1, σ=2.0, 10:1 demand/liquidity)
    all.push(Scenario {
        name: "WP_BANK_RUN_EXACT",
        label: "WP: Bank Run Exact (λ=0.1, σ=2.0)",
        category: "whitepaper-exact",
        gold: 163.0, demand: 0.95, panic: 0.0, nodes: 100, ticks: 2000,
        gold_curve: Some(bank_run_exact_gold),
        demand_curve: Some(bank_run_exact_demand),
        panic_curve: Some(bank_run_exact_panic),
        criteria: PassCriteria {
            max_conservation_error: 50.0,
            max_held_at_end: Some(50000),
            ..Default::default()
        },
        // Set Egress liquidity to 1/10th normal (10:1 demand/liquidity ratio)
        setup: Some(Box::new(|sim: &mut ArenaSimulation| {
            let base_crypto = 1000.0 * (100.0_f64 / 24.0).max(1.0) * 500.0;
            for i in 0..100u32 {
                if i % 4 == 1 { // Egress nodes
                    sim.set_node_crypto(i, base_crypto * 0.1);
                }
            }
        })),
        mid_event: None,
    });

    // Gap #7: Route Healing at Scale
    all.push(Scenario {
        name: "WP_ROUTE_HEALING",
        label: "WP: Route Healing (kill 2 Transit @ t=500)",
        category: "whitepaper-exact",
        gold: 163.0, demand: 0.5, panic: 0.0, nodes: 100, ticks: 2000,
        gold_curve: None, demand_curve: None, panic_curve: None,
        criteria: PassCriteria {
            max_conservation_error: 10.0,
            ..Default::default()
        },
        setup: None,
        // Kill 2 Transit nodes at tick 500
        mid_event: Some(Box::new(|sim: &mut ArenaSimulation, tick: u64| {
            if tick == 500 {
                // Kill Transit nodes (id % 4 == 2)
                sim.kill_node(2);
                sim.kill_node(6);
            }
        })),
    });

    // Demurrage Decay exact validation
    all.push(Scenario {
        name: "WP_DEMURRAGE_EXACT",
        label: "WP: Demurrage Decay (8K ticks, low demand)",
        category: "whitepaper-exact",
        gold: 163.0, demand: 0.1, panic: 0.0, nodes: 24, ticks: 8000,
        gold_curve: None, demand_curve: None, panic_curve: None,
        criteria: PassCriteria {
            max_held_at_end: Some(500),
            ..Default::default()
        },
        setup: None, mid_event: None,
    });

    all
}
