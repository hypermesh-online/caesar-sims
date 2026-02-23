// Copyright 2026 Hypermesh Foundation. All rights reserved.
// Caesar Protocol Simulation Suite ("The Arena") - PID Governor
//
// Implements a canonical PID controller for fee/demurrage governance.
// Replaces the earlier quadrant-based heuristic with a continuous
// control loop that tracks gold-price peg error and adjusts fees,
// demurrage, and verification complexity accordingly.

use crate::types::{GovernorOutput, MarketTier, WorldState};
use serde::{Deserialize, Serialize};

// ─── Constants ──────────────────────────────────────────────────────────────

const PID_KP: f64 = 0.5;
const PID_KI: f64 = 0.1;
const PID_KD: f64 = 0.05;

const INTEGRAL_CLAMP_MIN: f64 = -0.1;
const INTEGRAL_CLAMP_MAX: f64 = 0.1;

const PID_OUTPUT_MIN: f64 = -0.02;
const PID_OUTPUT_MAX: f64 = 0.02;

const BASE_FEE: f64 = 0.001;
const BASE_DEMURRAGE: f64 = 0.005;

// Health score weights (4-component, matches core governor)
const HEALTH_WEIGHT_GOLD: f64 = 0.4;
const HEALTH_WEIGHT_VOLATILITY: f64 = 0.3;
const HEALTH_WEIGHT_TRANSACTION: f64 = 0.2;
const HEALTH_WEIGHT_LIQUIDITY: f64 = 0.1;

const HIGH_VOLUME: f64 = 1_000_000.0;
const LOW_LIQUIDITY: f64 = 100_000.0;

const DEVIATION_THRESHOLD: f64 = 0.18;

const REWARD_SPLIT_EGRESS: f64 = 0.80;
const REWARD_SPLIT_TRANSIT: f64 = 0.20;

// Tier modifier scaling factors (from core governor/pid.rs)
// Each tier's fee modifier = 1.0 + pid_adjustment * TIER_SCALE_*
const TIER_SCALE_L0: f64 = 1.5;
const TIER_SCALE_L1: f64 = 1.2;
const TIER_SCALE_L2: f64 = 0.8;
const TIER_SCALE_L3: f64 = 0.5;

// ─── Pressure Quadrant ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PressureQuadrant {
    GoldenEra,
    Bubble,
    Crash,
    Stagnation,
    Bottleneck,
    Vacuum,
}

impl PressureQuadrant {
    /// Human-readable governance quadrant label.
    pub fn label(&self) -> &'static str {
        match self {
            Self::GoldenEra => "D: GOLDEN ERA",
            Self::Bubble => "A: BUBBLE",
            Self::Crash => "B: CRASH",
            Self::Stagnation => "C: STAGNATION",
            Self::Bottleneck => "E: BOTTLENECK",
            Self::Vacuum => "F: VACUUM",
        }
    }

    /// Status string describing current governance posture.
    pub fn status(&self) -> &'static str {
        match self {
            Self::GoldenEra => "STABLE",
            Self::Bubble => "OVER-PEG: VENTING",
            Self::Crash => "UNDER-PEG: EMERGENCY BRAKE",
            Self::Stagnation => "UNDER-PEG: STIMULUS",
            Self::Bottleneck => "CONGESTED: THROTTLING",
            Self::Vacuum => "LOW ACTIVITY: INCENTIVIZING",
        }
    }

    /// Demurrage override for the given quadrant.
    pub fn demurrage_override(&self) -> f64 {
        match self {
            Self::Bubble => 0.10,
            Self::Crash => 0.0,
            Self::Stagnation => 0.001,
            Self::Bottleneck => BASE_DEMURRAGE * 1.5,
            Self::Vacuum => BASE_DEMURRAGE * 0.5,
            Self::GoldenEra => BASE_DEMURRAGE,
        }
    }
}

// ─── Network Metrics ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkMetrics {
    pub current_gold_price: f64,
    pub target_gold_price: f64,
    pub market_volatility: f64,
    pub transaction_volume: f64,
    pub liquidity_depth: f64,
    pub network_velocity: f64,
    pub active_packets_by_tier: [u64; 4],
    pub in_transit_float: f64,
}

// ─── Tier Modifiers ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TierModifiers {
    pub l0: f64,
    pub l1: f64,
    pub l2: f64,
    pub l3: f64,
}

