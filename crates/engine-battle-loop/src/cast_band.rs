//! The cast band's carried state: the owed cast outcome, the player-summon
//! stager, the caster's clip-stage run and what a module's code half wrote.
//! The band itself (`engine-core`'s `world::battle::cast_band`) steps them.

/// A cast the action SM is carrying whose outcome is still owed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingCast {
    /// Casting actor slot (party or monster).
    pub caster: u8,
    /// The spell id (`actor[+0x1DF]`).
    pub spell_id: u8,
    /// Absolute actor slots the outcome folds onto.
    pub targets: Vec<u8>,
    /// The fold waits on the move's homing flight: each target is folded on
    /// the pass its effect child lands its hit
    /// (`World::settle_cast_band`, `World::fold_pending_cast_target`).
    pub on_flight: bool,
}

/// Where the stager is in its choreography.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SummonPhase {
    /// Armed by the cast trigger (`0x29`); the first tick (`0x34` entry)
    /// requests the creature spawn.
    Armed,
    /// Creature requested / seated: idle, then walk in to the strike point.
    Approach,
    /// Outcome folded; the creature holds at the strike point.
    Linger,
    /// Creature despawned; the next tick reports not busy.
    Done,
}

/// The caster's clip stages a capture-class body owes and its port does not
/// write ([`legaia_engine_cast::cast_module_ticks::CAPTURE_CASTER_STAGES`]),
/// in flight at the head of battle phase `0x70`.
///
/// Each stage is written into the caster's `+0x1DA` and the next one is
/// written behind it as soon as it commits, so the commit's boundary rule
/// plays every clip to its natural end; the last is followed by `0`, retail's
/// closing `sb zero,0x1DA(<caster>)`, and the run ends when the caster is
/// back on its idle entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CasterStageRun {
    /// The casting seat.
    pub slot: u8,
    /// The clips to stage, in order.
    pub clips: Vec<u8>,
    /// The module arm whose pass stages each clip, parallel to
    /// [`Self::clips`] - empty when the clips replay at the head of the band
    /// ([`legaia_engine_cast::cast_module_ticks::capture_caster_stage_arms`]).
    /// A placed clip does not hold the band: the arms run on while it plays.
    pub at_arms: Vec<u8>,
    /// Index of the clip being staged / waited on.
    pub cursor: usize,
    /// The seat the caster walks into reach of before the first stage
    /// ([`legaia_engine_cast::cast_module_ticks::capture_body_approaches`]); `None` for a
    /// body that strikes from where it stands, or a group target.
    pub approach: Option<u8>,
    /// Where the run is.
    pub phase: CasterStagePhase,
    /// Ticks spent holding the band so far (`CASTER_STAGE_TICK_LIMIT`).
    pub ticks: u16,
}

/// The three stretches of a [`CasterStageRun`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CasterStagePhase {
    /// Walking into reach: the walk entry `1` staged and the caster turned
    /// onto its victim every tick (its root motion carries it), until the
    /// range poll `FUN_8004E2F0` reads zero.
    Approach,
    /// Staging the clips: each is held until it has committed and either
    /// played out or reached its authored loop-window park.
    Staging,
    /// The caster sits on the last clip (parked, when it has a window) while
    /// the module's own arms run.
    Module,
    /// The module is done: the park is released (`sh zero,0x176(<caster>)`)
    /// and idle staged behind it (`sb zero,0x1DA(<caster>)`); the band holds
    /// until the caster is back on its idle entry.
    Closing,
}

/// One player summon in flight (see the module docs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SummonStager {
    /// Casting party slot.
    pub caster: u8,
    /// The spell id (`actor[+0x1DF]`).
    pub spell_id: u8,
    pub phase: SummonPhase,
    pub frames: u16,
    /// Where the creature is seated (behind the caster).
    pub spawn: [i16; 3],
    /// Where the walk ends and the outcome lands.
    pub goal: [i16; 3],
    /// The creature's walk has reached [`Self::goal`] (the answer a directed
    /// module's walk arm polls).
    pub walked: bool,
}

