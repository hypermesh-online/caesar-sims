// Copyright 2026 Hypermesh Foundation. All rights reserved.
// Caesar Protocol Simulation Suite ("The Arena") - ENGauge Logic

use crate::types::{NodeRole, SimNode};
use serde::{Deserialize, Serialize};

const MAX_WINDOW_SIZE: usize = 20;
const SPECULATIVE_THRESHOLD: f64 = 0.3;
const VELOCITY_FLOOR: f64 = 100.0;
const VELOCITY_DIVISOR: f64 = 1000.0;
const RATIO_FLOOR: f64 = 0.1;

// ---------------------------------------------------------------------------
// Free functions (called from simulation.rs - signatures preserved)
// ---------------------------------------------------------------------------

/// Update accumulated work for NGauge nodes based on demand factor.
/// Returns the ngauge_activity_index (clamped to [0.0, 1.0]).
pub fn update_ngauge_activity(nodes: &mut [SimNode], demand_factor: f64) -> f64 {
    let mut total_work = 0.0;
    for node in nodes.iter_mut() {
        if node.role == NodeRole::NGauge {
            node.accumulated_work += (demand_factor * 10.0).max(1.0);
            total_work += node.accumulated_work;
        }
    }
    (total_work / (nodes.len() as f64 * 100.0)).min(1.0)
}

/// Compute the organic ratio from ngauge activity and network velocity.
/// High velocity with low real work indicates speculative activity.
pub fn compute_organic_ratio(ngauge_activity_index: f64, network_velocity: f64) -> f64 {
    if network_velocity > VELOCITY_FLOOR {
        ngauge_activity_index / (network_velocity / VELOCITY_DIVISOR).max(RATIO_FLOOR)
    } else {
        1.0
    }
}

// ---------------------------------------------------------------------------
// NGaugeState - rolling window tracker for organic/speculative detection
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NGaugeState {
    activity_window: Vec<f64>,
    velocity_window: Vec<f64>,
    organic_ratio: f64,
    speculative_detected: bool,
}

impl Default for NGaugeState {
    fn default() -> Self {
        Self::new()
    }
}

impl NGaugeState {
    pub fn new() -> Self {
        Self {
            activity_window: Vec::with_capacity(MAX_WINDOW_SIZE),
            velocity_window: Vec::with_capacity(MAX_WINDOW_SIZE),
            organic_ratio: 1.0,
            speculative_detected: false,
        }
    }

    /// Push new activity and velocity samples, recompute classification.
    pub fn update(&mut self, activity: f64, velocity: f64) {
        push_and_trim(&mut self.activity_window, activity);
        push_and_trim(&mut self.velocity_window, velocity);
        self.classify();
    }

    /// Current organic ratio (1.0 = fully organic, <0.3 = speculative).
    pub fn organic_ratio(&self) -> f64 {
        self.organic_ratio
    }

    /// True when the rolling window shows organic demand patterns,
    /// meaning fees can safely be relaxed.
    pub fn should_relax_fees(&self) -> bool {
        !self.speculative_detected && self.organic_ratio > SPECULATIVE_THRESHOLD
    }

    /// True when the rolling window shows speculative activity patterns,
    /// meaning fees should be increased to dampen speculation.
    pub fn should_increase_fees(&self) -> bool {
        self.speculative_detected
    }

    // -----------------------------------------------------------------------
    // Internal classification
    // -----------------------------------------------------------------------

