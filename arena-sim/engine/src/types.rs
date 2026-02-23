// Copyright 2026 Hypermesh Foundation. All rights reserved.
// Caesar Protocol Simulation Suite ("The Arena") - Type Definitions

use serde::{Serialize, Deserialize};

// ─── Market Tier (v0.2) ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum MarketTier {
    L0 = 0,
    L1 = 1,
    L2 = 2,
    L3 = 3,
}

impl Default for MarketTier {
    fn default() -> Self { MarketTier::L0 }
}

impl MarketTier {
    pub fn fee_cap(&self) -> f64 {
        match self {
            Self::L0 => 0.05,
            Self::L1 => 0.02,
            Self::L2 => 0.005,
            Self::L3 => 0.001,
        }
    }

    /// Per-tick demurrage lambda (matches core DemurrageRate::lambda values)
    pub fn demurrage_lambda(&self) -> f64 {
        match self {
            Self::L0 => 1.39e-5,
            Self::L1 => 1.157e-8,
            Self::L2 => 1.157e-9,
            Self::L3 => 1.157e-10,
        }
    }

    /// Max TTL in seconds (matches core DemurrageRate::max_ttl_secs)
    pub fn max_ttl_secs(&self) -> u64 {
        match self {
            Self::L0 => 86_400,
            Self::L1 => 1_209_600,
            Self::L2 => 7_776_000,
            Self::L3 => 15_552_000,
        }
    }

    pub fn ttl_ticks(&self) -> u64 {
        match self {
            Self::L0 => 100,
            Self::L1 => 500,
            Self::L2 => 2000,
            Self::L3 => 7000,
        }
    }

    pub fn hop_limit(&self) -> u32 {
        match self {
            Self::L0 => 10,
            Self::L1 => 20,
            Self::L2 => 40,
            Self::L3 => 80,
        }
    }

    pub fn from_value(value: f64) -> Self {
        if value <= 10.0 {
            Self::L0
        } else if value <= 1_000.0 {
            Self::L1
        } else if value <= 100_000.0 {
            Self::L2
        } else {
            Self::L3
        }
    }
}

// ─── Node Role ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum NodeRole {
    Ingress = 0,
    Egress = 1,
    Transit = 2,
    NGauge = 3,
    Disabled = 4,
}

// ─── Node Strategy (E9) ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum NodeStrategy {
    RiskAverse = 0,
    Greedy = 1,
    Passive = 2,
}

pub fn default_strategy() -> NodeStrategy {
    NodeStrategy::Passive
}

// ─── Packet Status (canonical: matches core PacketState) ─────────────────────

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum PacketStatus {
    Minted = 0,       // just created at ingress
    InTransit = 1,    // moving through mesh
    Delivered = 2,    // arrived at destination, awaiting settlement
    Settling = 3,     // settlement in progress
    Settled = 4,      // TERMINAL: settled successfully
    Held = 5,         // receiver offline or surge exceeds fee budget
    Stalled = 6,      // route blocked, triggers rerouting
    Dispersed = 7,    // egress failed, shards re-dispersed
    Expired = 8,      // TTL reached, refund initiated
    Refunded = 9,     // TERMINAL: refunded to sender
    Dissolved = 10,   // TERMINAL: gravity dissolved
}

impl PacketStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Settled | Self::Refunded | Self::Dissolved)
    }
    pub fn is_active(&self) -> bool {
        !self.is_terminal()
    }
}

// ─── SimPacket ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimPacket {
    pub id: u64,
    pub original_value: f64,
    pub current_value: f64,
    pub arrival_tick: u64,
    pub status: PacketStatus,
    pub origin_node: u32,
    pub target_node: Option<u32>,
    pub hops: u32,
    pub route_history: Vec<u32>,
    #[serde(default)]
    pub orbit_start_tick: Option<u64>,
    // v0.2 fields
    #[serde(default)]
    pub tier: MarketTier,
    #[serde(default)]
    pub ttl: u64,
    #[serde(default)]
    pub hop_limit: u32,
    #[serde(default)]
    pub fee_budget: f64,
    #[serde(default)]
    pub fees_consumed: f64,
    #[serde(default)]
    pub fee_schedule: Vec<f64>,
    #[serde(default)]
    pub spawn_tick: u64,
}