impl Default for TierModifiers {
    fn default() -> Self {
        // Core uses neutral defaults (all 1.0x); dynamic adjustment via PID
        Self {
            l0: 1.0,
            l1: 1.0,
            l2: 1.0,
            l3: 1.0,
        }
    }
}

impl TierModifiers {
    /// Compute dynamic tier modifiers from PID fee adjustment.
    ///
    /// Core formula (governor/pid.rs):
    ///   l0: 1.0 + adj * 1.5
    ///   l1: 1.0 + adj * 1.2
    ///   l2: 1.0 + adj * 0.8
    ///   l3: 1.0 + adj * 0.5
    pub fn from_adjustment(adj: f64) -> Self {
        Self {
            l0: 1.0 + adj * TIER_SCALE_L0,
            l1: 1.0 + adj * TIER_SCALE_L1,
            l2: 1.0 + adj * TIER_SCALE_L2,
            l3: 1.0 + adj * TIER_SCALE_L3,
        }
    }

    pub fn for_tier(&self, tier: MarketTier) -> f64 {
        match tier {
            MarketTier::L0 => self.l0,
            MarketTier::L1 => self.l1,
            MarketTier::L2 => self.l2,
            MarketTier::L3 => self.l3,
        }
    }
}

// ─── Fee Caps ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeeCaps {
    pub l0: f64,
    pub l1: f64,
    pub l2: f64,
    pub l3: f64,
}

impl Default for FeeCaps {
    fn default() -> Self {
        Self {
            l0: 0.05,
            l1: 0.02,
            l2: 0.005,
            l3: 0.001,
        }
    }
}

impl FeeCaps {
    /// Constitutional fee cap for the given tier.
    pub fn cap_for(&self, tier: MarketTier) -> f64 {
        match tier {
            MarketTier::L0 => self.l0,
            MarketTier::L1 => self.l1,
            MarketTier::L2 => self.l2,
            MarketTier::L3 => self.l3,
        }
    }

    /// Clamp a fee so it never exceeds the constitutional cap for the tier,
    /// and never exceeds the packet value itself.
    pub fn clamp_fee(
        &self,
        tier: MarketTier,
        fee: f64,
        packet_value: f64,
    ) -> f64 {
        let cap = self.cap_for(tier) * packet_value;
        fee.min(cap).min(packet_value).max(0.0)
    }
}

// ─── Health-to-Fee Adjustment (core governor bracket mapping) ───────────────

/// Map a 0-10 health score to a base fee adjustment.
///
/// Core brackets (governor/pid.rs):
///   health >= 8.5: -0.008 (relax fees)
///   health >= 7.5: -0.006
///   health >= 6.5: -0.004
///   health >= 5.5: -0.002
///   health >= 5.0:  0.000 (neutral)
///   health >= 4.0: +0.002
///   health <  4.0: +0.005 (increase fees)
fn score_to_fee_adjustment(health: f64) -> f64 {
    if health >= 8.5 {
        -0.008
    } else if health >= 7.5 {
        -0.006
    } else if health >= 6.5 {
        -0.004
    } else if health >= 5.5 {
        -0.002
    } else if health >= 5.0 {
        0.0
    } else if health >= 4.0 {
        0.002
    } else {
        0.005
    }
}

// ─── PID Governor ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernorPid {
    pub kp: f64,
    pub ki: f64,
    pub kd: f64,
    pub integral_error: f64,
    pub previous_error: f64,
    pub fee_caps: FeeCaps,
}

impl Default for GovernorPid {
    fn default() -> Self {
        Self {
            kp: PID_KP,
            ki: PID_KI,
            kd: PID_KD,
            integral_error: 0.0,
            previous_error: 0.0,
            fee_caps: FeeCaps::default(),
        }
    }
}

