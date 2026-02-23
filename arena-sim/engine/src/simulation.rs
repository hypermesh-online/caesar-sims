// Copyright 2026 Hypermesh Foundation. All rights reserved.
// Caesar Protocol Simulation Suite ("The Arena") - Simulation Core

use std::collections::HashMap;
use wasm_bindgen::prelude::*;

use crate::conservation;
use crate::dissolution;
use crate::engauge;
use crate::routing;
use crate::types::*;

// ─── ArenaSimulation struct ──────────────────────────────────────────────────

#[wasm_bindgen]
pub struct ArenaSimulation {
    pub(crate) nodes: Vec<SimNode>,
    pub(crate) packets: Vec<SimPacket>,
    pub(crate) message_queue: Vec<SimPacket>,
    pub(crate) state: WorldState,
    pub(crate) node_buffers: HashMap<u32, Vec<SimPacket>>,

    pub(crate) total_input: f64,
    pub(crate) total_output: f64,
    pub(crate) total_burned: f64,
    pub(crate) total_fees: f64,
    pub(crate) total_rewards_egress: f64,
    pub(crate) total_rewards_transit: f64,

    pub(crate) packet_id_counter: u64,
    pub(crate) max_active_packets: usize,
    pub(crate) last_gold_price: f64,

    pub(crate) settlement_count: u32,
    pub(crate) revert_count: u32,
    pub(crate) total_settlement_hops: u64,
    pub(crate) total_settlement_time: u64,

    // E11: Rolling volatility window
    pub(crate) gold_price_history: Vec<f64>,

    // Lambda EMA for surge smoothing (10-tick effective window)
    pub(crate) lambda_ema: f64,

    // v0.2: Conservation circuit breaker and NGauge rolling window
    pub(crate) conservation_law: conservation::ConservationLaw,
    pub(crate) engauge_state: engauge::NGaugeState,

    // Core governor PID (Decimal-based, vendored from caesar-sim-core)
    pub(crate) core_pid: crate::core_governor::pid::GovernorPid,

    // Core conservation law (Decimal-based, vendored from caesar-sim-core)
    pub(crate) core_conservation: crate::core_conservation::ConservationLaw,
}

// ─── Internal Logic (Testable, pure Rust) ────────────────────────────────────

