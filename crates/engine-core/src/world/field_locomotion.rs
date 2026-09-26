//! Player field-locomotion state: run / slow / precise-movement gates, step deltas, ledge hop, vertical settle, wall probes and the per-tick movement cues.
//!
//! Split out of the composite [`World`] so the state one subsystem owns
//! reads as one unit. Fields keep their retail provenance notes.

use super::*;

/// Player field-locomotion state: run / slow / precise-movement gates, step deltas, ledge hop, vertical settle, wall probes and the per-tick movement cues.
pub struct FieldLocomotion {
    /// When set, field free-movement snaps the player's `world_y` to the
    /// per-scene terrain elevation each step via
    /// [`crate::world::World::sample_field_floor_height`] (the port of `FUN_80019278`).
    /// Off by default so the flat-Y locomotion oracles keep their constant
    /// `world_y`; enable it for terrain-following play. Only the pad
    /// locomotion path consults it - world-map walk keeps its own height
    /// model - and it no-ops harmlessly (height `0`) until a scene supplies a
    /// floor LUT + collision grid.
    pub follow_terrain_height: bool,
    /// The player's field idle/walk clip pair (PROT 0874 §1 locomotion
    /// bundle). Installed per scene by the host
    /// ([`crate::world::World::set_field_player_anim`]); the field tick advances it after
    /// the locomotion step and folds the output into the player actor's
    /// `pose_frame`, so hosts rebuild the posed mesh exactly like the battle
    /// animation path. `None` = static rest pose.
    pub player_anim: Option<crate::field_anim::FieldPlayerAnim>,
    /// When set, pad locomotion blocks a direction with retail's
    /// **three-probe leading-edge footprint** (`FIELD_WALL_PROBES`, the
    /// `DAT_801f2214` table `FUN_801cfe4c` walks) instead of a single
    /// candidate-centre test: the player rests ~47 units off a wall plane
    /// exactly like retail, instead of walking up to it. Off by default so
    /// the existing locomotion oracles (and BFS nav drivers, which keep the
    /// centre test regardless) are bit-identical; enable it for
    /// retail-faithful wall standoff. Validated against the wall-press
    /// captures by `engine-shell/tests/field_collision_discriminator.rs`.
    pub leading_edge_wall_probes: bool,
    /// Accumulated walked amount the field walk-regen tick drains (retail
    /// `_DAT_801F2274`). [`crate::world::World::step_field_locomotion`] bumps it on every
    /// retail frame whose locomotion step actually committed;
    /// [`crate::world::World::tick_field_walk_regen`] consumes
    /// [`crate::walk_regen::WALK_REGEN_STEP_COST`] per regen tick. The drain
    /// is retail-pinned, the fill unit is the engine's - see
    /// [`crate::world::World::tick_field_walk_regen`].
    pub walk_regen_steps: i32,
    /// The **Incense window** (retail `_DAT_8007B600`), counted in walk-regen
    /// ticks: the pause Items Incense confirm tops it up by `0x40` (cap
    /// `0x100`, `crate::field_menu_dispatch::apply_pause_items_outcome`), the
    /// walk-regen tick drains it by one, and the field region encounter roll
    /// skips while it is non-zero - on the field and on the overworld alike,
    /// since both run the field overlay's walk tick. On its zero edge retail
    /// installs the field-overlay record `0x801F2278` (kind byte `0x0B`) as
    /// the entry context `_DAT_8007B450` and spawns the submode driver
    /// (`0x801D0CEC..0x801D0D24`), which shows the wear-off notice
    /// ([`Self::incense_notice`]).
    pub walk_regen_window: i32,
    /// The Incense wear-off notice while it is up (`FUN_801F1E48` via the
    /// kind-`0x0B` entry context; see [`crate::incense_notice`]).
    pub incense_notice: Option<crate::incense_notice::IncenseNotice>,
    /// The pad-rotation octant `gp+0x2D8` (`_DAT_8007B5F0`): how many
    /// eighth-turns the pad remapper `FUN_800467E8` rotates the held
    /// direction by. Retail's writers are field-VM op `4C 2x` and the tile
    /// board's walker, which also saves the incoming value on entry and
    /// restores it at teardown. The port's free-roaming field walk derives
    /// its rotation from the camera instead
    /// ([`crate::world::World::field_pad_ring_rotation`]); this word is what
    /// the tile board reads and restores.
    pub pad_octant: u32,
    /// Camera azimuth (PSX 12-bit angle, `4096` = full turn) used to make
    /// d-pad locomotion camera-relative. Retail equivalent: the view
    /// direction `func_0x800467e8` remaps the held pad against. `0` maps
    /// "screen up" to world `+Z` (the default follow camera looking down
    /// `+Z`). Engines that orbit the camera write the current azimuth here
    /// each frame; the locomotion remap quantises it to the nearest 90°.
    pub camera_azimuth: u16,
    /// Opt-in precise-movement mode for pad locomotion. When set,
    /// [`crate::world::World::step_field_locomotion`] decodes the held direction
    /// **continuously** instead of through retail's 4/8-way quantisation:
    /// the camera azimuth is applied at full angular resolution (not
    /// snapped to the nearest 90°), key diagonals walk true 45° vectors at
    /// full speed, and an analog stick ([`crate::input::InputState::lstick`])
    /// passes its angle through untouched. Off by default - the default
    /// path stays bit-identical to the retail-faithful quantised remap
    /// (replays / oracles are unaffected unless a host opts in).
    pub precise_movement: bool,
    /// Field Move option: `false` = Walk, `true` = Run (retail config word
    /// `0x800846CC`, the pause menu's "Field Move" row - see
    /// [`crate::options::FieldMoveOpt`]). Hosts mirror their
    /// [`crate::options::OptionsState`] onto this the way they mirror
    /// [`crate::world::FieldLocomotion::precise_movement`].
    ///
    /// This is the *default*, not the state: the run button INVERTS it, so
    /// with Run selected the button walks. See
    /// [`crate::world::World::field_run_active`].
    pub run_default: bool,
    /// `true` while the field run button is held this frame. Derived from the
    /// pad word inside [`crate::world::World::set_pad`], so no host wires it separately.
    ///
    /// Retail reads it as `pad_held & mask`, where the held-pad word is
    /// `_DAT_8007B850` and the mask is the config word `0x800846DC` - `0x48`
    /// = **Cross | R1** in the packed pad layout, seeded once by the new-game
    /// data-init `FUN_80034A6C` at `0x80034AB8` and written by nothing else
    /// on the disc, so retail's run button is not configurable. The port's
    /// mask is [`crate::world::FieldLocomotion::run_button_mask`], and it defaults to those two
    /// buttons. The XOR structure around the flag, in
    /// [`crate::world::World::field_run_active`], is pinned as well.
    pub run_button_held: bool,
    /// Which pad buttons count as "the run button" for
    /// [`crate::world::FieldLocomotion::run_button_held`].
    ///
    /// Defaults to
    /// [`FIELD_RUN_BUTTON_MASK_DEFAULT`](crate::world::config::FIELD_RUN_BUTTON_MASK_DEFAULT)
    /// = retail's `Cross | R1` plus **Square**, the port's historical
    /// binding, kept as an alternate. Assign
    /// [`FIELD_RUN_BUTTON_MASK_RETAIL`](crate::world::config::FIELD_RUN_BUTTON_MASK_RETAIL)
    /// for the retail button set exactly. Which *key* produces each of those
    /// buttons is the host's binding table
    /// (`legaia-engine config set --binding`), not this mask.
    pub run_button_mask: u16,
    /// Forced-slow gate: retail's `_DAT_8007B6A8` arm of the base-step
    /// selector. Non-zero there selects base step
    /// [`crate::world::config::FIELD_BASE_STEP_FORCED_SLOW`] and **skips the
    /// run check entirely** - a forced walk cannot be run out of.
    ///
    /// NOT WIRED: no host drives this yet. `_DAT_8007B6A8` is the same word
    /// the action-button gate and the per-scene save-allow test read
    /// (`docs/subsystems/field-locomotion.md`), and the port has no
    /// equivalent of its writer, so the constant and the arm are ported and
    /// the flag stays `false`.
    pub forced_slow: bool,
    /// Sub-step remainder carried between precise-movement frames, in world
    /// units per axis (|carry| < one collision step). Lets shallow movement
    /// angles accumulate distance across frames instead of rounding to
    /// zero. Only touched while [`crate::world::FieldLocomotion::precise_movement`] is active with a
    /// direction held; reset when input releases.
    pub precise_move_carry: (f32, f32),
    /// Last frame's field position for every actor the motion detector
    /// tracks - the player (from its [`crate::vm::ActorMoveState`]) and every
    /// entry of [`crate::world::FieldNpcState::positions`]. Rewritten each field tick by
    /// [`crate::world::World::detect_field_actor_motion`], which is the only reader.
    ///
    /// Cleared on scene entry alongside [`crate::world::FieldNpcState::positions`]: a
    /// stale entry across a scene change would read the warp itself as one
    /// enormous step and start every actor walking on the landing frame.
    ///
    /// Public only because `World` is built with functional-update syntax in
    /// integration tests, which requires every field to be visible; treat it
    /// as internal to the detector.
    pub motion_prev: std::collections::HashMap<u8, (i16, i16)>,
    /// Placement slots whose field position CHANGED during the frame just
    /// ticked - the source-agnostic "this actor is moving" signal.
    ///
    /// Recomputed every field tick by [`crate::world::World::detect_field_actor_motion`] by
    /// diffing live positions against [`crate::world::FieldLocomotion::motion_prev`], so it is
    /// true for a walk driven by the pad, by a nav step, by a motion-VM
    /// patrol leg, by a cutscene `MoveTo`, or by anything else that commits a
    /// position - the animation layer does not have to know which. That is
    /// the point: selecting the walk clip off the *mover* rather than off the
    /// *motion* is what made script-driven actors glide.
    ///
    /// The player's own bit is folded straight into
    /// [`crate::field_anim::FieldPlayerAnim::moved_this_frame`] rather than
    /// left here for a host to read.
    pub actor_moving: std::collections::HashSet<u8>,
    /// The post-remap direction bits of this tick's movement attempt
    /// (`0x1000`/`0x4000`/`0x2000`/`0x8000`; `0` when no direction is held).
    /// The walk-touch dispatch derives its leading probe points from it -
    /// retail's touch fires from the same forward probes that block the
    /// step, so contact must be tested ahead of the player, not at the
    /// player's feet.
    pub last_move_dir_bits: u16,
    /// The last **committed** locomotion sub-step direction, one entry per
    /// axis at retail's fixed magnitude 8 - the engine's stand-in for the
    /// retail global pair `0x8007BDE0` (X) / `0x8007BDE4` (Z).
    ///
    /// Retail writes these inside the locomotion controller `FUN_801d01b0`
    /// (`0x801d0550` clears the pair on a no-input frame; `0x801d07bc` and
    /// its per-axis siblings record `+-8` alongside each committed 2-unit
    /// sub-step). The magnitude is a fixed probe scale, **not** the frame
    /// speed: the ledge-hop probe multiplies it by 4 and samples at 2x and
    /// 3x that, i.e. 64 and 96 world units ahead - one and one-and-a-half
    /// collision sub-cells.
    ///
    /// A wall-blocked axis records `0`, so a hop is only ever attempted
    /// along an axis the walk actually moved on.
    pub step_delta: (i16, i16),
    /// Run the retail vertical settle (`FUN_801d1ba0`'s rate-clamped glide
    /// toward the floor) instead of leaving the actor's Y alone.
    ///
    /// Default **off**, and deliberately separate from
    /// [`crate::world::FieldLocomotion::follow_terrain_height`]: that flag *snaps* Y to the sampled
    /// floor in one frame, and the engine's flat-Y default (Y untouched when
    /// the snap is off) is an invariant the locomotion oracles pin. Retail
    /// does neither - it glides at `delta_scalar * 12` units per frame, so a
    /// tall drop takes several frames. Enabling this replaces "untouched"
    /// with the retail glide; it does not override the snap, which stays
    /// authoritative when set.
    ///
    /// The ledge-hop trigger is **not** gated on this - a hop is posted off
    /// the step delta whether or not the settle runs.
    pub vertical_settle: bool,
    /// The ledge hop [`crate::world::World::try_field_ledge_hop`] posted this frame, if any
    /// (retail hands the same triple to `FUN_801d2404`). `None` on every
    /// frame that did not start a hop.
    pub ledge_hop: Option<FieldLedgeHop>,
    /// The player actor's `+0x8E` **inverted-Y latch** - retail's third
    /// height arm, and the one thing an eased move over the player publishes
    /// besides the position triple.
    ///
    /// `FUN_801DD4C4` stores `-Y` here on every frame of a move whose target
    /// carries [`EASE_TARGET_INVERT_Y`](legaia_engine_vm::field_actor_timers::EASE_TARGET_INVERT_Y)
    /// (`0x801DD6A4..0x801DD6B8`: `lw v0,0x10(a2)` / `lui v1,0x2000` / `and` /
    /// `subu v0,zero,a1` / `sh v0,0x8e(a2)`). The consumer is the field-actor
    /// driver `FUN_8003BC08`, whose height arm tests the same flag **first**
    /// (`0x8003BC4C..0x8003BC64`) and writes `-(+0x8E)` into the actor's
    /// `+0x16` in place of the ground height its other two arms would sample -
    /// so the latch is not decoration, it is what stops the floor controllers
    /// dragging an airborne scripted move back down to the terrain.
    ///
    /// The engine's two height controllers are exactly those other two arms
    /// ([`crate::world::FieldLocomotion::vertical_settle`] is the glide, and
    /// [`crate::world::FieldLocomotion::follow_terrain_height`] the snap), so both read this and step
    /// aside while it is armed. `None` is the no-mirror case, which is every
    /// ordinary frame.
    ///
    /// Player-only, and that is a real limit rather than a simplification:
    /// retail's `+0x8E` is per-actor, but a scene NPC in this engine is a
    /// placement slot with an `(x, z)` pair and a scene-build Y from
    /// [`legaia_asset::field_objects::Placement::world_y`] - it has no
    /// per-frame height controller for a mirror to override.
    pub eased_mirror_y: Option<i16>,
    /// Scripted player-move cues raised by cutscene-timeline `A2 F8
    /// <move_id>` ExecMove pokes, in emission order. The windowed host
    /// drains these each frame and queues the named clip as a one-shot on
    /// [`crate::world::FieldLocomotion::player_anim`] - the clip is scene-ANM-bundle record
    /// `move_id - 1`, the same `id - 1` record space the op-`0x4B` NPC cues
    /// use (live-pinned: the `town01` post-naming ExecMove 48/49 land the
    /// retail player anim pointer on scene records 47/48).
    pub player_move_cues: Vec<u8>,
    /// The clip base `_DAT_8007BDD8`: the 1-based slot inside the leader's
    /// seven-record locomotion bank the settle tail strides into the player's
    /// clip id. Written by the pad step (idle `2` / walk `1` / run `3`, or
    /// the scene sentinel `99`), the hop phase machine (`6` / `7` / `1`) and
    /// the walk-on dispatcher; seeded `2` on scene entry. See
    /// [`legaia_engine_vm::field_player_clip`].
    pub clip_base: u16,
    /// The player actor's clip id `+0x5C`, as the settle tail last stored it.
    /// The pad step writes a base only while it is positive.
    pub player_clip: i16,
    /// The player actor's party-bank bit `+0x10 & 0x01000000`
    /// ([`legaia_engine_vm::field_player_clip::PARTY_BANK_FLAG`]): raised by
    /// every pad step that writes a base, dropped by the scene-sentinel pick.
    pub player_party_bank: bool,
    /// The clip override `_DAT_8007B6AC`: field-VM op `4C CE <value>`
    /// (`0x801E2A20..0x801E2A30`) stores its byte here, and scene entry
    /// zeroes it (SCUS `0x8003B6F0`, inside `FUN_8003AEB0`). While it is
    /// non-zero a party-flagged pick binds `base + override - 1` from the
    /// **scene** bank - the two disc users (`jagaroom`, `urudre1`) point the
    /// player's walk / idle / run at scene-bundle records this way.
    pub clip_override: u32,
    /// The kind-0 warp's globals `_DAT_8007B6B0` (timer), `_DAT_8007B6B4`
    /// (post-warp pad hold) and the destination pair - see
    /// [`legaia_engine_vm::field_warp_tile`].
    pub warp: legaia_engine_vm::field_warp_tile::WarpTimer,
    /// Frames left before the warp's fade-in (the second `FUN_801D58F0`,
    /// delayed `0x29` frames from the crossing) replaces the fade-out in the
    /// one fade slot the port has.
    pub warp_fade_in_in: Option<i32>,
}