impl GovernorPid {
    /// Run one PID control cycle and produce a `GovernorOutput`.
    pub fn recalculate(&mut self, metrics: &NetworkMetrics) -> GovernorOutput {
        // --- Error signal ---
        let error = if metrics.target_gold_price.abs() < 1e-12 {
            0.0
        } else {
            (metrics.current_gold_price - metrics.target_gold_price)
                / metrics.target_gold_price
        };

        // --- Integral with anti-windup ---
        self.integral_error = (self.integral_error + error)
            .clamp(INTEGRAL_CLAMP_MIN, INTEGRAL_CLAMP_MAX);

        // --- Derivative ---
        let derivative = error - self.previous_error;
        self.previous_error = error;

        // --- PID law ---
        let pid_output = (self.kp * error
            + self.ki * self.integral_error
            + self.kd * derivative)
            .clamp(PID_OUTPUT_MIN, PID_OUTPUT_MAX);

        // --- Health score (0..10, matches core 4-component formula) ---
        let gold_component =
            ((1.0 - error.abs()).max(0.0) * 10.0).clamp(0.0, 10.0);
        let volatility_component =
            ((1.0 - metrics.market_volatility).max(0.0) * 10.0).clamp(0.0, 10.0);
        let transaction_component =
            (metrics.transaction_volume / HIGH_VOLUME * 10.0).clamp(0.0, 10.0);
        let liquidity_component =
            (metrics.liquidity_depth / LOW_LIQUIDITY * 10.0).clamp(0.0, 10.0);

        let health_raw = HEALTH_WEIGHT_GOLD * gold_component
            + HEALTH_WEIGHT_VOLATILITY * volatility_component
            + HEALTH_WEIGHT_TRANSACTION * transaction_component
            + HEALTH_WEIGHT_LIQUIDITY * liquidity_component;
        let health = health_raw / 10.0; // normalize to 0..1 for complexity mapping

        // --- Health-based fee adjustment (core bracket mapping) ---
        let base_adj = score_to_fee_adjustment(health_raw);

        // --- Combined fee adjustment: health brackets + PID ---
        let final_adj =
            (base_adj + pid_output).clamp(PID_OUTPUT_MIN, PID_OUTPUT_MAX);

        // --- Pressure classification ---
        let deviation = error.abs();
        let quadrant = classify_pressure(
            deviation,
            error,
            metrics.network_velocity,
            metrics.transaction_volume,
            metrics.liquidity_depth,
        );

        // --- Fee rate (base_adj from health + PID) ---
        let fee_rate = (BASE_FEE * (1.0 + final_adj)).max(0.0);

        // --- Dynamic tier modifiers from core formula ---
        let _tier_modifiers = TierModifiers::from_adjustment(final_adj);

        // --- Demurrage ---
        let demurrage = quadrant.demurrage_override();

        // --- Verification complexity from health ---
        // Lower health -> higher complexity (1..5 range)
        let verification_complexity =
            (1.0 + (1.0 - health) * 4.0).round() as u64;

        GovernorOutput {
            fee_rate,
            demurrage,
            quadrant: quadrant.label().to_string(),
            status: quadrant.status().to_string(),
            verification_complexity,
        }
    }
}

// ─── Pressure Classification ────────────────────────────────────────────────

/// Classify the network pressure quadrant from simulation-scale metrics.
///
/// Simulation-scale thresholds (tick-based, not real-world):
///   - velocity: settled_count * 100 in simulation
///   - volume: maps to network_velocity (tick-scaled)
///   - liquidity_depth: lambda * 1_000_000
///
/// Canonical core thresholds (governor/pid.rs, real-world scale):
///   HIGH_VELOCITY: 1.5 txn/sec   LOW_VELOCITY: 0.3 txn/sec
///   LOW_VOLUME:  100_000 gold-g   HIGH_VOLUME: 1_000_000 gold-g
///   LOW_LIQUIDITY: 100_000        HIGH_LIQUIDITY: 1_000_000
fn classify_pressure(
    deviation: f64,
    signed_error: f64,
    velocity: f64,
    volume: f64,
    liquidity: f64,
) -> PressureQuadrant {
    // Simulation-scale: velocity 80 ~ core HIGH_VELOCITY 1.5 txn/sec
    // Simulation-scale: volume 500 ~ core HIGH_VOLUME 1_000_000 gold-g
    if deviation > DEVIATION_THRESHOLD && signed_error > 0.0 {
        // High positive deviation: check if congestion or bubble
        // Simulation-scale: velocity > 80, volume > 500
        // Core-scale: velocity > HIGH_VELOCITY(1.5), volume > HIGH_VOLUME
        if velocity > 80.0 && volume > 500.0 {
            PressureQuadrant::Bottleneck
        } else {
            PressureQuadrant::Bubble
        }
    } else if deviation > DEVIATION_THRESHOLD && signed_error <= 0.0 {
        PressureQuadrant::Crash
    } else if velocity < 10.0 && volume < 50.0 {
        // Simulation-scale: velocity < 10, volume < 50
        // Core-scale: velocity < LOW_VELOCITY(0.3), volume < LOW_VOLUME
        PressureQuadrant::Stagnation
    } else if liquidity > 500_000.0 && volume < 100.0 {
        // Simulation-scale: liquidity > 500k, volume < 100
        // Core-scale: liquidity > HIGH_LIQUIDITY, volume < LOW_VOLUME
        PressureQuadrant::Vacuum
    } else {
        PressureQuadrant::GoldenEra
    }
}

