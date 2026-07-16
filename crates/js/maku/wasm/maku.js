/* @ts-self-types="./maku.d.ts" */

export class Maku {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        MakuFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_maku_free(ptr, 0);
    }
    /**
     * Register a card file in the virtual filesystem (path → text).
     * @param {string} path
     * @param {string} text
     */
    add_file(path, text) {
        const ptr0 = passStringToWasm0(path, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ptr1 = passStringToWasm0(text, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len1 = WASM_VECTOR_LEN;
        wasm.maku_add_file(this.__wbg_ptr, ptr0, len0, ptr1, len1);
    }
    /**
     * @returns {number}
     */
    basic_sprite_stride() {
        const ret = wasm.maku_basic_sprite_stride(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {Uint8Array}
     */
    basic_sprites() {
        const ret = wasm.maku_basic_sprites(this.__wbg_ptr);
        return ret;
    }
    /**
     * @param {string} path
     * @param {string | null} [pattern]
     */
    boot(path, pattern) {
        const ptr0 = passStringToWasm0(path, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        var ptr1 = isLikeNone(pattern) ? 0 : passStringToWasm0(pattern, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        var len1 = WASM_VECTOR_LEN;
        wasm.maku_boot(this.__wbg_ptr, ptr0, len0, ptr1, len1);
    }
    /**
     * Build the pack frame once. Consume the zero-copy typed-array views
     * before the next mutating wasm call: another build reuses their backing
     * vectors, and any wasm-memory growth invalidates JavaScript views.
     */
    build_render_frame() {
        const ret = wasm.maku_build_render_frame(this.__wbg_ptr);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * Debug: pattern-scoped control cells as "name=value" lines (an
     * inspector view — cells are not part of the host game contract).
     * @returns {string}
     */
    cells() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.maku_cells(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Numeric channel ($lives, $boss-hp, $graze, …); NaN when absent.
     * @param {string} name
     * @returns {number}
     */
    channel_num(name) {
        const ptr0 = passStringToWasm0(name, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.maku_channel_num(this.__wbg_ptr, ptr0, len0);
        return ret;
    }
    /**
     * [x, y] of a point-valued channel ($player, $boss, …), or empty.
     * @param {string} name
     * @returns {Float32Array}
     */
    channel_vec(name) {
        const ptr0 = passStringToWasm0(name, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.maku_channel_vec(this.__wbg_ptr, ptr0, len0);
        var v2 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v2;
    }
    /**
     * Command-tape ticks (orange markers on the slider).
     * @returns {Float32Array}
     */
    cmd_ticks() {
        const ret = wasm.maku_cmd_ticks(this.__wbg_ptr);
        var v1 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * Wire protocol (docs/player.md): run/swap/add/load/pattern/restart/
     * clear/seek/step/snapshots/resize-entities/pause/resume.
     * @param {string} line
     */
    command(line) {
        const ptr0 = passStringToWasm0(line, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.maku_command(this.__wbg_ptr, ptr0, len0);
    }
    /**
     * @returns {string}
     */
    current_pattern() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.maku_current_pattern(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * @returns {number}
     */
    draw_command_stride() {
        const ret = wasm.maku_draw_command_stride(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {Uint32Array}
     */
    draw_commands() {
        const ret = wasm.maku_draw_commands(this.__wbg_ptr);
        return ret;
    }
    /**
     * @returns {number}
     */
    entity_count() {
        const ret = wasm.maku_entity_count(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Recent positioned events for effect flashes: [code, age_ticks, x, y]*
     * Event symbols are converted to this host's numeric effect ids here.
     * Stateless — they replay under scrubbing.
     * @param {number} max_age
     * @returns {Float32Array}
     */
    flashes(max_age) {
        const ret = wasm.maku_flashes(this.__wbg_ptr, max_age);
        var v1 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * @returns {number}
     */
    frame_abi_version() {
        const ret = wasm.maku_frame_abi_version(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    graze() {
        const ret = wasm.maku_graze(this.__wbg_ptr);
        return ret;
    }
    /**
     * @returns {number}
     */
    hits() {
        const ret = wasm.maku_hits(this.__wbg_ptr);
        return ret;
    }
    /**
     * @returns {boolean}
     */
    iframes() {
        const ret = wasm.maku_iframes(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * Set a numeric input channel for subsequent steps ($move-x,
     * $p2-move-x, $focus-firing, $bomb — an open vocabulary, by name).
     * @param {string} name
     * @param {number} v
     */
    input_num(name, v) {
        const ptr0 = passStringToWasm0(name, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.maku_input_num(this.__wbg_ptr, ptr0, len0, v);
    }
    /**
     * Set a point input channel ($player mock, $nearest-enemy mock, …).
     * @param {string} name
     * @param {number} x
     * @param {number} y
     */
    input_vec2(name, x, y) {
        const ptr0 = passStringToWasm0(name, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.maku_input_vec2(this.__wbg_ptr, ptr0, len0, x, y);
    }
    /**
     * Lives column via the $lives channel; -1 when absent.
     * @returns {number}
     */
    lives() {
        const ret = wasm.maku_lives(this.__wbg_ptr);
        return ret;
    }
    /**
     * @param {number} index
     * @returns {number}
     */
    material_address_u(index) {
        const ret = wasm.maku_material_address_u(this.__wbg_ptr, index);
        return ret >>> 0;
    }
    /**
     * @param {number} index
     * @returns {number}
     */
    material_address_v(index) {
        const ret = wasm.maku_material_address_v(this.__wbg_ptr, index);
        return ret >>> 0;
    }
    /**
     * @param {number} index
     * @returns {number}
     */
    material_blend(index) {
        const ret = wasm.maku_material_blend(this.__wbg_ptr, index);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    material_count() {
        const ret = wasm.maku_material_count(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} index
     * @returns {number}
     */
    material_fixed_color(index) {
        const ret = wasm.maku_material_fixed_color(this.__wbg_ptr, index);
        return ret >>> 0;
    }
    /**
     * @param {number} index
     * @returns {string}
     */
    material_key(index) {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.maku_material_key(this.__wbg_ptr, index);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * @param {number} index
     * @returns {number}
     */
    material_layout(index) {
        const ret = wasm.maku_material_layout(this.__wbg_ptr, index);
        return ret >>> 0;
    }
    /**
     * @param {number} index
     * @returns {number}
     */
    material_mag_filter(index) {
        const ret = wasm.maku_material_mag_filter(this.__wbg_ptr, index);
        return ret >>> 0;
    }
    /**
     * @param {number} index
     * @returns {number}
     */
    material_min_filter(index) {
        const ret = wasm.maku_material_min_filter(this.__wbg_ptr, index);
        return ret >>> 0;
    }
    /**
     * @param {number} index
     * @returns {string}
     */
    material_pipeline(index) {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.maku_material_pipeline(this.__wbg_ptr, index);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * @param {number} index
     * @returns {number}
     */
    material_texture(index) {
        const ret = wasm.maku_material_texture(this.__wbg_ptr, index);
        return ret >>> 0;
    }
    /**
     * @param {string | null} [rig]
     */
    constructor(rig) {
        var ptr0 = isLikeNone(rig) ? 0 : passStringToWasm0(rig, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        var len0 = WASM_VECTOR_LEN;
        const ret = wasm.maku_new(ptr0, len0);
        this.__wbg_ptr = ret;
        MakuFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * Newline-joined pattern menu.
     * @returns {string}
     */
    patterns() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.maku_patterns(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * @returns {boolean}
     */
    paused() {
        const ret = wasm.maku_paused(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * [x, y] of the $player channel, or empty (sugar for channel_vec).
     * @returns {Float32Array}
     */
    player_pos() {
        const ret = wasm.maku_player_pos(this.__wbg_ptr);
        var v1 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * [x, y]* of alive entities carrying a column (:pilot, :boss, or any
     * card-declared marker) — generic tagged-entity positions.
     * @param {string} col
     * @returns {Float32Array}
     */
    positions(col) {
        const ptr0 = passStringToWasm0(col, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.maku_positions(this.__wbg_ptr, ptr0, len0);
        var v2 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v2;
    }
    /**
     * @returns {number}
     */
    recolor_sprite_stride() {
        const ret = wasm.maku_recolor_sprite_stride(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {Uint8Array}
     */
    recolor_sprites() {
        const ret = wasm.maku_recolor_sprites(this.__wbg_ptr);
        return ret;
    }
    /**
     * Deduplicated profile fallback diagnostics from the latest and prior
     * frame builds. One line per unknown style/color encountered.
     * @returns {string}
     */
    render_diagnostics() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.maku_render_diagnostics(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    restart() {
        wasm.maku_restart(this.__wbg_ptr);
    }
    /**
     * @returns {boolean}
     */
    running() {
        const ret = wasm.maku_running(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * @param {number} tick
     */
    seek(tick) {
        wasm.maku_seek(this.__wbg_ptr, tick);
    }
    /**
     * @param {number} idx
     */
    select(idx) {
        wasm.maku_select(this.__wbg_ptr, idx);
    }
    /**
     * @returns {string}
     */
    status() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.maku_status(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Advance up to `n` ticks with the pending inputs (host accumulates
     * frame time; 120 ticks = 1 s).
     * @param {number} n
     */
    step(n) {
        wasm.maku_step(this.__wbg_ptr, n);
    }
    /**
     * @returns {Uint32Array}
     */
    strip_indices() {
        const ret = wasm.maku_strip_indices(this.__wbg_ptr);
        return ret;
    }
    /**
     * @returns {number}
     */
    strip_vertex_stride() {
        const ret = wasm.maku_strip_vertex_stride(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {Uint8Array}
     */
    strip_vertices() {
        const ret = wasm.maku_strip_vertices(this.__wbg_ptr);
        return ret;
    }
    /**
     * @param {number} index
     * @returns {Uint8Array}
     */
    texture_bytes(index) {
        const ret = wasm.maku_texture_bytes(this.__wbg_ptr, index);
        return ret;
    }
    /**
     * @returns {number}
     */
    texture_count() {
        const ret = wasm.maku_texture_count(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @param {number} index
     * @returns {string}
     */
    texture_external_key(index) {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.maku_texture_external_key(this.__wbg_ptr, index);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * @param {number} index
     * @returns {number}
     */
    texture_height(index) {
        const ret = wasm.maku_texture_height(this.__wbg_ptr, index);
        return ret >>> 0;
    }
    /**
     * @param {number} index
     * @returns {string}
     */
    texture_key(index) {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.maku_texture_key(this.__wbg_ptr, index);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * @param {number} index
     * @returns {number}
     */
    texture_width(index) {
        const ret = wasm.maku_texture_width(this.__wbg_ptr, index);
        return ret >>> 0;
    }
    /**
     * @returns {number}
     */
    tick() {
        const ret = wasm.maku_tick(this.__wbg_ptr);
        return ret;
    }
    /**
     * [tick, tape_len] — timeline extent for the scrub slider.
     * @returns {Float32Array}
     */
    timeline() {
        const ret = wasm.maku_timeline(this.__wbg_ptr);
        var v1 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * @returns {number}
     */
    tinted_sprite_stride() {
        const ret = wasm.maku_tinted_sprite_stride(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {Uint8Array}
     */
    tinted_sprites() {
        const ret = wasm.maku_tinted_sprites(this.__wbg_ptr);
        return ret;
    }
    toggle_pause() {
        wasm.maku_toggle_pause(this.__wbg_ptr);
    }
}
if (Symbol.dispose) Maku.prototype[Symbol.dispose] = Maku.prototype.free;

/**
 * @returns {number}
 */
export function frameAbiVersion() {
    const ret = wasm.frameAbiVersion();
    return ret >>> 0;
}

/**
 * @returns {string}
 */
export function makuVersion() {
    let deferred1_0;
    let deferred1_1;
    try {
        const ret = wasm.makuVersion();
        deferred1_0 = ret[0];
        deferred1_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
    }
}

/**
 * @returns {string}
 */
export function sourceRevision() {
    let deferred1_0;
    let deferred1_1;
    try {
        const ret = wasm.sourceRevision();
        deferred1_0 = ret[0];
        deferred1_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
    }
}

/**
 * @param {string} name
 * @returns {string | undefined}
 */
export function stdlibSource(name) {
    const ptr0 = passStringToWasm0(name, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.stdlibSource(ptr0, len0);
    let v2;
    if (ret[0] !== 0) {
        v2 = getStringFromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
    }
    return v2;
}
function __wbg_get_imports() {
    const import0 = {
        __proto__: null,
        __wbg___wbindgen_throw_344f42d3211c4765: function(arg0, arg1) {
            throw new Error(getStringFromWasm0(arg0, arg1));
        },
        __wbg_new_with_length_e6785c33c8e4cce8: function(arg0) {
            const ret = new Uint8Array(arg0 >>> 0);
            return ret;
        },
        __wbindgen_cast_0000000000000001: function(arg0, arg1) {
            // Cast intrinsic for `Ref(Slice(U32)) -> NamedExternref("Uint32Array")`.
            const ret = getArrayU32FromWasm0(arg0, arg1);
            return ret;
        },
        __wbindgen_cast_0000000000000002: function(arg0, arg1) {
            // Cast intrinsic for `Ref(Slice(U8)) -> NamedExternref("Uint8Array")`.
            const ret = getArrayU8FromWasm0(arg0, arg1);
            return ret;
        },
        __wbindgen_cast_0000000000000003: function(arg0, arg1) {
            // Cast intrinsic for `Ref(String) -> Externref`.
            const ret = getStringFromWasm0(arg0, arg1);
            return ret;
        },
        __wbindgen_init_externref_table: function() {
            const table = wasm.__wbindgen_externrefs;
            const offset = table.grow(4);
            table.set(0, undefined);
            table.set(offset + 0, undefined);
            table.set(offset + 1, null);
            table.set(offset + 2, true);
            table.set(offset + 3, false);
        },
    };
    return {
        __proto__: null,
        "./maku_bg.js": import0,
    };
}

const MakuFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_maku_free(ptr, 1));

function getArrayF32FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getFloat32ArrayMemory0().subarray(ptr / 4, ptr / 4 + len);
}

function getArrayU32FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getUint32ArrayMemory0().subarray(ptr / 4, ptr / 4 + len);
}

function getArrayU8FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getUint8ArrayMemory0().subarray(ptr / 1, ptr / 1 + len);
}

let cachedFloat32ArrayMemory0 = null;
function getFloat32ArrayMemory0() {
    if (cachedFloat32ArrayMemory0 === null || cachedFloat32ArrayMemory0.byteLength === 0) {
        cachedFloat32ArrayMemory0 = new Float32Array(wasm.memory.buffer);
    }
    return cachedFloat32ArrayMemory0;
}

function getStringFromWasm0(ptr, len) {
    return decodeText(ptr >>> 0, len);
}

let cachedUint32ArrayMemory0 = null;
function getUint32ArrayMemory0() {
    if (cachedUint32ArrayMemory0 === null || cachedUint32ArrayMemory0.byteLength === 0) {
        cachedUint32ArrayMemory0 = new Uint32Array(wasm.memory.buffer);
    }
    return cachedUint32ArrayMemory0;
}

let cachedUint8ArrayMemory0 = null;
function getUint8ArrayMemory0() {
    if (cachedUint8ArrayMemory0 === null || cachedUint8ArrayMemory0.byteLength === 0) {
        cachedUint8ArrayMemory0 = new Uint8Array(wasm.memory.buffer);
    }
    return cachedUint8ArrayMemory0;
}

function isLikeNone(x) {
    return x === undefined || x === null;
}

function passStringToWasm0(arg, malloc, realloc) {
    if (realloc === undefined) {
        const buf = cachedTextEncoder.encode(arg);
        const ptr = malloc(buf.length, 1) >>> 0;
        getUint8ArrayMemory0().subarray(ptr, ptr + buf.length).set(buf);
        WASM_VECTOR_LEN = buf.length;
        return ptr;
    }

    let len = arg.length;
    let ptr = malloc(len, 1) >>> 0;

    const mem = getUint8ArrayMemory0();

    let offset = 0;

    for (; offset < len; offset++) {
        const code = arg.charCodeAt(offset);
        if (code > 0x7F) break;
        mem[ptr + offset] = code;
    }
    if (offset !== len) {
        if (offset !== 0) {
            arg = arg.slice(offset);
        }
        ptr = realloc(ptr, len, len = offset + arg.length * 3, 1) >>> 0;
        const view = getUint8ArrayMemory0().subarray(ptr + offset, ptr + len);
        const ret = cachedTextEncoder.encodeInto(arg, view);

        offset += ret.written;
        ptr = realloc(ptr, len, offset, 1) >>> 0;
    }

    WASM_VECTOR_LEN = offset;
    return ptr;
}

function takeFromExternrefTable0(idx) {
    const value = wasm.__wbindgen_externrefs.get(idx);
    wasm.__externref_table_dealloc(idx);
    return value;
}

let cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
cachedTextDecoder.decode();
const MAX_SAFARI_DECODE_BYTES = 2146435072;
let numBytesDecoded = 0;
function decodeText(ptr, len) {
    numBytesDecoded += len;
    if (numBytesDecoded >= MAX_SAFARI_DECODE_BYTES) {
        cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
        cachedTextDecoder.decode();
        numBytesDecoded = len;
    }
    return cachedTextDecoder.decode(getUint8ArrayMemory0().subarray(ptr, ptr + len));
}

const cachedTextEncoder = new TextEncoder();

if (!('encodeInto' in cachedTextEncoder)) {
    cachedTextEncoder.encodeInto = function (arg, view) {
        const buf = cachedTextEncoder.encode(arg);
        view.set(buf);
        return {
            read: arg.length,
            written: buf.length
        };
    };
}

let WASM_VECTOR_LEN = 0;

let wasmModule, wasmInstance, wasm;
function __wbg_finalize_init(instance, module) {
    wasmInstance = instance;
    wasm = instance.exports;
    wasmModule = module;
    cachedFloat32ArrayMemory0 = null;
    cachedUint32ArrayMemory0 = null;
    cachedUint8ArrayMemory0 = null;
    wasm.__wbindgen_start();
    return wasm;
}

async function __wbg_load(module, imports) {
    if (typeof Response === 'function' && module instanceof Response) {
        if (typeof WebAssembly.instantiateStreaming === 'function') {
            try {
                return await WebAssembly.instantiateStreaming(module, imports);
            } catch (e) {
                const validResponse = module.ok && expectedResponseType(module.type);

                if (validResponse && module.headers.get('Content-Type') !== 'application/wasm') {
                    console.warn("`WebAssembly.instantiateStreaming` failed because your server does not serve Wasm with `application/wasm` MIME type. Falling back to `WebAssembly.instantiate` which is slower. Original error:\n", e);

                } else { throw e; }
            }
        }

        const bytes = await module.arrayBuffer();
        return await WebAssembly.instantiate(bytes, imports);
    } else {
        const instance = await WebAssembly.instantiate(module, imports);

        if (instance instanceof WebAssembly.Instance) {
            return { instance, module };
        } else {
            return instance;
        }
    }

    function expectedResponseType(type) {
        switch (type) {
            case 'basic': case 'cors': case 'default': return true;
        }
        return false;
    }
}

function initSync(module) {
    if (wasm !== undefined) return wasm;


    if (module !== undefined) {
        if (Object.getPrototypeOf(module) === Object.prototype) {
            ({module} = module)
        } else {
            console.warn('using deprecated parameters for `initSync()`; pass a single object instead')
        }
    }

    const imports = __wbg_get_imports();
    if (!(module instanceof WebAssembly.Module)) {
        module = new WebAssembly.Module(module);
    }
    const instance = new WebAssembly.Instance(module, imports);
    return __wbg_finalize_init(instance, module);
}

async function __wbg_init(module_or_path) {
    if (wasm !== undefined) return wasm;


    if (module_or_path !== undefined) {
        if (Object.getPrototypeOf(module_or_path) === Object.prototype) {
            ({module_or_path} = module_or_path)
        } else {
            console.warn('using deprecated parameters for the initialization function; pass a single object instead')
        }
    }

    if (module_or_path === undefined) {
        module_or_path = new URL('maku_bg.wasm', import.meta.url);
    }
    const imports = __wbg_get_imports();

    if (typeof module_or_path === 'string' || (typeof Request === 'function' && module_or_path instanceof Request) || (typeof URL === 'function' && module_or_path instanceof URL)) {
        module_or_path = fetch(module_or_path);
    }

    const { instance, module } = await __wbg_load(await module_or_path, imports);

    return __wbg_finalize_init(instance, module);
}

export { initSync, __wbg_init as default };
