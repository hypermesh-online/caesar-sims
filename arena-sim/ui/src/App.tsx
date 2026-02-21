import { useEffect, useState, useRef, useCallback } from 'react';
import init, { ArenaSimulation } from './engine/arena_engine.js';
import { LineChart, Line, XAxis, YAxis, Tooltip, ResponsiveContainer, CartesianGrid } from 'recharts';
import { Play, Pause, Activity, DollarSign, Zap, ShieldAlert, Trash2, TrendingDown, Info, Package, Plus } from 'lucide-react';
import './App.css';

const NodeRole = { Ingress: 0, Egress: 1, Transit: 2, NGauge: 3 } as const;
type NodeRole = typeof NodeRole[keyof typeof NodeRole];

const ROLE_LABELS: Record<number, string> = {
  [NodeRole.Ingress]: 'Ingress',
  [NodeRole.Egress]: 'Egress',
  [NodeRole.Transit]: 'Transit',
  [NodeRole.NGauge]: 'NGauge',
};

interface Node {
  id: number; role: NodeRole; inventory_fiat: number; inventory_crypto: number;
  current_buffer_count: number; neighbors: number[]; x?: number; y?: number;
  total_fees_earned?: number; trust_score?: number; accumulated_work?: number;
}

interface TickResult { state: WorldState; active_packets: Packet[]; node_updates: NodeUpdate[]; }
interface NodeUpdate { id: number; buffer_count: number; inventory_fiat: number; inventory_crypto: number; }
interface Packet {
  id: number; status: number; current_value: number; origin_node: number;
  target_node?: number; arrival_tick: number; original_value?: number;
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
}

interface MetricPoint {
  tick: number; gold: number; velocity: number; deviation: number;
  fees: number; burn: number; feeRate: number; burnPerTick: number;
  spawnRate: number; settleRate: number;
}