// ─── Backward-Compatible Entry Point ────────────────────────────────────────

/// Compute the Caesar Governor output based on world state, volatility,
/// liquidity coefficient (lambda), and surge multiplier.
///
/// This is the backward-compatible wrapper used by `simulation.rs`.
/// It translates WorldState fields into `NetworkMetrics`, runs the PID
/// controller, then applies the legacy policy overrides (panic, NGauge
/// discount, speculation detection, surge pricing).
pub fn compute_governor(
    state: &WorldState,
    volatility: f64,
    lambda: f64,
    surge_multiplier: f64,
) -> GovernorOutput {
    // Build NetworkMetrics from the available WorldState fields.
    let metrics = NetworkMetrics {
        current_gold_price: state.gold_price,
        target_gold_price: 2600.0, // canonical Caesar peg target
        market_volatility: volatility,
        transaction_volume: state.network_velocity,
        liquidity_depth: lambda * 1_000_000.0, // scale lambda back up
        network_velocity: state.network_velocity,
        active_packets_by_tier: [
            state.tier_distribution[0] as u64,
            state.tier_distribution[1] as u64,
            state.tier_distribution[2] as u64,
            state.tier_distribution[3] as u64,
        ],
        in_transit_float: state.active_value,
    };

    let mut pid = GovernorPid::default();
    let mut gov = pid.recalculate(&metrics);

    // --- Legacy overrides (preserve original behavior) ---

    // S3: Panic level forces toward crisis behavior
    if state.panic_level > 0.7 {
        gov.fee_rate = gov.fee_rate.max(0.05);
        gov.demurrage *= 0.5;
    }

    // NGauge activity discount
    if state.ngauge_activity_index > 0.5 {
        gov.fee_rate *= 0.8;
    }

    // E7: Organic vs Speculative Detection
    let organic_ratio = if state.network_velocity > 100.0 {
        state.ngauge_activity_index
            / (state.network_velocity / 1000.0).max(0.1)
    } else {
        1.0
    };

    if organic_ratio < 0.3 {
        gov.fee_rate *= 1.5;
    }

    // E8: Surge pricing on fee rate during liquidity crunch
    if lambda < 0.5 {
        gov.fee_rate *= surge_multiplier;
    }

    gov
}

/// Compute per-tier effective fee rates from a base fee rate and adjustment.
///
/// Uses core tier modifier formula:
///   l0: 1.0 + adj * 1.5
///   l1: 1.0 + adj * 1.2
///   l2: 1.0 + adj * 0.8
///   l3: 1.0 + adj * 0.5
pub fn compute_tier_fee_rates(base_fee_rate: f64) -> [f64; 4] {
    let caps = FeeCaps::default();
    // Use the base_fee_rate as-is; the adjustment is already baked in.
    // Derive the implicit adjustment: fee_rate = BASE_FEE * (1 + adj)
    let adj = if BASE_FEE.abs() > 1e-15 {
        (base_fee_rate / BASE_FEE) - 1.0
    } else {
        0.0
    };
    let modifiers = TierModifiers::from_adjustment(adj);
    [
        (base_fee_rate * modifiers.l0).min(caps.l0).max(0.0),
        (base_fee_rate * modifiers.l1).min(caps.l1).max(0.0),
        (base_fee_rate * modifiers.l2).min(caps.l2).max(0.0),
        (base_fee_rate * modifiers.l3).min(caps.l3).max(0.0),
    ]
}

// ─── Tier-Aware Fee Calculation ─────────────────────────────────────────────

