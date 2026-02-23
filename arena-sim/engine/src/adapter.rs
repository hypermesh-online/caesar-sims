//! Adapter layer: converts between Arena's f64 world and core's Decimal types.

use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive;
use crate::core_types::{GoldGrams, MarketTier as CoreTier};
use crate::core_governor::pid::{GovernorPid as CoreGovernor, NetworkMetrics as CoreMetrics, TierCounts};
use crate::core_governor::params::GovernanceParams;
use crate::types::{MarketTier as ArenaTier, WorldState};

/// Convert f64 to Decimal (lossy but sufficient for simulation).
pub fn to_decimal(v: f64) -> Decimal {
    Decimal::from_f64(v).unwrap_or(Decimal::ZERO)
}

/// Convert Decimal to f64.
pub fn from_decimal(d: Decimal) -> f64 {
    use rust_decimal::prelude::ToPrimitive;
    d.to_f64().unwrap_or(0.0)
}

/// Arena MarketTier → Core MarketTier
pub fn to_core_tier(tier: &ArenaTier) -> CoreTier {
    match tier {
        ArenaTier::L0 => CoreTier::L0,
        ArenaTier::L1 => CoreTier::L1,
        ArenaTier::L2 => CoreTier::L2,
        ArenaTier::L3 => CoreTier::L3,
    }
}

/// Core MarketTier → Arena MarketTier
pub fn to_arena_tier(tier: &CoreTier) -> ArenaTier {
    match tier {
        CoreTier::L0 => ArenaTier::L0,
        CoreTier::L1 => ArenaTier::L1,
        CoreTier::L2 => ArenaTier::L2,
        CoreTier::L3 => ArenaTier::L3,
    }
}

/// Build core NetworkMetrics from Arena WorldState.
pub fn world_to_metrics(
    state: &WorldState,
    volatility: f64,
    lambda: f64,
) -> CoreMetrics {
    CoreMetrics {
        current_gold_price_usd: to_decimal(state.gold_price),
        target_gold_price_usd: to_decimal(2600.0), // canonical Caesar peg target
        market_volatility: to_decimal(volatility),
        transaction_volume: to_decimal(state.network_velocity),
        liquidity_depth: to_decimal(lambda * 1_000_000.0),
        network_velocity: to_decimal(state.network_velocity),
        active_packets_by_tier: TierCounts {
            l0: state.tier_distribution[0] as u64,
            l1: state.tier_distribution[1] as u64,
            l2: state.tier_distribution[2] as u64,
            l3: state.tier_distribution[3] as u64,
        },
        in_transit_float: to_decimal(state.active_value),
    }
}

/// Convert core GovernanceParams fee rate to f64.
/// Uses the recommended_fee_adjustment to compute the effective base fee rate.
pub fn params_to_fee_rate(params: &GovernanceParams) -> f64 {
    // Core base fee is 0.001 (same as Arena's BASE_FEE)
    let base = 0.001_f64;
    let adj = from_decimal(params.recommended_fee_adjustment);
    (base * (1.0 + adj)).max(0.0)
}

/// Calculate fee using core governor for a given tier and packet value.
pub fn calculate_fee_via_core(
    governor: &CoreGovernor,
    tier: &ArenaTier,
    base_rate: f64,
    packet_value: f64,
) -> f64 {
    let params = governor.last_params();
    let fee = governor.calculate_fee(
        params,
        to_core_tier(tier),
        to_decimal(base_rate),
        to_decimal(packet_value),
    );
    from_decimal(fee)
}

/// Split rewards using core 80/20 split.
pub fn split_rewards_via_core(governor: &CoreGovernor, total: f64) -> (f64, f64) {
    let split = governor.split_rewards(GoldGrams::from_decimal(to_decimal(total)));
    (from_decimal(split.egress_share.0), from_decimal(split.transit_share.0))
}

/// Compute capacity score using core's Decimal-based formula.
/// Returns a value in the same range as Arena's score_candidate (roughly -0.4..0.6).
pub fn score_capacity_via_core(
    bandwidth: f64,
    buffer_free_ratio: f64,
    latency_norm: f64,
    load_norm: f64,
) -> f64 {
    let bw = to_decimal(bandwidth);
    let buf = to_decimal(buffer_free_ratio);
    let lat = to_decimal(latency_norm);
    let load = to_decimal(load_norm);

    // Core formula weights (same as core_routing WEIGHT_* constants)
    let w_bw = to_decimal(0.35);
    let w_buf = to_decimal(0.25);
    let w_lat = to_decimal(0.25);
    let w_load = to_decimal(0.15);

    let score = w_bw * bw + w_buf * buf - w_lat * lat - w_load * load;
    from_decimal(score)
}

/// Cross-check a settlement against core's Decimal-based conservation law.
/// Returns (balanced, circuit_breaker_tripped).
/// This is a parallel validation — does NOT gate Arena's own conservation.
pub fn verify_settlement_via_core(
    law: &mut crate::core_conservation::ConservationLaw,
    initial: f64,
    settled: f64,
    fees: f64,
    demurrage: f64,
) -> (bool, bool) {
    use crate::core_types::GoldGrams;
    let result = law.verify_settlement(
        GoldGrams::from_decimal(to_decimal(initial)),
        GoldGrams::from_decimal(to_decimal(settled)),
        GoldGrams::from_decimal(to_decimal(fees)),
        GoldGrams::from_decimal(to_decimal(demurrage)),
    );
    let balanced = result.is_ok();
    let tripped = law.is_circuit_breaker_tripped();
    (balanced, tripped)
}

/// Distribute a fee using core's Decimal-based 80/20 splitter.
/// Returns (egress_amount, per_transit_amount).
/// transit_ids with 0 bytes → core does equal split (same as Arena's current behavior).
pub fn distribute_fee_via_core(
    total_fee: f64,
    egress_id: u32,
    transit_ids: &[u32],
) -> (f64, f64) {
    use crate::core_fee_distribution::FeeDistributor;
    use crate::core_types::{GoldGrams, NodeId};

    if total_fee <= 0.0 {
        return (0.0, 0.0);
    }

    let distributor = FeeDistributor::default();
    let egress_node = NodeId::from(format!("node-{}", egress_id));
    let transit_nodes: Vec<(NodeId, u64)> = transit_ids
        .iter()
        .map(|&id| (NodeId::from(format!("node-{}", id)), 0u64))
        .collect();

    match distributor.distribute_fee(
        GoldGrams::from_decimal(to_decimal(total_fee)),
        egress_node,
        &transit_nodes,
    ) {
        Ok(dist) => {
            let egress_amt = from_decimal(dist.egress_payment.amount.0);
            let per_transit = if dist.transit_payments.is_empty() {
                0.0
            } else {
                from_decimal(dist.transit_payments[0].amount.0)
            };
            (egress_amt, per_transit)
        }
        Err(_) => (0.0, 0.0),
    }
}
