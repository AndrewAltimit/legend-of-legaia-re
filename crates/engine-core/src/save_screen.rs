//! The save screen's **host half**, as one kernel.
//!
//! [`save_select`](crate::save_select) models the session: which phase the
//! screen is in, which pill the cursor sits on, when the "Now checking" beat
//! starts and ends. Everything *around* that session used to live in each
//! host - the 5x3 block-grid cursor, the card read the grid draws from, the
//! rule that a Load may not confirm an empty block, and the mapping from a
//! finished session's outcome back to "which port, which block". Two hosts
//! carrying that separately is exactly the divergence
//! `scripts/ci/check-ui-host-drift.py` exists to catch: same builder, same
//! session, two different models feeding them.
//!
//! So it lives here, once. A host owns a [`SaveScreenFlow`], answers the
//! block read it asks for, and asks it what to commit.
//!
//! ## Two racks, one flow
//!
//! [`SaveRack`](crate::save_select::SaveRack) is what the pill row
//! addresses, and it is the single thing that decides whether the session
//! runs retail's two-stage card flow:
//!
//! * [`SaveRack::CardPorts`](crate::save_select::SaveRack::CardPorts) - the
//!   pills are the console's memory-card **ports**, and the 5x3 grid is the
//!   chosen port's fifteen blocks. This is retail.
//! * [`SaveRack::Blocks`](crate::save_select::SaveRack::Blocks) - the pill
//!   row *is* the block list. The flat model, kept for headless drivers that
//!   drive a session directly.
//!
//! No host sets the card-slots flag any more; it constructs a rack and the
//! kernel derives the flag from it
//! ([`SaveSelectSession::for_rack`](crate::save_select::SaveSelectSession::for_rack)).
//!
//! ## What a host still owns
//!
//! The **bytes**. This module never reads a card image or a save file: it
//! asks (via [`SaveScreenFlow::pending_read`]) for the fifteen
//! [`SlotSnapshot`]s behind a port and takes whatever the host hands back
//! ([`SaveScreenFlow::install_blocks`]). That is what lets the browser back
//! its ports with imported `.mcr` images while the native shell backs port 1
//! with its own save directory, without the screen behaving differently.
//!
//! Cell-to-block addressing is likewise the backend's: [`SaveCommit`] names
//! the **grid cell**, because a PSX card's cell `i` is block `i + 1` (block 0
//! is the directory) while a plain save directory's cell `i` is slot `i`.
//!
//! REF: FUN_801E08D8 (the info-panel renderer the grid feeds)
//! REF: FUN_801E1208 (per-card block enumeration - the read a host answers)

use crate::input::PadButton;
use crate::save_select::{
    CardIoMachine, CardStatus, SaveSelectMode, SaveSelectSession, SelectOutcome, SelectPhase,
    SlotSnapshot, card_frame_tick,
};
use crate::save_subscreen::{
    FADE_OPAQUE, SaveEntryContext, SaveScreenMachine, SaveSubScreen, SubScreenEffect,
    SubScreenInput,
};

/// Columns in retail's slot-preview grid.
///
/// Mirrors `legaia-engine-ui`'s pinned `SLOT_GRID_COLS`; the cursor must walk
/// the cells the sprites are drawn at. `engine-ui` does not depend on
/// `engine-core`, so the value exists on both sides of that edge.
pub const SLOT_GRID_COLS: u8 = 5;

/// Rows in retail's slot-preview grid. Mirrors `legaia-engine-ui`'s
/// `SLOT_GRID_ROWS`; see [`SLOT_GRID_COLS`].
pub const SLOT_GRID_ROWS: u8 = 3;

/// Cells in retail's slot-preview grid - one per save block on a card
/// (`CARD_TOTAL_BLOCKS`).
pub const SLOT_GRID_CELLS: u8 = SLOT_GRID_COLS * SLOT_GRID_ROWS;

/// Which direction a finished save screen commits in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveCommitKind {
    /// Replace the running world with the block's contents.
    Load,
    /// Write the running world into the block.
    Save,
}

