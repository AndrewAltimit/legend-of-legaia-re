/* @ts-self-types="./legaia_web_viewer.d.ts" */

/**
 * In-browser audio extraction surface. Owns the loaded Mode2/2352 disc plus
 * its extracted PROT.DAT bytes; exposes JSON enumerators for the three
 * audio families (VAB / BGM / XA) and PCM-returning decoders for each.
 *
 * BGM playback uses [`legaia_engine_audio::WebAudioOut`] under the hood -
 * constructed lazily on the first `start_bgm` call so the autoplay policy
 * is satisfied (must happen inside a user-gesture handler on the JS side).
 */
export class LegaiaAudio {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        LegaiaAudioFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_legaiaaudio_free(ptr, 0);
    }
    /**
     * Sample rate of the browser's BGM `AudioContext`, or 0 when the BGM
     * output hasn't been opened yet. Surfaced to the JS console for
     * diagnostics when playback speed is off.
     * @returns {number}
     */
    bgm_device_rate() {
        const ret = wasm.legaiaaudio_bgm_device_rate(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Sample rate produced by [`Self::render_bgm_pcm_i16`] (the SPU's
     * internal 44.1 kHz). Surfaced so the JS side can build a correct
     * WAV header for `decodeAudioData`.
     * @returns {number}
     */
    bgm_render_rate() {
        const ret = wasm.legaiaaudio_bgm_render_rate(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Decode one VAG sample to mono i16 PCM at `vab_sample_rate()`.
     * Empty when the sample doesn't exist or has zero length.
     * @param {number} prot_index
     * @param {number} vab_offset
     * @param {number} sample_idx
     * @returns {Int16Array}
     */
    decode_vab_sample_i16(prot_index, vab_offset, sample_idx) {
        const ret = wasm.legaiaaudio_decode_vab_sample_i16(this.__wbg_ptr, prot_index, vab_offset, sample_idx);
        var v1 = getArrayI16FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 2, 2);
        return v1;
    }
    /**
     * Decode XA stream and return the i16 PCM for the channel at `stream_idx`
     * (index into the `xa_metadata_json` array). Empty when out of range.
     * @param {number} lba
     * @param {number} size
     * @param {number} stream_idx
     * @returns {Int16Array}
     */
    decode_xa_stream_i16(lba, size, stream_idx) {
        const ret = wasm.legaiaaudio_decode_xa_stream_i16(this.__wbg_ptr, lba, size, stream_idx);
        var v1 = getArrayI16FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 2, 2);
        return v1;
    }
    /**
     * JSON list of every BGM pair (`pBAV` + `pQES` in the same PROT entry).
     * Shape: `[{ prot_index, vab_offset, seq_offset, program_count, sample_count, ppqn, bpm }, ...]`.
     * @returns {string}
     */
    enumerate_bgm_pairs_json() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.legaiaaudio_enumerate_bgm_pairs_json(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * JSON list of every VAB sound bank in the loaded disc.
     * Shape: `[{ prot_index, vab_offset, version, program_count, sample_count, has_seq }, ...]`.
     * @returns {string}
     */
    enumerate_vabs_json() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.legaiaaudio_enumerate_vabs_json(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * JSON list of every `*.STR` / `*.XA` file on the disc, with its raw LBA
     * and byte size. Shape: `[{ path, lba, size }, ...]`.
     * @returns {string}
     */
    enumerate_xa_files_json() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.legaiaaudio_enumerate_xa_files_json(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Load a full Mode2/2352 disc image. Extracts `PROT.DAT` via the same
     * in-memory ISO walker the viewer uses, parses the TOC, and stashes
     * both slices for later VAB / BGM / XA queries. Returns the PROT entry
     * count for the JS UI.
     * @param {Uint8Array} bytes
     * @returns {number}
     */
    load_disc(bytes) {
        const ptr0 = passArray8ToWasm0(bytes, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.legaiaaudio_load_disc(this.__wbg_ptr, ptr0, len0);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    constructor() {
        const ret = wasm.legaiaaudio_new();
        this.__wbg_ptr = ret;
        LegaiaAudioFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * Render `duration_seconds` worth of interleaved stereo i16 PCM at
     * the SPU's 44.1 kHz rate for the BGM pair at (`prot_index`,
     * `vab_offset`, `seq_offset`). Used by the audio page to pre-render
     * a chunk and play it through `AudioBufferSourceNode` (sample-
     * accurate timing) instead of through `ScriptProcessorNode` (callback-
     * paced, drifts on some browsers).
     * @param {number} prot_index
     * @param {number} vab_offset
     * @param {number} seq_offset
     * @param {number} duration_seconds
     * @returns {Int16Array}
     */
    render_bgm_pcm_i16(prot_index, vab_offset, seq_offset, duration_seconds) {
        const ret = wasm.legaiaaudio_render_bgm_pcm_i16(this.__wbg_ptr, prot_index, vab_offset, seq_offset, duration_seconds);
        var v1 = getArrayI16FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 2, 2);
        return v1;
    }
    /**
     * Resume the BGM AudioContext. Browsers often construct the
     * `AudioContext` in `suspended` state even when the constructor
     * runs inside a user-gesture handler; the JS side calls this
     * immediately after `start_bgm` to make the audio actually audible.
     * @returns {Promise<any>}
     */
    resume_bgm() {
        const ret = wasm.legaiaaudio_resume_bgm(this.__wbg_ptr);
        return ret;
    }
    /**
     * Set the BGM playback gain. Retail SEQ + clean-room SPU output sits
     * around 1% of the i16 range, so the audio page defaults to ~25x to
     * bring playback to a comfortable level. `1.0` matches the native
     * engine-shell cpal path.
     * @param {number} gain
     */
    set_bgm_gain(gain) {
        wasm.legaiaaudio_set_bgm_gain(this.__wbg_ptr, gain);
    }
    /**
     * Pause / resume the active BGM sequencer. Notes that are already
     * sounding decay through their ADSR envelopes; the sequencer clock
     * freezes.
     * @param {boolean} paused
     */
    set_bgm_paused(paused) {
        wasm.legaiaaudio_set_bgm_paused(this.__wbg_ptr, paused);
    }
    /**
     * Start BGM playback for the given (`prot_index`, `vab_offset`,
     * `seq_offset`) tuple. Constructs the WebAudio output on the first call
     * (must be invoked from a user-gesture handler), parses VAB + SEQ,
     * uploads the bank to the embedded clean-room SPU, and attaches the
     * sequencer.
     * @param {number} prot_index
     * @param {number} vab_offset
     * @param {number} seq_offset
     */
    start_bgm(prot_index, vab_offset, seq_offset) {
        const ret = wasm.legaiaaudio_start_bgm(this.__wbg_ptr, prot_index, vab_offset, seq_offset);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * Stop the currently-playing BGM. Safe to call even when nothing is
     * playing (no-op).
     */
    stop_bgm() {
        wasm.legaiaaudio_stop_bgm(this.__wbg_ptr);
    }
    /**
     * Decode the frame at `frame_idx` of the currently-open STR movie to a
     * row-major RGBA8 buffer (`width * height * 4` bytes). Empty when no movie
     * is open or the index is out of range. Call `str_video_open` first.
     * @param {number} frame_idx
     * @returns {Uint8Array}
     */
    str_decode_frame(frame_idx) {
        const ret = wasm.legaiaaudio_str_decode_frame(this.__wbg_ptr, frame_idx);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * Drop the cached STR movie frames (frees the bitstream buffers).
     */
    str_video_close() {
        wasm.legaiaaudio_str_video_close(this.__wbg_ptr);
    }
    /**
     * Open an `MV*.STR` movie for video playback. Demuxes every MDEC video
     * frame's bitstream off the disc (skipping the interleaved audio) and
     * caches them, keyed by `lba`. Returns JSON
     * `{ "width", "height", "frame_count", "fps" }`. Frames are NOT decoded to
     * RGBA here - call `str_decode_frame(idx)` per displayed frame so the page
     * pays MDEC cost incrementally (a whole movie's RGBA is hundreds of MB).
     *
     * Idempotent for the same `lba`: a second open returns the cached metadata
     * without re-walking the disc. `.XA` (audio-only) files have no video and
     * come back with `frame_count: 0`.
     * @param {number} lba
     * @param {number} size
     * @returns {string}
     */
    str_video_open(lba, size) {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.legaiaaudio_str_video_open(this.__wbg_ptr, lba, size);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * JSON metadata for every VAG sample inside one VAB bank.
     * Shape: `[{ size_bytes, decoded_samples, duration_ms }, ...]`.
     * `decoded_samples` is the actual PCM length after walking the ADPCM
     * blocks (which stop at the first loop-end / garbage block), so it
     * reflects the audible length, not the raw on-disc body size. Useful
     * for the UI to dim out tiny/zero-length samples that would be
     * inaudible.
     * @param {number} prot_index
     * @param {number} vab_offset
     * @returns {string}
     */
    vab_sample_list_json(prot_index, vab_offset) {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.legaiaaudio_vab_sample_list_json(this.__wbg_ptr, prot_index, vab_offset);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Sample rate the JS side should use when playing a VAG-decoded buffer.
     * @returns {number}
     */
    vab_sample_rate() {
        const ret = wasm.legaiaaudio_vab_sample_rate(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Demux + decode an XA stream. Returns the decoded PCM of the first
     * audio channel (file_no=0, ch_no=0 typically) along with metadata
     * packed as JSON in the first method, then the PCM via this one.
     *
     * Two-step API so the JS side can show metadata (channels, sample rate)
     * before paying the decode cost.
     * @param {number} lba
     * @param {number} size
     * @returns {string}
     */
    xa_metadata_json(lba, size) {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.legaiaaudio_xa_metadata_json(this.__wbg_ptr, lba, size);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
}
if (Symbol.dispose) LegaiaAudio.prototype[Symbol.dispose] = LegaiaAudio.prototype.free;

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
     * Raw TIM bytes for battle-form atlas `atlas` (0..=6). 256x256 4bpp with
     * a 256x1 sub-CLUT row inside the TIM block.
     * @param {number} atlas
     * @returns {Uint8Array}
     */
    battle_char_atlas_bytes(atlas) {
        const ret = wasm.legaiaviewer_battle_char_atlas_bytes(this.__wbg_ptr, atlas);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * Bounding-sphere `[cx, cy, cz, r]` for the battle-form character.
     * Uses the **vertex centroid** (mean position) rather than the AABB
     * midpoint, so asymmetric poses (e.g. Vahn's stance with the weapon
     * extended past the body's X axis) don't pull the camera target off the
     * torso. Radius is the max distance from the centroid to any vertex.
     * @param {number} slot
     * @returns {Float32Array}
     */
    battle_char_mesh_bounds(slot) {
        const ret = wasm.legaiaviewer_battle_char_mesh_bounds(this.__wbg_ptr, slot);
        var v1 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * Per-vertex `[cba, tsb]` for the battle-form character.
     * @param {number} slot
     * @returns {Uint32Array}
     */
    battle_char_mesh_cba_tsb(slot) {
        const ret = wasm.legaiaviewer_battle_char_mesh_cba_tsb(this.__wbg_ptr, slot);
        var v1 = getArrayU32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * Triangle indices for the battle-form character at slot `slot`.
     * @param {number} slot
     * @returns {Uint32Array}
     */
    battle_char_mesh_indices(slot) {
        const ret = wasm.legaiaviewer_battle_char_mesh_indices(this.__wbg_ptr, slot);
        var v1 = getArrayU32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * Per-vertex normals for the battle-form character at slot `slot`.
     * @param {number} slot
     * @returns {Float32Array}
     */
    battle_char_mesh_normals(slot) {
        const ret = wasm.legaiaviewer_battle_char_mesh_normals(this.__wbg_ptr, slot);
        var v1 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * Per-vertex TMD object index for the battle-form character at slot
     * `slot`, parallel to [`Self::battle_char_mesh_positions`]. The JS-side
     * player-ANM animator uses it to apply per-bone (per-object) transforms.
     * @param {number} slot
     * @returns {Uint32Array}
     */
    battle_char_mesh_object_ids(slot) {
        const ret = wasm.legaiaviewer_battle_char_mesh_object_ids(this.__wbg_ptr, slot);
        var v1 = getArrayU32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * Per-vertex positions for the battle-form character at pack slot `slot`.
     * @param {number} slot
     * @returns {Float32Array}
     */
    battle_char_mesh_positions(slot) {
        const ret = wasm.legaiaviewer_battle_char_mesh_positions(this.__wbg_ptr, slot);
        var v1 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * Per-vertex `[u, v]` integer texel coords for the battle-form character.
     * @param {number} slot
     * @returns {Int32Array}
     */
    battle_char_mesh_uvs(slot) {
        const ret = wasm.legaiaviewer_battle_char_mesh_uvs(this.__wbg_ptr, slot);
        var v1 = getArrayI32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * JSON summary of PROT 1204 (`other5`) - the battle-form mesh pack:
     * 5 TMD slots + 7 character-atlas TIMs. Shape:
     * ```text
     * {
     *   "slots":   [{"slot":0,"label":"Vahn","disc_nobj":15,"tmd_bytes":33516,"file_offset":4}, ...],
     *   "atlases": [{"atlas":0,"clut_fb_y":490,"tim_bytes":33316,"file_offset":154628}, ...],
     *   "atlas_stride_bytes": 33316,
     *   "first_atlas_offset": 154628
     * }
     * ```
     * @returns {string}
     */
    battle_char_pack_json() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.legaiaviewer_battle_char_pack_json(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Raw disc-form TMD bytes for battle-form slot `slot`.
     * @param {number} slot
     * @returns {Uint8Array}
     */
    battle_char_tmd_bytes(slot) {
        const ret = wasm.legaiaviewer_battle_char_tmd_bytes(this.__wbg_ptr, slot);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * Build the 1 MB PSX VRAM with each of PROT 1204's seven atlas TIMs
     * uploaded **with its bundled CLUT** at the declared `(fb_x, fb_y)`
     * (rows 490..495, 497). These bundled sub-CLUTs are the pack's **authoring
     * palette** - what the Baka Fighter minigame renders with directly. Both
     * the Battle and Baka Fighter forms on the site render against this VRAM
     * with the mesh's nominal CBA ([`Self::battle_char_mesh_cba_tsb`]).
     *
     * A real turn-based battle relocates the same geometry + textures into a
     * packed per-slot VRAM band (rows 481..483) and recolours it with a
     * per-battle party palette that is a **separate, battle-allocated runtime
     * asset** (resident at RAM `0x800ebee8`+, 480 B / 15 sub-CLUTs per char) -
     * distinct from this bundled palette and **not recoverable from the disc by
     * byte search** (see `docs/formats/character-mesh.md`). Until that palette's
     * disc source is pinned (open thread - needs a battle-LOAD overlay capture),
     * the Battle form is the bundled-palette render, visually identical to Baka.
     * @returns {Uint8Array}
     */
    battle_char_vram_bytes() {
        const ret = wasm.legaiaviewer_battle_char_vram_bytes(this.__wbg_ptr);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * Battle VRAM with the **true per-battle palette** overlaid for the slots
     * whose disc palette source is known. This is the colour-correct render a
     * real turn-based battle produces - the party CLUTs decoded from the
     * character's `edstati3` record (`FUN_80052FA0`, see
     * [`legaia_asset::battle_char_palette`]) and STP-set onto the VRAM rows the
     * mesh's nominal CBA samples.
     *
     * Vahn (slot 0, extraction PROT `0863` - the `PLAYER1` file, raw TOC
     * `0x361`; see `docs/formats/cdname.md` § numbering space) is validated
     * byte-exact against a live battle VRAM capture (his tutorial-equipped
     * state via [`legaia_asset::battle_char_palette::parse_record`]). Noa
     * (slot 1, extraction `0864`) and Gala (slot 2, extraction `0865`) use the
     * equipment-robust [`legaia_asset::battle_char_palette::collect_palette`]
     * - record0 + the section separators' unequipped-default CLUTs, filtered
     * to the columns each mesh samples (validated against a full-party
     * capture: Noa ~98%, Gala 100%). All three player files load by
     * `char + 0x360` → `FUN_8003e8a8` → `toc[idx+2]` (a sector offset into
     * PROT.DAT); extraction entries `0863`/`0864`/`0865` begin exactly at
     * those player-file offsets. The Baka Fighter form keeps
     * [`Self::battle_char_vram_bytes`] (the bundled palette is the correct
     * minigame colouring).
     * @returns {Uint8Array}
     */
    battle_char_vram_bytes_battle() {
        const ret = wasm.legaiaviewer_battle_char_vram_bytes_battle(this.__wbg_ptr);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * Number of CLUT palettes available for cataloged TIM `id` (0 for
     * 16/24bpp TIMs, which carry no palette).
     * @param {number} id
     * @returns {number}
     */
    catalog_clut_count(id) {
        const ret = wasm.legaiaviewer_catalog_clut_count(this.__wbg_ptr, id);
        return ret >>> 0;
    }
    /**
     * JSON describing cataloged TIM `id` (offset, owning entry, dimensions,
     * CLUT count, byte length, fingerprint) for the info panel.
     * @param {number} id
     * @returns {string}
     */
    catalog_info_json(id) {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.legaiaviewer_catalog_info_json(this.__wbg_ptr, id);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Number of cataloged TIMs in the loaded PROT.DAT.
     * @returns {number}
     */
    catalog_len() {
        const ret = wasm.legaiaviewer_catalog_len(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Bounding-sphere `[cx, cy, cz, r]` so the JS viewer can frame the model.
     * Uses `centroid_bounds` so asymmetric poses (weapon extended, arm out)
     * don't pull the camera target off the body.
     * @param {number} slot
     * @param {number} equip_byte
     * @returns {Float32Array}
     */
    character_mesh_bounds(slot, equip_byte) {
        const ret = wasm.legaiaviewer_character_mesh_bounds(this.__wbg_ptr, slot, equip_byte);
        var v1 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * Per-vertex `[cba, tsb]` (CLUT-base / texture-page descriptor) so the
     * JS shader can resolve VRAM texel + palette per the standard PSX TMD
     * model. `2 u32` per vertex, parallel to [`Self::character_mesh_positions`].
     * @param {number} slot
     * @param {number} equip_byte
     * @returns {Uint32Array}
     */
    character_mesh_cba_tsb(slot, equip_byte) {
        const ret = wasm.legaiaviewer_character_mesh_cba_tsb(this.__wbg_ptr, slot, equip_byte);
        var v1 = getArrayU32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * Per-vertex flat/gouraud shading attribute for the field-character
     * **hybrid** render, parallel to [`Self::character_mesh_positions`]: 4
     * bytes per vertex `[r, g, b, textured_flag]`. The field-form player mesh
     * mixes textured prims (face / skin / clothing that sample the PROT 0874
     * §2 atlas - `textured_flag == 1`) with untextured flat / gouraud prims
     * (the bulk of the body - `textured_flag == 0`) that carry per-vertex RGB
     * in the TMD instead of UVs. The shader samples VRAM for textured verts
     * and uses `[r, g, b]` for untextured verts, so the body parts the pure
     * textured path would discard render in their real colours. Vertex order
     * matches the other `character_mesh_*` getters (same TMD walk).
     * @param {number} slot
     * @param {number} equip_byte
     * @returns {Uint8Array}
     */
    character_mesh_flat_colors(slot, equip_byte) {
        const ret = wasm.legaiaviewer_character_mesh_flat_colors(this.__wbg_ptr, slot, equip_byte);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * Triangle indices for the player character at pack slot `slot`,
     * `u32`, multiple of 3.
     * @param {number} slot
     * @param {number} equip_byte
     * @returns {Uint32Array}
     */
    character_mesh_indices(slot, equip_byte) {
        const ret = wasm.legaiaviewer_character_mesh_indices(this.__wbg_ptr, slot, equip_byte);
        var v1 = getArrayU32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * Per-vertex normals parallel to [`Self::character_mesh_positions`].
     * @param {number} slot
     * @param {number} equip_byte
     * @returns {Float32Array}
     */
    character_mesh_normals(slot, equip_byte) {
        const ret = wasm.legaiaviewer_character_mesh_normals(this.__wbg_ptr, slot, equip_byte);
        var v1 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * Per-vertex TMD object index for the player character at pack slot
     * `slot`, parallel to [`Self::character_mesh_positions`]. The JS-side
     * player-ANM animator uses it to apply per-bone (per-object) transforms
     * without re-uploading geometry.
     * @param {number} slot
     * @param {number} equip_byte
     * @returns {Uint32Array}
     */
    character_mesh_object_ids(slot, equip_byte) {
        const ret = wasm.legaiaviewer_character_mesh_object_ids(this.__wbg_ptr, slot, equip_byte);
        var v1 = getArrayU32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * Per-vertex positions for the player character at pack slot `slot`,
     * optionally with the equipment swap applied (`equip_byte` < 0 means
     * "no swap, draw disc-form mesh"). Empty if `slot` is out of range or
     * the disc isn't loaded.
     * @param {number} slot
     * @param {number} equip_byte
     * @returns {Float32Array}
     */
    character_mesh_positions(slot, equip_byte) {
        const ret = wasm.legaiaviewer_character_mesh_positions(this.__wbg_ptr, slot, equip_byte);
        var v1 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * Per-vertex `[u, v]` integer texel coords (parallel to
     * [`Self::character_mesh_positions`], 2 i32 per vertex). The site page
     * pairs these with the PROT 0876 atlas page to do its own NEAREST
     * sample; we keep the integer texels here instead of normalising
     * because the atlas dimensions aren't surfaced yet.
     * @param {number} slot
     * @param {number} equip_byte
     * @returns {Int32Array}
     */
    character_mesh_uvs(slot, equip_byte) {
        const ret = wasm.legaiaviewer_character_mesh_uvs(this.__wbg_ptr, slot, equip_byte);
        var v1 = getArrayI32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * JSON summary of the five character-pack slots.
     *
     * Shape:
     * ```json
     * { "slots": [
     *     { "slot": 0, "label": "Vahn", "disc_nobj": 12,
     *       "tmd_bytes": 13220,
     *       "patch": { "patched_group_index": 0,
     *                  "equip_byte_record_offset": 406 } },
     *     ...
     *   ],
     *   "patched_group_offset": 12,
     *   "group_descriptor_bytes": 28,
     *   "equip_group_zero_offset": 320,
     *   "equip_group_nonzero_offset": 292
     * }
     * ```
     * `patch` is present only for the 3 active-party slots (0..=2); slots
     * 3/4 carry the auxiliary actors with no equipment swap. Returns
     * `{"slots":[],"error":"..."}` when the disc is missing PROT 0874 or
     * the LZS section fails to decode.
     * @returns {string}
     */
    character_pack_json() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.legaiaviewer_character_pack_json(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Raw disc-form TMD bytes for slot `slot` - the same bytes the engine
     * installs into `DAT_8007C018[slot]`. Useful for an in-page .tmd
     * download / debug round-trip.
     * @param {number} slot
     * @returns {Uint8Array}
     */
    character_tmd_bytes(slot) {
        const ret = wasm.legaiaviewer_character_tmd_bytes(this.__wbg_ptr, slot);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
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
     * Number of CLUT palettes available for deep-catalog TIM `id`.
     * @param {number} id
     * @returns {number}
     */
    deep_catalog_clut_count(id) {
        const ret = wasm.legaiaviewer_deep_catalog_clut_count(this.__wbg_ptr, id);
        return ret >>> 0;
    }
    /**
     * JSON describing deep-catalog TIM `id` (owning entry, LZS section,
     * offset within the decoded section, dimensions, CLUT count, byte
     * length, fingerprint) for the info panel.
     * @param {number} id
     * @returns {string}
     */
    deep_catalog_info_json(id) {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.legaiaviewer_deep_catalog_info_json(this.__wbg_ptr, id);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Number of cataloged compressed TIMs in the loaded PROT.DAT.
     * @returns {number}
     */
    deep_catalog_len() {
        const ret = wasm.legaiaviewer_deep_catalog_len(this.__wbg_ptr);
        return ret >>> 0;
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
     * Build the 1 MB PSX VRAM with the **field-character textures** (PROT
     * 0874 **section 2**) uploaded, so the Field-form meshes render textured.
     *
     * Section 2 of the `player.lzs` container is an 8-TIM pack; entries 1/2/3
     * are the Vahn/Noa/Gala atlas pages at texpage `(832, 256)` with their
     * CLUTs on row 478 (cols 0..63 / 64..127 / 128..191). Each TIM is uploaded
     * via the retail `FUN_800198e0` semantic - image at its declared rect, CLUT
     * as a **flat horizontal strip** (`w*h` colours at one row), STP off - so
     * the meshes' per-primitive CBA columns sample the right palettes. Byte-
     * exact against a live field VRAM dump (see
     * [`legaia_asset::field_char_textures`]). The Field form renders against
     * this VRAM through the same paletted pipeline the Battle form uses.
     * @returns {Uint8Array}
     */
    field_char_vram_bytes() {
        const ret = wasm.legaiaviewer_field_char_vram_bytes(this.__wbg_ptr);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * Fog LUT bytes extracted from `SCUS_942.54` at disc-load time.
     * 4 KiB = 2048 u16 BGR555-shaped entries that the world-map overlay's
     * per-prim leaves at `0x801F7644..0x801F8690` consult on every vertex
     * (the shared depth-cue ramp; the per-kingdom hue mixes in from the
     * `fog_color` field at gp-0x2DC).
     *
     * Returns an empty Vec when no LUT was located - the JS side should
     * treat empty as "fall back to the kingdom-tinted baseline" and not
     * upload anything to the renderer.
     * @returns {Uint8Array}
     */
    fog_lut_bytes() {
        const ret = wasm.legaiaviewer_fog_lut_bytes(this.__wbg_ptr);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * Decoded RGBA8 pixels for one publisher-logo TIM (0..3). Returns
     * an empty vec when the disc doesn't have PROT 0895 or `idx` is
     * out of range. Width / height come from [`init_pak_logos_json`].
     * @param {number} idx
     * @returns {Uint8Array}
     */
    init_pak_logo_rgba(idx) {
        const ret = wasm.legaiaviewer_init_pak_logo_rgba(this.__wbg_ptr, idx);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * JSON metadata for the boot publisher-logo TIMs from PROT 0895
     * (`init.pak`). Returns an empty array `"[]"` if the disc doesn't
     * have PROT 0895 or the entry doesn't parse as init.pak.
     *
     * Each element shape:
     *   `{ "name": str, "width": u32, "height": u32, "mode": u32,
     *      "fb_x": u32, "fb_y": u32 }`
     * @returns {string}
     */
    init_pak_logos_json() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.legaiaviewer_init_pak_logos_json(this.__wbg_ptr);
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
     * Keyframes for monster `id`'s action animation at array `index` (the
     * position in [`Self::monster_animations_json`]). Same flat layout as
     * [`Self::monster_idle_animation_frames`]: six `i32` per part per frame,
     * `[tx, ty, tz, rx, ry, rz]`, with frame `f` / part `p` / component `c` at
     * `(f * part_count + p) * 6 + c`. Empty if the index is out of range or the
     * slot has no decodable animation.
     * @param {number} id
     * @param {number} index
     * @returns {Int32Array}
     */
    monster_animation_frames_at(id, index) {
        const ret = wasm.legaiaviewer_monster_animation_frames_at(this.__wbg_ptr, id, index);
        var v1 = getArrayI32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * Metadata for **every** decodable action animation of monster `id`, as a
     * JSON array in `+0x4C` action-table order:
     * `[{"action_id":N,"part_count":P,"frame_count":F}, ...]`. Array index `0`
     * is the idle loop (see [`Self::monster_idle_animation_header`]); the rest
     * are the monster's attack / spell / special actions. The array index is
     * the handle the JS viewer passes to [`Self::monster_animation_frames_at`]
     * to fetch a given action's keyframes. `"[]"` if the slot is empty / filler
     * or carries no decodable animation.
     * @param {number} id
     * @returns {string}
     */
    monster_animations_json(id) {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.legaiaviewer_monster_animations_json(this.__wbg_ptr, id);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Decode the global monster stat archive (PROT entry 867, the
     * `battle_data` block's extended footprint) into a JSON array of every
     * populated record. Sony bytes never leave the browser - the archive is
     * LZS-decoded from the user's own loaded disc, the same client-side model
     * the rest of this viewer uses; nothing is shipped with the static site.
     *
     * Shape:
     * ```json
     * { "records": [ { "id": u16, "name": "Gimard", "hp": u16, "mp": u16,
     *                  "stats": [u16; 6], "battle_stats": [u16; 6],
     *                  "magic_count": u8, "gold": u16,
     *                  "element": u8, "element_name": "fire"|null,
     *                  "exp": u16, "drop_item": u8, "drop_chance_pct": u8,
     *                  "steal_item": u8, "steal_item_name": "Incense"|null,
     *                  "steal_chance_pct": u8,
     *                  "spells": [ { "id": u8, "sp_cost": u8,
     *                               "castable": bool } ] }, ... ] }
     * ```
     *
     * Returns `{"records":[]}` when the entry isn't present (a standalone-TIM
     * or regional load that lacks PROT 867), or `{"error":...}` on a genuine
     * LZS decode failure.
     * @returns {string}
     */
    monster_archive_json() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.legaiaviewer_monster_archive_json(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Monster `id`'s mesh + baked texture + **all** action animations packed
     * into one binary glTF (`.glb`) blob - the universal format that carries
     * geometry, material, and animation together (Blender / three.js / etc.).
     * Each TMD object becomes an animated node; the texture is baked into a
     * per-palette atlas. Empty if the slot has no exportable mesh.
     * @param {number} id
     * @returns {Uint8Array}
     */
    monster_glb(id) {
        const ret = wasm.legaiaviewer_monster_glb(this.__wbg_ptr, id);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * Monster `id`'s idle animation keyframes as a flat `i32` array, six values
     * per part per frame: `[tx, ty, tz, rx, ry, rz]`. Frame `f`, part `p`,
     * component `c` is at `(f * part_count + p) * 6 + c`. Translations are
     * signed model units; rotations are unsigned 12-bit angles (`4096` = a full
     * turn). Empty if the slot has no decodable idle animation.
     * @param {number} id
     * @returns {Int32Array}
     */
    monster_idle_animation_frames(id) {
        const ret = wasm.legaiaviewer_monster_idle_animation_frames(this.__wbg_ptr, id);
        var v1 = getArrayI32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * `[part_count, frame_count]` for monster `id`'s **idle** animation (action
     * index 0). `[0, 0]` if the slot has no decodable animation. Pair with
     * [`Self::monster_idle_animation_frames`].
     * @param {number} id
     * @returns {Uint32Array}
     */
    monster_idle_animation_header(id) {
        const ret = wasm.legaiaviewer_monster_idle_animation_header(this.__wbg_ptr, id);
        var v1 = getArrayU32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * Bounding-sphere `[cx, cy, cz, r]` for monster `id`'s mesh, so the JS
     * side can frame the model without re-parsing the geometry.
     * @param {number} id
     * @returns {Float32Array}
     */
    monster_mesh_bounds(id) {
        const ret = wasm.legaiaviewer_monster_mesh_bounds(this.__wbg_ptr, id);
        var v1 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * Triangle indices for monster `id`'s mesh (`u32`, multiple of 3).
     * @param {number} id
     * @returns {Uint32Array}
     */
    monster_mesh_indices(id) {
        const ret = wasm.legaiaviewer_monster_mesh_indices(this.__wbg_ptr, id);
        var v1 = getArrayU32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * Per-vertex smooth normals for monster `id`'s mesh (parallel to
     * [`Self::monster_mesh_positions`]).
     * @param {number} id
     * @returns {Float32Array}
     */
    monster_mesh_normals(id) {
        const ret = wasm.legaiaviewer_monster_mesh_normals(this.__wbg_ptr, id);
        var v1 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * Per-vertex TMD object (body-part) index for monster `id`'s mesh, parallel
     * to [`Self::monster_mesh_positions`]. The JS idle-animation player uses it
     * to apply each animated part's per-frame transform. Empty if no mesh.
     * @param {number} id
     * @returns {Uint32Array}
     */
    monster_mesh_object_ids(id) {
        const ret = wasm.legaiaviewer_monster_mesh_object_ids(this.__wbg_ptr, id);
        var v1 = getArrayU32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * Per-vertex palette index (`cba & 0x3F`) for monster `id`'s mesh, as
     * floats (parallel to [`Self::monster_mesh_positions`]). The JS shader
     * uses it to pick the row of the palette texture.
     * @param {number} id
     * @returns {Float32Array}
     */
    monster_mesh_palette_index(id) {
        const ret = wasm.legaiaviewer_monster_mesh_palette_index(this.__wbg_ptr, id);
        var v1 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * Per-vertex `[x, y, z]` positions for monster `id`'s mesh (flat array,
     * 3 floats per vertex). Empty if the id has no mesh.
     * @param {number} id
     * @returns {Float32Array}
     */
    monster_mesh_positions(id) {
        const ret = wasm.legaiaviewer_monster_mesh_positions(this.__wbg_ptr, id);
        var v1 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * Per-vertex texture coords for monster `id`'s mesh, normalised to
     * `[0, 1]` against the texture-page dimensions (parallel to
     * [`Self::monster_mesh_positions`], 2 floats per vertex). Empty if the id
     * has no mesh or no texture.
     * @param {number} id
     * @returns {Float32Array}
     */
    monster_mesh_uvs(id) {
        const ret = wasm.legaiaviewer_monster_mesh_uvs(this.__wbg_ptr, id);
        var v1 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * `[width, height]` of monster `id`'s texture page in texels (128 or 256
     * wide, always 256 tall). `[0, 0]` if the id has no texture.
     * @param {number} id
     * @returns {Uint32Array}
     */
    monster_texture_dims(id) {
        const ret = wasm.legaiaviewer_monster_texture_dims(this.__wbg_ptr, id);
        var v1 = getArrayU32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * Monster `id`'s 4bpp texture page as one palette index (`0..=15`) per
     * texel, row-major (`width * height` bytes). Upload as an `R8UI`/`R8`
     * texture and pair with [`Self::monster_texture_palette_rgba`]. Empty if
     * the id has no texture.
     * @param {number} id
     * @returns {Uint8Array}
     */
    monster_texture_indices(id) {
        const ret = wasm.legaiaviewer_monster_texture_indices(this.__wbg_ptr, id);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * Monster `id`'s 15 palettes flattened to a `15 * 16` RGBA8 row (palette
     * `p`, colour `c` at pixel `p * 16 + c`). Index-0 transparent colours
     * carry alpha 0. Empty if the id has no texture.
     * @param {number} id
     * @returns {Uint8Array}
     */
    monster_texture_palette_rgba(id) {
        const ret = wasm.legaiaviewer_monster_texture_palette_rgba(this.__wbg_ptr, id);
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
     * 13-frame ocean CLUT animation table: 13 × 32 bytes = 416 bytes,
     * frame-0 first. Each frame is 16 BGR555 entries (the same shape as
     * the first 16 entries of [`Self::ocean_base_clut_bytes`]). The
     * runtime DMAs one frame at a time onto VRAM (0, 506) to cycle
     * the wave colours through the ocean tile.
     * @returns {Uint8Array}
     */
    ocean_animation_frames() {
        const ret = wasm.legaiaviewer_ocean_animation_frames(this.__wbg_ptr);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * Static base CLUT for the ocean tile row: 256 entries × 2 bytes
     * (BGR555 LE) = 512 bytes. The first 16 entries are the ones the
     * animation cycle overrides each frame; entries 16..255 stay fixed
     * and belong to other tiles sharing the same VRAM row.
     * @returns {Uint8Array}
     */
    ocean_base_clut_bytes() {
        const ret = wasm.legaiaviewer_ocean_base_clut_bytes(this.__wbg_ptr);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * Number of valid ocean animation frames (typically 13). Returns 0
     * when the kingdom doesn't have ocean assets.
     * @returns {number}
     */
    ocean_frame_count() {
        const ret = wasm.legaiaviewer_ocean_frame_count(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Ocean tile pixel data (4bpp indexed), 64 halfwords × 256 rows =
     * 32 768 bytes. Each byte holds 2 pixels (low nibble first). The
     * CLUT index addressing is `pixel = byte >> 4` for the high pixel
     * and `byte & 0x0F` for the low pixel. Empty when the kingdom is
     * not a world-map kingdom or the ocean TIM wasn't found.
     * @returns {Uint8Array}
     */
    ocean_texture_bytes() {
        const ret = wasm.legaiaviewer_ocean_texture_bytes(this.__wbg_ptr);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
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
     * JSON summary of every player-ANM bundle accessible from this disc.
     * Shape:
     * ```text
     * {
     *   "bundles": [
     *     {
     *       "prot_index": 4,
     *       "record_count": 69,
     *       "decoded_bytes": 96448,
     *       "records": [
     *         { "index": 0, "offset": 0x118, "size": 496, "marker_1": 0x080C },
     *         ...
     *       ]
     *     }, ...
     *   ]
     * }
     * ```
     * Surveys the corpus by walking each scene's first PROT slot
     * (parse_player_lzs descriptor count = 6, the canonical scene-bundle
     * shape) and emitting one entry per cleanly-decoded type-0x05 section.
     * @returns {string}
     */
    player_anm_corpus_json() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.legaiaviewer_player_anm_corpus_json(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Find a single player-ANM bundle by its PROT entry index and return
     * the LZS-decoded bytes. Empty if the entry doesn't carry a bundle.
     * @param {number} prot_index
     * @returns {Uint8Array}
     */
    player_anm_decoded(prot_index) {
        const ret = wasm.legaiaviewer_player_anm_decoded(this.__wbg_ptr, prot_index);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * Raw bytes of one record from the player-ANM bundle at `prot_index`.
     * Includes the per-record header (`a`, `b`, `marker_1 = 0x080C`, `flag`),
     * the 8-byte per-anim prologue, and the
     * `(frame_count × bone_count × 8)` byte frame table.
     * @param {number} prot_index
     * @param {number} record_index
     * @returns {Uint8Array}
     */
    player_anm_record_bytes(prot_index, record_index) {
        const ret = wasm.legaiaviewer_player_anm_record_bytes(this.__wbg_ptr, prot_index, record_index);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * `[bone_count, frame_count]` for one player-ANM record so the JS
     * animator can size its scratch buffers without re-walking the bundle.
     * Empty `[0, 0]` if the record doesn't exist or fails size invariants.
     * @param {number} prot_index
     * @param {number} record_index
     * @returns {Uint32Array}
     */
    player_anm_record_dims(prot_index, record_index) {
        const ret = wasm.legaiaviewer_player_anm_record_dims(this.__wbg_ptr, prot_index, record_index);
        var v1 = getArrayU32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * Per-frame bone-transform table for one player-ANM record, packed as
     * `i16` LE for ease of JS-side `Int16Array` overlay.
     *
     * Layout: `frame_count * bone_count * 4 i16` (`8` bytes per (bone, frame)
     * entry, read as 4 little-endian `i16`s). Returns an empty Vec on
     * out-of-range record or size-invariant failure.
     *
     * The semantic meaning of the 4 i16s per (bone, frame) entry is the
     * still-open thread (see `docs/formats/anm.md` § "Open threads"). The
     * working hypothesis is `(rot_x, rot_y, rot_z, _flag)` in PSX 12-bit
     * fixed-point (4096 = 360°). The viewer applies this and lets you see
     * what motion the bytes describe.
     * @param {number} prot_index
     * @param {number} record_index
     * @returns {Uint8Array}
     */
    player_anm_record_frames(prot_index, record_index) {
        const ret = wasm.legaiaviewer_player_anm_record_frames(this.__wbg_ptr, prot_index, record_index);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * Decoded per-record header for one player-ANM record. Returned as a
     * `Vec<i32>` packed as `[a, b, marker_1, flag, bone_count, frame_count,
     * frame0_bone0_u8[0..8]]` - total 14 entries (the 8 bytes after the
     * header are bone 0 of frame 0's TR entry, since the body sits
     * immediately after the 8-byte header - there is no prologue).
     * Returns an empty Vec on out-of-range record or size-invariant failure.
     * @param {number} prot_index
     * @param {number} record_index
     * @returns {Int32Array}
     */
    player_anm_record_header(prot_index, record_index) {
        const ret = wasm.legaiaviewer_player_anm_record_header(this.__wbg_ptr, prot_index, record_index);
        var v1 = getArrayI32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * Player-ANM record frames decoded into the same pose format the
     * site's `MonsterMeshView` animator consumes:
     * `Int32Array`, `6` entries per part per frame, as
     * `[tx, ty, tz, rx, ry, rz]`.
     *
     * Each 8-byte (bone, frame) entry is decoded as the retail engine does
     * it (`FUN_8001BE80`): bytes 0..4 hold three signed 12-bit translation
     * values (joint offset in actor-local space, PSX model units), bytes
     * 5/6/7 hold three u8 rotation angles that map to PSX 12-bit angles via
     * `<< 4` (so the JS animator's `4096`-unit convention applies
     * directly).
     *
     * The transforms are **absolute** per frame (NOT delta-from-frame-0):
     * frame 0 carries the rest-pose assembly transform that places each
     * TMD object at its joint position with its rest-pose orientation.
     * Applying these to objects whose vertices are in object-local space
     * produces the assembled character.
     *
     * The output is padded to `target_part_count` parts (typically the
     * TMD's `nobj`) - bones beyond the record's own `bone_count` get
     * identity transforms so the un-animated parts (e.g. field-form
     * equipment templates at groups 10/11) stay at their TMD-local
     * origin. Pass `0` to leave the part count at the record's own
     * bone_count.
     * @param {number} prot_index
     * @param {number} record_index
     * @param {number} target_part_count
     * @returns {Int32Array}
     */
    player_anm_record_pose_frames(prot_index, record_index, target_part_count) {
        const ret = wasm.legaiaviewer_player_anm_record_pose_frames(this.__wbg_ptr, prot_index, record_index, target_part_count);
        var v1 = getArrayI32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
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
     * Render cataloged TIM `id` with CLUT `clut` into the 2D canvas named
     * `canvas_id`. The catalog browser uses its own canvas (separate from
     * the PROT-entry browser's, which switches between 2D and WebGL), so it
     * takes the target id explicitly rather than the viewer's bound canvas.
     * @param {number} id
     * @param {number} clut
     * @param {string} canvas_id
     */
    render_catalog_tim(id, clut, canvas_id) {
        const ptr0 = passStringToWasm0(canvas_id, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.legaiaviewer_render_catalog_tim(this.__wbg_ptr, id, clut, ptr0, len0);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
    }
    /**
     * Render deep-catalog TIM `id` with CLUT `clut` into the 2D canvas named
     * `canvas_id`.
     * @param {number} id
     * @param {number} clut
     * @param {string} canvas_id
     */
    render_deep_catalog_tim(id, clut, canvas_id) {
        const ptr0 = passStringToWasm0(canvas_id, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.legaiaviewer_render_deep_catalog_tim(this.__wbg_ptr, id, clut, ptr0, len0);
        if (ret[1]) {
            throw takeFromExternrefTable0(ret[0]);
        }
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
     * wireframe, as `[amin, bmin, amax, bmax]` (i32) for the requested
     * axis pair (`"xz"` / `"xy"` / `"zy"`, etc). Useful for re-framing
     * the top-down camera when the overlay is toggled on. Empty vec
     * when slot 4 can't be decoded.
     * @param {number} prot_base
     * @param {string} axes
     * @returns {Int32Array}
     */
    slot4_wireframe_bounds(prot_base, axes) {
        const ptr0 = passStringToWasm0(axes, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.legaiaviewer_slot4_wireframe_bounds(this.__wbg_ptr, prot_base, ptr0, len0);
        var v2 = getArrayI32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v2;
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
     * @param {string} axes
     * @returns {Uint8Array}
     */
    slot4_wireframe_lines(prot_base, style, axes) {
        const ptr0 = passStringToWasm0(style, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ptr1 = passStringToWasm0(axes, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len1 = WASM_VECTOR_LEN;
        const ret = wasm.legaiaviewer_slot4_wireframe_lines(this.__wbg_ptr, prot_base, ptr0, len0, ptr1, len1);
        var v3 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v3;
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
     * @param {string} axes
     * @returns {Uint8Array}
     */
    slot4_wireframe_points(prot_base, axes) {
        const ptr0 = passStringToWasm0(axes, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.legaiaviewer_slot4_wireframe_points(this.__wbg_ptr, prot_base, ptr0, len0);
        var v2 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v2;
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
     * Per-vertex `[clut, tpage]` (PSX CBA + tpage words) of the walk-view
     * ground, flattened. Distinct per cell so grass / mountain / water / forest
     * cells sample their own VRAM page from the kingdom slot-0 atlas.
     * @returns {Uint16Array}
     */
    walk_ground_cba_tsb() {
        const ret = wasm.legaiaviewer_walk_ground_cba_tsb(this.__wbg_ptr);
        var v1 = getArrayU16FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 2, 2);
        return v1;
    }
    /**
     * Triangle indices of the walk-view ground (two triangles per cell quad).
     * @returns {Uint32Array}
     */
    walk_ground_indices() {
        const ret = wasm.legaiaviewer_walk_ground_indices(this.__wbg_ptr);
        var v1 = getArrayU32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * Per-vertex world positions of the walk-view continent ground
     * heightfield, flattened `[x, y, z, ...]`. Empty until a kingdom is loaded.
     * Same pre-Y-flip world frame as the landmark placement draws, so the JS
     * renderer applies the same `(1, -1, 1)` model flip (scale 1, no offset).
     * @returns {Float32Array}
     */
    walk_ground_positions() {
        const ret = wasm.legaiaviewer_walk_ground_positions(this.__wbg_ptr);
        var v1 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * Number of ground cells (quads) in the walk-view heightfield. 0 when no
     * kingdom is loaded or the heightfield couldn't be resolved.
     * @returns {number}
     */
    walk_ground_quad_count() {
        const ret = wasm.legaiaviewer_walk_ground_quad_count(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Per-vertex page-local UVs (`u8` pairs) of the walk-view ground, flattened
     * `[u, v, ...]`. Each cell's four corners cover its `32 x 32` atlas tile.
     * @returns {Uint8Array}
     */
    walk_ground_uvs() {
        const ret = wasm.legaiaviewer_walk_ground_uvs(this.__wbg_ptr);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * Number of walk-frame placed landmarks for the currently-loaded kingdom
     * (slot-1 pack meshes positioned on the continent terrain). 0 when no
     * kingdom is loaded or the walk `.MAP` / floor LUT couldn't be resolved.
     * @returns {number}
     */
    walk_placement_count() {
        const ret = wasm.legaiaviewer_walk_placement_count(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Per-placement world positions `[x, y, z, ...]` (flattened), in the same
     * pre-Y-flip `col*128` world frame as [`Self::walk_ground_positions`], so
     * the JS renderer draws each landmark with the same `(1, -1, 1)` model
     * flip at scale `1` (the slot-1 meshes are already in true world units).
     * @returns {Float32Array}
     */
    walk_placement_positions() {
        const ret = wasm.legaiaviewer_walk_placement_positions(this.__wbg_ptr);
        var v1 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * Per-placement kingdom pack-mesh slot (record `+0x10`), one `u32` per
     * walk-frame landmark in placement order. Feed each into `pack_mesh` to
     * select the mesh, then draw it at the matching
     * [`Self::walk_placement_positions`] entry.
     * @returns {Uint32Array}
     */
    walk_placement_slots() {
        const ret = wasm.legaiaviewer_walk_placement_slots(this.__wbg_ptr);
        var v1 = getArrayU32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
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

/**
 * Patch a user-supplied disc image with the chosen randomizer settings.
 *
 * `drops` / `encounters` / `chests` / `shops` / `casino` / `steals` / `arts` /
 * `doors` / `house_doors` are each `"shuffle"`, `"random"`, or `"none"`.
 * `arts` reassigns Tactical-Arts button combos (same-length, unique within
 * character; Miracle Arts untouched). `shops`
 * randomizes what town stores sell; `casino` the casino prize exchange. `door_coupling` is `"coupled"`
 * (bidirectional) or `"decoupled"` (one-way). `house_doors` honours only
 * `"shuffle"`. `starting_items` is the number of random starting consumables
 * the new game begins with (`0` = leave the vanilla Healing Leaf ×5). The
 * random fill shares the seed's capacity (7 slots, or 5 with `all_warps`) with
 * the convenience-item toggles below and takes whatever they leave, so it adds
 * on top of them. `door_of_wind` is how many Door of Wind (the warp consumable) to seed
 * into the starting bag (`0` = none); `incense` is how many Incense (the
 * encounter-rate consumable) to seed likewise (`0` = none); `speed_chain` /
 * `chicken_heart` / `good_luck_bell` seed those accessories the same way
 * (`0` = none each); `all_warps` presets the visited-towns
 * bitmask so Door of Wind can teleport to any town from the start (its own code
 * region, so it doesn't reduce the item count). `unused_enemies` adds the unused Evil Bat ids to the random-encounter
 * pool (only with `encounters = "random"`); `unused_items` adds the unused
 * "Something Good" / unnamed-accessory items to the random-fill pool (only the
 * `random` drop / chest / steal modes use it). `equipment_drops` injects a code
 * hook into the battle-end reward routine that, on a low per-battle chance,
 * grants one *extra* random weapon / armor / accessory on top of the normal
 * drop - additive, so `drops` is never disturbed. `monster_stats` / `move_power` /
 * `element_affinity` / `spell_cost` / `equip_bonus` are the battle-tuning +
 * equipment-bonus passes, each `"shuffle"` / `"random"` / `"none"`: monster
 * combat stats, special-attack power, the element-affinity matrix, spell MP
 * costs, and the equipment passive stat tuples (redistributed within each slot
 * category). `encounter_scope` widens the monster pool an
 * encounter roll draws from: `"scene"` (default - each scene's own monsters),
 * `"kingdom"` (any monster in the scene's Drake/Sebucus/Karisto kingdom), or
 * `"world"` (any monster on the disc, so late-game monsters can appear at the
 * start). Only matters when `encounters` is not `"none"`.
 * `solo_strong_encounters` (only with `encounters` set) forces any randomized
 * formation holding a monster much stronger than the area's natives down to that
 * lone enemy, so an over-strong monster is faced solo instead of in a pack.
 * `flee_exp` injects a code hook into the battle-action escape teardown so that
 * successfully running away banks a small slice of the fled fight's experience
 * into the party (vanilla awards nothing for fleeing). `seru_trade` adds an
 * in-shop trading vendor (a fourth Buy/Sell/Trade/Quit row) that swaps a party
 * member's learned Seru-magic for a different one at a fixed level, on a
 * time-bucketed schedule derived from the seed; all of it is hosted in the menu
 * overlay, so it composes with every other option here. `enemy_ally` injects a
 * code hook into battle setup so that, with a per-battle chance, a random enemy
 * is charmed onto the party's side as an uncontrolled ally (works in any fight,
 * bosses included), plus a one-word widen of the victory check so the ally isn't
 * an enemy you must defeat. `shiny_seru` injects code hooks so that, with a
 * per-battle chance, the frontmost *capturable* enemy spawns as a rare shiny
 * variant (+35% stats) whose captured Seru deals +35% damage on every future
 * cast (the flag rides the spell's level byte and is masked from the level-up +
 * menu readers).
 * `starting_level`
 * begins the new game at that character level instead of 1 (`0` or `1` =
 * vanilla; range 2..=14), seeding the lead character's XP and recomputing the
 * starting stats from the disc's growth curves. `seed` is a number or
 * any string (hashed). Returns `{ data, summary, seed }`.
 * @param {Uint8Array} image
 * @param {string} seed
 * @param {string} drops
 * @param {string} encounters
 * @param {string} encounter_scope
 * @param {string} chests
 * @param {string} shops
 * @param {string} casino
 * @param {string} steals
 * @param {string} arts
 * @param {string} doors
 * @param {string} door_coupling
 * @param {string} house_doors
 * @param {number} starting_items
 * @param {number} door_of_wind
 * @param {number} incense
 * @param {number} speed_chain
 * @param {number} chicken_heart
 * @param {number} good_luck_bell
 * @param {boolean} all_warps
 * @param {boolean} unused_enemies
 * @param {boolean} unused_items
 * @param {boolean} equipment_drops
 * @param {string} monster_stats
 * @param {string} move_power
 * @param {string} element_affinity
 * @param {string} spell_cost
 * @param {string} equip_bonus
 * @param {boolean} weapon_specialty
 * @param {number} starting_level
 * @param {boolean} solo_strong_encounters
 * @param {boolean} flee_exp
 * @param {boolean} seru_trade
 * @param {boolean} enemy_ally
 * @param {boolean} shiny_seru
 * @returns {any}
 */
export function patch_rom(image, seed, drops, encounters, encounter_scope, chests, shops, casino, steals, arts, doors, door_coupling, house_doors, starting_items, door_of_wind, incense, speed_chain, chicken_heart, good_luck_bell, all_warps, unused_enemies, unused_items, equipment_drops, monster_stats, move_power, element_affinity, spell_cost, equip_bonus, weapon_specialty, starting_level, solo_strong_encounters, flee_exp, seru_trade, enemy_ally, shiny_seru) {
    const ptr0 = passArray8ToWasm0(image, wasm.__wbindgen_malloc);
    const len0 = WASM_VECTOR_LEN;
    const ptr1 = passStringToWasm0(seed, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len1 = WASM_VECTOR_LEN;
    const ptr2 = passStringToWasm0(drops, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len2 = WASM_VECTOR_LEN;
    const ptr3 = passStringToWasm0(encounters, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len3 = WASM_VECTOR_LEN;
    const ptr4 = passStringToWasm0(encounter_scope, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len4 = WASM_VECTOR_LEN;
    const ptr5 = passStringToWasm0(chests, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len5 = WASM_VECTOR_LEN;
    const ptr6 = passStringToWasm0(shops, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len6 = WASM_VECTOR_LEN;
    const ptr7 = passStringToWasm0(casino, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len7 = WASM_VECTOR_LEN;
    const ptr8 = passStringToWasm0(steals, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len8 = WASM_VECTOR_LEN;
    const ptr9 = passStringToWasm0(arts, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len9 = WASM_VECTOR_LEN;
    const ptr10 = passStringToWasm0(doors, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len10 = WASM_VECTOR_LEN;
    const ptr11 = passStringToWasm0(door_coupling, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len11 = WASM_VECTOR_LEN;
    const ptr12 = passStringToWasm0(house_doors, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len12 = WASM_VECTOR_LEN;
    const ptr13 = passStringToWasm0(monster_stats, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len13 = WASM_VECTOR_LEN;
    const ptr14 = passStringToWasm0(move_power, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len14 = WASM_VECTOR_LEN;
    const ptr15 = passStringToWasm0(element_affinity, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len15 = WASM_VECTOR_LEN;
    const ptr16 = passStringToWasm0(spell_cost, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len16 = WASM_VECTOR_LEN;
    const ptr17 = passStringToWasm0(equip_bonus, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len17 = WASM_VECTOR_LEN;
    const ret = wasm.patch_rom(ptr0, len0, ptr1, len1, ptr2, len2, ptr3, len3, ptr4, len4, ptr5, len5, ptr6, len6, ptr7, len7, ptr8, len8, ptr9, len9, ptr10, len10, ptr11, len11, ptr12, len12, starting_items, door_of_wind, incense, speed_chain, chicken_heart, good_luck_bell, all_warps, unused_enemies, unused_items, equipment_drops, ptr13, len13, ptr14, len14, ptr15, len15, ptr16, len16, ptr17, len17, weapon_specialty, starting_level, solo_strong_encounters, flee_exp, seru_trade, enemy_ally, shiny_seru);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return takeFromExternrefTable0(ret[0]);
}

/**
 * Resolve a user seed string to the numeric seed, as a decimal string (so the
 * page can display / persist it without JS `BigInt` precision loss).
 * @param {string} seed
 * @returns {string}
 */
export function resolve_seed(seed) {
    let deferred2_0;
    let deferred2_1;
    try {
        const ptr0 = passStringToWasm0(seed, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.resolve_seed(ptr0, len0);
        deferred2_0 = ret[0];
        deferred2_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred2_0, deferred2_1, 1);
    }
}
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
        __wbg_createGain_33464d2fccb13fb8: function() { return handleError(function (arg0) {
            const ret = arg0.createGain();
            return ret;
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
        __wbg_gain_c994bc21cdd2e1b9: function(arg0) {
            const ret = arg0.gain;
            return ret;
        },
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
        __wbg_length_ba3c032602efe310: function(arg0) {
            const ret = arg0.length;
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
        __wbg_new_2fad8ca02fd00684: function() {
            const ret = new Object();
            return ret;
        },
        __wbg_new_a6b46eaf9085fbeb: function() { return handleError(function () {
            const ret = new lAudioContext();
            return ret;
        }, arguments); },
        __wbg_new_with_length_9011f5da794bf5d9: function(arg0) {
            const ret = new Uint8Array(arg0 >>> 0);
            return ret;
        },
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
        __wbg_resolve_9feb5d906ca62419: function(arg0) {
            const ret = Promise.resolve(arg0);
            return ret;
        },
        __wbg_resume_60c7fdf589dd7208: function() { return handleError(function (arg0) {
            const ret = arg0.resume();
            return ret;
        }, arguments); },
        __wbg_sampleRate_b7f221c5b3d93248: function(arg0) {
            const ret = arg0.sampleRate;
            return ret;
        },
        __wbg_set_5337f8ac82364a3f: function() { return handleError(function (arg0, arg1, arg2) {
            const ret = Reflect.set(arg0, arg1, arg2);
            return ret;
        }, arguments); },
        __wbg_set_b0d9dc239ecdb765: function(arg0, arg1, arg2) {
            arg0.set(getArrayU8FromWasm0(arg1, arg2));
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
        __wbg_set_value_78631e9dc5b69626: function(arg0, arg1) {
            arg0.value = arg1;
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
            // Cast intrinsic for `Closure(Closure { owned: true, function: Function { arguments: [NamedExternref("AudioProcessingEvent")], shim_idx: 256, ret: Unit, inner_ret: Some(Unit) }, mutable: true }) -> Externref`.
            const ret = makeMutClosure(arg0, arg1, wasm_bindgen__convert__closures_____invoke__h68646c9fea2fce23);
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
function wasm_bindgen__convert__closures_____invoke__h68646c9fea2fce23(arg0, arg1, arg2) {
    wasm.wasm_bindgen__convert__closures_____invoke__h68646c9fea2fce23(arg0, arg1, arg2);
}

const LegaiaAudioFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_legaiaaudio_free(ptr, 1));
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

function getArrayI16FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getInt16ArrayMemory0().subarray(ptr / 2, ptr / 2 + len);
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

let cachedInt16ArrayMemory0 = null;
function getInt16ArrayMemory0() {
    if (cachedInt16ArrayMemory0 === null || cachedInt16ArrayMemory0.byteLength === 0) {
        cachedInt16ArrayMemory0 = new Int16Array(wasm.memory.buffer);
    }
    return cachedInt16ArrayMemory0;
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
    cachedInt16ArrayMemory0 = null;
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