    fn classify(&mut self) {
        let avg_activity = window_mean(&self.activity_window);
        let avg_velocity = window_mean(&self.velocity_window);

        if avg_velocity > VELOCITY_FLOOR {
            let denominator = (avg_velocity / VELOCITY_DIVISOR).max(RATIO_FLOOR);
            self.organic_ratio = avg_activity / denominator;
            self.speculative_detected = self.organic_ratio < SPECULATIVE_THRESHOLD;
        } else {
            self.organic_ratio = 1.0;
            self.speculative_detected = false;
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn push_and_trim(window: &mut Vec<f64>, value: f64) {
    window.push(value);
    if window.len() > MAX_WINDOW_SIZE {
        window.remove(0);
    }
}

fn window_mean(window: &[f64]) -> f64 {
    if window.is_empty() {
        return 0.0;
    }
    window.iter().sum::<f64>() / window.len() as f64
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_state_defaults() {
        let state = NGaugeState::new();
        assert_eq!(state.organic_ratio(), 1.0);
        assert!(!state.should_increase_fees());
        assert!(state.should_relax_fees());
    }

    #[test]
    fn test_default_trait() {
        let state = NGaugeState::default();
        assert_eq!(state.organic_ratio(), 1.0);
    }

    #[test]
    fn test_low_velocity_always_organic() {
        let mut state = NGaugeState::new();
        state.update(0.01, 50.0);
        assert_eq!(state.organic_ratio(), 1.0);
        assert!(!state.speculative_detected);
        assert!(state.should_relax_fees());
        assert!(!state.should_increase_fees());
    }

    #[test]
    fn test_high_velocity_low_activity_is_speculative() {
        let mut state = NGaugeState::new();
        // Push enough samples to establish a window
        for _ in 0..5 {
            state.update(0.01, 500.0);
        }
        // avg_activity = 0.01, avg_velocity = 500.0
        // denominator = (500/1000).max(0.1) = 0.5
        // organic_ratio = 0.01 / 0.5 = 0.02 < 0.3
        assert!(state.organic_ratio() < SPECULATIVE_THRESHOLD);
        assert!(state.speculative_detected);
        assert!(state.should_increase_fees());
        assert!(!state.should_relax_fees());
    }

    #[test]
    fn test_high_velocity_high_activity_is_organic() {
        let mut state = NGaugeState::new();
        for _ in 0..5 {
            state.update(0.8, 200.0);
        }
        // avg_activity = 0.8, avg_velocity = 200.0
        // denominator = (200/1000).max(0.1) = 0.2
        // organic_ratio = 0.8 / 0.2 = 4.0 > 0.3
        assert!(state.organic_ratio() > SPECULATIVE_THRESHOLD);
        assert!(!state.speculative_detected);
        assert!(state.should_relax_fees());
    }

    #[test]
    fn test_rolling_window_trims_to_max() {
        let mut state = NGaugeState::new();
        for i in 0..30 {
            state.update(i as f64, i as f64);
        }
        assert_eq!(state.activity_window.len(), MAX_WINDOW_SIZE);
        assert_eq!(state.velocity_window.len(), MAX_WINDOW_SIZE);
        // Oldest entries (0..10) should have been evicted
        assert_eq!(state.activity_window[0], 10.0);
        assert_eq!(state.velocity_window[0], 10.0);
    }

    #[test]
    fn test_transition_from_speculative_to_organic() {
        let mut state = NGaugeState::new();
        // Start speculative
        for _ in 0..20 {
            state.update(0.01, 500.0);
        }
        assert!(state.should_increase_fees());

        // Gradually introduce organic activity to fill the window
        for _ in 0..20 {
            state.update(1.0, 200.0);
        }
        assert!(state.should_relax_fees());
        assert!(!state.should_increase_fees());
    }

    #[test]
    fn test_compute_organic_ratio_free_fn_high_velocity() {
        let ratio = compute_organic_ratio(0.5, 500.0);
        // 0.5 / (500/1000).max(0.1) = 0.5 / 0.5 = 1.0
        assert!((ratio - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_compute_organic_ratio_free_fn_low_velocity() {
        let ratio = compute_organic_ratio(0.01, 50.0);
        assert!((ratio - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_window_mean_empty() {
        assert_eq!(window_mean(&[]), 0.0);
    }

    #[test]
    fn test_window_mean_values() {
        let window = vec![2.0, 4.0, 6.0];
        assert!((window_mean(&window) - 4.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_serde_roundtrip() {
        let mut state = NGaugeState::new();
        state.update(0.5, 150.0);
        let json = serde_json::to_string(&state).unwrap();
        let deserialized: NGaugeState = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.organic_ratio(), state.organic_ratio());
        assert_eq!(deserialized.activity_window.len(), state.activity_window.len());
    }
}
