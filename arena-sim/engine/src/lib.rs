// Copyright 2026 Hypermesh Foundation. All rights reserved.
// Caesar Protocol Simulation Suite ("The Arena")

use serde::{Serialize, Deserialize};
use wasm_bindgen::prelude::*;
use std::collections::HashMap;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum NodeRole { Ingress = 0, Egress = 1, Transit = 2, NGauge = 3, Disabled = 4 }

// E9: Node personality strategies
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum NodeStrategy { RiskAverse = 0, Greedy = 1, Passive = 2 }

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
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum PacketStatus { Active = 0, Orbiting = 1, Settled = 2, Reverted = 3, InTransit = 4 }

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
    pub trust_score: f64,
    pub total_fees_earned: f64,
    pub accumulated_work: f64,
    // E9: Node strategy
    #[serde(default = "default_strategy")]
    pub strategy: NodeStrategy,
    // E12: Per-node liquidity pressure
    #[serde(default)]
    pub pressure: f64,
}

fn default_strategy() -> NodeStrategy { NodeStrategy::Passive }

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

    // E6: Average trust score
    #[serde(default)]
    pub avg_trust_score: f64,
    // E7: Organic ratio
    #[serde(default)]
    pub organic_ratio: f64,
    // E8: Surge multiplier
    #[serde(default)]
    pub surge_multiplier: f64,
}

#[derive(Debug, Serialize)]
pub struct TickResult {
    pub state: WorldState,
    pub active_packets: Vec<SimPacket>,
    pub node_updates: Vec<NodeUpdate>,
}

#[derive(Debug, Serialize)]
pub struct NodeUpdate {
    pub id: u32,
    pub buffer_count: u32,
    pub inventory_fiat: f64,
    pub inventory_crypto: f64,
}

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

#[wasm_bindgen]
pub struct ArenaSimulation {
    nodes: Vec<SimNode>,
    packets: Vec<SimPacket>,
    message_queue: Vec<SimPacket>,
    state: WorldState,
    node_buffers: HashMap<u32, Vec<SimPacket>>,

    total_input: f64,
    total_output: f64,
    total_burned: f64,
    total_fees: f64,
    total_rewards_egress: f64,
    total_rewards_transit: f64,

    packet_id_counter: u64,
    max_active_packets: usize,
    last_gold_price: f64,

    settlement_count: u32,
    revert_count: u32,
    total_settlement_hops: u64,
    total_settlement_time: u64,

    // E11: Rolling volatility window
    gold_price_history: Vec<f64>,
}

