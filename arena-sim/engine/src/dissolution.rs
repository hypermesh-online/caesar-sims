// Copyright 2026 Hypermesh Foundation. All rights reserved.
// Caesar Protocol Simulation Suite ("The Arena") - Gravity Dissolution
//
// Implements gravity dissolution logic matching caesar/src/settlement/gravity.rs.
// Residual value from expired or dissolved packets is distributed proportionally
// to qualified nodes, with shard holders receiving double weight.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// 90 days in seconds (90 * 24 * 60 * 60).
pub const DISSOLUTION_TIMEOUT_SECS: u64 = 7_776_000;

/// Simulation-scale equivalent of the 90-day dissolution timeout.
pub const DISSOLUTION_TIMEOUT_TICKS: u64 = 5000;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DissolutionError {
    /// No nodes passed all six qualification criteria.
    NoQualifiedNodes,
    /// Residual value was zero or negative.
    ZeroResidualValue,
    /// The entity is not yet eligible for dissolution.
    NotEligible,
}

// ---------------------------------------------------------------------------
// Qualification
// ---------------------------------------------------------------------------

/// All six criteria must be `true` for a node to participate in dissolution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GravityQualification {
    pub node_id: u32,
    /// Node participates in the UPI payment network.
    pub upi_active: bool,
    /// Node contributes to network metrics via engauge.
    pub engauge_active: bool,
    /// KYC attestation is valid and current.
    pub kyc_attested: bool,
    /// Node is an active Caesar participant.
    pub caesar_active: bool,
    /// Meets minimum PoSpace + bandwidth + compute requirements.
    pub demonstrable_capacity: bool,
    /// Routed traffic during the current epoch.
    pub active_routing_current_epoch: bool,
}

impl GravityQualification {
    /// Returns `true` only when every qualification criterion is met.
    pub fn is_qualified(&self) -> bool {
        self.upi_active
            && self.engauge_active
            && self.kyc_attested
            && self.caesar_active
            && self.demonstrable_capacity
            && self.active_routing_current_epoch
    }
}

// ---------------------------------------------------------------------------
// Distribution results
// ---------------------------------------------------------------------------

/// A single node's share of the dissolved residual value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GravityDistribution {
    pub node_id: u32,
    pub amount: f64,
    pub held_shards: bool,
}

/// Aggregate result of a dissolution operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DissolutionResult {
    pub total_dissolved: f64,
    pub distributions: Vec<GravityDistribution>,
}

// ---------------------------------------------------------------------------
// Core dissolution logic
// ---------------------------------------------------------------------------

/// Distribute `residual_value` among `qualified_nodes` proportionally.
///
/// Shard holders (nodes whose `node_id` appears in `shard_holder_ids`) receive
/// a weight of 2.0; all other qualified nodes receive 1.0.
///
/// # Errors
/// - `NoQualifiedNodes` if no node passes all six criteria.
/// - `ZeroResidualValue` if `residual_value <= 0.0`.
pub fn dissolve(
    residual_value: f64,
    qualified_nodes: &[GravityQualification],
    shard_holder_ids: &[u32],
) -> Result<DissolutionResult, DissolutionError> {
    let eligible: Vec<&GravityQualification> = qualified_nodes
        .iter()
        .filter(|q| q.is_qualified())
        .collect();

    if eligible.is_empty() {
        return Err(DissolutionError::NoQualifiedNodes);
    }

    if residual_value <= 0.0 {
        return Err(DissolutionError::ZeroResidualValue);
    }

    let weights: Vec<(u32, f64, bool)> = eligible
        .iter()
        .map(|q| {
            let held = shard_holder_ids.contains(&q.node_id);
            let weight = if held { 2.0 } else { 1.0 };
            (q.node_id, weight, held)
        })
        .collect();

    let total_weight: f64 = weights.iter().map(|(_, w, _)| w).sum();

    let distributions: Vec<GravityDistribution> = weights
        .iter()
        .map(|&(node_id, weight, held)| GravityDistribution {
            node_id,
            amount: residual_value * (weight / total_weight),
            held_shards: held,
        })
        .collect();

    Ok(DissolutionResult {
        total_dissolved: residual_value,
        distributions,
    })
}

// ---------------------------------------------------------------------------
// Eligibility checks
// ---------------------------------------------------------------------------

/// Returns `true` when at least 90 real-time days have elapsed.
pub fn is_eligible_secs(elapsed_secs: u64) -> bool {
    elapsed_secs >= DISSOLUTION_TIMEOUT_SECS
}

