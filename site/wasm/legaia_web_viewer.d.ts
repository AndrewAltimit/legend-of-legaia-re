/* tslint:disable */
/* eslint-disable */

/**
 * The site's Tactical-Arts animation host: a disc, plus one character's
 * assembled battle mesh + art-clip bank at a time.
 */
export class LegaiaArts {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Art clip `index`'s pose frames (the position in `set_character`'s
     * `arts` array). Empty when the index is out of range or that record's
     * stream did not decode - the page falls back to the idle pose.
     */
    art_pose_frames(index: number): Int32Array;
    /**
     * The SFX cue id an art strike fires: the art record's documented generic
     * "play sound" Hit Effect Cue kind. Resolve it to audio through
     * [`crate::sfx_view::LegaiaSfx`].
     */
    art_strike_cue(): number;
    /**
     * Frames of art clip `index` on which the page should fire the strike
     * sound cue ([`Self::art_strike_cue`]), ascending. See [`strike_frames`]
     * for what they are and why they are a fit rather than a traced timing.
     * Empty when the clip didn't decode.
     */
    art_strike_frames(index: number): Uint32Array;
    /**
     * The arts-voice PCM for the art at bank index `art_index`: mono i16 at
     * the rate reported in `set_character`'s `voice.channels[..].rate`
     * (37 800 Hz). The clip is the XA channel the art's `FUN_8004C140`
     * candidate pool selects (the art's `voice_channel`), trimmed of its
     * trailing silence. Empty when the character has no voice bank (raw
     * `PROT.DAT` load, Terra, demux failure) or the art has no voice entry.
     * Also exposed by-channel via [`Self::voice_channel_pcm_i16`].
     */
    art_voice_pcm_i16(art_index: number): Int16Array;
    /**
     * Bake the current character's assembled battle mesh **plus its whole
     * battle-animation bank** into a binary glTF (`.glb`) for download:
     * one node per rigid TMD object (the engine's `R . v + T` pose model
     * expressed as native TRS keyframe channels), textured from the same
     * runtime VRAM the canvas renders (every sampled `(cba, tsb-page)` pair
     * baked into one RGBA atlas). Animation 0 is the battle idle; every
     * art-bank record whose keyframe stream decoded follows, named by its
     * inline HUD art name where the record carries one, else by its
     * `anim_id` in hex (duplicated names get the hex id appended). Each
     * clip's timeline runs at its retail rate byte (`7.5 * rate`).
     *
     * Everything is baked client-side off the visitor's own disc; nothing
     * is uploaded. Empty until [`Self::set_character`] (or if the mesh has
     * nothing to export).
     */
    export_character_glb(): Uint8Array;
    /**
     * The idle loop's pose frames (see [`flatten_pose_frames`] layout).
     * Empty when the character has no decodable idle stream.
     */
    idle_pose_frames(): Int32Array;
    /**
     * Load a full Mode2/2352 disc image (or a raw `PROT.DAT`) and parse the
     * TOC. Returns `{"entries": N}` JSON; errors throw. On a full disc the
     * arts-voice banks ([`VOICE_XA_FILE`] = `XA2.XA` / `XA4.XA` / `XA6.XA`) are sliced out
     * alongside `PROT.DAT`; a raw `PROT.DAT` load simply has no voice audio.
     */
    load_disc(bytes: Uint8Array): string;
    /**
     * Bounding sphere `[cx, cy, cz, r]` (vertex centroid + max distance),
     * so the page can frame the model before the first pose lands.
     */
    mesh_bounds(): Float32Array;
    /**
     * Per-vertex `[cba, tsb]`, parallel to the positions.
     */
    mesh_cba_tsb(): Uint32Array;
    /**
     * Triangle indices (`u32`, multiple of 3).
     */
    mesh_indices(): Uint32Array;
    /**
     * Per-vertex TMD object index (the bone a vertex hangs from), parallel
     * to the positions.
     */
    mesh_object_ids(): Uint32Array;
    /**
     * Per-vertex positions of the current character's assembled battle mesh
     * (flat `f32`, 3 per vertex). Empty until [`Self::set_character`].
     */
    mesh_positions(): Float32Array;
    /**
     * Per-vertex `[u, v]` integer texel coords, parallel to the positions.
     */
    mesh_uvs(): Int32Array;
    constructor();
    /**
     * Assemble character `cslot` (0=Vahn, 1=Noa, 2=Gala, 3=Terra) and decode
     * its art-clip bank. Returns a JSON summary the page keys everything on:
     *
     * ```json
     * { "ok": true, "character": "Vahn", "part_count": 17,
     *   "idle": { "frames": 24, "rate": 2 },
     *   "arts": [ { "index": 0, "anim_id": 16, "name": "", "combo": [3,3],
     *               "rate": 4, "base": true, "ok": true, "frames": 20,
     *               "why": null }, ... ] }
     * ```
     *
     * `name` is the record's inline HUD art-name string (empty on the
     * un-named base records); `combo` the arts-matcher direction bytes
     * (`1=L 2=R 3=D 4=U`) - the page matches its curated art cards against
     * both. `{"ok":false,"why":...}` when the character doesn't assemble.
     */
    set_character(cslot: number): string;
    /**
     * The arts-voice PCM of the current character's XA channel `channel`,
     * regardless of any art mapping. Lets the page (and the listening aid)
     * address a specific voice clip directly. Empty when out of range or the
     * character has no voice bank.
     */
    voice_channel_pcm_i16(channel: number): Int16Array;
    /**
     * The 1 MB PSX VRAM for the current character: band-0 texture pixels at
     * the pinned retail placement + the character's decoded battle palette.
     */
    vram_bytes(): Uint8Array;
}

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
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Sample rate of the browser's BGM `AudioContext`, or 0 when the BGM
     * output hasn't been opened yet. Surfaced to the JS console for
     * diagnostics when playback speed is off.
     */
    bgm_device_rate(): number;
    /**
     * Sample rate produced by [`Self::render_bgm_pcm_i16`] (the SPU's
     * internal 44.1 kHz). Surfaced so the JS side can build a correct
     * WAV header for `decodeAudioData`.
     */
    bgm_render_rate(): number;
    /**
     * Decode one VAG sample to mono i16 PCM at `vab_sample_rate()`.
     * Empty when the sample doesn't exist or has zero length.
     */
    decode_vab_sample_i16(prot_index: number, vab_offset: number, sample_idx: number): Int16Array;
    /**
     * Decode XA stream and return the i16 PCM for the channel at `stream_idx`
     * (index into the `xa_metadata_json` array). Empty when out of range.
     */
    decode_xa_stream_i16(lba: number, size: number, stream_idx: number): Int16Array;
    /**
     * JSON list of every BGM pair (`pBAV` + `pQES` in the same PROT entry).
     * Shape: `[{ prot_index, vab_offset, seq_offset, program_count, sample_count, ppqn, bpm }, ...]`.
     */
    enumerate_bgm_pairs_json(): string;
    /**
     * JSON list of every VAB sound bank in the loaded disc.
     * Shape: `[{ prot_index, vab_offset, version, program_count, sample_count, has_seq }, ...]`.
     */
    enumerate_vabs_json(): string;
    /**
     * JSON list of every `*.STR` / `*.XA` file on the disc, with its raw LBA
     * and byte size. Shape: `[{ path, lba, size }, ...]`.
     */
    enumerate_xa_files_json(): string;
    /**
     * Load a full Mode2/2352 disc image. Extracts `PROT.DAT` via the same
     * in-memory ISO walker the viewer uses, parses the TOC, and stashes
     * both slices for later VAB / BGM / XA queries. Returns the PROT entry
     * count for the JS UI.
     */
    load_disc(bytes: Uint8Array): number;
    constructor();
    /**
     * Render `duration_seconds` worth of interleaved stereo i16 PCM at
     * the SPU's 44.1 kHz rate for the BGM pair at (`prot_index`,
     * `vab_offset`, `seq_offset`). Used by the audio page to pre-render
     * a chunk and play it through `AudioBufferSourceNode` (sample-
     * accurate timing) instead of through `ScriptProcessorNode` (callback-
     * paced, drifts on some browsers).
     */
    render_bgm_pcm_i16(prot_index: number, vab_offset: number, seq_offset: number, duration_seconds: number): Int16Array;
    /**
     * Resume the BGM AudioContext. Browsers often construct the
     * `AudioContext` in `suspended` state even when the constructor
     * runs inside a user-gesture handler; the JS side calls this
     * immediately after `start_bgm` to make the audio actually audible.
     */
    resume_bgm(): Promise<any>;
    /**
     * Set the BGM playback gain. Retail SEQ + clean-room SPU output sits
     * around 1% of the i16 range, so the audio page defaults to ~25x to
     * bring playback to a comfortable level. `1.0` matches the native
     * engine-shell cpal path.
     */
    set_bgm_gain(gain: number): void;
    /**
     * Pause / resume the active BGM sequencer. Notes that are already
     * sounding decay through their ADSR envelopes; the sequencer clock
     * freezes.
     */
    set_bgm_paused(paused: boolean): void;
    /**
     * Start BGM playback for the given (`prot_index`, `vab_offset`,
     * `seq_offset`) tuple. Constructs the WebAudio output on the first call
     * (must be invoked from a user-gesture handler), parses VAB + SEQ,
     * uploads the bank to the embedded clean-room SPU, and attaches the
     * sequencer.
     */
    start_bgm(prot_index: number, vab_offset: number, seq_offset: number): void;
    /**
     * Stop the currently-playing BGM. Safe to call even when nothing is
     * playing (no-op).
     */
    stop_bgm(): void;
    /**
     * Decode the frame at `frame_idx` of the currently-open STR movie to a
     * row-major RGBA8 buffer (`width * height * 4` bytes). Empty when no movie
     * is open or the index is out of range. Call `str_video_open` first.
     */
    str_decode_frame(frame_idx: number): Uint8Array;
    /**
     * Drop the cached STR movie frames (frees the bitstream buffers).
     */
    str_video_close(): void;
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
     */
    str_video_open(lba: number, size: number): string;
    /**
     * JSON metadata for every VAG sample inside one VAB bank.
     * Shape: `[{ size_bytes, decoded_samples, duration_ms }, ...]`.
     * `decoded_samples` is the actual PCM length after walking the ADPCM
     * blocks (which stop at the first loop-end / garbage block), so it
     * reflects the audible length, not the raw on-disc body size. Useful
     * for the UI to dim out tiny/zero-length samples that would be
     * inaudible.
     */
    vab_sample_list_json(prot_index: number, vab_offset: number): string;
    /**
     * Sample rate the JS side should use when playing a VAG-decoded buffer.
     */
    vab_sample_rate(): number;
    /**
     * Demux + decode an XA stream. Returns the decoded PCM of the first
     * audio channel (file_no=0, ch_no=0 typically) along with metadata
     * packed as JSON in the first method, then the PCM via this one.
     *
     * Two-step API so the JS side can show metadata (channels, sample rate)
     * before paying the decode cost.
     */
    xa_metadata_json(lba: number, size: number): string;
}

/**
 * The three side-games playable in the browser, plus the disc they read.
 */
