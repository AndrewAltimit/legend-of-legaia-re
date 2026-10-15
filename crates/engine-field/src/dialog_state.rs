//! Field dialogue state: the simplified dialog panel, the inline field-VM dialogue runner and the interact / talk latches.
//!
//! One subsystem's state, held as a unit on the composite `World`.
//! Fields keep their retail provenance notes.

/// Field dialogue state: the simplified dialog panel, the inline field-VM dialogue runner and the interact / talk latches.
pub struct DialogState {
    /// Active dialog request - populated by the field-VM op 0x3F handler,
    /// cleared by the engine after the user dismisses the box. The MES
    /// renderer reads `text_id` + `inline`; the world-coords + depth feed
    /// the box placement.
    pub current: Option<DialogRequest>,
    /// Active 3-actor talk session (field-VM op `0x43` sub-2; retail talk
    /// controller from `FUN_801D2D38`). Refreshed on every sub-2
    /// instruction; the paired system flag `0xD` is the retail talk-active
    /// lock. See [`ThreeActorTalk`].
    pub three_actor_talk: Option<ThreeActorTalk>,
    /// Host-latched "switch character" request for the active three-actor
    /// talk - the engine input standing in for retail's pad-derived word
    /// `_DAT_8007B874` bit `0x80` (the request route of `FUN_801D27E0`'s
    /// state-0 arm gate). Hosts latch it from their pad handler via
    /// `World::request_talk_leader_switch`; the controller poll
    /// (`World::tick_three_actor_talk`) consumes it on its next state-0
    /// frame and drops it when no talk is live.
    pub talk_switch_requested: bool,
    /// Last `field_interact` request. Cleared by the engine when handled
    /// (set to `None`).
    pub last_field_interact: Option<(u8, u8)>,
    /// The interaction-prologue record for the dialogue `World::trigger_field_interact`
    /// most recently opened (taken by `World::drive_inline_dialogue` when it
    /// starts the runner). `None` when the opened NPC has no prologue record.
    pub active_inline_prologue: Option<crate::man_field_scripts::InlineDialogPrologue>,
    /// While `World::step_inline_dialogue` is stepping the field VM over an
    /// NPC's interaction record, this carries that NPC's placement slot so the
    /// `0x4C 0x51` NPC-run host hook can route the walk to the right actor
    /// (the engine's stand-in for retail's per-actor script context pointer).
    pub stepping_inline_npc: Option<u8>,
    /// The placement slot `World::trigger_field_interact` most recently
    /// opened a dialogue for; consumed by `World::drive_inline_dialogue` so
    /// the inline runner knows which NPC its record belongs to.
    pub active_inline_slot: Option<u8>,
    /// Per-tick guard: set when a Cross/Circle press is consumed by a field
    /// dialogue open or dismiss this tick, so the script's `0x4C` dialog poll
    /// and the interaction probe can't both act on the same edge (double
    /// open/dismiss). Reset at the top of each [`legaia_engine_mode::mode::SceneMode::Field`] tick.
    pub input_consumed: bool,
    /// A running inline interaction script driven through the field VM (the
    /// faithful dialogue path). Opt-in alternative to the simplified
    /// [`DialogState::current`] / `OwnedDialogPanel` path: it *executes* the
    /// prologue flag tests, branch flag-sets, and scene changes between text
    /// boxes. See [`crate::inline_dialogue`] and `World::step_inline_dialogue`.
    pub inline: Option<crate::inline_dialogue::InlineDialogue>,
    /// The pager's automatic-press countdown `_DAT_80073F00`: field-VM op
    /// `4C 89` writes it, and a box page waiting for confirm counts it down
    /// by the frame step and presses for the player when it runs out
    /// ([`legaia_engine_dialog::dialog::OwnedDialogPanel::tick_at_auto`]). `<= 0` is idle.
    pub auto_press: i16,
    /// The cursor sprite primitive's per-kind frame / timer state
    /// (`0x801C6000..0x801C6020`, [`legaia_engine_dialog::cursor_sprite`]) the pager's page
    /// mark and option hand animate on. Global in retail, so it outlives a
    /// box: the next page at the same height carries the mark's phase on.
    pub cursor_sprites: legaia_engine_dialog::cursor_sprite::CursorSprites,
    /// A page-mark frame seated over the engine's own
    /// (`World::seat_page_mark_frame`); `None` outside the
    /// retail comparison's capture frame.
    pub page_mark_seat: Option<u8>,
    /// The next reading box's row capacity `_DAT_801F2740`, as field-VM op
    /// `4C E0 <n>` (`n` in `1..100`) set it; `3` otherwise. A box takes it
    /// when it opens and the pager's reset stores `3` again, so it lasts one
    /// box ([`legaia_engine_dialog::dialog::OwnedDialogPanel::set_window_format`]).
    pub window_rows: u8,
    /// The instant-page latch `_DAT_801F2744` (op `4C E0 00`): the next box
    /// loads its page whole.
    pub window_instant: bool,
}

