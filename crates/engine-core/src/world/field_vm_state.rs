//! Field-VM execution state beyond the main context: per-record channels, helper contexts, spawn requests, the op-0x49 submode block / screen, eased moves and the entry / free-roam latches.
//!
//! Split out of the composite [`World`] so the state one subsystem owns
//! reads as one unit. Fields keep their retail provenance notes.

/// Field-VM execution state beyond the main context: per-record channels, helper contexts, spawn requests, the op-0x49 submode block / screen, eased moves and the entry / free-roam latches.
pub struct FieldVmState {
    /// `_DAT_8007B868` - the dev/retail branch discriminator. **Retail is
    /// zero**; a non-zero value is the dev build's "not an ordinary field
    /// frame" state. Read by [`crate::world::World::man_load_actor_reset`] through
    /// [`crate::field_submode::scene_actor_initial_state`], which forces the
    /// spawned scene actor's state word to `1` when it is set. Mirrors of the
    /// same word live on [`crate::cd_dma`] and [`crate::overlay_loader`]; this
    /// is the field-side copy.
    pub mode_flags: u32,
    /// The op-`0x49` submode context block (`0x801F2734..`), in the order
    /// [`crate::field_submode::SUBMODE_CONTEXT_SEEDS`] lists its ten words -
    /// that const carries the offsets. Reseeded on every MAN load by
    /// [`crate::world::World::man_load_actor_reset`]; word `0` is the submode state
    /// ([`crate::field_submode::SUBMODE_STATE_OPEN`] once opened).
    pub submode_context: [u32; 10],
    /// `true` only while [`crate::world::World::pre_run_field_channel_prologues`] is
    /// executing the scene-entry spawn-prologue slices. The field-VM host
    /// reads it to give the prologue's `4C 51` NPC-run ops their load-time
    /// semantics (an initial SEAT written through the channel ctx) without
    /// touching the live free-roam / cutscene behaviour of the same op.
    pub entry_prerun: bool,
    /// Live eased-move records (field-VM op `0x43` sub-9 with a non-zero
    /// tick count, retail template `0x801F2840` / tick `FUN_801DD4C4`), each
    /// paired with the actor whose position triple it writes.
    pub eased_moves: Vec<crate::world::FieldEasedMove>,
    /// The op-`0x49` sub-screen the submode driver actor is running, if any.
    /// See [`crate::field_submode_screen`]; ticked by
    /// [`crate::world::World::tick_handler_actors`].
    pub submode_screen: crate::field_submode_screen::SubmodeScreen,
    /// Concurrent spawned-record contexts: partition-2 records spawned
    /// mid-play (field-VM op-`0x44` outside the opening chain) that execute
    /// as independent field-VM contexts, mirroring retail's per-record spawn
    /// (`FUN_8003BDE0` installs `ctx[+0x90]`/`ctx[+0x9E]` and lets the
    /// per-frame context sweep run it as a sibling). Unlike
    /// [`crate::world::CutsceneState::timeline`] these never seize the camera or lock
    /// player locomotion ([`crate::world::World::cutscene_timeline_active`] does not cover
    /// them); only cutscene-class records - the opening chain and gated
    /// walk-on beat records - install as the modal timeline. Installed by
    /// [`crate::world::World::install_spawned_helper_record`], stepped per frame by
    /// [`crate::world::World::step_helper_contexts`], bounded by
    /// [`crate::world::SPAWNED_CONTEXT_SLOTS`] (retail's context table is a small fixed
    /// actor-slot pool). A completed context is dropped the frame it ends.
    pub helper_contexts: Vec<crate::cutscene_timeline::CutsceneTimeline>,
    /// Set by [`crate::world::World::seed_free_roam_story_baseline`] for scene-picker /
    /// `--scene` entries: the world was staged for a free-roam visit with no
    /// story behind it, so the BGM host arm drops entry-window pauses (their
    /// authored repair - the opening records' sub-9 restarts - never runs
    /// here). The new-game / opening chain leaves this `false`.
    pub free_roam_staging: bool,
    /// [`crate::world::FrameClock::display_frames`] at the most recent free-roam staging / scene
    /// entry - the base of the entry window the pause-drop measures against.
    pub free_roam_entry_frame: u64,
    /// Per-actor field-VM channels: one spawned context per MAN partition-1
    /// placement record, mirroring the retail per-record spawn
    /// (`FUN_8003A1E4`). Spawned alongside a cutscene timeline
    /// ([`crate::world::World::install_cutscene_timeline_record`]) so the timeline's
    /// cross-context pokes (flag writes, animate cues, moves) land on real
    /// per-actor contexts - the opening prologue's vignette mechanism.
    /// Stepped run-until-yield per frame by [`crate::world::World::step_field_channels`].
    pub channels: Vec<crate::field_channels::FieldChannel>,
    /// A copy of [`Self::channels`] taken when a stepping pass moves the live
    /// vector out (`std::mem::take` in the channel and spawned-record
    /// steppers), so a host hook that resolves a cross-context id while a
    /// channel is executing - `FUN_8003C83C`'s actor-list walk, which in
    /// retail always sees every actor - still finds the other channels.
    /// Empty outside a stepping pass. Read it through
    /// [`crate::world::World::channel_view`], never directly.
    pub stepping_view: Vec<crate::field_channels::FieldChannel>,
    /// The MAN payload the channels' bytecode slices from (each channel's
    /// buffer base is its `record_offset` into this).
    pub channels_man: Option<std::sync::Arc<Vec<u8>>>,
    /// Placement index of the channel context currently executing (its own
    /// slice in [`crate::world::World::step_field_channels`], or the target of a
    /// cross-context poke from the cutscene timeline), so field-VM host hooks
    /// (animate, move) can attribute the side-effect to that placement's NPC.
    /// `None` outside a channel-targeted step.
    pub executing_channel: Option<u8>,
    /// `true` while [`crate::world::World::run_spawned_record_slice`] is stepping a spawned
    /// partition-2 record context (the modal cutscene timeline or a
    /// concurrent helper context). Host hooks use it to distinguish a
    /// spawned record's cross-context channel poke (seat the target exactly -
    /// the retail run settles on the op target) from the live channel
    /// stepper's own-script op (glide).
    pub in_spawned_record_slice: bool,
    /// The scene's `.MAP` object script binds
    /// (`(flat_record_index, contact_centre)`, retail `FUN_8003A55C`),
    /// stored at scene entry so a cutscene-timeline install that has to
    /// respawn the channel set can re-append the object-bind channels
    /// ([`crate::field_channels::spawn_object_channels`]).
    pub object_channel_binds: Vec<(usize, (i16, i16))>,
    /// Pending field-VM op-`0x44` SPAWN_RECORD requests: the GLOBAL record
    /// indices whose partition-2 records should spawn as new contexts.
    /// Recorded by the host hook (the VM borrow precludes resolving the MAN
    /// there); drained FIFO by `SceneHost::tick`, which re-bases each into
    /// partition 2 (`global - N0 - N1`, retail `FUN_8003BDE0`) and installs
    /// the record - as the modal cutscene timeline during the opening chain,
    /// as a concurrent [`crate::world::FieldVmState::helper_contexts`] entry otherwise - when its
    /// C1/C2 story-flag gates pass. A queue (bounded by
    /// [`crate::world::SPAWNED_CONTEXT_SLOTS`]) so a second spawn issued while another
    /// record executes is not dropped.
    pub pending_record_spawns: Vec<u8>,
}

impl FieldVmState {
    pub fn new() -> Self {
        Self {
            mode_flags: 0,
            submode_context: [0; 10],
            entry_prerun: false,
            eased_moves: Vec::new(),
            submode_screen: crate::field_submode_screen::SubmodeScreen::default(),
            helper_contexts: Vec::new(),
            free_roam_staging: false,
            free_roam_entry_frame: 0,
            channels: Vec::new(),
            stepping_view: Vec::new(),
            channels_man: None,
            executing_channel: None,
            in_spawned_record_slice: false,
            object_channel_binds: Vec::new(),
            pending_record_spawns: Vec::new(),
        }
    }
}

impl Default for FieldVmState {
    fn default() -> Self {
        Self::new()
    }
}
