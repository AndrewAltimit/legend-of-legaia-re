//! Field NPC state: positions, headings, motions, ambient anims, dialog bindings and the solid / animate toggles.
//!
//! One subsystem's state, held as a unit on the composite [`World`].
//! Fields keep their retail provenance notes.

use super::*;

/// One face / body deformation row (`DAT_8007BE60 + id * 12`), as op `43 07`
/// writes it (`0x801DF6F8..0x801DF758`): four halfwords (`+0`..`+6`, the
/// first three the object-space offset `FUN_80029888` adds to every vertex
/// before its rotation) and the flag word `+8`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FieldFaceRow {
    pub offset: [i16; 4],
    pub flags: u32,
}

/// Field NPC state: positions, headings, motions, ambient anims, dialog bindings and the solid / animate toggles.
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
        std::collections::HashMap<u8, legaia_engine_field::man_field_scripts::InlineDialogPrologue>,
    /// Talk proxies: placement `slot` -> (the placement whose conversation
    /// its interaction runs, the proxy's own spawn position). A proxy is an
    /// undrawn actor the facing probe can reach where the real speaker
    /// cannot be reached ([`legaia_engine_field::man_field_scripts::placement_talk_proxy_target`]).
    /// The probe admits the proxy's position and answers with the target.
    pub talk_proxies: std::collections::HashMap<u8, (u8, (i16, i16))>,
    /// Per-talkable-NPC spawn world position `(world_x, world_z)`, keyed by the
    /// same `slot` as [`crate::world::FieldNpcState::dialog`]. Populated at field-scene entry
    /// from the MAN actor placements. The interaction probe
    /// (`Self::tick_field_interaction_probe`) box-tests the player's position
    /// against these to fire a `field_interact` on the action button - the
    /// port-side analogue of retail's `FUN_801cf9f4` adjacency test.
    ///
    /// The runtime actor frame **is** the MAN placement frame: `FUN_8003A1E4`
    /// spawns each actor at `world = tile*128 + 0x40` (the placement's
    /// [`world_x`](legaia_scene_formats::man_section::ActorPlacement::world_x)), stored
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
    /// moved). Absent for an NPC nothing has turned yet, which stands at the
    /// spawn default: retail's seater leaves `+0x26 = 0` (Z-), the engine's
    /// [`SPAWN_HEADING`]. Hosts draw an absent entry as that default (the
    /// identity yaw); read it through [`Self::heading`], which applies it,
    /// never as `unwrap_or(0)` - engine `0` is the opposite compass point.
    pub headings: std::collections::HashMap<u8, i16>,
    /// Live per-NPC **pitch / roll** - retail `actor+0x24` and `actor+0x28`,
    /// the X and Z Euler angles the scripted-motion VM's `0x15` / `0x16`
    /// tween ([`legaia_engine_motion_vm::ambient_motion::AmbientMotion::pitch`] /
    /// [`roll`](legaia_engine_motion_vm::ambient_motion::AmbientMotion::roll)),
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
    pub(crate) tilts: std::collections::HashMap<u8, (i16, i16)>,
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
    /// Heading tweens a script scheduled with `4C 48 <value> <ticks>` (the
    /// field VM's nibble-4 sub-8 ramp on `+0x26`, `FUN_8003C5F0`), owner =
    /// placement slot. Ticked by `World::tick_npc_heading_ramps`.
    pub heading_ramps: legaia_engine_motion_vm::ambient_motion::RampScheduler,
    /// The face / body deformation rows `DAT_8007BE60 + id * 12` op `43 07`
    /// writes, keyed by row id ([`FieldFaceRow`]). Global in retail, so a row
    /// outlives the actor that wrote it.
    pub face_rows: std::collections::BTreeMap<u8, FieldFaceRow>,
    /// Per placement slot: the row id `+0x6D` and the ramped value `+0x7A`
    /// the field object draw (`FUN_8001B964` -> `FUN_80029888`) deforms the
    /// actor by while it is non-zero. Ticked by
    /// [`crate::world::World::tick_npc_heading_ramps`] off [`Self::face_ramps`].
    pub faces: std::collections::BTreeMap<u8, (u8, i16)>,
    /// The `+0x7A` ramps op `43 07` installs (`FUN_8003C5F0`, kind 2).
    pub face_ramps: legaia_engine_motion_vm::ambient_motion::RampScheduler,
    /// Per-NPC glide speed, keyed by the same placement `slot` as
    /// [`crate::world::FieldNpcState::dialog`]: the per-frame world-unit step
    /// `Self::start_field_npc_motion` writes into a leg's motion-VM
    /// [`legaia_engine_motion_vm::motion_vm::MotionState::speed`], decoded from the
    /// placement's real walk-kernel operands
    /// ([`legaia_engine_field::man_field_scripts::placement_glide_speed`]: the bound MAN
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
    /// streams ([`legaia_engine_field::man_field_scripts::motion_default_move_writes`]).
    /// The table the interaction motion-pause kick (`FUN_8003C9AC`, ported at
    /// [`legaia_engine_field::motion_pause`]) reloads a moving-class actor's
    /// requested-move pair from.
    pub default_moves: std::collections::BTreeMap<u8, [u8; 2]>,
    /// In-flight field-NPC walk legs, keyed by placement `slot`. Stepped once
    /// per field tick through the ported motion VM; each step writes the new
    /// position back into [`crate::world::FieldNpcState::positions`], so the moving NPC
    /// keeps its ±40-unit collision box and its interact box at the live
    /// position. Script-started legs (cutscene walk-to-tile pokes, actor
    /// VM `start_motion`) run regardless of [`crate::world::FieldNpcState::animate`].
    pub motions: std::collections::BTreeMap<u8, FieldNpcMotion>,
    /// Face-at legs a talk record armed on placements
    /// (`CC <id> 85|8E|8F <lo> <hi> <bind>` outside a cutscene timeline,
    /// which keeps its own), keyed by slot and stepped once per field tick
    /// ([`crate::world::World::tick_field_npc_face_legs`]).
    pub(crate) face_legs:
        std::collections::BTreeMap<u8, legaia_engine_field::inline_dialogue::TalkFaceRamp>,
    /// Budgeted `B8 <id> ..` rotate legs the same contexts armed, keyed and
    /// stepped alongside [`Self::face_legs`].
    pub(crate) rotate_legs:
        std::collections::BTreeMap<u8, legaia_engine_field::cutscene_timeline::TimelineFacing>,
    /// Compass-walk legs (`37` / `41 <b0> <b1>`, or the `B7` / `C1 <id>`
    /// cross-context form) a placement's load-frame slice armed on itself
    /// or on another placement, keyed by slot and stepped once per field
    /// tick alongside [`Self::face_legs`]. The arm halts the context
    /// (`+0x10 |= 0x400`) and seats the op on `+0x94`; `FUN_8003BC08` then
    /// runs the walk kernel `FUN_8003774C` on every tick the halt holds, and
    /// the kernel's landing clears it. `dolk`'s vendors (`P1[21]`
    /// `23 64 4D` / `41 06 01`) end half a tile east of their `MOVE_TO`
    /// tile that way, the counter between them and the player.
    /// The world tick a free-standing walk, turn or face-at leg on placement
    /// `slot` landed on, while the game-tick clock runs. The kernel visits a
    /// placement after the context that armed it - an armed target sits at
    /// the tail of its actor list (`town0c` flat 59 behind its talk in
    /// `rim_elm_queen_bee_battle`, `dolk2`'s face-at targets in
    /// `dolk2_market_noa`) - so a talk sees the landing on the next game
    /// frame, and the next op it aims at the placement waits until then.
    pub(crate) landed_ticks: std::collections::BTreeMap<u8, u64>,
    /// The same for the player's walk or turn a talk parked on
    /// ([`legaia_engine_field::inline_dialogue::InlineDialogue::player_walk`]).
    pub(crate) talk_player_landed: Option<u64>,
    pub(crate) glide_legs:
        std::collections::BTreeMap<u8, legaia_engine_field::cutscene_timeline::TimelineNpcGlide>,
    /// **Live per-slot model id**, keyed by placement `slot`: what the
    /// scripted-motion VM's op `0x0E` re-bound this actor's mesh to, in the
    /// raw operand space both model-pool consumers share (`< 0xF0` = the
    /// scene bank, `>= 0xF0` = the player bank at `operand - 0xF0`; see
    /// [`legaia_engine_scene::model_bank::resolve_model_id`]).
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
    /// [`legaia_engine_motion_vm::ambient_motion::AmbientMotion`]). Seeded at scene
    /// load from the MAN's tail-section-1 streams
    /// ([`crate::world::World::seed_field_npc_ambient`]) and stepped once per actor game
    /// tick by [`crate::world::World::tick_field_npc_ambient`]. Without it a standing town
    /// NPC holds one heading forever where retail NPCs slowly look around.
    pub ambient: std::collections::BTreeMap<u8, FieldNpcAmbient>,
    /// The scripted-motion streams bound to **placed objects**: MAN
    /// tail-section-1 records whose `actor_id` is a partition-0 record index
    /// (`FUN_8003A9D4` matches it against `+0x50`, which a `.MAP` object's
    /// actor carries as its bind record). Keyed by that record index. The
    /// koin3 dance hall's video wall is one: records `P0[5..=8]` cycle their
    /// panels' meshes with op `0x0E` every twelve ticks.
    pub(crate) object_ambient: std::collections::BTreeMap<usize, FieldNpcAmbient>,
    /// The live scene-bank model id an object's stream last swapped in
    /// (op `0x0E`), keyed by bind record; both hosts draw the record's
    /// placed objects with that mesh instead of the `.MAP` pack slot.
    pub(crate) object_models: std::collections::BTreeMap<usize, i16>,
    /// Every animated actor's side-buffer look rotation (field-VM `4C 45`)
    /// and the ramps driving it - [`legaia_engine_field::actor_look`].
    pub looks: legaia_engine_field::actor_look::ActorLooks,
    /// Publish the ambient tail-section-1 streams' walk steps
    /// ([`crate::world::World::tick_field_npc_ambient`]) - the villagers'
    /// authored wandering. The bare `World` default is off (NPCs rest at their
    /// placement anchors, as the engine-core locomotion oracles expect); both
    /// play hosts and the headless `BootSession` turn it on
    /// (`play-window --no-live-npcs` is the opt-out). Script-started motion
    /// is NOT gated by this flag, and neither is placement-script stepping:
    /// a placement's script runs only in its spawn pre-run
    /// ([`crate::world::World::pre_run_field_channel_prologues`]) and on a
    /// touch, whatever this says.
    pub animate: bool,
    /// Clip re-target cues raised by channel scripts - a cross-context
    /// `A2` ExecMove or a `4C 51` run's move-anim id, each an actor `+0x5C`
    /// write: `placement_index -> (count, move_id, extra bytes)`. The hosts
    /// drain these each frame and re-target the NPC's clip player.
    pub anim_cues: std::collections::HashMap<u8, (u8, u8, Vec<u8>)>,
    /// Each field actor's op-`0x4B` VDF morph lanes and ramp envelope, keyed
    /// by owner (a placement slot or a bound object's flat record), in the
    /// retail actor layout (`+0x6C` count, `+0xA0` weights, `+0xB0`
    /// sub-entry indices, `+0xB8` / `+0xC8` velocities, `+0x7C` done mask) -
    /// see [`crate::world::World::arm_field_morph`].
    pub morphs:
        std::collections::BTreeMap<crate::world::MorphOwner, legaia_engine_vm::move_vm::ActorState>,
    /// `.MAP` placed-object binds: flat MAN record -> the env-pack slots of
    /// the placed draws whose anchor tile binds it
    /// ([`crate::world::World::set_object_morph_targets`]).
    pub object_pack_slots: std::collections::BTreeMap<u16, Vec<usize>>,
    /// Placement slots whose morph deltas moved since a host last drained
    /// them ([`crate::world::World::take_npc_morph_dirty`]).
    pub morph_dirty: std::collections::BTreeSet<u8>,
    /// The move id each NPC's clip player last took from a single-move cue
    /// (`(1, id, [])` - a `+0x5C` write), keyed by placement slot: retail's
    /// `actor+0x5E`, the "clip now playing" half of the move-table
    /// consumer's change test. `FUN_800204F8` restarts the clip only when
    /// `+0x5C != +0x5E` (`0x80020570..0x800205A8`: the compare, then
    /// `+0x5E = +0x5C`, `+0x68 = 0`, `+0x56 = 1` and the new clip pointer), so
    /// re-requesting the playing move is a no-op, and
    /// [`crate::world::World::carry_npc_run_anim`] consults this to skip it.
    /// Written when [`crate::world::World::drain_field_anim_cues`] hands a
    /// cue to the hosts; an op-`0x4B` sequence cue removes the entry, since
    /// what then plays is not one move. Cleared with the cue queue.
    pub clip_current: std::collections::HashMap<u8, u8>,
    /// Each animated NPC actor's live clip cursor (`actor+0x68`) under its
    /// own `+0x62` control word, keyed by placement slot. The hosts own the
    /// decoded clip frames ([`legaia_engine_field::field_anim::FieldClipPlayer`]) but not
    /// the playhead: [`crate::world::World::tick_npc_clips`] advances these
    /// through the retail anim tick once per field frame, reading and writing
    /// the slot's channel `ctx.local_flags`, so an actor whose script holds
    /// or one-shots its clip - a treasure chest's `4C 35` closed hold, its
    /// touch's play-once-and-clamp lid, the `4C 36` already-opened snap -
    /// poses that way instead of looping. Bound by
    /// [`crate::world::World::bind_npc_clip_cursor`].
    pub clip_cursors: std::collections::BTreeMap<u8, legaia_engine_scene::field_env::PropAnim>,
    /// Op `4C C0`'s `rand()` draws not yet seated, keyed by placement slot.
    /// Retail binds the clip and seats `+0x68 = (rand() % frames) << 4`
    /// inside the op; the port binds a re-targeted clip when the cue drains,
    /// so the draw is taken at the op (keeping the `rand()` stream in retail
    /// order) and seated by whichever bind lands the clip
    /// ([`crate::world::World::randomize_npc_clip_phase`]).
    pub(crate) phase_draws: std::collections::HashMap<u8, u32>,
    /// Slots whose [`Self::clip_cursors`] entry the world bound itself, from
    /// the scene bundle's record header at the poke
    /// ([`crate::world::World::bind_npc_scene_clip`]): their cursor keeps the
    /// record's gate and divisor and takes its rate from the actor's live
    /// `+0x6A` each tick, as retail's clip tick does. A host's bind (whose
    /// step is already folded at the template rate) drops the slot.
    pub(crate) clip_rate_live: std::collections::BTreeSet<u8>,
    /// The bone count each animated NPC's clip was first bound with, keyed
    /// by placement slot - the count the hosts cut its mesh to. A re-target
    /// to a clip of another count is refused
    /// ([`crate::world::World::drain_field_anim_cues`]).
    pub clip_bones: std::collections::HashMap<u8, usize>,
    /// The Y of each field NPC on the **glide** height arm, keyed by
    /// placement slot: retail's `+0x16` while the actor's flag word carries
    /// `0x2000`, which `FUN_8003BC08` steps toward the floor by at most
    /// `frame_step * 6` per tick instead of snapping it there. Absent for an
    /// NPC on the snap arm, whose Y is the floor sample itself.
    /// Stepped by [`crate::world::World::tick_field_npc_heights`] and read by
    /// [`crate::world::World::field_npc_render_y`].
    pub(crate) glide_y: std::collections::HashMap<u8, i16>,
    /// The camera state the visibility cull `FUN_801D79E8` reads (focus,
    /// region box, visible tile window), as the camera last left it.
    /// Published by [`legaia_engine_field::camera::Camera::route_camera_events`], which
    /// both play hosts call every frame; `None` culls nothing.
    pub cull_view: Option<legaia_engine_field::camera::FieldCullView>,
}

