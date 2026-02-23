//! Vendored from caesar::models (operator preferences only, for routing)

use crate::core_types::GoldGrams;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Node operator soft preferences (whitepaper section 8.4).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorPreferences {
    pub tier_weights: TierWeights,
    pub preferred_min_packet: GoldGrams,
    pub preferred_max_packet: GoldGrams,
    pub auto_mode: bool,
}

impl Default for OperatorPreferences {
    fn default() -> Self {
        Self {
            tier_weights: TierWeights::default(),
            preferred_min_packet: GoldGrams::zero(),
            preferred_max_packet: GoldGrams::from_decimal(Decimal::MAX),
            auto_mode: true,
        }
    }
}

/// Soft preference weights for each market tier.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TierWeights {
    pub l0: Decimal,
    pub l1: Decimal,
    pub l2: Decimal,
    pub l3: Decimal,
}

impl Default for TierWeights {
    fn default() -> Self {
        Self {
            l0: Decimal::ONE,
            l1: Decimal::ONE,
            l2: Decimal::ONE,
            l3: Decimal::ONE,
        }
    }
}
