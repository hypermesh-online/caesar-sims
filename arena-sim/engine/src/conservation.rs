// Copyright 2026 Hypermesh Foundation. All rights reserved.
// Caesar Protocol Simulation Suite ("The Arena") - Conservation Logic

use serde::{Deserialize, Serialize};

/// Settlement tolerance: absolute error below this threshold is considered balanced.
const TOLERANCE: f64 = 0.0001;

// ---------------------------------------------------------------------------
// Original free function (called from simulation.rs)
// ---------------------------------------------------------------------------

/// Compute the conservation error (value leaked).
///
/// In a perfectly closed system:
///   total_input = total_output + total_burned + total_fees + active_value
///
/// Returns the absolute difference (leakage). Values near zero indicate
/// the thermodynamic accounting is sound.
pub fn compute_conservation(
    total_input: f64,
    total_output: f64,
    total_burned: f64,
    total_fees: f64,
    active_value: f64,
) -> f64 {
    let actual = total_output + total_burned + total_fees + active_value;
    (total_input - actual).abs()
}

// ---------------------------------------------------------------------------
// Conservation result
// ---------------------------------------------------------------------------

/// Outcome of a single conservation check (settlement or tick).
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ConservationResult {
    /// Whether the check passed within tolerance.
    pub balanced: bool,
    /// Absolute error for this check.
    pub error: f64,
    /// Whether the circuit breaker is currently tripped.
    pub circuit_breaker_tripped: bool,
}

// ---------------------------------------------------------------------------
// Conservation law (circuit breaker + settlement verification)
// ---------------------------------------------------------------------------

/// Tracks cumulative conservation error and trips a circuit breaker when
/// the error exceeds a configurable threshold.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ConservationLaw {
    /// Running total of absolute errors across all checks that violated tolerance.
    pub cumulative_error: f64,
    /// Maximum cumulative error before the circuit breaker trips.
    pub circuit_breaker_threshold: f64,
    /// Whether the circuit breaker is currently tripped.
    pub circuit_breaker_tripped: bool,
    /// Number of consecutive checks that violated tolerance.
    pub consecutive_violations: u32,
}

impl ConservationLaw {
    /// Create a new `ConservationLaw` with a custom circuit-breaker threshold.
    pub fn new(threshold: f64) -> Self {
        Self {
            cumulative_error: 0.0,
            circuit_breaker_threshold: threshold,
            circuit_breaker_tripped: false,
            consecutive_violations: 0,
        }
    }

    /// Verify conservation at settlement time.
    ///
    /// Invariant: `initial == settled + fees + demurrage`
    pub fn verify_settlement(
        &mut self,
        initial: f64,
        settled: f64,
        fees: f64,
        demurrage: f64,
    ) -> ConservationResult {
        let error = (initial - (settled + fees + demurrage)).abs();
        let balanced = error < TOLERANCE;

        if balanced {
            self.consecutive_violations = 0;
        } else {
            self.cumulative_error += error;
            self.consecutive_violations += 1;
        }

        if self.cumulative_error > self.circuit_breaker_threshold {
            self.circuit_breaker_tripped = true;
        }

        ConservationResult {
            balanced,
            error,
            circuit_breaker_tripped: self.circuit_breaker_tripped,
        }
    }

    /// Verify conservation at tick level.
    ///
    /// Invariant: `total_input == total_output + total_fees + total_burned + active_in_flight`
    pub fn verify_tick(
        &mut self,
        total_input: f64,
        total_output: f64,
        total_fees: f64,
        total_burned: f64,
        active_in_flight: f64,
    ) -> ConservationResult {
        let expected = total_output + total_fees + total_burned + active_in_flight;
        let error = (total_input - expected).abs();
        let balanced = error < TOLERANCE;

        if balanced {
            self.consecutive_violations = 0;
        } else {
            self.cumulative_error += error;
            self.consecutive_violations += 1;
        }

        if self.cumulative_error > self.circuit_breaker_threshold {
            self.circuit_breaker_tripped = true;
        }

        ConservationResult {
            balanced,
            error,
            circuit_breaker_tripped: self.circuit_breaker_tripped,
        }
    }