impl ArenaSimulation {
    pub fn tick_core(&mut self) -> TickResult {
        self.state.current_tick += 1;
        let current_tick = self.state.current_tick;

        // E11: Update gold price history (rolling window of 20)
        self.gold_price_history.push(self.state.gold_price);
        if self.gold_price_history.len() > 20 {
            self.gold_price_history.remove(0);
        }

        // S1: Deliver in-transit packets from message queue
        self.deliver_message_queue(current_tick);

        // E11: Proper volatility via rolling window (coefficient of variation)
        let volatility = compute_rolling_volatility(&self.gold_price_history);
        self.state.volatility = volatility;
        self.last_gold_price = self.state.gold_price;

        // Calculate Liquidity Coefficient (Lambda)
        let total_egress_capacity: f64 = self.nodes.iter()
            .filter(|n| n.role == NodeRole::Egress)
            .map(|n| n.inventory_crypto)
            .sum();
        let total_in_flight: f64 = self.node_buffers.values().flatten()
            .map(|p| p.current_value)
            .sum::<f64>()
            + self.message_queue.iter().map(|p| p.current_value).sum::<f64>()
            + 0.1;
        let raw_lambda = total_egress_capacity / total_in_flight;
        // Exponential moving average — 10-tick effective window
        self.lambda_ema = self.lambda_ema * 0.9 + raw_lambda * 0.1;
        let lambda = self.lambda_ema;

        // E8: Surge only in Bottleneck quadrant: sustained low lambda AND market stress
        let surge_multiplier = if lambda < 0.5
            && (self.state.panic_level > 0.1 || self.state.demand_factor > 0.5)
        {
            (1.0 / lambda).min(3.0)
        } else {
            1.0
        };
        self.state.surge_multiplier = surge_multiplier;

        // Simulate NGauge Activity
        self.state.ngauge_activity_index =
            engauge::update_ngauge_activity(&mut self.nodes, self.state.demand_factor);

        // 1. The Caesar Governor Logic (core PID, Decimal-based)
        let core_metrics = crate::adapter::world_to_metrics(&self.state, volatility, lambda);
        let core_params = self.core_pid.recalculate(&core_metrics);

        // Convert core GovernanceParams back to Arena GovernorOutput
        let fee_rate = crate::adapter::params_to_fee_rate(&core_params);
        let quadrant = match core_params.pressure {
            crate::core_governor::params::PressureQuadrant::GoldenEra => "D: GOLDEN ERA",
            crate::core_governor::params::PressureQuadrant::Bubble => "A: BUBBLE",
            crate::core_governor::params::PressureQuadrant::Crash => "B: CRASH",
            crate::core_governor::params::PressureQuadrant::Stagnation => "C: STAGNATION",
            crate::core_governor::params::PressureQuadrant::Bottleneck => "E: BOTTLENECK",
            crate::core_governor::params::PressureQuadrant::Vacuum => "F: VACUUM",
        };
        let status = match core_params.pressure {
            crate::core_governor::params::PressureQuadrant::GoldenEra => "STABLE",
            crate::core_governor::params::PressureQuadrant::Bubble => "OVER-PEG: VENTING",
            crate::core_governor::params::PressureQuadrant::Crash => "UNDER-PEG: EMERGENCY BRAKE",
            crate::core_governor::params::PressureQuadrant::Stagnation => "UNDER-PEG: STIMULUS",
            crate::core_governor::params::PressureQuadrant::Bottleneck => "CONGESTED: THROTTLING",
            crate::core_governor::params::PressureQuadrant::Vacuum => "LOW ACTIVITY: INCENTIVIZING",
        };
        // Health score normalized to 0..1 for complexity mapping
        let health = crate::adapter::from_decimal(core_params.health_score) / 10.0;
        let verification_complexity = (1.0 + (1.0 - health) * 4.0).round() as u64;

        let mut gov = GovernorOutput {
            fee_rate,
            demurrage: match core_params.pressure {
                crate::core_governor::params::PressureQuadrant::Bubble => 0.10,
                crate::core_governor::params::PressureQuadrant::Crash => 0.0,
                crate::core_governor::params::PressureQuadrant::Stagnation => 0.001,
                crate::core_governor::params::PressureQuadrant::Bottleneck => 0.005 * 1.5,
                crate::core_governor::params::PressureQuadrant::Vacuum => 0.005 * 0.5,
                crate::core_governor::params::PressureQuadrant::GoldenEra => 0.005,
            },
            quadrant: quadrant.to_string(),
            status: status.to_string(),
            verification_complexity,
        };

        // Legacy overrides (preserve simulation behavior)
        if self.state.panic_level > 0.7 {
            gov.fee_rate = gov.fee_rate.max(0.05);
            gov.demurrage *= 0.5;
        }
        if self.state.ngauge_activity_index > 0.5 {
            gov.fee_rate *= 0.8;
        }
        let organic_ratio = if self.state.network_velocity > 100.0 {
            self.state.ngauge_activity_index / (self.state.network_velocity / 1000.0).max(0.1)
        } else {
            1.0
        };
        if organic_ratio < 0.3 {
            gov.fee_rate *= 1.5;
        }
        if lambda < 0.5 {
            gov.fee_rate *= surge_multiplier;
        }

        // E7: Organic ratio (also computed inside governor, but store separately)
        self.state.organic_ratio =
            engauge::compute_organic_ratio(
                self.state.ngauge_activity_index,
                self.state.network_velocity,
            );

        // Feed rolling window tracker
        self.engauge_state.update(
            self.state.ngauge_activity_index,
            self.state.network_velocity,
        );
        // Use rolling window organic ratio, clamped to single-tick value
        self.state.organic_ratio = self.engauge_state.organic_ratio().min(
            engauge::compute_organic_ratio(
                self.state.ngauge_activity_index,
                self.state.network_velocity,
            ),
        );

        self.state.governance_quadrant = gov.quadrant.clone();
        self.state.governance_status = gov.status.clone();
        self.state.current_demurrage_rate = gov.demurrage;
        self.state.current_fee_rate = gov.fee_rate;
        // Compute per-tier effective fee rates from core fee modifiers
        {
            let caps = [0.05_f64, 0.02, 0.005, 0.001];
            let mods = [
                crate::adapter::from_decimal(core_params.fee_modifiers.l0),
                crate::adapter::from_decimal(core_params.fee_modifiers.l1),
                crate::adapter::from_decimal(core_params.fee_modifiers.l2),
                crate::adapter::from_decimal(core_params.fee_modifiers.l3),
            ];
            self.state.tier_fee_rates = [
                (gov.fee_rate * mods[0]).min(caps[0]).max(0.0),
                (gov.fee_rate * mods[1]).min(caps[1]).max(0.0),
                (gov.fee_rate * mods[2]).min(caps[2]).max(0.0),
                (gov.fee_rate * mods[3]).min(caps[3]).max(0.0),
            ];
        }
        // Recompute peg_deviation same way governor does internally
        let effective_rate = self.state.gold_price * (1.0 - gov.fee_rate);
        let peg_deviation = (effective_rate - self.state.gold_price) / self.state.gold_price;
        self.state.peg_deviation = peg_deviation - (self.state.panic_level * 0.15);
        self.state.verification_complexity = gov.verification_complexity;

        let demurrage = gov.demurrage;

        // S2: Auto Traffic Generation
        self.auto_spawn_traffic(current_tick);

        // 4. Node Execution Cycle (Sovereign Routing)
        let settled_count = self.execute_node_cycle(current_tick, demurrage);

        // E12: Compute per-node liquidity pressure
        self.compute_node_pressure();

        // 5. Finalize Stats
        self.finalize_stats(settled_count, current_tick)
    }