/// Where a finished save screen commits, in rack coordinates.
///
/// `cell` is the **grid cell**, not a block number - the backend that owns
/// the bytes decides what cell `i` addresses (card block `i + 1`, disk slot
/// `i`, ...). In the flat [`SaveRack::Blocks`](crate::save_select::SaveRack)
/// model `port` and `cell` are the same slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaveCommit {
    /// The card port the player picked off the pill row.
    pub port: u8,
    /// The cell they picked out of that port's 5x3 preview grid.
    pub cell: u8,
    pub kind: SaveCommitKind,
}

/// A host's save-screen state: the grid cursor and the card read behind it.
///
/// One per host. Construct it once, drive it around each
/// [`SaveSelectSession::tick`]:
///
/// ```text
/// if let Some(port) = flow.pending_read(&session) {
///     flow.install_blocks(port, backend.blocks_for(port));   // host's bytes
/// }
/// let edge = flow.before_tick(&session, edge);               // cursor + gate
/// session.tick(SelectInput::from_pad_edge(edge));
/// if session.is_done() {
///     if let Some(commit) = flow.commit(&session) { backend.apply(commit) }
/// }
/// ```
#[derive(Debug, Default, Clone)]
pub struct SaveScreenFlow {
    /// Cursor over the previewed port's 5x3 block grid. Only meaningful
    /// while the session is past the pill row in card-ports mode.
    grid_cursor: u8,
    /// `(port, blocks)` - the result of the card read, held for as long as
    /// its grid is up.
    ///
    /// This is what the "Now checking" beat is *for*: lifting fifteen SC
    /// blocks through `SaveFile::from_retail_sc_block` copies the better
    /// part of a card, so it happens once per read rather than once per
    /// frame in the draw path.
    blocks: Option<(u8, Vec<SlotSnapshot>)>,
    /// Retail's two-op card I/O driver, running behind the read above.
    ///
    /// The **card is the host's block backend**: the native shell's save
    /// directory, the browser's imported `.mcr`. So the machine's poll status
    /// is what the backend answered - blocks installed for the port on screen
    /// is `Ready`, a mount with nothing readable is `NoCard`, and a read the
    /// host has not answered yet is `Pending`. That is the same substitution
    /// the rest of this module makes (the flow asks for snapshots and never
    /// touches a device), applied to the beat instead of to the bytes.
    io: CardIoMachine,
    /// `DAT_801EF17C` - the backstop counter the arm states reset.
    io_poll: u16,
    /// The last result [`card_frame_tick`] published (`0` = still running).
    io_result: i32,
    /// `_DAT_801F0224` - the directory-rebuild request the commit beat
    /// consumes.
    io_rebuild: bool,
    /// Retail's outer save-UI dispatcher, running around the session.
    ///
    /// [`SaveSelectSession`] is the screen's *content* model; this is its
    /// control flow - the fade-in that gates input, the sub-screen id, and
    /// the card driver's own step machine. The two are joined at the card
    /// op: the driver's `script_busy` / `card_done` waits are answered by
    /// [`Self::io`]'s published result, so the graph moves because the
    /// backend answered a read, not because a frame counter expired.
    ///
    /// `None` until the first card-rack frame, which is where the session's
    /// direction is known (retail picks it at the root picker: row `5`
    /// `@Load` routes to `0x18`, row `6` `@Save` to `0x19`).
    machine: Option<SaveScreenMachine>,
    /// The effects the machine's last frame asked for.
    machine_effects: Vec<SubScreenEffect>,
}

/// How much of the outer fade one frame burns.
///
/// Retail runs its fade on a timer of its own and the machine takes the rate
/// from its caller ([`SaveScreenMachine::tick`]), so this is the port's rate,
/// not a recovered constant: `0xF2` down past the `0x7A` input threshold in
/// eight frames, which sits inside the "Now checking" beat by two orders of
/// magnitude and inside a human's reaction to the button that opened the
/// screen.
pub const SAVE_SCREEN_FADE_DELTA: u8 = 0x10;

impl SaveScreenFlow {
    pub fn new() -> Self {
        Self::default()
    }

    /// Cursor over the previewed port's 5x3 block grid (cell index, not a
    /// block number).
    pub fn grid_cursor(&self) -> u8 {
        self.grid_cursor
    }

    /// The previewed port's blocks, or empty when nothing has been read.
    pub fn blocks(&self) -> &[SlotSnapshot] {
        self.blocks
            .as_ref()
            .map(|(_, b)| b.as_slice())
            .unwrap_or(&[])
    }