    /// Reset the circuit breaker and all accumulated error state.
    pub fn reset_circuit_breaker(&mut self) {
        self.cumulative_error = 0.0;
        self.circuit_breaker_tripped = false;
        self.consecutive_violations = 0;
    }

    /// Returns `true` if the circuit breaker is currently tripped.
    pub fn is_tripped(&self) -> bool {
        self.circuit_breaker_tripped
    }
}

impl Default for ConservationLaw {
    fn default() -> Self {
        Self::new(0.001)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_conservation_exact() {
        let err = compute_conservation(100.0, 50.0, 10.0, 5.0, 35.0);
        assert!(err < f64::EPSILON, "expected zero error for balanced values");
    }

    #[test]
    fn test_compute_conservation_leakage() {
        let err = compute_conservation(100.0, 50.0, 10.0, 5.0, 30.0);
        assert!((err - 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_default_threshold() {
        let law = ConservationLaw::default();
        assert!((law.circuit_breaker_threshold - 0.001).abs() < f64::EPSILON);
        assert!(!law.circuit_breaker_tripped);
        assert_eq!(law.consecutive_violations, 0);
        assert!(law.cumulative_error.abs() < f64::EPSILON);
    }

    #[test]
    fn test_settlement_balanced() {
        let mut law = ConservationLaw::default();
        let result = law.verify_settlement(100.0, 95.0, 3.0, 2.0);
        assert!(result.balanced);
        assert!(result.error < TOLERANCE);
        assert!(!result.circuit_breaker_tripped);
        assert_eq!(law.consecutive_violations, 0);
    }

    #[test]
    fn test_settlement_violation() {
        let mut law = ConservationLaw::default();
        let result = law.verify_settlement(100.0, 90.0, 3.0, 2.0);
        assert!(!result.balanced);
        assert!((result.error - 5.0).abs() < f64::EPSILON);
        assert_eq!(law.consecutive_violations, 1);
        // 5.0 > 0.01 threshold, should trip
        assert!(result.circuit_breaker_tripped);
    }

    #[test]
    fn test_circuit_breaker_trips_on_cumulative() {
        let mut law = ConservationLaw::new(0.1);
        // Small violations that individually don't trip
        law.verify_settlement(100.0, 99.9, 0.0, 0.05);
        assert!(!law.is_tripped());
        law.verify_settlement(100.0, 99.9, 0.0, 0.05);
        assert!(!law.is_tripped());
        // cumulative is now 0.1, which isn't > 0.1 yet
        // one more pushes past
        law.verify_settlement(100.0, 99.9, 0.0, 0.05);
        assert!(law.is_tripped());
    }

    #[test]
    fn test_balanced_resets_consecutive() {
        let mut law = ConservationLaw::new(100.0);
        law.verify_settlement(100.0, 90.0, 3.0, 2.0);
        assert_eq!(law.consecutive_violations, 1);
        law.verify_settlement(100.0, 95.0, 3.0, 2.0);
        assert_eq!(law.consecutive_violations, 0);
    }

    #[test]
    fn test_reset_circuit_breaker() {
        let mut law = ConservationLaw::default();
        law.verify_settlement(100.0, 80.0, 3.0, 2.0);
        assert!(law.is_tripped());
        law.reset_circuit_breaker();
        assert!(!law.is_tripped());
        assert!(law.cumulative_error.abs() < f64::EPSILON);
        assert_eq!(law.consecutive_violations, 0);
    }

    #[test]
    fn test_verify_tick_balanced() {
        let mut law = ConservationLaw::default();
        let result = law.verify_tick(1000.0, 500.0, 100.0, 50.0, 350.0);
        assert!(result.balanced);
        assert!(!result.circuit_breaker_tripped);
    }

    #[test]
    fn test_verify_tick_violation() {
        let mut law = ConservationLaw::default();
        let result = law.verify_tick(1000.0, 500.0, 100.0, 50.0, 300.0);
        assert!(!result.balanced);
        assert!((result.error - 50.0).abs() < f64::EPSILON);
        assert!(result.circuit_breaker_tripped);
    }
}