export class LegaiaMinigames {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * `[bone_count, frame_count]` of one fighter's animation record.
     * Player actions index the PROT 1203 bank (`char*9 + action`), opponent
     * actions the fighter pack's own bank (typically 8 records, 0 = idle).
     */
    baka_anim_dims(side: number, id: number, action: number): Uint32Array;
    /**
     * One fighter animation record decoded to absolute per-(frame, bone)
     * `[tx, ty, tz, rx, ry, rz]` (PSX 4096-unit angles), padded to
     * `target_part_count` parts - the same pose format the site's mesh
     * animators consume.
     */
    baka_anim_pose_frames(side: number, id: number, action: number, target_part_count: number): Int32Array;
    /**
     * Number of animation records one fighter's bank carries (9 per player
     * character bank; the opponent packs carry their own count, idle first).
     */
    baka_anim_record_count(side: number, id: number): number;
    /**
     * Commit the visitor's attack this exchange: `1`/`2`/`3` are the three
     * rock-paper-scissors throws, `4` the special. Returns `false` when the
     * fighter can't act yet (cooldown, or a choice is already pending).
     */
    baka_choose(attack: number): boolean;
    /**
     * The duel's stage layout - which side of the arena each fighter stands
     * on and which way it faces - as JSON:
     *
     * ```json
     * { "player": { "side": -1, "facing": 1 },
     *   "opponent": { "side": 1, "facing": -1 } }
     * ```
     *
     * `side` is the sign of the fighter's X placement (the player stands on
     * the LEFT, the opponent on the RIGHT); `facing` is the sign of its
     * heading's X (the player faces RIGHT toward the opponent, the opponent
     * faces LEFT toward the player). Each `facing` is the negation of the
     * other fighter's `side`, so both look at their rival - the retail
     * arrangement (`docs/subsystems/minigame-baka-fighter.md`).
     *
     * This is the **single source of truth** for the duel facing: the site's
     * pose step (`site/js/minigame-baka.js`) turns `facing` into a world yaw
     * (`facing * PI/2`) instead of hard-coding it, so the facing is testable
     * off-disc. The player and opponent mesh families share the same intrinsic
     * authored facing, so they need **opposite** world yaws to face each
     * other; an earlier reading assumed opposite intrinsic facings and spun
     * both the same way, leaving both looking left.
     */
    baka_duel_facing_json(): string;
    /**
     * Build the duel's 1 MB PSX VRAM: the PROT 1203 HUD/stage pages, the
     * PROT 1204 party atlases (their bundled CLUT strips are the minigame's
     * own palette - see `docs/formats/character-mesh.md`), and the chosen
     * opponent's atlas last (roster 4's pack shares the `(512, 256)` page +
     * row-497 CLUT with party atlas 6; retail loads them one at a time too).
     */
    baka_duel_vram(opponent: number): Uint8Array;
    /**
     * Per-vertex `[cba, tsb]`, parallel to the positions.
     */
    baka_fighter_cba_tsb(side: number, id: number): Uint32Array;
    /**
     * Per-vertex `[r, g, b, textured_flag]` for the hybrid textured / flat
     * shader path (some fighter prims are untextured flat colour).
     */
    baka_fighter_flat_rgba(side: number, id: number): Uint8Array;
    /**
     * Triangle indices for one duel fighter.
     */
    baka_fighter_indices(side: number, id: number): Uint32Array;
    /**
     * Per-vertex TMD object index (the bone a vertex hangs from).
     */
    baka_fighter_object_ids(side: number, id: number): Uint32Array;
    /**
     * `[part_count]` for one fighter (TMD object count = pose rig width).
     */
    baka_fighter_part_count(side: number, id: number): number;
    /**
     * Per-vertex positions for one duel fighter. `side` 0 = player
     * (`id` = character 0..=2), `side` 1 = opponent (`id` = roster 3..=16).
     */
    baka_fighter_positions(side: number, id: number): Float32Array;
    /**
     * Per-vertex `[u, v]` texel coords, parallel to the positions.
     */
    baka_fighter_uvs(side: number, id: number): Int32Array;
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
     */
    baka_hud_json(): string;
    /**
     * The ladder the cabinet actually serves, as `[{stage, roster}]`.
     *
     * The stage counter starts at **2** and `roster = stage + 3`, so the first
     * lap is roster ids `5..=16` - across which the prize gold is strictly
     * monotonic. Roster `3` and `4` are only reachable after the all-clear
     * wraps the counter, which is why the roster's gold column looks out of
     * order if you read it straight down.
     */
    baka_ladder_json(): string;
    /**
     * The 17 fighter names, in roster order, read out of the roster records
     * (`+0x00`, 32-byte ASCII). Empty when the overlay didn't decode.
     */
    baka_names_json(): string;
    /**
     * One PROT 1203 art page decoded through one of its palettes, RGBA8.
     * Pages are 256x256 4bpp; the palette index comes from the widget record.
     */
    baka_page_rgba(page: number, palette: number): Uint8Array;
    /**
     * Pixel width of PROT 1203 art page `page` (`0` when it didn't decode).
     */
    baka_page_width(page: number): number;
    /**
     * Whether the duel's presentation assets decode off this disc: the HUD
     * art + widget table, the battle-form party pack, and at least the first
     * ladder fighter's pack.
     */
    baka_presentation_ready(): boolean;
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
     */
    baka_roster_json(): string;
    /**
     * Take "NEXT GAME" at the between-match choice: risk the pot on the next
     * rung. Returns the next opponent's roster id, or `-1` when no choice is
     * pending.
     */
    baka_run_fight_on(): number;
    /**
     * Report the current rung's match result into the run: `true` = the
     * player won (prize joins the pot; a choice - or the all-clear - is now
     * pending), `false` = lost (the pot is forfeited). Returns `false` when
     * no run is fighting.
     */
    baka_run_match_over(player_won: boolean): boolean;
    /**
     * Take "PAY OUT" at the between-match choice: bank the pot and end the
     * run. Returns the coins banked (`0` when no choice was pending).
     */
    baka_run_pay_out(): number;
    /**
     * Start a cabinet ladder run at `start_rung` (an index into
     * [`Self::baka_ladder_json`]'s serve order). Bookkeeping only: the caller
     * still starts each rung's duel with [`Self::baka_start`]. Returns the
     * first opponent's roster id, or `-1` when the tables didn't decode /
     * the rung is out of range.
     *
     * The run models the retail between-match choice - after every match win
     * the tally screen offers "NEXT GAME" (risk the accumulated pot on the
     * next rung) or "PAY OUT" (bank it and stop); the two cells live on the
     * PROT 1203 tally sheet next to "GET COIN" and its digit strip. A mid-run
     * loss forfeits the whole pot; clearing the last rung pays it in full.
     */
    baka_run_start(start_rung: number): number;
    /**
     * Live ladder-run state:
     *
     * ```json
     * { "live": true, "phase": "fighting"|"choice"|"paid_out"|"game_over"|"all_clear",
     *   "rung": 0, "len": 14, "roster": 5, "prize": 10,
     *   "pot": 0, "banked": 0, "forfeited": 0 }
     * ```
     */
    baka_run_state_json(): string;
    baka_stage_cba_tsb(index: number): Uint32Array;
    baka_stage_flat_rgba(index: number): Uint8Array;
    baka_stage_indices(index: number): Uint32Array;
    /**
     * Per-vertex positions of stage TMD `index` (PROT 1203 descriptor 1,
     * four meshes: three single-object dressing pieces + a 10-object set).
     */
    baka_stage_positions(index: number): Float32Array;
    /**
     * UVs / CBA-TSB / indices / flat colours of stage TMD `index`, matching
     * [`Self::baka_stage_positions`]'s vertex order.
     */
    baka_stage_uvs(index: number): Int32Array;
    /**
     * Start a best-of-3 duel: the visitor fights as roster fighter 0 (the
     * player-side default) against `opponent`. Returns `false` when the tables
     * didn't decode or the roster id is out of range.
     */
    baka_start(opponent: number, seed: number): boolean;
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
     */
    baka_state_json(): string;
    /**
     * Advance the duel one frame's worth of `frame_step` (the retail SM's
     * per-frame delta; `1` is a normal frame).
     */
    baka_tick(frame_step: number): void;
    /**
     * Whether the dance's art pack + widget table decoded off this disc.
     * When `false` the page falls back to its own glyphs - and says so.
     */
    dance_art_ready(): boolean;
    /**
     * Render `seconds` of the dance BGM to interleaved stereo i16 PCM at
     * [`Self::dance_bgm_rate`], through the clean-room SPU + sequencer -
     * the same path the audio page uses. Empty when the pair didn't decode.
     */
    dance_bgm_pcm_i16(alt: boolean, seconds: number): Int16Array;
    /**
     * Sample rate of [`Self::dance_bgm_pcm_i16`] (the SPU's 44.1 kHz).
     */
    dance_bgm_rate(): number;
    /**
     * Whether the dance BGM pair (VAB + SEQ in one `music_01` entry)
     * resolves: `{"ok":true,"prot":1048,"alt":true}`. The overlay starts one
     * of two songs by mode (`FUN_801cf470` state 6 branches on the mode
     * global); `alt = false` picks extraction 1048, `true` picks 1054.
     */
    dance_bgm_ready_json(): string;
    /**
     * `[bone_count, frame_count]` of dancer `dancer`'s clip slot `clip`
     * (0 = idle, 1 = the dance loop, `2 + k` = move pair `k`). Lenient on
     * the record-size invariant: several choreography records carry frame
     * data past the header count that the retail cursor never plays.
     */
    dance_body_anim_dims(dancer: number, clip: number): Uint32Array;
    /**
     * Per-vertex `[cba, tsb]`, parallel to the positions.
     */
    dance_body_cba_tsb(dancer: number): Uint32Array;
    /**
     * Number of dancer bodies (3 on the qualifier floor: left / centre /
     * right).
     */
    dance_body_count(): number;
    /**
     * Per-vertex `[r, g, b, textured_flag]` for the hybrid textured / flat
     * shader path (the field body mixes textured skin with flat body prims).
     */
    dance_body_flat_rgba(dancer: number): Uint8Array;
    /**
     * Display index of the human dancer (Noa - the centre of the retail
     * qualifier floor).
     */
    dance_body_human_index(): number;
    /**
     * Triangle indices for dancer `dancer`'s body.
     */
    dance_body_indices(dancer: number): Uint32Array;
    /**
     * Dancer `dancer`'s kind descriptor index (0 = Noa, 1 = Mary, 2/3 = the
     * competitor dancers, 4 = the Disco King) - also the face-stamp rig id
     * for kinds 0..=3. `255` when out of range.
     */
    dance_body_kind(dancer: number): number;
    /**
     * Per-vertex TMD object index (the bone a vertex hangs from), parallel to
     * the positions - the animator keys `R . v + T` on this.
     */
    dance_body_object_ids(dancer: number): Uint32Array;
    /**
     * TMD object count (pose rig width) of dancer `dancer`'s body.
     */
    dance_body_part_count(dancer: number): number;
    /**
     * Dancer `dancer`'s clip slot `clip` decoded to absolute per-(frame,
     * bone) `[tx, ty, tz, rx, ry, rz]` (PSX 4096-unit angles), padded to
     * `target_part_count` parts - the same pose stream the site's mesh
     * animator consumes (identical shape to `baka_anim_pose_frames`).
     */
    dance_body_pose_frames(dancer: number, clip: number, target_part_count: number): Int32Array;
    /**
     * Per-vertex positions of dancer `dancer`'s body (object-local; the pose
     * assembles them). Empty when the bodies didn't decode.
     */
    dance_body_positions(dancer: number): Float32Array;
    /**
     * Whether the dance cast (Noa + the dancer NPCs) and the choreography
     * bundle decoded off this disc.
     */
    dance_body_ready(): boolean;
    /**
     * Per-vertex `[u, v]` texel coords, parallel to the positions.
     */
    dance_body_uvs(dancer: number): Int32Array;
    /**
     * The 1 MB PSX VRAM the dancer bodies sample: the dance-hall scene's
     * full TIM upload (the dancer NPC atlases + their row-480/481 CLUTs)
     * merged with the PROT 0874 §2 field-character textures (Noa's atlas,
     * row-478 CLUTs). Empty when the cast didn't decode.
     */
    dance_body_vram(): Uint8Array;
    /**
     * The decoded cast + choreography map, so the page drives retail clips
     * rather than invented ones:
     *
     * ```json
     * { "human": 1,
     *   "dancers": [
     *     { "kind": 2, "model": 62, "x": 5952, "z": 13440,
     *       "clips": [ { "id": 0, "record": 32, "frames": 20, "rate": 8,
     *                    "translucent": false }, ... ] }, ... ],
     *   "moves": { "miss_square": 2, "miss_circle": 3,
     *              "seq_square": [4, 6, 8], "seq_circle": [5, 7, 9],
     *              "beat": [10, 11, 12] } }
     * ```
     *
     * Clip ids: `0` = idle (pre-game), `1` = the dance-groove loop, `2 + k` =
     * judge-triggered move pair `k` (`FUN_801d1af4`'s return, in pair units).
     * The `moves` map gives the clip id per judge event on each difficulty
     * lane. `"[]"`-empty when the cast didn't decode.
     */
    dance_cast_json(): string;
    /**
     * The whole decoded step chart, for the page's scrolling note lane:
     * `{"rows":[[u8; 32], ...]}` (one row per difficulty lane).
     */
    dance_chart_json(): string;
    /**
     * Per-vertex `[cba, tsb]` for the baked hall.
     */
    dance_env_cba_tsb(): Uint32Array;
    /**
     * Per-vertex `[r, g, b, textured_flag]` for the baked hall's hybrid
     * textured / vertex-colour render (same convention as the bodies).
     */
    dance_env_flat_rgba(): Uint8Array;
    /**
     * Triangle indices for the baked hall.
     */
    dance_env_indices(): Uint32Array;
    /**
     * Baked hall vertex positions (`[x, y, z, ...]`, dancer frame). Empty
     * when the scene's placement layers didn't resolve - the page then keeps
     * the neutral ground and says so.
     */
    dance_env_positions(): Float32Array;
    /**
     * Per-vertex `[u, v]` texel coords for the baked hall.
     */
    dance_env_uvs(): Int32Array;
    /**
     * Face window metadata:
     * `[{ "w":80, "h":64, "face":[0,0,32,48], "poses":5 }, ...]` - `w`/`h`
     * are the buffer dimensions [`Self::dance_face_rgba`] returns, `face`
     * the sub-rect that is the visible face (the rest of the window is
     * neighbouring atlas cells).
     */
    dance_face_meta_json(): string;
    /**
     * One dancer's live face window as RGBA8: the strip's top window with
     * pose `pose` stamped in by the two traced `MoveImage` blits
     * (`FUN_801d03c4`). `dancer` is the rig index `0..=3`: `0` = **Noa**
     * (her field atlas, PROT 0874 §2), `1..=3` = the pack strips. Pair with
     * [`Self::dance_face_meta_json`] for dimensions. Empty when the strip
     * didn't decode.
     */
    dance_face_rgba(dancer: number, pose: number): Uint8Array;
    /**
     * The 256x256 HUD page (VRAM `(512, 0)`) decoded through palette
     * `palette` of its own row-500 CLUT strip, as RGBA8. Palette selection
     * is load-bearing: the widget table names a palette per element, and
     * the beat-track flash / note tint are pure CLUT swaps over the same
     * texels (`0x7D08` idle / `0x7D0D` flash / `0x7D0E` notes).
     */
    dance_hud_page_rgba(palette: number): Uint8Array;
    /**
     * The dance's disco jukebox as JSON: one row per selectable track that
     * decodes on this disc -
     * `{"tracks":[{"bgm":2058,"label":"M114 - ...","role":"Dance stage (overlay track A)"},...]}`.
     * The first two rows are the tracks the dance overlay actually loads
     * (mode-selected, extraction 1048/1054); the rest are the Sol-disco
     * floor family in the same bank. Rows whose `[VAB][SEQ]` pair is absent
     * (e.g. a track dropped from the NA disc) are omitted.
     */
    dance_jukebox_json(): string;
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
     */
    dance_layout_json(): string;
    /**
     * Press a dance button. `1` = Square, `2` = Circle (the judged directions),
     * `3` = **Triangle**, the three-per-song "groovy move" wildcard.
     *
     * Returns the event name: `"miss"` / `"hit"` / `"sequence"` for a direction,
     * `"groovy"` / `"groovy_off"` for a triangle spent on / off the 4-beat combo
     * slot, `"no_charge"` when the stock is empty, and `"ignored"` while the
     * dancer is inside a groovy move (input is disrupted for its whole spin).
     * `"none"` with no live run.
     */
    dance_press(dir: number): string;
    /**
     * The retail cue ids (`FUN_801d1af4` sites): miss, the three combo-tier
     * stings, the run-start and intro cues.
     */
    dance_sfx_cue_ids(): string;
    /**
     * The dance's own cue bank (descriptors PROT 1228, samples PROT 1231):
     * `[{ "id":528, "program":0, "tone":1, "note":66, "rate":44100 }, ...]`.
     * Empty when either entry didn't decode - PROT 1231 sits in the PROT
     * TOC's zeroed tail, so an image whose TOC truncates early loses it.
     */
    dance_sfx_json(): string;
    /**
     * Decode one dance cue to mono PCM (`i16`). Empty when absent.
     */
    dance_sfx_pcm(cue: number): Int16Array;
    /**
     * Playback rate for [`Self::dance_sfx_pcm`] (`0` when absent).
     */
    dance_sfx_rate(cue: number): number;
    /**
     * Start a dance run on the disc's baked step chart, scoring tables and
     * qualifier cast (all rodata of PROT 0980). `long_song` picks the long
     * song-length limit. Returns `false` when the overlay didn't decode.
     */
    dance_start(long_song: boolean): boolean;
    /**
     * Live dance state.
     *
     * ```json
     * { "live": true, "score": 0, "gauge": 0, "lane": 0, "beat": 3,
     *   "phase": 40, "period": 281, "window": 210, "accuracy": 3200, "dead_zone": false,
     *   "combo_slot": true, "judged": 2, "displayed": 3,
     *   "triangles": 3, "lock": 0, "feedback": null,
     *   "rivals": [ {"score": 12, "gauge": 500, "lane": 0, "kind": 2, "triangles": 3}, .. ],
     *   "song_timer": 900, "song_len": 16860, "over": false, "passed": false,
     *   "winning": true }
     * ```
     *
     * **`judged` is the step to press.** Retail splits the chart lookup
     * (`FUN_801d1820`) into two halves: the hit judge (`FUN_801d1960`) matches
     * a press against the raw chart cell (`judged`), while the display /
     * auto-feed half substitutes the triangle symbol `3` on every 4th beat
     * (`displayed`). Both are surfaced; only `judged` scores a direction. `0` =
     * the beat carries no step, `null` = the dead zone between beats.
     *
     * `triangles` is the groovy-move stock (3 per song); `lock` is the frames of
     * groovy-move spin still disrupting input; `feedback` is `true`/`false`
     * while the post-spend caption window runs (whether it landed on the combo
     * slot), `null` otherwise. `rivals` are the two CPU dancers, scoring live
     * off the same chart.
     */
    dance_state_json(): string;
    /**
     * One layer of a good-step **hit sting**. Retail keys these directly
     * (`FUN_801d3d78`, bypassing the cue ring): a step picks `r = rand() % 3`
     * and keys VAB program 1 tones `2r` (layer 0) and `2r + 1` (layer 1)
     * together at note `0x3C + r`. Mono i16 PCM; empty when absent.
     */
    dance_sting_pcm(r: number, layer: number): Int16Array;
    /**
     * Playback rate for [`Self::dance_sting_pcm`] (`0` when absent).
     */
    dance_sting_rate(r: number, layer: number): number;
    /**
     * Advance the beat clock by `frames` frames (the retail clock steps
     * `frame_delta * 10` phase units per frame). This also runs the **CPU
     * dancers**: retail feeds them the chart every frame through the same judge
     * and award routine the human's presses take, so their scores climb here.
     */
    dance_tick(frames: number): void;
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
     */
    dance_widgets_json(): string;
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
     */
    load_disc(bytes: Uint8Array): string;
    /**
     * Render `seconds` of `game`'s BGM to interleaved-stereo i16 PCM at
     * [`Self::minigame_bgm_rate`]. Empty when the entry didn't decode.
     */
    minigame_bgm_pcm_i16(game: string, seconds: number): Int16Array;
    /**
     * Sample rate of [`Self::minigame_bgm_pcm_i16`] (the SPU's 44.1 kHz).
     */
    minigame_bgm_rate(): number;
    /**
     * Whether `game`'s BGM (`"slot"` / `"baka"`) resolves on this disc:
     * `{"ok":true,"prot":1043,"why":"..."}`. The dance's two-song check is
     * [`Self::dance_bgm_ready_json`].
     */
    minigame_bgm_ready_json(game: string): string;
    /**
     * Render a global-pool BGM id (`2000 + sound-test slot`) to a **seamless
     * loop** render: PCM plus the loop region the browser drives
     * `loopStart`/`loopEnd` from. This is the jukebox / minigame playback
     * path, superseding the fixed-window [`Self::minigame_bgm_pcm_i16`] hard
     * loop. Bounds the render at `max_seconds`. Returns an empty render when
     * the id isn't a bank slot or its `[VAB][SEQ]` pair doesn't decode.
     */
    music01_bgm_render(bgm_id: number, max_seconds: number): Music01Render;
    constructor();
    /**
     * One of the three retail 16x16 **save-file portrait** TIMs as a 1024-byte
     * RGBA8 buffer: `0` = Vahn, `1` = Noa, `2` = Gala.
     *
     * These are the load-screen slot-grid portraits, pinned in the unindexed
     * pre-`init_data` gap of `PROT.DAT` (offset `0x1AC90`, 192-byte stride;
     * `legaia_asset::title_pak::extract_overlay_load_portrait_tim`). Retail
     * bakes the lead's copy into every SC save block as the memory-card icon,
     * so these are exactly the faces a retail save carries. The site's save
     * bar decodes them once from the visitor's own disc and caches the pixels
     * locally - no art ships with the page. Empty when no disc is loaded or
     * the TIM doesn't parse.
     */
    save_portrait_rgba(char_id: number): Uint8Array;
    /**
     * Whether the slot machine's art pack decoded off this disc. When `false`
     * the page must fall back to symbol *ids*, not to invented artwork.
     */
    slot_art_ready(): boolean;
    /**
     * The **bonus game**: the two jackpot triggers, and - when a round is live -
     * the numbers on the reels and the **claimed-column tally** the machine
     * prints across its marquee.
     *
     * A matching line of the **blue "kick"** symbol (id 8) earns 1 bonus round;
     * the **red "punch"** symbol (id 9) earns 3 - the counts and symbol ids are
     * pinned in the disassembly (`FUN_801d13e8`) and the colours in the PROT
     * 1200 reel art. A bonus round swaps the reels onto the machine's *second*
     * strip - the numerals `1..=10`, their own artwork on art page 1 - and pays
     * the **product of the three numbers you stop on** (`1..=1000`).
     *
     * ```json
     * { "kick_symbol": 8, "kick_rounds": 1, "punch_symbol": 9, "punch_rounds": 3,
     *   "min": 1, "max": 1000, "active": true, "rounds_left": 2,
     *   "numbers": [9, 5, 3], "tally": [9, 5, 0], "claimed": [true, true, false],
     *   "complete": false, "product": 0 }
     * ```
     *
     * * `numbers` - the number **live on each reel's payline** right now, so the
     *   page can draw the wheels while they spin.
     * * `tally` - the machine's own claimed-column latch (`DAT_801d3d20`): `0`
     *   for a column whose reel is still spinning, its landed number once that
     *   stop is taken. This is the `0 x 0 x 0` -> `9 x 5 x 0` strip.
     * * `product` - the tally's product, i.e. the coins the round pays; `0`
     *   until all three columns are claimed (`complete`).
     *
     * The tally and the payout are **the same state**, not two copies: the
     * evaluator multiplies the very rows the tally latched. A page that renders
     * `tally` cannot show a line that disagrees with what the spin paid.
     */
    slot_bonus_json(): string;
    /**
     * One **bonus reel numeral** (`1..=10`) as a 64x64 RGBA8 buffer - the big
     * coloured digit the reels carry during a bonus round.
     *
     * These are the retail faces, not a scaled coin font: ten 64x64 cells of
     * their own artwork on art-pack page 1, each drawn through its own palette
     * column (`CLUT 0x7AC0 + n - 1`), which is why every numeral is a different
     * colour. `FUN_801d0fa8` reaches them by the same UV arithmetic it uses for
     * the symbols - a bonus strip value simply clears `0x10`, which bumps the
     * texpage to `0x0D` and the CLUT base to `0x7AC0`.
     *
     * Empty when the art pack didn't decode - in which case the page must say
     * so, not draw digits of its own.
     */
    slot_bonus_number_rgba(number: number): Uint8Array;
    /**
     * Tally the latched payout into the balance and return to idle. Returns
     * the credited coins. [`Self::slot_tick`] already does this on the frame a
     * spin resolves; this stays for hosts that drive the tally themselves.
     */
    slot_collect(): number;
    /**
     * The coin readout's font strip - the `"COIN"` label (`x = 0..64`) followed
     * by digits `0..=9` at `x = 64 + d * 16` - as a 224x16 RGBA8 buffer
     * (`FUN_801d2914`, CLUT `0x7A8D`).
     */
    slot_digits_rgba(): Uint8Array;
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
     */
    slot_hud_json(): string;
    /**
     * One of the 3 HUD widgets the retail rasteriser `FUN_801d2cc0` draws from
     * the descriptor table `DAT_801d347c`, decoded through *its own* texpage +
     * CLUT: `0` = the cabinet panel, `1` = the "COIN" label, `2` = the cash-out
     * cursor. RGBA8; pair with [`Self::slot_hud_json`] for the dimensions.
     */
    slot_hud_rgba(index: number): Uint8Array;
    /**
     * The **marquee message bank's roles** - which of the 21 dot-matrix bitmaps
     * in [`Self::slot_scene_json`] is which glyph, and the dot columns the
     * machine blits them at.
     *
     * The tally strip and the payout caption are not chrome the page invents:
     * they are `FUN_801cfff0` composing the *same* 78x13 dot matrix that
     * scrolls the attract legend in the normal game. This hands over the ids and
     * columns it uses, so the page draws the retail glyphs at the retail
     * positions rather than a font of its own.
     *
     * ```json
     * { "number_base": 6, "number_max": 10, "times": 17, "coins": 20,
     *   "pip_on": 18, "pip_off": 19,
     *   "tally_cols": [0, 32, 64], "times_cols": [16, 48], "pip_cols": [0, 32, 64],
     *   "payout_digit_cols": [0, 13, 26, 39], "payout_coins_col": 52,
     *   "payout_slide_rows": 13 }
     * ```
     *
     * `number_base + n` is the bitmap for the numeral `n`, `0..=10` - eleven
     * records, because a bonus reel can land on **10** and retail gives it a
     * glyph of its own rather than two digit cells.
     */
    slot_marquee_json(): string;
    /**
     * A whole art page decoded through one of its 16 palettes, as RGBA8. Every
     * on-screen rect the machine draws is traced to its emitter, so a caller
     * pairs this with the cells in [`Self::slot_scene_json`] rather than
     * cropping by eye. Pages 0..=3 are 256x256; page 4 is 512x256.
     */
    slot_page_rgba(page: number, palette: number): Uint8Array;
    /**
     * Pixel width of art page `page` (`0` when the pack didn't decode).
     */
    slot_page_width(page: number): number;
    /**
     * The machine's **paytable / coin info panel** - HUD record 0, the 127x239
     * board `FUN_801cfff0` draws at screen `(560, 128)` ("x30 back", "x9 back",
     * "Bonus games", with the coin readout under it). RGBA8.
     *
     * It has its own entry point because its page is sampled as **8bpp** (the
     * texpage attribute's colour bit), not the 4bpp its TIM header declares -
     * decoding it as the header claims yields noise.
     */
    slot_panel_rgba(): Uint8Array;
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
     */
    slot_press(): string;
    /**
     * The live reel positions (`DAT_801d3cc0`) - fixed-point angles whose high
     * byte is the strip row and whose low byte is the sub-symbol fraction. The
     * renderer needs the fraction: the reel is a 3D cylinder and the fraction is
     * what rotates it between symbols.
     */
    slot_reel_pos(): Int32Array;
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
     */
    slot_scene_json(): string;
    /**
     * Whether the machine's 3D scene graph decoded off this disc.
     */
    slot_scene_ready(): boolean;
    /**
     * The retail cue ids, so the page never has to hard-code a number:
     * `{"reel_stop":522,"payout_tick":521,"reach":512,"reach1":513,"reach2":514}`.
     */
    slot_sfx_cue_ids(): string;
    /**
     * The cue ids this disc's slot bank actually defines, with the VAB voice
     * each one keys:
     *
     * ```json
     * [ { "id": 522, "program": 1, "tone": 6, "note": 66, "rate": 46616 }, ... ]
     * ```
     *
     * `id` is decimal (`522` = `0x20A`, the reel-stop click).
     */
    slot_sfx_json(): string;
    /**
     * Decode one cue to mono PCM (`i16`). Empty when the cue isn't in the bank.
     */
    slot_sfx_pcm(cue: number): Int16Array;
    /**
     * The rate [`Self::slot_sfx_pcm`]'s samples must be played back at - the
     * cue's note against the VAG's own centre note *is* the pitch, so this
     * carries it. `0` when the cue isn't in the bank.
     */
    slot_sfx_rate(cue: number): number;
    /**
     * Charge the bet and start a spin. `false` when the machine isn't idle or
     * the balance is under the 3-coin gate.
     */
    slot_spin(): boolean;
    /**
     * The reel-spin motor **loop**, mono i16. Not a ring cue: the reel SM
     * keys class-2 VAB program 1 / tone 0 at note `0x3C` directly
     * (`FUN_801CF0D8` -> `func_0x80065034(0x13, 2, 1, 0, 0x3C, ...)`) and
     * releases the voice on all-reels-stop - the page loops this buffer for
     * as long as the reels turn. Empty when the VAB didn't decode.
     */
    slot_spin_pcm(): Int16Array;
    /**
     * Playback rate for [`Self::slot_spin_pcm`] (`0` when absent).
     */
    slot_spin_rate(): number;
    /**
     * Start a slot session on the disc's payout table with `balance` coins in
     * the machine. Returns `false` when the payout table didn't decode.
     */
    slot_start(seed: number, balance: number): boolean;
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
     */
    slot_state_json(): string;
    /**
     * Stop the leftmost still-spinning reel. `false` when stopping isn't
     * allowed yet (the reels are still spinning up).
     */
    slot_stop(): boolean;
    /**
     * The 20-symbol display strip of `reel`, as the renderer reads it.
     */
    slot_strip(reel: number): Uint8Array;
    /**
     * One reel symbol (`0..=9`) as a 64x64 RGBA8 buffer, at the exact cell and
     * **per-symbol CLUT** the retail reel renderer `FUN_801d0fa8` samples
     * (`U = (sym & 3) * 0x40`, `V = (sym & 0xC) * 0x10`, CLUT `0x7A80 + sym`).
     *
     * The palette is load-bearing: symbols 0/1/2 are one piece of artwork
     * recoloured three ways, and so are 4/5. Empty when the art didn't decode.
     */
    slot_symbol_rgba(sym: number): Uint8Array;
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
     */
    slot_tick(): number;
}

