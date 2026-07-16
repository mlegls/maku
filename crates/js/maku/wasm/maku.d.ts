/* tslint:disable */
/* eslint-disable */

export class Maku {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Register a card file in the virtual filesystem (path → text).
     */
    add_file(path: string, text: string): void;
    basic_sprite_stride(): number;
    basic_sprites(): Uint8Array;
    boot(path: string, pattern?: string | null): void;
    /**
     * Build the pack frame once. Consume the zero-copy typed-array views
     * before the next mutating wasm call: another build reuses their backing
     * vectors, and any wasm-memory growth invalidates JavaScript views.
     */
    build_render_frame(): void;
    /**
     * Debug: pattern-scoped control cells as "name=value" lines (an
     * inspector view — cells are not part of the host game contract).
     */
    cells(): string;
    /**
     * Numeric channel ($lives, $boss-hp, $graze, …); NaN when absent.
     */
    channel_num(name: string): number;
    /**
     * [x, y] of a point-valued channel ($player, $boss, …), or empty.
     */
    channel_vec(name: string): Float32Array;
    /**
     * Command-tape ticks (orange markers on the slider).
     */
    cmd_ticks(): Float32Array;
    /**
     * Wire protocol (docs/player.md): run/swap/add/load/pattern/restart/
     * clear/seek/step/snapshots/resize-entities/pause/resume.
     */
    command(line: string): void;
    current_pattern(): string;
    draw_command_stride(): number;
    draw_commands(): Uint32Array;
    entity_count(): number;
    /**
     * Recent positioned events for effect flashes: [code, age_ticks, x, y]*
     * Event symbols are converted to this host's numeric effect ids here.
     * Stateless — they replay under scrubbing.
     */
    flashes(max_age: number): Float32Array;
    frame_abi_version(): number;
    graze(): number;
    hits(): number;
    iframes(): boolean;
    /**
     * Set a numeric input channel for subsequent steps ($move-x,
     * $p2-move-x, $focus-firing, $bomb — an open vocabulary, by name).
     */
    input_num(name: string, v: number): void;
    /**
     * Set a point input channel ($player mock, $nearest-enemy mock, …).
     */
    input_vec2(name: string, x: number, y: number): void;
    /**
     * Lives column via the $lives channel; -1 when absent.
     */
    lives(): number;
    material_address_u(index: number): number;
    material_address_v(index: number): number;
    material_blend(index: number): number;
    material_count(): number;
    material_fixed_color(index: number): number;
    material_key(index: number): string;
    material_layout(index: number): number;
    material_mag_filter(index: number): number;
    material_min_filter(index: number): number;
    material_pipeline(index: number): string;
    material_texture(index: number): number;
    constructor(rig?: string | null);
    /**
     * Newline-joined pattern menu.
     */
    patterns(): string;
    paused(): boolean;
    /**
     * [x, y] of the $player channel, or empty (sugar for channel_vec).
     */
    player_pos(): Float32Array;
    /**
     * [x, y]* of alive entities carrying a column (:pilot, :boss, or any
     * card-declared marker) — generic tagged-entity positions.
     */
    positions(col: string): Float32Array;
    recolor_sprite_stride(): number;
    recolor_sprites(): Uint8Array;
    restart(): void;
    running(): boolean;
    seek(tick: number): void;
    select(idx: number): void;
    status(): string;
    /**
     * Advance up to `n` ticks with the pending inputs (host accumulates
     * frame time; 120 ticks = 1 s).
     */
    step(n: number): void;
    strip_indices(): Uint32Array;
    strip_vertex_stride(): number;
    strip_vertices(): Uint8Array;
    texture_bytes(index: number): Uint8Array;
    texture_count(): number;
    texture_external_key(index: number): string;
    texture_height(index: number): number;
    texture_key(index: number): string;
    texture_width(index: number): number;
    tick(): number;
    /**
     * [tick, tape_len] — timeline extent for the scrub slider.
     */
    timeline(): Float32Array;
    tinted_sprite_stride(): number;
    tinted_sprites(): Uint8Array;
    toggle_pause(): void;
}

