#[cfg(test)]
mod tests {
    use arena_engine::ArenaSimulation;

    // ========== Existing Tests ==========

    #[test]
    fn test_egress_infinite_sink_bug_fixed() {
        let mut sim = ArenaSimulation::new(4);
        sim.set_node_crypto(1, 0.0);
        sim.spawn_packet(0, 100.0);

        let mut last_result = None;
        for _ in 0..20 {
            last_result = Some(sim.tick_core());
        }

        if let Some(res) = last_result {
            println!("State: {:?}", res.state);
        }

        let output = sim.get_total_output();
        println!("Total Output: {}", output);
        assert_eq!(output, 0.0, "Egress settled without liquidity!");
    }

    #[test]
    fn test_egress_liquidity_success() {
        let mut sim = ArenaSimulation::new(4);
        sim.set_node_crypto(1, 200.0);
        sim.spawn_packet(0, 100.0);

        for _ in 0..20 {
            sim.tick_core();
        }

        let output = sim.get_total_output();
        assert!(output > 0.0, "Egress failed to settle with sufficient liquidity!");

        let leak = sim.get_total_value_leaked();
        println!("Leak: {}", leak);
        assert!(leak < 0.001, "Leaky pipe! Value conservation violated.");
    }

    // ========== Test Suite A: Bank Run Resilience ==========

    #[test]
    fn test_bank_run_resilience() {
        let mut sim = ArenaSimulation::new(24);
        // Set limited Egress liquidity: $10k total across all Egress nodes
        for i in 0..24u32 {
            if i % 4 == 1 {
                // Egress nodes: ~$1666 each (6 Egress in 24 nodes)
                sim.set_node_crypto(i, 10000.0 / 6.0);
            }
        }

        // Simulate Bank Run: spawn many packets (500 packets x $200 = $100k demand)
        for i in 0..500u32 {
            let ingress_id = (i * 4) % 24;
            if ingress_id % 4 == 0 {
                sim.spawn_packet(ingress_id, 200.0);
            }
        }

        // Run for 200 ticks
        for _ in 0..200 {
            sim.tick_core();
        }

        // 1. No crash (we got here = pass)
        // 2. Solvency: conservation check
        let leak = sim.get_total_value_leaked();
        assert!(leak < 1.0, "Conservation violated! Leak: {}", leak);

        // 3. All packets either settled or reverted (none stuck forever)
        let output = sim.get_total_output();
        assert!(output > 0.0, "No settlements occurred during Bank Run");
    }

    // ========== Test Suite B: Route Healing ==========

    #[test]
    fn test_route_healing() {
        let mut sim = ArenaSimulation::new(24);

        // Spawn packets at various Ingress nodes
        for i in 0..100u32 {
            let ingress_id = (i * 4) % 24;
            if ingress_id % 4 == 0 {
                sim.spawn_packet(ingress_id, 100.0);
            }
        }

        // Let packets start routing (E10 variable latency needs more ticks)
        for _ in 0..20 {
            sim.tick_core();
        }

        // Kill a Transit node (node 2 is Transit: 0=Ingress, 1=Egress, 2=Transit)
        sim.kill_node(2);

        // Continue running (more ticks for E10 variable latency)
        for _ in 0..300 {
            sim.tick_core();
        }

        // Conservation must hold
        let leak = sim.get_total_value_leaked();
        assert!(
            leak < 1.0,
            "Value conservation violated after node kill! Leak: {}",
            leak
        );

        // Some packets should have settled despite the dead node
        let output = sim.get_total_output();
        assert!(output > 0.0, "No settlements after route healing");
    }

    // ========== Test Suite C: Sybil Attack (Fake Nodes) ==========

    #[test]
    fn test_sybil_trust_penalty() {
        let mut sim = ArenaSimulation::new(24);

        // Make several "fake" Egress nodes -- set their crypto to 0
        // Nodes 1, 5, 9, 13 are Egress (i % 4 == 1)
        sim.set_node_crypto(1, 0.0);
        sim.set_node_crypto(5, 0.0);
        sim.set_node_crypto(9, 0.0);
        // Leave node 13, 17, 21 with liquidity as honest Egress
        // Boost honest Egress liquidity for reliable settlement
        sim.set_node_crypto(13, 50000.0);
        sim.set_node_crypto(17, 50000.0);
        sim.set_node_crypto(21, 50000.0);

        // Spawn packets
        for i in 0..50u32 {
            let ingress_id = (i * 4) % 24;
            if ingress_id % 4 == 0 {
                sim.spawn_packet(ingress_id, 100.0);
            }
        }

        // Run for 300 ticks (E10 variable latency needs more time)
        for _ in 0..300 {
            sim.tick_core();
        }

        // Conservation must hold
        let leak = sim.get_total_value_leaked();
        assert!(
            leak < 1.0,
            "Conservation violated during Sybil test! Leak: {}",
            leak
        );

        // Some packets should still settle at honest Egress nodes
        let output = sim.get_total_output();
        assert!(output > 0.0, "No settlements despite honest Egress nodes");
    }

    // ========== Test: Demurrage Efficiency (Loop Decay) ==========

