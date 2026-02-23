// Poisson Traffic Generator — seedable, statistically validated
// Replaces engine's deterministic auto_spawn_traffic with Poisson-distributed arrivals

use rand::Rng;
use rand_chacha::ChaCha8Rng;

/// Power-law tier distribution matching real market data
const TIER_CDF: [f64; 4] = [0.60, 0.85, 0.97, 1.00]; // L0: 60%, L1: 25%, L2: 12%, L3: 3%

/// Value ranges per tier (in grams)
const TIER_VALUE_RANGES: [(f64, f64); 4] = [
    (0.5, 10.0),        // L0: retail
    (10.0, 1000.0),     // L1: commercial
    (1_000.0, 100_000.0),  // L2: institutional
    (100_000.0, 500_000.0), // L3: sovereign
];

pub struct TrafficGenerator {
    rng: ChaCha8Rng,
    pub ingress_nodes: Vec<u32>,
    pub spawn_count: u32,
    pub tier_counts: [u32; 4],
    current_fee_rate: f64,
}

impl TrafficGenerator {
    pub fn new(rng: ChaCha8Rng, ingress_nodes: Vec<u32>) -> Self {
        Self {
            rng,
            ingress_nodes,
            spawn_count: 0,
            tier_counts: [0; 4],
            current_fee_rate: 0.0,
        }
    }

    /// Update current fee rate for demand destruction logic
    pub fn set_fee_rate(&mut self, rate: f64) {
        self.current_fee_rate = rate;
    }

    /// Generate Poisson-distributed traffic for one tick.
    /// Returns Vec of (node_id, amount) to spawn.
    /// `lambda` is the expected number of packets per tick.
    pub fn generate_tick(&mut self, lambda: f64) -> Vec<(u32, f64)> {
        if self.ingress_nodes.is_empty() || lambda <= 0.0 {
            return Vec::new();
        }

        let n_packets = poisson_sample(&mut self.rng, lambda);
        let mut spawns = Vec::with_capacity(n_packets as usize);

        for _ in 0..n_packets {
            // E4: Demand destruction — cancel if fee > 10%
            if self.current_fee_rate > 0.10 {
                let cancel_prob = ((self.current_fee_rate - 0.10) * 5.0).min(1.0);
                if self.rng.gen::<f64>() < cancel_prob {
                    continue;
                }
            }

            // Select ingress node uniformly
            let node_idx = self.rng.gen_range(0..self.ingress_nodes.len());
            let node_id = self.ingress_nodes[node_idx];

            // Power-law tier selection
            let tier_idx = select_tier(&mut self.rng);
            self.tier_counts[tier_idx] += 1;

            // Uniform value within tier range
            let (lo, hi) = TIER_VALUE_RANGES[tier_idx];
            let amount = self.rng.gen_range(lo..hi);

            spawns.push((node_id, amount));
            self.spawn_count += 1;
        }

        spawns
    }

    /// Compute Poisson lambda for a given scenario demand level and node count.
    /// Preserves existing demand scaling: λ = demand × 5.0 × sqrt(nodes/24)
    pub fn compute_lambda(demand: f64, nodes: u32) -> f64 {
        demand * 5.0 * (nodes as f64 / 24.0).sqrt()
    }
}

/// Poisson sampling via Knuth algorithm.
/// For λ < 30, uses direct method. For larger λ, uses normal approximation.
fn poisson_sample(rng: &mut ChaCha8Rng, lambda: f64) -> u32 {
    if lambda < 30.0 {
        // Knuth's algorithm
        let l = (-lambda).exp();
        let mut k: u32 = 0;
        let mut p: f64 = 1.0;
        loop {
            k += 1;
            p *= rng.gen::<f64>();
            if p <= l {
                return k - 1;
            }
        }
    } else {
        // Normal approximation for large lambda
        let u1: f64 = rng.gen();
        let u2: f64 = rng.gen();
        let z = (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos();
        let result = lambda + lambda.sqrt() * z;
        result.round().max(0.0) as u32
    }
}

/// Power-law tier selection based on CDF
fn select_tier(rng: &mut ChaCha8Rng) -> usize {
    let r: f64 = rng.gen();
    for (i, &cdf) in TIER_CDF.iter().enumerate() {
        if r < cdf {
            return i;
        }
    }
    3 // L3 fallback
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    #[test]
    fn test_poisson_mean() {
        let mut rng = ChaCha8Rng::seed_from_u64(42);
        let lambda = 10.0;
        let n = 10000;
        let sum: u64 = (0..n).map(|_| poisson_sample(&mut rng, lambda) as u64).sum();
        let mean = sum as f64 / n as f64;
        assert!((mean - lambda).abs() < 0.5, "Poisson mean {} far from λ={}", mean, lambda);
    }

    #[test]
    fn test_tier_distribution() {
        let mut rng = ChaCha8Rng::seed_from_u64(42);
        let n = 10000;
        let mut counts = [0u32; 4];
        for _ in 0..n {
            counts[select_tier(&mut rng)] += 1;
        }
        let pcts: Vec<f64> = counts.iter().map(|&c| c as f64 / n as f64 * 100.0).collect();
        // Within ~3% of target (60/25/12/3) at N=10000
        assert!((pcts[0] - 60.0).abs() < 3.0, "L0: {:.1}% expected ~60%", pcts[0]);
        assert!((pcts[1] - 25.0).abs() < 3.0, "L1: {:.1}% expected ~25%", pcts[1]);
        assert!((pcts[2] - 12.0).abs() < 3.0, "L2: {:.1}% expected ~12%", pcts[2]);
        assert!((pcts[3] - 3.0).abs() < 2.0, "L3: {:.1}% expected ~3%", pcts[3]);
    }
}