/// Returns `true` when at least `DISSOLUTION_TIMEOUT_TICKS` simulation ticks
/// have elapsed.
pub fn is_eligible_ticks(elapsed_ticks: u64) -> bool {
    elapsed_ticks >= DISSOLUTION_TIMEOUT_TICKS
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: build a fully-qualified node.
    fn qualified(node_id: u32) -> GravityQualification {
        GravityQualification {
            node_id,
            upi_active: true,
            engauge_active: true,
            kyc_attested: true,
            caesar_active: true,
            demonstrable_capacity: true,
            active_routing_current_epoch: true,
        }
    }

    /// Helper: build an unqualified node (one criterion false).
    fn unqualified(node_id: u32) -> GravityQualification {
        GravityQualification {
            node_id,
            upi_active: true,
            engauge_active: true,
            kyc_attested: false, // fails KYC
            caesar_active: true,
            demonstrable_capacity: true,
            active_routing_current_epoch: true,
        }
    }

    #[test]
    fn test_all_qualified_equal_distribution() {
        let nodes = vec![qualified(1), qualified(2), qualified(3)];
        let result = dissolve(300.0, &nodes, &[]).unwrap();

        assert!((result.total_dissolved - 300.0).abs() < f64::EPSILON);
        assert_eq!(result.distributions.len(), 3);

        for dist in &result.distributions {
            assert!((dist.amount - 100.0).abs() < f64::EPSILON);
            assert!(!dist.held_shards);
        }
    }

    #[test]
    fn test_shard_holders_double_weight() {
        // Node 1 is a shard holder (weight 2), node 2 is not (weight 1).
        // Total weight = 3. Node 1 gets 2/3, node 2 gets 1/3.
        let nodes = vec![qualified(1), qualified(2)];
        let result = dissolve(300.0, &nodes, &[1]).unwrap();

        assert_eq!(result.distributions.len(), 2);

        let d1 = result.distributions.iter().find(|d| d.node_id == 1).unwrap();
        let d2 = result.distributions.iter().find(|d| d.node_id == 2).unwrap();

        assert!((d1.amount - 200.0).abs() < f64::EPSILON);
        assert!(d1.held_shards);

        assert!((d2.amount - 100.0).abs() < f64::EPSILON);
        assert!(!d2.held_shards);
    }

    #[test]
    fn test_no_qualified_nodes() {
        let nodes = vec![unqualified(1), unqualified(2), unqualified(3)];
        let result = dissolve(300.0, &nodes, &[]);

        assert_eq!(result, Err(DissolutionError::NoQualifiedNodes));
    }

    #[test]
    fn test_partial_qualification() {
        // Nodes 1 and 3 qualified, node 2 unqualified.
        let nodes = vec![qualified(1), unqualified(2), qualified(3)];
        let result = dissolve(200.0, &nodes, &[]).unwrap();

        assert_eq!(result.distributions.len(), 2);

        let ids: Vec<u32> = result.distributions.iter().map(|d| d.node_id).collect();
        assert!(ids.contains(&1));
        assert!(ids.contains(&3));
        assert!(!ids.contains(&2));

        for dist in &result.distributions {
            assert!((dist.amount - 100.0).abs() < f64::EPSILON);
        }
    }

    #[test]
    fn test_zero_residual() {
        let nodes = vec![qualified(1), qualified(2)];
        let result = dissolve(0.0, &nodes, &[]);

        assert_eq!(result, Err(DissolutionError::ZeroResidualValue));
    }

    #[test]
    fn test_eligibility_secs() {
        assert!(!is_eligible_secs(0));
        assert!(!is_eligible_secs(7_775_999));
        assert!(is_eligible_secs(7_776_000));
        assert!(is_eligible_secs(10_000_000));
    }

    #[test]
    fn test_eligibility_ticks() {
        assert!(!is_eligible_ticks(0));
        assert!(!is_eligible_ticks(4999));
        assert!(is_eligible_ticks(5000));
        assert!(is_eligible_ticks(10_000));
    }

    #[test]
    fn test_single_qualified_node() {
        let nodes = vec![qualified(42)];
        let result = dissolve(500.0, &nodes, &[]).unwrap();

        assert_eq!(result.distributions.len(), 1);
        assert_eq!(result.distributions[0].node_id, 42);
        assert!((result.distributions[0].amount - 500.0).abs() < f64::EPSILON);
    }
}