/**
 * Bridge object the play page instantiates once. Holds a `World` +
 * `MenuRuntime` for the disc-free path, and - once `load_disc` has run - a
 * `SceneHost` plus the render state for the scene it is running.
 */
export class LegaiaRuntime {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Attempt to start the WebAudio backend. Must be called from a user-gesture
     * handler (browser autoplay policy). `true` on success.
     */
    audio_init(): boolean;
    /**
     * `[width, height]` of the title atlas; `[0, 0]` when none.
     */
    boot_title_atlas_dims(): Uint32Array;
    /**
     * The title art atlas (RGBA8) the sprite bands sample. Empty when none.
     */
    boot_title_atlas_rgba(): Uint8Array;
    /**
     * Abort the title flow (page navigated away / cancelled).
     */
    boot_title_close(): void;
    /**
     * Draw lists for the current title state, in surface pixels:
     * `{ "active": true, "sprites": [...title-atlas quads...],
     *    "glyphs": [...menu-glyph-atlas quads...],
     *    "texts": [...font quads...] }`. Rendered over black by the page.
     *
     * The three layers are mutually exclusive by design, and the split
     * mirrors the native window exactly. With the disc's title art present
     * the TIM's own NEW GAME / CONTINUE bands carry the menu (`sprites`).
     * Without it the rows fall back to the shared
     * [`ui::title_menu_draws_for`] builder sampling the menu-glyph atlas
     * (`glyphs`) - the same fallback the native window's
     * `title_menu_glyph_sprite_draws` serves - and only if that atlas is
     * missing too does the font stand-in (`texts`) draw.
     */
    boot_title_draws_json(surface_w: number, surface_h: number): string;
    /**
     * `[width, height]` of the menu-glyph atlas; `[0, 0]` when none.
     */
    boot_title_glyph_atlas_dims(): Uint32Array;
    /**
     * The menu-glyph atlas (RGBA8 stencil) the no-title-art menu rows
     * sample. Empty when it did not resolve.
     */
    boot_title_glyph_atlas_rgba(): Uint8Array;
    /**
     * `true` once the disc title art resolved (else the card renders text-only).
     */
    boot_title_has_atlas(): boolean;
    boot_title_is_active(): boolean;
    /**
     * Start the boot title screen. No-op with no disc loaded. Continue is left
     * disabled (the browser boot does not preload an engine save); the fade-in
     * is skipped so the card shows immediately.
     */
    boot_title_start(): void;
    /**
     * Advance the title one frame with an edge-triggered PSX pad word. Returns
     * `""` while the title runs, or the chosen outcome once the player
     * confirms: `"new_game"`, `"continue"`, or `"options"`. The caller acts on
     * the outcome (seed + enter the opening scene for New Game) and the title
     * clears itself.
     */
    boot_title_step(edge: number): string;
    /**
     * `true` when the card in `slot` holds in-game writes the page has not
     * exported yet.
     */
    card_slot_dirty(slot: number): boolean;
    /**
     * The whole rack as JSON - what the page's card picker renders:
     * ```text
     * [ { "slot": 0, "inserted": true, "label": "my card", "format": "mcr",
     *     "dirty": false,
     *     "blocks": [ { "block": 1, "present": true, "name": "Vahn",
     *                   "level": 12, "location": "Rim Elm", "money": 900 }, ... ] },
     *   { "slot": 1, "inserted": false, ... } ]
     * ```
     */
    card_slots_json(): string;
    /**
     * `[width, height]` of the caption image; `[0, 0]` when none.
     */
    cutscene_caption_dims(): Uint32Array;
    /**
     * The "It was the Seru." caption image (a baked TIM the prologue blits,
     * faded, between the two narration crawls), RGBA8. Empty when the
     * current scene carries none.
     */
    cutscene_caption_rgba(): Uint8Array;
    /**
     * `true` if a disc has been loaded.
     */
    disc_loaded(): boolean;
    /**
     * Remove the card from rack slot `slot`. Unexported writes are lost -
     * the page warns before calling this.
     */
    eject_card(slot: number): void;
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
     */
    enter_field(name: string): string;
    /**
     * The card in rack slot `slot`, as container bytes ready to download.
     *
     * Byte-identical to what was inserted apart from the SC blocks the
     * player saved into, so the player's emulator loads it straight back.
     * Empty when no card is in that slot. Clears the slot's dirty flag.
     */
    export_card(slot: number): Uint8Array;
    /**
     * Export the current engine session as LGSF bytes
     * (`World::save_full().write()`). The page offers this as a `.lgsf`
     * download and persists it (base64) in localStorage.
     */
    export_save(): Uint8Array;
    field_ground_cba_tsb(): Uint16Array;
    field_ground_indices(): Uint32Array;
    field_ground_positions(): Float32Array;
    field_ground_quad_count(): number;
    field_ground_uvs(): Uint8Array;
    /**
     * Field pause-menu model: the party (battle order) + inventory + gold the
     * page's menu overlay renders when the player presses Start. Shape:
     * ```text
     * { "gold": 240,
     *   "party": [{ "name": "Vahn", "level": 1, "hp": 60, "hp_max": 60,
     *               "mp": 8, "mp_max": 8 }, ...],
     *   "items": [{ "id": 32, "name": "Healing Leaf", "count": 3 }, ...] }
     * ```
     * `null` before a disc scene is entered. Item labels come from the SCUS
     * item-name table ([`Self::load_disc`]); a PROT.DAT-only load falls back to
     * the raw id. The retail pause menu is a native-only draw path (glyph atlas
     * + window-descriptor table); this feeds the browser's HTML overlay
     * equivalent so Start still surfaces the party / items on the play page.
     */
    field_menu_model_json(): string;
    /**
     * Select + build environment-pack slot `slot`; subsequent `field_mesh_*`
     * reads return that mesh.
     */
    field_mesh(slot: number): number;
    field_mesh_cba_tsb(): Uint16Array;
    field_mesh_flat_rgba(): Uint8Array;
    field_mesh_indices(): Uint32Array;
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
     */
    field_mesh_posed(slot: number, anim_id: number): number;
    /**
     * Positions of environment-pack slot `slot` **posed at clip frame
     * `frame`** of scene ANM record `anim_id - 1` - the per-frame re-pose the
     * draw walker (`FUN_8001B964`) does off a placed prop's live cursor.
     * Same vertex order as [`Self::field_mesh_posed`]'s frame-0 build (the two
     * differ only in the per-object transform), so the page can upload the
     * mesh once and rewrite just its positions each frame. Empty when the pose
     * can't resolve (no bundle / bone-count mismatch) - the caller then leaves
     * the prop at its rest pose.
     */
    field_mesh_posed_frame_positions(slot: number, anim_id: number, frame: number): Float32Array;
    field_mesh_positions(): Float32Array;
    field_mesh_uvs(): Uint8Array;
    /**
     * The off-map hide-box coordinate (`FIELD_OFFMAP_HIDE_XZ`). Retail parks
     * despawned / story-hidden actors at this far-corner sentinel tile
     * precisely so they never render; the page must skip drawing any NPC
     * whose **live** position is this tile on both axes, exactly as the
     * native play-window's draw pass does.
     */
    field_offmap_hide_xz(): number;
    /**
     * Per-placement object-bind animation id (parallel to
     * [`Self::field_placement_slots`]). `0` = unposed; nonzero = draw the
     * slot's mesh through [`Self::field_mesh_posed`] with this id, or the
     * prop's multi-object parts heap on the origin.
     */
    field_placement_anim_ids(): Uint32Array;
    /**
     * Live clip frame of each placement (parallel to
     * [`Self::field_placement_slots`]): `-1` for a static prop (no anim, or
     * no live prop-bank entry), else the prop's current cursor frame
     * (`PropAnimBank::frame`, the `actor+0x68 >> 4` the draw walker poses
     * from). The world advances every prop's cursor each field tick
     * (`tick_prop_interactions` -> `PropAnimBank::tick_anims`, retail's
     * `FUN_800204F8`), so an animated prop - the windmill sails, a swinging
     * door mid-swing - reports a changing frame, and the page re-poses it.
     */
    field_placement_frames(): Int32Array;
    field_placement_positions(): Float32Array;
    field_placement_rot_y(): Uint16Array;
    /**
     * Per-placement env-pack slot (parallel to
     * [`Self::field_placement_positions`] / [`Self::field_placement_rot_y`]).
     */
    field_placement_slots(): Uint32Array;
    /**
     * `{"pack_count", "placements", "terrain", "ground_quads"}` for the status
     * line; `null` before a scene is entered.
     */
    field_status_json(): string;
    field_terrain_positions(): Float32Array;
    field_terrain_rot_y(): Uint16Array;
    field_terrain_slots(): Uint32Array;
    /**
     * Field VRAM (1 MB) - the image every mesh below samples. The engine's own
     * scene VRAM, not a viewer-side rebuild.
     */
    field_vram_bytes(): Uint8Array;
    /**
     * Frame counter.
     */
    frame(): bigint;
    /**
     * Import a **retail emulator save** (block `block` of a card container)
     * into the live engine session: party records, story flags, inventory,
     * and gold, via [`SaveFile::from_retail_sc_block`]. Returns the block's
     * summary JSON (including the save's own `scene` label, so the page can
     * drop the player into the scene the save was made in).
     */
    import_card_save(bytes: Uint8Array, block: number): string;
    /**
     * Import an LGSF save into the live engine session. Validates the
     * magic/version envelope before touching the world; a bad file leaves
     * the session unchanged and throws a readable message. Returns the
     * same summary JSON as [`save_summary_json`].
     */
    import_save(bytes: Uint8Array): string;
    /**
     * Insert a memory-card image into rack slot `slot` (0 or 1 - the
     * console's two ports).
     *
     * `bytes` is the container exactly as the player exported it from their
     * emulator (`.mcr` / `.mcd` / `.gme` / `.mcs`); it is validated here and
     * then kept verbatim, so [`Self::export_card`] can hand it back in the
     * same shape. Returns the slot's JSON (same shape as one entry of
     * [`Self::card_slots_json`]); throws on an unrecognised container.
     */
    insert_card(slot: number, bytes: Uint8Array, label: string): string;
    /**
     * Load a disc image from raw in-memory bytes.
     *
     * `raw_bytes` may be either a Mode2/2352 full disc image (`.bin`) - PROT.DAT
     * and CDNAME.TXT are extracted via an ISO9660 walk - or the raw contents of
     * `PROT.DAT`. `cdname_text` overrides any CDNAME.TXT found on the disc; pass
     * an empty string to use the disc's own.
     *
     * Returns the number of PROT entries parsed. Nothing leaves the browser.
     */
    load_disc(raw_bytes: Uint8Array, cdname_text: string): number;
    menu_is_open(): boolean;
    menu_label(): string;
    /**
     * Tick the scaffold menu with a packed button mask
     * (`cross | circle<<1 | triangle<<2 | square<<3 | up<<4 | down<<5 |
     * left<<6 | right<<7`).
     */
    menu_tick(button_mask: number): any;
    /**
     * Draw lists for the overlay, in surface pixels:
     * `{ "open": bool, "sprites": [...menu-chrome quads...],
     *    "texts": [...dialog-font quads...] }` - the same two-layer shape
     * the pause menu / shop / dialog use, blitted by the page's chrome and
     * font atlas blitters.
     *
     * Both layers come from the shared `engine-ui` builders at the
     * retail-traced stage geometry, then through the common stage transform
     * so text and chrome stay locked together.
     */
    name_entry_draws_json(surface_w: number, surface_h: number): string;
    /**
     * Step the overlay one frame from an edge-triggered PSX pad word (same
     * bit layout as [`Self::set_pad`]). Cross confirms the cell under the
     * cursor (or the Yes/No row); Triangle is the backspace shortcut while
     * editing and cancels the confirm prompt.
     *
     * Returns `true` on the frame the name commits - at which point the
     * entry closes, the name is in the party record, and the op-`0x49` gate
     * releases the suspended opening script on its next step.
     *
     * The world frame counter is advanced here (and only here) while the
     * overlay is up, because the field tick is frozen under it and the
     * caret blink is derived from that counter.
     */
    name_entry_input(edge: number): boolean;
    /**
     * `true` while the name-entry overlay is up. The page freezes the field
     * and routes every pad edge into [`Self::name_entry_input`] while this
     * holds - the overlay is modal, exactly as it is natively.
     */
    name_entry_is_active(): boolean;
    /**
     * Live overlay state for the page's status line (and headless checks):
     * ```text
     * { "open": true, "name": "Vahn", "default": "Vahn", "cursor": 116,
     *   "control": 2,        // 0 = BS, 1 = restore default, 2 = Select
     *   "glyph": "A"|null,   // glyph under a grid cursor
     *   "confirming": false, "confirm_yes": false }
     * ```
     * `{"open":false}` when no entry is up. A read-only projection of the
     * engine SM - the page never writes name state, it only reports it.
     */
    name_entry_state_json(): string;
    constructor();
    /**
     * Open the disc-free scaffold menu (the headless [`MenuRuntime`] - the
     * retail pause menu's screens are a native-only draw path today).
     */
    open_menu(): void;
    /**
     * The committed display name for a party slot - the name-entry result
     * once confirmed, else the disc's new-game template default. The page
     * shows it in the HUD so the naming is visibly *in the save*, not just
     * on a screen that came and went.
     */
    party_display_name(slot: number): string;
    /**
     * Camera parameters for the cutscene shot, decoded from the timeline's
     * executed op-`0x45` Camera Configure params - the browser mirror of
     * the native window's `cutscene_view` (see that fn for the retail
     * provenance: focus X/Z stored negated in params 6/8; pitch/yaw in
     * params 0/1, PSX 4096 = turn; H in param 9; the eye-space translation
     * trio in params 3/4/5, divided by retail's folded-in 6x world scale).
     * Shape:
     * ```text
     * { "active": bool,  // a cutscene timeline is running
     *   "focus": [x, y, z], "pitch": rad, "yaw": rad,
     *   "h": f, "tr": [x, y, z] }
     * ```
     * `null` before a scene is entered.
     * REF: FUN_801DE084, FUN_800172C0
     */
    play_cutscene_camera_json(): string;
    /**
     * Per-frame cutscene presentation state:
     * ```text
     * { "locked": bool,          // freeze the pad this frame (feed 0)
     *   "chain": bool,           // opening chain playing (skip available)
     *   "narration": bool, "card": bool,
     *   "caption_alpha": 0.0,    // "It was the Seru." fade (0 = hidden)
     *   "grade": { "gold": [r,g,b], "strength": s } | null,
     *   "cue": { "far": [r,g,b], "near_z": f, "far_z": f, "max_ir0": f } | null }
     * ```
     * `grade` / `cue` mirror `World::scene_color_grade` /
     * `World::scene_depth_cue` - the prologue sepia multiply + gold DPCS
     * depth-cue ramp the native window stages into its renderer each frame.
     */
    play_cutscene_state_json(): string;
    /**
     * The narration crawl + title card as font-atlas text quads over a
     * `surface_w` x `surface_h` canvas - the same
     * `{ "open", "texts" }` quad shape as the menu / dialog draws (blit off
     * the font atlas; there are no chrome sprites). Line Ys are the
     * roller's PSX 240-line window scaled to the surface; each line is
     * centred, white - the native window's narration draw.
     * REF: FUN_80037174
     */
    play_cutscene_text_draws_json(surface_w: number, surface_h: number): string;
    /**
     * Draw lists for the retail dialog reading box over a `surface_w` x
     * `surface_h` canvas. Same shape as
     * [`Self::play_menu_draws_json`]: `{ "open", "sprites", "texts" }` -
     * `sprites` sample the chrome atlas, `texts` the font atlas (upload both
     * via the `play_menu_*` atlas accessors; this call builds the shared
     * assets on first use). `open` is `false` when no box is up this frame.
     *
     * Unlike the pause menu the field keeps running underneath - retail
     * draws the reading box over the live scene.
     */
    play_dialog_draws_json(surface_w: number, surface_h: number): string;
    /**
     * `[width, height]` of the chrome atlas; `[0, 0]` when none.
     */
    play_menu_chrome_dims(): Uint32Array;
    /**
     * The assembled menu-chrome atlas (RGBA8) the sprite draws sample. Empty
     * when no chrome resolved.
     */
    play_menu_chrome_rgba(): Uint8Array;
    /**
     * Close the menu (and any open sub-screen).
     */
    play_menu_close(): void;
    /**
     * Build the two draw lists for the current menu state, in surface pixels.
     * Shape:
     * ```text
     * { "open": true,
     *   "sprites": [ { "dst":[x,y,w,h], "src":[x,y,w,h], "color":[r,g,b,a] } ],
     *   "texts":   [ ... ] }
     * ```
     * `sprites` sample the chrome atlas, `texts` the font atlas. `open` is
     * `false` (and the lists empty) when no menu is up.
     */
    play_menu_draws_json(surface_w: number, surface_h: number): string;
    /**
     * `[width, height]` of the font atlas.
     */
    play_menu_font_dims(): Uint32Array;
    /**
     * The whitewashed font atlas (RGBA8) the text draws sample. Stable across
     * the session; the page uploads it once.
     */
    play_menu_font_rgba(): Uint8Array;
    /**
     * `true` once the gold chrome atlas resolved from the disc; `false` means
     * the menu renders glyphs only (PROT.DAT-only load).
     */
    play_menu_has_chrome(): boolean;
    /**
     * Drive the menu one frame from an edge-triggered PSX pad word (same bit
     * layout as [`Self::set_pad`]). Navigation:
     * - top-level: Up/Down move the cursor, Cross opens the row, Circle closes.
     * - a sub-screen: routes the edges to its session; Circle (or the session
     *   finishing) drops back to the top-level list.
     */
    play_menu_input(edge: number): void;
    play_menu_is_open(): boolean;
    /**
     * Open the retail pause menu. No-op with no disc loaded. The field is
     * frozen by the page while [`Self::play_menu_is_open`] is true.
     */
    play_menu_open(): void;
    /**
     * Take the CDNAME scene label an in-canvas card **Load** landed in, if
     * one is waiting; `""` otherwise. The page polls this after driving the
     * menu and, when it is a scene it can walk, enters it - retail resumes a
     * save in the scene it was written in. Consuming clears it.
     */
    play_menu_take_load_scene(): string;
    /**
     * The scene's NPC / actor catalog. Shape:
     * `{"anm_prot": 4, "npcs": [{"i", "slot", "model", "anim", "nobj",
     * "kind", "target_map", "dialog", "conditional", "x", "z"}, ...]}`.
     * `null` before a scene is entered.
     */
    play_npc_catalog_json(): string;
    /**
     * Live clip-playback state of every catalogued NPC, flattened
     * `[frame, generation, ...]` pairs in catalog order; `[-1, -1]` for an
     * entry with no live clip player. `frame` is the clip frame this render
     * should show ([`legaia_engine_core::field_anim::FieldClipPlayer::frame`],
     * advanced once per drained sim tick - the native window's sim-tick anim
     * contract); `generation` bumps when an ANIMATE cue re-targets the clip,
     * telling the page to re-read the pose behind the index.
     */
    play_npc_clip_states(): Int32Array;
    /**
     * Current pose of catalog entry `i`'s **live** clip: 6 `i32` per bone
     * (`[tx, ty, tz, rx, ry, rz]`, absolute), read WITHOUT advancing the
     * playhead ([`FieldClipPlayer::current_pose`] - the playhead moves only
     * in [`LegaiaRuntime::tick_frame`]). Unlike
     * [`Self::play_npc_pose_frames`] this follows ANIMATE-cue re-targets, so
     * a scripted actor's performed clip is what comes back. Empty when the
     * entry has no live clip player.
     *
     * [`FieldClipPlayer::current_pose`]: legaia_engine_core::field_anim::FieldClipPlayer::current_pose
     */
    play_npc_live_bones(i: number): Int32Array;
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
     */
    play_npc_mesh(i: number): number;
    play_npc_mesh_cba_tsb(): Uint16Array;
    play_npc_mesh_flat_rgba(): Uint8Array;
    play_npc_mesh_indices(): Uint32Array;
    /**
     * Per-vertex TMD object index for the built NPC mesh - the bone each
     * vertex hangs from. The page's animator keys its per-frame `R . v + T`
     * on this.
     */
    play_npc_mesh_object_ids(): Uint32Array;
    play_npc_mesh_positions(): Float32Array;
    play_npc_mesh_uvs(): Uint8Array;
    /**
     * `[frame_count, bone_count]` of catalog entry `i`'s clip; `[0, 0]` when
     * it has none. `bone_count` is the clip's own count - the stride of
     * [`Self::play_npc_pose_frames`], and the count
     * [`Self::play_npc_mesh`] truncated the object table to.
     */
    play_npc_pose_dims(i: number): Uint32Array;
    /**
     * Catalog entry `i`'s clip, decoded to the pose format the JS animator
     * consumes: `6` entries per bone per frame (`[tx, ty, tz, rx, ry, rz]`,
     * absolute). Empty when the placement names no clip or its bundle is
     * unavailable. An NPC's clip is its placement `anim_id - 1` in the
     * scene's own ANM bundle (`docs/formats/anm.md` § per-scene bundle); a
     * global-pool special's indexes the PROT 0874 locomotion bundle instead
     * (the native window's bundle split).
     */
    play_npc_pose_frames(i: number): Int32Array;
    /**
     * Live world state of every catalogued NPC, flattened
     * `[x, y, z, facing_units, ...]` in catalog order. Positions come from the
     * **world** (`field_npc_positions`), so an NPC walking its MAN-authored
     * route walks on screen; the MAN placement anchor is the fallback for one
     * that has never moved. `y` is the floor height under the NPC.
     */
    play_npc_transforms(): Float32Array;
    /**
     * Draw lists for the field shop panel and the post-action banners over a
     * `surface_w` x `surface_h` canvas.
     *
     * Same shape as [`Self::play_menu_draws_json`] and
     * [`Self::play_dialog_draws_json`]: `{ "open", "sprites", "texts" }`,
     * sampling the atlases the `play_menu_*` accessors upload. `open` is
     * `false` when neither a shop nor a banner is up this frame.
     *
     * Like the dialog box (and unlike the pause menu) these composite over
     * the live field - retail draws both over the running scene.
     */
    play_overlay_draws_json(surface_w: number, surface_h: number): string;
    /**
     * Drive the open shop one frame from an edge-triggered PSX pad word
     * (same bit layout as [`Self::set_pad`]).
     *
     * When the session ends (the player picked **Exit**, clearing
     * `shop_session`), this calls `World::finish_field_shop` so the
     * suspended op-`0x49` flips Armed -> Done and the field VM advances past
     * the merchant op on its next step. Without that call the script would
     * stay parked forever.
     */
    play_shop_input(edge: number): void;
    /**
     * `true` while a field-VM merchant shop is up. The page freezes field
     * input and routes pad edges to [`Self::play_shop_input`] while this
     * holds, the same way it defers to the pause menu.
     */
    play_shop_is_open(): boolean;
    /**
     * Poll the retail prologue intro-skip (`FUN_801D1344`): while the
     * opening chain plays with the handoff bit armed, a confirm press skips
     * the whole remaining opening to `town01`. Returns the target scene
     * label once (the page then enters it), else `""`.
     *
     * The engine-side handoff marks the upcoming `town01` entry as the
     * new-game opening, which installs the establishing-sweep timeline whose
     * pinned op-`0x49` opens the name-entry overlay. That mark is kept: the
     * page draws the overlay ([`crate::play_name_entry`]), so the skip lands
     * in the same naming prompt the native window reaches.
     */
    play_take_prologue_handoff(confirm: boolean): string;
    /**
     * `true` when the lead's field mesh resolved out of the global TMD pool.
     */
    player_has_mesh(): boolean;
    player_mesh_cba_tsb(): Uint16Array;
    player_mesh_flat_rgba(): Uint8Array;
    /**
     * Player mesh geometry (object-local; pair with
     * [`Self::player_mesh_positions`], which poses it).
     */
    player_mesh_indices(): Uint32Array;
    /**
     * The player's vertices **posed at the current frame**: the world's live
     * `pose_frame` (idle clip standing, walk clip moving), composed per bone.
     * Falls back to the object-local rest geometry when no clip is installed -
     * which is what a lead outside the Vahn / Noa / Gala trio gets, since the
     * locomotion bundle only banks those three.
     */
    player_mesh_positions(): Float32Array;
    player_mesh_uvs(): Uint8Array;
    /**
     * `[world_x, world_y, world_z, facing_units]` for the player actor.
     * `facing_units` is the engine heading (`render_26`, PSX 12-bit; `0` =
     * travelling `+Z`); the world coords are the raw retail frame (`+Y` down).
     */
    player_transform(): Float32Array;
    /**
     * Active scene mode as a stable enum string (`Field`, `WorldMap`, ...).
     */
    scene_mode(): string;
    /**
     * Tell the engine where the camera is looking, so the free-movement
     * controller remaps the d-pad camera-relative ("up" walks away from the
     * camera). PSX 12-bit angle units (`4096` = a full turn); the field
     * controller quantises it to the nearest quarter-turn, as retail does.
     */
    set_camera_azimuth(units: number): void;
    /**
     * Route this frame's left analog stick into the engine. PSX convention:
     * signed bytes, X right-positive, Y **down**-positive; only read by the
     * precise-locomotion decode ([`Self::set_precise_movement`]).
     */
    set_left_stick(x: number, y: number): void;
    /**
     * Route this frame's pad word into the engine. Bit layout is the PSX digital
     * pad ([`legaia_engine_core::input::PadButton`]): `0x0008` Start, `0x0010`
     * Up, `0x0020` Right, `0x0040` Down, `0x0080` Left, `0x1000` Triangle,
     * `0x2000` Circle, `0x4000` Cross, `0x8000` Square. Edge detection is the
     * engine's - just hand it the held set each frame.
     */
    set_pad(mask: number): void;
    /**
     * Opt in / out of the engine's continuous locomotion decode
     * ([`legaia_engine_core::world::World::precise_movement`]): the camera
     * azimuth rotates the movement vector at full angular resolution and the
     * left analog stick ([`Self::set_left_stick`]) supplies an arbitrary
     * screen angle. The play page's VR first-person mode drives this so
     * "stick forward" walks exactly where the headset looks; the keyboard
     * path keeps the retail quantised 8-way remap.
     */
    set_precise_movement(on: boolean): void;
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
     */
    state_json(): string;
    /**
     * Advance the engine one frame. Returns `""` normally, or the label of the
     * scene the engine just walked into (a door / warp) - the page rebuilds its
     * render state whenever the return is non-empty.
     */
    tick_frame(): string;
}

