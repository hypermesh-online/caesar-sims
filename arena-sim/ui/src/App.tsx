import { useEffect, useState, useRef, useCallback } from 'react';
import init, { ArenaSimulation } from './engine/arena_engine.js';
import { LineChart, Line, XAxis, YAxis, Tooltip, ResponsiveContainer, CartesianGrid } from 'recharts';
import { Play, Pause, Activity, DollarSign, Zap, ShieldAlert, Trash2, TrendingDown, Info, Package, Plus, Download, Search } from 'lucide-react';
import './App.css';

const NodeRole = { Ingress: 0, Egress: 1, Transit: 2, NGauge: 3 } as const;
type NodeRole = typeof NodeRole[keyof typeof NodeRole];

const ROLE_LABELS: Record<number, string> = {
  [NodeRole.Ingress]: 'Ingress', [NodeRole.Egress]: 'Egress',
  [NodeRole.Transit]: 'Transit', [NodeRole.NGauge]: 'NGauge',
};

const ROLE_COLORS: Record<number, string> = {
  [NodeRole.Ingress]: 'var(--accent-blue)', [NodeRole.Egress]: 'var(--accent-gold)',
  [NodeRole.Transit]: '#64748b', [NodeRole.NGauge]: 'var(--accent-green)',
};

const TIER_LABELS: Record<number, string> = { 0: 'L0', 1: 'L1', 2: 'L2', 3: 'L3' };
const TIER_COLORS: Record<number, string> = { 0: '#06b6d4', 1: '#f1f5f9', 2: '#f59e0b', 3: '#a855f7' };
const TIER_FEE_CAPS: Record<number, number> = { 0: 5.0, 1: 2.0, 2: 0.5, 3: 0.1 };

const PACKET_STATUS_LABELS: Record<number, string> = {
  0: 'Minted', 1: 'InTransit', 2: 'Delivered', 3: 'Settling',
  4: 'Settled', 5: 'Held', 6: 'Stalled', 7: 'Dispersed',
  8: 'Expired', 9: 'Refunded', 10: 'Dissolved',
};

interface Node {
  id: number; role: NodeRole; inventory_fiat: number; inventory_crypto: number;
  current_buffer_count: number; neighbors: number[]; x?: number; y?: number;
  total_fees_earned?: number; accumulated_work?: number;
  strategy?: string;
  // v0.2 fields
  transit_fee?: number;
  bandwidth?: number;
  latency?: number;
  uptime?: number;
  tier_preference?: number | null;
  pressure?: number;
  upi_active?: boolean;
  ngauge_running?: boolean;
  kyc_valid?: boolean;
}

interface TickResult { state: WorldState; active_packets: Packet[]; node_updates: NodeUpdate[]; }
interface NodeUpdate { id: number; buffer_count: number; inventory_fiat: number; inventory_crypto: number; }
interface Packet {
  id: number; status: number; current_value: number; origin_node: number;
  target_node?: number; arrival_tick: number; original_value?: number;
  hops?: number; route_history?: number[];
  // v0.2 fields
  tier?: number;          // 0=L0, 1=L1, 2=L2, 3=L3
  ttl?: number;           // expiry tick
  hop_limit?: number;     // tier-based max hops
  fee_budget?: number;    // pre-calculated max fees
  fees_consumed?: number; // accumulated fees
  fee_schedule?: number[]; // per-hop fees
}

interface WorldState {
  current_tick: number; gold_price: number; peg_deviation: number; network_velocity: number;
  demand_factor: number; panic_level: number;
  governance_quadrant: string; governance_status: string;
  total_rewards_egress: number; total_rewards_transit: number; total_fees_collected: number; total_demurrage_burned: number;
  current_fee_rate: number; current_demurrage_rate: number;
  verification_complexity: number; ngauge_activity_index: number;
  total_value_leaked: number;
  volatility?: number; settlement_count?: number; revert_count?: number; orbit_count?: number;
  total_input?: number; total_output?: number; active_value?: number;
  spawn_count?: number;
  total_spawned?: number; avg_time_to_settle?: number; avg_hops?: number;
  longest_orbit?: number;
  // v0.2 additions
  circuit_breaker_active?: boolean;
  ingress_throttle?: number;
  dissolved_count?: number;
  held_count?: number;
  tier_distribution?: number[];  // [L0, L1, L2, L3]
  effective_price_composite?: number;
  network_fee_component?: number;
  speculation_component?: number;
  float_component?: number;
  tier_fee_rates?: number[];  // [L0, L1, L2, L3]
  organic_ratio?: number;
  surge_multiplier?: number;
}

interface MetricPoint {
  tick: number; gold: number; velocity: number; deviation: number;
  fees: number; burn: number; feeRate: number; burnPerTick: number;
  spawnRate: number; settleRate: number;
}

interface LogEntry { tick: number; message: string; type: 'info' | 'warn' | 'error'; }

interface PassCriteria {
  maxConservationError: number;
  minSettlementRate?: number;
  maxFeeCapBreaches?: number;
  requireSettlementFinality?: boolean;
  requireCostCertainty?: boolean;
  requireAuditTrail?: boolean;
}

interface ScenarioConfig {
  name: string;
  label: string;
  category: 'market' | 'stress' | 'fiduciary';
  gold: number;
  demand: number;
  panic: number;
  nodes: number;
  ticks: number;
  goldCurve?: (tick: number) => number;
  demandCurve?: (tick: number) => number;
  passCriteria: PassCriteria;
}

interface TickLog {
  tick: number;
  goldPrice: number;
  feeRate: number;
  demurrageRate: number;
  pegDeviation: number;
  networkVelocity: number;
  volatility: number;
  surgeMultiplier: number;
  organicRatio: number;
  quadrant: string;
  conservationError: number;
  circuitBreakerActive: boolean;
  effectivePrice: number;
  networkFeeComponent: number;
  speculationComponent: number;
  floatComponent: number;
  tierDistribution: number[];
  tierFeeRates: number[];
  settlementCount: number;
  revertCount: number;
  dissolvedCount: number;
  heldCount: number;
  activePackets: number;
  activeValue: number;
  totalInput: number;
  totalOutput: number;
  totalBurned: number;
  totalFees: number;
  spawnCount: number;
}

interface BenchmarkResult {
  scenario: string;
  category: 'market' | 'stress' | 'fiduciary';
  settlementCount: number;
  revertCount: number;
  avgFee: number;
  conservationError: number;
  totalInput: number;
  totalOutput: number;
  pass: boolean;
  ticks: number;
  peakFee: number;
  dissolvedCount: number;
  heldCount: number;
  feeCapBreaches: number;
  settlementFinality: boolean;
  costCertainty: boolean;
  auditTrail: boolean;
  tierBreakdown: number[];
  tickHistory?: TickLog[];
}

interface RunStats {
  totalTicks: number; totalSpawned: number; totalSettled: number; totalReverted: number;
  totalOrbiting: number; settlementRate: number; avgFeeRate: number;
  avgTimeToSettle: number; avgHops: number; conservationError: number;
  peakFee: number; longestOrbit: number;
}

type SortKey = 'id' | 'role' | 'strategy' | 'fees' | 'capacity' | 'pressure' | 'buffer' | 'crypto' | 'fiat';