interface LogEntry { tick: number; message: string; type: 'info' | 'warn' | 'error'; }

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

  const canvasRef = useRef<HTMLCanvasElement>(null);
  const nodesRef = useRef<Node[]>([]);
  const packetsRef = useRef<Packet[]>([]);
  const tickRef = useRef(0);
  const prevQuadrantRef = useRef<string>('');
  const prevFeeRateRef = useRef<number>(0);
  const prevBurnRef = useRef<number>(0);
  const prevSpawnedRef = useRef<number>(0);
  const prevSettledRef = useRef<number>(0);

  // Suppress unused-var lint for icons referenced only in JSX
  void DollarSign; void Info; void Package; void Plus;

  useEffect(() => {
    init().then(() => {
      const sim = new ArenaSimulation(24);
      setEngine(sim);
      const initialNodes = sim.get_nodes();
      setNodes(initialNodes);
      nodesRef.current = initialNodes;
      addLog('Simulation v0.5.0: Hydraulic Governor Refactor', 'info');
    });
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const addLog = useCallback((message: string, type: 'info' | 'warn' | 'error' = 'info') => {
    setLogs(prev => [{ tick: tickRef.current, message, type }, ...prev].slice(0, 50));
  }, []);

  const applyPreset = (name: string) => {
    if (!engine) return;
    switch (name) {
      case 'PAX_ROMANA': engine.set_gold_price(2600); engine.set_demand_factor(0.2); engine.set_panic_level(0.0); break;
      case 'FIREHOSE': engine.set_gold_price(2600); engine.set_demand_factor(0.9); engine.set_panic_level(0.1); break;
      case 'BANK_RUN': engine.set_gold_price(2000); engine.set_demand_factor(0.5); engine.set_panic_level(0.9); break;
      case 'FLASH_CRASH': engine.set_gold_price(2000); engine.set_demand_factor(0.8); engine.set_panic_level(0.3); break;
      case 'DROUGHT': engine.set_gold_price(2600); engine.set_demand_factor(0.05); engine.set_panic_level(0.0); break;
    }
    addLog(`Preset applied: ${name}`, 'info');
  };

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

        result.node_updates.forEach(u => {
          const n = nodesRef.current[u.id];
          if (n) {
            n.current_buffer_count = u.buffer_count;
            n.inventory_fiat = u.inventory_fiat;
            n.inventory_crypto = u.inventory_crypto;
          }
        });

        if (state.current_tick % 5 === 0) {
          setWorldState(state);

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

          // Governance transition logging
          const currentQuadrant = state.governance_quadrant;
          if (prevQuadrantRef.current && currentQuadrant !== prevQuadrantRef.current) {
            addLog(`Governance shift: ${prevQuadrantRef.current} -> ${currentQuadrant}`, 'warn');
          }
          prevQuadrantRef.current = currentQuadrant;

          // Fee rate spike logging (>5% relative change)
          const feeRateDelta = Math.abs(state.current_fee_rate - prevFeeRateRef.current);
          if (prevFeeRateRef.current > 0 && feeRateDelta / prevFeeRateRef.current > 0.05) {
            addLog(
              `Fee rate spike: ${(state.current_fee_rate * 100).toFixed(2)}% (delta ${(feeRateDelta * 100).toFixed(2)}%)`,
              'warn',
            );
          }
          prevFeeRateRef.current = state.current_fee_rate;

          // Leak detection logging
          if (Math.abs(state.total_value_leaked) >= 0.01) {
            addLog(`LEAK DETECTED: ${state.total_value_leaked.toFixed(4)} value unaccounted`, 'error');
          }

          // Periodic settlement summary every 50 ticks
          if (state.current_tick % 50 === 0 && state.current_tick > 0) {
            addLog(
              `Tick ${state.current_tick}: ${result.active_packets.length} pkts, fee=${(state.current_fee_rate * 100).toFixed(1)}%`,
              'info',
            );
          }
        }
        lastTickTime = now;
      }
      draw();
      rafId = requestAnimationFrame(loop);
    };
    rafId = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(rafId);
  }, [engine, isRunning, playbackSpeed, addLog]);

  const draw = () => {
    const canvas = canvasRef.current; if (!canvas) return;
    const ctx = canvas.getContext('2d', { alpha: false }); if (!ctx) return;
    const curNodes = nodesRef.current;
    const packets = packetsRef.current;
    const curTick = tickRef.current;

    ctx.fillStyle = '#0f172a'; ctx.fillRect(0, 0, canvas.width, canvas.height);
    
    // Matrix Layout Calculation (6x4 Grid)
    const gridPadding = 80;
    const gridWidth = 6;
    const gridHeight = 4;
    const cellWidth = (canvas.width - gridPadding * 2) / (gridWidth - 1);
    const cellHeight = (canvas.height - gridPadding * 2) / (gridHeight - 1);

    curNodes.forEach((node, i) => {
      const col = i % gridWidth;
      const row = Math.floor(i / gridWidth);
      node.x = gridPadding + col * cellWidth;
      node.y = gridPadding + row * cellHeight;
    });

    // Enhanced edges: thickness and color based on buffer congestion
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

    curNodes.forEach((node) => {
      if (!node.x || !node.y) return;
      ctx.beginPath();
      ctx.arc(node.x, node.y, 8 + (node.current_buffer_count * 1.5), 0, Math.PI * 2);
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
      ctx.fillStyle = '#fff'; ctx.font = '10px Inter';
      ctx.fillText(node.id.toString(), node.x - 3, node.y + 3);
    });

    // Packet rendering with value-decay coloring
    packets.forEach(p => {
      const origin = curNodes[p.origin_node];
      const target = p.target_node !== undefined ? curNodes[p.target_node] : null;
      if (origin && origin.x && origin.y) {
        const origVal = p.original_value ?? 1000;
        const vRatio = origVal > 0 ? Math.max(0, Math.min(1, p.current_value / origVal)) : 0.5;
        const r = Math.floor(255 * (1 - vRatio));
        const g = Math.floor(255 * vRatio);
        ctx.fillStyle = `rgb(${r}, ${g}, 100)`;

        ctx.beginPath();
        if (p.status === 4 && target && target.x && target.y) {
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
      }
    });
  };

  // Conservation invariant calculations
  const totalFees = worldState?.total_fees_collected ?? 0;
  const totalBurned = worldState?.total_demurrage_burned ?? 0;
  const totalRewards = (worldState?.total_rewards_egress ?? 0) + (worldState?.total_rewards_transit ?? 0);
  const totalSpawned = worldState?.total_input ?? 0;
  const totalSettled = worldState?.total_output ?? 0;
  const totalRefunded = 0; // Refunds are included in total_output
  const conservationError = worldState?.total_value_leaked ?? 0;
  const isConserved = Math.abs(conservationError) < 0.01;

  const inFlightValue = packetsRef.current.reduce((sum, p) => sum + p.current_value, 0);

  // State indicator values
  const feeRate = worldState?.current_fee_rate ?? 0;
  const mintingStatus = getMintingStatus(feeRate);
  const burningStatus = getBurningStatus(feeRate);
  const governanceLevel = getGovernanceLevel(worldState?.governance_quadrant ?? 'GOLDEN_ERA');

  return (
    <div className="dashboard-container">
      <header className="header">
        <div className="title">
          <h1>THE ARENA</h1>
          <span className="subtitle">Diagnostic Twin v0.5.0</span>
        </div>
        <div className="governance-section">
          <div className="quadrant-badge">{worldState?.governance_quadrant || 'WAITING'}</div>
          <div className="status-badge">{worldState?.governance_status || 'STABLE'}</div>
          <div className="state-indicators">
            <span className="state-ind" style={{ color: mintingStatus.color }}>
              MINTING: {mintingStatus.label}
            </span>
            <span className="state-ind" style={{ color: burningStatus.color }}>
              BURNING: {burningStatus.label}
            </span>
            <span className="state-ind" style={{ color: governanceLevel.color }}>
              GOV: {governanceLevel.label}
            </span>
          </div>
        </div>
        <div className="global-stats">
          <div className="stat-card">
            <Activity size={16} />
            <div className="val">${totalRewards.toFixed(2)}<br /><label>REWARDS</label></div>
          </div>
          <div className="stat-card">
            <TrendingDown size={16} />
            <div className="val">${totalBurned.toFixed(2)}<br /><label>BURNED</label></div>
          </div>
          <div className="stat-card">
            <Zap size={16} />
            <div className="val">V:{worldState?.verification_complexity}<br /><label>PROOF</label></div>
          </div>
          <div className="stat-card">
            <ShieldAlert size={16} />
            <div className="val">
              {((worldState?.ngauge_activity_index ?? 0) * 100).toFixed(1)}%
              <br /><label>NGAUGE</label>
            </div>
          </div>
          <div className="stat-card">
            <Package size={16} />
            <div className="val">{packetCount}<br /><label>PACKETS</label></div>
          </div>
        </div>
        <div className="controls-top">
          <button className="btn-icon" onClick={() => setIsRunning(!isRunning)}>
            {isRunning ? <Pause /> : <Play />}
          </button>
          <button className="btn-icon" onClick={() => setLogs([])}>
            <Trash2 />
          </button>
        </div>
      </header>

      <div className="main-grid">
        <div className="panel visualizer-panel">
          <canvas
            ref={canvasRef}
            width={800}
            height={600}
            className="visualizer-canvas"
            onClick={(e) => {
              const rect = canvasRef.current!.getBoundingClientRect();
              const cx = e.clientX - rect.left;
              const cy = e.clientY - rect.top;
              const closest = nodesRef.current.find(
                n => Math.sqrt((n.x! - cx) ** 2 + (n.y! - cy) ** 2) < 20,
              );
              setSelectedNode(closest || null);
            }}
          />
          {selectedNode && (
            <div className="node-inspector">
              <h4>
                Node #{selectedNode.id}{' '}
                <span className="role-label">{ROLE_LABELS[selectedNode.role] ?? 'Unknown'}</span>
              </h4>
              <div className="inspector-grid">
                <span>Fiat:</span> <strong>${selectedNode.inventory_fiat.toFixed(0)}</strong>
                <span>Crypto:</span> <strong>{selectedNode.inventory_crypto.toFixed(3)}</strong>
                <span>Queue:</span> <strong>{selectedNode.current_buffer_count}</strong>
                <span>Fees Earned:</span> <strong>${(selectedNode.total_fees_earned ?? 0).toFixed(2)}</strong>
                <span>Trust:</span> <strong>{(selectedNode.trust_score ?? 0).toFixed(3)}</strong>
                {selectedNode.role === NodeRole.NGauge && (
                  <>
                    <span>Work:</span>
                    <strong>{(selectedNode.accumulated_work ?? 0).toFixed(1)}</strong>
                  </>
                )}
              </div>
              <button className="btn-sm" onClick={() => setSelectedNode(null)}>Close</button>
            </div>
          )}
        </div>

        <div className="right-column">
          {/* U1: Conservation Invariant Monitor */}
          <div className={`panel conservation-panel ${isConserved ? 'conserved' : 'leaked'}`}>
            <h3>
              <DollarSign size={14} /> THERMODYNAMIC CONSERVATION
            </h3>
            <div className={`conservation-status ${isConserved ? 'status-ok' : 'status-fail'}`}>
              {isConserved ? 'CONSERVED' : 'LEAK DETECTED'}
            </div>
            <div className="conservation-grid">
              <span>TOTAL INPUT:</span> <strong>${totalSpawned.toFixed(2)}</strong>
              <span>TOTAL OUTPUT:</span> <strong>${(totalSettled + totalRefunded).toFixed(2)}</strong>
              <span>TOTAL FEES:</span> <strong>${totalFees.toFixed(2)}</strong>
              <span>TOTAL BURNED:</span> <strong>${totalBurned.toFixed(2)}</strong>
              <span>IN FLIGHT:</span> <strong>${inFlightValue.toFixed(2)}</strong>
              <span>REWARDS DIST:</span> <strong>${totalRewards.toFixed(2)}</strong>
              <span className="conservation-error-label">CONSERVATION ERROR:</span>
              <strong className={Math.abs(conservationError) < 0.01 ? 'error-ok' : 'error-fail'}>
                {conservationError.toFixed(6)}
              </strong>
            </div>
          </div>

          {/* U4: Enhanced Charts - 2x2 Grid */}
          <div className="panel charts-panel">
            <div className="charts-grid">
              <div className="chart-cell">
                <h3>Peg Stability (Gold vs Deviation %)</h3>
                <ResponsiveContainer width="100%" height={110}>
                  <LineChart data={metrics}>
                    <CartesianGrid strokeDasharray="3 3" stroke="#334155" />
                    <XAxis dataKey="tick" hide />
                    <YAxis yAxisId="left" stroke="#f59e0b" fontSize={10} domain={['auto', 'auto']} />
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
                    <CartesianGrid strokeDasharray="3 3" stroke="#334155" />
                    <XAxis dataKey="tick" hide />
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
                    <CartesianGrid strokeDasharray="3 3" stroke="#334155" />
                    <XAxis dataKey="tick" hide />
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
                    <CartesianGrid strokeDasharray="3 3" stroke="#334155" />
                    <XAxis dataKey="tick" hide />
                    <YAxis stroke="#ef4444" fontSize={10} />
                    <Tooltip contentStyle={{ backgroundColor: '#1e293b', border: 'none', fontSize: '10px' }} />
                    <Line type="monotone" dataKey="burnPerTick" stroke="#ef4444" dot={false} strokeWidth={2} name="Burn/tick" />
                  </LineChart>
                </ResponsiveContainer>
              </div>
            </div>
          </div>

          {/* Controls Panel */}
          <div className="panel controls-panel">
            <div className="scenario-buttons">
              <button className="btn success" onClick={() => applyPreset('PAX_ROMANA')}>Pax</button>
              <button className="btn primary" onClick={() => applyPreset('FIREHOSE')}>Fire</button>
              <button className="btn danger" onClick={() => applyPreset('BANK_RUN')}>Bank</button>
              <button className="btn warning" onClick={() => applyPreset('FLASH_CRASH')}>Crash</button>
              <button className="btn muted" onClick={() => applyPreset('DROUGHT')}>Drought</button>
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
              <label>Sim Tick: {playbackSpeed}ms</label>
              <input type="range" min="16" max="500" value={playbackSpeed}
                onChange={(e) => setPlaybackSpeed(Number(e.target.value))} />
            </div>
            <div className="spawn-controls">
              <label><Info size={12} /> Manual Spawn</label>
              <div className="spawn-row">
                <input
                  type="number"
                  min="1"
                  max="100000"
                  value={spawnAmount}
                  onChange={(e) => setSpawnAmount(Number(e.target.value))}
                  className="spawn-input"
                />
                <button className="btn primary spawn-btn" onClick={spawnPacket}>
                  <Plus size={12} /> Spawn
                </button>
              </div>
            </div>
          </div>

          {/* Log Panel */}
          <div className="panel logs-panel">
            <div className="logs-list">
              {logs.map((log, i) => (
                <div key={i} className={`log-entry ${log.type}`}>
                  <span className="tick">[{log.tick}]</span> {log.message}
                </div>
              ))}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

export default App;
