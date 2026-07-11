/* tslint:disable */
/* eslint-disable */

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
 * Bridge object the JS shim instantiates once at page load. Holds a
 * `World` + a `MenuRuntime` for the headless path, and an optional
 * `SceneHost` once `load_disc` has been called.
 */
export class LegaiaRuntime {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Number of currently active actors.
     */
    active_actor_count(): number;
    /**
     * Attempt to initialise the WebAudio backend. Must be called from a
     * user-gesture handler (browser autoplay policy). Returns `true` if
     * audio started successfully, `false` otherwise (e.g. blocked by the
     * browser before any interaction or on a platform without WebAudio).
     *
     * Idempotent - calling a second time replaces the existing backend.
     */
    audio_init(): boolean;
    /**
     * `true` if a disc has been loaded via `load_disc`.
     */
    disc_loaded(): boolean;
    /**
     * Boot a named scene (CDNAME label, e.g. `"town01"`). Requires
     * `load_disc` to have been called first. Loads the scene's assets,
     * enters `SceneMode::Field`, and seeds the field-VM with record 0 of
     * the scene's event-script pack. Throws a JS error if the disc hasn't
     * been loaded or the scene name is unknown.
     */
    enter_scene(name: string): void;
    /**
     * Frame counter.
     */
    frame(): bigint;
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
     */
    load_disc(raw_bytes: Uint8Array, cdname_text: string): number;
    /**
     * Boolean: true if the menu is open.
     */
    menu_is_open(): boolean;
    /**
     * Read the menu's current label (e.g. "STATUS", "SAVE - PICK SLOT")
     * for HUD rendering.
     */
    menu_label(): string;
    /**
     * Tick the menu state machine with a packed PSX-pad button mask.
     * The mask matches `legaia_engine_vm::menu::MenuInput` field order:
     * `cross | (circle<<1) | (triangle<<2) | (square<<3) | (up<<4) | (down<<5) | (left<<6) | (right<<7)`.
     */
    menu_tick(button_mask: number): any;
    constructor();
    /**
     * Open the menu (sets MenuCtx state to Idle).
     */
    open_menu(): void;
    /**
     * Read the active scene mode as a stable enum string.
     */
    scene_mode(): string;
    /**
     * Tick the world once. Returns the current frame counter.
     */
    tick(): bigint;
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
 */
export function patch_rom(image: Uint8Array, seed: string, drops: string, encounters: string, encounter_scope: string, chests: string, shops: string, casino: string, steals: string, arts: string, doors: string, door_coupling: string, house_doors: string, starting_items: number, door_of_wind: number, incense: number, speed_chain: number, chicken_heart: number, good_luck_bell: number, all_warps: boolean, unused_enemies: boolean, unused_items: boolean, equipment_drops: boolean, monster_stats: string, move_power: string, element_affinity: string, spell_cost: string, equip_bonus: string, weapon_specialty: boolean, starting_level: number, solo_strong_encounters: boolean, flee_exp: boolean, seru_trade: boolean, enemy_ally: boolean, shiny_seru: boolean): any;

/**
 * Resolve a user seed string to the numeric seed, as a decimal string (so the
 * page can display / persist it without JS `BigInt` precision loss).
 */
export function resolve_seed(seed: string): string;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_legaiaaudio_free: (a: number, b: number) => void;
    readonly __wbg_legaiaruntime_free: (a: number, b: number) => void;
    readonly __wbg_legaiaviewer_free: (a: number, b: number) => void;
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
    readonly legaiaruntime_active_actor_count: (a: number) => number;
    readonly legaiaruntime_audio_init: (a: number) => number;
    readonly legaiaruntime_disc_loaded: (a: number) => number;
    readonly legaiaruntime_enter_scene: (a: number, b: number, c: number) => [number, number];
    readonly legaiaruntime_frame: (a: number) => bigint;
    readonly legaiaruntime_load_disc: (a: number, b: number, c: number, d: number, e: number) => [number, number, number];
    readonly legaiaruntime_menu_is_open: (a: number) => number;
    readonly legaiaruntime_menu_label: (a: number) => [number, number];
    readonly legaiaruntime_menu_tick: (a: number, b: number) => any;
    readonly legaiaruntime_new: () => number;
    readonly legaiaruntime_open_menu: (a: number) => void;
    readonly legaiaruntime_scene_mode: (a: number) => [number, number];
    readonly legaiaruntime_tick: (a: number) => bigint;
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
    readonly patch_rom: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number, i: number, j: number, k: number, l: number, m: number, n: number, o: number, p: number, q: number, r: number, s: number, t: number, u: number, v: number, w: number, x: number, y: number, z: number, a1: number, b1: number, c1: number, d1: number, e1: number, f1: number, g1: number, h1: number, i1: number, j1: number, k1: number, l1: number, m1: number, n1: number, o1: number, p1: number, q1: number, r1: number, s1: number, t1: number, u1: number, v1: number, w1: number, x1: number, y1: number, z1: number, a2: number) => [number, number, number];
    readonly resolve_seed: (a: number, b: number) => [number, number];
    readonly wasm_bindgen__convert__closures_____invoke__h68646c9fea2fce23: (a: number, b: number, c: any) => void;
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
