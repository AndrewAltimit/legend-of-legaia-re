//! Field NPC state: positions, headings, routes, motions, ambient anims, dialog bindings and the solid / animate toggles.
//!
//! Split out of the composite [`World`] so the state one subsystem owns
//! reads as one unit. Fields keep their retail provenance notes.

use super::*;

/// Field NPC state: positions, headings, routes, motions, ambient anims, dialog bindings and the solid / animate toggles.
pub struct FieldNpcState {
    /// When set, pad locomotion also blocks a direction when any of retail's
    /// **actor-collision probes** (`FIELD_ACTOR_PROBES`, the `DAT_801f21b4`
    /// sibling table) lands inside a field NPC's body box or a placed prop's
    /// static collision box ([`crate::world::World::field_actor_dir_blocked`]) - NPCs and
    /// props become solid, as in retail, where `FUN_801cfe4c`'s actor bits
    /// (`1`/`4`) gate a step exactly like the wall bit (`2`). Off by default
    /// for the same oracle-stability reason as
    /// [`crate::world::FieldLocomotion::leading_edge_wall_probes`]. The locomotion-path touch
    /// dispatch (the `FUN_801d5b5c` auto event post for prop walk-touch) is
    /// modelled separately and independent of this flag -
    /// `Self::check_field_walk_touch`; the button-press interact dispatch
    /// is (`Self::tick_field_interaction_probe`).
    pub solid: bool,
    /// Per-actor inline interaction-script dialogue, keyed by the actor's
    /// MAN partition-1 record index - the `slot` the talk probe addresses.
    /// Populated at field-scene entry from
    /// the scene's actor placements. This is the **real** field NPC dialogue
    /// source (the actor's inline MES text at retail `actor[+0x90]`), so
    /// `World::trigger_field_interact` (the talk path) opens the interacted
    /// actor's dialogue from here - not from a `0x3F` op (which is the named
    /// scene-change, not dialogue). Empty between field scenes.
    pub dialog: std::collections::HashMap<u8, Vec<u8>>,
    /// Prologue-aware companion to [`crate::world::FieldNpcState::dialog`], keyed by the same
    /// `slot`. Carries each talk NPC's **untruncated** interaction record (full
    /// body + entry PC + first-segment offset) so the opt-in field-VM dialogue
    /// runner ([`crate::world::WorldToggles::use_vm_dialogue`]) can execute the interaction prologue -
    /// the story-flag `SysFlag.Test`/`JmpRel` segment-selection bytecode before
    /// the first `0x1F` - instead of starting at the first segment. The default
    /// simplified path ignores this and uses `field_npc_dialog` unchanged.
    pub dialog_prologue:
        std::collections::HashMap<u8, crate::man_field_scripts::InlineDialogPrologue>,
    /// Per-talkable-NPC spawn world position `(world_x, world_z)`, keyed by the
    /// same `slot` as [`crate::world::FieldNpcState::dialog`]. Populated at field-scene entry
    /// from the MAN actor placements. The interaction probe
    /// (`Self::tick_field_interaction_probe`) box-tests the player's position
    /// against these to fire a `field_interact` on the action button - the
    /// port-side analogue of retail's `FUN_801cf9f4` adjacency test.
    ///
    /// The runtime actor frame **is** the MAN placement frame: `FUN_8003A1E4`
    /// spawns each actor at `world = tile*128 + 0x40` (the placement's
    /// [`world_x`](legaia_asset::man_section::ActorPlacement::world_x)), stored
    /// straight into `actor[+0x14/+0x18]` by `FUN_80024C88` with no anchor, and
    /// the player cold-spawn `0xA40` is `tile 20*128 + 0x40` in that same frame.
    /// So these placement positions compare directly against the player's
    /// [`crate::vm::ActorMoveState::world_x`]. Positions are LIVE, not just
    /// the spawn tile: `Self::tick_field_npc_motions` writes walking NPCs'
    /// per-frame positions back here, so collision and interact probes follow
    /// a moving NPC.
    pub positions: std::collections::HashMap<u8, (i16, i16)>,
    /// Snapshot of [`crate::world::FieldNpcState::positions`] taken right after the
    /// scene-entry spawn-prologue pre-run
    /// ([`crate::world::World::pre_run_field_channel_prologues`]) - each slot's story-true
    /// initial position (spawn tile, story relocation, or the off-map park).
    /// The cutscene-teardown un-hide restores parked slots to THIS state
    /// rather than the raw MAN spawn tile, so a story-parked actor stays
    /// parked when a timeline ends.
    pub entry_positions: std::collections::HashMap<u8, (i16, i16)>,
    /// Live per-NPC heading (PSX 12-bit angle, same `render_26` convention as
    /// the player: `0` = travel Z+), keyed by placement slot. Written by
    /// `Self::tick_field_npc_motions` from each walk step's direction, and
    /// retained when the walker stops (an NPC keeps facing the way it last
    /// moved). Absent for NPCs that have never walked - hosts render those
    /// unrotated (the placement record carries no facing byte; scripted
    /// initial facings are the per-actor field-VM channels, not yet
    /// executed).
    pub headings: std::collections::HashMap<u8, i16>,
    /// Live per-NPC **pitch / roll** - retail `actor+0x24` and `actor+0x28`,
    /// the X and Z Euler angles the scripted-motion VM's `0x15` / `0x16`
    /// tween ([`legaia_engine_vm::ambient_motion::AmbientMotion::pitch`] /
    /// [`roll`](legaia_engine_vm::ambient_motion::AmbientMotion::roll)),
    /// keyed by placement slot in the same 12-bit angle space as
    /// [`crate::world::FieldNpcState::headings`].
    ///
    /// Published by `Self::tick_field_npc_motions` alongside the heading, and
    /// read by both hosts through
    /// [`crate::world::World::field_npc_tilt`]. Absent for a slot whose
    /// channel never tweened either angle, which is the overwhelming majority:
    /// the disc-wide census finds `0x15` authored nowhere and `0x16` only in
    /// `juui1`, so a host's yaw-only fast path stays the common case.
    ///
    /// The angles are an actor draw's, not a placement's: the per-actor render
    /// dispatcher hands `actor+0x24` straight to the three-angle composer
    /// (`addiu a0,s0,0x24` / `jal 0x80026988` at `0x8001af04` in
    /// `FUN_8001ADA4`), which reads X at `+0`, Y at `+2` and Z at `+4`.
    ///
    /// REF: FUN_8001ADA4, FUN_80026988
    pub tilts: std::collections::HashMap<u8, (i16, i16)>,
    /// The talk-time facing save: `(placement slot, the heading the NPC stood
    /// with before the player addressed it)`.
    ///
    /// Retail's touch post copies the addressed actor's `+0x26` into `+0x5A`
    /// (`FUN_801D5B5C` at `0x801D5BE4`: `lhu a0,0x26(a2)` then `sh a0,0x5a(a2)`
    /// in the branch delay slot), and the dialog SM's teardown writes it back
    /// (`FUN_80039B7C` at `0x80039CE8` / `0x80039EBC`) - so an NPC that turned
    /// to answer the player returns to its authored heading afterwards.
    ///
    /// One slot at a time, because one interaction owns the player at a time
    /// (retail keeps the save per-actor and releases the engaged flag only
    /// once every overlapping touch is dismissed; the engine opens exactly one
    /// conversation, so a single slot is the whole of that state).
    ///
    /// PORT: FUN_801D5B5C (the `+0x26` -> `+0x5A` save)
    pub facing_save: Option<(u8, i16)>,
    /// Per-NPC autonomous walk routes, keyed by the same placement `slot` as
    /// [`crate::world::FieldNpcState::dialog`]: the ordered local waypoints the placement's
    /// own pre-text script walks the actor through (its `0x4C 0x51` NPC
    /// move-to-tile ops - [`crate::man_field_scripts::placement_motion_route`]).
    /// Driven through the motion VM by `Self::tick_field_npc_motions` when
    /// [`crate::world::FieldNpcState::animate`] is set. `BTreeMap` so the per-tick walk
    /// order is deterministic (the replay oracle requires bit-stable traces).
    pub routes: std::collections::BTreeMap<u8, Vec<(i16, i16)>>,
    /// Per-NPC glide speed, keyed by the same placement `slot` as
    /// [`crate::world::FieldNpcState::routes`]: the per-frame world-unit step
    /// `Self::start_field_npc_motion` writes into a leg's motion-VM
    /// [`legaia_engine_vm::motion_vm::MotionState::speed`], decoded from the
    /// placement's real walk-kernel operands
    /// ([`crate::man_field_scripts::placement_glide_speed`]: the bound MAN
    /// tail-section-1 wander/step ops first, then the record's own field-VM
    /// `0x37`/`0x41`/`0x47` yield ops, then the facing-nibble heuristic as a
    /// last resort). A slot with no decodable motion leg is absent and the
    /// leg falls back to the stand-in
    /// [`crate::world::FIELD_NPC_MOTION_SPEED`]. See
    /// `docs/subsystems/field-locomotion.md`.
    pub glide_speeds: std::collections::BTreeMap<u8, u16>,
    /// Per-NPC default-move pair `[move_id, anim_id]`, keyed by placement
    /// `slot`: the motion op-`0x17` writes into the retail per-actor table at
    /// `0x801C6470`, statically harvested from the scene MAN's tail-section-1
    /// streams ([`crate::man_field_scripts::motion_default_move_writes`]).
    /// The table the interaction motion-pause kick (`FUN_8003C9AC`, ported at
    /// [`legaia_engine_vm::motion_pause`]) reloads a moving-class actor's
    /// requested-move pair from.
    pub default_moves: std::collections::BTreeMap<u8, [u8; 2]>,
    /// In-flight field-NPC walk legs, keyed by placement `slot`. Stepped once
    /// per field tick through the ported motion VM; each step writes the new
    /// position back into [`crate::world::FieldNpcState::positions`], so the moving NPC
    /// keeps its ±40-unit collision box and its interact box at the live
    /// position. Script-started legs (interaction-prologue `0x4C 0x51`, actor
    /// VM `start_motion`) run regardless of [`crate::world::FieldNpcState::animate`].
    pub motions: std::collections::BTreeMap<u8, FieldNpcMotion>,
    /// **Live per-slot model id**, keyed by placement `slot`: what the
    /// scripted-motion VM's op `0x0E` re-bound this actor's mesh to, in the
    /// raw operand space both model-pool consumers share (`< 0xF0` = the
    /// scene bank, `>= 0xF0` = the player bank at `operand - 0xF0`; see
    /// [`crate::model_bank::resolve_model_id`]).
    ///
    /// Absent = the actor still draws the mesh its placement's
    /// `model_index` named, which is every actor until a script swaps one.
    /// Retail has no such map: `FUN_80024E08` writes the new id straight into
    /// `actor[+0x64]` and reloads the mesh, and the port's hosts hold the
    /// uploaded mesh instead of the actor, so the id has to live where both
    /// of them can see it.
    pub models: std::collections::BTreeMap<u8, i16>,
    /// Per-NPC **ambient facing** channels, keyed by placement `slot`: the
    /// second motion VM's idle turn-in-place behaviour (`FUN_80038158` ops
    /// `0x04` / `0x0D`, ported at
    /// [`legaia_engine_vm::ambient_motion::AmbientMotion`]). Seeded at scene
    /// load from the MAN's tail-section-1 streams
    /// ([`crate::world::World::seed_field_npc_ambient`]) and stepped once per actor game
    /// tick by [`crate::world::World::tick_field_npc_ambient`]. Without it a standing town
    /// NPC holds one heading forever where retail NPCs slowly look around.
    pub ambient: std::collections::BTreeMap<u8, FieldNpcAmbient>,
    /// Drive autonomous NPC patrol routes ([`crate::world::FieldNpcState::routes`]) through
    /// the motion VM. The engine default is off (NPCs rest at their placement
    /// anchors, as the locomotion oracles expect); both play hosts turn it on
    /// (`play-window --no-live-npcs` is the opt-out). Script-started motion
    /// is NOT gated by this flag.
    pub animate: bool,
    /// Animation cues raised by channel scripts (op `0x4B` ANIMATE):
    /// `placement_index -> (count, base_id, keyframe bytes)`. The windowed
    /// host drains these each frame and re-targets the NPC's clip player.
    pub anim_cues: std::collections::HashMap<u8, (u8, u8, Vec<u8>)>,
}

impl FieldNpcState {
    pub fn new() -> Self {
        Self {
            solid: false,
            dialog: std::collections::HashMap::new(),
            dialog_prologue: std::collections::HashMap::new(),
            positions: std::collections::HashMap::new(),
            entry_positions: std::collections::HashMap::new(),
            headings: std::collections::HashMap::new(),
            tilts: std::collections::HashMap::new(),
            facing_save: None,
            routes: std::collections::BTreeMap::new(),
            glide_speeds: std::collections::BTreeMap::new(),
            default_moves: std::collections::BTreeMap::new(),
            motions: std::collections::BTreeMap::new(),
            models: std::collections::BTreeMap::new(),
            ambient: std::collections::BTreeMap::new(),
            animate: false,
            anim_cues: std::collections::HashMap::new(),
        }
    }
}

impl Default for FieldNpcState {
    fn default() -> Self {
        Self::new()
    }
}
