// Copyright 2026 Hypermesh Foundation. All rights reserved.
// Caesar Protocol Simulation Suite ("The Arena") - Capacity-Based Routing

use crate::types::{MarketTier, NodeRole, SimNode, SimPacket};

// Geographic/overlay scoring weights (capacity weights now in adapter)
const W_DISTANCE: f64 = 0.2;
const W_UPTIME: f64 = 0.05;
const W_TRANSIT_FEE: f64 = 0.1;
const W_TIER_MATCH: f64 = 0.05;

const BUFFER_CAPACITY: f64 = 20.0;
const BANDWIDTH_NORM_CAP: f64 = 1000.0;
const LATENCY_NORM_CAP: f64 = 500.0;

/// Compute the raw capacity score for a single node.
///
/// This score reflects how suitable a node is as a routing candidate
/// based purely on its capacity metrics (bandwidth, buffer, latency, load).
/// Higher values indicate better candidates.
pub fn score_candidate(node: &SimNode) -> f64 {
    let bandwidth_norm = (node.bandwidth / BANDWIDTH_NORM_CAP).min(1.0);
    let buffer_norm = 1.0 - (node.current_buffer_count as f64 / BUFFER_CAPACITY).min(1.0);
    let latency_norm = (node.latency / LATENCY_NORM_CAP).min(1.0);
    let load_norm = (node.current_buffer_count as f64 / BUFFER_CAPACITY).min(1.0);

    // Delegate to core's Decimal-based capacity scoring via adapter
    crate::adapter::score_capacity_via_core(bandwidth_norm, buffer_norm, latency_norm, load_norm)
}

/// Find the best next hop for a packet from the given node.
///
/// Capacity-based routing strategy:
/// 1. Filter neighbors to exclude Disabled nodes
/// 2. Find the nearest Egress node with sufficient liquidity (>1.0 crypto)
/// 3. Score each neighbor by capacity metrics, geographic distance,
///    uptime, transit fee, and tier preference
/// 4. Return the neighbor with the highest combined score, or None
pub fn find_next_hop(
    nodes: &[SimNode],
    node_id: u32,
    packet: &SimPacket,
) -> Option<u32> {
    let current = &nodes[node_id as usize];

    let neighbors: Vec<u32> = current
        .neighbors
        .iter()
        .filter(|&&n| nodes[n as usize].role != NodeRole::Disabled)
        .copied()
        .collect();

    // Find nearest Egress node with actual liquidity for routing target
    let target_egress = nodes
        .iter()
        .filter(|n| n.role == NodeRole::Egress && n.inventory_crypto > 1.0)
        .min_by(|a, b| {
            let da = distance_sq(a.x, a.y, current.x, current.y);
            let db = distance_sq(b.x, b.y, current.x, current.y);
            da.partial_cmp(&db).unwrap()
        });

    let target = match target_egress {
        Some(t) => t,
        None => return None, // No Egress with liquidity found - enter orbit
    };

    let max_dist = compute_max_distance(nodes, &neighbors, target);

    let mut best_neighbor: Option<u32> = None;
    let mut best_score = f64::NEG_INFINITY;

    for &n_id in &neighbors {
        let neighbor = &nodes[n_id as usize];
        let score = score_neighbor(neighbor, target, max_dist, packet);
        if score > best_score {
            best_score = score;
            best_neighbor = Some(n_id);
        }
    }

    best_neighbor
}

/// Score a neighbor candidate with all routing factors combined.
fn score_neighbor(
    neighbor: &SimNode,
    target: &SimNode,
    max_dist: f64,
    packet: &SimPacket,
) -> f64 {
    let capacity = score_candidate(neighbor);

    let distance_norm = if max_dist > 0.0 {
        let dist = distance_sq(neighbor.x, neighbor.y, target.x, target.y).sqrt();
        (dist / max_dist).min(1.0)
    } else {
        0.0
    };

    let uptime_bonus = W_UPTIME * neighbor.uptime.clamp(0.0, 1.0);
    let fee_penalty = W_TRANSIT_FEE * neighbor.transit_fee.min(1.0);
    let tier_bonus = tier_match_bonus(neighbor.tier_preference, packet.tier);

    capacity - W_DISTANCE * distance_norm + uptime_bonus - fee_penalty + tier_bonus
}

/// Compute squared Euclidean distance between two points.
fn distance_sq(x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    (x1 - x2).powi(2) + (y1 - y2).powi(2)
}

/// Compute the maximum distance from any neighbor to the target egress.
/// Used to normalize distance scores into [0, 1].
fn compute_max_distance(nodes: &[SimNode], neighbors: &[u32], target: &SimNode) -> f64 {
    neighbors
        .iter()
        .map(|&n_id| {
            let n = &nodes[n_id as usize];
            distance_sq(n.x, n.y, target.x, target.y).sqrt()
        })
        .fold(0.0_f64, f64::max)
}

/// Return tier-match bonus if the node's preference matches the packet tier.
fn tier_match_bonus(preference: Option<MarketTier>, packet_tier: MarketTier) -> f64 {
    match preference {
        Some(pref) if pref == packet_tier => W_TIER_MATCH,
        _ => 0.0,
    }
}
