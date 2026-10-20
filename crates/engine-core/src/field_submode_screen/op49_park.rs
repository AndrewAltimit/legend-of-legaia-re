//! The op-`0x49` park: which field-VM context owns it, when a scripted menu open is pending,
//! and the board entries the parked instruction carries.

use super::*;

impl World {
    /// Which field-VM context is stepping right now - the owner an op-`0x49`
    /// park armed on this step belongs to, and the only one that may read it.
    ///
    /// [`crate::world::FieldVmState::in_spawned_record_slice`] is set by `run_spawned_record_slice`
    /// for both spawned-record shapes, and [`crate::world::CutsceneState::in_timeline`]
    /// only for the modal one, so the pair separates all three contexts.
    pub fn op49_park_owner(&self) -> Op49ParkOwner {
        if !self.field_vm.in_spawned_record_slice {
            Op49ParkOwner::FieldScript
        } else if self.cutscene.in_timeline {
            Op49ParkOwner::CutsceneTimeline
        } else {
            Op49ParkOwner::HelperContext
        }
    }

    /// The part of the subsystem actor's installer `FUN_801F1278` an
    /// op-`0x49` park changes on screen. Retail spawns the actor on the op's
    /// Idle arm and runs the installer on its first dispatch, with the
    /// operand pointer `_DAT_8007B450` live - so every arm takes the park
    /// branch.
    ///
    /// - **The player stands.** The clip base `_DAT_8007BDD8 = 2`, the
    ///   player's `+0x5C = 2 + leader * 7` and the party-bank bit
    ///   (`0x801F1318..0x801F1354`): the leader's idle clip, whatever the
    ///   player was playing when the script opened the screen.
    /// - **A held box comes down.** The rewind at `0x801F1434..0x801F144C`
    ///   takes the pager from its idle `4` - a `0x24` continuation, whose rows
    ///   the pager goes on drawing - to `1`, which draws nothing.
    ///
    /// PORT: FUN_801F1278 (the park-arm writes the engine mirrors)
    pub(crate) fn run_submode_installer(&mut self) {
        use legaia_engine_field::field_subsystem_enter::{
            CTX_PARTY_BANK, enter_context, rearm_state,
        };
        let leader = self.locomotion.player_anim.as_ref().map_or(0, |a| a.leader);
        let flags = if self.locomotion.player_party_bank {
            CTX_PARTY_BANK
        } else {
            0
        };
        let ctx = enter_context(flags, 0, leader);
        self.field_player_installer_clip(
            ctx.clip_base,
            ctx.player_clip,
            ctx.flags & CTX_PARTY_BANK != 0,
        );
        // One pager, one state word: whichever context's box it is still
        // drawing takes the rewind.
        let rewind = |held: &mut Option<legaia_engine_field::inline_dialogue::HeldBox>| {
            if let Some(state) = held.as_ref().and_then(|h| h.pager_state())
                && rearm_state(state) != state
            {
                *held = None;
            }
        };
        if let Some(inline) = self.dialog.inline.as_mut() {
            rewind(&mut inline.held_box);
        }
        if let Some(tl) = self.cutscene.timeline.as_mut() {
            rewind(&mut tl.held_box);
        }
        for tl in &mut self.field_vm.helper_contexts {
            rewind(&mut tl.held_box);
        }
    }

    /// Record an op-`0x49` arm's **kind byte** - retail's
    /// `_DAT_8007B450 = operand`, reduced to the one byte every consumer
    /// dereferences.
    ///
    /// Called from the field VM's arm edge for every sub-op, so the three
    /// the port resolves through dedicated host paths are recorded too;
    /// retail's store happens before any of those paths would diverge.
    ///
    /// The owner tag is the same one [`World::open_field_submode_screen`]
    /// applies, for the same reason: retail has one global and the port
    /// steps several field-VM contexts inside one `World::tick`, so a park
    /// armed by one context must not answer another context's question.
    ///
    /// PORT: FUN_801DE840 (`0x801e0984` / `0x801e09a8`)
    pub fn record_op49_park(&mut self, sub_op: u8) {
        self.field_vm.submode_screen.owner = self.op49_park_owner();
        self.field_vm.submode_screen.park_sub_op = Some(sub_op);
        // Every Idle arm spawns a fresh subsystem actor in retail, so every
        // arm is a fresh menu-button press.
        self.field_vm.submode_screen.scripted_menu_opened = false;
    }

