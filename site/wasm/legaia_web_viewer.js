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
 * The three side-games playable in the browser, plus the disc they read.
 */
export class LegaiaMinigames {
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        LegaiaMinigamesFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_legaiaminigames_free(ptr, 0);
    }
    /**
     * `[bone_count, frame_count]` of one fighter's animation record.
     * Player actions index the PROT 1203 bank (`char*9 + action`), opponent
     * actions the fighter pack's own bank (typically 8 records, 0 = idle).
     * @param {number} side
     * @param {number} id
     * @param {number} action
     * @returns {Uint32Array}
     */
    baka_anim_dims(side, id, action) {
        const ret = wasm.legaiaminigames_baka_anim_dims(this.__wbg_ptr, side, id, action);
        var v1 = getArrayU32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * One fighter animation record decoded to absolute per-(frame, bone)
     * `[tx, ty, tz, rx, ry, rz]` (PSX 4096-unit angles), padded to
     * `target_part_count` parts - the same pose format the site's mesh
     * animators consume.
     * @param {number} side
     * @param {number} id
     * @param {number} action
     * @param {number} target_part_count
     * @returns {Int32Array}
     */
    baka_anim_pose_frames(side, id, action, target_part_count) {
        const ret = wasm.legaiaminigames_baka_anim_pose_frames(this.__wbg_ptr, side, id, action, target_part_count);
        var v1 = getArrayI32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * Number of animation records one fighter's bank carries (9 per player
     * character bank; the opponent packs carry their own count, idle first).
     * @param {number} side
     * @param {number} id
     * @returns {number}
     */
    baka_anim_record_count(side, id) {
        const ret = wasm.legaiaminigames_baka_anim_record_count(this.__wbg_ptr, side, id);
        return ret >>> 0;
    }
    /**
     * Commit the visitor's attack this exchange: `1`/`2`/`3` are the three
     * rock-paper-scissors throws, `4` the special. Returns `false` when the
     * fighter can't act yet (cooldown, or a choice is already pending).
     * @param {number} attack
     * @returns {boolean}
     */
    baka_choose(attack) {
        const ret = wasm.legaiaminigames_baka_choose(this.__wbg_ptr, attack);
        return ret !== 0;
    }
    /**
     * Build the duel's 1 MB PSX VRAM: the PROT 1203 HUD/stage pages, the
     * PROT 1204 party atlases (their bundled CLUT strips are the minigame's
     * own palette - see `docs/formats/character-mesh.md`), and the chosen
     * opponent's atlas last (roster 4's pack shares the `(512, 256)` page +
     * row-497 CLUT with party atlas 6; retail loads them one at a time too).
     * @param {number} opponent
     * @returns {Uint8Array}
     */
    baka_duel_vram(opponent) {
        const ret = wasm.legaiaminigames_baka_duel_vram(this.__wbg_ptr, opponent);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * Per-vertex `[cba, tsb]`, parallel to the positions.
     * @param {number} side
     * @param {number} id
     * @returns {Uint32Array}
     */
    baka_fighter_cba_tsb(side, id) {
        const ret = wasm.legaiaminigames_baka_fighter_cba_tsb(this.__wbg_ptr, side, id);
        var v1 = getArrayU32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * Per-vertex `[r, g, b, textured_flag]` for the hybrid textured / flat
     * shader path (some fighter prims are untextured flat colour).
     * @param {number} side
     * @param {number} id
     * @returns {Uint8Array}
     */
    baka_fighter_flat_rgba(side, id) {
        const ret = wasm.legaiaminigames_baka_fighter_flat_rgba(this.__wbg_ptr, side, id);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * Triangle indices for one duel fighter.
     * @param {number} side
     * @param {number} id
     * @returns {Uint32Array}
     */
    baka_fighter_indices(side, id) {
        const ret = wasm.legaiaminigames_baka_fighter_indices(this.__wbg_ptr, side, id);
        var v1 = getArrayU32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * Per-vertex TMD object index (the bone a vertex hangs from).
     * @param {number} side
     * @param {number} id
     * @returns {Uint32Array}
     */
    baka_fighter_object_ids(side, id) {
        const ret = wasm.legaiaminigames_baka_fighter_object_ids(this.__wbg_ptr, side, id);
        var v1 = getArrayU32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * `[part_count]` for one fighter (TMD object count = pose rig width).
     * @param {number} side
     * @param {number} id
     * @returns {number}
     */
    baka_fighter_part_count(side, id) {
        const ret = wasm.legaiaminigames_baka_fighter_part_count(this.__wbg_ptr, side, id);
        return ret >>> 0;
    }
    /**
     * Per-vertex positions for one duel fighter. `side` 0 = player
     * (`id` = character 0..=2), `side` 1 = opponent (`id` = roster 3..=16).
     * @param {number} side
     * @param {number} id
     * @returns {Float32Array}
     */
    baka_fighter_positions(side, id) {
        const ret = wasm.legaiaminigames_baka_fighter_positions(this.__wbg_ptr, side, id);
        var v1 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * Per-vertex `[u, v]` texel coords, parallel to the positions.
     * @param {number} side
     * @param {number} id
     * @returns {Int32Array}
     */
    baka_fighter_uvs(side, id) {
        const ret = wasm.legaiaminigames_baka_fighter_uvs(this.__wbg_ptr, side, id);
        var v1 = getArrayI32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * The HUD widget descriptor table (`DAT_801d7160`, 51 records), as JSON:
     *
     * ```json
     * [ { "scale": 4096, "page": 0, "palette": 4, "u": 48, "v": 48,
     *     "w": 112, "h": 16, "rgb_top": [160,160,255],
     *     "rgb_bottom": [255,255,255], "semi": 1, "abr": 1 }, ... ]
     * ```
     *
     * `page` resolves the record's texpage into an index of the PROT 1203
     * art pack (pair with [`Self::baka_page_rgba`]); `palette` is the CLUT
     * column within that page's 256x1 strip. Empty when either side didn't
     * decode.
     * @returns {string}
     */
    baka_hud_json() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.legaiaminigames_baka_hud_json(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * The ladder the cabinet actually serves, as `[{stage, roster}]`.
     *
     * The stage counter starts at **2** and `roster = stage + 3`, so the first
     * lap is roster ids `5..=16` - across which the prize gold is strictly
     * monotonic. Roster `3` and `4` are only reachable after the all-clear
     * wraps the counter, which is why the roster's gold column looks out of
     * order if you read it straight down.
     * @returns {string}
     */
    baka_ladder_json() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.legaiaminigames_baka_ladder_json(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * The 17 fighter names, in roster order, read out of the roster records
     * (`+0x00`, 32-byte ASCII). Empty when the overlay didn't decode.
     * @returns {string}
     */
    baka_names_json() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.legaiaminigames_baka_names_json(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * One PROT 1203 art page decoded through one of its palettes, RGBA8.
     * Pages are 256x256 4bpp; the palette index comes from the widget record.
     * @param {number} page
     * @param {number} palette
     * @returns {Uint8Array}
     */
    baka_page_rgba(page, palette) {
        const ret = wasm.legaiaminigames_baka_page_rgba(this.__wbg_ptr, page, palette);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * Pixel width of PROT 1203 art page `page` (`0` when it didn't decode).
     * @param {number} page
     * @returns {number}
     */
    baka_page_width(page) {
        const ret = wasm.legaiaminigames_baka_page_width(this.__wbg_ptr, page);
        return ret >>> 0;
    }
    /**
     * Whether the duel's presentation assets decode off this disc: the HUD
     * art + widget table, the battle-form party pack, and at least the first
     * ladder fighter's pack.
     * @returns {boolean}
     */
    baka_presentation_ready() {
        const ret = wasm.legaiaminigames_baka_presentation_ready(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * The parsed roster, for the opponent picker. The disc carries no names for
     * these fighters - only their numbers - so each row is the record's own
     * stat block:
     *
     * ```json
     * [ { "id": 1, "gold": 30, "damage_mod": 20, "crit_chance": 10,
     *     "atk_tiers": [..], "def_tiers": [..], "pattern": [2,1,3],
     *     "power": [..] }, ... ]
     * ```
     * @returns {string}
     */
    baka_roster_json() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.legaiaminigames_baka_roster_json(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * @param {number} index
     * @returns {Uint32Array}
     */
    baka_stage_cba_tsb(index) {
        const ret = wasm.legaiaminigames_baka_stage_cba_tsb(this.__wbg_ptr, index);
        var v1 = getArrayU32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * @param {number} index
     * @returns {Uint8Array}
     */
    baka_stage_flat_rgba(index) {
        const ret = wasm.legaiaminigames_baka_stage_flat_rgba(this.__wbg_ptr, index);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * @param {number} index
     * @returns {Uint32Array}
     */
    baka_stage_indices(index) {
        const ret = wasm.legaiaminigames_baka_stage_indices(this.__wbg_ptr, index);
        var v1 = getArrayU32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * Per-vertex positions of stage TMD `index` (PROT 1203 descriptor 1,
     * four meshes: three single-object dressing pieces + a 10-object set).
     * @param {number} index
     * @returns {Float32Array}
     */
    baka_stage_positions(index) {
        const ret = wasm.legaiaminigames_baka_stage_positions(this.__wbg_ptr, index);
        var v1 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * UVs / CBA-TSB / indices / flat colours of stage TMD `index`, matching
     * [`Self::baka_stage_positions`]'s vertex order.
     * @param {number} index
     * @returns {Int32Array}
     */
    baka_stage_uvs(index) {
        const ret = wasm.legaiaminigames_baka_stage_uvs(this.__wbg_ptr, index);
        var v1 = getArrayI32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * Start a best-of-3 duel: the visitor fights as roster fighter 0 (the
     * player-side default) against `opponent`. Returns `false` when the tables
     * didn't decode or the roster id is out of range.
     * @param {number} opponent
     * @param {number} seed
     * @returns {boolean}
     */
    baka_start(opponent, seed) {
        const ret = wasm.legaiaminigames_baka_start(this.__wbg_ptr, opponent, seed);
        return ret !== 0;
    }
    /**
     * Live duel state.
     *
     * ```json
     * { "live": true, "phase": "fighting"|"round_over"|"match_over",
     *   "round": 0, "hp": [3200, 2900], "hp_start": 3200,
     *   "wins": [0, 1], "combo": [0, 2], "chosen": [2, null],
     *   "can_choose": true, "gold": 30, "winner": null,
     *   "last": { "winner": 0, "draw": false, "damage": 512,
     *             "critical": false, "special": false } }
     * ```
     * @returns {string}
     */
    baka_state_json() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.legaiaminigames_baka_state_json(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Advance the duel one frame's worth of `frame_step` (the retail SM's
     * per-frame delta; `1` is a normal frame).
     * @param {number} frame_step
     */
    baka_tick(frame_step) {
        wasm.legaiaminigames_baka_tick(this.__wbg_ptr, frame_step);
    }
    /**
     * Whether the dance's art pack + widget table decoded off this disc.
     * When `false` the page falls back to its own glyphs - and says so.
     * @returns {boolean}
     */
    dance_art_ready() {
        const ret = wasm.legaiaminigames_dance_art_ready(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * Render `seconds` of the dance BGM to interleaved stereo i16 PCM at
     * [`Self::dance_bgm_rate`], through the clean-room SPU + sequencer -
     * the same path the audio page uses. Empty when the pair didn't decode.
     * @param {boolean} alt
     * @param {number} seconds
     * @returns {Int16Array}
     */
    dance_bgm_pcm_i16(alt, seconds) {
        const ret = wasm.legaiaminigames_dance_bgm_pcm_i16(this.__wbg_ptr, alt, seconds);
        var v1 = getArrayI16FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 2, 2);
        return v1;
    }
    /**
     * Sample rate of [`Self::dance_bgm_pcm_i16`] (the SPU's 44.1 kHz).
     * @returns {number}
     */
    dance_bgm_rate() {
        const ret = wasm.legaiaminigames_dance_bgm_rate(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Whether the dance BGM pair (VAB + SEQ in one `music_01` entry)
     * resolves: `{"ok":true,"prot":1048,"alt":true}`. The overlay starts one
     * of two songs by mode (`FUN_801cf470` state 6 branches on the mode
     * global); `alt = false` picks extraction 1048, `true` picks 1054.
     * @returns {string}
     */
    dance_bgm_ready_json() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.legaiaminigames_dance_bgm_ready_json(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * The whole decoded step chart, for the page's scrolling note lane:
     * `{"rows":[[u8; 32], ...]}` (one row per difficulty lane).
     * @returns {string}
     */
    dance_chart_json() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.legaiaminigames_dance_chart_json(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Face window metadata:
     * `[{ "w":80, "h":64, "face":[0,0,32,48], "poses":5 }, ...]` - `w`/`h`
     * are the buffer dimensions [`Self::dance_face_rgba`] returns, `face`
     * the sub-rect that is the visible face (the rest of the window is
     * neighbouring atlas cells).
     * @returns {string}
     */
    dance_face_meta_json() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.legaiaminigames_dance_face_meta_json(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * One dancer's live face window as RGBA8: the strip's top window with
     * pose `pose` stamped in by the two traced `MoveImage` blits
     * (`FUN_801d03c4`). `dancer` is the rig index `0..=3`: `0` = **Noa**
     * (her field atlas, PROT 0874 §2), `1..=3` = the pack strips. Pair with
     * [`Self::dance_face_meta_json`] for dimensions. Empty when the strip
     * didn't decode.
     * @param {number} dancer
     * @param {number} pose
     * @returns {Uint8Array}
     */
    dance_face_rgba(dancer, pose) {
        const ret = wasm.legaiaminigames_dance_face_rgba(this.__wbg_ptr, dancer, pose);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * The 256x256 HUD page (VRAM `(512, 0)`) decoded through palette
     * `palette` of its own row-500 CLUT strip, as RGBA8. Palette selection
     * is load-bearing: the widget table names a palette per element, and
     * the beat-track flash / note tint are pure CLUT swaps over the same
     * texels (`0x7D08` idle / `0x7D0D` flash / `0x7D0E` notes).
     * @param {number} palette
     * @returns {Uint8Array}
     */
    dance_hud_page_rgba(palette) {
        const ret = wasm.legaiaminigames_dance_hud_page_rgba(this.__wbg_ptr, palette);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * The traced HUD geometry, so the page draws at retail positions rather
     * than invented ones. Everything here is an immediate in a traced
     * emitter (`FUN_801d231c` / `FUN_801d2524` / `FUN_801d32f8` /
     * `FUN_801d3e28` and the banner spawn sites in `FUN_801cf470` /
     * `FUN_801d1af4` / `FUN_801d40dc`), on the retail 320x240 frame.
     * Widgets draw **centred** on their `(x, y)`.
     *
     * - `score_boxes`: the three boxes; the **human dancer is the centre
     *   box** (`FUN_801d231c` draws score slot 0 at the centre digit base).
     * - `digit_bases`: x of digit slot 0 per box; 8 slots step 16, only
     *   significant digits draw, so a 1-digit score lands at `base + 112`.
     * - `track`: the beat lane - arrow, caps, 12 body tiles, the scrolling
     *   notes (`x = track.x + i*16 - (phase*16/281 + 5) - 4`, clip window
     *   `[track.x, track.x + 0x50)`), stock markers at `y + 16`.
     * - `banners`: spawn points (`FUN_801d3fd0` stores `x<<3` and draws at
     *   `>>3`): countdown / READY / GO / FINISH at centre, ratings below,
     *   stars flanking by tier (`0x38`/`0x50` for Cool/Great).
     * `screen_offset` is the global drawing-environment offset: every HUD
     * element in the retail VRAM capture (score-box border, track pill,
     * marker arrow) sits exactly 4 lines below the emitter's own `y`, so the
     * active draw environment carries a `+4` Y offset. Pixel-pinned against
     * the parked minigame capture.
     * @returns {string}
     */
    dance_layout_json() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.legaiaminigames_dance_layout_json(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Judge a directional press. `dir` is the chart symbol (`1` / `2` / `3`).
     * Returns `"miss"` / `"hit"` / `"sequence"` (`"none"` with no live run).
     * @param {number} dir
     * @returns {string}
     */
    dance_press(dir) {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.legaiaminigames_dance_press(this.__wbg_ptr, dir);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * The retail cue ids (`FUN_801d1af4` sites): miss, the three combo-tier
     * stings, the run-start and intro cues.
     * @returns {string}
     */
    dance_sfx_cue_ids() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.legaiaminigames_dance_sfx_cue_ids(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * The dance's own cue bank (descriptors PROT 1228, samples PROT 1231):
     * `[{ "id":528, "program":0, "tone":1, "note":66, "rate":44100 }, ...]`.
     * Empty when either entry didn't decode - PROT 1231 sits in the PROT
     * TOC's zeroed tail, so an image whose TOC truncates early loses it.
     * @returns {string}
     */
    dance_sfx_json() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.legaiaminigames_dance_sfx_json(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Decode one dance cue to mono PCM (`i16`). Empty when absent.
     * @param {number} cue
     * @returns {Int16Array}
     */
    dance_sfx_pcm(cue) {
        const ret = wasm.legaiaminigames_dance_sfx_pcm(this.__wbg_ptr, cue);
        var v1 = getArrayI16FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 2, 2);
        return v1;
    }
    /**
     * Playback rate for [`Self::dance_sfx_pcm`] (`0` when absent).
     * @param {number} cue
     * @returns {number}
     */
    dance_sfx_rate(cue) {
        const ret = wasm.legaiaminigames_dance_sfx_rate(this.__wbg_ptr, cue);
        return ret >>> 0;
    }
    /**
     * Start a dance run on the disc's baked step chart. `long_song` picks the
     * long song-length limit. Returns `false` when the chart didn't decode.
     * @param {boolean} long_song
     * @returns {boolean}
     */
    dance_start(long_song) {
        const ret = wasm.legaiaminigames_dance_start(this.__wbg_ptr, long_song);
        return ret !== 0;
    }
    /**
     * Live dance state.
     *
     * ```json
     * { "live": true, "score": 0, "gauge": 0, "lane": 0, "beat": 3,
     *   "phase": 40, "period": 281, "window": 210, "accuracy": 3200, "dead_zone": false,
     *   "judged": 2, "displayed": 3, "song_timer": 900, "song_len": 16860,
     *   "over": false, "passed": false }
     * ```
     *
     * **`judged` is the step to press.** Retail splits the chart lookup
     * (`FUN_801d1820`) into two halves: the hit judge (`FUN_801d1960`) matches
     * a press against the raw chart cell (`judged`), while the display /
     * auto-feed half substitutes the held-sequence symbol `3` on every 4th
     * beat (`displayed`). Both are surfaced; only `judged` scores. `0` = the
     * beat carries no step, `null` = the dead zone between beats.
     * @returns {string}
     */
    dance_state_json() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.legaiaminigames_dance_state_json(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * One layer of a good-step **hit sting**. Retail keys these directly
     * (`FUN_801d3d78`, bypassing the cue ring): a step picks `r = rand() % 3`
     * and keys VAB program 1 tones `2r` (layer 0) and `2r + 1` (layer 1)
     * together at note `0x3C + r`. Mono i16 PCM; empty when absent.
     * @param {number} r
     * @param {number} layer
     * @returns {Int16Array}
     */
    dance_sting_pcm(r, layer) {
        const ret = wasm.legaiaminigames_dance_sting_pcm(this.__wbg_ptr, r, layer);
        var v1 = getArrayI16FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 2, 2);
        return v1;
    }
    /**
     * Playback rate for [`Self::dance_sting_pcm`] (`0` when absent).
     * @param {number} r
     * @param {number} layer
     * @returns {number}
     */
    dance_sting_rate(r, layer) {
        const ret = wasm.legaiaminigames_dance_sting_rate(this.__wbg_ptr, r, layer);
        return ret >>> 0;
    }
    /**
     * Advance the beat clock by `frames` frames (the retail clock steps
     * `frame_delta * 10` phase units per frame).
     * @param {number} frames
     */
    dance_tick(frames) {
        wasm.legaiaminigames_dance_tick(this.__wbg_ptr, frames);
    }
    /**
     * The overlay's HUD widget table, one record per id `0..=33`:
     *
     * ```json
     * [ { "u":0, "v":0, "w":16, "h":24, "palette":0,
     *     "semi":0, "top":[255,255,255], "bottom":[255,255,255] }, ... ]
     * ```
     *
     * Cells index the HUD page; `palette` is the row-500 CLUT column the
     * record names (the emitters swap it at runtime for the flash states).
     * @returns {string}
     */
    dance_widgets_json() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.legaiaminigames_dance_widgets_json(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Load a full Mode2/2352 disc image (or a raw `PROT.DAT`), parse the TOC,
     * and pre-decode every minigame table that resolves. Returns a JSON status
     * object naming which games came up:
     *
     * ```json
     * { "entries": 1290,
     *   "dance":  { "ok": true, "rows": 3, "beats": 32 },
     *   "baka":   { "ok": true, "fighters": 17 },
     *   "slot":   { "ok": true, "payouts": [.., ..] } }
     * ```
     *
     * A game whose overlay or table doesn't resolve reports `{"ok":false}` with
     * a reason rather than throwing - a regional / modded disc can still play
     * the others.
     * @param {Uint8Array} bytes
     * @returns {string}
     */
    load_disc(bytes) {
        let deferred3_0;
        let deferred3_1;
        try {
            const ptr0 = passArray8ToWasm0(bytes, wasm.__wbindgen_malloc);
            const len0 = WASM_VECTOR_LEN;
            const ret = wasm.legaiaminigames_load_disc(this.__wbg_ptr, ptr0, len0);
            var ptr2 = ret[0];
            var len2 = ret[1];
            if (ret[3]) {
                ptr2 = 0; len2 = 0;
                throw takeFromExternrefTable0(ret[2]);
            }
            deferred3_0 = ptr2;
            deferred3_1 = len2;
            return getStringFromWasm0(ptr2, len2);
        } finally {
            wasm.__wbindgen_free(deferred3_0, deferred3_1, 1);
        }
    }
    constructor() {
        const ret = wasm.legaiaminigames_new();
        this.__wbg_ptr = ret;
        LegaiaMinigamesFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * Whether the slot machine's art pack decoded off this disc. When `false`
     * the page must fall back to symbol *ids*, not to invented artwork.
     * @returns {boolean}
     */
    slot_art_ready() {
        const ret = wasm.legaiaminigames_slot_art_ready(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * Tally the latched payout into the balance and return to idle. Returns
     * the credited coins. [`Self::slot_tick`] already does this on the frame a
     * spin resolves; this stays for hosts that drive the tally themselves.
     * @returns {number}
     */
    slot_collect() {
        const ret = wasm.legaiaminigames_slot_collect(this.__wbg_ptr);
        return ret;
    }
    /**
     * The coin readout's font strip - the `"COIN"` label (`x = 0..64`) followed
     * by digits `0..=9` at `x = 64 + d * 16` - as a 224x16 RGBA8 buffer
     * (`FUN_801d2914`, CLUT `0x7A8D`).
     * @returns {Uint8Array}
     */
    slot_digits_rgba() {
        const ret = wasm.legaiaminigames_slot_digits_rgba(this.__wbg_ptr);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * The 3 HUD widget descriptors, as parsed off the disc:
     *
     * ```json
     * [ { "u": 0, "v": 16, "w": 127, "h": 239,
     *     "page": 4, "palette": 0, "texpage": [640, 0], "clut": [0, 494] }, ... ]
     * ```
     *
     * `page` is the index into the art pack the record's texpage resolves to,
     * and `palette` the CLUT column - so a caller can re-decode the same traced
     * rect through a different palette. That is not academic: the retail
     * rasteriser `FUN_801d2cc0` lets the *call site* override the record's CLUT
     * (the id's high field swaps in `0x7D0F`), so a widget's on-screen colour
     * is not always the one its descriptor names.
     * @returns {string}
     */
    slot_hud_json() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.legaiaminigames_slot_hud_json(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * One of the 3 HUD widgets the retail rasteriser `FUN_801d2cc0` draws from
     * the descriptor table `DAT_801d347c`, decoded through *its own* texpage +
     * CLUT: `0` = the cabinet panel, `1` = the "COIN" label, `2` = the cash-out
     * cursor. RGBA8; pair with [`Self::slot_hud_json`] for the dimensions.
     * @param {number} index
     * @returns {Uint8Array}
     */
    slot_hud_rgba(index) {
        const ret = wasm.legaiaminigames_slot_hud_rgba(this.__wbg_ptr, index);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * A whole art page decoded through one of its 16 palettes, as RGBA8. Every
     * on-screen rect the machine draws is traced to its emitter, so a caller
     * pairs this with the cells in [`Self::slot_scene_json`] rather than
     * cropping by eye. Pages 0..=3 are 256x256; page 4 is 512x256.
     * @param {number} page
     * @param {number} palette
     * @returns {Uint8Array}
     */
    slot_page_rgba(page, palette) {
        const ret = wasm.legaiaminigames_slot_page_rgba(this.__wbg_ptr, page, palette);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * Pixel width of art page `page` (`0` when the pack didn't decode).
     * @param {number} page
     * @returns {number}
     */
    slot_page_width(page) {
        const ret = wasm.legaiaminigames_slot_page_width(this.__wbg_ptr, page);
        return ret >>> 0;
    }
    /**
     * The machine's **paytable / coin info panel** - HUD record 0, the 127x239
     * board `FUN_801cfff0` draws at screen `(560, 128)` ("x30 back", "x9 back",
     * "Bonus games", with the coin readout under it). RGBA8.
     *
     * It has its own entry point because its page is sampled as **8bpp** (the
     * texpage attribute's colour bit), not the 4bpp its TIM header declares -
     * decoding it as the header claims yields noise.
     * @returns {Uint8Array}
     */
    slot_panel_rgba() {
        const ret = wasm.legaiaminigames_slot_panel_rgba(this.__wbg_ptr);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * The machine's **single input**: one press means whatever the machine's
     * phase says it means. Folds the cabinet's three stop buttons onto one
     * key by taking them in sequence - press to spin, then press once per
     * reel, left to right.
     *
     * Returns what the press did:
     * - `"spin"` - idle, and the bet was charged (the reels are spinning up);
     * - `"spinup"` - the reels are still ramping, so retail refuses a stop.
     *   The host may hold the press and re-issue it when `can_stop` opens;
     * - `"stop"` - the next still-spinning reel took its stop;
     * - `"collect"` - a press landed on a resolved spin before the frame
     *   tally ran: it was tallied, but the balance can't fund another spin;
     * - `"broke"` - idle and under the 3-coin gate. The machine is empty; the
     *   host racks a new one;
     * - `"none"` - no machine, or it has cashed out.
     * @returns {string}
     */
    slot_press() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.legaiaminigames_slot_press(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * The live reel positions (`DAT_801d3cc0`) - fixed-point angles whose high
     * byte is the strip row and whose low byte is the sub-symbol fraction. The
     * renderer needs the fraction: the reel is a 3D cylinder and the fraction is
     * what rotates it between symbols.
     * @returns {Int32Array}
     */
    slot_reel_pos() {
        const ret = wasm.legaiaminigames_slot_reel_pos(this.__wbg_ptr);
        var v1 = getArrayI32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * The slot machine's **3D scene**, as the overlay's own rodata defines it,
     * plus the projection that puts it on the retail 640x240 framebuffer.
     *
     * The retail machine is not a sprite collage: every element is a quad in a
     * 3D scene projected through the GTE (see
     * [`legaia_asset::minigame_slot_scene`]). This hands the page the same
     * scene graph, in model space, so it can project it itself:
     *
     * ```json
     * { "proj": { "ofx": 253, "ofy": 118.5, "z0": 9324, "sx0": 0.2547,
     *             "aspect": 2, "xscale": 6, "w": 640, "h": 240 },
     *   "paylines":  [ { "a":[-640,-192,-768], "b":[640,-192,-768] }, ... ],
     *   "row_offsets": [[1,1,1],[0,0,0],[-1,-1,-1],[-1,0,1],[1,0,-1]],
     *   "medallions":[ { "pos":[-602,-192,-800], "art":1 }, ... ],
     *   "lamps":     [ { "pos":[632,-192,-800] }, ... ],
     *   "pedestals": [ { "pos":[-384,480,-800] }, ... ],
     *   "marquee":   [ { "pos":[-554,-560,-800], "clut":0, "half":[1024,320],
     *                    "cell":[0,0,64,64] }, ... ],
     *   "reels":     { "x":[-512,-128,256], "w":256, "faces":8,
     *                  "angle_base":896, "angle_step":256 },
     *   "cells": { "medallion":[168,128,32,32], "lamp_lit":[0,224,16,16], ... },
     *   "dots":  { "cols":78, "rows":13, "x0":-429, "y0":-640, "dx":11, "dy":12,
     *              "z":-800, "page":3, "blink_palettes":[0,1], "u_per_nibble":4 },
     *   "messages": [ { "w":84, "h":13, "bitmap":"0,0,1,..." }, ... ] }
     * ```
     *
     * `messages` are the dot-matrix marquee's 21 bitmaps, one palette *nibble*
     * per dot (`0` = unlit); `bitmap` is a comma-separated row-major run.
     * @returns {string}
     */
    slot_scene_json() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.legaiaminigames_slot_scene_json(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Whether the machine's 3D scene graph decoded off this disc.
     * @returns {boolean}
     */
    slot_scene_ready() {
        const ret = wasm.legaiaminigames_slot_scene_ready(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * The retail cue ids, so the page never has to hard-code a number:
     * `{"reel_stop":522,"payout_tick":521,"reach":512,"reach1":513,"reach2":514}`.
     * @returns {string}
     */
    slot_sfx_cue_ids() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.legaiaminigames_slot_sfx_cue_ids(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * The cue ids this disc's slot bank actually defines, with the VAB voice
     * each one keys:
     *
     * ```json
     * [ { "id": 522, "program": 1, "tone": 6, "note": 66, "rate": 46616 }, ... ]
     * ```
     *
     * `id` is decimal (`522` = `0x20A`, the reel-stop click).
     * @returns {string}
     */
    slot_sfx_json() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.legaiaminigames_slot_sfx_json(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Decode one cue to mono PCM (`i16`). Empty when the cue isn't in the bank.
     * @param {number} cue
     * @returns {Int16Array}
     */
    slot_sfx_pcm(cue) {
        const ret = wasm.legaiaminigames_slot_sfx_pcm(this.__wbg_ptr, cue);
        var v1 = getArrayI16FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 2, 2);
        return v1;
    }
    /**
     * The rate [`Self::slot_sfx_pcm`]'s samples must be played back at - the
     * cue's note against the VAG's own centre note *is* the pitch, so this
     * carries it. `0` when the cue isn't in the bank.
     * @param {number} cue
     * @returns {number}
     */
    slot_sfx_rate(cue) {
        const ret = wasm.legaiaminigames_slot_sfx_rate(this.__wbg_ptr, cue);
        return ret >>> 0;
    }
    /**
     * Charge the bet and start a spin. `false` when the machine isn't idle or
     * the balance is under the 3-coin gate.
     * @returns {boolean}
     */
    slot_spin() {
        const ret = wasm.legaiaminigames_slot_spin(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * Start a slot session on the disc's payout table with `balance` coins in
     * the machine. Returns `false` when the payout table didn't decode.
     * @param {number} seed
     * @param {number} balance
     * @returns {boolean}
     */
    slot_start(seed, balance) {
        const ret = wasm.legaiaminigames_slot_start(this.__wbg_ptr, seed, balance);
        return ret !== 0;
    }
    /**
     * Live machine state. `window` is the 3x3 grid of symbol ids actually on
     * screen (`window[reel][0..3]` = top / payline / bottom row), read off the
     * live reel positions so the page can render a spinning machine.
     *
     * ```json
     * { "live": true, "phase": "idle"|"spinning"|"stopping"|"payout"|"cashed_out",
     *   "balance": 97, "cost": 3, "can_spin": true, "can_stop": false,
     *   "stopped": 0, "feature_mode": 0, "bonus_spins": 0, "net_take": 6,
     *   "window": [[4,7,1],[2,2,9],[0,3,3]],
     *   "payouts": [..],
     *   "last": { "line": 0, "symbol": 7, "payout": 30,
     *             "bonus_triggered": false, "bonus_spin": false } }
     * ```
     * @returns {string}
     */
    slot_state_json() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.legaiaminigames_slot_state_json(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Stop the leftmost still-spinning reel. `false` when stopping isn't
     * allowed yet (the reels are still spinning up).
     * @returns {boolean}
     */
    slot_stop() {
        const ret = wasm.legaiaminigames_slot_stop(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * The 20-symbol display strip of `reel`, as the renderer reads it.
     * @param {number} reel
     * @returns {Uint8Array}
     */
    slot_strip(reel) {
        const ret = wasm.legaiaminigames_slot_strip(this.__wbg_ptr, reel);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * One reel symbol (`0..=9`) as a 64x64 RGBA8 buffer, at the exact cell and
     * **per-symbol CLUT** the retail reel renderer `FUN_801d0fa8` samples
     * (`U = (sym & 3) * 0x40`, `V = (sym & 0xC) * 0x10`, CLUT `0x7A80 + sym`).
     *
     * The palette is load-bearing: symbols 0/1/2 are one piece of artwork
     * recoloured three ways, and so are 4/5. Empty when the art didn't decode.
     * @param {number} sym
     * @returns {Uint8Array}
     */
    slot_symbol_rgba(sym) {
        const ret = wasm.legaiaminigames_slot_symbol_rgba(this.__wbg_ptr, sym);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * Advance the reels one frame and **tally a resolved spin automatically**.
     *
     * The retail cabinet has three stop buttons and a payout tray; a browser
     * page has one key. Collecting is therefore not an input here: the moment
     * the third reel lands and the spin evaluates
     * ([`SlotPhase::Payout`]), this runs the machine's own state-4 credit
     * ([`SlotMachine::collect`] - the payout arithmetic is untouched) and the
     * machine drops back to idle. The evaluated spin stays latched in
     * `last_result`, so the host can keep the winning line lit until the next
     * spin is charged. Returns the coins credited on this frame (`0` on a
     * losing spin or any frame that didn't resolve one).
     * @returns {number}
     */
    slot_tick() {
        const ret = wasm.legaiaminigames_slot_tick(this.__wbg_ptr);
        return ret;
    }
}
if (Symbol.dispose) LegaiaMinigames.prototype[Symbol.dispose] = LegaiaMinigames.prototype.free;

/**
 * Bridge object the play page instantiates once. Holds a `World` +
 * `MenuRuntime` for the disc-free path, and - once `load_disc` has run - a
 * `SceneHost` plus the render state for the scene it is running.
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
     * Attempt to start the WebAudio backend. Must be called from a user-gesture
     * handler (browser autoplay policy). `true` on success.
     * @returns {boolean}
     */
    audio_init() {
        const ret = wasm.legaiaruntime_audio_init(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * `true` if a disc has been loaded.
     * @returns {boolean}
     */
    disc_loaded() {
        const ret = wasm.legaiaruntime_disc_loaded(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * Boot a named CDNAME scene (e.g. `"town01"`) and assemble everything the
     * page draws. This is the real field entry: the scene's assets, the
     * walkability grid + elevation overrides, the MAN system script, the player
     * install, the encounter session. World-map labels (`map01`..`map03`) route
     * through the world-map entry, which installs the overworld controller
     * instead.
     *
     * Returns the same JSON as [`Self::state_json`]. Throws when the disc isn't
     * loaded or the label is unknown.
     * @param {string} name
     * @returns {string}
     */
    enter_field(name) {
        let deferred3_0;
        let deferred3_1;
        try {
            const ptr0 = passStringToWasm0(name, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len0 = WASM_VECTOR_LEN;
            const ret = wasm.legaiaruntime_enter_field(this.__wbg_ptr, ptr0, len0);
            var ptr2 = ret[0];
            var len2 = ret[1];
            if (ret[3]) {
                ptr2 = 0; len2 = 0;
                throw takeFromExternrefTable0(ret[2]);
            }
            deferred3_0 = ptr2;
            deferred3_1 = len2;
            return getStringFromWasm0(ptr2, len2);
        } finally {
            wasm.__wbindgen_free(deferred3_0, deferred3_1, 1);
        }
    }
    /**
     * @returns {Uint16Array}
     */
    field_ground_cba_tsb() {
        const ret = wasm.legaiaruntime_field_ground_cba_tsb(this.__wbg_ptr);
        var v1 = getArrayU16FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 2, 2);
        return v1;
    }
    /**
     * @returns {Uint32Array}
     */
    field_ground_indices() {
        const ret = wasm.legaiaruntime_field_ground_indices(this.__wbg_ptr);
        var v1 = getArrayU32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * @returns {Float32Array}
     */
    field_ground_positions() {
        const ret = wasm.legaiaruntime_field_ground_positions(this.__wbg_ptr);
        var v1 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * @returns {number}
     */
    field_ground_quad_count() {
        const ret = wasm.legaiaruntime_field_ground_quad_count(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {Uint8Array}
     */
    field_ground_uvs() {
        const ret = wasm.legaiaruntime_field_ground_uvs(this.__wbg_ptr);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * Select + build environment-pack slot `slot`; subsequent `field_mesh_*`
     * reads return that mesh.
     * @param {number} slot
     * @returns {number}
     */
    field_mesh(slot) {
        const ret = wasm.legaiaruntime_field_mesh(this.__wbg_ptr, slot);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * @returns {Uint16Array}
     */
    field_mesh_cba_tsb() {
        const ret = wasm.legaiaruntime_field_mesh_cba_tsb(this.__wbg_ptr);
        var v1 = getArrayU16FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 2, 2);
        return v1;
    }
    /**
     * @returns {Uint8Array}
     */
    field_mesh_flat_rgba() {
        const ret = wasm.legaiaruntime_field_mesh_flat_rgba(this.__wbg_ptr);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * @returns {Uint32Array}
     */
    field_mesh_indices() {
        const ret = wasm.legaiaruntime_field_mesh_indices(this.__wbg_ptr);
        var v1 = getArrayU32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * Select + build environment-pack slot `slot` **posed at frame 0** of
     * scene ANM record `anim_id - 1` - the rest state of a placed prop whose
     * object bind names a clip (cupboard doors closed on the cabinet's front
     * face, the windmill's sails on their hub). Falls back to the raw
     * object-local mesh when the pose can't resolve (no scene bundle, or the
     * clip's bone count doesn't match the mesh's object count - retail's
     * count-equality contract, `FUN_8001B964`), exactly as the native
     * play-window falls back to its unposed instance. `anim_id == 0` is the
     * plain unposed build ([`Self::field_mesh`]).
     * @param {number} slot
     * @param {number} anim_id
     * @returns {number}
     */
    field_mesh_posed(slot, anim_id) {
        const ret = wasm.legaiaruntime_field_mesh_posed(this.__wbg_ptr, slot, anim_id);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * @returns {Float32Array}
     */
    field_mesh_positions() {
        const ret = wasm.legaiaruntime_field_mesh_positions(this.__wbg_ptr);
        var v1 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * @returns {Uint8Array}
     */
    field_mesh_uvs() {
        const ret = wasm.legaiaruntime_field_mesh_uvs(this.__wbg_ptr);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * Per-placement object-bind animation id (parallel to
     * [`Self::field_placement_slots`]). `0` = unposed; nonzero = draw the
     * slot's mesh through [`Self::field_mesh_posed`] with this id, or the
     * prop's multi-object parts heap on the origin.
     * @returns {Uint32Array}
     */
    field_placement_anim_ids() {
        const ret = wasm.legaiaruntime_field_placement_anim_ids(this.__wbg_ptr);
        var v1 = getArrayU32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * @returns {Float32Array}
     */
    field_placement_positions() {
        const ret = wasm.legaiaruntime_field_placement_positions(this.__wbg_ptr);
        var v1 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * @returns {Uint16Array}
     */
    field_placement_rot_y() {
        const ret = wasm.legaiaruntime_field_placement_rot_y(this.__wbg_ptr);
        var v1 = getArrayU16FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 2, 2);
        return v1;
    }
    /**
     * Per-placement env-pack slot (parallel to
     * [`Self::field_placement_positions`] / [`Self::field_placement_rot_y`]).
     * @returns {Uint32Array}
     */
    field_placement_slots() {
        const ret = wasm.legaiaruntime_field_placement_slots(this.__wbg_ptr);
        var v1 = getArrayU32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * `{"pack_count", "placements", "terrain", "ground_quads"}` for the status
     * line; `null` before a scene is entered.
     * @returns {string}
     */
    field_status_json() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.legaiaruntime_field_status_json(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * @returns {Float32Array}
     */
    field_terrain_positions() {
        const ret = wasm.legaiaruntime_field_terrain_positions(this.__wbg_ptr);
        var v1 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * @returns {Uint16Array}
     */
    field_terrain_rot_y() {
        const ret = wasm.legaiaruntime_field_terrain_rot_y(this.__wbg_ptr);
        var v1 = getArrayU16FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 2, 2);
        return v1;
    }
    /**
     * @returns {Uint32Array}
     */
    field_terrain_slots() {
        const ret = wasm.legaiaruntime_field_terrain_slots(this.__wbg_ptr);
        var v1 = getArrayU32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * Field VRAM (1 MB) - the image every mesh below samples. The engine's own
     * scene VRAM, not a viewer-side rebuild.
     * @returns {Uint8Array}
     */
    field_vram_bytes() {
        const ret = wasm.legaiaruntime_field_vram_bytes(this.__wbg_ptr);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
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
     * `raw_bytes` may be either a Mode2/2352 full disc image (`.bin`) - PROT.DAT
     * and CDNAME.TXT are extracted via an ISO9660 walk - or the raw contents of
     * `PROT.DAT`. `cdname_text` overrides any CDNAME.TXT found on the disc; pass
     * an empty string to use the disc's own.
     *
     * Returns the number of PROT entries parsed. Nothing leaves the browser.
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
     * @returns {boolean}
     */
    menu_is_open() {
        const ret = wasm.legaiaruntime_menu_is_open(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
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
     * Tick the scaffold menu with a packed button mask
     * (`cross | circle<<1 | triangle<<2 | square<<3 | up<<4 | down<<5 |
     * left<<6 | right<<7`).
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
     * Open the disc-free scaffold menu (the headless [`MenuRuntime`] - the
     * retail pause menu's screens are a native-only draw path today).
     */
    open_menu() {
        wasm.legaiaruntime_open_menu(this.__wbg_ptr);
    }
    /**
     * The scene's NPC / actor catalog. Shape:
     * `{"anm_prot": 4, "npcs": [{"i", "slot", "model", "anim", "nobj",
     * "kind", "target_map", "dialog", "conditional", "x", "z"}, ...]}`.
     * `null` before a scene is entered.
     * @returns {string}
     */
    play_npc_catalog_json() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.legaiaruntime_play_npc_catalog_json(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Build catalog entry `i`'s mesh (hybrid: textured + vertex-colour prims,
     * with per-vertex bone ids). Returns `i`.
     *
     * Mirrors the native window's field-NPC bind: a special
     * (`model >= 0xF0`) resolves out of the world's **global TMD pool**
     * rather than the scene's, and when the placement names a clip the TMD's
     * object table is truncated to the clip's bone count (the objects past it
     * are equipment-swap templates the clip never poses - drawn, they'd
     * litter the actor's feet with raw parts).
     * @param {number} i
     * @returns {number}
     */
    play_npc_mesh(i) {
        const ret = wasm.legaiaruntime_play_npc_mesh(this.__wbg_ptr, i);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * @returns {Uint16Array}
     */
    play_npc_mesh_cba_tsb() {
        const ret = wasm.legaiaruntime_play_npc_mesh_cba_tsb(this.__wbg_ptr);
        var v1 = getArrayU16FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 2, 2);
        return v1;
    }
    /**
     * @returns {Uint8Array}
     */
    play_npc_mesh_flat_rgba() {
        const ret = wasm.legaiaruntime_play_npc_mesh_flat_rgba(this.__wbg_ptr);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * @returns {Uint32Array}
     */
    play_npc_mesh_indices() {
        const ret = wasm.legaiaruntime_play_npc_mesh_indices(this.__wbg_ptr);
        var v1 = getArrayU32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * Per-vertex TMD object index for the built NPC mesh - the bone each
     * vertex hangs from. The page's animator keys its per-frame `R . v + T`
     * on this.
     * @returns {Uint32Array}
     */
    play_npc_mesh_object_ids() {
        const ret = wasm.legaiaruntime_play_npc_mesh_object_ids(this.__wbg_ptr);
        var v1 = getArrayU32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * @returns {Float32Array}
     */
    play_npc_mesh_positions() {
        const ret = wasm.legaiaruntime_play_npc_mesh_positions(this.__wbg_ptr);
        var v1 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * @returns {Uint8Array}
     */
    play_npc_mesh_uvs() {
        const ret = wasm.legaiaruntime_play_npc_mesh_uvs(this.__wbg_ptr);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * `[frame_count, bone_count]` of catalog entry `i`'s clip; `[0, 0]` when
     * it has none. `bone_count` is the clip's own count - the stride of
     * [`Self::play_npc_pose_frames`], and the count
     * [`Self::play_npc_mesh`] truncated the object table to.
     * @param {number} i
     * @returns {Uint32Array}
     */
    play_npc_pose_dims(i) {
        const ret = wasm.legaiaruntime_play_npc_pose_dims(this.__wbg_ptr, i);
        var v1 = getArrayU32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * Catalog entry `i`'s clip, decoded to the pose format the JS animator
     * consumes: `6` entries per bone per frame (`[tx, ty, tz, rx, ry, rz]`,
     * absolute). Empty when the placement names no clip or its bundle is
     * unavailable. An NPC's clip is its placement `anim_id - 1` in the
     * scene's own ANM bundle (`docs/formats/anm.md` § per-scene bundle); a
     * global-pool special's indexes the PROT 0874 locomotion bundle instead
     * (the native window's bundle split).
     * @param {number} i
     * @returns {Int32Array}
     */
    play_npc_pose_frames(i) {
        const ret = wasm.legaiaruntime_play_npc_pose_frames(this.__wbg_ptr, i);
        var v1 = getArrayI32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * Live world state of every catalogued NPC, flattened
     * `[x, y, z, facing_units, ...]` in catalog order. Positions come from the
     * **world** (`field_npc_positions`), so an NPC walking its MAN-authored
     * route walks on screen; the MAN placement anchor is the fallback for one
     * that has never moved. `y` is the floor height under the NPC.
     * @returns {Float32Array}
     */
    play_npc_transforms() {
        const ret = wasm.legaiaruntime_play_npc_transforms(this.__wbg_ptr);
        var v1 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * `true` when the lead's field mesh resolved out of the global TMD pool.
     * @returns {boolean}
     */
    player_has_mesh() {
        const ret = wasm.legaiaruntime_player_has_mesh(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * @returns {Uint16Array}
     */
    player_mesh_cba_tsb() {
        const ret = wasm.legaiaruntime_player_mesh_cba_tsb(this.__wbg_ptr);
        var v1 = getArrayU16FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 2, 2);
        return v1;
    }
    /**
     * @returns {Uint8Array}
     */
    player_mesh_flat_rgba() {
        const ret = wasm.legaiaruntime_player_mesh_flat_rgba(this.__wbg_ptr);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * Player mesh geometry (object-local; pair with
     * [`Self::player_mesh_positions`], which poses it).
     * @returns {Uint32Array}
     */
    player_mesh_indices() {
        const ret = wasm.legaiaruntime_player_mesh_indices(this.__wbg_ptr);
        var v1 = getArrayU32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * The player's vertices **posed at the current frame**: the world's live
     * `pose_frame` (idle clip standing, walk clip moving), composed per bone.
     * Falls back to the object-local rest geometry when no clip is installed -
     * which is what a lead outside the Vahn / Noa / Gala trio gets, since the
     * locomotion bundle only banks those three.
     * @returns {Float32Array}
     */
    player_mesh_positions() {
        const ret = wasm.legaiaruntime_player_mesh_positions(this.__wbg_ptr);
        var v1 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * @returns {Uint8Array}
     */
    player_mesh_uvs() {
        const ret = wasm.legaiaruntime_player_mesh_uvs(this.__wbg_ptr);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * `[world_x, world_y, world_z, facing_units]` for the player actor.
     * `facing_units` is the engine heading (`render_26`, PSX 12-bit; `0` =
     * travelling `+Z`); the world coords are the raw retail frame (`+Y` down).
     * @returns {Float32Array}
     */
    player_transform() {
        const ret = wasm.legaiaruntime_player_transform(this.__wbg_ptr);
        var v1 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * Active scene mode as a stable enum string (`Field`, `WorldMap`, ...).
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
     * Tell the engine where the camera is looking, so the free-movement
     * controller remaps the d-pad camera-relative ("up" walks away from the
     * camera). PSX 12-bit angle units (`4096` = a full turn); the field
     * controller quantises it to the nearest quarter-turn, as retail does.
     * @param {number} units
     */
    set_camera_azimuth(units) {
        wasm.legaiaruntime_set_camera_azimuth(this.__wbg_ptr, units);
    }
    /**
     * Route this frame's pad word into the engine. Bit layout is the PSX digital
     * pad ([`legaia_engine_core::input::PadButton`]): `0x0008` Start, `0x0010`
     * Up, `0x0020` Right, `0x0040` Down, `0x0080` Left, `0x1000` Triangle,
     * `0x2000` Circle, `0x4000` Cross, `0x8000` Square. Edge detection is the
     * engine's - just hand it the held set each frame.
     * @param {number} mask
     */
    set_pad(mask) {
        wasm.legaiaruntime_set_pad(this.__wbg_ptr, mask);
    }
    /**
     * One-line engine state for the HUD:
     * ```text
     * { "scene": "town01", "frame": 421, "mode": "Field",
     *   "actors": 12, "npcs": 9,
     *   "player": { "x": 2688, "y": -256, "z": 2432, "facing": 2048,
     *               "walking": true },
     *   "dialog": { "text": "...", "options": ["Yes", "No"], "cursor": 0 } }
     * ```
     * `dialog` is `null` when no box is up.
     * @returns {string}
     */
    state_json() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.legaiaruntime_state_json(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Advance the engine one frame. Returns `""` normally, or the label of the
     * scene the engine just walked into (a door / warp) - the page rebuilds its
     * render state whenever the return is non-empty.
     * @returns {string}
     */
    tick_frame() {
        let deferred2_0;
        let deferred2_1;
        try {
            const ret = wasm.legaiaruntime_tick_frame(this.__wbg_ptr);
            var ptr1 = ret[0];
            var len1 = ret[1];
            if (ret[3]) {
                ptr1 = 0; len1 = 0;
                throw takeFromExternrefTable0(ret[2]);
            }
            deferred2_0 = ptr1;
            deferred2_1 = len1;
            return getStringFromWasm0(ptr1, len1);
        } finally {
            wasm.__wbindgen_free(deferred2_0, deferred2_1, 1);
        }
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
     * The loaded scene's NPC catalog as JSON. Shape:
     * ```text
     * {
     *   "scene": "town01",
     *   "anm_prot": 4,            // null when the scene ships no ANM bundle
     *   "special_count": 3,       // party / savepoint heads (not listed)
     *   "unposable_count": 0,     // multi-object actors with no pose source
     *   "npcs": [
     *     { "i": 0,               // catalog index -> field_npc_mesh(i)
     *       "slot": 7,            // MAN partition-1 record index
     *       "model": 42,          // scene TMD-pool index (the mesh identity)
     *       "anim": 9,            // ANM record index + 1; 0 = no clip
     *       "nobj": 12,           // TMD object count
     *       "kind": "talk",       // talk | door | prop
     *       "target_map": null,
     *       "dialog": "Hey, Vahn!",
     *       "conditional": false, // true = script-gated spawn (parked off-map)
     *       "x": 1088, "z": 2624  // spawn, world units
     *     }, ...
     *   ]
     * }
     * ```
     * `null` when no catalog is loaded.
     * @returns {string}
     */
    field_npc_catalog_json() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.legaiaviewer_field_npc_catalog_json(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Build (and cache) one catalogued NPC's mesh. The **field-hybrid** build:
     * textured prims that sample the scene VRAM plus the untextured
     * flat/gouraud prims that carry per-vertex RGB, in one vertex stream with
     * parallel per-vertex object ids - so the page can both render the
     * colour-only body parts and compose the ANM pose. Returns the catalog
     * index.
     * @param {number} catalog_idx
     * @returns {number}
     */
    field_npc_mesh(catalog_idx) {
        const ret = wasm.legaiaviewer_field_npc_mesh(this.__wbg_ptr, catalog_idx);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * Bounding sphere `[cx, cy, cz, r]` of the built mesh, for camera framing.
     * @returns {Float32Array}
     */
    field_npc_mesh_bounds() {
        const ret = wasm.legaiaviewer_field_npc_mesh_bounds(this.__wbg_ptr);
        var v1 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * @returns {Uint16Array}
     */
    field_npc_mesh_cba_tsb() {
        const ret = wasm.legaiaviewer_field_npc_mesh_cba_tsb(this.__wbg_ptr);
        var v1 = getArrayU16FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 2, 2);
        return v1;
    }
    /**
     * Per-vertex `[r, g, b, textured_flag]` for the hybrid render.
     * @returns {Uint8Array}
     */
    field_npc_mesh_flat_rgba() {
        const ret = wasm.legaiaviewer_field_npc_mesh_flat_rgba(this.__wbg_ptr);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * @returns {Uint32Array}
     */
    field_npc_mesh_indices() {
        const ret = wasm.legaiaviewer_field_npc_mesh_indices(this.__wbg_ptr);
        var v1 = getArrayU32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * Per-vertex TMD object index, parallel to the positions - the bone each
     * vertex belongs to. The page's animator keys the per-frame
     * `R . v + T` on this.
     * @returns {Uint32Array}
     */
    field_npc_mesh_object_ids() {
        const ret = wasm.legaiaviewer_field_npc_mesh_object_ids(this.__wbg_ptr);
        var v1 = getArrayU32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * @returns {Float32Array}
     */
    field_npc_mesh_positions() {
        const ret = wasm.legaiaviewer_field_npc_mesh_positions(this.__wbg_ptr);
        var v1 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * @returns {Uint8Array}
     */
    field_npc_mesh_uvs() {
        const ret = wasm.legaiaviewer_field_npc_mesh_uvs(this.__wbg_ptr);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * @returns {Uint16Array}
     */
    field_scene_ground_cba_tsb() {
        const ret = wasm.legaiaviewer_field_scene_ground_cba_tsb(this.__wbg_ptr);
        var v1 = getArrayU16FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 2, 2);
        return v1;
    }
    /**
     * @returns {Uint32Array}
     */
    field_scene_ground_indices() {
        const ret = wasm.legaiaviewer_field_scene_ground_indices(this.__wbg_ptr);
        var v1 = getArrayU32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * Ground-heightfield accessors (same layout as the kingdom
     * `walk_ground_*` family; empty when the scene has no resolvable floor
     * grid).
     * @returns {Float32Array}
     */
    field_scene_ground_positions() {
        const ret = wasm.legaiaviewer_field_scene_ground_positions(this.__wbg_ptr);
        var v1 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * @returns {number}
     */
    field_scene_ground_quad_count() {
        const ret = wasm.legaiaviewer_field_scene_ground_quad_count(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * @returns {Uint8Array}
     */
    field_scene_ground_uvs() {
        const ret = wasm.legaiaviewer_field_scene_ground_uvs(this.__wbg_ptr);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * Select the active environment-pack slot and build its mesh: the
     * textured prims whose pages/CLUTs are resident in the field VRAM
     * (matches the engine's per-prim filter) **plus** the untextured
     * `F*`/`G*` vertex-colour prims, merged by [`build_hybrid_env_mesh`]
     * (the engine-shell's colour-mesh pipeline sibling). Returns the slot,
     * or an error when out of range. Subsequent `field_scene_mesh_*` calls
     * read the built mesh.
     * @param {number} slot
     * @returns {number}
     */
    field_scene_mesh(slot) {
        const ret = wasm.legaiaviewer_field_scene_mesh(this.__wbg_ptr, slot);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
    }
    /**
     * @returns {Uint16Array}
     */
    field_scene_mesh_cba_tsb() {
        const ret = wasm.legaiaviewer_field_scene_mesh_cba_tsb(this.__wbg_ptr);
        var v1 = getArrayU16FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 2, 2);
        return v1;
    }
    /**
     * Per-vertex `[r, g, b, flag]` bytes for the current mesh's hybrid
     * flat-colour render (`flag` 255 = textured vertex, sample VRAM; 0 =
     * untextured vertex, use the RGB). **Empty** when the mesh carries no
     * untextured prims - the JS side then skips binding the attribute and
     * the draw behaves exactly like the pure-textured path.
     * @returns {Uint8Array}
     */
    field_scene_mesh_flat_rgba() {
        const ret = wasm.legaiaviewer_field_scene_mesh_flat_rgba(this.__wbg_ptr);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * @returns {Uint32Array}
     */
    field_scene_mesh_indices() {
        const ret = wasm.legaiaviewer_field_scene_mesh_indices(this.__wbg_ptr);
        var v1 = getArrayU32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * @returns {Float32Array}
     */
    field_scene_mesh_positions() {
        const ret = wasm.legaiaviewer_field_scene_mesh_positions(this.__wbg_ptr);
        var v1 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * @returns {Uint8Array}
     */
    field_scene_mesh_uvs() {
        const ret = wasm.legaiaviewer_field_scene_mesh_uvs(this.__wbg_ptr);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * Number of TMDs in the loaded field scene's environment pack. 0 when
     * no field scene is loaded.
     * @returns {number}
     */
    field_scene_pack_count() {
        const ret = wasm.legaiaviewer_field_scene_pack_count(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Per-placement world positions `[x, y, z, ...]` (flattened), same
     * pre-Y-flip world frame as the ground heightfield (draw with the shared
     * `(1, -1, 1)` model flip at scale 1).
     * @returns {Float32Array}
     */
    field_scene_placement_positions() {
        const ret = wasm.legaiaviewer_field_scene_placement_positions(this.__wbg_ptr);
        var v1 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * Per-placement authored yaw (object record `+0x0A`), PSX angle units
     * (`4096` = full revolution), in placement order. Convert with
     * `rotY = -(rot & 0xFFF) * Math.PI / 2048` for `placementModelScaled*`.
     * @returns {Uint16Array}
     */
    field_scene_placement_rot_y() {
        const ret = wasm.legaiaviewer_field_scene_placement_rot_y(this.__wbg_ptr);
        var v1 = getArrayU16FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 2, 2);
        return v1;
    }
    /**
     * Per-placement env-pack slot, one `u32` per placed object. Feed each
     * into [`Self::field_scene_mesh`] and draw at the matching
     * [`Self::field_scene_placement_positions`] entry.
     * @returns {Uint32Array}
     */
    field_scene_placement_slots() {
        const ret = wasm.legaiaviewer_field_scene_placement_slots(this.__wbg_ptr);
        var v1 = getArrayU32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * One-line JSON status for the UI:
     * `{"name", "pack_count", "placements", "terrain", "ground_quads"}`.
     * @returns {string}
     */
    field_scene_status_json() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.legaiaviewer_field_scene_status_json(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Per-terrain-tile world positions `[x, y, z, ...]` (flattened).
     * @returns {Float32Array}
     */
    field_scene_terrain_positions() {
        const ret = wasm.legaiaviewer_field_scene_terrain_positions(this.__wbg_ptr);
        var v1 = getArrayF32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * Per-terrain-tile authored yaw, same encoding as
     * [`Self::field_scene_placement_rot_y`].
     * @returns {Uint16Array}
     */
    field_scene_terrain_rot_y() {
        const ret = wasm.legaiaviewer_field_scene_terrain_rot_y(this.__wbg_ptr);
        var v1 = getArrayU16FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 2, 2);
        return v1;
    }
    /**
     * Per-terrain-tile env-pack slot (the dense `CELL_VISIBLE` decor layer).
     * @returns {Uint32Array}
     */
    field_scene_terrain_slots() {
        const ret = wasm.legaiaviewer_field_scene_terrain_slots(this.__wbg_ptr);
        var v1 = getArrayU32FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 4, 4);
        return v1;
    }
    /**
     * Field-mode VRAM bytes (1 MB) shared by every env-pack mesh + the
     * ground heightfield. Empty when no field scene is loaded.
     * @returns {Uint8Array}
     */
    field_scene_vram_bytes() {
        const ret = wasm.legaiaviewer_field_scene_vram_bytes(this.__wbg_ptr);
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
     *                  "spells": [ { "id": u8, "agl_cost": u8,
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
     * Place mesh handle `mesh` at `(tx, ty, tz)` with `rot_y` radians about
     * +Y and uniform `scale` - the same triple the page's
     * `placementModelScaledY` builds its model matrix from.
     * @param {number} mesh
     * @param {number} tx
     * @param {number} ty
     * @param {number} tz
     * @param {number} rot_y
     * @param {number} scale
     */
    scene_export_add_instance(mesh, tx, ty, tz, rot_y, scale) {
        wasm.legaiaviewer_scene_export_add_instance(this.__wbg_ptr, mesh, tx, ty, tz, rot_y, scale);
    }
    /**
     * Register a reusable mesh (the exact streams the page renders:
     * `positions` f32 xyz PSX-space, `uvs` u8 page-local texel pairs,
     * `cba_tsb` u16 `[cba, tsb]` pairs, u32 triangle indices, and the
     * optional hybrid `flat_rgba` side channel - pass an empty array for
     * pure-textured meshes). Returns the mesh handle for
     * [`Self::scene_export_add_instance`], or `u32::MAX` when no session
     * is open.
     * @param {string} name
     * @param {Float32Array} positions
     * @param {Uint8Array} uvs
     * @param {Uint16Array} cba_tsb
     * @param {Uint32Array} indices
     * @param {Uint8Array} flat_rgba
     * @returns {number}
     */
    scene_export_add_mesh(name, positions, uvs, cba_tsb, indices, flat_rgba) {
        const ptr0 = passStringToWasm0(name, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ptr1 = passArrayF32ToWasm0(positions, wasm.__wbindgen_malloc);
        const len1 = WASM_VECTOR_LEN;
        const ptr2 = passArray8ToWasm0(uvs, wasm.__wbindgen_malloc);
        const len2 = WASM_VECTOR_LEN;
        const ptr3 = passArray16ToWasm0(cba_tsb, wasm.__wbindgen_malloc);
        const len3 = WASM_VECTOR_LEN;
        const ptr4 = passArray32ToWasm0(indices, wasm.__wbindgen_malloc);
        const len4 = WASM_VECTOR_LEN;
        const ptr5 = passArray8ToWasm0(flat_rgba, wasm.__wbindgen_malloc);
        const len5 = WASM_VECTOR_LEN;
        const ret = wasm.legaiaviewer_scene_export_add_mesh(this.__wbg_ptr, ptr0, len0, ptr1, len1, ptr2, len2, ptr3, len3, ptr4, len4, ptr5, len5);
        return ret >>> 0;
    }
    /**
     * Start a fresh export session named `name` (becomes the glTF root
     * node name). Discards any prior unfinished session.
     * @param {string} name
     */
    scene_export_begin(name) {
        const ptr0 = passStringToWasm0(name, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.legaiaviewer_scene_export_begin(this.__wbg_ptr, ptr0, len0);
    }
    /**
     * Bake the accumulated session into `.glb` bytes and close it. Returns
     * an empty array when the session is missing or contains no drawable
     * geometry.
     * @returns {Uint8Array}
     */
    scene_export_finish() {
        const ret = wasm.legaiaviewer_scene_export_finish(this.__wbg_ptr);
        var v1 = getArrayU8FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
        return v1;
    }
    /**
     * Supply the 1 MiB VRAM image (`1024*512` LE u16 words - the same bytes
     * the page uploads to its R16UI texture) the atlas bake reads from.
     * @param {Uint8Array} bytes
     */
    scene_export_set_vram(bytes) {
        const ptr0 = passArray8ToWasm0(bytes, wasm.__wbindgen_malloc);
        const len0 = WASM_VECTOR_LEN;
        wasm.legaiaviewer_scene_export_set_vram(this.__wbg_ptr, ptr0, len0);
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
     * Load a CDNAME scene (e.g. `"town01"`, `"korb3"`) as an **assembled
     * full map**: field-mode VRAM + the environment mesh pack + the `.MAP`
     * placement / terrain draws + the walk-ground heightfield. Returns the
     * environment pack's TMD count (the `field_scene_mesh` slot space).
     *
     * Requires a full disc image (CDNAME.TXT resolves the scene block).
     * World-map scenes (`map01..03`) load their walk-frame landmark
     * placements; every other field scene loads the placed-object +
     * terrain-tile layers.
     * @param {string} name
     * @returns {number}
     */
    set_scene_field(name) {
        const ptr0 = passStringToWasm0(name, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.legaiaviewer_set_scene_field(this.__wbg_ptr, ptr0, len0);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        return ret[0] >>> 0;
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
     * Load a CDNAME scene and catalog every NPC / actor its MAN places.
     * Loads the field scene first when it isn't already resident (so
     * `field_scene_vram_bytes` is the VRAM these meshes sample). Returns the
     * number of catalogued placements.
     * @param {string} name
     * @returns {number}
     */
    set_scene_npcs(name) {
        const ptr0 = passStringToWasm0(name, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.legaiaviewer_set_scene_npcs(this.__wbg_ptr, ptr0, len0);
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
     * Per-placement authored yaw (object record `+0x0A`), one value per
     * walk-frame landmark in placement order, in PSX angle units (`4096` =
     * full revolution) - the Sebucus island bridges' quarter-turns and the
     * decoration layer's per-tree variety. The JS renderer converts with
     * `rotY = -(rot & 0xFFF) * Math.PI / 2048` (retail's yaw sense is the
     * opposite of `placementModelScaled*`'s).
     * @returns {Uint16Array}
     */
    walk_placement_rot_y() {
        const ret = wasm.legaiaviewer_walk_placement_rot_y(this.__wbg_ptr);
        var v1 = getArrayU16FromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 2, 2);
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
            // Cast intrinsic for `Closure(Closure { owned: true, function: Function { arguments: [NamedExternref("AudioProcessingEvent")], shim_idx: 108, ret: Unit, inner_ret: Some(Unit) }, mutable: true }) -> Externref`.
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
const LegaiaMinigamesFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_legaiaminigames_free(ptr, 1));
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

function passArray16ToWasm0(arg, malloc) {
    const ptr = malloc(arg.length * 2, 2) >>> 0;
    getUint16ArrayMemory0().set(arg, ptr / 2);
    WASM_VECTOR_LEN = arg.length;
    return ptr;
}

function passArray32ToWasm0(arg, malloc) {
    const ptr = malloc(arg.length * 4, 4) >>> 0;
    getUint32ArrayMemory0().set(arg, ptr / 4);
    WASM_VECTOR_LEN = arg.length;
    return ptr;
}

function passArray8ToWasm0(arg, malloc) {
    const ptr = malloc(arg.length * 1, 1) >>> 0;
    getUint8ArrayMemory0().set(arg, ptr / 1);
    WASM_VECTOR_LEN = arg.length;
    return ptr;
}

function passArrayF32ToWasm0(arg, malloc) {
    const ptr = malloc(arg.length * 4, 4) >>> 0;
    getFloat32ArrayMemory0().set(arg, ptr / 4);
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