/**
 * The site's shared sound-cue surface: renders every cue the minigame + arts
 * pages fire, once, off the loaded disc.
 */
export class LegaiaSfx {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Tactical-arts event -> cue map (see [`ART_EVENTS`]).
     */
    art_cues_json(): string;
    /**
     * Baka Fighter event -> cue map, with per-event provenance. The page
     * names events (`"hit"`, `"confirm"`, ...) and never hard-codes a cue id.
     */
    baka_cues_json(): string;
    /**
     * PROT entry the cues were rendered from (0 until [`Self::load_disc`]).
     */
    bank_prot_index(): number;
    /**
     * Resolve one event name to its cue id (`255` when the event is unknown -
     * no real descriptor uses `0xFF`).
     */
    cue_for_event(table: string, event: string): number;
    /**
     * Cue ids that rendered, in ascending order.
     */
    cue_ids(): Uint32Array;
    /**
     * One cue's interleaved-stereo i16 PCM at [`Self::sample_rate`]. Empty
     * when the id didn't render on this disc.
     */
    cue_pcm_i16(id: number): Int16Array;
    /**
     * Peak absolute sample of one cue (0 when absent). The page stages gain
     * off this so a quiet cue is audible without the loud ones clipping.
     */
    cue_peak(id: number): number;
    /**
     * Decode + render every site cue from a full Mode2/2352 disc image.
     *
     * Walks the retail chain: `SCUS_942.54` -> the static SFX descriptor
     * table, `PROT.DAT` -> the class-2 sound bank ([`SFX_BANK_PROT_INDEX`]),
     * then each cue's descriptor -> a one-shot through the clean-room SPU.
     * Holds only the rendered PCM afterwards (the disc bytes are dropped), so
     * a page can call this alongside its own decoder without a second copy of
     * the image.
     *
     * Returns JSON:
     * ```json
     * { "ok": true, "bank": 869, "rate": 44100,
     *   "cues": [ { "id": 9, "samples": 5400, "peak": 8123 }, ... ] }
     * ```
     */
    load_disc(bytes: Uint8Array): string;
    constructor();
    /**
     * Sample rate of every buffer [`Self::cue_pcm_i16`] returns.
     */
    sample_rate(): number;
}

