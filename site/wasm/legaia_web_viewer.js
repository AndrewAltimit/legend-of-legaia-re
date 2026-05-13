/* @ts-self-types="./legaia_web_viewer.d.ts" */

/**
 * Bridge object the JS shim instantiates once at page load. Holds a
 * `World` + a `MenuRuntime` for the headless path, and an optional
 * `SceneHost` once `load_disc` has been called.
 */
export class LegaiaRuntime {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        LegaiaRuntimeFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_legaiaruntime_free(ptr, 0);
    }
    /**
     * Number of currently active actors.
     * @returns {number}
     */
    active_actor_count() {
        const ret = wasm.legaiaruntime_active_actor_count(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Attempt to initialise the WebAudio backend. Must be called from a
     * user-gesture handler (browser autoplay policy). Returns `true` if
     * audio started successfully, `false` otherwise (e.g. blocked by the
     * browser before any interaction or on a platform without WebAudio).
     *
     * Idempotent - calling a second time replaces the existing backend.
     * @returns {boolean}
     */
    audio_init() {
        const ret = wasm.legaiaruntime_audio_init(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * `true` if a disc has been loaded via `load_disc`.
     * @returns {boolean}
     */
    disc_loaded() {
        const ret = wasm.legaiaruntime_disc_loaded(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * Boot a named scene (CDNAME label, e.g. `"town01"`). Requires
     * `load_disc` to have been called first. Loads the scene's assets,
     * enters `SceneMode::Field`, and seeds the field-VM with record 0 of
     * the scene's event-script pack. Throws a JS error if the disc hasn't
     * been loaded or the scene name is unknown.
     * @param {string} name
     */
    enter_scene(name) {
        const ptr0 = passStringToWasm0(name, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.legaiaruntime_enter_scene(this.__wbg_ptr, ptr0, len0);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * Frame counter.
     * @returns {bigint}
     */
    frame() {
        const ret = wasm.legaiaruntime_frame(this.__wbg_ptr);
        return BigInt.asUintN(64, ret);
    }
    /**
     * Load a disc image from raw in-memory bytes.
     *
     * `raw_bytes` may be either:
     * - A Mode2/2352 full disc image (`.bin`): PROT.DAT and CDNAME.TXT are
     *   extracted automatically via ISO9660 walk.
     * - The raw contents of `PROT.DAT` directly.
     *
     * `cdname_text` overrides any CDNAME.TXT found on the disc. Pass an empty
     * string to use the disc's own CDNAME.TXT (full disc) or skip scene-name
     * resolution (PROT.DAT-only path without a CDNAME).
     *
     * Returns the number of PROT entries parsed, or throws a JS error on
     * parse failure.
     * @param {Uint8Array} raw_bytes
     * @param {string} cdname_text
     * @returns {number}
     */
    load_disc(raw_bytes, cdname_text) {
        const ptr0 = passArray8ToWasm0(raw_bytes, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ptr1 = passStringToWasm0(cdname_text, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len1 = WASM_VECTOR_LEN;
        const ret = wasm.legaiaruntime_load_disc(this.__wbg_ptr, ptr0, len0, ptr1, len1);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * Boolean: true if the menu is open.
     * @returns {boolean}
     */
    menu_is_open() {
        const ret = wasm.legaiaruntime_menu_is_open(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * Read the menu's current label (e.g. "STATUS", "SAVE - PICK SLOT")
     * for HUD rendering.
     * @returns {string}
     */
    menu_label() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.legaiaruntime_menu_label(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Tick the menu state machine with a packed PSX-pad button mask.
     * The mask matches `legaia_engine_vm::menu::MenuInput` field order:
     * `cross | (circle<<1) | (triangle<<2) | (square<<3) | (up<<4) | (down<<5) | (left<<6) | (right<<7)`.
     * @param {number} button_mask
     * @returns {any}
     */
    menu_tick(button_mask) {
        const ret = wasm.legaiaruntime_menu_tick(this.__wbg_ptr, button_mask);
        return ret;
    }
    constructor() {
        const ret = wasm.legaiaruntime_new();
        this.__wbg_ptr = ret;
        LegaiaRuntimeFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * Open the menu (sets MenuCtx state to Idle).
     */
    open_menu() {
        wasm.legaiaruntime_open_menu(this.__wbg_ptr);
    }
    /**
     * Read the active scene mode as a stable enum string.
     * @returns {string}
     */
    scene_mode() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.legaiaruntime_scene_mode(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Tick the world once. Returns the current frame counter.
     * @returns {bigint}
     */
    tick() {
        const ret = wasm.legaiaruntime_tick(this.__wbg_ptr);
        return BigInt.asUintN(64, ret);
    }
}
if (Symbol.dispose) LegaiaRuntime.prototype[Symbol.dispose] = LegaiaRuntime.prototype.free;

export class LegaiaViewer {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        LegaiaViewerFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_legaiaviewer_free(ptr, 0);
    }
    /**
     * Number of TMDs in the currently-loaded continent pack. 0 when no
     * continent pack was found for this kingdom.
     * @returns {number}
     */
    continent_pack_count() {
        const ret = wasm.legaiaviewer_continent_pack_count(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Select the active continent pack slot. Parallel to `pack_mesh` but
     * operates on the continent pack.
     * @param {number} slot
     * @returns {number}
     */
    continent_pack_mesh(slot) {
        const ret = wasm.legaiaviewer_continent_pack_mesh(this.__wbg_ptr, slot);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * @returns {Float32Array}
     */
    continent_pack_mesh_bounds() {
        const ret = wasm.legaiaviewer_continent_pack_mesh_bounds(this.__wbg_ptr);
        var v1 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * @returns {Uint16Array}
     */
    continent_pack_mesh_cba_tsb() {
        const ret = wasm.legaiaviewer_continent_pack_mesh_cba_tsb(this.__wbg_ptr);
        var v1 = getArrayU16FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 2, 2);
        return v1;
    }
    /**
     * @returns {Uint32Array}
     */
    continent_pack_mesh_indices() {
        const ret = wasm.legaiaviewer_continent_pack_mesh_indices(this.__wbg_ptr);
        var v1 = getArrayU32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * @returns {Float32Array}
     */
    continent_pack_mesh_positions() {
        const ret = wasm.legaiaviewer_continent_pack_mesh_positions(this.__wbg_ptr);
        var v1 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * @returns {Uint8Array}
     */
    continent_pack_mesh_uvs() {
        const ret = wasm.legaiaviewer_continent_pack_mesh_uvs(this.__wbg_ptr);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * VRAM bytes (1 MB) built from the continent pack's slot 0. Distinct from
     * the landmark VRAM since the two packs ship independent TIM_LISTs.
     * @returns {Uint8Array}
     */
    continent_pack_vram_bytes() {
        const ret = wasm.legaiaviewer_continent_pack_vram_bytes(this.__wbg_ptr);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * PROT index the continent pack was loaded from (0 when none).
     * @returns {number}
     */
    continent_prot_index() {
        const ret = wasm.legaiaviewer_continent_prot_index(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * JSON-encoded summary of the current entry - class label, byte size,
     * MES record count (if any), SEQ presence (if any), VAB presence
     * (if any). The JS side parses this and shows it in the inspector
     * panel without needing N round-trips for each individual field.
     * @returns {string}
     */
    current_entry_info_json() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.legaiaviewer_current_entry_info_json(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * True if the current entry has a parseable TMD, suitable for the 3D
     * rendering path. JS uses this to decide whether to switch to the 3D
     * render loop instead of the TIM blit.
     * @returns {boolean}
     */
    current_has_tmd() {
        const ret = wasm.legaiaviewer_current_has_tmd(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * @returns {number}
     */
    current_index() {
        const ret = wasm.legaiaviewer_current_index(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Resolve a MES message id to its first 64 bytes as a hex string (for
     * preview in the inspector panel). Returns an empty string if the
     * current entry isn't a MES container or `text_id` is out of range.
     * @param {number} text_id
     * @returns {string}
     */
    current_mes_message_hex(text_id) {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.legaiaviewer_current_mes_message_hex(this.__wbg_ptr, text_id);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Build a 1024×512 PSX VRAM from every TIM the current entry contains.
     * Returns the raw bytes (2 MB if a CLUT block is present, but VRAM is
     * always exactly 1 MB = 1024×512×2). Used by the WebGL2 path to upload
     * to a R16UI texture.
     * @returns {Uint8Array}
     */
    current_vram_bytes() {
        const ret = wasm.legaiaviewer_current_vram_bytes(this.__wbg_ptr);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * @returns {number}
     */
    entry_count() {
        const ret = wasm.legaiaviewer_entry_count(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Returns a JSON array describing every viewable entry: PROT index, class,
     * dimensions, has-TMD flag. The UI uses this to populate a sidebar list / search.
     * @returns {string}
     */
    entry_list_json() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.legaiaviewer_entry_list_json(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Load a disc image. Auto-detects: full Mode2/2352 .bin, raw PROT.DAT,
     * or single TIM. Returns the count of viewable entries (entries with at
     * least one decodable TIM) for the JS UI.
     * @param {Uint8Array} bytes
     * @returns {number}
     */
    load_disc(bytes) {
        const ptr0 = passArray8ToWasm0(bytes, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.legaiaviewer_load_disc(this.__wbg_ptr, ptr0, len0);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * Returns the model's bounding sphere center (`[cx, cy, cz]`) and radius
     * `r` packed as `[cx, cy, cz, r]`. JS uses this to build the MVP matrix
     * without re-parsing the TMD each frame.
     * @returns {Float32Array}
     */
    mesh_bounds() {
        const ret = wasm.legaiaviewer_mesh_bounds(this.__wbg_ptr);
        var v1 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * @returns {Uint16Array}
     */
    mesh_cba_tsb() {
        const ret = wasm.legaiaviewer_mesh_cba_tsb(this.__wbg_ptr);
        var v1 = getArrayU16FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 2, 2);
        return v1;
    }
    /**
     * @returns {Uint32Array}
     */
    mesh_indices() {
        const ret = wasm.legaiaviewer_mesh_indices(this.__wbg_ptr);
        var v1 = getArrayU32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * Returns the mesh data for the current entry's TMD as four typed arrays
     * concatenated by use:
     *   `[positions(f32 ×3 per vert), uvs(u8 ×2), cba_tsb(u16 ×2), indices(u32)]`
     * Each as a separate getter so JS can pull them as typed arrays without
     * reparsing JSON.
     * @returns {Float32Array}
     */
    mesh_positions() {
        const ret = wasm.legaiaviewer_mesh_positions(this.__wbg_ptr);
        var v1 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * @returns {Uint8Array}
     */
    mesh_uvs() {
        const ret = wasm.legaiaviewer_mesh_uvs(this.__wbg_ptr);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * @param {string} canvas_id
     */
    constructor(canvas_id) {
        const ptr0 = passStringToWasm0(canvas_id, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.legaiaviewer_new(ptr0, len0);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        this.__wbg_ptr = ret[0];
        LegaiaViewerFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * @returns {number}
     */
    next_entry() {
        const ret = wasm.legaiaviewer_next_entry(this.__wbg_ptr);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * Number of TMDs in the currently-loaded kingdom pack. 0 when no
     * kingdom is loaded.
     * @returns {number}
     */
    pack_count() {
        const ret = wasm.legaiaviewer_pack_count(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Set the active pack-mesh slot. Subsequent `pack_mesh_*` calls source
     * from `pack[byte_offsets[slot]..byte_ends[slot]]`. Returns an error
     * when no kingdom is loaded or `slot >= pack count`.
     * @param {number} slot
     * @returns {number}
     */
    pack_mesh(slot) {
        const ret = wasm.legaiaviewer_pack_mesh(this.__wbg_ptr, slot);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * @returns {Float32Array}
     */
    pack_mesh_bounds() {
        const ret = wasm.legaiaviewer_pack_mesh_bounds(this.__wbg_ptr);
        var v1 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * @returns {Uint16Array}
     */
    pack_mesh_cba_tsb() {
        const ret = wasm.legaiaviewer_pack_mesh_cba_tsb(this.__wbg_ptr);
        var v1 = getArrayU16FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 2, 2);
        return v1;
    }
    /**
     * @returns {Uint32Array}
     */
    pack_mesh_indices() {
        const ret = wasm.legaiaviewer_pack_mesh_indices(this.__wbg_ptr);
        var v1 = getArrayU32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * Parallel to [`Self::mesh_positions`] but sources from the currently
     * selected kingdom pack slot.
     * @returns {Float32Array}
     */
    pack_mesh_positions() {
        const ret = wasm.legaiaviewer_pack_mesh_positions(this.__wbg_ptr);
        var v1 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * @returns {Uint8Array}
     */
    pack_mesh_uvs() {
        const ret = wasm.legaiaviewer_pack_mesh_uvs(this.__wbg_ptr);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * VRAM bytes (1 MB) built from every TIM in the kingdom's slot 0
     * (TIM_LIST). Reuse across every `pack_mesh_*` call - the kingdom
     * pack's per-slot TMDs all sample from this one shared image.
     * @returns {Uint8Array}
     */
    pack_vram_bytes() {
        const ret = wasm.legaiaviewer_pack_vram_bytes(this.__wbg_ptr);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * @returns {number}
     */
    prev_entry() {
        const ret = wasm.legaiaviewer_prev_entry(this.__wbg_ptr);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * Render the current entry's TMD at the given rotation into a flat
     * `Vec<f32>` of triangle data (7 floats per triangle, painter's-sorted
     * back-to-front).
     *
     * Format per triangle: `[x0, y0, x1, y1, x2, y2, brightness 0..1]`.
     *
     * Returns an empty vec if the current entry has no TMD or the TMD has
     * no triangles.
     * @param {number} yaw
     * @param {number} pitch
     * @param {number} distance
     * @param {number} pan_x
     * @param {number} pan_y
     * @param {number} viewport_w
     * @param {number} viewport_h
     * @returns {Float32Array}
     */
    render_tmd_triangles(yaw, pitch, distance, pan_x, pan_y, viewport_w, viewport_h) {
        const ret = wasm.legaiaviewer_render_tmd_triangles(this.__wbg_ptr, yaw, pitch, distance, pan_x, pan_y, viewport_w, viewport_h);
        var v1 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * Parse a mednafen save state and return the GPU's currently-displayed
     * framebuffer as an RGBA8 byte buffer + dimensions.
     *
     * Layout of the returned `Vec<u8>`:
     * `[u16 width, u16 height, RGBA8 pixels...]` packed little-endian. JS
     * reads the leading 4 bytes for the dimensions and then wraps the rest
     * in an `ImageData` to blit into a 2D canvas.
     *
     * This is the in-game top-down world-map view: the game's renderer has
     * already composed the ~10,000 textured polygons that form the kingdom
     * terrain, and the result is sitting in VRAM at the display-area
     * offset. We just read it back. Source-mesh reconstruction is a separate
     * follow-up (the live PSX GPU prim-pool sits around `0x800AD408` and
     * the underlying mesh / tilemap data lives in the kingdom's
     * `scene_v12_table` at PROT base+8 - both still being characterised).
     * @param {Uint8Array} save_state_bytes
     * @returns {Uint8Array}
     */
    save_state_framebuffer(save_state_bytes) {
        const ptr0 = passArray8ToWasm0(save_state_bytes, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.legaiaviewer_save_state_framebuffer(this.__wbg_ptr, ptr0, len0);
        if (ret[3]) {
            throw takeFromExternrefTable0(ret[2]);
        }
        var v2 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v2;
    }
    /**
     * `flags` packs the prim cmd-byte mode bits: bit 0 = semi-transparent,
     * bit 1 = raw texture (skip color modulation). JS computes the model-view
     * matrix from `screen_w / screen_h` (orthographic 0..w x h..0 viewport).
     * @param {Uint8Array} save_state_bytes
     * @returns {Uint8Array}
     */
    save_state_prim_replay(save_state_bytes) {
        const ptr0 = passArray8ToWasm0(save_state_bytes, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.legaiaviewer_save_state_prim_replay(this.__wbg_ptr, ptr0, len0);
        if (ret[3]) {
            throw takeFromExternrefTable0(ret[2]);
        }
        var v2 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v2;
    }
    /**
     * @param {number} idx
     */
    set_clut(idx) {
        const ret = wasm.legaiaviewer_set_clut(this.__wbg_ptr, idx);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * Open a world-map kingdom's 7-asset bundle, LZS-decode slot 0
     * (TIM_LIST) into a shared VRAM, and LZS-decode slot 1 (TMD pack) for
     * per-slot mesh access. Returns the pack count (= number of scene-pool
     * TMDs available to `pack_mesh`).
     *
     * `prot_base` is the kingdom's leading PROT entry index - 85 for Drake
     * (`map01`), 244 for Sebucus (`map02`), 391 for Karisto (`map03`).
     * Either the `scene_scripted_asset_table` (PROT base) or the bare
     * `scene_asset_table` (PROT base+1) works; the detector finds the
     * 7-asset table at the first 0x800-aligned offset whose `u32_le[0] == 7`
     * and `descriptor[0].data_offset == 0x40`.
     *
     * Implementation mirrors `FUN_8001F05C case 2` (TMD-pack dispatch): the
     * pack is `[u32 count][u32 word_offsets[count]][TMD bodies]` with
     * offsets in 4-byte words (`puVar1 + puVar5[1]` on `uint*`). The
     * VRAM upload is unconditional (every TIM in slot 0 is uploaded);
     * per-prim filtering happens later in `pack_mesh_*`.
     * @param {number} prot_base
     * @returns {number}
     */
    set_scene_kingdom(prot_base) {
        const ret = wasm.legaiaviewer_set_scene_kingdom(this.__wbg_ptr, prot_base);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * Jump to the slot in the filtered list (NOT the PROT index). Used by
     * the dropdown / list-click UI.
     * @param {number} slot
     * @returns {number}
     */
    set_slot(slot) {
        const ret = wasm.legaiaviewer_set_slot(this.__wbg_ptr, slot);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * Per-body inventory of the slot-4 wireframe, as a JSON string.
     * Used by the inspector panel to show which bodies are present.
     * Returns `"[]"` when slot 4 can't be decoded.
     * @param {number} prot_base
     * @returns {string}
     */
    slot4_body_inventory_json(prot_base) {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.legaiaviewer_slot4_body_inventory_json(this.__wbg_ptr, prot_base);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Bounding box of every non-zero record in the kingdom's slot-4
     * wireframe, as `[xmin, zmin, xmax, zmax]` (i32). Useful for
     * re-framing the top-down camera when the overlay is toggled on.
     * Empty vec when slot 4 can't be decoded.
     * @param {number} prot_base
     * @returns {Int32Array}
     */
    slot4_wireframe_bounds(prot_base) {
        const ret = wasm.legaiaviewer_slot4_wireframe_bounds(this.__wbg_ptr, prot_base);
        var v1 = getArrayI32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * Decode the slot-4 world-map overlay wireframe for the kingdom at
     * `prot_base` and return a packed line-segment list for top-down
     * rendering.
     *
     * The wireframe is the dev-menu top-view overlay - coastline curves
     * (Drake body 12 = 1200-vertex outline) and the ±32K world-boundary
     * frame (Drake body 13). Loaded verbatim into RAM at `0x8011A624` for
     * Drake (32304 bytes); format is fully reversed (see
     * [`docs/formats/world-map-overlay.md`]).
     *
     * `style` selects the polyline-construction mode:
     * `"row"` (each group as one polyline), `"col"` (each record-slot as
     * one polyline across groups), `"pairs"` (every 2 consecutive
     * records emit one segment), or `"grid"` (both row and column
     * edges of the `count_a x count_b` vertex grid). Unknown values
     * fall back to `"row"`.
     *
     * Output layout (single packed `Vec<u8>`, little-endian):
     *
     * ```text
     * [u32 line_count]
     * [Line; line_count]   ; struct, 12 bytes each:
     *     u8  body_index
     *     u8  group_index_low   ; group_index = (low | (high << 8))
     *     u8  group_index_high
     *     u8  _pad
     *     i16 x0
     *     i16 z0
     *     i16 x1
     *     i16 z1
     * ```
     *
     * Returns an empty buffer when slot 4 is missing or fails to parse.
     * The JS-side renderer assigns per-body colors based on `body_index`.
     * @param {number} prot_base
     * @param {string} style
     * @returns {Uint8Array}
     */
    slot4_wireframe_lines(prot_base, style) {
        const ptr0 = passStringToWasm0(style, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.legaiaviewer_slot4_wireframe_lines(this.__wbg_ptr, prot_base, ptr0, len0);
        var v2 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v2;
    }
    /**
     * Decode the slot-4 world-map overlay as a topology-free point cloud.
     * Useful when the on-disc draw-mode dispatch isn't fully reverse-
     * engineered: the points themselves are byte-verified against live
     * RAM, so plotting them straight is the most honest visualization.
     *
     * Output layout (little-endian):
     *
     * ```text
     * [u32 point_count]
     * [Point; point_count] ; 8 bytes each:
     *     u8  body_index
     *     u8  group_index_low
     *     u8  group_index_high
     *     u8  _pad
     *     i16 x
     *     i16 z
     * ```
     * @param {number} prot_base
     * @returns {Uint8Array}
     */
    slot4_wireframe_points(prot_base) {
        const ret = wasm.legaiaviewer_slot4_wireframe_points(this.__wbg_ptr, prot_base);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * JSON status string: PROT index, class name, dims, current slot.
     * @returns {string}
     */
    status() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.legaiaviewer_status(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Decode the live PSX GPU primitive pool out of a mednafen save state
     * and return per-vertex attribute arrays for replay in WebGL2 against
     * the save state's VRAM.
     *
     * Pool location is per `legaia_mednafen::prim_pool::POOL_BASE_DEFAULT`
     * (= `0x800AD400`, consistent across the Drake / Sebucus / Karisto
     * top-view captures). Each accepted primitive (POLY_FT4, POLY_GT4,
     * POLY_FT3, POLY_GT3, SPRT_16, SPRT_8) is expanded into two
     * triangles in screen-space.
     *
     * Return layout (single packed `Vec<u8>`, little-endian, in this order):
     *
     * ```text
     * [u16 vram_width = 1024]
     * [u16 vram_height = 512]
     * [u32 vram_byte_len = 1048576]
     * [u8;  1048576] VRAM bytes (raw BGR555+STP halfwords)
     * [u16 screen_w]
     * [u16 screen_h]
     * [u32 vertex_count]
     * [Vertex; vertex_count]   ; struct, 14 bytes each:
     *     i16 x, i16 y
     *     u8  u, u8 v
     *     u16 cba, u16 tsb
     *     u8  r, u8 g, u8 b, u8 flags
     * ```
     *
     * JSON dump of the world-map quick-travel menu parsed out of
     * `SCUS_942.54` at disc-load time. Returns `null` if no disc was
     * loaded as a Mode2/2352 image (raw PROT.DAT paths skip SCUS).
     *
     * Shape:
     * ```json
     * { "names": [..16 strings..],
     *   "placements": [{ "index": u32, "name_idx": u8,
     *                    "discovery_flag": u8, "scene_id": u16,
     *                    "menu_x": u8, "menu_y": u8 }, ...] }
     * ```
     * @returns {string}
     */
    worldmap_menu_json() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.legaiaviewer_worldmap_menu_json(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
}
if (Symbol.dispose) LegaiaViewer.prototype[Symbol.dispose] = LegaiaViewer.prototype.free;
function __wbg_get_imports() {
    const import0 = {
        __proto__: null,
        __wbg___wbindgen_debug_string_07cb72cfcc952e2b: function(arg0, arg1) {
            const ret = debugString(arg1);
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg___wbindgen_is_undefined_244a92c34d3b6ec0: function(arg0) {
            const ret = arg0 === undefined;
            return ret;
        },
        __wbg___wbindgen_throw_9c75d47bf9e7731e: function(arg0, arg1) {
            throw new Error(getStringFromWasm0(arg0, arg1));
        },
        __wbg__wbg_cb_unref_158e43e869788cdc: function(arg0) {
            arg0._wbg_cb_unref();
        },
        __wbg_connect_b0c6d44e9984ca8e: function() { return handleError(function (arg0, arg1) {
            const ret = arg0.connect(arg1);
            return ret;
        }, arguments); },
        __wbg_copyToChannel_be740358a55f7ec4: function() { return handleError(function (arg0, arg1, arg2, arg3) {
            arg0.copyToChannel(getArrayF32FromWasm0(arg1, arg2), arg3);
        }, arguments); },
        __wbg_createScriptProcessor_6af6560e010dc72e: function() { return handleError(function (arg0, arg1, arg2, arg3) {
            const ret = arg0.createScriptProcessor(arg1 >>> 0, arg2 >>> 0, arg3 >>> 0);
            return ret;
        }, arguments); },
        __wbg_destination_a7fb84721246ff2f: function(arg0) {
            const ret = arg0.destination;
            return ret;
        },
        __wbg_document_69bb6a2f7927d532: function(arg0) {
            const ret = arg0.document;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_error_48655ee7e4756f8b: function(arg0) {
            console.error(arg0);
        },
        __wbg_error_a6fa202b58aa1cd3: function(arg0, arg1) {
            let deferred0_0;
            let deferred0_1;
            try {
                deferred0_0 = arg0;
                deferred0_1 = arg1;
                console.error(getStringFromWasm0(arg0, arg1));
            } finally {
                wasm.__wbindgen_free(deferred0_0, deferred0_1, 1);
            }
        },
        __wbg_fillRect_9219f775d7e8e73e: function(arg0, arg1, arg2, arg3, arg4) {
            arg0.fillRect(arg1, arg2, arg3, arg4);
        },
        __wbg_fillText_9fbea3af94326c74: function() { return handleError(function (arg0, arg1, arg2, arg3, arg4) {
            arg0.fillText(getStringFromWasm0(arg1, arg2), arg3, arg4);
        }, arguments); },
        __wbg_getContext_f17252002286474d: function() { return handleError(function (arg0, arg1, arg2) {
            const ret = arg0.getContext(getStringFromWasm0(arg1, arg2));
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        }, arguments); },
        __wbg_getElementById_22becc83cca95cc2: function(arg0, arg1, arg2) {
            const ret = arg0.getElementById(getStringFromWasm0(arg1, arg2));
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_instanceof_CanvasRenderingContext2d_b433938013de3a1e: function(arg0) {
            let result;
            try {
                result = arg0 instanceof CanvasRenderingContext2D;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_instanceof_HtmlCanvasElement_0ac74d5643067956: function(arg0) {
            let result;
            try {
                result = arg0 instanceof HTMLCanvasElement;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_instanceof_Window_4153c1818a1c0c0b: function(arg0) {
            let result;
            try {
                result = arg0 instanceof Window;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_length_f3b8e74fce8baae2: function(arg0) {
            const ret = arg0.length;
            return ret;
        },
        __wbg_log_72d22df918dcc232: function(arg0) {
            console.log(arg0);
        },
        __wbg_new_227d7c05414eb861: function() {
            const ret = new Error();
            return ret;
        },
        __wbg_new_a6b46eaf9085fbeb: function() { return handleError(function () {
            const ret = new lAudioContext();
            return ret;
        }, arguments); },
        __wbg_new_with_u8_clamped_array_and_sh_a4ac3311668de769: function() { return handleError(function (arg0, arg1, arg2, arg3) {
            const ret = new ImageData(getClampedArrayU8FromWasm0(arg0, arg1), arg2 >>> 0, arg3 >>> 0);
            return ret;
        }, arguments); },
        __wbg_outputBuffer_22a53fe3e8b904c5: function() { return handleError(function (arg0) {
            const ret = arg0.outputBuffer;
            return ret;
        }, arguments); },
        __wbg_putImageData_3c24c64a03f8b92f: function() { return handleError(function (arg0, arg1, arg2, arg3) {
            arg0.putImageData(arg1, arg2, arg3);
        }, arguments); },
        __wbg_sampleRate_b7f221c5b3d93248: function(arg0) {
            const ret = arg0.sampleRate;
            return ret;
        },
        __wbg_set_fillStyle_a3656c7c5d4ad803: function(arg0, arg1, arg2) {
            arg0.fillStyle = getStringFromWasm0(arg1, arg2);
        },
        __wbg_set_font_5b1b8c76449f5864: function(arg0, arg1, arg2) {
            arg0.font = getStringFromWasm0(arg1, arg2);
        },
        __wbg_set_height_89a4ecd0f9cc3dfa: function(arg0, arg1) {
            arg0.height = arg1 >>> 0;
        },
        __wbg_set_onaudioprocess_60182ff0cf43770e: function(arg0, arg1) {
            arg0.onaudioprocess = arg1;
        },
        __wbg_set_width_d2ec5d6689655fa9: function(arg0, arg1) {
            arg0.width = arg1 >>> 0;
        },
        __wbg_stack_3b0d974bbf31e44f: function(arg0, arg1) {
            const ret = arg1.stack;
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg_static_accessor_GLOBAL_THIS_1c7f1bd6c6941fdb: function() {
            const ret = typeof globalThis === 'undefined' ? null : globalThis;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_static_accessor_GLOBAL_e039bc914f83e74e: function() {
            const ret = typeof global === 'undefined' ? null : global;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_static_accessor_SELF_8bf8c48c28420ad5: function() {
            const ret = typeof self === 'undefined' ? null : self;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_static_accessor_WINDOW_6aeee9b51652ee0f: function() {
            const ret = typeof window === 'undefined' ? null : window;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbindgen_cast_0000000000000001: function(arg0, arg1) {
            // Cast intrinsic for `Closure(Closure { owned: true, function: Function { arguments: [NamedExternref("AudioProcessingEvent")], shim_idx: 429, ret: Unit, inner_ret: Some(Unit) }, mutable: true }) -> Externref`.
            const ret = makeMutClosure(arg0, arg1, wasm_bindgen__convert__closures_____invoke__h90bbf554010c78df);
            return ret;
        },
        __wbindgen_cast_0000000000000002: function(arg0, arg1) {
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
        "./legaia_web_viewer_bg.js": import0,
    };
}

const lAudioContext = (typeof AudioContext !== 'undefined' ? AudioContext : (typeof webkitAudioContext !== 'undefined' ? webkitAudioContext : undefined));
function wasm_bindgen__convert__closures_____invoke__h90bbf554010c78df(arg0, arg1, arg2) {
    wasm.wasm_bindgen__convert__closures_____invoke__h90bbf554010c78df(arg0, arg1, arg2);
}

const LegaiaRuntimeFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_legaiaruntime_free(ptr, 1));
const LegaiaViewerFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_legaiaviewer_free(ptr, 1));

function addToExternrefTable0(obj) {
    const idx = wasm.__externref_table_alloc();
    wasm.__wbindgen_externrefs.set(idx, obj);
    return idx;
}

const CLOSURE_DTORS = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(state => wasm.__wbindgen_destroy_closure(state.a, state.b));

function debugString(val) {
    // primitive types
    const type = typeof val;
    if (type == 'number' || type == 'boolean' || val == null) {
        return  `${val}`;
    }
    if (type == 'string') {
        return `"${val}"`;
    }
    if (type == 'symbol') {
        const description = val.description;
        if (description == null) {
            return 'Symbol';
        } else {
            return `Symbol(${description})`;
        }
    }
    if (type == 'function') {
        const name = val.name;
        if (typeof name == 'string' && name.length > 0) {
            return `Function(${name})`;
        } else {
            return 'Function';
        }
    }
    // objects
    if (Array.isArray(val)) {
        const length = val.length;
        let debug = '[';
        if (length > 0) {
            debug += debugString(val[0]);
        }
        for(let i = 1; i < length; i++) {
            debug += ', ' + debugString(val[i]);
        }
        debug += ']';
        return debug;
    }
    // Test for built-in
    const builtInMatches = /\[object ([^\]]+)\]/.exec(toString.call(val));
    let className;
    if (builtInMatches && builtInMatches.length > 1) {
        className = builtInMatches[1];
    } else {
        // Failed to match the standard '[object ClassName]'
        return toString.call(val);
    }
    if (className == 'Object') {
        // we're a user defined class or Object
        // JSON.stringify avoids problems with cycles, and is generally much
        // easier than looping through ownProperties of `val`.
        try {
            return 'Object(' + JSON.stringify(val) + ')';
        } catch (_) {
            return 'Object';
        }
    }
    // errors
    if (val instanceof Error) {
        return `${val.name}: ${val.message}\n${val.stack}`;
    }
    // TODO we could test for more things here, like `Set`s and `Map`s.
    return className;
}

function getArrayF32FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getFloat32ArrayMemory0().subarray(ptr / 4, ptr / 4 + len);
}

function getArrayI32FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getInt32ArrayMemory0().subarray(ptr / 4, ptr / 4 + len);
}

function getArrayU16FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getUint16ArrayMemory0().subarray(ptr / 2, ptr / 2 + len);
}

function getArrayU32FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getUint32ArrayMemory0().subarray(ptr / 4, ptr / 4 + len);
}

function getArrayU8FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getUint8ArrayMemory0().subarray(ptr / 1, ptr / 1 + len);
}

function getClampedArrayU8FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getUint8ClampedArrayMemory0().subarray(ptr / 1, ptr / 1 + len);
}

let cachedDataViewMemory0 = null;
function getDataViewMemory0() {
    if (cachedDataViewMemory0 === null || cachedDataViewMemory0.buffer.detached === true || (cachedDataViewMemory0.buffer.detached === undefined && cachedDataViewMemory0.buffer !== wasm.memory.buffer)) {
        cachedDataViewMemory0 = new DataView(wasm.memory.buffer);
    }
    return cachedDataViewMemory0;
}

let cachedFloat32ArrayMemory0 = null;
function getFloat32ArrayMemory0() {
    if (cachedFloat32ArrayMemory0 === null || cachedFloat32ArrayMemory0.byteLength === 0) {
        cachedFloat32ArrayMemory0 = new Float32Array(wasm.memory.buffer);
    }
    return cachedFloat32ArrayMemory0;
}

let cachedInt32ArrayMemory0 = null;
function getInt32ArrayMemory0() {
    if (cachedInt32ArrayMemory0 === null || cachedInt32ArrayMemory0.byteLength === 0) {
        cachedInt32ArrayMemory0 = new Int32Array(wasm.memory.buffer);
    }
    return cachedInt32ArrayMemory0;
}

function getStringFromWasm0(ptr, len) {
    return decodeText(ptr >>> 0, len);
}

let cachedUint16ArrayMemory0 = null;
function getUint16ArrayMemory0() {
    if (cachedUint16ArrayMemory0 === null || cachedUint16ArrayMemory0.byteLength === 0) {
        cachedUint16ArrayMemory0 = new Uint16Array(wasm.memory.buffer);
    }
    return cachedUint16ArrayMemory0;
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

let cachedUint8ClampedArrayMemory0 = null;
function getUint8ClampedArrayMemory0() {
    if (cachedUint8ClampedArrayMemory0 === null || cachedUint8ClampedArrayMemory0.byteLength === 0) {
        cachedUint8ClampedArrayMemory0 = new Uint8ClampedArray(wasm.memory.buffer);
    }
    return cachedUint8ClampedArrayMemory0;
}

function handleError(f, args) {
    try {
        return f.apply(this, args);
    } catch (e) {
        const idx = addToExternrefTable0(e);
        wasm.__wbindgen_exn_store(idx);
    }
}

function isLikeNone(x) {
    return x === undefined || x === null;
}

function makeMutClosure(arg0, arg1, f) {
    const state = { a: arg0, b: arg1, cnt: 1 };
    const real = (...args) => {

        // First up with a closure we increment the internal reference
        // count. This ensures that the Rust closure environment won't
        // be deallocated while we're invoking it.
        state.cnt++;
        const a = state.a;
        state.a = 0;
        try {
            return f(a, state.b, ...args);
        } finally {
            state.a = a;
            real._wbg_cb_unref();
        }
    };
    real._wbg_cb_unref = () => {
        if (--state.cnt === 0) {
            wasm.__wbindgen_destroy_closure(state.a, state.b);
            state.a = 0;
            CLOSURE_DTORS.unregister(state);
        }
    };
    CLOSURE_DTORS.register(real, state, state);
    return real;
}

function passArray8ToWasm0(arg, malloc) {
    const ptr = malloc(arg.length * 1, 1) >>> 0;
    getUint8ArrayMemory0().set(arg, ptr / 1);
    WASM_VECTOR_LEN = arg.length;
    return ptr;
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
    cachedDataViewMemory0 = null;
    cachedFloat32ArrayMemory0 = null;
    cachedInt32ArrayMemory0 = null;
    cachedUint16ArrayMemory0 = null;
    cachedUint32ArrayMemory0 = null;
    cachedUint8ArrayMemory0 = null;
    cachedUint8ClampedArrayMemory0 = null;
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
        module_or_path = new URL('legaia_web_viewer_bg.wasm', import.meta.url);
    }
    const imports = __wbg_get_imports();

    if (typeof module_or_path === 'string' || (typeof Request === 'function' && module_or_path instanceof Request) || (typeof URL === 'function' && module_or_path instanceof URL)) {
        module_or_path = fetch(module_or_path);
    }

    const { instance, module } = await __wbg_load(await module_or_path, imports);

    return __wbg_finalize_init(instance, module);
}

export { initSync, __wbg_init as default };
