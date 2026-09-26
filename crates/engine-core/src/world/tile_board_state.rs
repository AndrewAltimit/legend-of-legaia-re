//! Tile-board grid-mode state (the op-0x49 puzzle board, not town locomotion).
//!
//! Split out of the composite [`World`] so the state one subsystem owns
//! reads as one unit. Fields keep their retail provenance notes.

/// Tile-board grid-mode state (the op-0x49 puzzle board, not town locomotion).
pub struct TileBoardState {
    /// Tile-board grid (puzzle / board minigame mode; movement +
    /// collision). `Some` when a field scene has installed a tile board
    /// (retail field-VM op `0x49`). Drives discrete cell-to-cell player
    /// movement in the `SceneMode::Field` tick. This is *not* general
    /// town locomotion (Legaia towns use free movement). See
    /// [`crate::tile_board`].
    pub grid: Option<crate::tile_board::TileBoard>,
    /// While stepping on a tile board, the world `(x, z)` the player
    /// actor is interpolating toward (the destination tile centre).
    /// `None` when the player is idle and ready to accept a new
    /// direction. Mirrors the walk SM's "interpolate to target" state
    /// (`overlay_0897_801ef2b0` case 2).
    pub target: Option<(i32, i32)>,
    /// `true` while a field-VM op-0x49 sub-5 board install holds the
    /// script suspended (the engine face of retail's `_DAT_8007b450`
    /// arm for the board consumer). The op reads `Armed` while
    /// [`crate::world::TileBoardState::grid`] is installed and `Done` once the board
    /// exits (an event cell landing), then clears this on resume.
    pub armed: bool,
    /// The parsed op-49 board header (radius / mode flag / actor
    /// template ids) kept for the render + event consumers while the
    /// board is installed.
    pub header: Option<crate::tile_board::TileBoardHeader>,
    /// Per-cell-value tile-actor table (retail `DAT_801f35bc`, 15 entries).
    /// Index = cell value: slot `0` = the player actor, `2..=14` = the
    /// per-value tile actors spawned at board install from
    /// `tile_template_base + (value - 2)`. Each entry is the actor-pool
    /// slot holding that value's instance, or `None` when the value is not
    /// present on the board / the pool was exhausted. Cleared on teardown.
    pub actor_slots: [Option<u8>; crate::tile_board::TILE_ACTOR_TABLE_LEN],
    /// Per-frame tile-board draw list: one entry per drawn cell, naming the
    /// tile actor to draw and the world-centre position to draw it at
    /// (`overlay_0897_801e0f3c`). Refreshed every field tick while a board
    /// is installed (honouring the header `+6` full-vs-windowed mode and
    /// `+5` radius); empty otherwise. The deferred renderer consumes this
    /// to draw each tile actor's mesh at every listed cell.
    pub draw_list: Vec<crate::tile_board::TileDraw>,
    /// The walk SM's state (the controller's `+0x54`, values in
    /// [`crate::tile_board::sm`]): the fade-in, walking, the quit prompt and
    /// the exit states down to teardown.
    pub sm: u8,
    /// The fade value (`+0x9C`, 4.12): what the SM copies into every tile
    /// actor's `+0x72` render scale while it fades.
    pub fade: i16,
    /// The quit prompt's cursor (`_DAT_8007BB88`, row `0` = quit).
    pub prompt_cursor: u32,
}

impl TileBoardState {
    pub fn new() -> Self {
        Self {
            grid: None,
            target: None,
            armed: false,
            header: None,
            actor_slots: [None; crate::tile_board::TILE_ACTOR_TABLE_LEN],
            draw_list: Vec::new(),
            sm: crate::tile_board::sm::WALK,
            fade: crate::tile_board::FADE_FULL,
            prompt_cursor: 0,
        }
    }
}

impl Default for TileBoardState {
    fn default() -> Self {
        Self::new()
    }
}