// ─── SimNode ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimNode {
    pub id: u32,
    pub role: NodeRole,
    pub x: f64,
    pub y: f64,
    pub inventory_fiat: f64,
    pub inventory_crypto: f64,
    pub current_buffer_count: u32,
    pub neighbors: Vec<u32>,
    pub distance_to_egress: u32,
    pub total_fees_earned: f64,
    pub accumulated_work: f64,
    #[serde(default = "default_strategy")]
    pub strategy: NodeStrategy,
    #[serde(default)]
    pub pressure: f64,
    // v0.2 fields
    #[serde(default)]
    pub transit_fee: f64,
    #[serde(default)]
    pub bandwidth: f64,
    #[serde(default)]
    pub latency: f64,
    #[serde(default)]
    pub uptime: f64,
    #[serde(default)]
    pub tier_preference: Option<MarketTier>,
    #[serde(default)]
    pub upi_active: bool,
    #[serde(default)]
    pub ngauge_running: bool,
    #[serde(default)]
    pub kyc_valid: bool,
}

// ─── WorldState ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldState {
    pub current_tick: u64,
    pub gold_price: f64,
    pub peg_deviation: f64,
    pub network_velocity: f64,
    pub demand_factor: f64,
    pub panic_level: f64,
    pub governance_quadrant: String,
    pub governance_status: String,

    // Thermodynamic Stats
    pub total_rewards_egress: f64,
    pub total_rewards_transit: f64,
    pub total_fees_collected: f64,
    pub total_demurrage_burned: f64,
    pub current_fee_rate: f64,
    pub current_demurrage_rate: f64,
    pub verification_complexity: u64,
    pub ngauge_activity_index: f64,

    pub total_value_leaked: f64,
    pub total_network_utility: f64,

    #[serde(default)]
    pub volatility: f64,
    #[serde(default)]
    pub settlement_count: u32,
    #[serde(default)]
    pub revert_count: u32,
    #[serde(default)]
    pub orbit_count: u32,
    #[serde(default)]
    pub total_input: f64,
    #[serde(default)]
    pub total_output: f64,
    #[serde(default)]
    pub active_value: f64,
    #[serde(default)]
    pub spawn_count: u32,

    // E7: Organic ratio
    #[serde(default)]
    pub organic_ratio: f64,
    // E8: Surge multiplier
    #[serde(default)]
    pub surge_multiplier: f64,

    // v0.2 fields
    #[serde(default)]
    pub circuit_breaker_active: bool,
    #[serde(default)]
    pub ingress_throttle: f64,
    #[serde(default)]
    pub dissolved_count: u32,
    #[serde(default)]
    pub held_count: u32,
    #[serde(default)]
    pub tier_distribution: [u32; 4],
    #[serde(default)]
    pub effective_price_composite: f64,
    #[serde(default)]
    pub network_fee_component: f64,
    #[serde(default)]
    pub speculation_component: f64,
    #[serde(default)]
    pub float_component: f64,
    #[serde(default)]
    pub tier_fee_rates: [f64; 4],
}

// ─── TickResult ──────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct TickResult {
    pub state: WorldState,
    pub active_packets: Vec<SimPacket>,
    pub node_updates: Vec<NodeUpdate>,
}

// ─── NodeUpdate ──────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct NodeUpdate {
    pub id: u32,
    pub buffer_count: u32,
    pub inventory_fiat: f64,
    pub inventory_crypto: f64,
}

// ─── SimStats ────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct SimStats {
    pub total_input: f64,
    pub total_output: f64,
    pub total_burned: f64,
    pub total_fees: f64,
    pub total_leaked: f64,
    pub settlement_count: u32,
    pub revert_count: u32,
    pub orbit_count: u32,
    pub avg_hops: f64,
    pub avg_time_to_settle: f64,
}

// ─── GovernorOutput (v0.2) ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernorOutput {
    pub fee_rate: f64,
    pub demurrage: f64,
    pub quadrant: String,
    pub status: String,
    pub verification_complexity: u64,
}
