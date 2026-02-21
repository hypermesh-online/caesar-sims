# Core Caesar Refactor: From Token-Legacy to Ephemeral-Flow

This document outlines the required refactor for the `../core/caesar` crate to align the codebase with the high-velocity, ephemeral value-packet model ("The Truth") verified in the Arena Simulation.

## 1. Eliminate Token-Store Architecture
**Deviation:** The current core treats CAES as a standard ERC-20 style token with a fixed supply and persistent wallet balances.
**Refactor:**
- **Remove:** `EconomicsConfig::total_supply`, `EconomicsConfig::initial_distribution`.
- **Implement:** `CaesPacket` as the primary data structure. Value must exist only in-flight within a `PendingTransaction` or `TransitBuffer`.
- **Change:** `WalletBalance` from a persistent database entry to a "Gateway Liquid Inventory" snapshot.

## 2. Deprecate Staking and Yield
**Deviation:** The core contains a `StakingManager` and `APY` calculations, which encourage holding (velocity = 0).
**Refactor:**
- **Remove:** `src/staking.rs`, `src/staking_manager.rs`, and all associated APY logic.
- **Implement:** **Demurrage Engine**. Replace yield with a decay function: `V_t = V_0 * e^(-λt)`.
- **Rationale:** Holding CAES must be mathematically impossible without loss, ensuring it functions as a "Carrier Wave" rather than a "Store of Value."

## 3. Implement the "Governor" (PID Controller)
**Deviation:** Current exchange logic uses a static `csr_usd_rate`.
**Refactor:**
- **Implement:** `GovernorEngine`. This must ingest the XAU/USD (Gold) price and calculate the **Peg Deviation**.
- **Logic:**
    - If Price > 1.2G: Lower Minting Fees, Increase Demurrage, Add PoW Difficulty.
    - If Price < 0.8G: Pause Demurrage, Increase Exit Fees (Surge Pricing), Lower PoW Difficulty.
- **Integration:** The Governor must modulate the `TransitReward` and `EgressReward` in real-time.

## 4. Transition to Thermodynamic Consistency (0% Inflation)
**Deviation:** `RewardCalculator` uses a `base_rate_per_hour` (Inflationary printing).
**Refactor:**
- **Change:** Reward source must be the **Transaction Fee + Demurrage Burn**.
- **Formula:** `Input_Value = Output_Value + Egress_Reward + Transit_Reward`.
- **Action:** Remove any logic that "mints" rewards from thin air. Rewards are a split of the user's input value.

## 5. Upgrade NGauge Integration (The Throttle)
**Deviation:** NGauge is currently treated as a separate analytics module.
**Refactor:**
- **Implement:** `MetricsFeedbackLoop`. The Governor must query NGauge for "Work Done" (Bytes served / Compute cycles).
- **Logic:** High NGauge activity signals "Organic Velocity" $	o$ Governor relaxes rate limits. Low NGauge activity + High Caesar velocity signals "Speculation" $	o$ Governor tightens limits.

## 6. Universal Exchange Interface (UEI) Expansion
**Deviation:** Banking bridge logic is currently mocked or limited to basic providers.
**Refactor:**
- **Implement:** Stateless `Egress` and `Ingress` traits.
- **Action:** Every node becomes a "Sovereign Bank" that can mint CAES upon fiat/crypto deposit confirmation and burn it upon delivery. The "Bank" is the node's local inventory, not a global pool.