impl FieldLocomotion {
    pub fn new() -> Self {
        Self {
            follow_terrain_height: false,
            player_anim: None,
            leading_edge_wall_probes: false,
            walk_regen_steps: 0,
            walk_regen_window: 0,
            incense_notice: None,
            pad_octant: 0,
            camera_azimuth: 0,
            precise_movement: false,
            run_default: false,
            run_button_held: false,
            run_button_mask: crate::world::config::FIELD_RUN_BUTTON_MASK_DEFAULT,
            forced_slow: false,
            precise_move_carry: (0.0, 0.0),
            motion_prev: std::collections::HashMap::new(),
            actor_moving: std::collections::HashSet::new(),
            last_move_dir_bits: 0,
            step_delta: (0, 0),
            vertical_settle: false,
            ledge_hop: None,
            eased_mirror_y: None,
            player_move_cues: Vec::new(),
            clip_base: legaia_engine_vm::field_player_clip::BASE_IDLE,
            player_clip: legaia_engine_vm::field_player_clip::BASE_IDLE as i16,
            player_party_bank: true,
            clip_override: 0,
            warp: legaia_engine_vm::field_warp_tile::WarpTimer::default(),
            warp_fade_in_in: None,
        }
    }
}

impl Default for FieldLocomotion {
    fn default() -> Self {
        Self::new()
    }
}
