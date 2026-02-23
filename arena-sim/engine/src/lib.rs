// Copyright 2026 Hypermesh Foundation. All rights reserved.
// Caesar Protocol Simulation Suite ("The Arena")

pub mod types;
pub mod simulation;
pub mod routing;
pub mod governor;
pub mod engauge;
pub mod conservation;
pub mod dissolution;

// Vendored core Caesar modules (production code, adapted for arena)
pub mod core_types;
pub mod core_governor;
pub mod core_models;
pub mod core_routing;
pub mod core_conservation;
pub mod core_fee_distribution;
pub mod adapter;

pub use types::*;
pub use simulation::ArenaSimulation;

use wasm_bindgen::prelude::*;
use std::collections::HashMap;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
}

// ─── WASM Interface ──────────────────────────────────────────────────────────

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

            // Scale initial node inventory with network size
            let base_crypto = 1000.0 * (node_count as f64 / 24.0).max(1.0);
            // Egress nodes are well-capitalized settlement providers (500x base)
            let inventory_crypto = if role == NodeRole::Egress {
                base_crypto * 500.0
            } else {
                base_crypto
            };

            nodes.push(SimNode {
                id: i, role, x: gx, y: gy,
                inventory_fiat: 10000.0, inventory_crypto: inventory_crypto,
                current_buffer_count: 0,
                neighbors, distance_to_egress: u32::MAX,
                total_fees_earned: 0.0, accumulated_work: 0.0,
                strategy,
                pressure: 0.0,
                // v0.2 fields
                transit_fee: 0.01,
                bandwidth: 100.0,
                latency: 1.0,
                uptime: 1.0,
                tier_preference: None,
                upi_active: true,
                ngauge_running: true,
                kyc_valid: true,
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
                organic_ratio: 1.0,
                surge_multiplier: 1.0,
                // v0.2 fields
                circuit_breaker_active: false,
                ingress_throttle: 0.0,
                dissolved_count: 0,
                held_count: 0,
                tier_distribution: [0; 4],
                effective_price_composite: 0.0,
                network_fee_component: 0.0,
                speculation_component: 0.0,
                float_component: 0.0,
                tier_fee_rates: [0.0; 4],
            },
            node_buffers, total_input: 0.0, total_output: 0.0,
            total_burned: 0.0, total_fees: 0.0,
            total_rewards_egress: 0.0, total_rewards_transit: 0.0,
            packet_id_counter: 0, max_active_packets: 1000,
            last_gold_price: 2600.0,
            settlement_count: 0, revert_count: 0,
            total_settlement_hops: 0, total_settlement_time: 0,
            gold_price_history: vec![2600.0],
            lambda_ema: 1.0,
            conservation_law: conservation::ConservationLaw::default(),
            engauge_state: engauge::NGaugeState::default(),
            core_pid: crate::core_governor::pid::GovernorPid::new(),
            core_conservation: crate::core_conservation::ConservationLaw::new(
                crate::adapter::to_decimal(1000.0), // High threshold — parallel validation only
            ),
        }
    }

    pub fn tick(&mut self) -> JsValue {
        let result = self.tick_core();
        serde_wasm_bindgen::to_value(&result).unwrap_or(JsValue::NULL)
    }

    pub fn spawn_packet(&mut self, node_id: u32, amount: f64) -> u64 {
        let p_id = self.packet_id_counter;
        self.packet_id_counter += 1;
        let tier = MarketTier::from_value(amount);
        let p = SimPacket {
            id: p_id, original_value: amount, current_value: amount,
            arrival_tick: self.state.current_tick, status: PacketStatus::Minted,
            origin_node: node_id, target_node: None, hops: 0,
            route_history: vec![node_id],
            orbit_start_tick: None,
            tier,
            ttl: self.state.current_tick + tier.ttl_ticks(),
            hop_limit: tier.hop_limit(),
            fee_budget: tier.fee_cap() * amount,
            fees_consumed: 0.0,
            fee_schedule: Vec::new(),
            spawn_tick: self.state.current_tick,
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
            .filter(|p| p.status == PacketStatus::Held)
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
                    p.status = PacketStatus::Minted;
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

}
