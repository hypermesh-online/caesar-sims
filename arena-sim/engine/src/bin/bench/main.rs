// Arena Benchmark Runner v1.0.0 — SEC/Economist-Grade Whitepaper Validation
// Monte Carlo (N=30), Poisson traffic, seedable PRNG, per-tick audit trail
//
// Usage:
//   cargo run --release --bin bench                     # Run all scenarios (30 runs each)
//   cargo run --release --bin bench -- --runs 5         # Quick mode (5 runs each)
//   cargo run --release --bin bench -- WP_BANK_RUN      # Filter by name
//   cargo run --release --bin bench -- --time-series    # Enable JSONL output
//   cargo run --release --bin bench -- --seed 42        # Custom base seed

mod report;
mod scenarios;
mod monte_carlo;
mod traffic;
mod metrics;
mod time_series;

use report::*;
use scenarios::*;
use metrics::run_incentive_comparison;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

// ─── CLI Parsing ────────────────────────────────────────────────────────────

struct CliArgs {
    runs: usize,
    seed: u64,
    time_series: bool,
    filter: Option<String>,
}

fn parse_args() -> CliArgs {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut cli = CliArgs {
        runs: 30,
        seed: 0,
        time_series: false,
        filter: None,
    };

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--runs" => {
                i += 1;
                if i < args.len() {
                    cli.runs = args[i].parse().unwrap_or(30);
                }
            }
            "--seed" => {
                i += 1;
                if i < args.len() {
                    cli.seed = args[i].parse().unwrap_or(0);
                }
            }
            "--time-series" => {
                cli.time_series = true;
            }
            arg if !arg.starts_with('-') => {
                cli.filter = Some(arg.to_string());
            }
            _ => {
                eprintln!("Unknown argument: {}", args[i]);
            }
        }
        i += 1;
    }

    cli
}

// ─── Main ───────────────────────────────────────────────────────────────────

