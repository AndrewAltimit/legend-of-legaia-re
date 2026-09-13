//! Field ambient animation state: the CLUT-walk / CLUT-cell cyclers, VDF pulse, script VRAM moves and their vsync accumulators.
//!
//! Split out of the composite [`World`] so the state one subsystem owns
//! reads as one unit. Fields keep their retail provenance notes.

/// Field ambient animation state: the CLUT-walk / CLUT-cell cyclers, VDF pulse, script VRAM moves and their vsync accumulators.
pub struct AmbientFxState {
    /// Live **ambient** move-VM effect parts - the scene-entry effect tree
    /// the MAN partition-1 effect-actor scripts install (jou's pulsating
    /// flesh / lightning director). Unlike [`Self::active_field_fx`] these
    /// parts read + self-modify the shared prescript bundle in place
    /// (retail `_DAT_8007B8D0`) and spawn op-`0x25` children. Spawned by
    /// [`World::spawn_ambient_record`] at scene entry, ticked on the retail
    /// game-tick clock by [`World::step_ambient_fx`].
    pub fx: Vec<crate::world::ambient::AmbientPart>,
    /// Retail game ticks banked for the ambient effect parts (the sibling
    /// of [`Self::clut_pending_game_ticks`], same clock law).
    pub pending_game_ticks: u32,
    /// Vsync sub-accumulator for the ambient game-tick bank.
    pub vsync_accum: u8,
    /// Per-rect VRAM capture cache for the ambient CLUT-cell cyclers: the
    /// texels op `0x2C` stored (`FUN_8005842C` StoreImage) keyed by the
    /// captured rect. Filled lazily by [`World::step_ambient_fx`] from the
    /// host's VRAM the first time a cell fires; cleared on scene entry.
    pub cell_captures: std::collections::HashMap<(u16, u16, u16, u16), Vec<u16>>,
    /// The limiter's per-rect applied `(v_add, white)` state - what the
    /// last [`World::step_ambient_fx`] actually wrote, keyed like
    /// [`Self::ambient_cell_captures`] and cleared with it on scene entry.
    pub flash_applied: std::collections::HashMap<(u16, u16, u16, u16), (i16, i16)>,
    /// Scene-entry **VDF pulse** (enhancement): a rolling ramp envelope over
    /// the scene's populated VDF pack for scenes whose entry-ambient tree
    /// arms no morph lanes of its own (jou). Installed by
    /// [`World::install_entry_vdf_pulse`], ticked with the ambient bank,
    /// surfaced through [`World::current_morph_deltas`]. `None` = retail
    /// behaviour (see `docs/subsystems/field-ambient-fx.md`).
    pub entry_vdf_pulse: Option<crate::vdf_pulse::EntryVdfPulse>,
    /// `(pack_slot, group)` pairs whose morph deltas changed during the last
    /// ambient drain - the renderer-facing dirty set
    /// ([`World::take_morph_dirty_slots`]).
    pub morph_dirty_slots: std::collections::BTreeSet<(usize, u32)>,
    /// Vsyncs accumulated toward the next retail *game tick* (a game tick
    /// spans [`Self::frame_step`] vsyncs). Advanced by [`World::tick`] on the
    /// sim ticks that map to a retail vsync ([`Self::field_frame_step`]).
    pub clut_vsync_accum: u8,
    /// Retail game ticks elapsed since the host last drained the scripted
    /// CLUT effects ([`World::step_clut_fx`] consumes these). Only
    /// accumulates while [`Self::clut_fx`] is non-empty, and saturates at a
    /// small cap so a host that never drains can't wind up an unbounded
    /// backlog.
    pub clut_pending_game_ticks: u32,
    /// Live scripted CLUT-cell effects (field-VM `0x4C` n6 sub-`0x61`):
    /// pending one-shot cell writes and in-flight cross-fades. Spawned by
    /// [`World::spawn_clut_cell_fx`] (the `op4c_n6_sub_61_emitter` host
    /// hook), stepped + applied against the host's software VRAM by
    /// [`World::step_clut_fx`], cleared on scene entry.
    pub clut_fx: Vec<crate::world::ClutCellFx>,
    /// Queued field-VM `4C 60` literal-operand VRAM `MoveImage` stamps (the
    /// sibling of [`Self::clut_fx`] - retail's one-shot face-frame stamps
    /// onto the player texture atlas). Queued by
    /// [`World::queue_script_vram_move`] (the `op4c_n6_sub0_emitter6` host
    /// hook), drained against the host's software VRAM by
    /// [`World::apply_script_vram_moves`], cleared on scene entry.
    pub script_vram_moves: Vec<crate::world::ScriptVramMove>,
}

impl AmbientFxState {
    pub fn new() -> Self {
        Self {
            fx: Vec::new(),
            pending_game_ticks: 0,
            vsync_accum: 0,
            cell_captures: std::collections::HashMap::new(),
            flash_applied: std::collections::HashMap::new(),
            entry_vdf_pulse: None,
            morph_dirty_slots: std::collections::BTreeSet::new(),
            clut_vsync_accum: 0,
            clut_pending_game_ticks: 0,
            clut_fx: Vec::new(),
            script_vram_moves: Vec::new(),
        }
    }
}

impl Default for AmbientFxState {
    fn default() -> Self {
        Self::new()
    }
}