// Internal Logic (Testable, pure Rust)
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
                p.status = PacketStatus::Active;
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
                        p.status = PacketStatus::Orbiting;
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

        // E11: Proper volatility via rolling window (coefficient of variation)
        let volatility = compute_rolling_volatility(&self.gold_price_history);
        self.state.volatility = volatility;
        self.last_gold_price = self.state.gold_price;

        // E6: Trust decay toward 0.5 baseline each tick
        for node in self.nodes.iter_mut() {
            if node.role != NodeRole::Disabled {
                node.trust_score += (0.5 - node.trust_score) * 0.001;
            }
        }

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
        let lambda = total_egress_capacity / total_in_flight;

        // E8: Compute surge multiplier from lambda
        let surge_multiplier = if lambda < 0.5 {
            (1.0 / lambda).min(10.0)
        } else {
            1.0
        };
        self.state.surge_multiplier = surge_multiplier;

        // Simulate NGauge Activity
        let mut total_work = 0.0;
        for node in self.nodes.iter_mut() {
            if node.role == NodeRole::NGauge {
                node.accumulated_work += (self.state.demand_factor * 10.0).max(1.0);
                total_work += node.accumulated_work;
            }
        }
        self.state.ngauge_activity_index =
            (total_work / (self.nodes.len() as f64 * 100.0)).min(1.0);

        // 1. The Caesar Governor Logic
        let mut demurrage = 0.005;
        let base_fee = 0.001;
        let sigma = 1.0 + volatility;
        let safe_lambda = lambda.max(0.1);
        let mut fee_rate = base_fee * (sigma / (safe_lambda * safe_lambda));

        let mut verification_complexity: u64 = 1;
        let mut quadrant = "D: GOLDEN ERA";
        let mut status = "STABLE";

        let effective_rate = self.state.gold_price * (1.0 - fee_rate);
        let peg_deviation = (effective_rate - self.state.gold_price) / self.state.gold_price;
        let effective_deviation = peg_deviation - (self.state.panic_level * 0.15);

        if effective_deviation > 0.10 {
            quadrant = "A: BUBBLE";
            status = "OVER-PEG: VENTING";
            fee_rate = 0.0;
            demurrage = 0.10;
            verification_complexity = (2.0 + (effective_deviation * 20.0)) as u64;
        } else if effective_deviation < -0.10 {
            if self.state.network_velocity > 500.0 {
                quadrant = "B: CRASH";
                status = "UNDER-PEG: EMERGENCY BRAKE";
                demurrage = 0.0;
                fee_rate = fee_rate.max(0.05);
                verification_complexity = 1;
            } else {
                quadrant = "C: STAGNATION";
                status = "UNDER-PEG: STIMULUS";
                demurrage = 0.001;
                fee_rate = 0.0005;
            }
        }

        // S3: Panic level forces toward crisis behavior
        if self.state.panic_level > 0.7 {
            fee_rate = fee_rate.max(0.05);
            demurrage *= 0.5;
        }

        if self.state.ngauge_activity_index > 0.5 {
            fee_rate *= 0.8;
            verification_complexity = verification_complexity.saturating_sub(1);
        }

        // E7: Organic vs Speculative Detection
        let organic_ratio = if self.state.network_velocity > 100.0 {
            self.state.ngauge_activity_index
                / (self.state.network_velocity / 1000.0).max(0.1)
        } else {
            1.0 // Low velocity is always "organic"
        };
        self.state.organic_ratio = organic_ratio;

        if organic_ratio < 0.3 {
            // Speculative: high velocity but low real work
            fee_rate *= 1.5;
            verification_complexity += 2;
            if status == "STABLE" {
                status = "SPECULATION DETECTED";
            }
        }

        // E8: Surge pricing on fee rate during liquidity crunch
        if lambda < 0.5 {
            fee_rate *= surge_multiplier;
        }

        self.state.governance_quadrant = quadrant.to_string();
        self.state.governance_status = status.to_string();
        self.state.current_demurrage_rate = demurrage;
        self.state.current_fee_rate = fee_rate;
        self.state.peg_deviation = effective_deviation;
        self.state.verification_complexity = verification_complexity;

        // S2: Auto Traffic Generation
        let spawn_rate = self.state.demand_factor * 5.0
            * if self.state.panic_level > 0.5 { 1.0 + self.state.panic_level } else { 1.0 };
        let packets_to_spawn = spawn_rate as u32;
        let ingress_nodes: Vec<u32> = self.nodes.iter()
            .filter(|n| n.role == NodeRole::Ingress)
            .map(|n| n.id)
            .collect();
        if !ingress_nodes.is_empty() {
            for i in 0..packets_to_spawn {
                let node_idx = (current_tick as usize + i as usize) % ingress_nodes.len();
                let node_id = ingress_nodes[node_idx];
                let amount = 1000.0 + ((current_tick + i as u64) % 10) as f64 * 100.0;

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
                let packet = SimPacket {
                    id: self.packet_id_counter,
                    original_value: amount,
                    current_value: amount,
                    arrival_tick: current_tick,
                    status: PacketStatus::Active,
                    origin_node: node_id,
                    target_node: None,
                    hops: 0,
                    route_history: vec![node_id],
                    orbit_start_tick: None,
                };
                self.node_buffers.entry(node_id).or_default().push(packet);
                self.nodes[node_id as usize].current_buffer_count += 1;
                self.total_input += amount;
                self.state.spawn_count += 1;
            }
        }

        // 4. Node Execution Cycle (Sovereign Routing)
        let mut settled_count: u32 = 0;
        let mut _reverted_count: u32 = 0;
        let node_indices: Vec<u32> = self.node_buffers.keys().cloned().collect();

        // Snapshot node strategies and volatility for routing decisions
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

                // E1: Exponential demurrage
                let old_v = p.current_value;
                p.current_value *= (-demurrage).exp();
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

                // E5: Orbit timeout check
                if p.status == PacketStatus::Orbiting {
                    if p.orbit_start_tick.is_none() {
                        p.orbit_start_tick = Some(current_tick);
                    }
                    if current_tick - p.orbit_start_tick.unwrap() > 50 {
                        // REVERT: refund remaining value
                        p.status = PacketStatus::Reverted;
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
                    // Hold packet in buffer, don't route
                    buf.insert(j, p);
                    j += 1;
                    continue;
                }

                // Egress settlement
                if node_role == NodeRole::Egress && p.current_value > 0.0 {
                    if self.nodes[node_id as usize].inventory_crypto >= p.current_value {
                        // S5 + E3: 80/20 reward split with velocity bonus
                        let total_fee = (p.original_value * self.state.current_fee_rate)
                            .min(p.current_value);
                        p.route_history.push(node_id);

                        let velocity_bonus = if p.hops <= 3 { 1.2 }
                            else if p.hops <= 6 { 1.0 }
                            else { 0.8 };

                        // E9: Strategy-based trust gain
                        let trust_gain = match node_strategy {
                            NodeStrategy::RiskAverse => 0.02,
                            NodeStrategy::Greedy => 0.005,
                            NodeStrategy::Passive => 0.01,
                        };

                        // E9: Greedy fee modifier
                        let strategy_fee_mod = match node_strategy {
                            NodeStrategy::Greedy => 1.5,
                            _ => 1.0,
                        };
                        let adjusted_fee = total_fee * strategy_fee_mod;
                        let capped_fee = adjusted_fee.min(p.current_value);

                        // Egress gets 80%
                        let egress_reward = capped_fee * 0.8 * velocity_bonus;
                        self.nodes[node_id as usize].total_fees_earned += egress_reward;
                        // E6: Trust increment based on strategy
                        self.nodes[node_id as usize].trust_score =
                            (self.nodes[node_id as usize].trust_score + trust_gain).min(1.0);
                        self.total_rewards_egress += capped_fee * 0.8;

                        // Transit nodes split 20%
                        let transit_nodes: Vec<u32> = p.route_history.iter()
                            .filter(|&&n| {
                                n != node_id
                                    && self.nodes.get(n as usize)
                                        .map(|node| node.role != NodeRole::Ingress)
                                        .unwrap_or(false)
                            })
                            .copied()
                            .collect();
                        let transit_pool = capped_fee * 0.2;
                        if !transit_nodes.is_empty() {
                            let per_transit =
                                (transit_pool * velocity_bonus) / transit_nodes.len() as f64;
                            for &tn in &transit_nodes {
                                if let Some(node) = self.nodes.get_mut(tn as usize) {
                                    node.total_fees_earned += per_transit;
                                    let t_gain = match node.strategy {
                                        NodeStrategy::RiskAverse => 0.02,
                                        NodeStrategy::Greedy => 0.005,
                                        NodeStrategy::Passive => 0.01,
                                    };
                                    node.trust_score =
                                        (node.trust_score + t_gain).min(1.0);
                                }
                            }
                        }
                        self.total_rewards_transit += transit_pool;

                        let settlement_val = (p.current_value - capped_fee).max(0.0);
                        self.nodes[node_id as usize].inventory_crypto -= p.current_value;
                        self.total_output += settlement_val;
                        self.total_fees += capped_fee;
                        settled_count += 1;
                        self.settlement_count += 1;
                        self.total_settlement_hops += p.hops as u64;
                        self.total_settlement_time +=
                            current_tick.saturating_sub(p.arrival_tick);
                        self.nodes[node_id as usize].current_buffer_count =
                            self.nodes[node_id as usize].current_buffer_count
                                .saturating_sub(1);
                        continue;
                    } else {
                        // E6: Penalty on failed routing to Egress without liquidity
                        self.nodes[node_id as usize].trust_score =
                            (self.nodes[node_id as usize].trust_score - 0.05).max(0.0);
                    }
                }

                // Force orbit if packet has bounced too many times (hop limit)
                if p.hops > 20 {
                    p.status = PacketStatus::Orbiting;
                    if p.orbit_start_tick.is_none() {
                        p.orbit_start_tick = Some(current_tick);
                    }
                    buf.insert(j, p);
                    j += 1;
                    continue;
                }

                // Routing: find path to Egress (skip Disabled nodes)
                let neighbors: Vec<u32> = self.nodes[node_id as usize].neighbors.iter()
                    .filter(|&&n| self.nodes[n as usize].role != NodeRole::Disabled)
                    .copied()
                    .collect();

                // Only consider Egress nodes with actual liquidity for routing
                let target_egress = self.nodes.iter()
                    .filter(|n| n.role == NodeRole::Egress && n.inventory_crypto > 1.0)
                    .min_by(|a, b| {
                        let da = (a.x - self.nodes[node_id as usize].x).powi(2)
                            + (a.y - self.nodes[node_id as usize].y).powi(2);
                        let db = (b.x - self.nodes[node_id as usize].x).powi(2)
                            + (b.y - self.nodes[node_id as usize].y).powi(2);
                        da.partial_cmp(&db).unwrap()
                    });

                let next_hop = if let Some(target) = target_egress {
                    let mut best_neighbor = None;
                    let mut best_score = f64::MAX;
                    for &n_id in &neighbors {
                        let neighbor = &self.nodes[n_id as usize];
                        let dist_to_target = (target.x - neighbor.x).powi(2)
                            + (target.y - neighbor.y).powi(2);
                        let congestion = neighbor.current_buffer_count as f64 * 5.0;
                        // E6: Trust penalty in routing heuristic
                        let trust_penalty = (1.0 - neighbor.trust_score) * 10.0;
                        let score = dist_to_target + congestion + trust_penalty;
                        if score < best_score {
                            best_score = score;
                            best_neighbor = Some(n_id);
                        }
                    }
                    best_neighbor
                } else {
                    // No Egress with liquidity found - enter orbit
                    None
                };

                if let Some(target) = next_hop {
                    p.status = PacketStatus::InTransit;
                    p.target_node = Some(target);
                    p.hops += 1;
                    p.route_history.push(node_id);
                    p.orbit_start_tick = None; // Reset orbit timer on successful route

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
                    // E6: Penalty when packet can't be routed (node that held it)
                    // Only penalize if this node was supposed to route it forward
                    p.status = PacketStatus::Orbiting;
                    if p.orbit_start_tick.is_none() {
                        p.orbit_start_tick = Some(current_tick);
                    }
                    buf.insert(j, p);
                    j += 1;
                }
            }
        }

        // E12: Compute per-node liquidity pressure
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

        // 5. Finalize Stats
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
        let actual = self.total_output + self.total_burned + self.total_fees + active_val;
        self.state.total_value_leaked = (self.total_input - actual).abs();

        // Count orbiting packets
        let orbit_count: u32 = self.node_buffers.values().flatten()
            .filter(|p| p.status == PacketStatus::Orbiting)
            .count() as u32;
        self.state.orbit_count = orbit_count;

        // E6: Compute average trust score
        let trust_sum: f64 = self.nodes.iter()
            .filter(|n| n.role != NodeRole::Disabled)
            .map(|n| n.trust_score)
            .sum();
        let trust_count = self.nodes.iter()
            .filter(|n| n.role != NodeRole::Disabled)
            .count() as f64;
        self.state.avg_trust_score = if trust_count > 0.0 {
            trust_sum / trust_count
        } else {
            0.5
        };

        let mut active_packets = self.message_queue.clone();
        for b in self.node_buffers.values() { active_packets.extend(b.clone()); }

        TickResult {
            state: self.state.clone(),
            active_packets,
            node_updates: self.nodes.iter().map(|n| NodeUpdate {
                id: n.id, buffer_count: n.current_buffer_count,
                inventory_fiat: n.inventory_fiat, inventory_crypto: n.inventory_crypto,
            }).collect(),
        }
    }

    pub fn get_total_output(&self) -> f64 { self.total_output }
    pub fn get_total_value_leaked(&self) -> f64 { self.state.total_value_leaked }
    pub fn get_node_pressure(&self, node_id: usize) -> f64 {
        self.nodes.get(node_id).map_or(0.0, |n| n.pressure)
    }
}

