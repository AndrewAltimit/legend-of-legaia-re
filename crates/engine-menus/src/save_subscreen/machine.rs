//! `SaveScreenMachine`: entry context, phases, fade thresholds, per-frame input and the effects it emits.

use super::*;

/// What opened the save UI. Retail decodes an entry-context pointer into
/// the starting sub-screen; the pointer's *target byte* selects, except
/// for the sentinel value which is never dereferenced.
///
/// REF: FUN_801DC6B4 (state 0's entry-context decode).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveEntryContext {
    /// The sentinel pointer value `1`, which retail tests for and never
    /// dereferences (`bne v0,s1` at `0x801DC868`, then
    /// `_DAT_8007B450 = 0`). It opens sub-screen `0x02` - the developer
    /// character-parameter editor, not the save flow. None of the four
    /// context bytes below reaches a save screen either - the card
    /// drivers are entered from the root picker's own rows.
    DebugParamEditor,
    /// Context byte `0x01` - a field script's save point: retail opens
    /// straight on the save-card driver `0x19`
    /// (`0x801DC8AC..0x801DC8B4`), skipping the root picker entirely.
    ScriptSave,
    /// Context byte `0x07` - the casino ticket counter, which opens the
    /// prize-exchange confirm [`SaveSubScreen::CasinoPrizeConfirm`]
    /// (`0x801DC8C8`). Read as "auto-save" before the screen it opens was
    /// traced.
    CasinoPrizeCounter,
    /// Context byte `0x0D` - returning after a save completed. The same
    /// kind byte hides the root picker's Load row and arms its
    /// leave-confirm (`crate::pause_screens::ROOT_MENU_CONTEXT_LOCKED`).
    PostSave,
    /// Context byte `0x00` - a **town shop**. The byte is the op-`0x49`
    /// record's own kind, and it opens the shop's mode select
    /// [`SaveSubScreen::ShopModeSelect`] (`0x801DC89C`).
    ShopEntry,
}

impl SaveEntryContext {
    /// The sub-screen this context opens on.
    pub fn start_screen(self) -> SaveSubScreen {
        match self {
            Self::DebugParamEditor => SaveSubScreen::CharParamEditor,
            Self::ScriptSave => SaveSubScreen::CardSave,
            Self::CasinoPrizeCounter => SaveSubScreen::CasinoPrizeConfirm,
            Self::PostSave => SaveSubScreen::PostSaveReturn,
            Self::ShopEntry => SaveSubScreen::ShopModeSelect,
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
    /// The bag holds at least one sellable entry in the scanned window
    /// (`0x80085958`, stride 2). The shop's mode select refuses to open the
    /// Sell list without one. Named for the superseded "save block
    /// existence table" reading of the same walk.
    pub sellable_items_available: bool,
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
    /// The current sub-screen is one another `engine-core` module ports:
    /// hand the frame to [`RoutedPort::engine_module`] rather than stepping
    /// it here.
    ///
    /// Retail has no counterpart - its dispatcher just indirects through the
    /// pointer table - so this is the port's seam, not a retail effect. It
    /// exists because the alternative is what this module used to do: return
    /// an empty effect list, which is indistinguishable from "this screen had
    /// nothing to do this frame" and let 19 of the table's 33 ids read as
    /// unknown.
    Route(RoutedPort),
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
            SaveSubScreen::ShopModeSelect => self.tick_shop_mode_select(input),
            SaveSubScreen::QuantitySpinner => self.tick_quantity_spinner(input),
            // `FUN_801DD310` is a bare flush wrapper: no step, no transition,
            // nothing for a host to do. The menu image's routine at that VA
            // (other images hold unrelated code there, hence the stem):
            // PORT: overlay_menu_0899_801dd310
            SaveSubScreen::FrameFlushTick => Vec::new(),
            // A screen another module ports. Retail indirects into it through
            // the pointer table; the engine names the module instead, so a
            // host can hand the frame on rather than watch the flow park.
            SaveSubScreen::Routed(id) => routed_port(id)
                .map(|port| vec![SubScreenEffect::Route(port)])
                .unwrap_or_default(),
            // Named here but not stepped here: each is a screen this module
            // decodes (its entry-context route, its exit code, its id) whose
            // per-frame body belongs to another module - the slot grid and
            // the info panel to `crate::save_select`, the character-parameter
            // editor and the generic picker to no host at all, the casino
            // confirm to `legaia_engine_minigames::prize_exchange`. A host drives them through
            // `goto`, as it always has.
            SaveSubScreen::SlotSelect
            | SaveSubScreen::CharParamEditor
            | SaveSubScreen::GenericPicker
            | SaveSubScreen::CasinoPrizeConfirm => Vec::new(),
            // An id outside the retail table (`>= 0x21`). Retail reads `0`
            // there; the port parks rather than inventing a transition.
            SaveSubScreen::Unpinned(_) => Vec::new(),
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
                self.goto(SaveSubScreen::Routed(0x05));
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
                    self.goto(SaveSubScreen::Routed(0x06));
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
                    self.goto(SaveSubScreen::Routed(0x13));
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
    /// argument (`1` save, `2` load) - which is why they share one
    /// implementation. Both return to the slot selector when the
    /// transfer lands.
    ///
    /// One retail asymmetry is not modelled: the **save** driver's final
    /// step re-tests the entry-context pointer and leaves for the terminal
    /// screen instead of the selector when it is set - a save raised by a
    /// field script hands control back to the script rather than to the
    /// menu - where the load driver has no such branch. Modelling it needs
    /// an input this struct does not carry, so the shared machine always
    /// takes the selector exit.
    ///
    /// PORT: FUN_801DAE24 (load), FUN_801DAEF4 (save)
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

    /// Sub-screen `0x1A`: the shop's Buy / Sell / Quit mode select, a
    /// three-row list.
    ///
    /// The rows do not share an exit. Row `2` and the cancel button both
    /// leave for the terminal screen; row `0` (Buy) leaves for the buy list
    /// `0x1B`; row `1` (Sell) is the only one that can *proceed*, and only
    /// after a walk of the item bag finds something to sell. Failing that
    /// walk buzzes and leaves the screen where it is - retail does not fall
    /// through to a transition.
    ///
    /// The row-`2` exit fires *two* audio calls in retail - the same cue
    /// entry point the other screens use, with id `0`, plus a second
    /// routine with `0x37`. Only the `0x37` one is modelled here, since
    /// [`SubScreenEffect::Sfx`] does not distinguish the two entry
    /// points.
    ///
    /// PORT: FUN_801DAFD4
    fn tick_shop_mode_select(&mut self, input: SubScreenInput) -> Vec<SubScreenEffect> {
        match self.step {
            0 => {
                self.step = 1;
                vec![SubScreenEffect::ClearStaging, SubScreenEffect::RunScript]
            }
            1 if !input.script_busy => match (input.nav, input.cursor & 0xFFF) {
                (1, 0) => {
                    self.goto(SaveSubScreen::Routed(0x1B));
                    vec![SubScreenEffect::ClearStaging]
                }
                (1, 1) => {
                    if input.sellable_items_available {
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
                    self.goto(SaveSubScreen::ShopModeSelect);
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
            2 => self.goto(SaveSubScreen::Routed(0x1F)),
            _ => {}
        }
        effects
    }
}