    /// Whether an op-`0x49` park is a **scripted menu-button press** that no
    /// host has answered yet - the cue for a host to open the pause menu
    /// without a Start edge.
    ///
    /// The op's Idle arm spawns the same subsystem actor the locomotion
    /// controller's menu accept spawns: `FUN_80020DE0(0x8007065C,
    /// *0x8007C34C)` at `0x801E0998..0x801E09A4`, against the pad path's
    /// call at `0x801D0324`. That actor's enter half `FUN_801F1278` stores
    /// handler id `7` into `+0x50` (`0x801F140C`) and zeroes `+0x54`
    /// (`0x801F141C`) **before** it reads the sub-op table at `0x801F33A4`
    /// (`0x801F1468`); a `-1` row only skips the overwrite (`0x801F1470`), so
    /// handler `7` stands. Handler `7` is the state pick `FUN_801F1F4C`,
    /// which with a park live always picks `0x30`, the pause-menu session
    /// `FUN_801ED308`. So a `-1` row the port does not route elsewhere -
    /// sub-op `1`, a save point, and sub-op `0x0D` - opens the menu by
    /// itself, and the entry decode then picks its first screen off the kind
    /// byte ([`legaia_engine_menus::pause_screens::menu_entry_subscreen`]).
    ///
    /// Not under the opening narration crawl or a title card: the timeline
    /// owns the scene there and the press waits for it to end. Both halves
    /// live here so every host asks the same question - the native window
    /// once added the crawl term beside this call and the browser page did
    /// not.
    ///
    /// Only the field-run modes the menu can open in. The player's
    /// engagement does **not** refuse it, unlike the Start path: the press
    /// comes from the interaction the dialog SM is running, and the
    /// subsystem actor is not the locomotion controller whose engaged-bit
    /// branch (`0x801D01F0`) guards the pad accept.
    ///
    /// PORT: FUN_801F1278 (the `-1`-row arm: `+0x50 = 7`, then the table read)
    /// REF: FUN_801F1F4C, FUN_801ED308, FUN_80020DE0
    pub fn scripted_menu_open_pending(&self) -> bool {
        let s = &self.field_vm.submode_screen;
        s.park_sub_op.is_some_and(op49_sub_op_is_menu_press)
            && !s.scripted_menu_opened
            && self.scene_mode_takes_menu_open()
            && self.cutscene.card.is_none()
            && !self.cutscene_narration_active()
    }

    /// Spend the pending scripted menu press: a host opened the menu for it.
    pub fn note_scripted_menu_opened(&mut self) {
        self.field_vm.submode_screen.scripted_menu_opened = true;
    }

    /// Record the op-`0x49` operand's payload bytes for the screen this arm
    /// opens - retail's `_DAT_8007B450 + 1..=3`, read through the parked
    /// pointer by [`legaia_engine_field::submode_hub_actors::start_menu`].
    ///
    /// `instr` is the instruction from its opcode byte, so `instr[1]` is the
    /// sub-op (the byte the park points at) and `instr[2..=4]` are the three
    /// the menu counts.
    pub fn set_submode_board_entries(&mut self, instr: &[u8]) {
        let mut out = [0u8; 3];
        for (i, slot) in out.iter_mut().enumerate() {
            *slot = instr.get(2 + i).copied().unwrap_or(0);
        }
        self.field_vm.submode_screen.board_entries = out;
        let mut op = [0u8; 7];
        for (i, b) in op.iter_mut().enumerate() {
            *b = instr.get(1 + i).copied().unwrap_or(0);
        }
        self.field_vm.submode_screen.op49_operand = op;
    }

    /// Clear the park on resume - retail's `sw zero,-0x4bb0(s0)` at
    /// `0x801e08d8`, which runs on the Done edge before the PC advances.
    ///
    /// Only the context that armed the park may clear it, for the same
    /// reason it is the only one that may read it.
    pub fn clear_op49_park(&mut self) {
        if self.field_vm.submode_screen.owner == self.op49_park_owner() {
            self.field_vm.submode_screen.park_sub_op = None;
        }
    }
}