/// Where a module's code half wrote, so a caller can see the kernel ran
/// without re-reading every actor.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CastModuleCodeRun {
    /// The band entry that was resident (`903 + row` / `935 + sub_id`).
    pub prot_entry: u32,
    /// The module phase after the tick (`ctx+0x279`).
    pub phase: u8,
    /// `ctx+0x278` after the tick.
    pub ctx_278: u8,
    /// Seats an AoE stager swept, with the amount applied to each.
    pub aoe_hits: Vec<legaia_engine_cast::cast_module_ticks::AoeHit>,
    /// `true` while the module's phase machine still reports busy.
    ///
    /// Seeded `true` for a resident module and only *lowered* by a ported
    /// tick body, so it is meaningless on its own - read it together with
    /// [`Self::tick_ported`].
    pub busy: bool,
    /// Whether a ported tick body actually ran this frame. `false` means the
    /// entry is resident but its code half is one of the band's unported
    /// rows, and then [`Self::busy`] carries no information: a caller that
    /// held on it would hold forever.
    pub tick_ported: bool,
    /// The item PROT 0955's turn-steal arms handed back to the bag
    /// (`FUN_800421D4(victim[+0x1DF], 1)`), when the victim had an Item
    /// action queued.
    pub item_refund: Option<u8>,
    /// What PROT 0955's Void Accessories arm decided this frame.
    pub voided_accessory: Option<legaia_engine_cast::cast_module_ticks::VoidAccessoriesOutcome>,
    /// `(element, group)` PROT 0964's Element Change committed onto the first
    /// monster seat's record this frame (`+0x1D` / `+0x1C`).
    pub element_change: Option<(u8, u8)>,
    /// The camera shot the module armed this frame
    /// (`vm::battle_cam_script::ModuleShot`).
    pub camera_shot: Option<legaia_engine_battle_vm::battle_cam_script::ModuleShot>,
    /// The case-6 follow the module re-armed this frame
    /// ([`legaia_engine_cast::cast_module_camera::ModuleFollow`]).
    pub camera_follow: Option<legaia_engine_cast::cast_module_camera::ModuleFollow>,
    /// The module framed the acting actor through case 8 this frame
    /// ([`legaia_engine_cast::cast_module_camera::ArmDirection::end_frame`]).
    pub camera_end_frame: bool,
    /// The drift the module wrote into the camera globals this frame.
    pub camera_nudge: Option<legaia_engine_cast::cast_module_camera::ModuleNudge>,
    /// A capture-class body's drift this frame
    /// ([`legaia_engine_cast::cast_module_camera::capture_camera_director`]).
    pub capture_drift: Option<legaia_engine_cast::cast_module_camera::CaptureDrift>,
    /// The drift is the next pass's, applied after the shot is armed
    /// ([`legaia_engine_cast::cast_module_camera::ArmDirection::drift_after_shot`]).
    pub capture_drift_after_shot: bool,
    /// A capture director's absolute camera stores on this pass
    /// ([`legaia_engine_cast::cast_module_camera::CaptureCamArm::pose_set`]).
    pub capture_pose_set: Option<legaia_engine_cast::cast_module_camera::CapturePoseSet>,
    /// A capture director's focus z drift on this pass.
    pub capture_focus_z: i16,
    /// The capture director drifts before it shoots
    /// ([`legaia_engine_cast::cast_module_camera::CaptureCamArm::drift_first`]).
    pub capture_drift_first: bool,
    /// The spawn calls the module's arm made this frame, for a director that
    /// reports them ([`legaia_engine_cast::cast_module_camera::ModuleProfile::stages_spawns`]).
    pub spawns: &'static [legaia_engine_cast::cast_module_camera::ModuleSpawn],
    /// The spawn calls a summon-creature arm makes on every pass, held or
    /// not, beside [`Self::spawns`]
    /// ([`legaia_engine_cast::cast_module_camera::creature_arm_spawns`]).
    pub every_spawns: &'static [legaia_engine_cast::cast_module_camera::ModuleSpawn],
    /// The point the arm built for its [`legaia_engine_cast::cast_module_camera::SpawnAnchor::ArmPoint`]
    /// spawns this frame.
    pub arm_point: Option<([i16; 3], [i16; 3])>,
    /// The `MoveImage` the module's arm issued this frame.
    pub vram_move: Option<legaia_engine_cast::cast_module_camera::ModuleVramMove>,
    /// The text the module's arm put up this frame.
    pub caption: Option<legaia_engine_cast::cast_module_camera::ModuleCaption>,
    /// The fades the module's arm spawned this frame, each `(template, id)`.
    pub fades: &'static [(
        legaia_engine_battle_vm::battle_action::SummonFadeTemplate,
        i16,
    )],
    /// The arm killed the module's earlier fades first.
    pub kills_fades: bool,
}