export class LegaiaViewer {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Raw TIM bytes for battle-form atlas `atlas` (0..=6). 256x256 4bpp with
     * a 256x1 sub-CLUT row inside the TIM block.
     */
    battle_char_atlas_bytes(atlas: number): Uint8Array;
    /**
     * Bounding-sphere `[cx, cy, cz, r]` for the battle-form character.
     * Uses the **vertex centroid** (mean position) rather than the AABB
     * midpoint, so asymmetric poses (e.g. Vahn's stance with the weapon
     * extended past the body's X axis) don't pull the camera target off the
     * torso. Radius is the max distance from the centroid to any vertex.
     */
    battle_char_mesh_bounds(slot: number): Float32Array;
    /**
     * Per-vertex `[cba, tsb]` for the battle-form character.
     */
    battle_char_mesh_cba_tsb(slot: number): Uint32Array;
    /**
     * Triangle indices for the battle-form character at slot `slot`.
     */
    battle_char_mesh_indices(slot: number): Uint32Array;
    /**
     * Per-vertex normals for the battle-form character at slot `slot`.
     */
    battle_char_mesh_normals(slot: number): Float32Array;
    /**
     * Per-vertex TMD object index for the battle-form character at slot
     * `slot`, parallel to [`Self::battle_char_mesh_positions`]. The JS-side
     * player-ANM animator uses it to apply per-bone (per-object) transforms.
     */
    battle_char_mesh_object_ids(slot: number): Uint32Array;
    /**
     * Per-vertex positions for the battle-form character at pack slot `slot`.
     */
    battle_char_mesh_positions(slot: number): Float32Array;
    /**
     * Per-vertex `[u, v]` integer texel coords for the battle-form character.
     */
    battle_char_mesh_uvs(slot: number): Int32Array;
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
     */
    battle_char_pack_json(): string;
    /**
     * Raw disc-form TMD bytes for battle-form slot `slot`.
     */
    battle_char_tmd_bytes(slot: number): Uint8Array;
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
     */
    battle_char_vram_bytes(): Uint8Array;
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
     */
    battle_char_vram_bytes_battle(): Uint8Array;
    /**
     * Number of CLUT palettes available for cataloged TIM `id` (0 for
     * 16/24bpp TIMs, which carry no palette).
     */
    catalog_clut_count(id: number): number;
    /**
     * JSON describing cataloged TIM `id` (offset, owning entry, dimensions,
     * CLUT count, byte length, fingerprint) for the info panel.
     */
    catalog_info_json(id: number): string;
    /**
     * Number of cataloged TIMs in the loaded PROT.DAT.
     */
    catalog_len(): number;
    /**
     * Bounding-sphere `[cx, cy, cz, r]` so the JS viewer can frame the model.
     * Uses `centroid_bounds` so asymmetric poses (weapon extended, arm out)
     * don't pull the camera target off the body.
     */
    character_mesh_bounds(slot: number, equip_byte: number): Float32Array;
    /**
     * Per-vertex `[cba, tsb]` (CLUT-base / texture-page descriptor) so the
     * JS shader can resolve VRAM texel + palette per the standard PSX TMD
     * model. `2 u32` per vertex, parallel to [`Self::character_mesh_positions`].
     */
    character_mesh_cba_tsb(slot: number, equip_byte: number): Uint32Array;
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
     */
    character_mesh_flat_colors(slot: number, equip_byte: number): Uint8Array;
    /**
     * Triangle indices for the player character at pack slot `slot`,
     * `u32`, multiple of 3.
     */
    character_mesh_indices(slot: number, equip_byte: number): Uint32Array;
    /**
     * Per-vertex normals parallel to [`Self::character_mesh_positions`].
     */
    character_mesh_normals(slot: number, equip_byte: number): Float32Array;
    /**
     * Per-vertex TMD object index for the player character at pack slot
     * `slot`, parallel to [`Self::character_mesh_positions`]. The JS-side
     * player-ANM animator uses it to apply per-bone (per-object) transforms
     * without re-uploading geometry.
     */
    character_mesh_object_ids(slot: number, equip_byte: number): Uint32Array;
    /**
     * Per-vertex positions for the player character at pack slot `slot`,
     * optionally with the equipment swap applied (`equip_byte` < 0 means
     * "no swap, draw disc-form mesh"). Empty if `slot` is out of range or
     * the disc isn't loaded.
     */
    character_mesh_positions(slot: number, equip_byte: number): Float32Array;
    /**
     * Per-vertex `[u, v]` integer texel coords (parallel to
     * [`Self::character_mesh_positions`], 2 i32 per vertex). The site page
     * pairs these with the PROT 0876 atlas page to do its own NEAREST
     * sample; we keep the integer texels here instead of normalising
     * because the atlas dimensions aren't surfaced yet.
     */
    character_mesh_uvs(slot: number, equip_byte: number): Int32Array;
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
     */
    character_pack_json(): string;
    /**
     * Raw disc-form TMD bytes for slot `slot` - the same bytes the engine
     * installs into `DAT_8007C018[slot]`. Useful for an in-page .tmd
     * download / debug round-trip.
     */
    character_tmd_bytes(slot: number): Uint8Array;
    /**
     * Number of TMDs in the currently-loaded continent pack. 0 when no
     * continent pack was found for this kingdom.
     */
    continent_pack_count(): number;
    /**
     * Select the active continent pack slot. Parallel to `pack_mesh` but
     * operates on the continent pack.
     */
    continent_pack_mesh(slot: number): number;
    continent_pack_mesh_bounds(): Float32Array;
    continent_pack_mesh_cba_tsb(): Uint16Array;
    continent_pack_mesh_indices(): Uint32Array;
    continent_pack_mesh_positions(): Float32Array;
    continent_pack_mesh_uvs(): Uint8Array;
    /**
     * VRAM bytes (1 MB) built from the continent pack's slot 0. Distinct from
     * the landmark VRAM since the two packs ship independent TIM_LISTs.
     */
    continent_pack_vram_bytes(): Uint8Array;
    /**
     * PROT index the continent pack was loaded from (0 when none).
     */
    continent_prot_index(): number;
    /**
     * JSON-encoded summary of the current entry - class label, byte size,
     * MES record count (if any), SEQ presence (if any), VAB presence
     * (if any). The JS side parses this and shows it in the inspector
     * panel without needing N round-trips for each individual field.
     */
    current_entry_info_json(): string;
    /**
     * True if the current entry has a parseable TMD, suitable for the 3D
     * rendering path. JS uses this to decide whether to switch to the 3D
     * render loop instead of the TIM blit.
     */
    current_has_tmd(): boolean;
    current_index(): number;
    /**
     * Resolve a MES message id to its first 64 bytes as a hex string (for
     * preview in the inspector panel). Returns an empty string if the
     * current entry isn't a MES container or `text_id` is out of range.
     */
    current_mes_message_hex(text_id: number): string;
    /**
     * Build a 1024×512 PSX VRAM from every TIM the current entry contains.
     * Returns the raw bytes (2 MB if a CLUT block is present, but VRAM is
     * always exactly 1 MB = 1024×512×2). Used by the WebGL2 path to upload
     * to a R16UI texture.
     */
    current_vram_bytes(): Uint8Array;
    /**
     * Number of CLUT palettes available for deep-catalog TIM `id`.
     */
    deep_catalog_clut_count(id: number): number;
    /**
     * JSON describing deep-catalog TIM `id` (owning entry, LZS section,
     * offset within the decoded section, dimensions, CLUT count, byte
     * length, fingerprint) for the info panel.
     */
    deep_catalog_info_json(id: number): string;
    /**
     * Number of cataloged compressed TIMs in the loaded PROT.DAT.
     */
    deep_catalog_len(): number;
    entry_count(): number;
    /**
     * Returns a JSON array describing every viewable entry: PROT index, class,
     * dimensions, has-TMD flag. The UI uses this to populate a sidebar list / search.
     */
    entry_list_json(): string;
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
     */
    field_char_vram_bytes(): Uint8Array;
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
     */
    field_npc_catalog_json(): string;
    /**
     * Build (and cache) one catalogued NPC's mesh. The **field-hybrid** build:
     * textured prims that sample the scene VRAM plus the untextured
     * flat/gouraud prims that carry per-vertex RGB, in one vertex stream with
     * parallel per-vertex object ids - so the page can both render the
     * colour-only body parts and compose the ANM pose. Returns the catalog
     * index.
     */
    field_npc_mesh(catalog_idx: number): number;
    /**
     * Bounding sphere `[cx, cy, cz, r]` of the built mesh, for camera framing.
     */
    field_npc_mesh_bounds(): Float32Array;
    field_npc_mesh_cba_tsb(): Uint16Array;
    /**
     * Per-vertex `[r, g, b, textured_flag]` for the hybrid render.
     */
    field_npc_mesh_flat_rgba(): Uint8Array;
    field_npc_mesh_indices(): Uint32Array;
    /**
     * Per-vertex TMD object index, parallel to the positions - the bone each
     * vertex belongs to. The page's animator keys the per-frame
     * `R . v + T` on this.
     */
    field_npc_mesh_object_ids(): Uint32Array;
    field_npc_mesh_positions(): Float32Array;
    field_npc_mesh_uvs(): Uint8Array;
    field_scene_ground_cba_tsb(): Uint16Array;
    field_scene_ground_indices(): Uint32Array;
    /**
     * Ground-heightfield accessors (same layout as the kingdom
     * `walk_ground_*` family; empty when the scene has no resolvable floor
     * grid).
     */
    field_scene_ground_positions(): Float32Array;
    field_scene_ground_quad_count(): number;
    field_scene_ground_uvs(): Uint8Array;
    /**
     * Select the active environment-pack slot and build its mesh: the
     * textured prims whose pages/CLUTs are resident in the field VRAM
     * (matches the engine's per-prim filter) **plus** the untextured
     * `F*`/`G*` vertex-colour prims, merged by [`build_hybrid_env_mesh`]
     * (the engine-shell's colour-mesh pipeline sibling). Returns the slot,
     * or an error when out of range. Subsequent `field_scene_mesh_*` calls
     * read the built mesh.
     */
    field_scene_mesh(slot: number): number;
    field_scene_mesh_cba_tsb(): Uint16Array;
    /**
     * Per-vertex `[r, g, b, flag]` bytes for the current mesh's hybrid
     * flat-colour render (`flag` 255 = textured vertex, sample VRAM; 0 =
     * untextured vertex, use the RGB). **Empty** when the mesh carries no
     * untextured prims - the JS side then skips binding the attribute and
     * the draw behaves exactly like the pure-textured path.
     */
    field_scene_mesh_flat_rgba(): Uint8Array;
    field_scene_mesh_indices(): Uint32Array;
    field_scene_mesh_positions(): Float32Array;
    field_scene_mesh_uvs(): Uint8Array;
    /**
     * Number of TMDs in the loaded field scene's environment pack. 0 when
     * no field scene is loaded.
     */
    field_scene_pack_count(): number;
    /**
     * Per-placement world positions `[x, y, z, ...]` (flattened), same
     * pre-Y-flip world frame as the ground heightfield (draw with the shared
     * `(1, -1, 1)` model flip at scale 1).
     */
    field_scene_placement_positions(): Float32Array;
    /**
     * Per-placement authored yaw (object record `+0x0A`), PSX angle units
     * (`4096` = full revolution), in placement order. Convert with
     * `rotY = -(rot & 0xFFF) * Math.PI / 2048` for `placementModelScaled*`.
     */
    field_scene_placement_rot_y(): Uint16Array;
    /**
     * Per-placement env-pack slot, one `u32` per placed object. Feed each
     * into [`Self::field_scene_mesh`] and draw at the matching
     * [`Self::field_scene_placement_positions`] entry.
     */
    field_scene_placement_slots(): Uint32Array;
    /**
     * One-line JSON status for the UI:
     * `{"name", "pack_count", "placements", "terrain", "ground_quads"}`.
     */
    field_scene_status_json(): string;
    /**
     * Per-terrain-tile world positions `[x, y, z, ...]` (flattened).
     */
    field_scene_terrain_positions(): Float32Array;
    /**
     * Per-terrain-tile authored yaw, same encoding as
     * [`Self::field_scene_placement_rot_y`].
     */
    field_scene_terrain_rot_y(): Uint16Array;
    /**
     * Per-terrain-tile env-pack slot (the dense `CELL_VISIBLE` decor layer).
     */
    field_scene_terrain_slots(): Uint32Array;
    /**
     * Field-mode VRAM bytes (1 MB) shared by every env-pack mesh + the
     * ground heightfield. Empty when no field scene is loaded.
     */
    field_scene_vram_bytes(): Uint8Array;
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
     */
    fog_lut_bytes(): Uint8Array;
    /**
     * Decoded RGBA8 pixels for one publisher-logo TIM (0..3). Returns
     * an empty vec when the disc doesn't have PROT 0895 or `idx` is
     * out of range. Width / height come from [`init_pak_logos_json`].
     */
    init_pak_logo_rgba(idx: number): Uint8Array;
    /**
     * JSON metadata for the boot publisher-logo TIMs from PROT 0895
     * (`init.pak`). Returns an empty array `"[]"` if the disc doesn't
     * have PROT 0895 or the entry doesn't parse as init.pak.
     *
     * Each element shape:
     *   `{ "name": str, "width": u32, "height": u32, "mode": u32,
     *      "fb_x": u32, "fb_y": u32 }`
     */
    init_pak_logos_json(): string;
    /**
     * Load a disc image. Auto-detects: full Mode2/2352 .bin, raw PROT.DAT,
     * or single TIM. Returns the count of viewable entries (entries with at
     * least one decodable TIM) for the JS UI.
     */
    load_disc(bytes: Uint8Array): number;
    /**
     * Locate a `PROT.DAT` byte offset -> the entry that truly owns it.
     *
     * `offset` is decimal or `0x`-hex text (as a hex editor / the CLI shows).
     * When `in_entry` is `Some(n)`, `offset` is instead read as an offset
     * inside entry `n`'s extracted `.BIN` file and first translated to an
     * absolute `PROT.DAT` offset - the common "my hex editor is 0x… into
     * `0866_*.BIN`" case.
     *
     * Shape (all `*_hex` are strings; byte quantities are also given raw):
     * ```json
     * { "query": "0x17855", "in_entry": 866,
     *   "abs_offset": 96341, "abs_offset_hex": "0x17855",
     *   "owner": { "index": 867, "block": "battle_data",
     *              "start": 96256, "start_hex": "0x17800",
     *              "footprint": 15622144, "footprint_hex": "0xEE6000",
     *              "local_offset": 85, "local_offset_hex": "0x55",
     *              "is_monster_archive": true },
     *   "over_read": { "queried_entry": 866, "queried_footprint": 2048,
     *                  "queried_footprint_hex": "0x800",
     *                  "message": "0x17855 is past entry 866's footprint ...",
     *                  "true_owner_label": "entry 867 battle_data" },
     *   "covering": [ { "index": 866, "block": "battle_data", "role": "over-read copy" },
     *                 { "index": 867, "block": "battle_data", "role": "true source" } ] }
     * ```
     * `owner` is `null` when the offset is past every entry's footprint (tail
     * padding). `over_read` is `null` unless the offset sits in more than one
     * extracted window (i.e. at least one file carries it as a neighbour's
     * over-read copy). `{ "error": "..." }` on a bad offset / unparsable disc.
     */
    locate_offset_json(offset: string, in_entry?: number | null): string;
    /**
     * Returns the model's bounding sphere center (`[cx, cy, cz]`) and radius
     * `r` packed as `[cx, cy, cz, r]`. JS uses this to build the MVP matrix
     * without re-parsing the TMD each frame.
     */
    mesh_bounds(): Float32Array;
    mesh_cba_tsb(): Uint16Array;
    mesh_indices(): Uint32Array;
    /**
     * Returns the mesh data for the current entry's TMD as four typed arrays
     * concatenated by use:
     *   `[positions(f32 ×3 per vert), uvs(u8 ×2), cba_tsb(u16 ×2), indices(u32)]`
     * Each as a separate getter so JS can pull them as typed arrays without
     * reparsing JSON.
     */
    mesh_positions(): Float32Array;
    mesh_uvs(): Uint8Array;
    /**
     * Keyframes for monster `id`'s action animation at array `index` (the
     * position in [`Self::monster_animations_json`]). Same flat layout as
     * [`Self::monster_idle_animation_frames`]: six `i32` per part per frame,
     * `[tx, ty, tz, rx, ry, rz]`, with frame `f` / part `p` / component `c` at
     * `(f * part_count + p) * 6 + c`. Empty if the index is out of range or the
     * slot has no decodable animation.
     */
    monster_animation_frames_at(id: number, index: number): Int32Array;
    /**
     * Metadata for **every** decodable action animation of monster `id`, as a
     * JSON array in `+0x4C` action-table order:
     * `[{"action_id":N,"part_count":P,"frame_count":F}, ...]`. Array index `0`
     * is the idle loop (see [`Self::monster_idle_animation_header`]); the rest
     * are the monster's attack / spell / special actions. The array index is
     * the handle the JS viewer passes to [`Self::monster_animation_frames_at`]
     * to fetch a given action's keyframes. `"[]"` if the slot is empty / filler
     * or carries no decodable animation.
     */
    monster_animations_json(id: number): string;
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
     */
    monster_archive_json(): string;
    /**
     * Monster `id`'s mesh + baked texture + **all** action animations packed
     * into one binary glTF (`.glb`) blob - the universal format that carries
     * geometry, material, and animation together (Blender / three.js / etc.).
     * Each TMD object becomes an animated node; the texture is baked into a
     * per-palette atlas. Empty if the slot has no exportable mesh.
     */
    monster_glb(id: number): Uint8Array;
    /**
     * Monster `id`'s idle animation keyframes as a flat `i32` array, six values
     * per part per frame: `[tx, ty, tz, rx, ry, rz]`. Frame `f`, part `p`,
     * component `c` is at `(f * part_count + p) * 6 + c`. Translations are
     * signed model units; rotations are unsigned 12-bit angles (`4096` = a full
     * turn). Empty if the slot has no decodable idle animation.
     */
    monster_idle_animation_frames(id: number): Int32Array;
    /**
     * `[part_count, frame_count]` for monster `id`'s **idle** animation (action
     * index 0). `[0, 0]` if the slot has no decodable animation. Pair with
     * [`Self::monster_idle_animation_frames`].
     */
    monster_idle_animation_header(id: number): Uint32Array;
    /**
     * Bounding-sphere `[cx, cy, cz, r]` for monster `id`'s mesh, so the JS
     * side can frame the model without re-parsing the geometry.
     */
    monster_mesh_bounds(id: number): Float32Array;
    /**
     * Triangle indices for monster `id`'s mesh (`u32`, multiple of 3).
     */
    monster_mesh_indices(id: number): Uint32Array;
    /**
     * Per-vertex smooth normals for monster `id`'s mesh (parallel to
     * [`Self::monster_mesh_positions`]).
     */
    monster_mesh_normals(id: number): Float32Array;
    /**
     * Per-vertex TMD object (body-part) index for monster `id`'s mesh, parallel
     * to [`Self::monster_mesh_positions`]. The JS idle-animation player uses it
     * to apply each animated part's per-frame transform. Empty if no mesh.
     */
    monster_mesh_object_ids(id: number): Uint32Array;
    /**
     * Per-vertex palette index (`cba & 0x3F`) for monster `id`'s mesh, as
     * floats (parallel to [`Self::monster_mesh_positions`]). The JS shader
     * uses it to pick the row of the palette texture.
     */
    monster_mesh_palette_index(id: number): Float32Array;
    /**
     * Per-vertex `[x, y, z]` positions for monster `id`'s mesh (flat array,
     * 3 floats per vertex). Empty if the id has no mesh.
     */
    monster_mesh_positions(id: number): Float32Array;
    /**
     * Per-vertex texture coords for monster `id`'s mesh, normalised to
     * `[0, 1]` against the texture-page dimensions (parallel to
     * [`Self::monster_mesh_positions`], 2 floats per vertex). Empty if the id
     * has no mesh or no texture.
     */
    monster_mesh_uvs(id: number): Float32Array;
    /**
     * `[width, height]` of monster `id`'s texture page in texels (128 or 256
     * wide, always 256 tall). `[0, 0]` if the id has no texture.
     */
    monster_texture_dims(id: number): Uint32Array;
    /**
     * Monster `id`'s 4bpp texture page as one palette index (`0..=15`) per
     * texel, row-major (`width * height` bytes). Upload as an `R8UI`/`R8`
     * texture and pair with [`Self::monster_texture_palette_rgba`]. Empty if
     * the id has no texture.
     */
    monster_texture_indices(id: number): Uint8Array;
    /**
     * Monster `id`'s 15 palettes flattened to a `15 * 16` RGBA8 row (palette
     * `p`, colour `c` at pixel `p * 16 + c`). Index-0 transparent colours
     * carry alpha 0. Empty if the id has no texture.
     */
    monster_texture_palette_rgba(id: number): Uint8Array;
    constructor(canvas_id: string);
    next_entry(): number;
    /**
     * 13-frame ocean CLUT animation table: 13 × 32 bytes = 416 bytes,
     * frame-0 first. Each frame is 16 BGR555 entries (the same shape as
     * the first 16 entries of [`Self::ocean_base_clut_bytes`]). The
     * runtime DMAs one frame at a time onto VRAM (0, 506) to cycle
     * the wave colours through the ocean tile.
     */
    ocean_animation_frames(): Uint8Array;
    /**
     * Static base CLUT for the ocean tile row: 256 entries × 2 bytes
     * (BGR555 LE) = 512 bytes. The first 16 entries are the ones the
     * animation cycle overrides each frame; entries 16..255 stay fixed
     * and belong to other tiles sharing the same VRAM row.
     */
    ocean_base_clut_bytes(): Uint8Array;
    /**
     * Number of valid ocean animation frames (typically 13). Returns 0
     * when the kingdom doesn't have ocean assets.
     */
    ocean_frame_count(): number;
    /**
     * Ocean tile pixel data (4bpp indexed), 64 halfwords × 256 rows =
     * 32 768 bytes. Each byte holds 2 pixels (low nibble first). The
     * CLUT index addressing is `pixel = byte >> 4` for the high pixel
     * and `byte & 0x0F` for the low pixel. Empty when the kingdom is
     * not a world-map kingdom or the ocean TIM wasn't found.
     */
    ocean_texture_bytes(): Uint8Array;
    /**
     * Number of TMDs in the currently-loaded kingdom pack. 0 when no
     * kingdom is loaded.
     */
    pack_count(): number;
    /**
     * Set the active pack-mesh slot. Subsequent `pack_mesh_*` calls source
     * from `pack[byte_offsets[slot]..byte_ends[slot]]`. Returns an error
     * when no kingdom is loaded or `slot >= pack count`.
     */
    pack_mesh(slot: number): number;
    pack_mesh_bounds(): Float32Array;
    pack_mesh_cba_tsb(): Uint16Array;
    pack_mesh_indices(): Uint32Array;
    /**
     * Parallel to [`Self::mesh_positions`] but sources from the currently
     * selected kingdom pack slot.
     */
    pack_mesh_positions(): Float32Array;
    pack_mesh_uvs(): Uint8Array;
    /**
     * VRAM bytes (1 MB) built from every TIM in the kingdom's slot 0
     * (TIM_LIST). Reuse across every `pack_mesh_*` call - the kingdom
     * pack's per-slot TMDs all sample from this one shared image.
     */
    pack_vram_bytes(): Uint8Array;
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
     */
    player_anm_corpus_json(): string;
    /**
     * Find a single player-ANM bundle by its PROT entry index and return
     * the LZS-decoded bytes. Empty if the entry doesn't carry a bundle.
     */
    player_anm_decoded(prot_index: number): Uint8Array;
    /**
     * Raw bytes of one record from the player-ANM bundle at `prot_index`.
     * Includes the per-record header (`a`, `b`, `marker_1 = 0x080C`, `flag`),
     * the 8-byte per-anim prologue, and the
     * `(frame_count × bone_count × 8)` byte frame table.
     */
    player_anm_record_bytes(prot_index: number, record_index: number): Uint8Array;
    /**
     * `[bone_count, frame_count]` for one player-ANM record so the JS
     * animator can size its scratch buffers without re-walking the bundle.
     * Empty `[0, 0]` if the record doesn't exist or fails size invariants.
     */
    player_anm_record_dims(prot_index: number, record_index: number): Uint32Array;
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
     */
    player_anm_record_frames(prot_index: number, record_index: number): Uint8Array;
    /**
     * Decoded per-record header for one player-ANM record. Returned as a
     * `Vec<i32>` packed as `[a, b, marker_1, flag, bone_count, frame_count,
     * frame0_bone0_u8[0..8]]` - total 14 entries (the 8 bytes after the
     * header are bone 0 of frame 0's TR entry, since the body sits
     * immediately after the 8-byte header - there is no prologue).
     * Returns an empty Vec on out-of-range record or size-invariant failure.
     */
    player_anm_record_header(prot_index: number, record_index: number): Int32Array;
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
     */
    player_anm_record_pose_frames(prot_index: number, record_index: number, target_part_count: number): Int32Array;
    prev_entry(): number;
    /**
     * Every entry whose extracted `.BIN` window over-reads its true footprint -
     * i.e. its tail carries the next entry's bytes. Mirrors the `OVR` column of
     * `prot-extract list`, but returns only the flagged rows (the trap-bearing
     * ones); the vast majority of entries declare exactly their footprint.
     *
     * Shape:
     * ```json
     * { "total_entries": 1231, "over_read_count": 2,
     *   "entries": [ { "index": 865, "block": "battle_data", "lba": 76288,
     *                  "byte_offset": 156237824, "byte_offset_hex": "0x9500000",
     *                  "declared_size": 16777216, "declared_size_hex": "0x1000000",
     *                  "footprint": 2048, "footprint_hex": "0x800",
     *                  "over_read_bytes": 16775168 }, ... ] }
     * ```
     * `{ "error": "..." }` when no disc is loaded / the TOC won't parse.
     */
    prot_over_read_json(): string;
    /**
     * Render cataloged TIM `id` with CLUT `clut` into the 2D canvas named
     * `canvas_id`. The catalog browser uses its own canvas (separate from
     * the PROT-entry browser's, which switches between 2D and WebGL), so it
     * takes the target id explicitly rather than the viewer's bound canvas.
     */
    render_catalog_tim(id: number, clut: number, canvas_id: string): void;
    /**
     * Render deep-catalog TIM `id` with CLUT `clut` into the 2D canvas named
     * `canvas_id`.
     */
    render_deep_catalog_tim(id: number, clut: number, canvas_id: string): void;
    /**
     * Render the current entry's TMD at the given rotation into a flat
     * `Vec<f32>` of triangle data (7 floats per triangle, painter's-sorted
     * back-to-front).
     *
     * Format per triangle: `[x0, y0, x1, y1, x2, y2, brightness 0..1]`.
     *
     * Returns an empty vec if the current entry has no TMD or the TMD has
     * no triangles.
     */
    render_tmd_triangles(yaw: number, pitch: number, distance: number, pan_x: number, pan_y: number, viewport_w: number, viewport_h: number): Float32Array;
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
     */
    save_state_framebuffer(save_state_bytes: Uint8Array): Uint8Array;
    /**
     * `flags` packs the prim cmd-byte mode bits: bit 0 = semi-transparent,
     * bit 1 = raw texture (skip color modulation). JS computes the model-view
     * matrix from `screen_w / screen_h` (orthographic 0..w x h..0 viewport).
     */
    save_state_prim_replay(save_state_bytes: Uint8Array): Uint8Array;
    /**
     * Place mesh handle `mesh` at `(tx, ty, tz)` with `rot_y` radians about
     * +Y and uniform `scale` - the same triple the page's
     * `placementModelScaledY` builds its model matrix from.
     */
    scene_export_add_instance(mesh: number, tx: number, ty: number, tz: number, rot_y: number, scale: number): void;
    /**
     * Register a reusable mesh (the exact streams the page renders:
     * `positions` f32 xyz PSX-space, `uvs` u8 page-local texel pairs,
     * `cba_tsb` u16 `[cba, tsb]` pairs, u32 triangle indices, and the
     * optional hybrid `flat_rgba` side channel - pass an empty array for
     * pure-textured meshes). Returns the mesh handle for
     * [`Self::scene_export_add_instance`], or `u32::MAX` when no session
     * is open.
     */
    scene_export_add_mesh(name: string, positions: Float32Array, uvs: Uint8Array, cba_tsb: Uint16Array, indices: Uint32Array, flat_rgba: Uint8Array): number;
    /**
     * Start a fresh export session named `name` (becomes the glTF root
     * node name). Discards any prior unfinished session.
     */
    scene_export_begin(name: string): void;
    /**
     * Bake the accumulated session into `.glb` bytes and close it. Returns
     * an empty array when the session is missing or contains no drawable
     * geometry.
     */
    scene_export_finish(): Uint8Array;
    /**
     * Supply the 1 MiB VRAM image (`1024*512` LE u16 words - the same bytes
     * the page uploads to its R16UI texture) the atlas bake reads from.
     */
    scene_export_set_vram(bytes: Uint8Array): void;
    set_clut(idx: number): void;
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
     */
    set_scene_field(name: string): number;
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
     */
    set_scene_kingdom(prot_base: number): number;
    /**
     * Load a CDNAME scene and catalog every NPC / actor its MAN places.
     * Loads the field scene first when it isn't already resident (so
     * `field_scene_vram_bytes` is the VRAM these meshes sample). Returns the
     * number of catalogued placements.
     */
    set_scene_npcs(name: string): number;
    /**
     * Jump to the slot in the filtered list (NOT the PROT index). Used by
     * the dropdown / list-click UI.
     */
    set_slot(slot: number): number;
    /**
     * Per-body inventory of the slot-4 wireframe, as a JSON string.
     * Used by the inspector panel to show which bodies are present.
     * Returns `"[]"` when slot 4 can't be decoded.
     */
    slot4_body_inventory_json(prot_base: number): string;
    /**
     * Bounding box of every non-zero record in the kingdom's slot-4
     * wireframe, as `[amin, bmin, amax, bmax]` (i32) for the requested
     * axis pair (`"xz"` / `"xy"` / `"zy"`, etc). Useful for re-framing
     * the top-down camera when the overlay is toggled on. Empty vec
     * when slot 4 can't be decoded.
     */
    slot4_wireframe_bounds(prot_base: number, axes: string): Int32Array;
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
     */
    slot4_wireframe_lines(prot_base: number, style: string, axes: string): Uint8Array;
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
     */
    slot4_wireframe_points(prot_base: number, axes: string): Uint8Array;
    /**
     * JSON status string: PROT index, class name, dims, current slot.
     */
    status(): string;
    /**
     * Per-vertex `[clut, tpage]` (PSX CBA + tpage words) of the walk-view
     * ground, flattened. Distinct per cell so grass / mountain / water / forest
     * cells sample their own VRAM page from the kingdom slot-0 atlas.
     */
    walk_ground_cba_tsb(): Uint16Array;
    /**
     * Triangle indices of the walk-view ground (two triangles per cell quad).
     */
    walk_ground_indices(): Uint32Array;
    /**
     * Per-vertex world positions of the walk-view continent ground
     * heightfield, flattened `[x, y, z, ...]`. Empty until a kingdom is loaded.
     * Same pre-Y-flip world frame as the landmark placement draws, so the JS
     * renderer applies the same `(1, -1, 1)` model flip (scale 1, no offset).
     */
    walk_ground_positions(): Float32Array;
    /**
     * Number of ground cells (quads) in the walk-view heightfield. 0 when no
     * kingdom is loaded or the heightfield couldn't be resolved.
     */
    walk_ground_quad_count(): number;
    /**
     * Per-vertex page-local UVs (`u8` pairs) of the walk-view ground, flattened
     * `[u, v, ...]`. Each cell's four corners cover its `32 x 32` atlas tile.
     */
    walk_ground_uvs(): Uint8Array;
    /**
     * Number of walk-frame placed landmarks for the currently-loaded kingdom
     * (slot-1 pack meshes positioned on the continent terrain). 0 when no
     * kingdom is loaded or the walk `.MAP` / floor LUT couldn't be resolved.
     */
    walk_placement_count(): number;
    /**
     * Per-placement world positions `[x, y, z, ...]` (flattened), in the same
     * pre-Y-flip `col*128` world frame as [`Self::walk_ground_positions`], so
     * the JS renderer draws each landmark with the same `(1, -1, 1)` model
     * flip at scale `1` (the slot-1 meshes are already in true world units).
     */
    walk_placement_positions(): Float32Array;
    /**
     * Per-placement authored yaw (object record `+0x0A`), one value per
     * walk-frame landmark in placement order, in PSX angle units (`4096` =
     * full revolution) - the Sebucus island bridges' quarter-turns and the
     * decoration layer's per-tree variety. The JS renderer converts with
     * `rotY = -(rot & 0xFFF) * Math.PI / 2048` (retail's yaw sense is the
     * opposite of `placementModelScaled*`'s).
     */
    walk_placement_rot_y(): Uint16Array;
    /**
     * Per-placement kingdom pack-mesh slot (record `+0x10`), one `u32` per
     * walk-frame landmark in placement order. Feed each into `pack_mesh` to
     * select the mesh, then draw it at the matching
     * [`Self::walk_placement_positions`] entry.
     */
    walk_placement_slots(): Uint32Array;
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
     */
    worldmap_menu_json(): string;
}

