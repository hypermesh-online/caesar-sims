#[cfg(test)]
mod tests {
    use arena_engine::ArenaSimulation;

    #[test]
    fn test_egress_infinite_sink_bug_fixed() {
        let mut sim = ArenaSimulation::new(4);
        
        // Node 1 is Egress in a 4-node setup
        sim.set_node_crypto(1, 0.0);
        
        // Spawn packet at Ingress (Node 0) with value 100.0
        sim.spawn_packet(0, 100.0);
        
        // Run for 20 ticks
        let mut last_result = None;
        for _ in 0..20 {
            last_result = Some(sim.tick_core());
        }
        
        if let Some(res) = last_result {
            println!("State: {:?}", res.state);
        }

        let output = sim.get_total_output();
        println!("Total Output: {}", output);
        println!("Total Output: {}", output);
        
        // Assert that output IS 0.0 (Bug Fixed: No settlement without liquidity)
        assert_eq!(output, 0.0, "Egress settled without liquidity!");
    }

    #[test]
    fn test_egress_liquidity_success() {
        let mut sim = ArenaSimulation::new(4);
        
        // Node 1 is Egress
        sim.set_node_crypto(1, 200.0);
        
        // Spawn packet
        sim.spawn_packet(0, 100.0);
        
        for _ in 0..20 {
            sim.tick_core();
        }
        
        let output = sim.get_total_output();
        // Should be around 99.9 (100 - fee)
        assert!(output > 0.0, "Egress failed to settle with sufficient liquidity!");
        
        let leak = sim.get_total_value_leaked();
        println!("Leak: {}", leak);
        assert!(leak < 0.001, "Leaky pipe! Value conservation violated.");
    }
}