/// The heading a placement stands in before anything turns it, in the
/// engine's [`FieldNpcState::headings`] space: retail `+0x26 = 0` (Z-),
/// which is `engine = retail + 0x800`. The placement record carries no
/// facing byte; a spawn prologue's `0x4C 0x51` / `0x38` seeds the others.
pub const SPAWN_HEADING: i16 = 0x800;

impl FieldNpcState {
    /// The live heading of placement `slot`, the spawn default
    /// ([`SPAWN_HEADING`]) when nothing has written one.
    pub fn heading(&self, slot: u8) -> i16 {
        self.headings.get(&slot).copied().unwrap_or(SPAWN_HEADING)
    }

    /// Does placement `slot` have a compass-walk leg in flight
    /// ([`Self::glide_legs`]): a `37` / `41` its own spawn section or a talk
    /// record armed, which the walk kernel is still stepping? That motion is
    /// the script's, not the ambient stream's.
    pub fn glide_leg_active(&self, slot: u8) -> bool {
        self.glide_legs.contains_key(&slot)
    }

    pub fn new() -> Self {
        Self {
            solid: false,
            dialog: std::collections::HashMap::new(),
            dialog_prologue: std::collections::HashMap::new(),
            talk_proxies: std::collections::HashMap::new(),
            positions: std::collections::HashMap::new(),
            entry_positions: std::collections::HashMap::new(),
            headings: std::collections::HashMap::new(),
            tilts: std::collections::HashMap::new(),
            facing_save: None,
            heading_ramps: legaia_engine_motion_vm::ambient_motion::RampScheduler::new(),
            face_rows: std::collections::BTreeMap::new(),
            faces: std::collections::BTreeMap::new(),
            face_ramps: legaia_engine_motion_vm::ambient_motion::RampScheduler::new(),
            glide_speeds: std::collections::BTreeMap::new(),
            default_moves: std::collections::BTreeMap::new(),
            motions: std::collections::BTreeMap::new(),
            face_legs: std::collections::BTreeMap::new(),
            rotate_legs: std::collections::BTreeMap::new(),
            landed_ticks: std::collections::BTreeMap::new(),
            talk_player_landed: None,
            glide_legs: std::collections::BTreeMap::new(),
            models: std::collections::BTreeMap::new(),
            ambient: std::collections::BTreeMap::new(),
            object_ambient: std::collections::BTreeMap::new(),
            object_models: std::collections::BTreeMap::new(),
            looks: Default::default(),
            animate: false,
            anim_cues: std::collections::HashMap::new(),
            morphs: std::collections::BTreeMap::new(),
            object_pack_slots: std::collections::BTreeMap::new(),
            morph_dirty: std::collections::BTreeSet::new(),
            clip_current: std::collections::HashMap::new(),
            clip_cursors: std::collections::BTreeMap::new(),
            phase_draws: std::collections::HashMap::new(),
            clip_rate_live: std::collections::BTreeSet::new(),
            clip_bones: std::collections::HashMap::new(),
            glide_y: std::collections::HashMap::new(),
            cull_view: None,
        }
    }
}

impl Default for FieldNpcState {
    fn default() -> Self {
        Self::new()
    }
}