const SCENARIOS: ScenarioConfig[] = [
  // Market Conditions (5)
  { name: 'NORMAL_MARKET', label: 'Normal Market', category: 'market',
    gold: 2600, demand: 0.3, panic: 0.0, nodes: 24, ticks: 600,
    passCriteria: { maxConservationError: 1.0, minSettlementRate: 50 } },
  { name: 'BULL_RUN', label: 'Bull Run', category: 'market',
    gold: 3200, demand: 0.8, panic: 0.05, nodes: 24, ticks: 200,
    passCriteria: { maxConservationError: 1.0, minSettlementRate: 15 } },
  { name: 'BEAR_MARKET', label: 'Bear Market', category: 'market',
    gold: 1800, demand: 0.1, panic: 0.4, nodes: 24, ticks: 200,
    passCriteria: { maxConservationError: 1.0 } },
  { name: 'BLACK_SWAN', label: 'Black Swan', category: 'market',
    gold: 2600, demand: 0.9, panic: 0.95, nodes: 24, ticks: 300,
    goldCurve: (tick: number) => {
      if (tick < 100) return 2600 - tick * 11;
      if (tick < 200) return 1500 + (tick - 100) * 3;
      return 1800;
    },
    passCriteria: { maxConservationError: 2.0 } },
  { name: 'STAGFLATION', label: 'Stagflation', category: 'market',
    gold: 2600, demand: 0.05, panic: 0.3, nodes: 24, ticks: 200,
    passCriteria: { maxConservationError: 1.0 } },
  // Stress Tests (8)
  { name: 'SCALE_100', label: 'Scale 100', category: 'stress',
    gold: 2600, demand: 0.3, panic: 0.0, nodes: 100, ticks: 200,
    passCriteria: { maxConservationError: 5.0, minSettlementRate: 30 } },
  { name: 'SCALE_250', label: 'Scale 250', category: 'stress',
    gold: 2600, demand: 0.3, panic: 0.0, nodes: 250, ticks: 200,
    passCriteria: { maxConservationError: 10.0, minSettlementRate: 20 } },
  { name: 'SCALE_500', label: 'Scale 500', category: 'stress',
    gold: 2600, demand: 0.5, panic: 0.0, nodes: 500, ticks: 200,
    passCriteria: { maxConservationError: 20.0 } },
  { name: 'TIER_ISOLATION', label: 'Tier Isolation', category: 'stress',
    gold: 2600, demand: 0.5, panic: 0.0, nodes: 24, ticks: 200,
    passCriteria: { maxConservationError: 1.0 } },
  { name: 'FEE_CAP_STRESS', label: 'Fee Cap Stress', category: 'stress',
    gold: 2600, demand: 0.95, panic: 0.8, nodes: 24, ticks: 300,
    passCriteria: { maxConservationError: 2.0, maxFeeCapBreaches: 0 } },
  { name: 'GOVERNOR_STRESS', label: 'Governor Stress', category: 'stress',
    gold: 2600, demand: 0.5, panic: 0.0, nodes: 24, ticks: 200,
    goldCurve: (tick: number) => 2600 + Math.sin(tick / 10) * 800,
    demandCurve: (tick: number) => 0.5 + Math.sin(tick / 15) * 0.4,
    passCriteria: { maxConservationError: 2.0 } },
  { name: 'DISSOLUTION_TEST', label: 'Dissolution', category: 'stress',
    gold: 2600, demand: 0.3, panic: 0.0, nodes: 24, ticks: 8000,
    passCriteria: { maxConservationError: 1.0 } },
  { name: 'AML_DETECTION', label: 'AML Detection', category: 'stress',
    gold: 2600, demand: 0.9, panic: 0.0, nodes: 24, ticks: 200,
    passCriteria: { maxConservationError: 1.0 } },
  // Fiduciary Tests (3)
  { name: 'SETTLEMENT_FINALITY', label: 'Settlement Finality', category: 'fiduciary',
    gold: 2600, demand: 0.5, panic: 0.0, nodes: 24, ticks: 200,
    passCriteria: { maxConservationError: 0.01, requireSettlementFinality: true } },
  { name: 'COST_CERTAINTY', label: 'Cost Certainty', category: 'fiduciary',
    gold: 2600, demand: 0.5, panic: 0.2, nodes: 24, ticks: 200,
    passCriteria: { maxConservationError: 0.1, requireCostCertainty: true } },
  { name: 'AUDIT_TRAIL', label: 'Audit Trail', category: 'fiduciary',
    gold: 2600, demand: 0.3, panic: 0.0, nodes: 24, ticks: 200,
    passCriteria: { maxConservationError: 0.1, requireAuditTrail: true } },
];

function getMintingStatus(feeRate: number): { label: string; color: string } {
  if (feeRate < 0.01) return { label: 'OPEN', color: 'var(--accent-green)' };
  if (feeRate < 0.05) return { label: 'THROTTLED', color: 'var(--accent-gold)' };
  return { label: 'CLOSED', color: 'var(--accent-red)' };
}

function getBurningStatus(feeRate: number): { label: string; color: string } {
  if (feeRate > 0.02) return { label: 'SURGE PRICING', color: 'var(--accent-red)' };
  return { label: 'NORMAL', color: 'var(--accent-green)' };
}

function getGovernanceLevel(quadrant: string): { label: string; color: string } {
  const q = quadrant.toUpperCase();
  if (q.includes('GOLDEN')) return { label: 'STABLE', color: 'var(--accent-green)' };
  if (q.includes('STAGNATION')) return { label: 'DEFCON 3', color: 'var(--accent-gold)' };
  return { label: 'DEFCON 1', color: 'var(--accent-red)' };
}

function lerpColor(idle: [number, number, number], hot: [number, number, number], t: number): string {
  const r = Math.round(idle[0] + (hot[0] - idle[0]) * t);
  const g = Math.round(idle[1] + (hot[1] - idle[1]) * t);
  const b = Math.round(idle[2] + (hot[2] - idle[2]) * t);
  return `rgb(${r},${g},${b})`;
}

