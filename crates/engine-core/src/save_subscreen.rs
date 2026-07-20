//! Save-UI sub-screen graph.
//!
//! PORT: FUN_801DC6B4 (outer dispatcher), FUN_801E4F40 (sub-screen pointer table)
//!
//! The retail save UI is not one screen but a graph of small step
//! machines. An outer dispatcher runs a fade-in / dispatch / fade-out
//! cycle, and the dispatch case indirects through a pointer table into
//! whichever sub-screen is current. Each sub-screen owns a step counter,
//! invokes an actor-VM display script on its first step, waits for that
//! script to go idle, then either advances its own step or writes a new
//! sub-screen id - which is how control moves through the graph.
//!
//! Two globals carry all of it: the sub-screen id and the step counter.
//! A sub-screen never returns a value; it *is* the transition, by writing
//! the id global. That makes the graph a plain state machine once lifted
//! out of the pointer-table indirection, which is what this module is.
//!
//! ## What this models, and what it does not
//!
//! This is the **control flow** - which screen follows which, on what
//! input, and where the outer fade sits around it. The screens' *content*
//! (panels, slot previews, the info panel) is [`crate::save_select`],
//! which models the same UI as player-facing phases rather than retail
//! ids. The two are complementary: a host drives the session for content
//! and can key retail-exact chrome off [`SaveSubScreen`].
//!
//! Sub-screens whose retail behaviour is not yet pinned are represented
//! in [`SaveSubScreen`] (so the id space stays complete and a transition
//! into one is expressible) but have no step machine here; ticking one
//! parks. See `docs/subsystems/save-screen.md` for the table.
//!
//! NOT WIRED: nothing constructs a [`SaveScreenMachine`] outside this
//! module's own tests. The engine's save UI runs on
//! [`crate::save_select`]'s player-facing phase model, which
//! `engine-shell`'s window driver does use; this module is the retail
//! control-flow mirror alongside it and no host keys off it yet. The
//! step machines below are therefore verified against the disassembly
//! but exercised only by unit tests.

/// Sub-screen ids, as indexed out of the retail pointer table.
///
/// The id space is the table's, so the discriminants are the retail
/// numbers and a transition can be written as the number the decompile
/// stores. Ids the table fills with screens whose behaviour is not yet
/// pinned are [`SaveSubScreen::Unpinned`], which keeps the space total.
///
/// PORT: FUN_801E4F40 (the table this indexes)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveSubScreen {
    /// `0x00` - terminal screen; exits the save flow with code 3.
    FinalExit,
    /// `0x01` - the slot selector.
    SlotSelect,
    /// `0x02` - save entry, reached from the pause menu.
    SaveEntry,
    /// `0x03` - Yes/No confirm, cursor defaulting to `No`.
    ConfirmYesNo,
    /// `0x04` - post-save "press any button" return.
    PostSaveReturn,
    /// `0x08` - message screen that waits for the pad to be *released*.
    PadReleaseWait,
    /// `0x0B` - Yes/No confirm whose Yes branch exits with code 4.
    ConfirmExit,
    /// `0x12` - scrollable party-count picker.
    PartyPicker,
    /// `0x17` - generic picker wrapper over slots `0..=9`.
    GenericPicker,
    /// `0x18` - save-card driver (RAM to card).
    CardSave,
    /// `0x19` - load-card driver (card to RAM).
    CardLoad,
    /// `0x1A` - save-slot confirm.
    SaveConfirm,
    /// `0x1E` - inventory spinner ahead of the quantity screen.
    QuantitySpinner,
    /// `0x20` - auto-save path.
    AutoSave,
    /// A table slot whose screen is not yet pinned. Carries its id so a
    /// transition into one round-trips.
    Unpinned(u8),
}

impl SaveSubScreen {
    /// The retail table index for this screen.
    pub fn id(self) -> u8 {
        match self {
            Self::FinalExit => 0x00,
            Self::SlotSelect => 0x01,
            Self::SaveEntry => 0x02,
            Self::ConfirmYesNo => 0x03,
            Self::PostSaveReturn => 0x04,
            Self::PadReleaseWait => 0x08,
            Self::ConfirmExit => 0x0B,
            Self::PartyPicker => 0x12,
            Self::GenericPicker => 0x17,
            Self::CardSave => 0x18,
            Self::CardLoad => 0x19,
            Self::SaveConfirm => 0x1A,
            Self::QuantitySpinner => 0x1E,
            Self::AutoSave => 0x20,
            Self::Unpinned(id) => id,
        }
    }

    /// Resolve a retail table index into a screen.
    pub fn from_id(id: u8) -> Self {
        match id {
            0x00 => Self::FinalExit,
            0x01 => Self::SlotSelect,
            0x02 => Self::SaveEntry,
            0x03 => Self::ConfirmYesNo,
            0x04 => Self::PostSaveReturn,
            0x08 => Self::PadReleaseWait,
            0x0B => Self::ConfirmExit,
            0x12 => Self::PartyPicker,
            0x17 => Self::GenericPicker,
            0x18 => Self::CardSave,
            0x19 => Self::CardLoad,
            0x1A => Self::SaveConfirm,
            0x1E => Self::QuantitySpinner,
            0x20 => Self::AutoSave,
            other => Self::Unpinned(other),
        }
    }