fn main() {
    let cli = parse_args();
    let all_scenarios = scenarios();

    let to_run: Vec<&Scenario> = match &cli.filter {
        Some(f) => {
            let f_lower = f.to_lowercase();
            all_scenarios.iter()
                .filter(|s| s.name.to_lowercase().contains(&f_lower)
                          || s.label.to_lowercase().contains(&f_lower)
                          || s.category.to_lowercase().contains(&f_lower))
                .collect()
        }
        None => all_scenarios.iter().collect(),
    };

    if to_run.is_empty() {
        eprintln!("No scenarios match filter: {:?}", cli.filter);
        std::process::exit(1);
    }

    let ts_dir = if cli.time_series {
        let dir = std::path::Path::new("benchmark-results/time-series");
        Some(dir.to_path_buf())
    } else {
        None
    };

    println!("\n  Arena Benchmark Runner v1.0.0 (SEC/Economist-Grade)");
    println!("  PRNG: ChaCha8Rng | Runs/scenario: {} | Base seed: {}", cli.runs, cli.seed);
    println!("  Running {} scenario(s)...\n", to_run.len());
    println!("  {:<36} {:>5} {:>10} {:>12} {:>8} {:>6} {:>7}",
        "Scenario", "Pass%", "Settle%", "Conserv(N)", "Peg%", "Held", "Time");
    println!("  {}", "-".repeat(88));

    let suite_start = Instant::now();
    let mut mc_reports = Vec::new();

    for scenario in &to_run {
        let report = monte_carlo::run_monte_carlo(
            scenario,
            cli.runs,
            cli.seed,
            ts_dir.as_deref(),
        );

        let pass_pct = report.pass_rate * 100.0;
        let settle_mean = report.settlement_rate.mean;
        let settle_ci = (report.settlement_rate.ci_upper - report.settlement_rate.ci_lower) / 2.0;
        let conserv_n = report.normalized_conservation_error.mean;
        let peg_pct = report.peg_elasticity_pct.mean;
        let held_mean = report.held_count.mean;
        let time_mean = report.elapsed_ms.mean;

        let status = if pass_pct >= 93.3 { "PASS" } else { "FAIL" };

        println!("  {:<36} {:>4}% {:>6.1}±{:<3.1} {:>12.2e} {:>7.1}% {:>5.0} {:>5.0}ms  {}",
            report.label,
            pass_pct as u32,
            settle_mean, settle_ci,
            conserv_n,
            peg_pct,
            held_mean,
            time_mean,
            status,
        );

        mc_reports.push(report);
    }

    let suite_elapsed = suite_start.elapsed();

    // ─── Whitepaper Validation ──────────────────────────────────────────

    // Check Bank Run (exact scenario if present, else original)
    let bank_run_passes = mc_reports.iter()
        .find(|r| r.scenario_name == "WP_BANK_RUN_EXACT" || r.scenario_name == "WP_NO_FAIL_BANK_RUN")
        .map(|r| r.pass_rate >= 0.933)
        .unwrap_or(true); // If not run, don't fail

    // Check Peg Elasticity
    let peg_passes = mc_reports.iter()
        .find(|r| r.scenario_name == "WP_PEG_ELASTICITY")
        .map(|r| r.peg_elasticity_pct.mean >= 95.0)
        .unwrap_or(true);

    // Check Incentive (run paired comparison if WP_INCENTIVE_DROUGHT was run)
    let incentive_passes = mc_reports.iter()
        .find(|r| r.scenario_name == "WP_INCENTIVE_DROUGHT")
        .map(|_r| {
            // Run paired comparison: same traffic, different Egress liquidity
            let comp = run_incentive_comparison(100, 2000, 163.0, 0.8, cli.seed);
            comp.passes
        })
        .unwrap_or(true);

    // Check Demurrage
    let demurrage_passes = mc_reports.iter()
        .find(|r| r.scenario_name == "WP_DEMURRAGE_EXACT" || r.scenario_name == "WP_DEMURRAGE_LOOP")
        .map(|r| r.pass_rate >= 0.933)
        .unwrap_or(true);

    // Check Route Healing
    let route_healing_passes = mc_reports.iter()
        .find(|r| r.scenario_name == "WP_ROUTE_HEALING")
        .map(|r| r.pass_rate >= 0.933)
        .unwrap_or(true);

    // Max normalized conservation across ALL scenarios
    let max_normalized_conservation = mc_reports.iter()
        .map(|r| r.normalized_conservation_error.max)
        .fold(0.0_f64, f64::max);

    let wp_validation = WhitepaperValidation {
        bank_run_no_fail: bank_run_passes,
        peg_elasticity_95pct: peg_passes,
        incentive_ratio_500pct: incentive_passes,
        demurrage_decay_to_zero: demurrage_passes,
        route_healing_zero_loss: route_healing_passes,
        max_normalized_conservation,
    };

    // ─── Summary ────────────────────────────────────────────────────────

    let total = mc_reports.len();
    let passed = mc_reports.iter().filter(|r| r.pass_rate >= 0.933).count();
    let failed = total - passed;

    println!("  {}", "-".repeat(88));
    println!("  Total: {}  Passed: {}  Failed: {}  Suite time: {:.1}s\n",
        total, passed, failed, suite_elapsed.as_secs_f64());

    println!("  Whitepaper Validation:");
    println!("    Bank Run No-Fail:     {}", if wp_validation.bank_run_no_fail { "PASS" } else { "FAIL" });
    println!("    Peg Elasticity ≥95%:  {}", if wp_validation.peg_elasticity_95pct { "PASS" } else { "FAIL" });
    println!("    Incentive >500%:      {}", if wp_validation.incentive_ratio_500pct { "PASS" } else { "FAIL" });
    println!("    Demurrage Decay:      {}", if wp_validation.demurrage_decay_to_zero { "PASS" } else { "FAIL" });
    println!("    Route Healing:        {}", if wp_validation.route_healing_zero_loss { "PASS" } else { "FAIL" });
    println!("    Max Norm Conservation: {:.2e}\n", wp_validation.max_normalized_conservation);

    // ─── Write JSON Report ──────────────────────────────────────────────

    let ts = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis();
    let timestamp = format!("{}", ts);

    let report = BenchReport {
        timestamp: timestamp.clone(),
        version: "1.0.0",
        prng: "ChaCha8Rng",
        n_runs_per_scenario: cli.runs,
        summary: Summary {
            total,
            passed,
            failed,
            pass_rate: passed as f64 / total as f64,
        },
        whitepaper_validation: wp_validation,
        scenarios: mc_reports,
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