function App() {
  const [engine, setEngine] = useState<ArenaSimulation | null>(null);
  const [, setNodes] = useState<Node[]>([]);
  const [selectedNode, setSelectedNode] = useState<Node | null>(null);
  const [worldState, setWorldState] = useState<WorldState | null>(null);
  const [metrics, setMetrics] = useState<MetricPoint[]>([]);
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [isRunning, setIsRunning] = useState(false);
  const [playbackSpeed, setPlaybackSpeed] = useState(100);
  const [spawnAmount, setSpawnAmount] = useState(1000);
  const [packetCount, setPacketCount] = useState(0);
  const [nodeCount, setNodeCount] = useState(24);

  // B1: Benchmark state
  const [benchResults, setBenchResults] = useState<BenchmarkResult[]>([]);
  const [benchRunning, setBenchRunning] = useState(false);
  const [benchProgress, setBenchProgress] = useState('');
  const [benchCategory, setBenchCategory] = useState<'all' | 'market' | 'stress' | 'fiduciary'>('all');

  // B2: Halt on leak
  const [haltOnLeak, setHaltOnLeak] = useState(false);
  const [leakTrend, setLeakTrend] = useState<'up' | 'down' | 'flat'>('flat');
  const prevLeakRef = useRef(0);

  // B3: Run statistics
  const [runStats, setRunStats] = useState<RunStats | null>(null);

  // B5: Node economics table
  const [showNodeTable, setShowNodeTable] = useState(false);
  const [sortBy, setSortBy] = useState<SortKey>('id');
  const [sortDir, setSortDir] = useState<'asc' | 'desc'>('asc');

  // B6: Packet tracer
  const [tracePacketId, setTracePacketId] = useState('');
  const [tracedPacket, setTracedPacket] = useState<Packet | null>(null);
  const [traceError, setTraceError] = useState('');

  // B8: Liquidity depth
  const [liquidityDepth, setLiquidityDepth] = useState(100);

  // Tab state
  const [activeTab, setActiveTab] = useState<'live' | 'bench'>('live');

  // Tracking refs
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const nodesRef = useRef<Node[]>([]);
  const packetsRef = useRef<Packet[]>([]);
  const tickRef = useRef(0);
  const prevQuadrantRef = useRef<string>('');
  const prevFeeRateRef = useRef<number>(0);
  const prevBurnRef = useRef<number>(0);
  const prevSpawnedRef = useRef<number>(0);
  const prevSettledRef = useRef<number>(0);
  const peakFeeRef = useRef(0);
  const pegDeviationCountRef = useRef({ total: 0, withinBand: 0 });
  const maxOrbitRef = useRef(0);
  const haltOnLeakRef = useRef(false);
  const engineRef = useRef<ArenaSimulation | null>(null);

  // Keep ref in sync
  haltOnLeakRef.current = haltOnLeak;
  engineRef.current = engine;

  // Suppress unused-var lint for icons referenced only in JSX
  void DollarSign; void Info; void Package; void Plus; void Download; void Search;
  // Suppress unused-var lint for tier constants referenced only in JSX
  void TIER_LABELS; void TIER_COLORS; void TIER_FEE_CAPS; void PACKET_STATUS_LABELS;

  useEffect(() => {
    init().then(() => {
      const sim = new ArenaSimulation(24);
      setEngine(sim);
      engineRef.current = sim;
      const initialNodes = sim.get_nodes();
      setNodes(initialNodes);
      nodesRef.current = initialNodes;
      addLog('Simulation v0.7.0: Tier Economics & PID Governor', 'info');
    });
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const addLog = useCallback((message: string, type: 'info' | 'warn' | 'error' = 'info') => {
    setLogs(prev => [{ tick: tickRef.current, message, type }, ...prev].slice(0, 50));
  }, []);

  const reinitializeEngine = useCallback((count: number) => {
    if (engineRef.current) {
      engineRef.current.free();
    }
    setIsRunning(false);
    const sim = new ArenaSimulation(count);
    setEngine(sim);
    engineRef.current = sim;
    const initialNodes = sim.get_nodes();
    setNodes(initialNodes);
    nodesRef.current = initialNodes;
    setNodeCount(count);
    setMetrics([]);
    setLogs([]);
    setBenchResults([]);
    setRunStats(null);
    tickRef.current = 0;
    peakFeeRef.current = 0;
    maxOrbitRef.current = 0;
    prevLeakRef.current = 0;
    prevBurnRef.current = 0;
    prevSpawnedRef.current = 0;
    prevSettledRef.current = 0;
    pegDeviationCountRef.current = { total: 0, withinBand: 0 };
    addLog(`Engine reinitialized with ${count} nodes`, 'info');
  }, [addLog]);

  const applyPreset = useCallback((name: string) => {
    const eng = engineRef.current;
    if (!eng) return;
    const scenario = SCENARIOS.find(s => s.name === name);
    if (scenario) {
      eng.set_gold_price(scenario.gold);
      eng.set_demand_factor(scenario.demand);
      eng.set_panic_level(scenario.panic);
    }
  }, []);

  const spawnPacket = () => {
    if (!engine) return;
    const ingressNodes = nodesRef.current.filter(n => n.role === NodeRole.Ingress);
    if (ingressNodes.length === 0) return;
    const target = ingressNodes[Math.floor(Math.random() * ingressNodes.length)];
    const spawner = engine as unknown as Record<string, ((nodeId: number, amount: number) => void) | undefined>;
    if (typeof spawner.spawn_packet === 'function') {
      spawner.spawn_packet(target.id, spawnAmount);
    }
    addLog(`Manual spawn: $${spawnAmount} at Ingress #${target.id}`, 'info');
  };

  const updateRunStats = useCallback((state: WorldState, packets: Packet[]) => {
    const settled = state.settlement_count ?? 0;
    const reverted = state.revert_count ?? 0;
    const spawned = state.total_spawned ?? state.total_input ?? 0;
    const orbiting = state.held_count ?? state.orbit_count ?? packets.filter(p => p.status === 5).length;
    const rate = (settled + reverted) > 0 ? (settled / (settled + reverted)) * 100 : 0;
    const stats = (engineRef.current as unknown as Record<string, (() => Record<string, number>) | undefined>);
    const engineStats = typeof stats.get_stats === 'function' ? stats.get_stats() : {};

    setRunStats({
      totalTicks: state.current_tick,
      totalSpawned: spawned,
      totalSettled: settled,
      totalReverted: reverted,
      totalOrbiting: orbiting,
      settlementRate: rate,
      avgFeeRate: state.current_fee_rate * 100,
      avgTimeToSettle: (engineStats as Record<string, number>)?.avg_time_to_settle ?? 0,
      avgHops: (engineStats as Record<string, number>)?.avg_hops ?? 0,
      conservationError: state.total_value_leaked,
      peakFee: peakFeeRef.current * 100,
      longestOrbit: maxOrbitRef.current,
    });
  }, []);

  useEffect(() => {
    if (!engine || !isRunning) return;
    let rafId: number;
    let lastTickTime = performance.now();

    const loop = (now: number) => {
      if (now - lastTickTime >= playbackSpeed) {
        const result: TickResult = engine.tick();
        const state = result.state;
        packetsRef.current = result.active_packets;
        tickRef.current = state.current_tick;
        setPacketCount(result.active_packets.length);

        // Track peak fee
        peakFeeRef.current = Math.max(peakFeeRef.current, state.current_fee_rate);

        // Track peg deviation for B4 metric
        pegDeviationCountRef.current.total++;
        if (Math.abs(state.peg_deviation) < 0.20) {
          pegDeviationCountRef.current.withinBand++;
        }

        // Track max orbit duration (status 5 = Held in v0.2)
        for (const p of result.active_packets) {
          if (p.status === 5) {
            const orbitDuration = state.current_tick - p.arrival_tick;
            maxOrbitRef.current = Math.max(maxOrbitRef.current, orbitDuration);
          }
        }

        result.node_updates.forEach(u => {
          const n = nodesRef.current[u.id];
          if (n) {
            n.current_buffer_count = u.buffer_count;
            n.inventory_fiat = u.inventory_fiat;
            n.inventory_crypto = u.inventory_crypto;
          }
        });

        // B2: Halt on leak check
        if (haltOnLeakRef.current && Math.abs(state.total_value_leaked) > 0.1) {
          setIsRunning(false);
          addLog(`HALT: Leak threshold exceeded (${state.total_value_leaked.toFixed(6)})`, 'error');
        }

        // Leak trend tracking
        const currentLeak = Math.abs(state.total_value_leaked);
        const prevLeak = prevLeakRef.current;
        if (currentLeak > prevLeak + 0.001) setLeakTrend('up');
        else if (currentLeak < prevLeak - 0.001) setLeakTrend('down');
        else setLeakTrend('flat');
        prevLeakRef.current = currentLeak;

        if (state.current_tick % 5 === 0) {
          setWorldState(state);
          updateRunStats(state, result.active_packets);

          const currentBurn = state.total_demurrage_burned;
          const burnPerTick = currentBurn - prevBurnRef.current;
          prevBurnRef.current = currentBurn;

          const currentSpawned = state.total_input ?? 0;
          const currentSettled = state.settlement_count ?? 0;
          const spawnRate = currentSpawned - prevSpawnedRef.current;
          const settleRate = currentSettled - prevSettledRef.current;
          prevSpawnedRef.current = currentSpawned;
          prevSettledRef.current = currentSettled;

          setMetrics(prev => [...prev.slice(-49), {
            tick: state.current_tick, gold: state.gold_price,
            velocity: state.network_velocity, deviation: state.peg_deviation * 100,
            fees: state.total_fees_collected, burn: state.total_demurrage_burned,
            feeRate: state.current_fee_rate * 100,
            burnPerTick: Math.max(0, burnPerTick),
            spawnRate: Math.max(0, spawnRate),
            settleRate: Math.max(0, settleRate),
          }]);
          setNodes([...nodesRef.current]);

          const currentQuadrant = state.governance_quadrant;
          if (prevQuadrantRef.current && currentQuadrant !== prevQuadrantRef.current) {
            addLog(`Governance shift: ${prevQuadrantRef.current} -> ${currentQuadrant}`, 'warn');
          }
          prevQuadrantRef.current = currentQuadrant;

          const feeRateDelta = Math.abs(state.current_fee_rate - prevFeeRateRef.current);
          if (prevFeeRateRef.current > 0 && feeRateDelta / prevFeeRateRef.current > 0.05) {
            addLog(`Fee rate spike: ${(state.current_fee_rate * 100).toFixed(2)}% (delta ${(feeRateDelta * 100).toFixed(2)}%)`, 'warn');
          }
          prevFeeRateRef.current = state.current_fee_rate;

          if (Math.abs(state.total_value_leaked) >= 0.01) {
            addLog(`LEAK DETECTED: ${state.total_value_leaked.toFixed(4)} value unaccounted`, 'error');
          }

          if (state.current_tick % 50 === 0 && state.current_tick > 0) {
            addLog(`Tick ${state.current_tick}: ${result.active_packets.length} pkts, fee=${(state.current_fee_rate * 100).toFixed(1)}%`, 'info');
          }
        }
        lastTickTime = now;
      }
      draw();
      rafId = requestAnimationFrame(loop);
    };
    rafId = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(rafId);
  }, [engine, isRunning, playbackSpeed, addLog, updateRunStats]);

  const draw = () => {
    const canvas = canvasRef.current; if (!canvas) return;
    const ctx = canvas.getContext('2d', { alpha: false }); if (!ctx) return;
    const curNodes = nodesRef.current;
    const packets = packetsRef.current;
    const curTick = tickRef.current;

    ctx.fillStyle = '#0f172a'; ctx.fillRect(0, 0, canvas.width, canvas.height);

    const gridPadding = 80;
    const gridWidth = Math.ceil(Math.sqrt(curNodes.length * 1.5));
    const gridHeight = Math.ceil(curNodes.length / gridWidth);
    const cellWidth = gridWidth > 1 ? (canvas.width - gridPadding * 2) / (gridWidth - 1) : canvas.width / 2;
    const cellHeight = gridHeight > 1 ? (canvas.height - gridPadding * 2) / (gridHeight - 1) : canvas.height / 2;

    curNodes.forEach((node, i) => {
      const col = i % gridWidth;
      const row = Math.floor(i / gridWidth);
      node.x = gridPadding + col * cellWidth;
      node.y = gridPadding + row * cellHeight;
    });

    const idleColor: [number, number, number] = [30, 41, 59];
    const hotColor: [number, number, number] = [239, 68, 68];
    curNodes.forEach(node => {
      node.neighbors.forEach(nId => {
        const neighbor = curNodes[nId];
        if (neighbor && node.x && node.y && neighbor.x && neighbor.y) {
          const combinedBuffer = node.current_buffer_count + neighbor.current_buffer_count;
          const congestion = Math.min(combinedBuffer / 10, 1);
          const thickness = 1 + Math.min(combinedBuffer, 10) * 0.3;
          ctx.strokeStyle = lerpColor(idleColor, hotColor, congestion);
          ctx.lineWidth = thickness;
          ctx.beginPath();
          ctx.moveTo(node.x, node.y);
          ctx.lineTo(neighbor.x, neighbor.y);
          ctx.stroke();
        }
      });
    });

    const nodeRadius = curNodes.length <= 50 ? 8 : curNodes.length <= 150 ? 5 : curNodes.length <= 300 ? 3 : 2;
    const showLabels = curNodes.length <= 150;

    curNodes.forEach((node) => {
      if (!node.x || !node.y) return;
      ctx.beginPath();
      ctx.arc(node.x, node.y, nodeRadius + (node.current_buffer_count * 1.5), 0, Math.PI * 2);
      if (node.role === 0) ctx.fillStyle = '#3b82f6';
      else if (node.role === 1) ctx.fillStyle = '#f59e0b';
      else if (node.role === 2) ctx.fillStyle = '#64748b';
      else {
        const alpha = 0.5 + Math.sin(curTick / 2) * 0.5;
        ctx.fillStyle = `rgba(16, 185, 129, ${alpha})`;
      }
      ctx.fill();
      if (selectedNode?.id === node.id) {
        ctx.strokeStyle = '#fff'; ctx.lineWidth = 2; ctx.stroke();
      }
      if (showLabels) {
        ctx.fillStyle = '#fff'; ctx.font = '10px Inter';
        ctx.fillText(node.id.toString(), node.x - 3, node.y + 3);
      }
    });

    packets.forEach(p => {
      const origin = curNodes[p.origin_node];
      const target = p.target_node !== undefined && p.target_node !== null ? curNodes[p.target_node] : null;
      if (origin && origin.x && origin.y) {
        // Color by tier
        const tierColor = TIER_COLORS[p.tier ?? 0] ?? TIER_COLORS[0];
        ctx.fillStyle = tierColor;
        ctx.globalAlpha = 0.8;
        ctx.beginPath();
        if (p.status === 1 && target && target.x && target.y) {
          const progress = 1.0 - ((p.arrival_tick - curTick) / 5);
          const px = origin.x + (target.x - origin.x) * Math.max(0, Math.min(1, progress));
          const py = origin.y + (target.y - origin.y) * Math.max(0, Math.min(1, progress));
          ctx.arc(px, py, 3, 0, Math.PI * 2);
        } else {
          const ox = origin.x + (Math.sin(p.id + curTick / 10) * 15);
          const oy = origin.y + (Math.cos(p.id + curTick / 10) * 15);
          ctx.arc(ox, oy, 2, 0, Math.PI * 2);
        }
        ctx.fill();
        ctx.globalAlpha = 1.0;
      }
    });
  };

  // B1: Benchmark runner
  const runBenchmarkScenario = useCallback(async (scenario: ScenarioConfig): Promise<BenchmarkResult> => {
    const benchEngine = new ArenaSimulation(scenario.nodes);
    benchEngine.set_gold_price(scenario.gold);
    benchEngine.set_demand_factor(scenario.demand);
    benchEngine.set_panic_level(scenario.panic);

    let peakFee = 0;
    let feeCapBreaches = 0;
    let lastState: WorldState | null = null;
    let allPacketsSettledFinal = true;
    let costCertaintyViolations = 0;
    let auditTrailViolations = 0;
    const tickCount = scenario.ticks;

    // Tick-by-tick logging for post-simulation analysis
    const tickHistory: TickLog[] = [];
    const sampleInterval = 5;

    for (let i = 0; i < tickCount; i++) {
      if (scenario.goldCurve) benchEngine.set_gold_price(scenario.goldCurve(i));
      if (scenario.demandCurve) benchEngine.set_demand_factor(scenario.demandCurve(i));

      const result: TickResult = benchEngine.tick();
      lastState = result.state;
      peakFee = Math.max(peakFee, result.state.current_fee_rate);

      // Sample tick history for analysis
      if (i % sampleInterval === 0) {
        tickHistory.push({
          tick: result.state.current_tick,
          goldPrice: result.state.gold_price,
          feeRate: result.state.current_fee_rate,
          demurrageRate: result.state.current_demurrage_rate,
          pegDeviation: result.state.peg_deviation,
          networkVelocity: result.state.network_velocity,
          volatility: result.state.volatility ?? 0,
          surgeMultiplier: result.state.surge_multiplier ?? 1,
          organicRatio: result.state.organic_ratio ?? 1,
          quadrant: result.state.governance_quadrant,
          conservationError: Math.abs(result.state.total_value_leaked),
          circuitBreakerActive: result.state.circuit_breaker_active ?? false,
          effectivePrice: result.state.effective_price_composite ?? result.state.gold_price,
          networkFeeComponent: result.state.network_fee_component ?? 0,
          speculationComponent: result.state.speculation_component ?? 0,
          floatComponent: result.state.float_component ?? 0,
          tierDistribution: result.state.tier_distribution ? [...result.state.tier_distribution] : [0, 0, 0, 0],
          tierFeeRates: result.state.tier_fee_rates ? [...result.state.tier_fee_rates] : [0, 0, 0, 0],
          settlementCount: result.state.settlement_count ?? 0,
          revertCount: result.state.revert_count ?? 0,
          dissolvedCount: result.state.dissolved_count ?? 0,
          heldCount: result.state.held_count ?? 0,
          activePackets: result.active_packets.length,
          activeValue: result.state.active_value ?? 0,
          totalInput: result.state.total_input ?? 0,
          totalOutput: result.state.total_output ?? 0,
          totalBurned: result.state.total_demurrage_burned,
          totalFees: result.state.total_fees_collected,
          spawnCount: result.state.spawn_count ?? 0,
        });
      }

      const tierRates = result.state.tier_fee_rates ?? [0, 0, 0, 0];
      const caps = [0.05, 0.02, 0.005, 0.001];
      for (let t = 0; t < 4; t++) {
        if (tierRates[t] > caps[t] + 0.0001) feeCapBreaches++;
      }

      for (const p of result.active_packets) {
        if (p.fee_budget && p.fee_budget > 0 && (p.fees_consumed ?? 0) > p.fee_budget + 0.0001) {
          costCertaintyViolations++;
        }
        if (!p.route_history || p.route_history.length === 0) {
          auditTrailViolations++;
        }
        if (p.status === 4) allPacketsSettledFinal = false;
      }

      if (i % 50 === 0) await new Promise(r => setTimeout(r, 0));
    }

    benchEngine.free();

    if (!lastState) {
      return {
        scenario: scenario.label, category: scenario.category,
        settlementCount: 0, revertCount: 0, avgFee: 0,
        conservationError: 0, totalInput: 0, totalOutput: 0, pass: false,
        ticks: tickCount, peakFee: 0,
        dissolvedCount: 0, heldCount: 0, feeCapBreaches: 0,
        settlementFinality: false, costCertainty: false, auditTrail: false,
        tierBreakdown: [0, 0, 0, 0],
        tickHistory: [],
      };
    }

    const settled = lastState.settlement_count ?? 0;
    const reverted = lastState.revert_count ?? 0;
    const error = Math.abs(lastState.total_value_leaked);
    const avgFee = lastState.current_fee_rate;
    const criteria = scenario.passCriteria;

    let pass = error <= criteria.maxConservationError;
    if (criteria.minSettlementRate && settled > 0) {
      const spawned = lastState.spawn_count ?? 1;
      const rate = (settled / spawned) * 100;
      if (rate < criteria.minSettlementRate) pass = false;
    }
    if (criteria.maxFeeCapBreaches !== undefined && feeCapBreaches > criteria.maxFeeCapBreaches) pass = false;
    if (criteria.requireSettlementFinality && !allPacketsSettledFinal) pass = false;
    if (criteria.requireCostCertainty && costCertaintyViolations > 0) pass = false;
    if (criteria.requireAuditTrail && auditTrailViolations > 0) pass = false;

    return {
      scenario: scenario.label, category: scenario.category,
      settlementCount: settled, revertCount: reverted,
      avgFee: avgFee * 100, conservationError: error,
      totalInput: lastState.total_input ?? 0, totalOutput: lastState.total_output ?? 0,
      pass, ticks: tickCount, peakFee: peakFee * 100,
      dissolvedCount: lastState.dissolved_count ?? 0,
      heldCount: lastState.held_count ?? 0,
      feeCapBreaches,
      settlementFinality: allPacketsSettledFinal,
      costCertainty: costCertaintyViolations === 0,
      auditTrail: auditTrailViolations === 0,
      tierBreakdown: lastState.tier_distribution ? [...lastState.tier_distribution] : [0, 0, 0, 0],
      tickHistory,
    };
  }, []);

  const runAllBenchmarks = useCallback(async () => {
    setBenchRunning(true);
    setBenchResults([]);
    const scenariosToRun = benchCategory === 'all'
      ? SCENARIOS
      : SCENARIOS.filter(s => s.category === benchCategory);
    const results: BenchmarkResult[] = [];
    for (const scenario of scenariosToRun) {
      setBenchProgress(`Running: ${scenario.label} (${results.length + 1}/${scenariosToRun.length})...`);
      const result = await runBenchmarkScenario(scenario);
      results.push(result);
      setBenchResults([...results]);
    }
    setBenchProgress('Complete');
    setBenchRunning(false);
  }, [runBenchmarkScenario, benchCategory]);

  // B6: Trace packet
  const tracePacket = useCallback(() => {
    if (!engine || !tracePacketId) { setTraceError('Enter a packet ID'); return; }
    try {
      const pkt = engine.get_packet(BigInt(tracePacketId));
      if (pkt) { setTracedPacket(pkt); setTraceError(''); }
      else { setTracedPacket(null); setTraceError('Packet not found'); }
    } catch { setTracedPacket(null); setTraceError('Invalid packet ID'); }
  }, [engine, tracePacketId]);

  // B7: Export data
  const exportData = useCallback(() => {
    const data = {
      worldState, metrics, runStats,
      nodes: nodesRef.current, packets: packetsRef.current,
      benchmarks: benchResults.map(r => ({
        scenario: r.scenario,
        category: r.category,
        pass: r.pass,
        settlementCount: r.settlementCount,
        revertCount: r.revertCount,
        dissolvedCount: r.dissolvedCount,
        heldCount: r.heldCount,
        avgFee: r.avgFee,
        peakFee: r.peakFee,
        conservationError: r.conservationError,
        feeCapBreaches: r.feeCapBreaches,
        settlementFinality: r.settlementFinality,
        costCertainty: r.costCertainty,
        auditTrail: r.auditTrail,
        tierBreakdown: r.tierBreakdown,
        tickHistory: r.tickHistory ?? [],
      })),
      tierBreakdown: worldState?.tier_distribution ?? [0, 0, 0, 0],
      timestamp: new Date().toISOString(), version: '0.7.0',
    };
    const blob = new Blob([JSON.stringify(data, null, 2)], { type: 'application/json' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url; a.download = `arena-export-${Date.now()}.json`; a.click();
    URL.revokeObjectURL(url);
  }, [worldState, metrics, benchResults, runStats]);

  // B8: Apply liquidity depth
  const applyLiquidity = useCallback((depth: number) => {
    if (!engine) return;
    setLiquidityDepth(depth);
    const setter = engine as unknown as Record<string, ((id: number, val: number) => void) | undefined>;
    nodesRef.current.forEach(n => {
      if (n.role === NodeRole.Egress && typeof setter.set_node_crypto === 'function') {
        const baseInventory = 100.0;
        setter.set_node_crypto(n.id, baseInventory * (depth / 100));
      }
    });
  }, [engine]);

  // B5: Sorted node data
  const getSortedNodes = useCallback((): Node[] => {
    const nodes = [...nodesRef.current];
    const dir = sortDir === 'asc' ? 1 : -1;
    return nodes.sort((a, b) => {
      switch (sortBy) {
        case 'role': return dir * (a.role - b.role);
        case 'fees': return dir * ((a.total_fees_earned ?? 0) - (b.total_fees_earned ?? 0));
        case 'capacity': return dir * ((a.bandwidth ?? 0) - (b.bandwidth ?? 0));
        case 'pressure': return dir * ((a.pressure ?? 0) - (b.pressure ?? 0));
        case 'buffer': return dir * (a.current_buffer_count - b.current_buffer_count);
        case 'crypto': return dir * (a.inventory_crypto - b.inventory_crypto);
        case 'fiat': return dir * (a.inventory_fiat - b.inventory_fiat);
        default: return dir * (a.id - b.id);
      }
    });
  }, [sortBy, sortDir]);

  const toggleSort = (key: SortKey) => {
    if (sortBy === key) setSortDir(d => d === 'asc' ? 'desc' : 'asc');
    else { setSortBy(key); setSortDir('asc'); }
  };

  // B4: Pass/fail metric calculations
  const getPassFailMetrics = useCallback(() => {
    const settled = worldState?.settlement_count ?? 0;
    const reverted = worldState?.revert_count ?? 0;
    const spawned = worldState?.spawn_count ?? 0;
    const noFail = spawned > 0 ? (settled + reverted) / spawned >= 0.99 : false;
    const pegRef = pegDeviationCountRef.current;
    const pegElasticity = pegRef.total > 0 ? (pegRef.withinBand / pegRef.total) >= 0.95 : false;
    const peakFee = peakFeeRef.current;
    const incentiveAlign = peakFee > 0.05;
    const demurrageEff = maxOrbitRef.current <= 50;
    return { noFail, pegElasticity, incentiveAlign, demurrageEff };
  }, [worldState]);

  // Conservation invariant calculations
  const totalFees = worldState?.total_fees_collected ?? 0;
  const totalBurned = worldState?.total_demurrage_burned ?? 0;
  const totalRewards = (worldState?.total_rewards_egress ?? 0) + (worldState?.total_rewards_transit ?? 0);
  const totalSpawned = worldState?.total_input ?? 0;
  const totalSettled = worldState?.total_output ?? 0;
  const totalRefunded = 0;
  const conservationError = worldState?.total_value_leaked ?? 0;
  const isConserved = Math.abs(conservationError) < 0.01;
  const inFlightValue = packetsRef.current.reduce((sum, p) => sum + p.current_value, 0);

  const feeRate = worldState?.current_fee_rate ?? 0;
  const mintingStatus = getMintingStatus(feeRate);
  const burningStatus = getBurningStatus(feeRate);
  const governanceLevel = getGovernanceLevel(worldState?.governance_quadrant ?? 'GOLDEN_ERA');

  const passFailMetrics = getPassFailMetrics();

  return (
    <div className="dashboard-container">
      <header className="header">
        <div className="title">
          <h1>THE ARENA</h1>
          <span className="subtitle">Diagnostic Twin v0.7.0</span>
        </div>
        <div className="governance-section">
          <div className="quadrant-badge">{worldState?.governance_quadrant || 'WAITING'}</div>
          <div className="status-badge">{worldState?.governance_status || 'STABLE'}</div>
          <div className="state-indicators">
            <span className="state-ind" style={{ color: mintingStatus.color }}>MINTING: {mintingStatus.label}</span>
            <span className="state-ind" style={{ color: burningStatus.color }}>BURNING: {burningStatus.label}</span>
            <span className="state-ind" style={{ color: governanceLevel.color }}>GOV: {governanceLevel.label}</span>
          </div>
        </div>
        <div className="global-stats">
          <div className="stat-card"><Activity size={16} /><div className="val">${totalRewards.toFixed(2)}<br /><label>REWARDS</label></div></div>
          <div className="stat-card"><TrendingDown size={16} /><div className="val">${totalBurned.toFixed(2)}<br /><label>BURNED</label></div></div>
          <div className="stat-card"><Zap size={16} /><div className="val">V:{worldState?.verification_complexity}<br /><label>PROOF</label></div></div>
          <div className="stat-card"><ShieldAlert size={16} /><div className="val">{((worldState?.ngauge_activity_index ?? 0) * 100).toFixed(1)}%<br /><label>NGAUGE</label></div></div>
          <div className="stat-card"><Package size={16} /><div className="val">{packetCount}<br /><label>PACKETS</label></div></div>
          <div className="stat-card"><div className="val">{worldState?.dissolved_count ?? 0}<br /><label>DISSOLVED</label></div></div>
        </div>
        <div className="controls-top">
          <select className="node-select" value={nodeCount} onChange={(e) => reinitializeEngine(Number(e.target.value))}>
            <option value={24}>24 Nodes</option>
            <option value={50}>50 Nodes</option>
            <option value={100}>100 Nodes</option>
            <option value={250}>250 Nodes</option>
            <option value={500}>500 Nodes</option>
          </select>
          <button className="btn-icon" onClick={() => setIsRunning(!isRunning)}>{isRunning ? <Pause /> : <Play />}</button>
          <button className="btn-icon" onClick={() => setLogs([])}><Trash2 /></button>
          <button className="btn-icon export-icon" onClick={exportData} title="Export Data"><Download size={18} /></button>
        </div>
      </header>

      <div className="main-grid">
        <div className="panel visualizer-panel">
          <canvas ref={canvasRef} width={800} height={600} className="visualizer-canvas"
            onClick={(e) => {
              const rect = canvasRef.current!.getBoundingClientRect();
              const cx = e.clientX - rect.left;
              const cy = e.clientY - rect.top;
              const closest = nodesRef.current.find(n => Math.sqrt((n.x! - cx) ** 2 + (n.y! - cy) ** 2) < 20);
              setSelectedNode(closest || null);
            }}
          />
          {selectedNode && (
            <div className="node-inspector">
              <h4>Node #{selectedNode.id} <span className="role-label">{ROLE_LABELS[selectedNode.role] ?? 'Unknown'}</span></h4>
              <div className="inspector-grid">
                <span>Fiat:</span> <strong>${selectedNode.inventory_fiat.toFixed(0)}</strong>
                <span>Crypto:</span> <strong>{selectedNode.inventory_crypto.toFixed(3)}</strong>
                <span>Queue:</span> <strong>{selectedNode.current_buffer_count}</strong>
                <span>Fees Earned:</span> <strong>${(selectedNode.total_fees_earned ?? 0).toFixed(2)}</strong>
                <span>Transit Fee:</span><strong>{((selectedNode.transit_fee ?? 0) * 100).toFixed(2)}%</strong>
                <span>Bandwidth:</span><strong>{(selectedNode.bandwidth ?? 0).toFixed(0)}</strong>
                <span>Latency:</span><strong>{(selectedNode.latency ?? 0).toFixed(1)}ms</strong>
                <span>Uptime:</span><strong>{((selectedNode.uptime ?? 0) * 100).toFixed(0)}%</strong>
                <span>Pressure:</span><strong>{(selectedNode.pressure ?? 0).toFixed(3)}</strong>
                {selectedNode.role === NodeRole.NGauge && (
                  <><span>Work:</span><strong>{(selectedNode.accumulated_work ?? 0).toFixed(1)}</strong></>
                )}
              </div>
              <button className="btn-sm" onClick={() => setSelectedNode(null)}>Close</button>
            </div>
          )}
        </div>

        <div className="right-column">
          {/* Tab Navigation */}
          <div className="tabs">
            <button className={`tab-btn ${activeTab === 'live' ? 'active' : ''}`} onClick={() => setActiveTab('live')}>LIVE</button>
            <button className={`tab-btn ${activeTab === 'bench' ? 'active' : ''}`} onClick={() => setActiveTab('bench')}>BENCH</button>
          </div>

          {activeTab === 'live' ? (
            <>
              {/* Conservation Panel with B2 enhancements */}
              <div className={`panel conservation-panel ${isConserved ? 'conserved' : 'leaked'}`}>
                <h3><DollarSign size={14} /> THERMODYNAMIC CONSERVATION</h3>
                <div className="conservation-header-row">
                  <div className={`conservation-status ${isConserved ? 'status-ok' : 'status-fail'}`}>
                    {isConserved ? 'CONSERVED' : 'LEAK DETECTED'}
                  </div>
                  <button className={`btn-sm halt-btn ${haltOnLeak ? 'halt-active' : ''}`}
                    onClick={() => setHaltOnLeak(!haltOnLeak)}>
                    {haltOnLeak ? 'HALT: ON' : 'HALT: OFF'}
                  </button>
                  <span className={`leak-trend trend-${leakTrend}`}>
                    {leakTrend === 'up' ? '\u2191' : leakTrend === 'down' ? '\u2193' : '\u2194'}
                  </span>
                </div>
                <div className="conservation-grid">
                  <span>TOTAL INPUT:</span> <strong>${totalSpawned.toFixed(2)}</strong>
                  <span>TOTAL OUTPUT:</span> <strong>${(totalSettled + totalRefunded).toFixed(2)}</strong>
                  <span>TOTAL FEES:</span> <strong>${totalFees.toFixed(2)}</strong>
                  <span>TOTAL BURNED:</span> <strong>${totalBurned.toFixed(2)}</strong>
                  <span>IN FLIGHT:</span> <strong>${inFlightValue.toFixed(2)}</strong>
                  <span>REWARDS DIST:</span> <strong>${totalRewards.toFixed(2)}</strong>
                  <span className="conservation-error-label">CONSERVATION ERROR:</span>
                  <strong className={Math.abs(conservationError) < 0.01 ? 'error-ok' : 'error-fail'}>{conservationError.toFixed(6)}</strong>
                </div>
              </div>

              {/* Governor Panel */}
              <div className="panel governor-panel">
                <h3><Zap size={14} /> PID GOVERNOR</h3>
                <div className="governor-grid">
                  <span>Quadrant:</span><strong style={{ color: governanceLevel.color }}>{worldState?.governance_quadrant ?? 'N/A'}</strong>
                  <span>Fee Rate:</span><strong>{(feeRate * 100).toFixed(3)}%</strong>
                  <span>Demurrage:</span><strong>{((worldState?.current_demurrage_rate ?? 0) * 100).toFixed(3)}%</strong>
                  <span>Surge:</span><strong>{(worldState?.surge_multiplier ?? 1).toFixed(2)}x</strong>
                  <span>Organic Ratio:</span><strong>{(worldState?.organic_ratio ?? 1).toFixed(3)}</strong>
                  <span>Circuit Breaker:</span>
                  <strong style={{ color: worldState?.circuit_breaker_active ? 'var(--accent-red)' : 'var(--accent-green)' }}>
                    {worldState?.circuit_breaker_active ? 'TRIPPED' : 'OK'}
                  </strong>
                </div>
                {/* Tier fee rates */}
                <div className="tier-fees">
                  <h4>Tier Fee Rates (cap)</h4>
                  <div className="tier-fee-row">
                    {[0, 1, 2, 3].map(t => (
                      <div key={t} className="tier-fee-cell" style={{ borderColor: TIER_COLORS[t] }}>
                        <span className="tier-label" style={{ color: TIER_COLORS[t] }}>{TIER_LABELS[t]}</span>
                        <span className="tier-rate">{((worldState?.tier_fee_rates?.[t] ?? 0) * 100).toFixed(3)}%</span>
                        <span className="tier-cap">cap: {TIER_FEE_CAPS[t]}%</span>
                      </div>
                    ))}
                  </div>
                </div>
              </div>

              {/* Effective Price Panel */}
              <div className="panel price-panel">
                <h3><DollarSign size={14} /> EFFECTIVE PRICE COMPOSITE</h3>
                <div className="price-grid">
                  <span>Gold Spot:</span><strong>${(worldState?.gold_price ?? 2600).toFixed(2)}</strong>
                  <span>Network Fees:</span><strong>+{((worldState?.network_fee_component ?? 0) * 100).toFixed(4)}%</strong>
                  <span>Speculation:</span><strong>+{((worldState?.speculation_component ?? 0) * 100).toFixed(4)}%</strong>
                  <span>Float Shadow:</span><strong>+{((worldState?.float_component ?? 0) * 100).toFixed(4)}%</strong>
                  <span className="composite-label">Effective Price:</span>
                  <strong className="composite-value">${(worldState?.effective_price_composite ?? worldState?.gold_price ?? 2600).toFixed(2)}</strong>
                </div>
                {/* Tier distribution */}
                <div className="tier-dist">
                  <h4>Active Packets by Tier</h4>
                  <div className="tier-dist-row">
                    {[0, 1, 2, 3].map(t => (
                      <div key={t} className="tier-dist-cell">
                        <span style={{ color: TIER_COLORS[t] }}>{TIER_LABELS[t]}</span>
                        <strong>{worldState?.tier_distribution?.[t] ?? 0}</strong>
                      </div>
                    ))}
                  </div>
                </div>
              </div>

              {/* Charts */}
              <div className="panel charts-panel">
                <div className="charts-grid">
                  <div className="chart-cell">
                    <h3>Peg Stability (Gold vs Deviation %)</h3>
                    <ResponsiveContainer width="100%" height={110}>
                      <LineChart data={metrics}>
                        <CartesianGrid strokeDasharray="3 3" stroke="#334155" />
                        <XAxis dataKey="tick" hide /><YAxis yAxisId="left" stroke="#f59e0b" fontSize={10} domain={['auto', 'auto']} />
                        <YAxis yAxisId="right" orientation="right" stroke="#ef4444" fontSize={10} domain={[-50, 50]} />
                        <Tooltip contentStyle={{ backgroundColor: '#1e293b', border: 'none', fontSize: '10px' }} />
                        <Line yAxisId="left" type="monotone" dataKey="gold" stroke="#f59e0b" dot={false} strokeWidth={2} name="Price" />
                        <Line yAxisId="right" type="monotone" dataKey="deviation" stroke="#ef4444" dot={false} strokeWidth={2} name="Dev %" />
                      </LineChart>
                    </ResponsiveContainer>
                  </div>
                  <div className="chart-cell">
                    <h3>Pressure (Spawn vs Settle Rate)</h3>
                    <ResponsiveContainer width="100%" height={110}>
                      <LineChart data={metrics}>
                        <CartesianGrid strokeDasharray="3 3" stroke="#334155" /><XAxis dataKey="tick" hide />
                        <YAxis stroke="#3b82f6" fontSize={10} />
                        <Tooltip contentStyle={{ backgroundColor: '#1e293b', border: 'none', fontSize: '10px' }} />
                        <Line type="monotone" dataKey="spawnRate" stroke="#3b82f6" dot={false} strokeWidth={2} name="Spawned" />
                        <Line type="monotone" dataKey="settleRate" stroke="#10b981" dot={false} strokeWidth={2} name="Settled" />
                      </LineChart>
                    </ResponsiveContainer>
                  </div>
                  <div className="chart-cell">
                    <h3>Fee Mean (%)</h3>
                    <ResponsiveContainer width="100%" height={110}>
                      <LineChart data={metrics}>
                        <CartesianGrid strokeDasharray="3 3" stroke="#334155" /><XAxis dataKey="tick" hide />
                        <YAxis stroke="#f59e0b" fontSize={10} />
                        <Tooltip contentStyle={{ backgroundColor: '#1e293b', border: 'none', fontSize: '10px' }} />
                        <Line type="monotone" dataKey="feeRate" stroke="#f59e0b" dot={false} strokeWidth={2} name="Fee %" />
                      </LineChart>
                    </ResponsiveContainer>
                  </div>
                  <div className="chart-cell">
                    <h3>Demurrage Burn (per tick)</h3>
                    <ResponsiveContainer width="100%" height={110}>
                      <LineChart data={metrics}>
                        <CartesianGrid strokeDasharray="3 3" stroke="#334155" /><XAxis dataKey="tick" hide />
                        <YAxis stroke="#ef4444" fontSize={10} />
                        <Tooltip contentStyle={{ backgroundColor: '#1e293b', border: 'none', fontSize: '10px' }} />
                        <Line type="monotone" dataKey="burnPerTick" stroke="#ef4444" dot={false} strokeWidth={2} name="Burn/tick" />
                      </LineChart>
                    </ResponsiveContainer>
                  </div>
                </div>
              </div>

              {/* Controls */}
              <div className="panel controls-panel">
                <div className="scenario-buttons">
                  <button className="btn success" onClick={() => { applyPreset('NORMAL_MARKET'); addLog('Preset: Normal Market', 'info'); }}>Normal</button>
                  <button className="btn primary" onClick={() => { applyPreset('BULL_RUN'); addLog('Preset: Bull Run', 'info'); }}>Bull</button>
                  <button className="btn danger" onClick={() => { applyPreset('BEAR_MARKET'); addLog('Preset: Bear Market', 'info'); }}>Bear</button>
                  <button className="btn warning" onClick={() => { applyPreset('BLACK_SWAN'); addLog('Preset: Black Swan', 'info'); }}>Swan</button>
                  <button className="btn muted" onClick={() => { applyPreset('STAGFLATION'); addLog('Preset: Stagflation', 'info'); }}>Stag</button>
                </div>
                <div className="slider-group">
                  <label>Gold: ${(worldState?.gold_price ?? 2600).toFixed(0)}</label>
                  <input type="range" min="1500" max="3500" value={worldState?.gold_price ?? 2600}
                    onChange={(e) => engine?.set_gold_price(Number(e.target.value))} />
                  <label>Demand: {((worldState?.demand_factor ?? 0.2) * 100).toFixed(0)}%</label>
                  <input type="range" min="0" max="1" step="0.01" value={worldState?.demand_factor ?? 0.2}
                    onChange={(e) => engine?.set_demand_factor(Number(e.target.value))} />
                  <label>Panic: {((worldState?.panic_level ?? 0) * 100).toFixed(0)}%</label>
                  <input type="range" min="0" max="1" step="0.01" value={worldState?.panic_level ?? 0}
                    onChange={(e) => engine?.set_panic_level(Number(e.target.value))} />
                  <label>Liquidity: {liquidityDepth}%</label>
                  <input type="range" min="0" max="100" value={liquidityDepth}
                    onChange={(e) => applyLiquidity(Number(e.target.value))} />
                  <label>Sim Tick: {playbackSpeed}ms</label>
                  <input type="range" min="16" max="500" value={playbackSpeed}
                    onChange={(e) => setPlaybackSpeed(Number(e.target.value))} />
                </div>
                <div className="spawn-controls">
                  <label><Info size={12} /> Manual Spawn</label>
                  <div className="spawn-row">
                    <input type="number" min="1" max="100000" value={spawnAmount}
                      onChange={(e) => setSpawnAmount(Number(e.target.value))} className="spawn-input" />
                    <button className="btn primary spawn-btn" onClick={spawnPacket}><Plus size={12} /> Spawn</button>
                  </div>
                </div>
              </div>

              {/* Logs */}
              <div className="panel logs-panel">
                <div className="logs-list">
                  {logs.map((log, i) => (
                    <div key={i} className={`log-entry ${log.type}`}>
                      <span className="tick">[{log.tick}]</span> {log.message}
                    </div>
                  ))}
                </div>
              </div>
            </>
          ) : (
            /* BENCH TAB */
            <div className="bench-scroll">
              {/* B1: Benchmark Runner */}
              <div className="panel bench-panel">
                <h3>AUTOMATED SCENARIO RUNNER (16 Scenarios)</h3>
                <div className="bench-category-tabs">
                  {(['all', 'market', 'stress', 'fiduciary'] as const).map(cat => (
                    <button key={cat} className={`btn-sm ${benchCategory === cat ? 'tab-active' : ''}`}
                      onClick={() => setBenchCategory(cat)}>
                      {cat.toUpperCase()} ({cat === 'all' ? SCENARIOS.length : SCENARIOS.filter(s => s.category === cat).length})
                    </button>
                  ))}
                </div>
                <div className="bench-controls">
                  <button className="btn primary bench-run-all" onClick={runAllBenchmarks} disabled={benchRunning}>
                    {benchRunning ? benchProgress : `RUN ${benchCategory === 'all' ? 'ALL' : benchCategory.toUpperCase()} SCENARIOS`}
                  </button>
                  <div className="bench-individual">
                    {SCENARIOS.filter(s => benchCategory === 'all' || s.category === benchCategory).map(s => (
                      <button key={s.name} className={`btn muted bench-single bench-cat-${s.category}`} disabled={benchRunning}
                        onClick={async () => {
                          setBenchRunning(true);
                          setBenchProgress(`Running: ${s.label}...`);
                          const result = await runBenchmarkScenario(s);
                          setBenchResults(prev => {
                            const filtered = prev.filter(r => r.scenario !== s.label);
                            return [...filtered, result];
                          });
                          setBenchRunning(false);
                          setBenchProgress('');
                        }}>
                        {s.label}
                      </button>
                    ))}
                  </div>
                </div>
                {benchResults.length > 0 && (
                  <div className="bench-table-wrap">
                    <table className="bench-table">
                      <thead>
                        <tr>
                          <th>Category</th><th>Scenario</th><th>Nodes</th><th>Ticks</th>
                          <th>Settled</th><th>Reverted</th><th>Dissolved</th>
                          <th>Avg Fee</th><th>Error</th><th>Peak Fee</th>
                          <th>Result</th>
                        </tr>
                      </thead>
                      <tbody>
                        {benchResults
                          .filter(r => benchCategory === 'all' || r.category === benchCategory)
                          .map((r, i) => {
                            const scenario = SCENARIOS.find(s => s.label === r.scenario);
                            return (
                              <tr key={i} className={`bench-result ${r.pass ? 'pass' : 'fail'} bench-cat-${r.category}`}>
                                <td><span className={`cat-badge cat-${r.category}`}>{r.category}</span></td>
                                <td>{r.scenario}</td>
                                <td>{scenario?.nodes ?? 24}</td>
                                <td>{r.ticks}</td>
                                <td>{r.settlementCount}</td>
                                <td>{r.revertCount}</td>
                                <td>{r.dissolvedCount}</td>
                                <td>{r.avgFee.toFixed(2)}%</td>
                                <td>{r.conservationError.toFixed(6)}</td>
                                <td>{r.peakFee.toFixed(2)}%</td>
                                <td>
                                  <span className={`metric-badge ${r.pass ? 'pass' : 'fail'}`}>{r.pass ? 'PASS' : 'FAIL'}</span>
                                  {r.category === 'fiduciary' && (
                                    <span className="fiduciary-badges">
                                      {r.settlementFinality && <span className="fid-badge">FIN</span>}
                                      {r.costCertainty && <span className="fid-badge">COST</span>}
                                      {r.auditTrail && <span className="fid-badge">AUDIT</span>}
                                    </span>
                                  )}
                                </td>
                              </tr>
                            );
                          })}
                      </tbody>
                    </table>
                  </div>
                )}
                {benchResults.length > 0 && (
                  <div className="bench-export-row">
                    <button className="btn primary" onClick={exportData}>
                      <Download size={14} /> Export Full Analysis ({benchResults.reduce((sum, r) => sum + (r.tickHistory?.length ?? 0), 0)} data points)
                    </button>
                  </div>
                )}
              </div>

              {/* B4: Pass/Fail Metrics Dashboard */}
              <div className="panel bench-panel">
                <h3>SPEC SUCCESS METRICS</h3>
                <div className="metrics-dashboard">
                  <div className="metric-card">
                    <span className="metric-label">No-Fail Clearance</span>
                    <span className={`metric-badge ${passFailMetrics.noFail ? 'pass' : 'fail'}`}>
                      {passFailMetrics.noFail ? 'PASS' : 'FAIL'}
                    </span>
                    <span className="metric-desc">(settled+reverted)/spawned &gt;= 99%</span>
                  </div>
                  <div className="metric-card">
                    <span className="metric-label">Peg Elasticity</span>
                    <span className={`metric-badge ${passFailMetrics.pegElasticity ? 'pass' : 'fail'}`}>
                      {passFailMetrics.pegElasticity ? 'PASS' : 'FAIL'}
                    </span>
                    <span className="metric-desc">|peg_dev| &lt; 20% for 95% of ticks</span>
                  </div>
                  <div className="metric-card">
                    <span className="metric-label">Incentive Alignment</span>
                    <span className={`metric-badge ${passFailMetrics.incentiveAlign ? 'pass' : 'fail'}`}>
                      {passFailMetrics.incentiveAlign ? 'PASS' : 'FAIL'}
                    </span>
                    <span className="metric-desc">Peak fee &gt; 5% during stress</span>
                  </div>
                  <div className="metric-card">
                    <span className="metric-label">Demurrage Efficiency</span>
                    <span className={`metric-badge ${passFailMetrics.demurrageEff ? 'pass' : 'fail'}`}>
                      {passFailMetrics.demurrageEff ? 'PASS' : 'FAIL'}
                    </span>
                    <span className="metric-desc">No packets orbiting &gt; 50 ticks</span>
                  </div>
                </div>
              </div>

              {/* B3: Statistical Summary */}
              {runStats && (
                <div className="panel bench-panel">
                  <h3>STATISTICAL SUMMARY</h3>
                  <div className="stats-grid">
                    <span>Total Ticks:</span><strong>{runStats.totalTicks}</strong>
                    <span>Spawned:</span><strong>{runStats.totalSpawned}</strong>
                    <span>Settled:</span><strong>{runStats.totalSettled}</strong>
                    <span>Reverted:</span><strong>{runStats.totalReverted}</strong>
                    <span>Orbiting:</span><strong>{runStats.totalOrbiting}</strong>
                    <span>Settlement Rate:</span><strong>{runStats.settlementRate.toFixed(1)}%</strong>
                    <span>Current Fee Rate:</span><strong>{runStats.avgFeeRate.toFixed(2)}%</strong>
                    <span>Peak Fee Rate:</span><strong>{runStats.peakFee.toFixed(2)}%</strong>
                    <span>Avg Time-to-Settle:</span><strong>{runStats.avgTimeToSettle.toFixed(1)} ticks</strong>
                    <span>Avg Hops:</span><strong>{runStats.avgHops.toFixed(1)}</strong>
                    <span>Conservation Error:</span><strong>{runStats.conservationError.toFixed(6)}</strong>
                    <span>Longest Orbit:</span><strong>{runStats.longestOrbit} ticks</strong>
                    <span>Dissolved:</span><strong>{worldState?.dissolved_count ?? 0}</strong>
                    <span>Held:</span><strong>{worldState?.held_count ?? 0}</strong>
                    <span>Circuit Breaker:</span>
                    <strong style={{ color: worldState?.circuit_breaker_active ? 'var(--accent-red)' : 'var(--accent-green)' }}>
                      {worldState?.circuit_breaker_active ? 'TRIPPED' : 'OK'}
                    </strong>
                  </div>
                </div>
              )}

              {/* B5: Node Economics Table */}
              <div className="panel bench-panel">
                <h3>
                  <button className="btn-sm toggle-btn" onClick={() => setShowNodeTable(!showNodeTable)}>
                    {showNodeTable ? 'HIDE' : 'SHOW'} NODE ECONOMICS
                  </button>
                </h3>
                {showNodeTable && (
                  <div className="node-table-wrap">
                    <table className="node-table">
                      <thead>
                        <tr>
                          {([
                            ['id', 'ID'], ['role', 'Role'], ['fees', 'Fees Earned'],
                            ['capacity', 'Capacity'], ['pressure', 'Pressure'], ['buffer', 'Buffer'], ['crypto', 'Crypto'], ['fiat', 'Fiat'],
                          ] as [SortKey, string][]).map(([key, label]) => (
                            <th key={key} onClick={() => toggleSort(key)} className="sortable-th">
                              {label} {sortBy === key ? (sortDir === 'asc' ? '\u25B2' : '\u25BC') : ''}
                            </th>
                          ))}
                        </tr>
                      </thead>
                      <tbody>
                        {getSortedNodes().map(n => (
                          <tr key={n.id} style={{ borderLeft: `3px solid ${ROLE_COLORS[n.role] ?? '#64748b'}` }}>
                            <td>{n.id}</td>
                            <td><span className="role-chip" style={{ background: ROLE_COLORS[n.role] }}>{ROLE_LABELS[n.role]}</span></td>
                            <td>${(n.total_fees_earned ?? 0).toFixed(2)}</td>
                            <td>{(n.bandwidth ?? 0).toFixed(0)}</td>
                            <td>{(n.pressure ?? 0).toFixed(3)}</td>
                            <td>{n.current_buffer_count}</td>
                            <td>{n.inventory_crypto.toFixed(3)}</td>
                            <td>${n.inventory_fiat.toFixed(0)}</td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  </div>
                )}
              </div>

              {/* B6: Packet Tracer */}
              <div className="panel bench-panel packet-tracer">
                <h3>PACKET LIFECYCLE TRACER</h3>
                <div className="trace-controls">
                  <input type="text" placeholder="Packet ID" value={tracePacketId}
                    onChange={e => setTracePacketId(e.target.value)} className="spawn-input" />
                  <button className="btn primary" onClick={tracePacket}><Search size={12} /> Trace</button>
                </div>
                {traceError && <div className="trace-error">{traceError}</div>}
                {tracedPacket && (
                  <div className="trace-result">
                    <div className="trace-grid">
                      <span>ID:</span><strong>{tracedPacket.id}</strong>
                      <span>Origin:</span><strong>Node #{tracedPacket.origin_node}</strong>
                      <span>Status:</span><strong>{PACKET_STATUS_LABELS[tracedPacket.status] ?? `Unknown(${tracedPacket.status})`}</strong>
                      <span>Original Value:</span><strong>${(tracedPacket.original_value ?? 0).toFixed(2)}</strong>
                      <span>Current Value:</span><strong>${tracedPacket.current_value.toFixed(2)}</strong>
                      <span>Value Decay:</span>
                      <strong className={tracedPacket.current_value < (tracedPacket.original_value ?? 0) * 0.9 ? 'error-fail' : 'error-ok'}>
                        {tracedPacket.original_value ? ((1 - tracedPacket.current_value / tracedPacket.original_value) * 100).toFixed(1) : '0'}%
                      </strong>
                      <span>Hops:</span><strong>{tracedPacket.hops ?? 'N/A'}</strong>
                      <span>Arrival Tick:</span><strong>{tracedPacket.arrival_tick}</strong>
                      <span>Tier:</span>
                      <strong style={{ color: TIER_COLORS[tracedPacket.tier ?? 0] }}>
                        {TIER_LABELS[tracedPacket.tier ?? 0]}
                      </strong>
                      <span>TTL:</span><strong>{tracedPacket.ttl ?? 'N/A'}</strong>
                      <span>Hop Limit:</span><strong>{tracedPacket.hop_limit ?? 'N/A'}</strong>
                      <span>Fee Budget:</span><strong>${(tracedPacket.fee_budget ?? 0).toFixed(4)}</strong>
                      <span>Fees Consumed:</span><strong>${(tracedPacket.fees_consumed ?? 0).toFixed(4)}</strong>
                    </div>
                    {tracedPacket.route_history && tracedPacket.route_history.length > 0 && (
                      <div className="trace-route">
                        <span className="trace-route-label">Route:</span>
                        {tracedPacket.route_history.map((nodeId, i) => (
                          <span key={i} className="trace-hop">
                            {i > 0 && ' \u2192 '}#{nodeId}
                          </span>
                        ))}
                      </div>
                    )}
                    {/* Value decay bar */}
                    <div className="decay-bar-container">
                      <div className="decay-bar" style={{
                        width: `${tracedPacket.original_value ? (tracedPacket.current_value / tracedPacket.original_value) * 100 : 100}%`,
                        background: tracedPacket.current_value < (tracedPacket.original_value ?? 0) * 0.5
                          ? 'var(--accent-red)' : 'var(--accent-green)',
                      }} />
                    </div>
                    {/* Fee budget bar */}
                    {tracedPacket.fee_budget && tracedPacket.fee_budget > 0 && (
                      <div className="fee-budget-bar-container">
                        <span className="fee-budget-label">Fee Budget: ${(tracedPacket.fees_consumed ?? 0).toFixed(2)} / ${tracedPacket.fee_budget.toFixed(2)}</span>
                        <div className="decay-bar-container">
                          <div className="decay-bar" style={{
                            width: `${((tracedPacket.fees_consumed ?? 0) / tracedPacket.fee_budget) * 100}%`,
                            background: (tracedPacket.fees_consumed ?? 0) > tracedPacket.fee_budget * 0.8
                              ? 'var(--accent-red)' : 'var(--accent-blue)',
                          }} />
                        </div>
                      </div>
                    )}
                  </div>
                )}
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

export default App;