    /// Whether this module carries a step machine for the screen.
    pub fn is_pinned(self) -> bool {
        !matches!(self, Self::Unpinned(_))
    }
}

/// What opened the save UI. Retail decodes an entry-context pointer into
/// the starting sub-screen; the pointer's *target byte* selects, except
/// for the sentinel value which is never dereferenced.
///
/// REF: FUN_801DC6B4 (state 0's entry-context decode).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveEntryContext {
    /// The sentinel pointer - opened from the pause menu to save.
    MenuSave,
    /// Context byte `0x01` - load a slot.
    Load,
    /// Context byte `0x07` - auto-save.
    AutoSave,
    /// Context byte `0x0D` - returning after a save completed.
    PostSave,
    /// Context byte `0x00` - cancelled / backing out.
    Cancel,
}

impl SaveEntryContext {
    /// The sub-screen this context opens on.
    pub fn start_screen(self) -> SaveSubScreen {
        match self {
            Self::MenuSave => SaveSubScreen::SaveEntry,
            Self::Load => SaveSubScreen::CardLoad,
            Self::AutoSave => SaveSubScreen::AutoSave,
            Self::PostSave => SaveSubScreen::PostSaveReturn,
            // `0x1A` is the save-confirm screen, which this module does
            // carry a step machine for. Naming it `Unpinned(0x1A)` here
            // would round-trip the id but compare unequal to
            // `SaveSubScreen::SaveConfirm`, so the dispatcher would park
            // instead of running the machine.
            Self::Cancel => SaveSubScreen::SaveConfirm,
        }
    }
}

/// Outer state-machine phase.
///
/// REF: FUN_801DC6B4 (the 9-case switch on its state global).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SavePhase {
    /// State 0 - one-shot init; seeds the fade and the start screen.
    Init,
    /// State 1 - waiting for the fade-in to clear the input threshold.
    FadeIn,
    /// State 2 - dispatching the current sub-screen every frame.
    Dispatch,
    /// States 3..5 - fading out, gated on the fade climbing back to
    /// [`FADE_OPAQUE`]. Each of the three adds `3` to reach a terminal
    /// state, which is how the exit code survives into `Done`.
    FadeOut,
    /// State >= 6 - terminal.
    Done,
}

/// Fade level retail seeds on init: fully opaque. `0` is transparent, so
/// the flow fades *in* from `FADE_OPAQUE` down to `0` and back *out* up to
/// `FADE_OPAQUE` on the way,  which is the direction both phases run.
pub const FADE_OPAQUE: u8 = 0xF2;

/// Fade level pad input is suppressed at or above.
///
/// Retail masks the pad globals while the level is `>= 0x7A` (`slti
/// 0x7a` guarding the mask block), so input reaches the sub-screens from
/// `0x79` down.
pub const FADE_INPUT_THRESHOLD: u8 = 0x7A;

/// Fade level the fade-in wait advances to dispatch below (`slti 0x79`).
///
/// One below [`FADE_INPUT_THRESHOLD`] - retail really does use two
/// distinct constants here, so dispatch starts on the first frame after
/// input has already been let through.
pub const FADE_DISPATCH_THRESHOLD: u8 = 0x79;

/// Outer state a sub-screen writes to end the flow normally.
///
/// Retail's "exit code" is a write to the dispatcher's own state global:
/// states `3..=5` are the fade-out entries, and each adds `3` to reach a
/// terminal state `>= 6`. So `3` and `4` are not return values but the
/// fade-out state the screen jumps the outer machine to, and they survive
/// into the terminal state as the record of how the flow ended.
pub const EXIT_CODE_NORMAL: u8 = 3;

/// Outer state the Yes-branch of the confirm-exit screen writes.
pub const EXIT_CODE_CONFIRMED: u8 = 4;

/// Per-frame inputs a sub-screen step machine reads.
///
/// Retail reads these from globals the actor VM and pad layer maintain;
/// bundling them keeps the step machines pure.
#[derive(Debug, Clone, Copy, Default)]
pub struct SubScreenInput {
    /// The display script is still running. Every wait step blocks on
    /// this going false.
    pub script_busy: bool,
    /// Any button is currently held. Two screens branch on it - one
    /// waits for a press, the other for a release.
    pub any_button_held: bool,
    /// The list navigator's result for screens that own a cursor:
    /// `1` confirm, `2` cancel, `3` moved, `0` none.
    pub nav: u8,
    /// The cursor's index, masked to its low 12 bits.
    pub cursor: u16,
    /// The card driver finished this frame.
    pub card_done: bool,
    /// At least one save block in the scanned range is both present and
    /// valid. The save-confirm screen refuses to proceed without one.
    pub save_blocks_available: bool,
    /// The spinner's outcome selector: `2` commits to the quantity
    /// screen, `3` re-runs the spinner's second display script.
    pub spinner_result: u8,
}

/// A side effect a step machine asks the host to perform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubScreenEffect {
    /// Run the sub-screen's display script (an actor-VM invocation).
    RunScript,
    /// Play a UI sound cue.
    Sfx(u8),
    /// Install the memory-card handle ahead of a card operation.
    InstallCardHandle,
    /// Drive the card transfer in this direction.
    CardOp(CardOp),
    /// Read the focused inventory entry into the screen's staging cells.
    ReadInventoryEntry,
    /// Zero the screen's staging cells and reset the list parameter.
    ClearStaging,
}

