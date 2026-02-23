// Per-Tick JSONL Time Series Recorder
// Outputs one JSON line per tick for independent analysis

use serde::Serialize;
use arena_engine::WorldState;
use std::io::Write;

#[derive(Debug, Serialize)]
pub struct TickSnapshot {
    pub tick: u64,
    pub gold_price: f64,
    pub demand_factor: f64,
    pub panic_level: f64,
    pub conservation_error: f64,
    pub normalized_conservation_error: f64,
    pub total_input: f64,
    pub total_output: f64,
    pub active_value: f64,
    pub current_fee_rate: f64,
    pub tier_fee_rates: [f64; 4],
    pub settlement_count: u32,
    pub held_count: u32,
    pub orbit_count: u32,
    pub egress_profit_cumulative: f64,
    pub transit_profit_cumulative: f64,
    pub demurrage_burned_cumulative: f64,
    pub effective_exchange_rate: f64,
    pub peg_within_band: bool,
    pub surge_multiplier: f64,
    pub volatility: f64,
    pub dissolved_count: u32,
}

impl TickSnapshot {
    pub fn from_state(state: &WorldState) -> Self {
        let effective_exchange_rate = state.gold_price * (1.0 - state.current_fee_rate);
        let normalized_conservation_error = if state.total_input > 0.0 {
            state.total_value_leaked.abs() / state.total_input
        } else {
            0.0
        };

        Self {
            tick: state.current_tick,
            gold_price: state.gold_price,
            demand_factor: state.demand_factor,
            panic_level: state.panic_level,
            conservation_error: state.total_value_leaked,
            normalized_conservation_error,
            total_input: state.total_input,
            total_output: state.total_output,
            active_value: state.active_value,
            current_fee_rate: state.current_fee_rate,
            tier_fee_rates: state.tier_fee_rates,
            settlement_count: state.settlement_count,
            held_count: state.held_count,
            orbit_count: state.orbit_count,
            egress_profit_cumulative: state.total_rewards_egress,
            transit_profit_cumulative: state.total_rewards_transit,
            demurrage_burned_cumulative: state.total_demurrage_burned,
            effective_exchange_rate,
            peg_within_band: state.current_fee_rate <= 0.20,
            surge_multiplier: state.surge_multiplier,
            volatility: state.volatility,
            dissolved_count: state.dissolved_count,
        }
    }
}

/// Time series recorder that accumulates snapshots and writes JSONL
pub struct TimeSeriesRecorder {
    snapshots: Vec<TickSnapshot>,
}

impl TimeSeriesRecorder {
    pub fn new() -> Self {
        Self { snapshots: Vec::new() }
    }

    pub fn record(&mut self, state: &WorldState) {
        self.snapshots.push(TickSnapshot::from_state(state));
    }

    /// Write all snapshots to a JSONL file
    pub fn write_jsonl(&self, path: &std::path::Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = std::fs::File::create(path)?;
        for snapshot in &self.snapshots {
            let line = serde_json::to_string(snapshot)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
            writeln!(file, "{}", line)?;
        }
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.snapshots.len()
    }
}