    /// Deliver in-transit packets whose arrival tick has been reached.
    fn deliver_message_queue(&mut self, current_tick: u64) {
        let mut delivered = Vec::new();
        let mut remaining = Vec::new();
        for p in self.message_queue.drain(..) {
            if p.arrival_tick <= current_tick {
                delivered.push(p);
            } else {
                remaining.push(p);
            }
        }
        self.message_queue = remaining;
        for mut p in delivered {
            if let Some(target) = p.target_node {
                p.status = PacketStatus::Minted;
                let target_role = self.nodes.get(target as usize).map(|n| n.role);
                if target_role == Some(NodeRole::Disabled) {
                    let reroute_to = self.nodes.get(target as usize)
                        .map(|n| n.neighbors.clone())
                        .unwrap_or_default()
                        .into_iter()
                        .find(|&n| self.nodes[n as usize].role != NodeRole::Disabled);
                    if let Some(dest) = reroute_to {
                        p.target_node = Some(dest);
                        self.nodes[dest as usize].current_buffer_count += 1;
                        self.node_buffers.entry(dest).or_default().push(p);
                    } else {
                        p.status = PacketStatus::Held;
                        p.orbit_start_tick = Some(current_tick);
                        let origin = p.origin_node;
                        self.node_buffers.entry(origin).or_default().push(p);
                    }
                } else if target_role.is_some() {
                    self.nodes[target as usize].current_buffer_count += 1;
                    self.node_buffers.entry(target).or_default().push(p);
                }
            }
        }
    }