/// Direction of a card transfer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardOp {
    /// Card to RAM.
    Load,
    /// RAM to card.
    Save,
}

/// The save UI's outer dispatcher plus the current sub-screen's step
/// counter - the two globals that between them are the whole flow.
///
/// PORT: FUN_801DC6B4
#[derive(Debug, Clone)]
pub struct SaveScreenMachine {
    phase: SavePhase,
    screen: SaveSubScreen,
    step: u8,
    fade: u8,
    exit_code: Option<u8>,
    entry: SaveEntryContext,
}

impl SaveScreenMachine {
    /// Open the save UI from an entry context.
    pub fn new(entry: SaveEntryContext) -> Self {
        Self {
            phase: SavePhase::Init,
            screen: entry.start_screen(),
            step: 0,
            fade: FADE_OPAQUE,
            exit_code: None,
            entry,
        }
    }

    /// The entry context this flow opened on.
    pub fn entry(&self) -> SaveEntryContext {
        self.entry
    }

    /// The outer phase.
    pub fn phase(&self) -> SavePhase {
        self.phase
    }

    /// The current sub-screen.
    pub fn screen(&self) -> SaveSubScreen {
        self.screen
    }

    /// The current sub-screen's step counter.
    pub fn step(&self) -> u8 {
        self.step
    }

    /// Current fade level; `0` is transparent.
    pub fn fade(&self) -> u8 {
        self.fade
    }

    /// Whether the flow has terminated.
    pub fn is_done(&self) -> bool {
        self.phase == SavePhase::Done
    }

    /// The exit code a sub-screen wrote, once one has.
    pub fn exit_code(&self) -> Option<u8> {
        self.exit_code
    }

    /// Whether pad input reaches the sub-screens this frame. Retail
    /// suppresses it while the fade is still above the threshold.
    pub fn input_active(&self) -> bool {
        self.fade < FADE_INPUT_THRESHOLD
    }

    /// Force a sub-screen transition, resetting the step counter the way
    /// a retail screen's id write does.
    pub fn goto(&mut self, screen: SaveSubScreen) {
        self.screen = screen;
        self.step = 0;
    }

    /// Advance one frame, returning whatever effects the current
    /// sub-screen asked for.
    ///
    /// `fade_delta` is how much the fade level drops this frame; retail
    /// runs the fade on its own timer, so the caller owns its rate.
    pub fn tick(&mut self, input: SubScreenInput, fade_delta: u8) -> Vec<SubScreenEffect> {
        match self.phase {
            SavePhase::Init => {
                // Retail's init seeds a full fade and decodes the entry
                // context into the starting screen, then falls straight
                // through to the fade-in wait.
                self.fade = FADE_OPAQUE;
                self.screen = self.entry.start_screen();
                self.step = 0;
                self.phase = SavePhase::FadeIn;
                Vec::new()
            }
            SavePhase::FadeIn => {
                self.fade = self.fade.saturating_sub(fade_delta);
                if self.fade < FADE_DISPATCH_THRESHOLD {
                    self.phase = SavePhase::Dispatch;
                }
                Vec::new()
            }
            SavePhase::Dispatch => {
                let effects = self.dispatch(input);
                if self.exit_code.is_some() {
                    self.phase = SavePhase::FadeOut;
                }
                effects
            }
            SavePhase::FadeOut => {
                // The fade-out ramps back *up* to opaque; retail's exiting
                // screen flips the fade delta positive and the fade-out
                // state completes on `fade >= 0xF2`, not on zero.
                self.fade = self.fade.saturating_add(fade_delta).min(FADE_OPAQUE);
                if self.fade >= FADE_OPAQUE {
                    self.phase = SavePhase::Done;
                }
                Vec::new()
            }
            SavePhase::Done => Vec::new(),
        }
    }

    /// Run the current sub-screen's step machine for one frame.
    fn dispatch(&mut self, input: SubScreenInput) -> Vec<SubScreenEffect> {
        match self.screen {
            SaveSubScreen::FinalExit => self.tick_final_exit(input),
            SaveSubScreen::ConfirmYesNo => self.tick_confirm_yes_no(input),
            SaveSubScreen::PostSaveReturn => self.tick_post_save_return(input),
            SaveSubScreen::PadReleaseWait => self.tick_pad_release_wait(input),
            SaveSubScreen::ConfirmExit => self.tick_confirm_exit(input),
            SaveSubScreen::PartyPicker => self.tick_party_picker(input),
            SaveSubScreen::CardSave => self.tick_card_driver(input, CardOp::Save),
            SaveSubScreen::CardLoad => self.tick_card_driver(input, CardOp::Load),
            SaveSubScreen::SaveConfirm => self.tick_save_confirm(input),
            SaveSubScreen::QuantitySpinner => self.tick_quantity_spinner(input),
            // Screens with no step machine here park rather than
            // transitioning; a host drives them through `goto`.
            _ => Vec::new(),
        }
    }

