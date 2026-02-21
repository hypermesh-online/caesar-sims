// Copyright © 2026 Hypermesh Foundation. All rights reserved.
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
}

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

    // New fields
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

    // New tracking fields
    settlement_count: u32,
    revert_count: u32,
    total_settlement_hops: u64,
    total_settlement_time: u64,
}

// Internal Logic (Testable, pure Rust)
impl ArenaSimulation {
    pub fn tick_core(&mut self) -> TickResult {
        self.state.current_tick += 1;

        // S1: Deliver in-transit packets from message queue
        let current_tick = self.state.current_tick;
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
                    // Collect neighbor info before mutating
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

        // S4: Separate volatility from peg deviation
        let volatility = ((self.state.gold_price - self.last_gold_price) / self.last_gold_price).abs();
        self.last_gold_price = self.state.gold_price;
        self.state.volatility = volatility;

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

        // Simulate NGauge Activity
        let mut total_work = 0.0;
        for node in self.nodes.iter_mut() {
            if node.role == NodeRole::NGauge {
                node.accumulated_work += (self.state.demand_factor * 10.0).max(1.0);
                total_work += node.accumulated_work;
            }
        }
        self.state.ngauge_activity_index = (total_work / (self.nodes.len() as f64 * 100.0)).min(1.0);

        // 1. The Caesar Governor Logic
        let mut demurrage = 0.005;
        let base_fee = 0.001;
        let sigma = 1.0 + volatility; // S4: sigma per spec formula
        let safe_lambda = lambda.max(0.1);
        let mut fee_rate = base_fee * (sigma / (safe_lambda * safe_lambda));

        let mut verification_complexity: u64 = 1;
        let mut quadrant = "D: GOLDEN ERA";
        let mut status = "STABLE";

        // S4: Compute peg deviation from effective exchange rate (after fee calc)
        let effective_rate = self.state.gold_price * (1.0 - fee_rate);
        let peg_deviation = (effective_rate - self.state.gold_price) / self.state.gold_price;
        // This equals -fee_rate, but during panic/crisis the combined effect is larger
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
            demurrage *= 0.5; // Reduce demurrage during panic (Emergency Brake)
        }

        if self.state.ngauge_activity_index > 0.5 {
            fee_rate *= 0.8;
            verification_complexity = verification_complexity.saturating_sub(1);
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

                // E4: Demand destruction - cancel if fees too high
                if self.state.current_fee_rate > 0.10 {
                    let cancel_prob = ((self.state.current_fee_rate - 0.10) * 5.0).min(1.0);
                    let check = ((self.packet_id_counter * 7 + i as u64) % 100) as f64 / 100.0;
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
            }
        }

        // 4. Node Execution Cycle (Sovereign Routing)
        let mut settled_count: u32 = 0;
        let mut _reverted_count: u32 = 0;
        let node_indices: Vec<u32> = self.node_buffers.keys().cloned().collect();