    /// S2: Auto traffic generation based on demand and panic.
    fn auto_spawn_traffic(&mut self, current_tick: u64) {
        let spawn_rate = self.state.demand_factor * 5.0
            * if self.state.panic_level > 0.5 { 1.0 + self.state.panic_level } else { 1.0 };
        let packets_to_spawn = spawn_rate as u32;
        let ingress_nodes: Vec<u32> = self.nodes.iter()
            .filter(|n| n.role == NodeRole::Ingress)
            .map(|n| n.id)
            .collect();
        if !ingress_nodes.is_empty() {
            let tier_base = self.packet_id_counter;
            for i in 0..packets_to_spawn {
                let node_idx = (current_tick as usize + i as usize) % ingress_nodes.len();
                let node_id = ingress_nodes[node_idx];
                // Generate diverse tier traffic
                let tier_selector = (tier_base + i as u64) % 4;
                let amount = match tier_selector {
                    0 => 1.0 + ((current_tick + i as u64) % 9) as f64,           // L0: 1-9g
                    1 => 50.0 + ((current_tick + i as u64) % 950) as f64,        // L1: 50-999g
                    2 => 1000.0 + ((current_tick + i as u64) % 99000) as f64,    // L2: 1000-99999g
                    _ => 100000.0 + ((current_tick + i as u64) % 900000) as f64, // L3: 100000-999999g
                };

                // E4: Demand destruction
                if self.state.current_fee_rate > 0.10 {
                    let cancel_prob =
                        ((self.state.current_fee_rate - 0.10) * 5.0).min(1.0);
                    let check = ((self.packet_id_counter * 7 + i as u64) % 100)
                        as f64 / 100.0;
                    if check < cancel_prob {
                        continue;
                    }
                }

                self.packet_id_counter += 1;
                let tier = MarketTier::from_value(amount);
                let ttl = current_tick + tier.ttl_ticks();
                let hop_limit = tier.hop_limit();
                let fee_budget = tier.fee_cap() * amount;
                let packet = SimPacket {
                    id: self.packet_id_counter,
                    original_value: amount,
                    current_value: amount,
                    arrival_tick: current_tick,
                    status: PacketStatus::Minted,
                    origin_node: node_id,
                    target_node: None,
                    hops: 0,
                    route_history: vec![node_id],
                    orbit_start_tick: None,
                    tier,
                    ttl,
                    hop_limit,
                    fee_budget,
                    fees_consumed: 0.0,
                    fee_schedule: Vec::new(),
                    spawn_tick: current_tick,
                };
                self.node_buffers.entry(node_id).or_default().push(packet);
                self.nodes[node_id as usize].current_buffer_count += 1;
                self.total_input += amount;
                self.state.spawn_count += 1;
            }
        }
    }