/**
 * One rendered `music_01` track handed to the site jukebox: seamless-loop PCM
 * plus the loop region and sample rate. Getters copy into JS typed arrays; a
 * consumer calls `pcm` once. `ok` is false when the entry didn't decode.
 */
export class Music01Render {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Frame index where the loop body ends (`loopEnd`); one SEQ period after
     * [`Self::loop_start`]. Equals the frame length of [`Self::pcm`].
     */
    readonly loop_end: number;
    /**
     * Frame index where the repeatable loop body starts (`AudioBufferSourceNode.loopStart`
     * = `loop_start / rate`). `0` when no loop region was found.
     */
    readonly loop_start: number;
    /**
     * Whether the track decoded (non-empty PCM).
     */
    readonly ok: boolean;
    /**
     * Interleaved-stereo i16 PCM (`[l0, r0, l1, r1, ...]`) at [`Self::rate`].
     */
    readonly pcm: Int16Array;
    /**
     * PCM sample rate (the SPU's 44.1 kHz).
     */
    readonly rate: number;
}

/**
 * The 16x16 memory-card icon baked into save block `block` of a card
 * container, as 1024 RGBA8 bytes. For Legaia saves this is the lead
 * character's portrait - the retail save writer copies the load-screen
 * portrait TIM into the SC block (palette `+0x60`, 4bpp pixels `+0x80`).
 * The site's save bar draws it as the slot's face.
 */
export function card_icon_rgba(bytes: Uint8Array, block: number): Uint8Array;

/**
 * Bank a coin balance into save block `block` of a card container,
 * returning the whole container with **only those 4 bytes changed** - the
 * same format it came in, still a valid retail save (the retail payload
 * carries no checksum; the card's directory-frame checksums are untouched).
 */
export function card_patch_coins(bytes: Uint8Array, block: number, coins: number): Uint8Array;

/**
 * Read the casino coin bank from save block `block` of a card container.
 */
export function card_read_coins(bytes: Uint8Array, block: number): number;

/**
 * List the Legaia saves inside an emulator save container.
 *
 * Accepts raw `.mcr`/`.mcd` card images, DexDrive `.gme`, and single-save
 * `.mcs`. Returns
 * `{"format": "mcr"|"gme"|"mcs", "saves": [{block, product_code, valid,
 * party, money, coins, location, scene}, ...]}`. Errors (thrown as JS
 * strings) on unknown containers and on signed `.psv` exports.
 */
export function card_saves_json(bytes: Uint8Array): string;

/**
 * One of the three retail 16x16 **save-file portrait** TIMs decoded to a
 * 1024-byte RGBA8 buffer: `0` = Vahn, `1` = Noa, `2` = Gala. Accepts either
 * a full Mode2/2352 disc image or raw `PROT.DAT` bytes - the same input
 * [`LegaiaRuntime::load_disc`] takes - so the play page can draw the party
 * roster faces beside each save tile from the disc it already loaded, exactly
 * as the minigames save bar does from its `LegaiaMinigames`
 * (`save_portrait_rgba`). These are the load-screen slot-grid portraits
 * pinned in the pre-`init_data` gap of `PROT.DAT` (offset `0x1AC90`, 192-byte
 * stride); retail bakes the lead's copy into every SC block, so they are the
 * exact faces a retail save carries. Empty when no PROT is found or the TIM
 * doesn't parse - the bar falls back to initial chips.
 */
export function disc_portrait_rgba(bytes: Uint8Array, char_id: number): Uint8Array;

/**
 * Export a **working** language pack (source-bearing, all `translation:`
 * fields empty) from the user's own disc, as YAML text they can download and
 * fill in. This is the authoring on-ramp - the community can produce their own
 * packs without any tooling beyond the browser. The exported text is the
 * user's own disc data and never leaves the browser.
 *
 * `language` stamps the pack header (`fr`, `de`, ...); pass `en` for a plain
 * source dump. Returns the YAML string.
 */
export function export_lang_pack(image: Uint8Array, language: string): string;

/**
 * Lift the **official** French / German / Italian localization off a PAL disc
 * the user also owns, re-keyed onto their USA disc's coordinate space.
 *
 * Same user-supplied-asset model as the base disc: `source_image` is the
 * user's own PAL `.bin` (`SCES_019.44` FR / `.45` DE / `.46` IT), it is read
 * in this tab, and neither image is uploaded anywhere. The result is a
 * **working** pack (`source:` = USA text, `translation:` = official text) that
 * the page feeds straight back into [`patch_rom`]'s `lang_pack` argument, so
 * the official text goes through the exact same two-phase import - and the
 * same per-section coverage report - as any community pack. Both discs are
 * consumed and dropped when this returns, so the caller can re-supply the USA
 * image for the patch run without holding two copies at once.
 *
 * The pack is filled with the game's copyrighted text: it belongs in the
 * user's browser (or their own scratchpad), never in the repo.
 *
 * `fold_accents` (recommended) rewrites the accented glyph cells the NTSC font
 * leaves empty onto plain ASCII - `Epee` for `Épée`. With it off the raw PAL
 * accent bytes are kept, which is byte-faithful but renders blank until the
 * font atlas is patched; either way the count is reported, never silent.
 *
 * Returns `{ yaml, language, exe, summary, tables: [{name, located, pal_base,
 * valid_pct, paired}], names_filled, names_unmapped, party_filled,
 * party_total, man_total, man_paired, raw_total, raw_paired, folded,
 * unfolded }`.
 */
export function lift_official_pack(target_image: Uint8Array, source_image: Uint8Array, fold_accents: boolean): any;

/**
 * Patch a user-supplied disc image with the chosen randomizer settings.
 *
 * `drops` / `encounters` / `chests` / `shops` / `casino` / `steals` / `arts` /
 * `doors` / `house_doors` are each `"shuffle"`, `"random"`, or `"none"`.
 * `arts` reassigns Tactical-Arts button combos (same-length, unique within
 * character; Miracle Arts untouched). `shops`
 * randomizes what town stores sell; `casino` the casino prize exchange. `door_coupling` is `"coupled"`
 * (bidirectional) or `"decoupled"` (one-way). `house_doors` honours only
 * `"shuffle"` and covers both intra-town door classes: the scripted door
 * warps and the `.MAP` kind-0 intra-scene teleports (most house exits),
 * the latter rewired per scene only when walk-component reachability is
 * preserved. `starting_items` is the number of random starting consumables
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
 * menu readers). `jewel_fix` retargets the boss cinematic casts' damage calls
 * from the resist-ladder-bypassing wrapper to the guard-respecting one, so
 * elemental jewels / guards / All Guard apply to Xain's Bloody Horns / Terio
 * Punch, Cort's Guilty Cross, and the Delilas trio's signature moves (a fix,
 * not a randomization - it is seedless).
 * `starting_level`
 * begins the new game at that character level instead of 1 (`0` or `1` =
 * vanilla; range 2..=14), seeding the lead character's XP and recomputing the
 * starting stats from the disc's growth curves. `seed` is a number or
 * any string (hashed).
 *
 * `lang_pack` is an **optional** `legaia-text-pack-v1` YAML document (empty
 * string = no language patch, the default). It is applied **first**, before
 * any randomizer pass, because a translation edit is keyed by a byte offset
 * into a scene's decompressed MAN and the door / starting-bag passes relocate
 * those records - translate-then-randomize composes, the reverse loses the
 * moved scenes' lines. Per-entry skips (a line over budget, a wrong-disc
 * mismatch) are counted in the summary but never abort the patch. Returns
 * `{ data, summary, seed }`.
 */
export function patch_rom(image: Uint8Array, seed: string, lang_pack: string, drops: string, encounters: string, encounter_scope: string, chests: string, shops: string, casino: string, steals: string, arts: string, doors: string, door_coupling: string, house_doors: string, starting_items: number, door_of_wind: number, incense: number, speed_chain: number, chicken_heart: number, good_luck_bell: number, all_warps: boolean, unused_enemies: boolean, unused_items: boolean, equipment_drops: boolean, monster_stats: string, move_power: string, element_affinity: string, spell_cost: string, equip_bonus: string, weapon_specialty: boolean, starting_level: number, solo_strong_encounters: boolean, flee_exp: boolean, seru_trade: boolean, enemy_ally: boolean, shiny_seru: boolean, jewel_fix: boolean): any;

/**
 * Resolve a user seed string to the numeric seed, as a decimal string (so the
 * page can display / persist it without JS `BigInt` precision loss).
 */
export function resolve_seed(seed: string): string;

/**
 * Summarise save bytes of either family (LGSF or an emulator card
 * container) without touching the runtime - what the "your games" strip
 * uses to describe a stored slot. Throws on unrecognised bytes.
 */
export function save_summary_json(bytes: Uint8Array): string;

/**
 * Validate a `legaia-text-pack-v1` YAML document **against the user's own
 * disc**, client-side. Returns `{ ok, language, applied, skipped, message }`:
 * `applied` is how many entries would be written, `skipped` how many the disc
 * rejected (over budget or not matching this image), and `message` a short
 * human summary. This is the same dry run the CLI's `translate stats --input`
 * does - the only way to check a distributable pack's budgets, which are
 * hints until a disc is there to measure. Nothing is written.
 */