        for node_id in node_indices {
            let node_role = self.nodes[node_id as usize].role;
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
                            self.nodes[node_id as usize].current_buffer_count.saturating_sub(1);
                        continue; // Packet exits system
                    }
                }

                // Egress settlement
                if node_role == NodeRole::Egress && p.current_value > 0.0 {
                    if self.nodes[node_id as usize].inventory_crypto >= p.current_value {
                        // S5 + E3: 80/20 reward split with velocity bonus
                        let total_fee = (p.original_value * self.state.current_fee_rate).min(p.current_value);
                        p.route_history.push(node_id);

                        let velocity_bonus = if p.hops <= 3 { 1.2 }
                            else if p.hops <= 6 { 1.0 }
                            else { 0.8 };

                        // Egress gets 80%
                        let egress_reward = total_fee * 0.8 * velocity_bonus;
                        self.nodes[node_id as usize].total_fees_earned += egress_reward;
                        self.nodes[node_id as usize].trust_score =
                            (self.nodes[node_id as usize].trust_score + 0.01).min(1.0);
                        self.total_rewards_egress += total_fee * 0.8;

                        // Transit nodes split 20%
                        let transit_nodes: Vec<u32> = p.route_history.iter()
                            .filter(|&&n| n != node_id && self.nodes.get(n as usize)
                                .map(|node| node.role != NodeRole::Ingress).unwrap_or(false))
                            .copied()
                            .collect();
                        let transit_pool = total_fee * 0.2;
                        if !transit_nodes.is_empty() {
                            let per_transit = (transit_pool * velocity_bonus) / transit_nodes.len() as f64;
                            for &tn in &transit_nodes {
                                if let Some(node) = self.nodes.get_mut(tn as usize) {
                                    node.total_fees_earned += per_transit;
                                    node.trust_score = (node.trust_score + 0.01).min(1.0);
                                }
                            }
                        }
                        self.total_rewards_transit += transit_pool;

                        let settlement_val = (p.current_value - total_fee).max(0.0);
                        self.nodes[node_id as usize].inventory_crypto -= p.current_value;
                        self.total_output += settlement_val;
                        self.total_fees += total_fee;
                        settled_count += 1;
                        self.settlement_count += 1;
                        self.total_settlement_hops += p.hops as u64;
                        self.total_settlement_time += current_tick.saturating_sub(p.arrival_tick);
                        self.nodes[node_id as usize].current_buffer_count =
                            self.nodes[node_id as usize].current_buffer_count.saturating_sub(1);
                        continue;
                    }
                }

                // Routing: find path to Egress (skip Disabled nodes)
                let neighbors: Vec<u32> = self.nodes[node_id as usize].neighbors.iter()
                    .filter(|&&n| self.nodes[n as usize].role != NodeRole::Disabled)
                    .copied()
                    .collect();

                let target_egress = self.nodes.iter()
                    .filter(|n| n.role == NodeRole::Egress)
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
                        let score = dist_to_target + congestion;
                        if score < best_score {
                            best_score = score;
                            best_neighbor = Some(n_id);
                        }
                    }
                    best_neighbor
                } else {
                    neighbors.iter()
                        .min_by_key(|&&n| self.nodes[n as usize].current_buffer_count)
                        .cloned()
                };

                if let Some(target) = next_hop {
                    p.status = PacketStatus::InTransit;
                    p.target_node = Some(target);
                    p.hops += 1;
                    p.route_history.push(node_id);
                    p.orbit_start_tick = None; // Reset orbit timer on successful route
                    p.arrival_tick = current_tick + 1 + self.state.verification_complexity;
                    self.message_queue.push(p);
                    self.nodes[node_id as usize].current_buffer_count =
                        self.nodes[node_id as usize].current_buffer_count.saturating_sub(1);
                } else {
                    p.status = PacketStatus::Orbiting;
                    if p.orbit_start_tick.is_none() {
                        p.orbit_start_tick = Some(current_tick);
                    }
                    buf.insert(j, p);
                    j += 1;
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

        let active_val: f64 = self.node_buffers.values().flatten().map(|p| p.current_value).sum::<f64>()
            + self.message_queue.iter().map(|p| p.current_value).sum::<f64>();
        self.state.active_value = active_val;
        let actual = self.total_output + self.total_burned + self.total_fees + active_val;
        self.state.total_value_leaked = (self.total_input - actual).abs();

        // Count orbiting packets
        let orbit_count: u32 = self.node_buffers.values().flatten()
            .filter(|p| p.status == PacketStatus::Orbiting)
            .count() as u32;
        self.state.orbit_count = orbit_count;

        let mut active_packets = self.message_queue.clone();
        for b in self.node_buffers.values() { active_packets.extend(b.clone()); }

        TickResult {
            state: self.state.clone(),
            active_packets,
            node_updates: self.nodes.iter().map(|n| NodeUpdate {
                id: n.id, buffer_count: n.current_buffer_count,
                inventory_fiat: n.inventory_fiat, inventory_crypto: n.inventory_crypto
            }).collect(),
        }
    }

    pub fn set_node_crypto(&mut self, node_id: u32, val: f64) {
        if let Some(node) = self.nodes.get_mut(node_id as usize) {
            node.inventory_crypto = val;
        }
    }

    pub fn get_total_output(&self) -> f64 { self.total_output }
    pub fn get_total_value_leaked(&self) -> f64 { self.state.total_value_leaked }
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
            let gx = (i % grid_width) as f64;
            let gy = (i / grid_width) as f64;

            let mut neighbors = Vec::new();
            let row = i / grid_width;
            let col = i % grid_width;
            if col > 0 && (i - 1) < node_count { neighbors.push(i - 1); }
            if col < grid_width - 1 && (i + 1) < node_count { neighbors.push(i + 1); }
            if row > 0 && (i - grid_width) < node_count { neighbors.push(i - grid_width); }
            if row < grid_height - 1 && (i + grid_width) < node_count { neighbors.push(i + grid_width); }

            nodes.push(SimNode {
                id: i, role, x: gx, y: gy,
                inventory_fiat: 10000.0, inventory_crypto: 100.0, current_buffer_count: 0,
                neighbors, distance_to_egress: u32::MAX,
                trust_score: 0.5, total_fees_earned: 0.0, accumulated_work: 0.0
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
                current_tick: 0, gold_price: 2600.0, peg_deviation: 0.0, network_velocity: 0.0,
                demand_factor: 0.2, panic_level: 0.0,
                governance_quadrant: "D: GOLDEN ERA".to_string(),
                governance_status: "STABLE".to_string(),
                total_rewards_egress: 0.0, total_rewards_transit: 0.0,
                total_fees_collected: 0.0, total_demurrage_burned: 0.0,
                current_fee_rate: 0.001, current_demurrage_rate: 0.005,
                verification_complexity: 1, ngauge_activity_index: 0.0,
                total_value_leaked: 0.0, total_network_utility: 0.0,
                volatility: 0.0, settlement_count: 0, revert_count: 0, orbit_count: 0,
                total_input: 0.0, total_output: 0.0, active_value: 0.0,
            },
            node_buffers, total_input: 0.0, total_output: 0.0, total_burned: 0.0, total_fees: 0.0,
            total_rewards_egress: 0.0, total_rewards_transit: 0.0,
            packet_id_counter: 0, max_active_packets: 1000,
            last_gold_price: 2600.0,
            settlement_count: 0, revert_count: 0,
            total_settlement_hops: 0, total_settlement_time: 0,
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
            total_leaked: (self.total_input - (self.total_output + self.total_burned + self.total_fees + active_val)).abs(),
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
                        .find(|&&n| self.nodes[n as usize].role != NodeRole::Disabled) {
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
}