export function frameAbiVersion(): number;

export function makuVersion(): string;

export function sourceRevision(): string;

export function stdlibSource(name: string): string | undefined;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_maku_free: (a: number, b: number) => void;
    readonly frameAbiVersion: () => number;
    readonly makuVersion: () => [number, number];
    readonly maku_add_file: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly maku_basic_sprite_stride: (a: number) => number;
    readonly maku_basic_sprites: (a: number) => any;
    readonly maku_boot: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly maku_build_render_frame: (a: number) => [number, number];
    readonly maku_cells: (a: number) => [number, number];
    readonly maku_channel_num: (a: number, b: number, c: number) => number;
    readonly maku_channel_vec: (a: number, b: number, c: number) => [number, number];
    readonly maku_cmd_ticks: (a: number) => [number, number];
    readonly maku_command: (a: number, b: number, c: number) => void;
    readonly maku_current_pattern: (a: number) => [number, number];
    readonly maku_draw_command_stride: (a: number) => number;
    readonly maku_draw_commands: (a: number) => any;
    readonly maku_entity_count: (a: number) => number;
    readonly maku_flashes: (a: number, b: number) => [number, number];
    readonly maku_frame_abi_version: (a: number) => number;
    readonly maku_graze: (a: number) => number;
    readonly maku_hits: (a: number) => number;
    readonly maku_iframes: (a: number) => number;
    readonly maku_input_num: (a: number, b: number, c: number, d: number) => void;
    readonly maku_input_vec2: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly maku_lives: (a: number) => number;
    readonly maku_material_address_u: (a: number, b: number) => number;
    readonly maku_material_address_v: (a: number, b: number) => number;
    readonly maku_material_blend: (a: number, b: number) => number;
    readonly maku_material_count: (a: number) => number;
    readonly maku_material_fixed_color: (a: number, b: number) => number;
    readonly maku_material_key: (a: number, b: number) => [number, number];
    readonly maku_material_layout: (a: number, b: number) => number;
    readonly maku_material_mag_filter: (a: number, b: number) => number;
    readonly maku_material_min_filter: (a: number, b: number) => number;
    readonly maku_material_pipeline: (a: number, b: number) => [number, number];
    readonly maku_material_texture: (a: number, b: number) => number;
    readonly maku_new: (a: number, b: number) => number;
    readonly maku_patterns: (a: number) => [number, number];
    readonly maku_paused: (a: number) => number;
    readonly maku_player_pos: (a: number) => [number, number];
    readonly maku_positions: (a: number, b: number, c: number) => [number, number];
    readonly maku_recolor_sprite_stride: (a: number) => number;
    readonly maku_recolor_sprites: (a: number) => any;
    readonly maku_restart: (a: number) => void;
    readonly maku_running: (a: number) => number;
    readonly maku_seek: (a: number, b: number) => void;
    readonly maku_select: (a: number, b: number) => void;
    readonly maku_status: (a: number) => [number, number];
    readonly maku_step: (a: number, b: number) => void;
    readonly maku_strip_indices: (a: number) => any;
    readonly maku_strip_vertex_stride: (a: number) => number;
    readonly maku_strip_vertices: (a: number) => any;
    readonly maku_texture_bytes: (a: number, b: number) => any;
    readonly maku_texture_count: (a: number) => number;
    readonly maku_texture_external_key: (a: number, b: number) => [number, number];
    readonly maku_texture_height: (a: number, b: number) => number;
    readonly maku_texture_key: (a: number, b: number) => [number, number];
    readonly maku_texture_width: (a: number, b: number) => number;
    readonly maku_tick: (a: number) => number;
    readonly maku_timeline: (a: number) => [number, number];
    readonly maku_tinted_sprite_stride: (a: number) => number;
    readonly maku_tinted_sprites: (a: number) => any;
    readonly maku_toggle_pause: (a: number) => void;
    readonly sourceRevision: () => [number, number];
    readonly stdlibSource: (a: number, b: number) => [number, number];
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __externref_table_dealloc: (a: number) => void;
    readonly __wbindgen_start: () => void;
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