    /// The port the cached read came from, if there is one.
    pub fn read_port(&self) -> Option<u8> {
        self.blocks.as_ref().map(|(p, _)| *p)
    }

    /// The focused block, when it holds something loadable.
    pub fn focused_block(&self) -> Option<&SlotSnapshot> {
        self.blocks()
            .get(self.grid_cursor as usize)
            .filter(|b| b.present)
    }

    /// What the 5x3 preview grid draws, as `(cells, focused)`.
    ///
    /// In the two-stage rack that is the picked **port's** blocks and the
    /// grid cursor - a different list from the pill row, which is the trap
    /// this exists to close: drawing `session.slots()` there captions the
    /// grid with the card rather than with the saves on it. A flat rack has
    /// only the one list, so it previews itself.
    ///
    /// The blocks are empty until the host has answered
    /// [`Self::pending_read`] for the port on screen; a half-read card draws
    /// an empty grid rather than the previous card's.
    pub fn preview<'a>(&'a self, session: &'a SaveSelectSession) -> (&'a [SlotSnapshot], u8) {
        if !session.card_slots_mode() {
            return (session.slots(), session.current_slot());
        }
        if self.read_port() == Some(session.current_slot()) {
            (self.blocks(), self.grid_cursor)
        } else {
            (&[], 0)
        }
    }

    /// Forget the cached read and park the cursor. Called on the way into a
    /// fresh screen so a second visit does not draw the previous card.
    pub fn reset(&mut self) {
        self.grid_cursor = 0;
        self.blocks = None;
        self.io = CardIoMachine::new();
        self.io_poll = 0;
        self.io_result = 0;
        self.io_rebuild = false;
        self.machine = None;
        self.machine_effects.clear();
    }

    /// The retail sub-screen id the flow is on, or `None` before the first
    /// card-rack frame.
    ///
    /// This is the id a host keys retail-exact chrome off: the card driver
    /// (`0x18` / `0x19`) while the op runs, then the slot selector (`0x01`)
    /// once it published.
    pub fn retail_subscreen(&self) -> Option<SaveSubScreen> {
        self.machine.as_ref().map(|m| m.screen())
    }

    /// The outer dispatcher's fade level (`0xF2` opaque, `0` clear).
    pub fn retail_fade(&self) -> u8 {
        self.machine.as_ref().map_or(FADE_OPAQUE, |m| m.fade())
    }

    /// What the last frame of the outer dispatcher asked the host for.
    pub fn subscreen_effects(&self) -> &[SubScreenEffect] {
        &self.machine_effects
    }

    /// The card I/O driver's last published result: `1` once the two-op cycle
    /// completed, `-1` no card, `-2` a stray completion, `-3` abort/timeout,
    /// `0` while it is still running.
    pub fn card_io_result(&self) -> i32 {
        self.io_result
    }

    /// Whether the driver spent its retry budget without the backend ever
    /// answering with readable blocks - retail's "no card" verdict.
    pub fn card_absent(&self) -> bool {
        self.io_result == -1
    }

    /// This frame's poll status for the card behind `port`, derived from what
    /// the host has answered [`Self::pending_read`] with.
    fn card_status(&self, port: u8) -> CardStatus {
        match self.blocks.as_ref() {
            Some((p, blocks)) if *p == port && blocks.iter().any(|b| b.present) => {
                CardStatus::Ready
            }
            // The host answered and the port holds nothing readable: an empty
            // mount reads as no card, which is what spends the retry budget.
            Some((p, _)) if *p == port => CardStatus::NoCard,
            _ => CardStatus::Pending,
        }
    }

    /// The port whose blocks the host must lift this frame, or `None` when
    /// the cache is current (or no read is in progress).
    ///
    /// Rebuilds only when the cache is missing or holds a *different* port,
    /// so the grid's draw path never re-parses a card.
    pub fn pending_read(&self, session: &SaveSelectSession) -> Option<u8> {
        if !session.card_slots_mode() {
            return None;
        }
        if !matches!(
            session.phase(),
            SelectPhase::NowChecking { .. }
                | SelectPhase::SlotPreview { .. }
                | SelectPhase::ConfirmOverwrite { .. }
                | SelectPhase::ConfirmDelete { .. }
        ) {
            return None;
        }
        let port = session.current_slot();
        (self.read_port() != Some(port)).then_some(port)
    }

    /// Install the blocks the host lifted for `port`.
    pub fn install_blocks(&mut self, port: u8, blocks: Vec<SlotSnapshot>) {
        self.blocks = Some((port, blocks));
    }

    /// Step the grid cursor for this frame's pad edge and return the edge the
    /// session should see. Call **before** [`SaveSelectSession::tick`], so a
    /// confirm on the same edge commits the cell the player is looking at.
    ///
    /// Two things happen here, both of which the session cannot do itself:
    ///
    /// * the 5x3 cursor walks (the session's `SlotPreview` ignores
    ///   directions - the grid is not its model), and
    /// * a Load confirm on an **empty** cell is suppressed. The session only
    ///   knows the phase, so a Cross on an empty cell would report `Loaded`
    ///   and leave the host to fail parsing a block that holds no save,
    ///   closing the screen with nothing to show for it. Retail simply
    ///   refuses. Saving into an empty block is legitimate - that is how a
    ///   new save is made - so this gates Load only.
    ///
    /// A flat-rack session gets its edge back untouched: there is no second
    /// stage to walk.
    pub fn before_tick(&mut self, session: &SaveSelectSession, edge: u16) -> u16 {
        if !session.card_slots_mode() {
            return edge;
        }
        // The card I/O beat, once per frame for as long as a card screen is
        // up. Retail's ticker runs `FUN_801E3294` every frame while its gate
        // word allows and latches the first non-zero result; the commit phase
        // it also watches is the one the *save* direction raises, which the
        // port performs in one call, so the rebuild arm never fires here.
        let status = self.card_status(session.current_slot());
        let (result, _effect, rebuilt) = card_frame_tick(
            &mut self.io,
            status,
            &mut self.io_poll,
            true,
            0,
            &mut self.io_rebuild,
            &[],
        );
        debug_assert!(rebuilt.is_none(), "no commit phase is raised here");
        if result != 0 {
            self.io_result = result;
        }
        // The outer dispatcher, around the same beat. Retail enters the card
        // direction from the root picker's rows rather than from an entry
        // context, so the flow opens the context a field save point uses and
        // routes to the driver the session's direction names.
        let machine = self.machine.get_or_insert_with(|| {
            let mut m = SaveScreenMachine::new(SaveEntryContext::ScriptSave);
            if session.mode() == SaveSelectMode::Load {
                m.goto(SaveSubScreen::CardLoad);
            }
            m
        });
        let sub_input = SubScreenInput {
            // The display script is "busy" until the card op published: that
            // is what the driver's step-1 wait is waiting for here.
            script_busy: self.io_result == 0,
            any_button_held: edge != 0,
            nav: if edge & PadButton::Cross.mask() != 0 {
                1
            } else if edge & PadButton::Circle.mask() != 0 {
                2
            } else {
                0
            },
            cursor: u16::from(self.grid_cursor),
            card_done: self.io_result > 0,
            ..Default::default()
        };
        self.machine_effects = machine.tick(sub_input, SAVE_SCREEN_FADE_DELTA);
        // Retail suppresses the pad while the fade is still above its
        // threshold (`FADE_INPUT_THRESHOLD`), which is what keeps the button
        // that opened the screen from being consumed as its first input.
        let edge = if machine.input_active() { edge } else { 0 };
        match session.phase() {
            SelectPhase::SlotPreview { .. } => {
                self.grid_cursor = step_grid_cursor(self.grid_cursor, edge);
            }
            // The grid is not up yet: park the cursor on the first cell so
            // each card read starts at the top-left block, and (on the pill
            // row) drop the previous read - the player may be about to pick
            // the other port.
            SelectPhase::Browsing { .. } => {
                self.grid_cursor = 0;
                self.blocks = None;
            }
            SelectPhase::NowChecking { .. } => self.grid_cursor = 0,
            _ => {}
        }
        if !matches!(session.phase(), SelectPhase::SlotPreview { .. })
            || edge & PadButton::Cross.mask() == 0
        {
            return edge;
        }
        // A confirm of either direction needs the card op to have completed:
        // retail's screens sit on the driver's result word and a write into a
        // card whose two-op cycle has not published success is the one thing
        // that corrupts a block. The grid is only up after the "Now checking"
        // beat, which is two orders of magnitude longer than the cycle, so
        // this refuses a confirm exactly when the backend never answered.
        if self.io_result <= 0 {
            return edge & !PadButton::Cross.mask();
        }
        if session.mode() != SaveSelectMode::Load {
            return edge;
        }
        if self.focused_block().is_some() {
            edge
        } else {
            edge & !PadButton::Cross.mask()
        }
    }

    /// Where a finished session commits, in rack coordinates. `None` for a
    /// cancelled screen, for a Delete (not reachable from the card flow), and
    /// while the session is still running.
    pub fn commit(&self, session: &SaveSelectSession) -> Option<SaveCommit> {
        let (port, kind) = match session.outcome()? {
            SelectOutcome::Loaded(p) => (p, SaveCommitKind::Load),
            SelectOutcome::Saved(p) => (p, SaveCommitKind::Save),
            SelectOutcome::Deleted(_) | SelectOutcome::Cancelled => return None,
        };
        // Flat rack: the pill row IS the block list, so the slot the outcome
        // names is both the port and the cell.
        let cell = if session.card_slots_mode() {
            self.grid_cursor
        } else {
            port
        };
        Some(SaveCommit { port, cell, kind })
    }
}

