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
    set_clut(idx: number): void;
    /**
     * Jump to the slot in the filtered list (NOT the PROT index). Used by
     * the dropdown / list-click UI.
     */
    set_slot(slot: number): number;
    /**
     * JSON status string: PROT index, class name, dims, current slot.
     */
    status(): string;
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
    readonly legaiaviewer_current_entry_info_json: (a: number) => [number, number];
    readonly legaiaviewer_current_has_tmd: (a: number) => number;
    readonly legaiaviewer_current_index: (a: number) => number;
    readonly legaiaviewer_current_mes_message_hex: (a: number, b: number) => [number, number];
    readonly legaiaviewer_current_vram_bytes: (a: number) => [number, number];
    readonly legaiaviewer_entry_count: (a: number) => number;
    readonly legaiaviewer_entry_list_json: (a: number) => [number, number];
    readonly legaiaviewer_load_disc: (a: number, b: number, c: number) => [number, number, number];
    readonly legaiaviewer_mesh_bounds: (a: number) => [number, number];
    readonly legaiaviewer_mesh_cba_tsb: (a: number) => [number, number];
    readonly legaiaviewer_mesh_indices: (a: number) => [number, number];
    readonly legaiaviewer_mesh_positions: (a: number) => [number, number];
    readonly legaiaviewer_mesh_uvs: (a: number) => [number, number];
    readonly legaiaviewer_new: (a: number, b: number) => [number, number, number];
    readonly legaiaviewer_next_entry: (a: number) => [number, number, number];
    readonly legaiaviewer_prev_entry: (a: number) => [number, number, number];
    readonly legaiaviewer_render_tmd_triangles: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number) => [number, number];
    readonly legaiaviewer_set_clut: (a: number, b: number) => [number, number];
    readonly legaiaviewer_set_slot: (a: number, b: number) => [number, number, number];
    readonly legaiaviewer_status: (a: number) => [number, number];
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