    /// Process all node buffers: demurrage, orbit timeout, settlement, routing.
    /// Returns the number of settled packets this tick.
    fn execute_node_cycle(
        &mut self,
        current_tick: u64,
        _demurrage: f64,
    ) -> u32 {
        let mut settled_count: u32 = 0;
        let mut _reverted_count: u32 = 0;
        let node_indices: Vec<u32> = self.node_buffers.keys().cloned().collect();
        let current_volatility = self.state.volatility;

        for node_id in node_indices {
            let node_role = self.nodes[node_id as usize].role;
            let node_strategy = self.nodes[node_id as usize].strategy;
            if node_role == NodeRole::Disabled {
                continue;
            }

            let buf = match self.node_buffers.get_mut(&node_id) {
                Some(b) => b,
                None => continue,
            };
            let mut j = 0;
            while j < buf.len() {
                let mut p = buf.remove(j);

                // E1: Per-tier exponential demurrage V_t = V_0 * e^(-lambda * dt)
                let lambda = p.tier.demurrage_lambda();
                let old_v = p.current_value;
                p.current_value *= (-lambda).exp(); // dt=1 tick
                self.total_burned += old_v - p.current_value;

                // E8: Surge pricing per packet (escalating cost for orbiting >10 ticks)
                if let Some(orbit_start) = p.orbit_start_tick {
                    let orbit_ticks = current_tick.saturating_sub(orbit_start);
                    if orbit_ticks > 10 {
                        let surge_burn = p.current_value
                            * ((orbit_ticks - 10) as f64 * 0.01).min(0.5);
                        p.current_value -= surge_burn;
                        self.total_burned += surge_burn;
                    }
                }

                // TTL expiry check - uses per-tier TTL set at minting
                if p.ttl > 0 && current_tick >= p.ttl {
                    p.status = PacketStatus::Expired;
                    self.total_output += p.current_value;
                    _reverted_count += 1;
                    self.revert_count += 1;
                    self.nodes[node_id as usize].current_buffer_count =
                        self.nodes[node_id as usize].current_buffer_count
                            .saturating_sub(1);
                    continue;
                }

                // Gravity dissolution for packets exceeding total age threshold.
                // Checked BEFORE orbit timeout — dissolution takes priority.
                if p.status == PacketStatus::Held {
                    let total_age = current_tick.saturating_sub(p.spawn_tick);
                    if dissolution::is_eligible_ticks(total_age) && p.current_value > 0.0 {
                        let qualifications: Vec<dissolution::GravityQualification> =
                            self.nodes.iter()
                                .filter(|n| n.role != NodeRole::Disabled)
                                .map(|n| dissolution::GravityQualification {
                                    node_id: n.id,
                                    upi_active: n.upi_active,
                                    engauge_active: n.ngauge_running,
                                    kyc_attested: n.kyc_valid,
                                    caesar_active: n.role != NodeRole::Disabled,
                                    demonstrable_capacity: n.bandwidth >= 10.0,
                                    active_routing_current_epoch:
                                        n.current_buffer_count > 0
                                        || n.total_fees_earned > 0.0,
                                })
                                .collect();
                        let shard_holders: Vec<u32> = p.route_history.clone();
                        if let Ok(result) = dissolution::dissolve(
                            p.current_value,
                            &qualifications,
                            &shard_holders,
                        ) {
                            for dist in &result.distributions {
                                if let Some(node) =
                                    self.nodes.get_mut(dist.node_id as usize)
                                {
                                    node.inventory_fiat += dist.amount;
                                }
                            }
                            p.status = PacketStatus::Dissolved;
                            self.total_output += p.current_value;
                            self.state.dissolved_count += 1;
                            self.nodes[node_id as usize].current_buffer_count =
                                self.nodes[node_id as usize].current_buffer_count
                                    .saturating_sub(1);
                            continue;
                        }
                    }
                }

                // E5: Orbit timeout for Held packets (separate from TTL)
                if p.status == PacketStatus::Held {
                    if p.orbit_start_tick.is_none() {
                        p.orbit_start_tick = Some(current_tick);
                    }
                    let orbit_ticks = current_tick - p.orbit_start_tick.unwrap();
                    // L3 packets can orbit past dissolution threshold (5000 ticks)
                    // Other tiers use TTL/2 as orbit limit
                    let orbit_limit = if p.tier == MarketTier::L3 {
                        dissolution::DISSOLUTION_TIMEOUT_TICKS + 500 // 5500: beyond dissolution
                    } else {
                        p.tier.ttl_ticks() / 2
                    };
                    if orbit_ticks > orbit_limit {
                        p.status = PacketStatus::Refunded;
                        self.total_output += p.current_value;
                        _reverted_count += 1;
                        self.revert_count += 1;
                        self.nodes[node_id as usize].current_buffer_count =
                            self.nodes[node_id as usize].current_buffer_count
                                .saturating_sub(1);
                        continue;
                    }
                }

                // E9: RiskAverse strategy - buffer packets during high volatility
                if node_strategy == NodeStrategy::RiskAverse
                    && current_volatility > 0.1
                    && node_role != NodeRole::Egress
                {
                    buf.insert(j, p);
                    j += 1;
                    continue;
                }

                // Egress settlement (inlined to avoid borrow conflict with buf)
                if node_role == NodeRole::Egress && p.current_value > 0.0 {
                    if self.nodes[node_id as usize].inventory_crypto >= p.current_value {
                        // S5 + E3: 80/20 reward split with velocity bonus
                        let total_fee = crate::adapter::calculate_fee_via_core(
                            &self.core_pid,
                            &p.tier,
                            self.state.current_fee_rate,
                            p.original_value,
                        ).min(p.current_value);
                        p.route_history.push(node_id);

                        let velocity_bonus = if p.hops <= 3 { 1.2 }
                            else if p.hops <= 6 { 1.0 }
                            else { 0.8 };

                        // E9: Greedy fee modifier
                        let strategy_fee_mod = match node_strategy {
                            NodeStrategy::Greedy => 1.5,
                            _ => 1.0,
                        };
                        let adjusted_fee = total_fee * strategy_fee_mod;
                        // Cost certainty: cap settlement fee to remaining budget
                        let remaining_budget = (p.fee_budget - p.fees_consumed).max(0.0);
                        let capped_fee = adjusted_fee.min(p.current_value).min(remaining_budget);
                        p.fees_consumed += capped_fee;

                        // Fee distribution via core's Decimal-based 80/20 splitter
                        let transit_node_ids: Vec<u32> = p.route_history.iter()
                            .filter(|&&n| {
                                n != node_id
                                    && self.nodes.get(n as usize)
                                        .map(|node| node.role != NodeRole::Ingress)
                                        .unwrap_or(false)
                            })
                            .copied()
                            .collect();
                        let (core_egress_amt, core_per_transit) =
                            crate::adapter::distribute_fee_via_core(
                                capped_fee, node_id, &transit_node_ids,
                            );

                        // Apply velocity_bonus as arena-specific overlay
                        let egress_reward = core_egress_amt * velocity_bonus;
                        self.nodes[node_id as usize].total_fees_earned += egress_reward;
                        self.total_rewards_egress += core_egress_amt;

                        // Transit distribution
                        if !transit_node_ids.is_empty() {
                            let per_transit = core_per_transit * velocity_bonus;
                            for &tn in &transit_node_ids {
                                if let Some(node) = self.nodes.get_mut(tn as usize) {
                                    node.total_fees_earned += per_transit;
                                }
                            }
                        }
                        self.total_rewards_transit += capped_fee - core_egress_amt;

                        let settlement_val = (p.current_value - capped_fee).max(0.0);
                        self.nodes[node_id as usize].inventory_crypto -= p.current_value;
                        self.total_output += settlement_val;
                        self.total_fees += capped_fee;
                        self.settlement_count += 1;
                        self.total_settlement_hops += p.hops as u64;
                        self.total_settlement_time +=
                            current_tick.saturating_sub(p.arrival_tick);
                        self.nodes[node_id as usize].current_buffer_count =
                            self.nodes[node_id as usize].current_buffer_count
                                .saturating_sub(1);

                        // Conservation verify at settlement
                        // fees_consumed already includes capped_fee (added at line 428)
                        let demurrage_burned =
                            p.original_value - p.current_value - p.fees_consumed;
                        self.conservation_law.verify_settlement(
                            p.original_value,
                            settlement_val,
                            p.fees_consumed,
                            demurrage_burned.max(0.0),
                        );

                        // Core conservation cross-check (Decimal-based, parallel validation)
                        let _core_conservation_result = crate::adapter::verify_settlement_via_core(
                            &mut self.core_conservation,
                            p.original_value,
                            settlement_val,
                            p.fees_consumed,
                            demurrage_burned.max(0.0),
                        );

                        settled_count += 1;
                        continue;
                    }
                }

                // Force orbit if packet has bounced too many times (hop limit)
                if p.hops > p.hop_limit {
                    p.status = PacketStatus::Held;
                    if p.orbit_start_tick.is_none() {
                        p.orbit_start_tick = Some(current_tick);
                    }
                    buf.insert(j, p);
                    j += 1;
                    continue;
                }

                // Routing: find path to Egress (skip Disabled nodes)
                let next_hop = routing::find_next_hop(&self.nodes, node_id, &p);

                if let Some(target) = next_hop {
                    // Charge transit fee for this hop
                    let transit_fee =
                        self.nodes[target as usize].transit_fee * p.current_value;
                    let remaining_budget = (p.fee_budget - p.fees_consumed).max(0.0);
                    let capped_transit_fee = transit_fee
                        .min(p.current_value * p.tier.fee_cap())
                        .min(remaining_budget);
                    p.current_value -= capped_transit_fee;
                    p.fees_consumed += capped_transit_fee;
                    p.fee_schedule.push(capped_transit_fee);
                    self.total_fees += capped_transit_fee;
                    self.nodes[target as usize].total_fees_earned += capped_transit_fee;

                    p.status = PacketStatus::InTransit;
                    p.target_node = Some(target);
                    p.hops += 1;
                    p.route_history.push(node_id);
                    p.orbit_start_tick = None;

                    // E10: Variable latency based on distance
                    let distance = (
                        (self.nodes[node_id as usize].x
                            - self.nodes[target as usize].x).powi(2)
                        + (self.nodes[node_id as usize].y
                            - self.nodes[target as usize].y).powi(2)
                    ).sqrt();
                    let base_latency = 1 + (distance as u64);
                    p.arrival_tick =
                        current_tick + base_latency + self.state.verification_complexity;

                    self.message_queue.push(p);
                    self.nodes[node_id as usize].current_buffer_count =
                        self.nodes[node_id as usize].current_buffer_count
                            .saturating_sub(1);
                } else {
                    p.status = PacketStatus::Held;
                    if p.orbit_start_tick.is_none() {
                        p.orbit_start_tick = Some(current_tick);
                    }
                    buf.insert(j, p);
                    j += 1;
                }
            }
        }

        settled_count
    }