/// Step the 5x3 block-grid cursor for one pad edge. Columns wrap within a row
/// and rows wrap top-to-bottom, matching the retail grid's cursor.
fn step_grid_cursor(cursor: u8, edge: u16) -> u8 {
    let cell = cursor.min(SLOT_GRID_CELLS - 1);
    let (mut col, mut row) = (cell % SLOT_GRID_COLS, cell / SLOT_GRID_COLS);
    let pressed = |b: PadButton| edge & b.mask() != 0;
    if pressed(PadButton::Left) {
        col = (col + SLOT_GRID_COLS - 1) % SLOT_GRID_COLS;
    }
    if pressed(PadButton::Right) {
        col = (col + 1) % SLOT_GRID_COLS;
    }
    if pressed(PadButton::Up) {
        row = (row + SLOT_GRID_ROWS - 1) % SLOT_GRID_ROWS;
    }
    if pressed(PadButton::Down) {
        row = (row + 1) % SLOT_GRID_ROWS;
    }
    (row * SLOT_GRID_COLS + col).min(SLOT_GRID_CELLS - 1)
}

/// A card port's pill entry: `present` means "something is mounted here",
/// which is what a card-ports session gates its confirm on.
///
/// Both hosts build their pill row through this so a port's pill carries the
/// same fields whatever backs it - an imported `.mcr` in the browser, the
/// engine's save directory in the native shell.
pub fn card_port_snapshot(port: u8, mounted: Option<&str>) -> SlotSnapshot {
    match mounted {
        Some(label) => SlotSnapshot {
            slot: port,
            present: true,
            label: label.to_string(),
            ..SlotSnapshot::empty(port)
        },
        None => SlotSnapshot::empty(port),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::save_select::{SaveRack, SelectInput};

    fn card_rack(ports: &[bool]) -> SaveRack {
        SaveRack::CardPorts(
            ports
                .iter()
                .enumerate()
                .map(|(i, &m)| card_port_snapshot(i as u8, m.then_some("CARD")))
                .collect(),
        )
    }

    fn block(cell: u8, present: bool) -> SlotSnapshot {
        SlotSnapshot {
            slot: cell,
            present,
            label: if present {
                "Vahn".into()
            } else {
                "<empty>".into()
            },
            ..SlotSnapshot::empty(cell)
        }
    }

    fn cross() -> u16 {
        PadButton::Cross.mask()
    }

    /// The rack is what turns the two-stage flow on - not a host flag.
    #[test]
    fn rack_kind_decides_card_slots_mode() {
        let card = SaveSelectSession::for_rack(SaveSelectMode::Save, &card_rack(&[true, false]));
        assert!(card.card_slots_mode(), "card ports run the two-stage flow");
        let flat = SaveSelectSession::for_rack(
            SaveSelectMode::Save,
            &SaveRack::Blocks(vec![block(0, true), block(1, false)]),
        );
        assert!(!flat.card_slots_mode(), "a flat block rack does not");
    }

    #[test]
    fn grid_cursor_wraps_both_axes() {
        assert_eq!(step_grid_cursor(0, PadButton::Left.mask()), 4);
        assert_eq!(step_grid_cursor(4, PadButton::Right.mask()), 0);
        assert_eq!(step_grid_cursor(0, PadButton::Up.mask()), 10);
        assert_eq!(step_grid_cursor(10, PadButton::Down.mask()), 0);
        assert_eq!(step_grid_cursor(SLOT_GRID_CELLS, 0), SLOT_GRID_CELLS - 1);
    }

    /// Retail's grid cursor cannot address `SLOT_INFO_RETURN_CELL`, so the
    /// port's must not either.
    ///
    /// The two clamps in the tick's shared stepper bound the cursor words
    /// to `col in 0..=4` (`slti v0,v0,0x5`, `0x801E017C`) and
    /// `row in 0..=2` (`slti v0,v0,0x3`, `0x801E01B0`), and the wrapper
    /// forms the cell as `col + row*5` (`0x801E06D0`), so `14` is the
    /// ceiling. Sweeping every cursor against every direction combination
    /// is the port-side statement of that bound.
    #[test]
    fn grid_cursor_never_reaches_the_return_cell() {
        let dirs = [
            PadButton::Left.mask(),
            PadButton::Right.mask(),
            PadButton::Up.mask(),
            PadButton::Down.mask(),
        ];
        for cursor in 0..=u8::MAX {
            for combo in 0..(1u16 << dirs.len()) {
                let mut edge = 0u16;
                for (i, d) in dirs.iter().enumerate() {
                    if combo & (1 << i) != 0 {
                        edge |= d;
                    }
                }
                let next = step_grid_cursor(cursor, edge);
                assert!(
                    next < SLOT_GRID_CELLS,
                    "cursor {cursor} + edge {edge:#06x} left the 5x3 grid at {next}"
                );
                assert_ne!(
                    next,
                    crate::save_select::SLOT_INFO_RETURN_CELL,
                    "cursor {cursor} + edge {edge:#06x} reached the dead Return cell"
                );
            }
        }
    }


    /// Drive the flow the way the module's own usage note says a host must:
    /// answer the card read the moment it is asked for, and call
    /// [`SaveScreenFlow::before_tick`] on **every** frame, not only the ones
    /// carrying an edge. The card I/O driver and the outer dispatcher both
    /// advance on that call, so a shortcut driver leaves them parked - which
    /// is what these tests used to be.
    fn run_beat(flow: &mut SaveScreenFlow, s: &mut SaveSelectSession, frames: u16) {
        for _ in 0..frames {
            if let Some(port) = flow.pending_read(s) {
                flow.install_blocks(port, (0..15).map(|i| block(i, i == 2)).collect());
            }
            let edge = flow.before_tick(s, 0);
            s.tick(SelectInput::from_pad_edge(edge));
        }
    }

    /// The read is asked for once per port, not once per frame.
    #[test]
    fn pending_read_asks_once_per_port() {
        let mut s = SaveSelectSession::for_rack(SaveSelectMode::Load, &card_rack(&[true, true]));
        let mut flow = SaveScreenFlow::new();
        assert_eq!(flow.pending_read(&s), None, "nothing to read on the pills");
        s.tick(SelectInput {
            cross: true,
            ..Default::default()
        });
        assert_eq!(flow.pending_read(&s), Some(0), "port 0 was picked");
        flow.install_blocks(0, (0..15).map(|i| block(i, i == 2)).collect());
        assert_eq!(flow.pending_read(&s), None, "cache is current");
        // A different port invalidates it.
        flow.install_blocks(1, Vec::new());
        assert_eq!(flow.pending_read(&s), Some(0));
    }

    /// A flat-rack session never asks for a block read - there is no second
    /// stage behind its pills.
    #[test]
    fn flat_rack_never_reads_blocks() {
        let mut s = SaveSelectSession::for_rack(
            SaveSelectMode::Load,
            &SaveRack::Blocks((0..15).map(|i| block(i, true)).collect()),
        );
        let flow = SaveScreenFlow::new();
        s.tick(SelectInput {
            cross: true,
            ..Default::default()
        });
        assert_eq!(flow.pending_read(&s), None);
    }

    /// Loading an empty cell is refused; the same cell in Save mode is not.
    #[test]
    fn load_confirm_is_gated_on_an_empty_cell() {
        for (mode, expect_gated) in [(SaveSelectMode::Load, true), (SaveSelectMode::Save, false)] {
            let mut s = SaveSelectSession::for_rack(mode, &card_rack(&[true, false]));
            let mut flow = SaveScreenFlow::new();
            s.tick(SelectInput {
                cross: true,
                ..Default::default()
            });
            // Run out the card-read beat, driving the flow every frame: the
            // card op has to publish before a confirm of either direction is
            // accepted, and the outer fade has to clear the input threshold.
            let beat = s.now_checking_frames() + 1;
            run_beat(&mut flow, &mut s, beat);
            assert!(matches!(s.phase(), SelectPhase::SlotPreview { .. }));
            assert_eq!(flow.card_io_result(), 1, "the card op published success");
            // Cell 0 is empty.
            let gated = flow.before_tick(&s, cross()) & PadButton::Cross.mask() == 0;
            assert_eq!(gated, expect_gated, "{mode:?} on an empty cell");
            // Cell 2 holds a save - never gated.
            flow.grid_cursor = 2;
            assert_ne!(flow.before_tick(&s, cross()) & PadButton::Cross.mask(), 0);
        }
    }

    /// The commit names the port off the outcome and the block off the grid.
    #[test]
    fn commit_pairs_the_outcome_port_with_the_grid_cell() {
        let mut s = SaveSelectSession::for_rack(SaveSelectMode::Load, &card_rack(&[false, true]));
        let mut flow = SaveScreenFlow::new();
        // Move to port 1 (port 0 is empty), confirm, run the beat out.
        s.tick(SelectInput {
            down: true,
            ..Default::default()
        });
        s.tick(SelectInput {
            cross: true,
            ..Default::default()
        });
        let beat = s.now_checking_frames() + 1;
        run_beat(&mut flow, &mut s, beat);
        flow.install_blocks(1, (0..15).map(|i| block(i, true)).collect());
        let edge = flow.before_tick(&s, PadButton::Right.mask());
        s.tick(SelectInput::from_pad_edge(edge));
        assert_eq!(flow.grid_cursor(), 1);
        let edge = flow.before_tick(&s, cross());
        s.tick(SelectInput::from_pad_edge(edge));
        assert_eq!(
            flow.commit(&s),
            Some(SaveCommit {
                port: 1,
                cell: 1,
                kind: SaveCommitKind::Load,
            })
        );
    }

    /// A flat rack commits the pill slot as both port and cell, so a host
    /// driving the legacy model keeps addressing the same save.
    #[test]
    fn flat_commit_uses_the_pill_slot_for_both() {
        let mut s = SaveSelectSession::for_rack(
            SaveSelectMode::Load,
            &SaveRack::Blocks((0..15).map(|i| block(i, true)).collect()),
        );
        let flow = SaveScreenFlow::new();
        s.tick(SelectInput {
            down: true,
            ..Default::default()
        });
        s.tick(SelectInput {
            cross: true,
            ..Default::default()
        });
        for _ in 0..s.now_checking_frames() + 1 {
            s.tick(SelectInput::default());
        }
        s.tick(SelectInput {
            cross: true,
            ..Default::default()
        });
        assert_eq!(
            flow.commit(&s),
            Some(SaveCommit {
                port: 1,
                cell: 1,
                kind: SaveCommitKind::Load,
            })
        );
    }

    /// Backing out to the pill row drops the read, so picking the other port
    /// cannot draw the previous card's blocks.
    #[test]
    fn returning_to_the_pills_drops_the_read() {
        let mut s = SaveSelectSession::for_rack(SaveSelectMode::Load, &card_rack(&[true, true]));
        let mut flow = SaveScreenFlow::new();
        s.tick(SelectInput {
            cross: true,
            ..Default::default()
        });
        for _ in 0..s.now_checking_frames() + 1 {
            s.tick(SelectInput::default());
        }
        flow.install_blocks(0, (0..15).map(|i| block(i, true)).collect());
        flow.grid_cursor = 7;
        s.tick(SelectInput {
            circle: true,
            ..Default::default()
        });
        assert!(matches!(s.phase(), SelectPhase::Browsing { .. }));
        flow.before_tick(&s, 0);
        assert!(flow.blocks().is_empty(), "the card read went with the grid");
        assert_eq!(flow.grid_cursor(), 0);
    }
}