// E11: Compute coefficient of variation from rolling price window
fn compute_rolling_volatility(history: &[f64]) -> f64 {
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

// WASM Interface
#[wasm_bindgen]
impl ArenaSimulation {
    #[wasm_bindgen(constructor)]
    pub fn new(node_count: u32) -> Self {
        #[cfg(target_arch = "wasm32")]
        std::panic::set_hook(Box::new(console_error_panic_hook::hook));

        let mut nodes = Vec::new();
        let mut node_buffers = HashMap::new();
        let grid_width = 6;
        let grid_height = 4;

        for i in 0..node_count {
            let role = match i % 4 {
                0 => NodeRole::Ingress,
                1 => NodeRole::Egress,
                2 => NodeRole::Transit,
                _ => NodeRole::NGauge,
            };
            // E9: Assign strategy cyclically
            let strategy = match i % 3 {
                0 => NodeStrategy::RiskAverse,
                1 => NodeStrategy::Greedy,
                _ => NodeStrategy::Passive,
            };
            let gx = (i % grid_width) as f64;
            let gy = (i / grid_width) as f64;

            let mut neighbors = Vec::new();
            let row = i / grid_width;
            let col = i % grid_width;
            if col > 0 && (i - 1) < node_count { neighbors.push(i - 1); }
            if col < grid_width - 1 && (i + 1) < node_count { neighbors.push(i + 1); }
            if row > 0 && (i - grid_width) < node_count { neighbors.push(i - grid_width); }
            if row < grid_height - 1 && (i + grid_width) < node_count {
                neighbors.push(i + grid_width);
            }

            nodes.push(SimNode {
                id: i, role, x: gx, y: gy,
                inventory_fiat: 10000.0, inventory_crypto: 100.0,
                current_buffer_count: 0,
                neighbors, distance_to_egress: u32::MAX,
                trust_score: 0.5, total_fees_earned: 0.0, accumulated_work: 0.0,
                strategy,
                pressure: 0.0,
            });
            node_buffers.insert(i, Vec::new());
        }

        // BFS to calculate distances
        let mut queue = std::collections::VecDeque::new();
        for node in &mut nodes {
            if node.role == NodeRole::Egress {
                node.distance_to_egress = 0;
                queue.push_back(node.id);
            }
        }
        while let Some(current_id) = queue.pop_front() {
            let current_dist = nodes[current_id as usize].distance_to_egress;
            let neighbors = nodes[current_id as usize].neighbors.clone();
            for neighbor_id in neighbors {
                let neighbor = &mut nodes[neighbor_id as usize];
                if neighbor.distance_to_egress == u32::MAX {
                    neighbor.distance_to_egress = current_dist + 1;
                    queue.push_back(neighbor_id);
                }
            }
        }

        Self {
            nodes, packets: Vec::new(), message_queue: Vec::new(),
            state: WorldState {
                current_tick: 0, gold_price: 2600.0, peg_deviation: 0.0,
                network_velocity: 0.0, demand_factor: 0.2, panic_level: 0.0,
                governance_quadrant: "D: GOLDEN ERA".to_string(),
                governance_status: "STABLE".to_string(),
                total_rewards_egress: 0.0, total_rewards_transit: 0.0,
                total_fees_collected: 0.0, total_demurrage_burned: 0.0,
                current_fee_rate: 0.001, current_demurrage_rate: 0.005,
                verification_complexity: 1, ngauge_activity_index: 0.0,
                total_value_leaked: 0.0, total_network_utility: 0.0,
                volatility: 0.0, settlement_count: 0, revert_count: 0, orbit_count: 0,
                total_input: 0.0, total_output: 0.0, active_value: 0.0,
                spawn_count: 0,
                avg_trust_score: 0.5,
                organic_ratio: 1.0,
                surge_multiplier: 1.0,
            },
            node_buffers, total_input: 0.0, total_output: 0.0,
            total_burned: 0.0, total_fees: 0.0,
            total_rewards_egress: 0.0, total_rewards_transit: 0.0,
            packet_id_counter: 0, max_active_packets: 1000,
            last_gold_price: 2600.0,
            settlement_count: 0, revert_count: 0,
            total_settlement_hops: 0, total_settlement_time: 0,
            gold_price_history: vec![2600.0],
        }
    }

    pub fn tick(&mut self) -> JsValue {
        let result = self.tick_core();
        serde_wasm_bindgen::to_value(&result).unwrap_or(JsValue::NULL)
    }

    pub fn spawn_packet(&mut self, node_id: u32, amount: f64) -> u64 {
        let p_id = self.packet_id_counter;
        self.packet_id_counter += 1;
        let p = SimPacket {
            id: p_id, original_value: amount, current_value: amount,
            arrival_tick: self.state.current_tick, status: PacketStatus::Active,
            origin_node: node_id, target_node: None, hops: 0,
            route_history: vec![node_id],
            orbit_start_tick: None,
        };
        self.total_input += amount;
        self.node_buffers.entry(node_id).or_default().push(p);
        self.nodes[node_id as usize].current_buffer_count += 1;
        p_id
    }

    pub fn get_nodes(&self) -> JsValue {
        serde_wasm_bindgen::to_value(&self.nodes).unwrap_or(JsValue::NULL)
    }

    pub fn set_gold_price(&mut self, val: f64) { self.state.gold_price = val; }
    pub fn set_demand_factor(&mut self, val: f64) { self.state.demand_factor = val; }
    pub fn set_panic_level(&mut self, val: f64) { self.state.panic_level = val; }

    pub fn get_stats(&self) -> JsValue {
        let orbit_count = self.node_buffers.values().flatten()
            .filter(|p| p.status == PacketStatus::Orbiting)
            .count() as u32;
        let active_val: f64 = self.node_buffers.values().flatten()
            .map(|p| p.current_value).sum::<f64>()
            + self.message_queue.iter().map(|p| p.current_value).sum::<f64>();
        let stats = SimStats {
            total_input: self.total_input,
            total_output: self.total_output,
            total_burned: self.total_burned,
            total_fees: self.total_fees,
            total_leaked: (self.total_input
                - (self.total_output + self.total_burned
                    + self.total_fees + active_val)).abs(),
            settlement_count: self.settlement_count,
            revert_count: self.revert_count,
            orbit_count,
            avg_hops: if self.settlement_count > 0 {
                self.total_settlement_hops as f64 / self.settlement_count as f64
            } else { 0.0 },
            avg_time_to_settle: if self.settlement_count > 0 {
                self.total_settlement_time as f64 / self.settlement_count as f64
            } else { 0.0 },
        };
        serde_wasm_bindgen::to_value(&stats).unwrap_or(JsValue::NULL)
    }

    pub fn kill_node(&mut self, node_id: u32) {
        if let Some(node) = self.nodes.get_mut(node_id as usize) {
            node.role = NodeRole::Disabled;
            let neighbor_ids = node.neighbors.clone();
            if let Some(packets) = self.node_buffers.remove(&node_id) {
                for mut p in packets {
                    p.target_node = None;
                    p.status = PacketStatus::Active;
                    if let Some(&dest) = neighbor_ids.iter()
                        .find(|&&n| self.nodes[n as usize].role != NodeRole::Disabled)
                    {
                        self.nodes[dest as usize].current_buffer_count += 1;
                        self.node_buffers.entry(dest).or_default().push(p);
                    }
                }
            }
        }
    }

    pub fn get_packet(&self, packet_id: u64) -> JsValue {
        let packet = self.node_buffers.values()
            .flat_map(|b| b.iter())
            .chain(self.message_queue.iter())
            .find(|p| p.id == packet_id);
        match packet {
            Some(p) => serde_wasm_bindgen::to_value(p).unwrap_or(JsValue::NULL),
            None => JsValue::NULL,
        }
    }

    /// Run N ticks without returning results (fast batch mode for benchmarking)
    pub fn run_batch(&mut self, ticks: u32) {
        for _ in 0..ticks {
            self.tick_core();
        }
    }

    pub fn set_node_crypto(&mut self, node_id: u32, val: f64) {
        if let Some(node) = self.nodes.get_mut(node_id as usize) {
            node.inventory_crypto = val;
        }
    }

    /// Reset simulation to initial state
    pub fn reset(&mut self) {
        *self = ArenaSimulation::new(self.nodes.len() as u32);
    }

    /// Get node trust scores as array
    pub fn get_trust_scores(&self) -> JsValue {
        let scores: Vec<f64> = self.nodes.iter().map(|n| n.trust_score).collect();
        serde_wasm_bindgen::to_value(&scores).unwrap_or(JsValue::NULL)
    }
}