impl DialogState {
    pub fn new() -> Self {
        Self {
            current: None,
            three_actor_talk: None,
            talk_switch_requested: false,
            last_field_interact: None,
            active_inline_prologue: None,
            stepping_inline_npc: None,
            active_inline_slot: None,
            input_consumed: false,
            inline: None,
            auto_press: 0,
            cursor_sprites: Default::default(),
            page_mark_seat: None,
            window_rows: legaia_engine_dialog::dialog_window::WINDOW_ROWS as u8,
            window_instant: false,
        }
    }
}

impl Default for DialogState {
    fn default() -> Self {
        Self::new()
    }
}

/// Active 3-actor talk session - the engine mirror of the talk-controller
/// actor `FUN_801D2D38` spawns from pool `0x801F22C4` for field-VM op
/// `0x43` sub-2 (three-way cutscene conversations).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ThreeActorTalk {
    /// The three participant ids from the instruction (retail resolves them
    /// through the actor-list walk `FUN_8003C83C` and stores the pointers at
    /// controller `+0x80/+0x84/+0x88`).
    pub actor_ids: [u8; 3],
    /// Controller script id (retail `+0x50` from the instruction's u16).
    pub script_id: u16,
    /// The instruction's record byte: the **global** index of the record
    /// every leader swap spawns (not a duration, whatever the field name).
    /// Retail stores `arg - a - b` at controller `+0x72`, where `(a, b)` is
    /// the scene-MAN header pair read via `FUN_8003D064(_DAT_8007B898 +
    /// 0x22)` - the first two partition counts - and the swap's
    /// `FUN_8003BDE0(x, z, +0x72, 1)` adds them back, so the engine keeps
    /// the raw operand and queues it as an op-`0x44` spawn.
    pub duration: u8,
    /// Positions + headings of the three participants captured when the
    /// talk armed. Retail: the controller SM's state 0 (`FUN_801D27E0`)
    /// writes the 3-record table at `0x800845E4`; a re-arm while the talk
    /// flag is up restores from it (`FUN_801D2D38`'s else-branch loop).
    pub saved: [Option<((i16, i16), i16)>; 3],
    /// Story-party actor slots as they stood BEFORE the first arm collapsed
    /// the list to its leader (`FUN_801D2D38`'s `count = 1, ids =
    /// [leader, 0, 0, 0]` stores). Only the first [`Self::saved_party_len`]
    /// entries are meaningful. Retail rebuilds the party through the scene
    /// script's own party ops after the talk; the engine restores this
    /// snapshot when the talk lock (system flag `0xD`) drops
    /// (`World::end_three_actor_talk`) so the collapse is never a one-way
    /// door, and any script party ops that follow still apply on top.
    /// Carried unchanged across re-arms (the re-arm branch never
    /// re-collapses).
    pub saved_party: [Option<u8>; 4],
    /// Number of live entries in [`Self::saved_party`].
    pub saved_party_len: u8,
    /// `PartyState::party_leader_slot` before the collapse.
    pub saved_leader: Option<u8>,
    /// The controller SM itself (`FUN_801D27E0` `+0x54` phase + `+0x9E` fade
    /// counter): state 0 polls the talk lock and arms the mid-talk leader
    /// switch, states 1..=4 run the fade-out / swap / fade-in cycle, state 5
    /// despawns. Reset to phase 0 on every op-`0x43` sub-2 (re)arm - each arm
    /// spawns a fresh controller in retail. Its `flag_base` is
    /// [`Self::script_id`] (controller `+0x50`), the base the presence flags
    /// `flag_base + 0..=2` are tested against. Stepped by
    /// `World::tick_three_actor_talk`.
    pub swap: crate::leader_swap::LeaderSwap,
}

/// Pending dialog request for the field-VM op 0x3F handler. The engine
/// renders + advances; clearing `World::dialog.current` signals the script
/// to resume.
#[derive(Debug, Clone, PartialEq)]
pub struct DialogRequest {
    pub text_id: u16,
    pub inline: Vec<u8>,
    pub world_x: u16,
    pub world_z: u16,
    pub depth_id: u8,
}