/// Calculate the fee for a given tier, base rate, packet value, and optional
/// tier modifiers.
///
/// raw = base_rate * tier_modifier * packet_value, clamped to the
/// constitutional cap for that tier.
///
/// When `modifiers` is `None`, neutral defaults (all 1.0) are used.
pub fn calculate_fee(
    tier: MarketTier,
    base_rate: f64,
    packet_value: f64,
) -> f64 {
    calculate_fee_with_modifiers(tier, base_rate, packet_value, None)
}

/// Calculate the fee with explicit tier modifiers.
pub fn calculate_fee_with_modifiers(
    tier: MarketTier,
    base_rate: f64,
    packet_value: f64,
    modifiers: Option<&TierModifiers>,
) -> f64 {
    let default_mods = TierModifiers::default();
    let mods = modifiers.unwrap_or(&default_mods);
    let caps = FeeCaps::default();

    let raw = base_rate * mods.for_tier(tier) * packet_value;
    caps.clamp_fee(tier, raw, packet_value)
}

// ─── Reward Splitting ───────────────────────────────────────────────────────

/// Split total fee rewards into (egress_share, transit_share).
/// 80% to egress nodes, 20% to transit nodes.
pub fn split_rewards(total: f64) -> (f64, f64) {
    let egress = total * REWARD_SPLIT_EGRESS;
    let transit = total * REWARD_SPLIT_TRANSIT;
    (egress, transit)
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn default_metrics() -> NetworkMetrics {
        NetworkMetrics {
            current_gold_price: 2600.0,
            target_gold_price: 2600.0,
            market_volatility: 0.05,
            transaction_volume: 200.0,
            liquidity_depth: 500_000.0,
            network_velocity: 50.0,
            active_packets_by_tier: [100, 50, 20, 5],
            in_transit_float: 10_000.0,
        }
    }

    fn default_world_state() -> WorldState {
        WorldState {
            current_tick: 100,
            gold_price: 2600.0,
            peg_deviation: 0.0,
            network_velocity: 200.0,
            demand_factor: 0.5,
            panic_level: 0.0,
            governance_quadrant: "D: GOLDEN ERA".into(),
            governance_status: "STABLE".into(),
            total_rewards_egress: 0.0,
            total_rewards_transit: 0.0,
            total_fees_collected: 0.0,
            total_demurrage_burned: 0.0,
            current_fee_rate: 0.001,
            current_demurrage_rate: 0.005,
            verification_complexity: 1,
            ngauge_activity_index: 0.6,
            total_value_leaked: 0.0,
            total_network_utility: 0.0,
            volatility: 0.05,
            settlement_count: 0,
            revert_count: 0,
            orbit_count: 0,
            total_input: 0.0,
            total_output: 0.0,
            active_value: 5000.0,
            spawn_count: 0,
            organic_ratio: 1.0,
            surge_multiplier: 1.0,
            circuit_breaker_active: false,
            ingress_throttle: 0.0,
            dissolved_count: 0,
            held_count: 0,
            tier_distribution: [100, 50, 20, 5],
            effective_price_composite: 0.0,
            network_fee_component: 0.0,
            speculation_component: 0.0,
            float_component: 0.0,
            tier_fee_rates: [0.0; 4],
        }
    }

    #[test]
    fn pid_golden_era_at_peg() {
        let mut pid = GovernorPid::default();
        let metrics = default_metrics();
        let out = pid.recalculate(&metrics);

        assert_eq!(out.quadrant, "D: GOLDEN ERA");
        assert_eq!(out.status, "STABLE");
        // At peg (error=0), PID output is 0. Health score:
        //   gold=10*0.4=4, vol=(1-0.05)*10*0.3=2.85, txn=200/1e6*10*0.2=0.0004,
        //   liq=500k/100k*10*0.1=1.0 (capped) => raw~7.85 => bracket >= 7.5 => -0.006
        // final_adj = -0.006 + 0.0 = -0.006
        // fee_rate = 0.001 * (1.0 + (-0.006)) = 0.001 * 0.994 = 0.000994
        assert!(
            (out.fee_rate - BASE_FEE).abs() < 0.001,
            "fee_rate {} should be close to BASE_FEE {}",
            out.fee_rate,
            BASE_FEE
        );
        assert!((out.demurrage - BASE_DEMURRAGE).abs() < 1e-9);
    }

    #[test]
    fn pid_bubble_on_high_positive_deviation() {
        let mut pid = GovernorPid::default();
        let mut metrics = default_metrics();
        // 30% above target
        metrics.current_gold_price = 3380.0;
        let out = pid.recalculate(&metrics);

        assert_eq!(out.quadrant, "A: BUBBLE");
        assert_eq!(out.demurrage, 0.10);
    }

    #[test]
    fn pid_crash_on_high_negative_deviation() {
        let mut pid = GovernorPid::default();
        let mut metrics = default_metrics();
        // 30% below target
        metrics.current_gold_price = 1820.0;
        let out = pid.recalculate(&metrics);

        assert_eq!(out.quadrant, "B: CRASH");
        assert_eq!(out.demurrage, 0.0);
    }

    #[test]
    fn pid_stagnation_on_low_activity() {
        let mut pid = GovernorPid::default();
        let mut metrics = default_metrics();
        metrics.network_velocity = 5.0;
        metrics.transaction_volume = 10.0;
        let out = pid.recalculate(&metrics);

        assert_eq!(out.quadrant, "C: STAGNATION");
        assert_eq!(out.demurrage, 0.001);
    }

    #[test]
    fn pid_vacuum_high_liquidity_low_volume() {
        let mut pid = GovernorPid::default();
        let mut metrics = default_metrics();
        metrics.liquidity_depth = 600_000.0;
        metrics.transaction_volume = 50.0;
        metrics.network_velocity = 50.0;
        let out = pid.recalculate(&metrics);

        assert_eq!(out.quadrant, "F: VACUUM");
    }

    #[test]
    fn pid_bottleneck_high_deviation_congested() {
        let mut pid = GovernorPid::default();
        let mut metrics = default_metrics();
        metrics.current_gold_price = 3400.0; // large positive deviation
        metrics.network_velocity = 100.0;
        metrics.transaction_volume = 600.0;
        let out = pid.recalculate(&metrics);

        assert_eq!(out.quadrant, "E: BOTTLENECK");
    }

    #[test]
    fn pid_integral_anti_windup() {
        let mut pid = GovernorPid::default();
        let mut metrics = default_metrics();
        // Push error repeatedly in one direction
        metrics.current_gold_price = 5000.0;
        for _ in 0..100 {
            pid.recalculate(&metrics);
        }
        // Integral must be clamped
        assert!(pid.integral_error <= INTEGRAL_CLAMP_MAX);
        assert!(pid.integral_error >= INTEGRAL_CLAMP_MIN);
    }

    #[test]
    fn pid_output_clamped() {
        let mut pid = GovernorPid::default();
        let mut metrics = default_metrics();
        metrics.current_gold_price = 10_000.0; // extreme deviation
        let out = pid.recalculate(&metrics);

        // final_adj is clamped to PID_OUTPUT_MAX, so fee_rate <= BASE_FEE * (1 + PID_OUTPUT_MAX)
        let max_fee = BASE_FEE * (1.0 + PID_OUTPUT_MAX);
        assert!(out.fee_rate <= max_fee + 1e-12);
    }

    #[test]
    fn compute_governor_backward_compat() {
        let state = default_world_state();
        let out = compute_governor(&state, 0.05, 0.8, 1.5);

        // Should produce valid output
        assert!(out.fee_rate >= 0.0);
        assert!(out.demurrage >= 0.0);
        assert!(!out.quadrant.is_empty());
        assert!(!out.status.is_empty());
        assert!(out.verification_complexity >= 1);
    }

    #[test]
    fn compute_governor_panic_override() {
        let mut state = default_world_state();
        state.panic_level = 0.9;
        // Set ngauge low so the discount does not reduce the panic floor
        state.ngauge_activity_index = 0.2;
        // Set velocity low so organic ratio stays high (no speculation mult)
        state.network_velocity = 50.0;
        let out = compute_governor(&state, 0.05, 0.8, 1.5);

        // Panic > 0.7 forces fee_rate >= 0.05
        assert!(out.fee_rate >= 0.05);
    }

    #[test]
    fn compute_governor_ngauge_discount() {
        let mut state = default_world_state();
        state.ngauge_activity_index = 0.8;
        let out_high = compute_governor(&state, 0.05, 0.8, 1.0);

        state.ngauge_activity_index = 0.2;
        let out_low = compute_governor(&state, 0.05, 0.8, 1.0);

        // Higher NGauge activity should yield lower fee rate
        assert!(out_high.fee_rate < out_low.fee_rate);
    }

    #[test]
    fn compute_governor_speculation_detection() {
        let mut state = default_world_state();
        state.ngauge_activity_index = 0.01;
        state.network_velocity = 5000.0;
        let out = compute_governor(&state, 0.05, 0.8, 1.0);

        // Organic ratio < 0.3 should inflate fee
        let baseline_state = default_world_state();
        let baseline = compute_governor(&baseline_state, 0.05, 0.8, 1.0);
        assert!(out.fee_rate > baseline.fee_rate);
    }

    #[test]
    fn compute_governor_surge_pricing() {
        let state = default_world_state();
        let out_no_surge = compute_governor(&state, 0.05, 0.8, 2.0);
        let out_surge = compute_governor(&state, 0.05, 0.3, 2.0);

        // lambda < 0.5 activates surge multiplier
        assert!(out_surge.fee_rate > out_no_surge.fee_rate);
    }

    #[test]
    fn calculate_fee_l0() {
        // Uses default (neutral) modifiers: all 1.0
        let fee = calculate_fee(MarketTier::L0, 0.01, 1000.0);
        // raw = 0.01 * 1.0 * 1000 = 10.0
        // cap = 0.05 * 1000 = 50.0
        // clamped to min(10, 50, 1000) = 10
        assert!((fee - 10.0).abs() < 1e-9);
    }

    #[test]
    fn calculate_fee_l3_capped() {
        let fee = calculate_fee(MarketTier::L3, 0.05, 1000.0);
        // raw = 0.05 * 1.0 * 1000 = 50.0
        // cap = 0.001 * 1000 = 1.0
        // clamped to 1.0
        assert!((fee - 1.0).abs() < 1e-9);
    }

    #[test]
    fn calculate_fee_with_dynamic_modifiers() {
        // PID adjustment of +0.01 means L0 modifier = 1.0 + 0.01*1.5 = 1.015
        let mods = TierModifiers::from_adjustment(0.01);
        let fee = calculate_fee_with_modifiers(
            MarketTier::L0,
            0.01,
            1000.0,
            Some(&mods),
        );
        // raw = 0.01 * 1.015 * 1000 = 10.15
        // cap = 0.05 * 1000 = 50.0
        assert!((fee - 10.15).abs() < 1e-9);
    }

    #[test]
    fn calculate_fee_never_negative() {
        let fee = calculate_fee(MarketTier::L0, -0.5, 1000.0);
        assert!(fee >= 0.0);
    }

    #[test]
    fn split_rewards_80_20() {
        let (egress, transit) = split_rewards(100.0);
        assert!((egress - 80.0).abs() < 1e-9);
        assert!((transit - 20.0).abs() < 1e-9);
    }

    #[test]
    fn split_rewards_zero() {
        let (egress, transit) = split_rewards(0.0);
        assert!((egress).abs() < 1e-12);
        assert!((transit).abs() < 1e-12);
    }

    #[test]
    fn tier_modifiers_defaults() {
        let m = TierModifiers::default();
        // Core uses neutral 1.0x defaults for all tiers
        assert!((m.for_tier(MarketTier::L0) - 1.0).abs() < 1e-9);
        assert!((m.for_tier(MarketTier::L1) - 1.0).abs() < 1e-9);
        assert!((m.for_tier(MarketTier::L2) - 1.0).abs() < 1e-9);
        assert!((m.for_tier(MarketTier::L3) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn tier_modifiers_from_adjustment() {
        let m = TierModifiers::from_adjustment(0.02);
        assert!((m.l0 - (1.0 + 0.02 * 1.5)).abs() < 1e-9);
        assert!((m.l1 - (1.0 + 0.02 * 1.2)).abs() < 1e-9);
        assert!((m.l2 - (1.0 + 0.02 * 0.8)).abs() < 1e-9);
        assert!((m.l3 - (1.0 + 0.02 * 0.5)).abs() < 1e-9);

        // Negative adjustment
        let m_neg = TierModifiers::from_adjustment(-0.01);
        assert!((m_neg.l0 - (1.0 - 0.01 * 1.5)).abs() < 1e-9);
    }

    #[test]
    fn score_to_fee_adjustment_brackets() {
        assert!((score_to_fee_adjustment(9.0) - (-0.008)).abs() < 1e-12);
        assert!((score_to_fee_adjustment(8.5) - (-0.008)).abs() < 1e-12);
        assert!((score_to_fee_adjustment(8.0) - (-0.006)).abs() < 1e-12);
        assert!((score_to_fee_adjustment(7.0) - (-0.004)).abs() < 1e-12);
        assert!((score_to_fee_adjustment(6.0) - (-0.002)).abs() < 1e-12);
        assert!((score_to_fee_adjustment(5.0) - 0.0).abs() < 1e-12);
        assert!((score_to_fee_adjustment(4.5) - 0.002).abs() < 1e-12);
        assert!((score_to_fee_adjustment(3.0) - 0.005).abs() < 1e-12);
    }

    #[test]
    fn fee_caps_defaults() {
        let c = FeeCaps::default();
        assert!((c.cap_for(MarketTier::L0) - 0.05).abs() < 1e-9);
        assert!((c.cap_for(MarketTier::L1) - 0.02).abs() < 1e-9);
        assert!((c.cap_for(MarketTier::L2) - 0.005).abs() < 1e-9);
        assert!((c.cap_for(MarketTier::L3) - 0.001).abs() < 1e-9);
    }

    #[test]
    fn fee_caps_clamp_respects_packet_value() {
        let caps = FeeCaps::default();
        // Fee larger than packet value
        let clamped = caps.clamp_fee(MarketTier::L0, 2000.0, 100.0);
        // cap = 0.05 * 100 = 5.0, packet = 100 -> min(2000, 5, 100) = 5.0
        assert!((clamped - 5.0).abs() < 1e-9);
    }

    #[test]
    fn pid_zero_target_guard() {
        let mut pid = GovernorPid::default();
        let mut metrics = default_metrics();
        metrics.target_gold_price = 0.0;
        let out = pid.recalculate(&metrics);
        // Should not panic, fee should be close to base
        // error=0, pid=0, health_raw is high => bracket -0.006 or -0.008
        // final_adj = base_adj + 0 => fee = BASE_FEE * (1 + base_adj)
        assert!(out.fee_rate > 0.0);
        assert!((out.fee_rate - BASE_FEE).abs() < 0.001);
    }

    #[test]
    fn health_score_bounds() {
        let mut pid = GovernorPid::default();

        // Best health: at peg (error=0), zero volatility, max txn, max liq
        let mut metrics = default_metrics();
        metrics.current_gold_price = 2600.0;
        metrics.market_volatility = 0.0;
        metrics.transaction_volume = HIGH_VOLUME;
        metrics.liquidity_depth = LOW_LIQUIDITY;
        let out_best = pid.recalculate(&metrics);
        // health_raw = 10.0, health = 1.0 => complexity = 1
        assert_eq!(out_best.verification_complexity, 1);

        // Worst health: max deviation, max volatility, zero txn, zero liq
        metrics.current_gold_price = 5200.0;
        metrics.market_volatility = 1.0;
        metrics.transaction_volume = 0.0;
        metrics.liquidity_depth = 0.0;
        pid.integral_error = 0.0;
        pid.previous_error = 0.0;
        let out_worst = pid.recalculate(&metrics);
        // health_raw = 0.0, health = 0.0 => complexity = 5
        assert_eq!(out_worst.verification_complexity, 5);
    }

    #[test]
    fn compute_tier_fee_rates_produces_valid_array() {
        let rates = compute_tier_fee_rates(0.001);
        // All rates should be non-negative
        for rate in &rates {
            assert!(*rate >= 0.0);
        }
        // L0 should have the highest modifier (1.5x scale) so highest rate
        // L3 should have the lowest modifier (0.5x scale) so lowest rate
        // At base_fee=0.001, adj is ~0, so all rates are close to 0.001
        // but ordering holds: L0 >= L1 >= L2 >= L3
        assert!(rates[0] >= rates[3]);
    }

    #[test]
    fn compute_tier_fee_rates_caps_respected() {
        // Use a very high base_fee_rate to trigger caps
        let rates = compute_tier_fee_rates(1.0);
        let caps = FeeCaps::default();
        assert!(rates[0] <= caps.l0);
        assert!(rates[1] <= caps.l1);
        assert!(rates[2] <= caps.l2);
        assert!(rates[3] <= caps.l3);
    }
}
