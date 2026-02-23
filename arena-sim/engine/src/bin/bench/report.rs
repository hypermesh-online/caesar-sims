// SEC/Economist-Grade Benchmark Report Types
// Structured output for independent analysis and whitepaper validation

use serde::Serialize;

// ─── Statistics (per-metric Monte Carlo aggregation) ────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct Stats {
    pub mean: f64,
    pub std_dev: f64,
    pub ci_lower: f64,
    pub ci_upper: f64,
    pub min: f64,
    pub max: f64,
    pub n: usize,
}

impl Stats {
    pub fn from_samples(samples: &[f64]) -> Self {
        let n = samples.len();
        if n == 0 {
            return Self { mean: 0.0, std_dev: 0.0, ci_lower: 0.0, ci_upper: 0.0, min: 0.0, max: 0.0, n: 0 };
        }
        let mean = samples.iter().sum::<f64>() / n as f64;
        let variance = if n > 1 {
            samples.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1) as f64
        } else {
            0.0
        };
        let std_dev = variance.sqrt();
        let stderr = std_dev / (n as f64).sqrt();
        let z = 1.96; // 95% CI
        Self {
            mean,
            std_dev,
            ci_lower: mean - z * stderr,
            ci_upper: mean + z * stderr,
            min: samples.iter().cloned().fold(f64::INFINITY, f64::min),
            max: samples.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
            n,
        }
    }
}

// ─── Single-Run Result ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct BenchResult {
    pub scenario: String,
    pub name: String,
    pub category: String,
    pub seed: u64,
    pub pass: bool,
    pub settlement_count: u32,
    pub revert_count: u32,
    pub spawn_count: u32,
    pub settlement_rate: f64,
    pub conservation_error: f64,
    pub normalized_conservation_error: f64,
    pub avg_fee: f64,
    pub peak_fee: f64,
    pub dissolved_count: u32,
    pub held_count: u32,
    pub fee_cap_breaches: u32,
    pub settlement_finality: bool,
    pub cost_certainty: bool,
    pub audit_trail: bool,
    pub tier_breakdown: [u32; 4],
    pub ticks: u64,
    pub elapsed_ms: u128,
    pub packets_per_tick: f64,
    pub demand_scale_factor: f64,
    pub egress_profit_total: f64,
    pub transit_profit_total: f64,
    pub demurrage_total: f64,
    pub conservation_holds: bool,
    pub final_held_count: u32,
    pub final_orbit_count: u32,
    pub throughput_per_sec: f64,
    pub peg_elasticity_pct: f64,
    pub max_normalized_conservation: f64,
}

// ─── Monte Carlo Report (per-scenario aggregation) ──────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct MonteCarloReport {
    pub scenario_name: String,
    pub label: String,
    pub category: String,
    pub n_runs: usize,
    pub pass_rate: f64,
    pub conservation_error: Stats,
    pub normalized_conservation_error: Stats,
    pub settlement_rate: Stats,
    pub peg_elasticity_pct: Stats,
    pub egress_profit: Stats,
    pub transit_profit: Stats,
    pub demurrage_total: Stats,
    pub held_count: Stats,
    pub elapsed_ms: Stats,
    pub throughput_per_sec: Stats,
    pub packets_per_tick: Stats,
    pub individual_runs: Vec<BenchResult>,
}

// ─── Whitepaper Validation Summary ──────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct WhitepaperValidation {
    pub bank_run_no_fail: bool,
    pub peg_elasticity_95pct: bool,
    pub incentive_ratio_500pct: bool,
    pub demurrage_decay_to_zero: bool,
    pub route_healing_zero_loss: bool,
    pub max_normalized_conservation: f64,
}

impl WhitepaperValidation {
    pub fn all_pass(&self) -> bool {
        self.bank_run_no_fail
            && self.peg_elasticity_95pct
            && self.incentive_ratio_500pct
            && self.demurrage_decay_to_zero
            && self.route_healing_zero_loss
            && self.max_normalized_conservation < 1e-10
    }
}

// ─── Top-Level Report ───────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct BenchReport {
    pub timestamp: String,
    pub version: &'static str,
    pub prng: &'static str,
    pub n_runs_per_scenario: usize,
    pub summary: Summary,
    pub whitepaper_validation: WhitepaperValidation,
    pub scenarios: Vec<MonteCarloReport>,
}

#[derive(Debug, Serialize)]
pub struct Summary {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub pass_rate: f64,
}