export function validate_lang_pack(image: Uint8Array, pack_yaml: string): any;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_legaiaarts_free: (a: number, b: number) => void;
    readonly __wbg_legaiaaudio_free: (a: number, b: number) => void;
    readonly __wbg_legaiaminigames_free: (a: number, b: number) => void;
    readonly __wbg_legaiaruntime_free: (a: number, b: number) => void;
    readonly __wbg_legaiasfx_free: (a: number, b: number) => void;
    readonly __wbg_legaiaviewer_free: (a: number, b: number) => void;
    readonly __wbg_music01render_free: (a: number, b: number) => void;
    readonly card_icon_rgba: (a: number, b: number, c: number) => [number, number, number, number];
    readonly card_patch_coins: (a: number, b: number, c: number, d: number) => [number, number, number, number];
    readonly card_read_coins: (a: number, b: number, c: number) => [number, number, number];
    readonly card_saves_json: (a: number, b: number) => [number, number, number, number];
    readonly disc_portrait_rgba: (a: number, b: number, c: number) => [number, number];
    readonly export_lang_pack: (a: number, b: number, c: number, d: number) => [number, number, number, number];
    readonly legaiaarts_art_pose_frames: (a: number, b: number) => [number, number];
    readonly legaiaarts_art_strike_cue: (a: number) => number;
    readonly legaiaarts_art_strike_frames: (a: number, b: number) => [number, number];
    readonly legaiaarts_art_voice_pcm_i16: (a: number, b: number) => [number, number];
    readonly legaiaarts_export_character_glb: (a: number) => [number, number];
    readonly legaiaarts_idle_pose_frames: (a: number) => [number, number];
    readonly legaiaarts_load_disc: (a: number, b: number, c: number) => [number, number, number, number];
    readonly legaiaarts_mesh_bounds: (a: number) => [number, number];
    readonly legaiaarts_mesh_cba_tsb: (a: number) => [number, number];
    readonly legaiaarts_mesh_indices: (a: number) => [number, number];
    readonly legaiaarts_mesh_object_ids: (a: number) => [number, number];
    readonly legaiaarts_mesh_positions: (a: number) => [number, number];
    readonly legaiaarts_mesh_uvs: (a: number) => [number, number];
    readonly legaiaarts_new: () => number;
    readonly legaiaarts_set_character: (a: number, b: number) => [number, number];
    readonly legaiaarts_voice_channel_pcm_i16: (a: number, b: number) => [number, number];
    readonly legaiaarts_vram_bytes: (a: number) => [number, number];
    readonly legaiaaudio_bgm_device_rate: (a: number) => number;
    readonly legaiaaudio_bgm_render_rate: (a: number) => number;
    readonly legaiaaudio_decode_vab_sample_i16: (a: number, b: number, c: number, d: number) => [number, number];
    readonly legaiaaudio_decode_xa_stream_i16: (a: number, b: number, c: number, d: number) => [number, number];
    readonly legaiaaudio_enumerate_bgm_pairs_json: (a: number) => [number, number];
    readonly legaiaaudio_enumerate_vabs_json: (a: number) => [number, number];
    readonly legaiaaudio_enumerate_xa_files_json: (a: number) => [number, number];
    readonly legaiaaudio_load_disc: (a: number, b: number, c: number) => [number, number, number];
    readonly legaiaaudio_new: () => number;
    readonly legaiaaudio_render_bgm_pcm_i16: (a: number, b: number, c: number, d: number, e: number) => [number, number];
    readonly legaiaaudio_resume_bgm: (a: number) => any;
    readonly legaiaaudio_set_bgm_gain: (a: number, b: number) => void;
    readonly legaiaaudio_set_bgm_paused: (a: number, b: number) => void;
    readonly legaiaaudio_start_bgm: (a: number, b: number, c: number, d: number) => [number, number];
    readonly legaiaaudio_stop_bgm: (a: number) => void;
    readonly legaiaaudio_str_decode_frame: (a: number, b: number) => [number, number];
    readonly legaiaaudio_str_video_close: (a: number) => void;
    readonly legaiaaudio_str_video_open: (a: number, b: number, c: number) => [number, number];
    readonly legaiaaudio_vab_sample_list_json: (a: number, b: number, c: number) => [number, number];
    readonly legaiaaudio_vab_sample_rate: (a: number) => number;
    readonly legaiaaudio_xa_metadata_json: (a: number, b: number, c: number) => [number, number];
    readonly legaiaminigames_baka_anim_dims: (a: number, b: number, c: number, d: number) => [number, number];
    readonly legaiaminigames_baka_anim_pose_frames: (a: number, b: number, c: number, d: number, e: number) => [number, number];
    readonly legaiaminigames_baka_anim_record_count: (a: number, b: number, c: number) => number;
    readonly legaiaminigames_baka_choose: (a: number, b: number) => number;
    readonly legaiaminigames_baka_duel_facing_json: (a: number) => [number, number];
    readonly legaiaminigames_baka_duel_vram: (a: number, b: number) => [number, number];
    readonly legaiaminigames_baka_fighter_cba_tsb: (a: number, b: number, c: number) => [number, number];
    readonly legaiaminigames_baka_fighter_flat_rgba: (a: number, b: number, c: number) => [number, number];
    readonly legaiaminigames_baka_fighter_indices: (a: number, b: number, c: number) => [number, number];
    readonly legaiaminigames_baka_fighter_object_ids: (a: number, b: number, c: number) => [number, number];
    readonly legaiaminigames_baka_fighter_part_count: (a: number, b: number, c: number) => number;
    readonly legaiaminigames_baka_fighter_positions: (a: number, b: number, c: number) => [number, number];
    readonly legaiaminigames_baka_fighter_uvs: (a: number, b: number, c: number) => [number, number];
    readonly legaiaminigames_baka_hud_json: (a: number) => [number, number];
    readonly legaiaminigames_baka_ladder_json: (a: number) => [number, number];
    readonly legaiaminigames_baka_names_json: (a: number) => [number, number];
    readonly legaiaminigames_baka_page_rgba: (a: number, b: number, c: number) => [number, number];
    readonly legaiaminigames_baka_page_width: (a: number, b: number) => number;
    readonly legaiaminigames_baka_presentation_ready: (a: number) => number;
    readonly legaiaminigames_baka_roster_json: (a: number) => [number, number];
    readonly legaiaminigames_baka_run_fight_on: (a: number) => number;
    readonly legaiaminigames_baka_run_match_over: (a: number, b: number) => number;
    readonly legaiaminigames_baka_run_pay_out: (a: number) => number;
    readonly legaiaminigames_baka_run_start: (a: number, b: number) => number;
    readonly legaiaminigames_baka_run_state_json: (a: number) => [number, number];
    readonly legaiaminigames_baka_stage_cba_tsb: (a: number, b: number) => [number, number];
    readonly legaiaminigames_baka_stage_flat_rgba: (a: number, b: number) => [number, number];
    readonly legaiaminigames_baka_stage_indices: (a: number, b: number) => [number, number];
    readonly legaiaminigames_baka_stage_positions: (a: number, b: number) => [number, number];
    readonly legaiaminigames_baka_stage_uvs: (a: number, b: number) => [number, number];
    readonly legaiaminigames_baka_start: (a: number, b: number, c: number) => number;
    readonly legaiaminigames_baka_state_json: (a: number) => [number, number];
    readonly legaiaminigames_baka_tick: (a: number, b: number) => void;
    readonly legaiaminigames_dance_art_ready: (a: number) => number;
    readonly legaiaminigames_dance_bgm_pcm_i16: (a: number, b: number, c: number) => [number, number];
    readonly legaiaminigames_dance_bgm_ready_json: (a: number) => [number, number];
    readonly legaiaminigames_dance_body_anim_dims: (a: number, b: number, c: number) => [number, number];
    readonly legaiaminigames_dance_body_cba_tsb: (a: number, b: number) => [number, number];
    readonly legaiaminigames_dance_body_count: (a: number) => number;
    readonly legaiaminigames_dance_body_flat_rgba: (a: number, b: number) => [number, number];
    readonly legaiaminigames_dance_body_human_index: (a: number) => number;
    readonly legaiaminigames_dance_body_indices: (a: number, b: number) => [number, number];
    readonly legaiaminigames_dance_body_kind: (a: number, b: number) => number;
    readonly legaiaminigames_dance_body_object_ids: (a: number, b: number) => [number, number];
    readonly legaiaminigames_dance_body_part_count: (a: number, b: number) => number;
    readonly legaiaminigames_dance_body_pose_frames: (a: number, b: number, c: number, d: number) => [number, number];
    readonly legaiaminigames_dance_body_positions: (a: number, b: number) => [number, number];
    readonly legaiaminigames_dance_body_ready: (a: number) => number;
    readonly legaiaminigames_dance_body_uvs: (a: number, b: number) => [number, number];
    readonly legaiaminigames_dance_body_vram: (a: number) => [number, number];
    readonly legaiaminigames_dance_cast_json: (a: number) => [number, number];
    readonly legaiaminigames_dance_chart_json: (a: number) => [number, number];
    readonly legaiaminigames_dance_env_cba_tsb: (a: number) => [number, number];
    readonly legaiaminigames_dance_env_flat_rgba: (a: number) => [number, number];
    readonly legaiaminigames_dance_env_indices: (a: number) => [number, number];
    readonly legaiaminigames_dance_env_positions: (a: number) => [number, number];
    readonly legaiaminigames_dance_env_uvs: (a: number) => [number, number];
    readonly legaiaminigames_dance_face_meta_json: (a: number) => [number, number];
    readonly legaiaminigames_dance_face_rgba: (a: number, b: number, c: number) => [number, number];
    readonly legaiaminigames_dance_hud_page_rgba: (a: number, b: number) => [number, number];
    readonly legaiaminigames_dance_jukebox_json: (a: number) => [number, number];
    readonly legaiaminigames_dance_layout_json: (a: number) => [number, number];
    readonly legaiaminigames_dance_press: (a: number, b: number) => [number, number];
    readonly legaiaminigames_dance_sfx_cue_ids: (a: number) => [number, number];
    readonly legaiaminigames_dance_sfx_json: (a: number) => [number, number];
    readonly legaiaminigames_dance_sfx_pcm: (a: number, b: number) => [number, number];
    readonly legaiaminigames_dance_sfx_rate: (a: number, b: number) => number;
    readonly legaiaminigames_dance_start: (a: number, b: number) => number;
    readonly legaiaminigames_dance_state_json: (a: number) => [number, number];
    readonly legaiaminigames_dance_sting_pcm: (a: number, b: number, c: number) => [number, number];
    readonly legaiaminigames_dance_sting_rate: (a: number, b: number, c: number) => number;
    readonly legaiaminigames_dance_tick: (a: number, b: number) => void;
    readonly legaiaminigames_dance_widgets_json: (a: number) => [number, number];
    readonly legaiaminigames_load_disc: (a: number, b: number, c: number) => [number, number, number, number];
    readonly legaiaminigames_minigame_bgm_pcm_i16: (a: number, b: number, c: number, d: number) => [number, number];
    readonly legaiaminigames_minigame_bgm_ready_json: (a: number, b: number, c: number) => [number, number];
    readonly legaiaminigames_music01_bgm_render: (a: number, b: number, c: number) => number;
    readonly legaiaminigames_new: () => number;
    readonly legaiaminigames_save_portrait_rgba: (a: number, b: number) => [number, number];
    readonly legaiaminigames_slot_art_ready: (a: number) => number;
    readonly legaiaminigames_slot_bonus_json: (a: number) => [number, number];
    readonly legaiaminigames_slot_bonus_number_rgba: (a: number, b: number) => [number, number];
    readonly legaiaminigames_slot_collect: (a: number) => number;
    readonly legaiaminigames_slot_digits_rgba: (a: number) => [number, number];
    readonly legaiaminigames_slot_hud_json: (a: number) => [number, number];
    readonly legaiaminigames_slot_hud_rgba: (a: number, b: number) => [number, number];
    readonly legaiaminigames_slot_marquee_json: (a: number) => [number, number];
    readonly legaiaminigames_slot_page_rgba: (a: number, b: number, c: number) => [number, number];
    readonly legaiaminigames_slot_page_width: (a: number, b: number) => number;
    readonly legaiaminigames_slot_panel_rgba: (a: number) => [number, number];
    readonly legaiaminigames_slot_press: (a: number) => [number, number];
    readonly legaiaminigames_slot_reel_pos: (a: number) => [number, number];
    readonly legaiaminigames_slot_scene_json: (a: number) => [number, number];
    readonly legaiaminigames_slot_scene_ready: (a: number) => number;
    readonly legaiaminigames_slot_sfx_cue_ids: (a: number) => [number, number];
    readonly legaiaminigames_slot_sfx_json: (a: number) => [number, number];
    readonly legaiaminigames_slot_sfx_pcm: (a: number, b: number) => [number, number];
    readonly legaiaminigames_slot_sfx_rate: (a: number, b: number) => number;
    readonly legaiaminigames_slot_spin: (a: number) => number;
    readonly legaiaminigames_slot_spin_pcm: (a: number) => [number, number];
    readonly legaiaminigames_slot_spin_rate: (a: number) => number;
    readonly legaiaminigames_slot_start: (a: number, b: number, c: number) => number;
    readonly legaiaminigames_slot_state_json: (a: number) => [number, number];
    readonly legaiaminigames_slot_stop: (a: number) => number;
    readonly legaiaminigames_slot_strip: (a: number, b: number) => [number, number];
    readonly legaiaminigames_slot_symbol_rgba: (a: number, b: number) => [number, number];
    readonly legaiaminigames_slot_tick: (a: number) => number;
    readonly legaiaruntime_audio_init: (a: number) => number;
    readonly legaiaruntime_boot_title_atlas_dims: (a: number) => [number, number];
    readonly legaiaruntime_boot_title_atlas_rgba: (a: number) => [number, number];
    readonly legaiaruntime_boot_title_close: (a: number) => void;
    readonly legaiaruntime_boot_title_draws_json: (a: number, b: number, c: number) => [number, number];
    readonly legaiaruntime_boot_title_glyph_atlas_dims: (a: number) => [number, number];
    readonly legaiaruntime_boot_title_glyph_atlas_rgba: (a: number) => [number, number];
    readonly legaiaruntime_boot_title_has_atlas: (a: number) => number;
    readonly legaiaruntime_boot_title_is_active: (a: number) => number;
    readonly legaiaruntime_boot_title_start: (a: number) => void;
    readonly legaiaruntime_boot_title_step: (a: number, b: number) => [number, number];
    readonly legaiaruntime_card_slot_dirty: (a: number, b: number) => number;
    readonly legaiaruntime_card_slots_json: (a: number) => [number, number];
    readonly legaiaruntime_cutscene_caption_dims: (a: number) => [number, number];
    readonly legaiaruntime_cutscene_caption_rgba: (a: number) => [number, number];
    readonly legaiaruntime_disc_loaded: (a: number) => number;
    readonly legaiaruntime_eject_card: (a: number, b: number) => void;
    readonly legaiaruntime_enter_field: (a: number, b: number, c: number) => [number, number, number, number];
    readonly legaiaruntime_export_card: (a: number, b: number) => [number, number];
    readonly legaiaruntime_export_save: (a: number) => [number, number];
    readonly legaiaruntime_field_ground_cba_tsb: (a: number) => [number, number];
    readonly legaiaruntime_field_ground_indices: (a: number) => [number, number];
    readonly legaiaruntime_field_ground_positions: (a: number) => [number, number];
    readonly legaiaruntime_field_ground_quad_count: (a: number) => number;
    readonly legaiaruntime_field_ground_uvs: (a: number) => [number, number];
    readonly legaiaruntime_field_menu_model_json: (a: number) => [number, number];
    readonly legaiaruntime_field_mesh: (a: number, b: number) => [number, number, number];
    readonly legaiaruntime_field_mesh_cba_tsb: (a: number) => [number, number];
    readonly legaiaruntime_field_mesh_flat_rgba: (a: number) => [number, number];
    readonly legaiaruntime_field_mesh_indices: (a: number) => [number, number];
    readonly legaiaruntime_field_mesh_posed: (a: number, b: number, c: number) => [number, number, number];
    readonly legaiaruntime_field_mesh_posed_frame_positions: (a: number, b: number, c: number, d: number) => [number, number];
    readonly legaiaruntime_field_mesh_positions: (a: number) => [number, number];
    readonly legaiaruntime_field_mesh_uvs: (a: number) => [number, number];
    readonly legaiaruntime_field_offmap_hide_xz: (a: number) => number;
    readonly legaiaruntime_field_placement_anim_ids: (a: number) => [number, number];
    readonly legaiaruntime_field_placement_frames: (a: number) => [number, number];
    readonly legaiaruntime_field_placement_positions: (a: number) => [number, number];
    readonly legaiaruntime_field_placement_rot_y: (a: number) => [number, number];
    readonly legaiaruntime_field_placement_slots: (a: number) => [number, number];
    readonly legaiaruntime_field_status_json: (a: number) => [number, number];
    readonly legaiaruntime_field_terrain_positions: (a: number) => [number, number];
    readonly legaiaruntime_field_terrain_rot_y: (a: number) => [number, number];
    readonly legaiaruntime_field_terrain_slots: (a: number) => [number, number];
    readonly legaiaruntime_field_vram_bytes: (a: number) => [number, number];
    readonly legaiaruntime_frame: (a: number) => bigint;
    readonly legaiaruntime_import_card_save: (a: number, b: number, c: number, d: number) => [number, number, number, number];
    readonly legaiaruntime_import_save: (a: number, b: number, c: number) => [number, number, number, number];
    readonly legaiaruntime_insert_card: (a: number, b: number, c: number, d: number, e: number, f: number) => [number, number, number, number];
    readonly legaiaruntime_load_disc: (a: number, b: number, c: number, d: number, e: number) => [number, number, number];
    readonly legaiaruntime_menu_is_open: (a: number) => number;
    readonly legaiaruntime_menu_label: (a: number) => [number, number];
    readonly legaiaruntime_menu_tick: (a: number, b: number) => any;
    readonly legaiaruntime_name_entry_draws_json: (a: number, b: number, c: number) => [number, number];
    readonly legaiaruntime_name_entry_input: (a: number, b: number) => number;
    readonly legaiaruntime_name_entry_is_active: (a: number) => number;
    readonly legaiaruntime_name_entry_state_json: (a: number) => [number, number];
    readonly legaiaruntime_new: () => number;
    readonly legaiaruntime_open_menu: (a: number) => void;
    readonly legaiaruntime_party_display_name: (a: number, b: number) => [number, number];
    readonly legaiaruntime_play_cutscene_camera_json: (a: number) => [number, number];
    readonly legaiaruntime_play_cutscene_state_json: (a: number) => [number, number];
    readonly legaiaruntime_play_cutscene_text_draws_json: (a: number, b: number, c: number) => [number, number];
    readonly legaiaruntime_play_dialog_draws_json: (a: number, b: number, c: number) => [number, number];
    readonly legaiaruntime_play_menu_chrome_dims: (a: number) => [number, number];
    readonly legaiaruntime_play_menu_chrome_rgba: (a: number) => [number, number];
    readonly legaiaruntime_play_menu_close: (a: number) => void;
    readonly legaiaruntime_play_menu_draws_json: (a: number, b: number, c: number) => [number, number];
    readonly legaiaruntime_play_menu_font_dims: (a: number) => [number, number];
    readonly legaiaruntime_play_menu_font_rgba: (a: number) => [number, number];
    readonly legaiaruntime_play_menu_has_chrome: (a: number) => number;
    readonly legaiaruntime_play_menu_input: (a: number, b: number) => void;
    readonly legaiaruntime_play_menu_is_open: (a: number) => number;
    readonly legaiaruntime_play_menu_open: (a: number) => void;
    readonly legaiaruntime_play_menu_take_load_scene: (a: number) => [number, number];
    readonly legaiaruntime_play_npc_catalog_json: (a: number) => [number, number];
    readonly legaiaruntime_play_npc_clip_states: (a: number) => [number, number];
    readonly legaiaruntime_play_npc_live_bones: (a: number, b: number) => [number, number];
    readonly legaiaruntime_play_npc_mesh: (a: number, b: number) => [number, number, number];
    readonly legaiaruntime_play_npc_mesh_cba_tsb: (a: number) => [number, number];
    readonly legaiaruntime_play_npc_mesh_flat_rgba: (a: number) => [number, number];
    readonly legaiaruntime_play_npc_mesh_indices: (a: number) => [number, number];
    readonly legaiaruntime_play_npc_mesh_object_ids: (a: number) => [number, number];
    readonly legaiaruntime_play_npc_mesh_positions: (a: number) => [number, number];
    readonly legaiaruntime_play_npc_mesh_uvs: (a: number) => [number, number];
    readonly legaiaruntime_play_npc_pose_dims: (a: number, b: number) => [number, number];
    readonly legaiaruntime_play_npc_pose_frames: (a: number, b: number) => [number, number];
    readonly legaiaruntime_play_npc_transforms: (a: number) => [number, number];
    readonly legaiaruntime_play_overlay_draws_json: (a: number, b: number, c: number) => [number, number];
    readonly legaiaruntime_play_shop_input: (a: number, b: number) => void;
    readonly legaiaruntime_play_shop_is_open: (a: number) => number;
    readonly legaiaruntime_play_take_prologue_handoff: (a: number, b: number) => [number, number];
    readonly legaiaruntime_player_has_mesh: (a: number) => number;
    readonly legaiaruntime_player_mesh_cba_tsb: (a: number) => [number, number];
    readonly legaiaruntime_player_mesh_flat_rgba: (a: number) => [number, number];
    readonly legaiaruntime_player_mesh_indices: (a: number) => [number, number];
    readonly legaiaruntime_player_mesh_positions: (a: number) => [number, number];
    readonly legaiaruntime_player_mesh_uvs: (a: number) => [number, number];
    readonly legaiaruntime_player_transform: (a: number) => [number, number];
    readonly legaiaruntime_scene_mode: (a: number) => [number, number];
    readonly legaiaruntime_set_camera_azimuth: (a: number, b: number) => void;
    readonly legaiaruntime_set_left_stick: (a: number, b: number, c: number) => void;
    readonly legaiaruntime_set_pad: (a: number, b: number) => void;
    readonly legaiaruntime_set_precise_movement: (a: number, b: number) => void;
    readonly legaiaruntime_state_json: (a: number) => [number, number];
    readonly legaiaruntime_tick_frame: (a: number) => [number, number, number, number];
    readonly legaiasfx_art_cues_json: (a: number) => [number, number];
    readonly legaiasfx_baka_cues_json: (a: number) => [number, number];
    readonly legaiasfx_bank_prot_index: (a: number) => number;
    readonly legaiasfx_cue_for_event: (a: number, b: number, c: number, d: number, e: number) => number;
    readonly legaiasfx_cue_ids: (a: number) => [number, number];
    readonly legaiasfx_cue_pcm_i16: (a: number, b: number) => [number, number];
    readonly legaiasfx_cue_peak: (a: number, b: number) => number;
    readonly legaiasfx_load_disc: (a: number, b: number, c: number) => [number, number, number, number];
    readonly legaiasfx_new: () => number;
    readonly legaiaviewer_battle_char_atlas_bytes: (a: number, b: number) => [number, number];
    readonly legaiaviewer_battle_char_mesh_bounds: (a: number, b: number) => [number, number];
    readonly legaiaviewer_battle_char_mesh_cba_tsb: (a: number, b: number) => [number, number];
    readonly legaiaviewer_battle_char_mesh_indices: (a: number, b: number) => [number, number];
    readonly legaiaviewer_battle_char_mesh_normals: (a: number, b: number) => [number, number];
    readonly legaiaviewer_battle_char_mesh_object_ids: (a: number, b: number) => [number, number];
    readonly legaiaviewer_battle_char_mesh_positions: (a: number, b: number) => [number, number];
    readonly legaiaviewer_battle_char_mesh_uvs: (a: number, b: number) => [number, number];
    readonly legaiaviewer_battle_char_pack_json: (a: number) => [number, number];
    readonly legaiaviewer_battle_char_tmd_bytes: (a: number, b: number) => [number, number];
    readonly legaiaviewer_battle_char_vram_bytes: (a: number) => [number, number];
    readonly legaiaviewer_battle_char_vram_bytes_battle: (a: number) => [number, number];
    readonly legaiaviewer_catalog_clut_count: (a: number, b: number) => number;
    readonly legaiaviewer_catalog_info_json: (a: number, b: number) => [number, number];
    readonly legaiaviewer_catalog_len: (a: number) => number;
    readonly legaiaviewer_character_mesh_bounds: (a: number, b: number, c: number) => [number, number];
    readonly legaiaviewer_character_mesh_cba_tsb: (a: number, b: number, c: number) => [number, number];
    readonly legaiaviewer_character_mesh_flat_colors: (a: number, b: number, c: number) => [number, number];
    readonly legaiaviewer_character_mesh_indices: (a: number, b: number, c: number) => [number, number];
    readonly legaiaviewer_character_mesh_normals: (a: number, b: number, c: number) => [number, number];
    readonly legaiaviewer_character_mesh_object_ids: (a: number, b: number, c: number) => [number, number];
    readonly legaiaviewer_character_mesh_positions: (a: number, b: number, c: number) => [number, number];
    readonly legaiaviewer_character_mesh_uvs: (a: number, b: number, c: number) => [number, number];
    readonly legaiaviewer_character_pack_json: (a: number) => [number, number];
    readonly legaiaviewer_character_tmd_bytes: (a: number, b: number) => [number, number];
    readonly legaiaviewer_continent_pack_count: (a: number) => number;
    readonly legaiaviewer_continent_pack_mesh: (a: number, b: number) => [number, number, number];
    readonly legaiaviewer_continent_pack_mesh_bounds: (a: number) => [number, number];
    readonly legaiaviewer_continent_pack_mesh_cba_tsb: (a: number) => [number, number];
    readonly legaiaviewer_continent_pack_mesh_indices: (a: number) => [number, number];
    readonly legaiaviewer_continent_pack_mesh_positions: (a: number) => [number, number];
    readonly legaiaviewer_continent_pack_mesh_uvs: (a: number) => [number, number];
    readonly legaiaviewer_continent_pack_vram_bytes: (a: number) => [number, number];
    readonly legaiaviewer_continent_prot_index: (a: number) => number;
    readonly legaiaviewer_current_entry_info_json: (a: number) => [number, number];
    readonly legaiaviewer_current_has_tmd: (a: number) => number;
    readonly legaiaviewer_current_index: (a: number) => number;
    readonly legaiaviewer_current_mes_message_hex: (a: number, b: number) => [number, number];
    readonly legaiaviewer_current_vram_bytes: (a: number) => [number, number];
    readonly legaiaviewer_deep_catalog_clut_count: (a: number, b: number) => number;
    readonly legaiaviewer_deep_catalog_info_json: (a: number, b: number) => [number, number];
    readonly legaiaviewer_deep_catalog_len: (a: number) => number;
    readonly legaiaviewer_entry_count: (a: number) => number;
    readonly legaiaviewer_entry_list_json: (a: number) => [number, number];
    readonly legaiaviewer_field_char_vram_bytes: (a: number) => [number, number];
    readonly legaiaviewer_field_npc_catalog_json: (a: number) => [number, number];
    readonly legaiaviewer_field_npc_mesh: (a: number, b: number) => [number, number, number];
    readonly legaiaviewer_field_npc_mesh_bounds: (a: number) => [number, number];
    readonly legaiaviewer_field_npc_mesh_cba_tsb: (a: number) => [number, number];
    readonly legaiaviewer_field_npc_mesh_flat_rgba: (a: number) => [number, number];
    readonly legaiaviewer_field_npc_mesh_indices: (a: number) => [number, number];
    readonly legaiaviewer_field_npc_mesh_object_ids: (a: number) => [number, number];
    readonly legaiaviewer_field_npc_mesh_positions: (a: number) => [number, number];
    readonly legaiaviewer_field_npc_mesh_uvs: (a: number) => [number, number];
    readonly legaiaviewer_field_scene_ground_cba_tsb: (a: number) => [number, number];
    readonly legaiaviewer_field_scene_ground_indices: (a: number) => [number, number];
    readonly legaiaviewer_field_scene_ground_positions: (a: number) => [number, number];
    readonly legaiaviewer_field_scene_ground_quad_count: (a: number) => number;
    readonly legaiaviewer_field_scene_ground_uvs: (a: number) => [number, number];
    readonly legaiaviewer_field_scene_mesh: (a: number, b: number) => [number, number, number];
    readonly legaiaviewer_field_scene_mesh_cba_tsb: (a: number) => [number, number];
    readonly legaiaviewer_field_scene_mesh_flat_rgba: (a: number) => [number, number];
    readonly legaiaviewer_field_scene_mesh_indices: (a: number) => [number, number];
    readonly legaiaviewer_field_scene_mesh_positions: (a: number) => [number, number];
    readonly legaiaviewer_field_scene_mesh_uvs: (a: number) => [number, number];
    readonly legaiaviewer_field_scene_pack_count: (a: number) => number;
    readonly legaiaviewer_field_scene_placement_positions: (a: number) => [number, number];
    readonly legaiaviewer_field_scene_placement_rot_y: (a: number) => [number, number];
    readonly legaiaviewer_field_scene_placement_slots: (a: number) => [number, number];
    readonly legaiaviewer_field_scene_status_json: (a: number) => [number, number];
    readonly legaiaviewer_field_scene_terrain_positions: (a: number) => [number, number];
    readonly legaiaviewer_field_scene_terrain_rot_y: (a: number) => [number, number];
    readonly legaiaviewer_field_scene_terrain_slots: (a: number) => [number, number];
    readonly legaiaviewer_field_scene_vram_bytes: (a: number) => [number, number];
    readonly legaiaviewer_fog_lut_bytes: (a: number) => [number, number];
    readonly legaiaviewer_init_pak_logo_rgba: (a: number, b: number) => [number, number];
    readonly legaiaviewer_init_pak_logos_json: (a: number) => [number, number];
    readonly legaiaviewer_load_disc: (a: number, b: number, c: number) => [number, number, number];
    readonly legaiaviewer_locate_offset_json: (a: number, b: number, c: number, d: number) => [number, number];
    readonly legaiaviewer_mesh_bounds: (a: number) => [number, number];
    readonly legaiaviewer_mesh_cba_tsb: (a: number) => [number, number];
    readonly legaiaviewer_mesh_indices: (a: number) => [number, number];
    readonly legaiaviewer_mesh_positions: (a: number) => [number, number];
    readonly legaiaviewer_mesh_uvs: (a: number) => [number, number];
    readonly legaiaviewer_monster_animation_frames_at: (a: number, b: number, c: number) => [number, number];
    readonly legaiaviewer_monster_animations_json: (a: number, b: number) => [number, number];
    readonly legaiaviewer_monster_archive_json: (a: number) => [number, number];
    readonly legaiaviewer_monster_glb: (a: number, b: number) => [number, number];
    readonly legaiaviewer_monster_idle_animation_frames: (a: number, b: number) => [number, number];
    readonly legaiaviewer_monster_idle_animation_header: (a: number, b: number) => [number, number];
    readonly legaiaviewer_monster_mesh_bounds: (a: number, b: number) => [number, number];
    readonly legaiaviewer_monster_mesh_indices: (a: number, b: number) => [number, number];
    readonly legaiaviewer_monster_mesh_normals: (a: number, b: number) => [number, number];
    readonly legaiaviewer_monster_mesh_object_ids: (a: number, b: number) => [number, number];
    readonly legaiaviewer_monster_mesh_palette_index: (a: number, b: number) => [number, number];
    readonly legaiaviewer_monster_mesh_positions: (a: number, b: number) => [number, number];
    readonly legaiaviewer_monster_mesh_uvs: (a: number, b: number) => [number, number];
    readonly legaiaviewer_monster_texture_dims: (a: number, b: number) => [number, number];
    readonly legaiaviewer_monster_texture_indices: (a: number, b: number) => [number, number];
    readonly legaiaviewer_monster_texture_palette_rgba: (a: number, b: number) => [number, number];
    readonly legaiaviewer_new: (a: number, b: number) => [number, number, number];
    readonly legaiaviewer_next_entry: (a: number) => [number, number, number];
    readonly legaiaviewer_ocean_animation_frames: (a: number) => [number, number];
    readonly legaiaviewer_ocean_base_clut_bytes: (a: number) => [number, number];
    readonly legaiaviewer_ocean_frame_count: (a: number) => number;
    readonly legaiaviewer_ocean_texture_bytes: (a: number) => [number, number];
    readonly legaiaviewer_pack_count: (a: number) => number;
    readonly legaiaviewer_pack_mesh: (a: number, b: number) => [number, number, number];
    readonly legaiaviewer_pack_mesh_bounds: (a: number) => [number, number];
    readonly legaiaviewer_pack_mesh_cba_tsb: (a: number) => [number, number];
    readonly legaiaviewer_pack_mesh_indices: (a: number) => [number, number];
    readonly legaiaviewer_pack_mesh_positions: (a: number) => [number, number];
    readonly legaiaviewer_pack_mesh_uvs: (a: number) => [number, number];
    readonly legaiaviewer_pack_vram_bytes: (a: number) => [number, number];
    readonly legaiaviewer_player_anm_corpus_json: (a: number) => [number, number];
    readonly legaiaviewer_player_anm_decoded: (a: number, b: number) => [number, number];
    readonly legaiaviewer_player_anm_record_bytes: (a: number, b: number, c: number) => [number, number];
    readonly legaiaviewer_player_anm_record_dims: (a: number, b: number, c: number) => [number, number];
    readonly legaiaviewer_player_anm_record_frames: (a: number, b: number, c: number) => [number, number];
    readonly legaiaviewer_player_anm_record_header: (a: number, b: number, c: number) => [number, number];
    readonly legaiaviewer_player_anm_record_pose_frames: (a: number, b: number, c: number, d: number) => [number, number];
    readonly legaiaviewer_prev_entry: (a: number) => [number, number, number];
    readonly legaiaviewer_prot_over_read_json: (a: number) => [number, number];
    readonly legaiaviewer_render_catalog_tim: (a: number, b: number, c: number, d: number, e: number) => [number, number];
    readonly legaiaviewer_render_deep_catalog_tim: (a: number, b: number, c: number, d: number, e: number) => [number, number];
    readonly legaiaviewer_render_tmd_triangles: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number) => [number, number];
    readonly legaiaviewer_save_state_framebuffer: (a: number, b: number, c: number) => [number, number, number, number];
    readonly legaiaviewer_save_state_prim_replay: (a: number, b: number, c: number) => [number, number, number, number];
    readonly legaiaviewer_scene_export_add_instance: (a: number, b: number, c: number, d: number, e: number, f: number, g: number) => void;
    readonly legaiaviewer_scene_export_add_mesh: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number, i: number, j: number, k: number, l: number, m: number) => number;
    readonly legaiaviewer_scene_export_begin: (a: number, b: number, c: number) => void;
    readonly legaiaviewer_scene_export_finish: (a: number) => [number, number];
    readonly legaiaviewer_scene_export_set_vram: (a: number, b: number, c: number) => void;
    readonly legaiaviewer_set_clut: (a: number, b: number) => [number, number];
    readonly legaiaviewer_set_scene_field: (a: number, b: number, c: number) => [number, number, number];
    readonly legaiaviewer_set_scene_kingdom: (a: number, b: number) => [number, number, number];
    readonly legaiaviewer_set_scene_npcs: (a: number, b: number, c: number) => [number, number, number];
    readonly legaiaviewer_set_slot: (a: number, b: number) => [number, number, number];
    readonly legaiaviewer_slot4_body_inventory_json: (a: number, b: number) => [number, number];
    readonly legaiaviewer_slot4_wireframe_bounds: (a: number, b: number, c: number, d: number) => [number, number];
    readonly legaiaviewer_slot4_wireframe_lines: (a: number, b: number, c: number, d: number, e: number, f: number) => [number, number];
    readonly legaiaviewer_slot4_wireframe_points: (a: number, b: number, c: number, d: number) => [number, number];
    readonly legaiaviewer_status: (a: number) => [number, number];
    readonly legaiaviewer_walk_ground_cba_tsb: (a: number) => [number, number];
    readonly legaiaviewer_walk_ground_indices: (a: number) => [number, number];
    readonly legaiaviewer_walk_ground_positions: (a: number) => [number, number];
    readonly legaiaviewer_walk_ground_quad_count: (a: number) => number;
    readonly legaiaviewer_walk_ground_uvs: (a: number) => [number, number];
    readonly legaiaviewer_walk_placement_count: (a: number) => number;
    readonly legaiaviewer_walk_placement_positions: (a: number) => [number, number];
    readonly legaiaviewer_walk_placement_rot_y: (a: number) => [number, number];
    readonly legaiaviewer_walk_placement_slots: (a: number) => [number, number];
    readonly legaiaviewer_worldmap_menu_json: (a: number) => [number, number];
    readonly lift_official_pack: (a: number, b: number, c: number, d: number, e: number) => [number, number, number];
    readonly music01render_loop_end: (a: number) => number;
    readonly music01render_ok: (a: number) => number;
    readonly music01render_pcm: (a: number) => [number, number];
    readonly music01render_rate: (a: number) => number;
    readonly patch_rom: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number, i: number, j: number, k: number, l: number, m: number, n: number, o: number, p: number, q: number, r: number, s: number, t: number, u: number, v: number, w: number, x: number, y: number, z: number, a1: number, b1: number, c1: number, d1: number, e1: number, f1: number, g1: number, h1: number, i1: number, j1: number, k1: number, l1: number, m1: number, n1: number, o1: number, p1: number, q1: number, r1: number, s1: number, t1: number, u1: number, v1: number, w1: number, x1: number, y1: number, z1: number, a2: number, b2: number, c2: number, d2: number) => [number, number, number];
    readonly resolve_seed: (a: number, b: number) => [number, number];
    readonly save_summary_json: (a: number, b: number) => [number, number, number, number];
    readonly validate_lang_pack: (a: number, b: number, c: number, d: number) => [number, number, number];
    readonly music01render_loop_start: (a: number) => number;
    readonly legaiaminigames_dance_bgm_rate: (a: number) => number;
    readonly legaiaminigames_minigame_bgm_rate: (a: number) => number;
    readonly legaiasfx_sample_rate: (a: number) => number;
    readonly wasm_bindgen__convert__closures_____invoke__hc20c1a455dcd1273: (a: number, b: number, c: any) => void;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_exn_store: (a: number) => void;
    readonly __externref_table_alloc: () => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __wbindgen_destroy_closure: (a: number, b: number) => void;
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
