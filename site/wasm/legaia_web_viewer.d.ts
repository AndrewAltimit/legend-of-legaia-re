/* tslint:disable */
/* eslint-disable */

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
    entry_count(): number;
    /**
     * Returns a JSON array describing every viewable entry: PROT index, class,
     * dimensions, has-TMD flag. The UI uses this to populate a sidebar list / search.
     */
    entry_list_json(): string;
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
    prev_entry(): number;
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
    set_clut(idx: number): void;
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

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_legaiaruntime_free: (a: number, b: number) => void;
    readonly __wbg_legaiaviewer_free: (a: number, b: number) => void;
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
    readonly legaiaviewer_entry_count: (a: number) => number;
    readonly legaiaviewer_entry_list_json: (a: number) => [number, number];
    readonly legaiaviewer_fog_lut_bytes: (a: number) => [number, number];
    readonly legaiaviewer_load_disc: (a: number, b: number, c: number) => [number, number, number];
    readonly legaiaviewer_mesh_bounds: (a: number) => [number, number];
    readonly legaiaviewer_mesh_cba_tsb: (a: number) => [number, number];
    readonly legaiaviewer_mesh_indices: (a: number) => [number, number];
    readonly legaiaviewer_mesh_positions: (a: number) => [number, number];
    readonly legaiaviewer_mesh_uvs: (a: number) => [number, number];
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
    readonly legaiaviewer_prev_entry: (a: number) => [number, number, number];
    readonly legaiaviewer_render_tmd_triangles: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number) => [number, number];
    readonly legaiaviewer_save_state_framebuffer: (a: number, b: number, c: number) => [number, number, number, number];
    readonly legaiaviewer_save_state_prim_replay: (a: number, b: number, c: number) => [number, number, number, number];
    readonly legaiaviewer_set_clut: (a: number, b: number) => [number, number];
    readonly legaiaviewer_set_scene_kingdom: (a: number, b: number) => [number, number, number];
    readonly legaiaviewer_set_slot: (a: number, b: number) => [number, number, number];
    readonly legaiaviewer_slot4_body_inventory_json: (a: number, b: number) => [number, number];
    readonly legaiaviewer_slot4_wireframe_bounds: (a: number, b: number, c: number, d: number) => [number, number];
    readonly legaiaviewer_slot4_wireframe_lines: (a: number, b: number, c: number, d: number, e: number, f: number) => [number, number];
    readonly legaiaviewer_slot4_wireframe_points: (a: number, b: number, c: number, d: number) => [number, number];
    readonly legaiaviewer_status: (a: number) => [number, number];
    readonly legaiaviewer_worldmap_menu_json: (a: number) => [number, number];
    readonly wasm_bindgen__convert__closures_____invoke__h90bbf554010c78df: (a: number, b: number, c: any) => void;
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
