// Vendored from hypermesh-lib
//
// Minimal subset of types needed by the arena simulation engine.
// Extracted from hypermesh-lib types.rs (NodeId) and economic.rs
// (PacketId, GoldGrams, MarketTier, PacketState, DemurrageRate).

use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::{Add, Sub};

// ---------------------------------------------------------------------------
// NodeId (from types.rs)
// ---------------------------------------------------------------------------

/// Unique node identifier in the Block-MATRIX topology
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct NodeId(pub String);

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for NodeId {
    fn from(s: String) -> Self { NodeId(s) }
}

impl From<&str> for NodeId {
    fn from(s: &str) -> Self { NodeId(s.to_string()) }
}

// ---------------------------------------------------------------------------
// PacketId (from economic.rs)
// ---------------------------------------------------------------------------

/// Unique EVP packet identifier (32-byte hash)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PacketId(pub [u8; 32]);

impl PacketId {
    /// Create from raw bytes
    pub fn new(data: [u8; 32]) -> Self {
        Self(data)
    }

    /// Create a zeroed identifier (for defaults/tests)
    pub fn zero() -> Self {
        Self([0u8; 32])
    }
}

impl fmt::Display for PacketId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in &self.0[..8] {
            write!(f, "{:02x}", byte)?;
        }
        write!(f, "...")
    }
}

// ---------------------------------------------------------------------------
// GoldGrams (from economic.rs)
// ---------------------------------------------------------------------------

/// Gold-gram denomination backed by `rust_decimal::Decimal`
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct GoldGrams(pub Decimal);

impl GoldGrams {
    /// Zero value
    pub fn zero() -> Self {
        Self(Decimal::ZERO)
    }

    /// Create from a `Decimal` value
    pub fn from_decimal(d: Decimal) -> Self {
        Self(d)
    }

    /// Whether the value is exactly zero
    pub fn is_zero(&self) -> bool {
        self.0.is_zero()
    }
}

impl Add for GoldGrams {
    type Output = Self;
    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

impl Sub for GoldGrams {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self::Output {
        Self(self.0 - rhs.0)
    }
}

impl fmt::Display for GoldGrams {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}g", self.0)
    }
}

// ---------------------------------------------------------------------------
// MarketTier (from economic.rs)
// ---------------------------------------------------------------------------

/// Market tier classified by transaction value amount
///
/// L0 (retail) through L3 (sovereign), each with distinct demurrage parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MarketTier {
    /// Retail / Consumers
    L0,
    /// Professional / Small Institutions
    L1,
    /// Major Institutions
    L2,
    /// Sovereign / Systemic Actors
    L3,
}

impl MarketTier {
    /// Human-readable description of this tier
    pub fn description(&self) -> &'static str {
        match self {
            Self::L0 => "Retail / Consumers",
            Self::L1 => "Professional / Small Institutions",
            Self::L2 => "Major Institutions",
            Self::L3 => "Sovereign / Systemic Actors",
        }
    }

    /// Default demurrage parameters for this tier
    pub fn default_demurrage_rate(&self) -> DemurrageRate {
        match self {
            Self::L0 => DemurrageRate { lambda: 1.39e-5, max_ttl_secs: 86_400 },       // ~5%/hr, TTL 1 day
            Self::L1 => DemurrageRate { lambda: 1.157e-8, max_ttl_secs: 1_209_600 },   // ~0.1%/day, TTL 14 days
            Self::L2 => DemurrageRate { lambda: 1.157e-9, max_ttl_secs: 7_776_000 },   // ~0.01%/day, TTL 90 days
            Self::L3 => DemurrageRate { lambda: 1.157e-10, max_ttl_secs: 15_552_000 }, // ~0.001%/day, TTL 180 days
        }
    }
}

// ---------------------------------------------------------------------------
// PacketState (from economic.rs)
// ---------------------------------------------------------------------------

/// EVP lifecycle state
///
/// Born at `Minted`, dies at `Settled` / `Refunded` / `Dissolved`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PacketState {
    /// Just created at ingress
    Minted,
    /// Moving through mesh
    InTransit,
    /// Arrived at destination, awaiting settlement
    Delivered,
    /// External settlement in progress (egress adapter executing)
    Settling,
    /// TERMINAL: Successfully settled
    Settled,
    /// In holding pattern (orbit buffer) -- recipient offline/unavailable
    Held,
    /// Delivery failed, awaiting retry or refund
    Stalled,
    /// Egress settlement failed -- shards re-dispersed for retry
    Dispersed,
    /// TTL expired -- refund process initiated (non-terminal)
    Expired,
    /// TERMINAL: TTL expired, refund completed to sender
    Refunded,
    /// TERMINAL: Both parties abandoned, gravity bonus distributed
    Dissolved,
}

impl PacketState {
    /// Whether this state is terminal (no further transitions)
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Settled | Self::Refunded | Self::Dissolved)
    }

    /// Whether this state is active (packet still in flight or actionable)
    pub fn is_active(&self) -> bool {
        matches!(
            self,
            Self::Minted
                | Self::InTransit
                | Self::Delivered
                | Self::Settling
                | Self::Held
                | Self::Stalled
                | Self::Dispersed
                | Self::Expired
        )
    }
}

// ---------------------------------------------------------------------------
// DemurrageRate (from economic.rs)
// ---------------------------------------------------------------------------

/// Per-tier demurrage (decay) parameters
///
/// Value decays exponentially: `V_t = V_0 * e^(-lambda * t)`
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DemurrageRate {
    /// Decay rate per second (lambda)
    pub lambda: f64,
    /// Maximum time-to-live in seconds before forced expiry
    pub max_ttl_secs: u64,
}

impl DemurrageRate {
    /// Calculate remaining value after `elapsed_secs` of decay.
    ///
    /// Uses `V_t = V_0 * e^(-lambda * t)`. Returns zero if elapsed exceeds max TTL.
    pub fn calculate_remaining(&self, initial: GoldGrams, elapsed_secs: u64) -> GoldGrams {
        if elapsed_secs >= self.max_ttl_secs {
            return GoldGrams::zero();
        }
        let factor = (-self.lambda * elapsed_secs as f64).exp();
        let factor_dec = match Decimal::from_f64(factor) {
            Some(d) => d,
            None => return GoldGrams::zero(),
        };
        GoldGrams(initial.0 * factor_dec)
    }
}