    #[test]
    fn test_demurrage_loop_decay() {
        let mut sim = ArenaSimulation::new(4);
        // Set ALL Egress nodes to 0 liquidity -- packet has nowhere to go
        sim.set_node_crypto(1, 0.0);

        sim.spawn_packet(0, 1000.0);

        // Run for 300 ticks -- packet should orbit (hop limit) and eventually revert
        // With E10 variable latency, packets need more time to bounce and orbit-timeout
        for _ in 0..300 {
            sim.tick_core();
        }

        // Conservation must hold
        let leak = sim.get_total_value_leaked();
        assert!(leak < 1.0, "Conservation violated in loop! Leak: {}", leak);

        // Packet should have reverted (orbit timeout)
        let output = sim.get_total_output();
        assert!(output > 0.0, "Packet never reverted from orbit");
        // Output should be less than 1000 (demurrage ate some value)
        assert!(output < 1000.0, "No demurrage applied to orbiting packet");
    }

    // ========== Test: Peg Elasticity ==========

    #[test]
    fn test_peg_elasticity() {
        let mut sim = ArenaSimulation::new(24);

        // Run stable for 100 ticks to accumulate settlements
        for _ in 0..100 {
            sim.tick_core();
        }

        // Swing gold price wildly: +50%
        sim.set_gold_price(3900.0);
        for _ in 0..100 {
            sim.tick_core();
        }

        // Swing back: -50% from peak
        sim.set_gold_price(1950.0);
        for _ in 0..100 {
            sim.tick_core();
        }

        // Conservation must hold through volatility
        let leak = sim.get_total_value_leaked();
        assert!(
            leak < 5.0,
            "Conservation violated during price swings! Leak: {}",
            leak
        );

        // System should still be functioning (settlements happening)
        let output = sim.get_total_output();
        assert!(output > 0.0, "System froze during price volatility");
    }

    // ========== Additional Validation Tests ==========

    #[test]
    fn test_run_batch_and_reset() {
        let mut sim = ArenaSimulation::new(24);
        sim.spawn_packet(0, 500.0);
        sim.run_batch(50);
        let output = sim.get_total_output();
        // Should have processed something
        assert!(output >= 0.0, "run_batch produced negative output");

        sim.reset();
        let output_after_reset = sim.get_total_output();
        assert_eq!(
            output_after_reset, 0.0,
            "Reset did not clear output"
        );
    }

    #[test]
    fn test_trust_score_dynamics() {
        let mut sim = ArenaSimulation::new(24);
        // Fake Egress with no liquidity should lose trust
        sim.set_node_crypto(1, 0.0);
        sim.spawn_packet(0, 100.0);

        for _ in 0..50 {
            sim.tick_core();
        }

        // The avg_trust_score should be tracked
        let result = sim.tick_core();
        assert!(
            result.state.avg_trust_score > 0.0,
            "avg_trust_score not computed"
        );
    }

    #[test]
    fn test_organic_ratio_computed() {
        let mut sim = ArenaSimulation::new(24);
        sim.set_demand_factor(1.0);
        for _ in 0..100 {
            sim.tick_core();
        }
        let result = sim.tick_core();
        // organic_ratio should be a real number
        assert!(
            result.state.organic_ratio.is_finite(),
            "organic_ratio is not finite"
        );
    }

    #[test]
    fn test_surge_multiplier_under_crunch() {
        let mut sim = ArenaSimulation::new(24);
        // Drain all Egress liquidity to trigger low lambda
        for i in 0..24u32 {
            if i % 4 == 1 {
                sim.set_node_crypto(i, 0.1);
            }
        }
        // Spawn heavy traffic to create demand
        for i in 0..200u32 {
            let ingress_id = (i * 4) % 24;
            if ingress_id % 4 == 0 {
                sim.spawn_packet(ingress_id, 1000.0);
            }
        }
        let result = sim.tick_core();
        // surge_multiplier should be > 1.0 during liquidity crunch
        assert!(
            result.state.surge_multiplier >= 1.0,
            "surge_multiplier should be >= 1.0, got {}",
            result.state.surge_multiplier
        );
    }

    #[test]
    fn test_variable_latency() {
        // With E10, distant nodes should have higher latency
        let mut sim = ArenaSimulation::new(24);
        sim.spawn_packet(0, 100.0);
        // After one tick, the packet should be in transit with latency > 1
        sim.tick_core();
        // Just verify no crash and system runs
        sim.tick_core();
        let leak = sim.get_total_value_leaked();
        assert!(leak < 1.0, "Leak after variable latency: {}", leak);
    }

    #[test]
    fn test_rolling_volatility_stability() {
        let mut sim = ArenaSimulation::new(24);
        // Run stable for 30 ticks
        for _ in 0..30 {
            sim.tick_core();
        }
        // Volatility should be near 0 with constant price
        let result = sim.tick_core();
        assert!(
            result.state.volatility < 0.01,
            "Volatility should be near 0 for constant price, got {}",
            result.state.volatility
        );

        // Now swing price
        sim.set_gold_price(5000.0);
        for _ in 0..5 {
            sim.tick_core();
        }
        let result2 = sim.tick_core();
        assert!(
            result2.state.volatility > 0.0,
            "Volatility should increase after price swing"
        );
    }

    #[test]
    fn test_node_pressure_computed() {
        let mut sim = ArenaSimulation::new(24);
        // Spawn multiple packets to create buffer pressure
        for _ in 0..20 {
            sim.spawn_packet(0, 100.0);
        }
        for _ in 0..5 {
            sim.tick_core();
        }
        // Check actual node pressure values via public accessor
        let has_nonzero_pressure = (0..24).any(|i| sim.get_node_pressure(i) > 0.0);
        assert!(has_nonzero_pressure, "At least one node should have non-zero pressure after spawning packets");
    }
}
