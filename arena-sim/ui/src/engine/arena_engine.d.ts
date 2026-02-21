/* tslint:disable */
/* eslint-disable */

export class ArenaSimulation {
    free(): void;
    [Symbol.dispose](): void;
    get_nodes(): any;
    get_packet(packet_id: bigint): any;
    get_stats(): any;
    /**
     * Get node trust scores as array
     */
    get_trust_scores(): any;
    kill_node(node_id: number): void;
    constructor(node_count: number);
    /**
     * Reset simulation to initial state
     */
    reset(): void;
    /**
     * Run N ticks without returning results (fast batch mode for benchmarking)
     */
    run_batch(ticks: number): void;
    set_demand_factor(val: number): void;
    set_gold_price(val: number): void;
    set_node_crypto(node_id: number, val: number): void;
    set_panic_level(val: number): void;
    spawn_packet(node_id: number, amount: number): bigint;
    tick(): any;
}

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_arenasimulation_free: (a: number, b: number) => void;
    readonly arenasimulation_new: (a: number) => number;
    readonly arenasimulation_tick: (a: number) => number;
    readonly arenasimulation_spawn_packet: (a: number, b: number, c: number) => bigint;
    readonly arenasimulation_get_nodes: (a: number) => number;
    readonly arenasimulation_set_gold_price: (a: number, b: number) => void;
    readonly arenasimulation_set_demand_factor: (a: number, b: number) => void;
    readonly arenasimulation_set_panic_level: (a: number, b: number) => void;
    readonly arenasimulation_get_stats: (a: number) => number;
    readonly arenasimulation_kill_node: (a: number, b: number) => void;
    readonly arenasimulation_get_packet: (a: number, b: bigint) => number;
    readonly arenasimulation_run_batch: (a: number, b: number) => void;
    readonly arenasimulation_set_node_crypto: (a: number, b: number, c: number) => void;
    readonly arenasimulation_reset: (a: number) => void;
    readonly arenasimulation_get_trust_scores: (a: number) => number;
    readonly __wbindgen_export: (a: number, b: number, c: number) => void;
    readonly __wbindgen_export2: (a: number, b: number) => number;
    readonly __wbindgen_export3: (a: number, b: number, c: number, d: number) => number;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
