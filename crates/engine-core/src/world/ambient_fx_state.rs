//! Field ambient animation state: the CLUT-walk / CLUT-cell cyclers, VDF pulse, script VRAM moves and their vsync accumulators.
//!
//! Split out of the composite [`World`] so the state one subsystem owns
//! reads as one unit. Fields keep their retail provenance notes.

/// Field ambient animation state: the CLUT-walk / CLUT-cell cyclers, VDF pulse, script VRAM moves and their vsync accumulators.
pub struct AmbientFxState {
    /// Live **ambient** move-VM effect parts - the scene-entry effect tree
    /// the MAN partition-1 effect-actor scripts install (jou's pulsating
    /// flesh / lightning director). Unlike [`crate::world::FieldPropState::active_fx`] these
    /// parts read + self-modify the shared prescript bundle in place
    /// (retail `_DAT_8007B8D0`) and spawn op-`0x25` children. Spawned by
    /// [`crate::world::World::spawn_ambient_record`] at scene entry, ticked on the retail
    /// game-tick clock by [`crate::world::World::step_ambient_fx`].
    pub fx: Vec<crate::world::ambient::AmbientPart>,
    /// Retail game ticks banked for the ambient effect parts (the sibling
    /// of [`crate::world::AmbientFxState::clut_pending_game_ticks`], same clock law).
    pub pending_game_ticks: u32,
    /// Vsync sub-accumulator for the ambient game-tick bank.
    pub vsync_accum: u8,
    /// Per-rect VRAM capture cache for the ambient CLUT-cell cyclers: the
    /// texels op `0x2C` stored (`FUN_8005842C` StoreImage) keyed by the
    /// captured rect. Filled lazily by [`crate::world::World::step_ambient_fx`] from the
    /// host's VRAM the first time a cell fires; cleared on scene entry.
    pub cell_captures: std::collections::HashMap<(u16, u16, u16, u16), Vec<u16>>,
    /// The limiter's per-rect applied `(v_add, white)` state - what the
    /// last [`crate::world::World::step_ambient_fx`] actually wrote, keyed like
    /// [`crate::world::AmbientFxState::cell_captures`] and cleared with it on scene entry.
    pub flash_applied: std::collections::HashMap<(u16, u16, u16, u16), (i16, i16)>,
    /// Scene-entry **VDF pulse** (enhancement): a rolling ramp envelope over
    /// the scene's populated VDF pack for scenes whose entry-ambient tree
    /// arms no morph lanes of its own (jou). Installed by
    /// [`crate::world::World::install_entry_vdf_pulse`], ticked with the ambient bank,
    /// surfaced through [`crate::world::World::current_morph_deltas`]. `None` = retail
    /// behaviour (see `docs/subsystems/field-ambient-fx.md`).
    pub entry_vdf_pulse: Option<crate::vdf_pulse::EntryVdfPulse>,
    /// `(pack_slot, group)` pairs whose morph deltas changed during the last
    /// ambient drain - the renderer-facing dirty set
    /// ([`crate::world::World::take_morph_dirty_slots`]).
    pub morph_dirty_slots: std::collections::BTreeSet<(usize, u32)>,
    /// Vsyncs accumulated toward the next retail *game tick* (a game tick
    /// spans [`crate::world::FrameClock::frame_step`] vsyncs). Advanced by [`crate::world::World::tick`] on the
    /// sim ticks that map to a retail vsync ([`crate::world::FrameClock::display_frame_step`]).
    pub clut_vsync_accum: u8,
    /// Retail game ticks elapsed since the host last drained the scripted
    /// CLUT effects ([`crate::world::World::step_clut_fx`] consumes these). Only
    /// accumulates while [`crate::world::AmbientFxState::clut_fx`] is non-empty, and saturates at a
    /// small cap so a host that never drains can't wind up an unbounded
    /// backlog.
    pub clut_pending_game_ticks: u32,
    /// Live scripted CLUT-cell effects (field-VM `0x4C` n6 sub-`0x61`):
    /// pending one-shot cell writes and in-flight cross-fades. Spawned by
    /// [`crate::world::World::spawn_clut_cell_fx`] (the `op4c_n6_sub_61_emitter` host
    /// hook), stepped + applied against the host's software VRAM by
    /// [`crate::world::World::step_clut_fx`], cleared on scene entry.
    pub clut_fx: Vec<crate::world::ClutCellFx>,
    /// Live field-VM `4C DB` single-source CLUT blend fades, spawned by
    /// [`crate::world::World::spawn_clut_blend_fx`] and stepped by
    /// [`crate::world::World::step_clut_fx`] on the same game-tick bank;
    /// cleared on scene entry.
    pub clut_blend_fx: Vec<crate::world::ClutBlendFx>,
    /// Queued field-VM `4C 60` literal-operand VRAM `MoveImage` stamps (the
    /// sibling of [`crate::world::AmbientFxState::clut_fx`] - retail's one-shot face-frame stamps
    /// onto the player texture atlas). Queued by
    /// [`crate::world::World::queue_script_vram_move`] (the `op4c_n6_sub0_emitter6` host
    /// hook), drained against the host's software VRAM by
    /// [`crate::world::World::apply_script_vram_moves`], cleared on scene entry.
    pub script_vram_moves: Vec<crate::world::ScriptVramMove>,
    /// Queued field-VM op-`0x43` sub-`0x12` VRAM rectangle copies, in the
    /// emission order the arm resolved them (one call, or two for a copy
    /// wider than a VRAM page). Queued by
    /// [`crate::world::World::queue_vram_rect_copies`] (the
    /// `FieldHost::op43_vram_rect_copy` host hook), drained against the
    /// host's software VRAM by
    /// [`crate::world::World::apply_vram_rect_copies`].
    pub vram_rect_copies: Vec<legaia_engine_vm::vram_rect_copy::RectCopyCall>,
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
            clut_blend_fx: Vec::new(),
            script_vram_moves: Vec::new(),
            vram_rect_copies: Vec::new(),
        }
    }
}

impl Default for AmbientFxState {
    fn default() -> Self {
        Self::new()
    }
}