    /// E12: Compute per-node liquidity pressure.
    fn compute_node_pressure(&mut self) {
        for node in self.nodes.iter_mut() {
            if node.role == NodeRole::Disabled {
                node.pressure = 0.0;
                continue;
            }
            match node.role {
                NodeRole::Egress => {
                    node.pressure = node.inventory_crypto
                        / (node.current_buffer_count as f64 * 100.0 + 1.0);
                }
                NodeRole::Ingress => {
                    node.pressure = node.current_buffer_count as f64 / 10.0;
                }
                _ => {
                    node.pressure = node.current_buffer_count as f64 / 10.0;
                }
            }
        }
    }

    /// Finalize tick statistics and build the TickResult.
    fn finalize_stats(&mut self, settled_count: u32, _current_tick: u64) -> TickResult {
        self.state.network_velocity = settled_count as f64 * 100.0;
        self.state.total_rewards_egress = self.total_rewards_egress;
        self.state.total_rewards_transit = self.total_rewards_transit;
        self.state.total_fees_collected = self.total_fees;
        self.state.total_demurrage_burned = self.total_burned;
        self.state.settlement_count = self.settlement_count;
        self.state.revert_count = self.revert_count;
        self.state.total_input = self.total_input;
        self.state.total_output = self.total_output;

        let active_val: f64 = self.node_buffers.values().flatten()
            .map(|p| p.current_value).sum::<f64>()
            + self.message_queue.iter().map(|p| p.current_value).sum::<f64>();
        self.state.active_value = active_val;
        self.state.total_value_leaked = conservation::compute_conservation(
            self.total_input,
            self.total_output,
            self.total_burned,
            self.total_fees,
            active_val,
        );

        // Circuit breaker check
        let conservation_result = self.conservation_law.verify_tick(
            self.total_input,
            self.total_output,
            self.total_fees,
            self.total_burned,
            active_val,
        );
        self.state.circuit_breaker_active = conservation_result.circuit_breaker_tripped;

        // Count orbiting packets
        let orbit_count: u32 = self.node_buffers.values().flatten()
            .filter(|p| p.status == PacketStatus::Held)
            .count() as u32;
        self.state.orbit_count = orbit_count;

        // Track tier distribution
        let mut tier_dist = [0u32; 4];
        for p in self.node_buffers.values().flatten()
            .chain(self.message_queue.iter())
        {
            match p.tier {
                MarketTier::L0 => tier_dist[0] += 1,
                MarketTier::L1 => tier_dist[1] += 1,
                MarketTier::L2 => tier_dist[2] += 1,
                MarketTier::L3 => tier_dist[3] += 1,
            }
        }
        self.state.tier_distribution = tier_dist;

        // Count held packets
        self.state.held_count = self.node_buffers.values().flatten()
            .filter(|p| p.status == PacketStatus::Held)
            .count() as u32;

        // Effective price composite (properly scaled)
        let total_active_count = self.node_buffers.values().flatten().count() as f64
            + self.message_queue.len() as f64;
        // Network fee component: average fee per active packet as fraction of gold price
        self.state.network_fee_component = if total_active_count > 0.0 && self.state.gold_price > 0.0 {
            (self.total_fees / total_active_count) / self.state.gold_price
        } else {
            0.0
        };
        // Speculation component: ingress/egress flow imbalance (capped)
        let ingress_flow: f64 = self.nodes.iter()
            .filter(|n| n.role == NodeRole::Ingress)
            .map(|n| n.current_buffer_count as f64)
            .sum();
        let egress_flow: f64 = self.nodes.iter()
            .filter(|n| n.role == NodeRole::Egress)
            .map(|n| n.current_buffer_count as f64)
            .sum();
        self.state.speculation_component = if egress_flow > 0.0 {
            (((ingress_flow / egress_flow.max(1.0)) - 1.0).max(0.0) * 0.001).min(0.05)
        } else {
            0.0
        };
        // Float component: in-flight value as fraction of total input (capped)
        self.state.float_component = if self.total_input > 0.0 {
            (active_val / self.total_input * 0.001).min(0.05)
        } else {
            0.0
        };
        self.state.effective_price_composite = self.state.gold_price
            * (1.0 + self.state.network_fee_component
                + self.state.speculation_component
                + self.state.float_component);

        let mut active_packets = self.message_queue.clone();
        for b in self.node_buffers.values() {
            active_packets.extend(b.clone());
        }

        TickResult {
            state: self.state.clone(),
            active_packets,
            node_updates: self.nodes.iter().map(|n| NodeUpdate {
                id: n.id,
                buffer_count: n.current_buffer_count,
                inventory_fiat: n.inventory_fiat,
                inventory_crypto: n.inventory_crypto,
            }).collect(),
        }
    }

    pub fn get_total_output(&self) -> f64 { self.total_output }
    pub fn get_total_value_leaked(&self) -> f64 { self.state.total_value_leaked }
    pub fn get_node_pressure(&self, node_id: usize) -> f64 {
        self.nodes.get(node_id).map_or(0.0, |n| n.pressure)
    }
}

// ─── Rolling Volatility ──────────────────────────────────────────────────────

/// E11: Compute coefficient of variation from rolling price window
pub(crate) fn compute_rolling_volatility(history: &[f64]) -> f64 {
    if history.len() < 2 {
        return 0.0;
    }
    let n = history.len() as f64;
    let mean = history.iter().sum::<f64>() / n;
    if mean.abs() < 1e-12 {
        return 0.0;
    }
    let variance = history.iter()
        .map(|&p| (p - mean).powi(2))
        .sum::<f64>() / n;
    let std_dev = variance.sqrt();
    std_dev / mean
}