    /// Sub-screen `0x00`: run the terminal display script, then exit.
    ///
    /// PORT: FUN_801DD12C
    fn tick_final_exit(&mut self, input: SubScreenInput) -> Vec<SubScreenEffect> {
        match self.step {
            0 => {
                self.step = 1;
                vec![SubScreenEffect::RunScript]
            }
            1 if !input.script_busy => {
                // Retail writes `0xF2` to the fade *delta*, not the fade
                // level - it flips the ramp positive so the fade-out
                // phase climbs back to opaque. Slamming the level here
                // would end the fade-out on its first frame.
                self.exit_code = Some(EXIT_CODE_NORMAL);
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    /// Sub-screen `0x03`: Yes/No confirm defaulting to `No`.
    ///
    /// The cursor seeds to `1`, and confirming on `1` returns to the slot
    /// selector while confirming on `0` falls through to the terminal
    /// screen. Cancel returns the same way `1` does.
    ///
    /// PORT: FUN_801D6D38
    fn tick_confirm_yes_no(&mut self, input: SubScreenInput) -> Vec<SubScreenEffect> {
        match self.step {
            0 => {
                self.step = 1;
                vec![SubScreenEffect::RunScript]
            }
            1 if !input.script_busy => match input.nav {
                1 => {
                    // Retail writes the exit screen first and overwrites
                    // it when the cursor sits on the default row, so the
                    // exit is the fallthrough, not the choice.
                    let next = if input.cursor & 0xFFF == 1 {
                        SaveSubScreen::SlotSelect
                    } else {
                        SaveSubScreen::FinalExit
                    };
                    self.goto(next);
                    vec![SubScreenEffect::Sfx(0x20)]
                }
                2 => {
                    self.goto(SaveSubScreen::SlotSelect);
                    Vec::new()
                }
                _ => Vec::new(),
            },
            _ => Vec::new(),
        }
    }

    /// Sub-screen `0x04`: "press any button" after a save.
    ///
    /// The wait is for a button to go *down*, unlike the release-wait
    /// screen; that is the only difference between the two.
    ///
    /// PORT: FUN_801DD1B8
    fn tick_post_save_return(&mut self, input: SubScreenInput) -> Vec<SubScreenEffect> {
        match self.step {
            0 => {
                self.step = 1;
                vec![SubScreenEffect::RunScript]
            }
            1 if !input.script_busy && input.any_button_held => {
                self.goto(SaveSubScreen::SlotSelect);
                vec![SubScreenEffect::Sfx(0x20)]
            }
            _ => Vec::new(),
        }
    }

    /// Sub-screen `0x08`: message screen that waits for the pad to be
    /// released before moving on.
    ///
    /// PORT: FUN_801DD26C
    fn tick_pad_release_wait(&mut self, input: SubScreenInput) -> Vec<SubScreenEffect> {
        match self.step {
            0 => {
                self.step = 1;
                vec![SubScreenEffect::RunScript]
            }
            1 if !input.script_busy && !input.any_button_held => {
                self.goto(SaveSubScreen::Unpinned(0x05));
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    /// Sub-screen `0x0B`: Yes/No confirm whose Yes branch plays a second
    /// display script and then exits with the confirmed code.
    ///
    /// PORT: FUN_801D8A58
    fn tick_confirm_exit(&mut self, input: SubScreenInput) -> Vec<SubScreenEffect> {
        match self.step {
            0 => {
                self.step = 1;
                vec![SubScreenEffect::RunScript]
            }
            1 if !input.script_busy => match input.nav {
                1 if input.cursor & 0xFFF == 0 => {
                    // Yes: play the confirm script and advance to the
                    // wait step rather than leaving the screen.
                    self.step = 2;
                    vec![SubScreenEffect::RunScript, SubScreenEffect::Sfx(0x88)]
                }
                1 | 2 => {
                    self.goto(SaveSubScreen::Unpinned(0x06));
                    Vec::new()
                }
                _ => Vec::new(),
            },
            2 if !input.script_busy => {
                // As in `tick_final_exit`: the `0xF2` retail writes here
                // is the fade delta, not the level.
                self.exit_code = Some(EXIT_CODE_CONFIRMED);
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    /// Sub-screen `0x12`: the scrollable party-count picker.
    ///
    /// PORT: FUN_801D98F0
    fn tick_party_picker(&mut self, input: SubScreenInput) -> Vec<SubScreenEffect> {
        match self.step {
            0 => {
                self.step = 1;
                vec![SubScreenEffect::RunScript]
            }
            1 if !input.script_busy => match input.nav {
                1 => {
                    self.goto(SaveSubScreen::Unpinned(0x13));
                    vec![SubScreenEffect::Sfx(0x20)]
                }
                2 => {
                    self.goto(SaveSubScreen::SlotSelect);
                    Vec::new()
                }
                _ => Vec::new(),
            },
            _ => Vec::new(),
        }
    }

    /// Sub-screens `0x18` / `0x19`: the card drivers.
    ///
    /// The two are the same four-step machine; only the transfer
    /// direction differs - retail passes it as the card driver's second
    /// argument (`2` save, `1` load) - which is why they share one
    /// implementation. Both return to the slot selector when the
    /// transfer lands.
    ///
    /// One retail asymmetry is not modelled: the load driver's final
    /// step re-tests a status word and leaves for the terminal screen
    /// instead of the selector when it is clear, where the save driver
    /// has no such branch. Modelling it needs an input this struct does
    /// not carry, so the shared machine always takes the selector exit.
    ///
    /// PORT: FUN_801DAE24 (save), FUN_801DAEF4 (load)
    fn tick_card_driver(&mut self, input: SubScreenInput, op: CardOp) -> Vec<SubScreenEffect> {
        match self.step {
            0 => {
                self.step = 1;
                vec![
                    SubScreenEffect::InstallCardHandle,
                    SubScreenEffect::RunScript,
                ]
            }
            1 => {
                if !input.script_busy {
                    self.step = 2;
                }
                Vec::new()
            }
            2 => {
                if input.card_done {
                    self.step = 3;
                }
                vec![SubScreenEffect::CardOp(op)]
            }
            3 => {
                self.goto(SaveSubScreen::SlotSelect);
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    /// Sub-screen `0x1A`: the save-slot confirm, a three-row list.
    ///
    /// The rows do not share an exit. Row `2` and the cancel button both
    /// leave for the terminal screen; row `0` leaves for the card-full
    /// error screen; row `1` is the only one that can *proceed*, and only
    /// after a scan finds a save block that is both present and valid.
    /// Failing that scan plays an error cue and leaves the screen where
    /// it is - retail does not fall through to a transition.
    ///
    /// The row-`2` exit fires *two* audio calls in retail - the same cue
    /// entry point the other screens use, with id `0`, plus a second
    /// routine with `0x37`. Only the `0x37` one is modelled here, since
    /// [`SubScreenEffect::Sfx`] does not distinguish the two entry
    /// points.
    ///
    /// PORT: FUN_801DAFD4
    fn tick_save_confirm(&mut self, input: SubScreenInput) -> Vec<SubScreenEffect> {
        match self.step {
            0 => {
                self.step = 1;
                vec![SubScreenEffect::ClearStaging, SubScreenEffect::RunScript]
            }
            1 if !input.script_busy => match (input.nav, input.cursor & 0xFFF) {
                (1, 0) => {
                    self.goto(SaveSubScreen::Unpinned(0x1B));
                    vec![SubScreenEffect::ClearStaging]
                }
                (1, 1) => {
                    if input.save_blocks_available {
                        self.step = 2;
                        vec![SubScreenEffect::RunScript]
                    } else {
                        vec![SubScreenEffect::Sfx(0x23)]
                    }
                }
                (1, _) => {
                    self.goto(SaveSubScreen::FinalExit);
                    vec![SubScreenEffect::Sfx(0x37)]
                }
                (2, _) => {
                    self.goto(SaveSubScreen::FinalExit);
                    Vec::new()
                }
                _ => Vec::new(),
            },
            2 if !input.script_busy => {
                self.goto(SaveSubScreen::QuantitySpinner);
                vec![SubScreenEffect::ClearStaging]
            }
            _ => Vec::new(),
        }
    }

    /// Sub-screen `0x1E`: the inventory spinner ahead of the quantity
    /// screen.
    ///
    /// Steps 1 and 2 both land on the staging read, so the screen reads
    /// the focused inventory entry on the frame it settles and every
    /// frame it re-runs.
    ///
    /// PORT: FUN_801DBC5C
    fn tick_quantity_spinner(&mut self, input: SubScreenInput) -> Vec<SubScreenEffect> {
        match self.step {
            0 => {
                self.step = 1;
                return vec![SubScreenEffect::RunScript];
            }
            1 if input.script_busy => return Vec::new(),
            1 => self.step = 2,
            2 => {}
            3 => {
                if !input.script_busy {
                    self.goto(SaveSubScreen::SaveConfirm);
                }
                return Vec::new();
            }
            _ => return Vec::new(),
        }

        // Steps 1-settling and 2 share the staging read and the outcome
        // branch below.
        let mut effects = vec![SubScreenEffect::ReadInventoryEntry];
        match input.spinner_result {
            3 => {
                self.step = 3;
                effects.push(SubScreenEffect::RunScript);
            }
            2 => self.goto(SaveSubScreen::Unpinned(0x1F)),
            _ => {}
        }
        effects
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idle() -> SubScreenInput {
        SubScreenInput::default()
    }

    /// Every id the table indexes round-trips, including the ones with no
    /// step machine - the enum has to keep the space total or a retail
    /// transition into an unpinned screen would be inexpressible.
    #[test]
    fn subscreen_ids_round_trip() {
        for id in 0u8..=0x20 {
            assert_eq!(SaveSubScreen::from_id(id).id(), id, "id {id:#04x}");
        }
        assert!(!SaveSubScreen::from_id(0x05).is_pinned());
        assert!(SaveSubScreen::from_id(0x18).is_pinned());
    }

    /// The entry context picks the opening screen, which is what makes
    /// the same overlay serve Load, Save and auto-save.
    #[test]
    fn entry_context_picks_the_start_screen() {
        assert_eq!(
            SaveEntryContext::MenuSave.start_screen(),
            SaveSubScreen::SaveEntry
        );
        assert_eq!(
            SaveEntryContext::Load.start_screen(),
            SaveSubScreen::CardLoad
        );
        assert_eq!(
            SaveEntryContext::AutoSave.start_screen(),
            SaveSubScreen::AutoSave
        );
        assert_eq!(
            SaveEntryContext::PostSave.start_screen(),
            SaveSubScreen::PostSaveReturn
        );
    }

    /// Input is gated on the fade, so a machine that has only just opened
    /// must not dispatch pad reads.
    #[test]
    fn fade_gates_input_then_dispatch_starts() {
        let mut m = SaveScreenMachine::new(SaveEntryContext::MenuSave);
        m.tick(idle(), 0x10); // Init
        assert_eq!(m.phase(), SavePhase::FadeIn);
        assert!(!m.input_active());
        // Fade down past the threshold.
        while m.phase() == SavePhase::FadeIn {
            m.tick(idle(), 0x10);
        }
        assert_eq!(m.phase(), SavePhase::Dispatch);
        assert!(m.input_active());
    }

    fn dispatching(entry: SaveEntryContext) -> SaveScreenMachine {
        let mut m = SaveScreenMachine::new(entry);
        while m.phase() != SavePhase::Dispatch {
            m.tick(idle(), 0x40);
        }
        m
    }

    /// The terminal screen runs its script, waits for it, then ends the
    /// flow with the normal exit code and a re-opaqued fade.
    #[test]
    fn final_exit_runs_script_then_exits() {
        let mut m = dispatching(SaveEntryContext::MenuSave);
        m.goto(SaveSubScreen::FinalExit);

        let fx = m.tick(idle(), 0);
        assert_eq!(fx, vec![SubScreenEffect::RunScript]);
        assert_eq!(m.step(), 1);

        // Still busy: nothing happens.
        m.tick(
            SubScreenInput {
                script_busy: true,
                ..idle()
            },
            0,
        );
        assert!(m.exit_code().is_none());

        m.tick(idle(), 0);
        assert_eq!(m.exit_code(), Some(EXIT_CODE_NORMAL));
        // The screen writes the fade *delta*, so the level is unchanged
        // at the point the exit lands; the fade-out phase is what walks
        // it back up to opaque.
        assert!(m.fade() < FADE_DISPATCH_THRESHOLD);
        assert_eq!(m.phase(), SavePhase::FadeOut);
    }

    /// Confirming on the default row returns to the slot selector;
    /// confirming on the other row falls through to the exit screen.
    /// Retail writes the exit first and overwrites it, so getting this
    /// backwards is the easy mistake.
    #[test]
    fn confirm_yes_no_default_row_returns_to_the_selector() {
        for (cursor, expected) in [
            (1u16, SaveSubScreen::SlotSelect),
            (0, SaveSubScreen::FinalExit),
        ] {
            let mut m = dispatching(SaveEntryContext::MenuSave);
            m.goto(SaveSubScreen::ConfirmYesNo);
            m.tick(idle(), 0);
            let fx = m.tick(
                SubScreenInput {
                    nav: 1,
                    cursor,
                    ..idle()
                },
                0,
            );
            assert_eq!(m.screen(), expected, "cursor {cursor}");
            assert_eq!(fx, vec![SubScreenEffect::Sfx(0x20)]);
        }
    }

    /// Cancelling the confirm goes back to the selector, the same place
    /// the default row goes.
    #[test]
    fn confirm_yes_no_cancel_returns_to_the_selector() {
        let mut m = dispatching(SaveEntryContext::MenuSave);
        m.goto(SaveSubScreen::ConfirmYesNo);
        m.tick(idle(), 0);
        m.tick(SubScreenInput { nav: 2, ..idle() }, 0);
        assert_eq!(m.screen(), SaveSubScreen::SlotSelect);
    }

    /// The two pad-wait screens are mirror images: one needs a button
    /// down, the other needs the pad clear.
    #[test]
    fn pad_wait_screens_mirror_each_other() {
        let mut press = dispatching(SaveEntryContext::PostSave);
        press.tick(idle(), 0);
        // Pad clear: the press-wait screen stays put.
        press.tick(idle(), 0);
        assert_eq!(press.screen(), SaveSubScreen::PostSaveReturn);
        press.tick(
            SubScreenInput {
                any_button_held: true,
                ..idle()
            },
            0,
        );
        assert_eq!(press.screen(), SaveSubScreen::SlotSelect);

        let mut release = dispatching(SaveEntryContext::MenuSave);
        release.goto(SaveSubScreen::PadReleaseWait);
        release.tick(idle(), 0);
        // Button held: the release-wait screen stays put.
        release.tick(
            SubScreenInput {
                any_button_held: true,
                ..idle()
            },
            0,
        );
        assert_eq!(release.screen(), SaveSubScreen::PadReleaseWait);
        release.tick(idle(), 0);
        assert_eq!(release.screen(), SaveSubScreen::Unpinned(0x05));
    }

    /// The Yes branch of the confirm-exit screen does not leave: it runs
    /// a second script and exits with the confirmed code once that
    /// settles.
    #[test]
    fn confirm_exit_yes_branch_runs_a_second_script() {
        let mut m = dispatching(SaveEntryContext::MenuSave);
        m.goto(SaveSubScreen::ConfirmExit);
        m.tick(idle(), 0);

        let fx = m.tick(
            SubScreenInput {
                nav: 1,
                cursor: 0,
                ..idle()
            },
            0,
        );
        assert!(fx.contains(&SubScreenEffect::RunScript));
        assert!(fx.contains(&SubScreenEffect::Sfx(0x88)));
        assert_eq!(m.step(), 2);
        assert_eq!(m.screen(), SaveSubScreen::ConfirmExit);

        m.tick(idle(), 0);
        assert_eq!(m.exit_code(), Some(EXIT_CODE_CONFIRMED));
    }

    /// The No branch leaves for a different screen than the Yes branch's
    /// exit, and cancel goes the same way No does.
    #[test]
    fn confirm_exit_no_and_cancel_leave() {
        for nav in [1u8, 2] {
            let mut m = dispatching(SaveEntryContext::MenuSave);
            m.goto(SaveSubScreen::ConfirmExit);
            m.tick(idle(), 0);
            m.tick(
                SubScreenInput {
                    nav,
                    cursor: 1,
                    ..idle()
                },
                0,
            );
            assert_eq!(m.screen(), SaveSubScreen::Unpinned(0x06), "nav {nav}");
            assert!(m.exit_code().is_none());
        }
    }

    /// Both card drivers are the same machine; only the op differs, and
    /// both land back on the slot selector.
    #[test]
    fn card_drivers_differ_only_in_direction() {
        for (screen, op) in [
            (SaveSubScreen::CardSave, CardOp::Save),
            (SaveSubScreen::CardLoad, CardOp::Load),
        ] {
            let mut m = dispatching(SaveEntryContext::MenuSave);
            m.goto(screen);

            let fx = m.tick(idle(), 0);
            assert!(fx.contains(&SubScreenEffect::InstallCardHandle));
            assert!(fx.contains(&SubScreenEffect::RunScript));

            m.tick(idle(), 0); // step 1 -> 2 (script idle)
            assert_eq!(m.step(), 2);

            // Transfer in flight: the op keeps being driven.
            let fx = m.tick(idle(), 0);
            assert_eq!(fx, vec![SubScreenEffect::CardOp(op)]);
            assert_eq!(m.step(), 2);

            m.tick(
                SubScreenInput {
                    card_done: true,
                    ..idle()
                },
                0,
            );
            assert_eq!(m.step(), 3);
            m.tick(idle(), 0);
            assert_eq!(m.screen(), SaveSubScreen::SlotSelect);
        }
    }

    /// The save-confirm's three rows do not share an exit: only row 1
    /// can proceed, row 0 goes to the error screen, row 2 leaves.
    #[test]
    fn save_confirm_rows_have_distinct_exits() {
        let cases = [
            (0u16, SaveSubScreen::Unpinned(0x1B)),
            (2, SaveSubScreen::FinalExit),
        ];
        for (cursor, expected) in cases {
            let mut m = dispatching(SaveEntryContext::MenuSave);
            m.goto(SaveSubScreen::SaveConfirm);
            m.tick(idle(), 0);
            m.tick(
                SubScreenInput {
                    nav: 1,
                    cursor,
                    ..idle()
                },
                0,
            );
            assert_eq!(m.screen(), expected, "cursor {cursor}");
        }
    }

    /// Row 1 proceeds only when the scan found a usable save block;
    /// without one it plays the error cue and stays put rather than
    /// falling through to a transition.
    #[test]
    fn save_confirm_row_one_needs_an_available_block() {
        let mut blocked = dispatching(SaveEntryContext::MenuSave);
        blocked.goto(SaveSubScreen::SaveConfirm);
        blocked.tick(idle(), 0);
        let fx = blocked.tick(
            SubScreenInput {
                nav: 1,
                cursor: 1,
                save_blocks_available: false,
                ..idle()
            },
            0,
        );
        assert_eq!(fx, vec![SubScreenEffect::Sfx(0x23)]);
        assert_eq!(blocked.screen(), SaveSubScreen::SaveConfirm);
        assert_eq!(blocked.step(), 1);

        let mut ok = dispatching(SaveEntryContext::MenuSave);
        ok.goto(SaveSubScreen::SaveConfirm);
        ok.tick(idle(), 0);
        ok.tick(
            SubScreenInput {
                nav: 1,
                cursor: 1,
                save_blocks_available: true,
                ..idle()
            },
            0,
        );
        assert_eq!(ok.step(), 2);
        ok.tick(idle(), 0);
        assert_eq!(ok.screen(), SaveSubScreen::QuantitySpinner);
    }

    /// The spinner reads the focused inventory entry once it settles, and
    /// its result selector picks between committing and re-running.
    #[test]
    fn quantity_spinner_commits_on_result_two() {
        let mut m = dispatching(SaveEntryContext::MenuSave);
        m.goto(SaveSubScreen::QuantitySpinner);
        m.tick(idle(), 0);

        let fx = m.tick(
            SubScreenInput {
                spinner_result: 2,
                ..idle()
            },
            0,
        );
        assert!(fx.contains(&SubScreenEffect::ReadInventoryEntry));
        assert_eq!(m.screen(), SaveSubScreen::Unpinned(0x1F));
    }

    /// Result `3` re-runs the second display script and parks on the
    /// wait step rather than leaving the screen.
    #[test]
    fn quantity_spinner_rerun_parks_on_the_wait_step() {
        let mut m = dispatching(SaveEntryContext::MenuSave);
        m.goto(SaveSubScreen::QuantitySpinner);
        m.tick(idle(), 0);

        let fx = m.tick(
            SubScreenInput {
                spinner_result: 3,
                ..idle()
            },
            0,
        );
        assert!(fx.contains(&SubScreenEffect::RunScript));
        assert_eq!(m.step(), 3);
        assert_eq!(m.screen(), SaveSubScreen::QuantitySpinner);

        m.tick(idle(), 0);
        assert_eq!(m.screen(), SaveSubScreen::SaveConfirm);
    }

    /// A full Load flow: the card driver runs, lands on the selector, and
    /// the confirm's default row keeps the player there.
    #[test]
    fn load_flow_walks_card_driver_into_the_selector() {
        let mut m = dispatching(SaveEntryContext::Load);
        assert_eq!(m.screen(), SaveSubScreen::CardLoad);

        m.tick(idle(), 0); // install + script
        m.tick(idle(), 0); // script idle
        m.tick(
            SubScreenInput {
                card_done: true,
                ..idle()
            },
            0,
        );
        m.tick(idle(), 0);
        assert_eq!(m.screen(), SaveSubScreen::SlotSelect);
        assert!(!m.is_done());
    }

    /// The flow only terminates after the fade-out finishes, so an exit
    /// code alone does not mean the UI is gone.
    #[test]
    fn exit_code_still_waits_for_the_fade_out() {
        let mut m = dispatching(SaveEntryContext::MenuSave);
        m.goto(SaveSubScreen::FinalExit);
        m.tick(idle(), 0);
        m.tick(idle(), 0);
        assert_eq!(m.phase(), SavePhase::FadeOut);
        assert!(!m.is_done());

        while !m.is_done() {
            m.tick(idle(), 0x20);
        }
        // The fade-out ends opaque, not transparent - it is the reverse
        // of the fade-in, and retail's terminal state is reached by the
        // level climbing back to `0xF2`.
        assert_eq!(m.fade(), FADE_OPAQUE);
    }

    /// Backing out of the save UI opens on the save-confirm screen, and
    /// that screen must be the *dispatchable* variant. Naming it as an
    /// unpinned id would round-trip the number while comparing unequal
    /// to `SaveConfirm`, so the dispatcher would park on a screen this
    /// module implements.
    #[test]
    fn cancel_context_opens_a_dispatchable_save_confirm() {
        let start = SaveEntryContext::Cancel.start_screen();
        assert_eq!(start, SaveSubScreen::SaveConfirm);
        assert_eq!(start.id(), 0x1A);
        assert_eq!(start, SaveSubScreen::from_id(0x1A));
        assert!(start.is_pinned());

        // It really dispatches: step 0 stages and runs the display
        // script rather than returning nothing.
        let mut m = dispatching(SaveEntryContext::Cancel);
        let effects = m.tick(idle(), 0);
        assert!(effects.contains(&SubScreenEffect::RunScript), "{effects:?}");
    }

    /// Retail uses two distinct fade constants: pad input is unmasked
    /// from `0x79` down, but the fade-in only hands over to dispatch
    /// below `0x79`. Collapsing them into one would let dispatch start a
    /// frame early.
    #[test]
    fn input_unmasks_one_level_before_dispatch_begins() {
        assert_eq!(FADE_INPUT_THRESHOLD, 0x7A);
        assert_eq!(FADE_DISPATCH_THRESHOLD, 0x79);

        let mut m = SaveScreenMachine::new(SaveEntryContext::MenuSave);
        m.tick(idle(), 0); // Init
        // Land exactly on 0x79: input is already live, dispatch is not.
        while m.fade() > 0x79 {
            m.tick(idle(), 1);
        }
        assert_eq!(m.fade(), 0x79);
        assert!(m.input_active());
        assert_eq!(m.phase(), SavePhase::FadeIn);

        m.tick(idle(), 1);
        assert_eq!(m.phase(), SavePhase::Dispatch);
    }

    /// The fade-out climbs to opaque and stops there - it neither
    /// overshoots nor completes on a transparent screen.
    #[test]
    fn fade_out_ramps_up_to_opaque_and_clamps() {
        let mut m = dispatching(SaveEntryContext::MenuSave);
        m.goto(SaveSubScreen::FinalExit);
        m.tick(idle(), 0);
        m.tick(idle(), 0);
        assert_eq!(m.phase(), SavePhase::FadeOut);
        // The exiting screen writes a delta, so the level is still the
        // transparent one the dispatch phase ran at.
        assert!(m.fade() < FADE_DISPATCH_THRESHOLD);

        let mut seen = vec![m.fade()];
        while !m.is_done() {
            m.tick(idle(), 0x30);
            seen.push(m.fade());
        }
        assert!(
            seen.windows(2).all(|w| w[1] >= w[0]),
            "not monotonic: {seen:?}"
        );
        assert_eq!(m.fade(), FADE_OPAQUE);
    }
}
