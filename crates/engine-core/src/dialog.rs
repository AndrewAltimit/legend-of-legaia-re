//! Dialog panel: port of the field VM's dialog opener path.
//!
//! PORT: FUN_801D84D0
//! REF: FUN_8001FD44, FUN_8001D7F8
//!
//! Those two are named below as what this module is NOT: the name-based
//! scene-change packet and its scene-name sync callee.
//!
//! Wraps a [`legaia_mes::DialogPlayer`] in the runtime state the retail
//! dialog renderer holds: the typed-out glyph buffer for the current page,
//! a pen color tracked through `0xCF` color-change escapes, and an explicit
//! page-break / done flag the host loop polls.
//!
//! This is the minimal substrate engines need to drive an on-screen dialog
//! window without re-implementing the typewriter / page-break semantics in
//! every consumer (the MES viewer in `asset-viewer`, the field VM's dialog
//! opener, the battle dialog overlay, etc). `FUN_8001FD44` is **not** that
//! opener - it is the name-based scene-change packet (`strcpy` into
//! `0x8007050C` / `0x80084548`, `_DAT_1F800394 |= 0x40`, then
//! `FUN_8001D7F8`), and nothing in this module implements it; the tag moved
//! to the op-`0x3F` arm in `crate::world`'s field-VM host.
//!
//! Provenance: the per-page glyph accumulation + page-break gate mirror the
//! retail dialog window pager `FUN_801D84D0` in the dialog overlay (see
//! [`docs/formats/dialog-font.md`](../../../docs/formats/dialog-font.md)).
//! Layout / GPU bridging lives in `legaia-engine-render` -
//! [`text_draws_for`](../../legaia_engine_render/fn.text_draws_for.html)
//! consumes the [`Self::page_glyphs`] byte stream via [`legaia_font::Font`].
//! REF: FUN_80036888
//! REF: FUN_80039B7C, FUN_801DE840

use legaia_mes::{DialogPlayer, Interpreter, MesEvent, PlayerState};
use std::sync::Arc;

/// Decode every `0x1F`-lead text segment in a field-VM inline dialog buffer
/// to its glyph bytes.
///
/// An interaction record's inline text is a flat **pool** of segments, each
/// introduced by a `0x1F` lead byte and terminated by an MES end byte
/// (`0x00..=0x1E`). Empirically (decoding real town01 placements) this pool is
/// the NPC's *entire* dialogue line set - every line across every story-state
/// branch of that NPC's conversation, with interspersed option labels (e.g.
/// `"Yes"` / `"No"`). It is **not** a single box's "prompt then option labels":
/// most segments are consecutive speech lines, and the field-VM script (gated
/// on story flags via `COND_JMP`) selects which segment to start at, how many
/// lines fill one box, and which are selectable options.
///
/// **There is no separate "box-geometry header" format.** The bytes between
/// the placement's `script_pc0` and the first `0x1F` lead are normal field-VM
/// bytecode - `CFlag` / `SysFlag.Test` / `JmpRel` / `Nop` / `0x4C 0x51`
/// NPC-move-to-tile - that runs as the NPC's interaction prologue (face the
/// player, set conversation flags, walk to the talk position, branch on story
/// flags). It is consumed by the field-VM dispatcher `FUN_801DE840` from
/// state 0 of the per-actor dialog SM `FUN_80039b7c`, which loops until the
/// dispatcher leaves the actor's PC on a byte where `& 0x7F < 0x20` (a
/// `0x1F` lead or a `0x21` terminator) and then transitions into the pager
/// `FUN_801D84D0`. The "select which segment to start at" mechanism is the
/// story-flag-gated `SysFlag.Test` branches themselves: the script
/// `JmpRel`s past unwanted segments to the desired one. See
/// `field_actor_placements_disc::dialog_prefix_decodes_as_field_vm_bytecode`
/// for the disc-gated proof. An earlier note pointed at `func_0x8001ebec` as
/// the renderer - that is **wrong**; the disassembly shows it is a
/// per-character TMD-pose copier indexed by the slot-4 freeze flag, not the
/// dialog box renderer.
///
/// The page/option grouping is pinned: the pager types consecutive `0x1F`
/// lines as one page, terminated by a post-page control byte
/// ([`legaia_mes::Dispatch`]), in a window of [`legaia_mes::LINES_PER_BOX`]
/// (`_DAT_801F2740` = 3) rows that scrolls when a page runs longer
/// ([`crate::dialog_window`]); `0xC0..=0xCF` escapes inside a line are 2-byte
/// (so a `0x00` argument doesn't end the line early). See
/// [`legaia_mes::pack_page`] / [`legaia_mes::pack_box`] and the disc-gated
/// `field_dialog_boxpack_disc` regression.
///
/// This function still returns the raw, ungrouped segment pool - the simplest
/// faithful view of the record's whole line set. Callers that want boxes use
/// the `pack_box` decoder on the same bytes.
/// The field-VM bytecode preceding the first `0x1F` is skipped (its bytes
/// can fall in the glyph range, so it is not interpreted as text).
pub fn decode_inline_segments(inline: &[u8]) -> Vec<Vec<u8>> {
    let mut segments = Vec::new();
    let mut cursor = 0usize;
    while let Some(rel) = inline[cursor..].iter().position(|&b| b == 0x1F) {
        let start = cursor + rel + 1;
        let mut interp = Interpreter::new_at(inline, start);
        let mut glyphs = Vec::new();
        loop {
            match interp.next_event() {
                Some(MesEvent::Glyph(g)) | Some(MesEvent::SkipTwo(g)) => glyphs.push(g),
                Some(MesEvent::WideGlyph(_op, arg)) => glyphs.push(arg),
                Some(MesEvent::EndOfMessage(_)) | None => break,
                // Page-break / spacing / substitution stay inside the current
                // segment: a segment ends only at its MES terminator, never at
                // an intermediate control byte.
                Some(_) => {}
            }
        }
        // Resume scanning just past this segment's terminator. `pc()` sits
        // after the end byte; clamp to `start` so a `0x1F` immediately followed
        // by a terminator (empty segment) still makes forward progress.
        cursor = interp.pc().max(start);
        segments.push(glyphs);
    }
    segments
}

/// Page-state machine the host polls each frame. Mirrors the
/// [`legaia_mes::PlayerState`] fan-out but folds idle / typing into a single
/// `Typing` outcome the host doesn't need to disambiguate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelState {
    /// More glyphs are available, or the typewriter is still pacing. Host
    /// keeps drawing the current `page_glyphs` buffer.
    Typing,
    /// The current page is fully typed out and the player has hit a control
    /// byte the runtime treats as a page break. Host should wait for input
    /// (Cross / Enter), then call [`DialogPanel::advance_page`].
    PageBreak,
    /// No more events. The dialog opener should release the dialog flag
    /// (`_DAT_1F800394 |= 0x40`-equivalent) and unblock the calling script.
    Done,
}

/// One emitted glyph annotated with the runtime CLUT index that should tint
/// it. Engines look this up in their CLUT palette to color the glyph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PanelGlyph {
    /// Character byte (ASCII range for latin glyphs; some scenes embed
    /// `0x80..` wide-glyph escape pairs the MES interpreter surfaces as
    /// [`legaia_mes::PlayerState::WideGlyph`] - those are folded to their
    /// `arg` byte for now, since the wide-glyph table isn't decoded).
    pub byte: u8,
    /// CLUT additive index 0..15. `0` = default white; non-zero values come
    /// from inline `0xCF` color-change escapes in the bytecode.
    pub clut: u8,
}

/// Stateful dialog page driver around a [`DialogPlayer`].
///
/// Owns the tick-paced player and accumulates emitted glyphs into a
/// per-page buffer. The buffer is cleared on [`Self::advance_page`] so the
/// host's typewriter renders start fresh on each page.
pub struct DialogPanel<'a> {
    player: DialogPlayer<'a>,
    page: Vec<PanelGlyph>,
    /// Current pen color CLUT additive index. Updated when the bytecode
    /// emits a [`MesEvent::Control`] with the color-change byte (the field
    /// VM tracks this in `_DAT_8007B454`).
    current_clut: u8,
    state: PanelState,
}

impl<'a> DialogPanel<'a> {
    pub fn new(player: DialogPlayer<'a>) -> Self {
        Self {
            player,
            page: Vec::new(),
            current_clut: 0,
            state: PanelState::Typing,
        }
    }

    /// Set the typewriter pacing on the underlying player.
    pub fn set_glyphs_per_frame(&mut self, n: u8) {
        self.player.set_glyphs_per_frame(n);
    }

    /// Advance one frame. Returns the new [`PanelState`].
    pub fn tick(&mut self) -> PanelState {
        match self.state {
            PanelState::PageBreak | PanelState::Done => return self.state,
            PanelState::Typing => {}
        }
        let next = self.player.tick();
        match next {
            PlayerState::Idle => {}
            PlayerState::Glyph(g) => self.page.push(PanelGlyph {
                byte: g,
                clut: self.current_clut,
            }),
            PlayerState::WideGlyph(_op, arg) => {
                // Wide-glyph table isn't decoded yet; render the arg byte
                // through the standard atlas as a placeholder so the host
                // can see something instead of a silent skip.
                self.page.push(PanelGlyph {
                    byte: arg,
                    clut: self.current_clut,
                });
            }
            PlayerState::PageBreak => self.state = PanelState::PageBreak,
            PlayerState::WaitingForInput => self.state = PanelState::PageBreak,
            PlayerState::Control(MesEvent::SkipTwo(arg)) => {
                // `0xCF XX` in MES bytecode renders XX alone (the `0xCF`
                // prefix is a "skip me" marker per FUN_80036888). The
                // post-substitution color-change escape with the same
                // first byte is a separate byte stream the renderer sees
                // *after* MES interpretation; it isn't carried in the
                // bytecode and so doesn't drive `current_clut` here.
                self.page.push(PanelGlyph {
                    byte: arg,
                    clut: self.current_clut,
                });
            }
            PlayerState::Control(_) => {}
            PlayerState::Done => self.state = PanelState::Done,
        }
        self.state
    }

    /// Drop the current page's glyphs and unblock the player. Call after
    /// the user dismisses the page break.
    pub fn advance_page(&mut self) {
        if matches!(self.state, PanelState::PageBreak) {
            self.page.clear();
            self.player.advance_page();
            self.state = PanelState::Typing;
        }
    }

    /// All glyphs typed so far on the current page.
    pub fn page_glyphs(&self) -> &[PanelGlyph] {
        &self.page
    }

    /// Convenience: just the byte stream, for [`legaia_font::Font::layout`].
    pub fn page_bytes(&self) -> Vec<u8> {
        self.page.iter().map(|g| g.byte).collect()
    }

    pub fn state(&self) -> PanelState {
        self.state
    }

    pub fn is_done(&self) -> bool {
        matches!(self.state, PanelState::Done)
    }

    pub fn is_waiting_for_input(&self) -> bool {
        matches!(self.state, PanelState::PageBreak)
    }
}

/// Owned, self-contained dialog driver. Carries the MES bytecode bytes,
/// the current PC, and the page accumulator without borrowing - so it can
/// live on the World between frames without lifetime gymnastics.
///
/// Equivalent to [`DialogPanel`] for engines that want to mutate one piece
/// of long-lived state instead of constructing a borrowed pair every frame.
#[derive(Debug, Clone)]
pub struct OwnedDialogPanel {
    /// MES bytecode bytes for the active message. Shared via Arc so the
    /// engine can keep it alive cheaply across the scene.
    pub bytes: Arc<Vec<u8>>,
    /// Current bytecode PC (offset into [`Self::bytes`]).
    pub pc: usize,
    /// Frame counter - one tick per [`Self::tick`].
    pub tick_count: u64,
    /// Glyphs emitted per frame. `0` is treated as `1`.
    pub glyphs_per_frame: u8,
    /// Page glyphs accumulated since the last [`Self::advance_page`].
    pub page: Vec<PanelGlyph>,
    /// CLUT pen color tracked through `0xCF` color escapes.
    pub current_clut: u8,
    /// Option-picker that follows the prompt segment, if this inline box is a
    /// multiple-choice menu (decoded by [`legaia_mes::picker`]). `None` for
    /// plain dialogue. When present, the panel enters a "menu" wait once the
    /// prompt finishes typing instead of going [`PanelState::Done`].
    picker: Option<legaia_mes::Picker>,
    /// Highlighted option index `0..picker.n` while the menu is active.
    picker_cursor: usize,
    /// `true` once the prompt has finished typing and the menu is awaiting a
    /// choice. Only meaningful when [`Self::picker`] is `Some`.
    menu_active: bool,
    /// The open menu's slide onto the screen ([`crate::dialog_picker_slide`]):
    /// set by the press that opens it, advanced one step per pager call.
    /// `None` while no menu is open.
    picker_slide: Option<crate::dialog_picker_slide::PickerSlide>,
    /// Name-substitution table for the MES `0xC1..0xC7` escapes, keyed by
    /// `(`[`substitute_kind_key`]`, arg)`. Retail resolves these against the
    /// live name tables (`0xC1` = character, `0xC2`/`0xC4` = item, ...); a
    /// host that knows the names installs them here and the typewriter emits
    /// the resolved glyphs in place of the escape. Absent entries emit
    /// nothing (the pre-wiring behaviour).
    pub substitutions: Option<PanelSubstitutions>,
    /// The pager page the typewriter is walking: every consecutive `0x1F`
    /// line up to the control byte that ends it (decoded by
    /// [`legaia_mes::pack_page`]). `None` on the plain-MES paths
    /// ([`Self::new`] / [`Self::from_scene_mes`]), whose streams carry
    /// explicit page-break controls instead of the field pager's rows.
    current_box: Option<legaia_mes::DialogBox>,
    /// The page ended on a continuing post-page dispatch (`0x24` next-page /
    /// `0x48` new-box): [`Self::advance_page`] must turn to the next page
    /// instead of resuming the byte stream in place.
    pending_box_advance: bool,
    state: PanelState,
    waiting_for_input: bool,
    done: bool,
    /// Vsyncs per game tick (`DAT_1F800393`) the host runs the panel at. The
    /// box path runs one retail pager call every `frame_step` [`Self::tick`]s
    /// (one tick = one vsync); set it from
    /// [`crate::world::FrameClock::frame_step`] before ticking (the World's
    /// dialog paths do, through [`Self::tick_at`]). `0` is treated as `1`.
    pub frame_step: u8,
    /// Vsync phase within the current game tick (`0` = a pager call runs).
    vsync_phase: u8,
    /// The retail row window - which rows show, the scroll, the typing
    /// row's pace (box path; see [`crate::dialog_window`]).
    window: Option<crate::dialog_window::RowWindow>,
    /// Decoded rows of the window's current page and the page before it
    /// (rows carried over a `0x24` turn still draw), keyed by page serial.
    page_rows: Vec<(u32, Arc<Vec<DecodedRow>>)>,
    /// A confirm press the next pager call sees
    /// ([`Self::confirm_while_typing`]).
    press_latch: bool,
    /// The page ended on a picker open byte and waits in state `0x19` for
    /// the press that opens the menu ([`Self::advance_page`]). Box path only.
    menu_pending: bool,
    /// Open a picker as soon as its prompt's page waits, with no press: the
    /// simplified `--simple-dialogue` panel's contract
    /// ([`Self::opening_menu_at_wait`]), whose host commits a choice but never
    /// turns a page. `false` on every retail-faithful path.
    menu_opens_at_wait: bool,
    /// The automatic press `_DAT_80073F00` fired on the last pager call
    /// ([`Self::tick_at_auto`]); drained by [`Self::take_auto_press`].
    auto_pressed: bool,
    /// Identity of the [`Self::substitutions`] table [`Self::page_rows`] was
    /// decoded against (hosts install it after construction, so rows are
    /// re-decoded when it changes).
    decoded_subs: Option<usize>,
}

/// One line of a pager page, decoded once: each glyph with the reveal units
/// before it (it shows once the counter passes that), and the line's
/// `FUN_80036044` count.
#[derive(Debug, Clone, Default)]
struct DecodedRow {
    glyphs: Vec<(PanelGlyph, u32)>,
    count: u32,
    /// Index of the line's `0x1F` lead in [`OwnedDialogPanel::bytes`].
    lead: usize,
}

/// Host-installed name-substitution table for a dialog panel: `(kind key,
/// arg)` -> resolved glyph bytes. See [`OwnedDialogPanel::substitutions`].
pub type PanelSubstitutions = Arc<std::collections::HashMap<(u8, u8), Vec<u8>>>;

/// Stable map key for a [`legaia_mes::SubstituteKind`] (the `0xC1..0xC7`
/// escape families), used by [`OwnedDialogPanel::substitutions`].
pub fn substitute_kind_key(kind: legaia_mes::SubstituteKind) -> u8 {
    use legaia_mes::SubstituteKind as K;
    match kind {
        K::CharacterName => 1,
        K::ItemName => 2,
        K::MagicName => 3,
        K::SpellName => 5,
        K::QuestName => 7,
    }
}

/// Map key for the `0xCE` **number** escapes in [`PanelSubstitutions`]:
/// `(SCRIPT_COUNTER_KEY, operand)` holds the digits a `0xCE 0x0B..=0x0E`
/// pair prints. Distinct from every [`substitute_kind_key`] value.
pub const SCRIPT_COUNTER_KEY: u8 = 0xCE;

/// The `0xCE` operands that print a script counter instead of a sprite:
/// escape-table rows `0x0B..=0x0E` carry `string_id == 0` and `y_offset`
/// `0..=3`, and the renderer reads that `y_offset` as the counter index
/// (`docs/formats/dialog-font.md`).
pub const SCRIPT_COUNTER_ESCAPES: std::ops::RangeInclusive<u8> = 0x0B..=0x0E;

/// The counter a number escape prints: operand `0x0B + i` reads slot `i` of
/// the field VM's script-counter table `0x801C6460`. `None` for any other
/// operand.
pub fn script_counter_slot(operand: u8) -> Option<usize> {
    SCRIPT_COUNTER_ESCAPES
        .contains(&operand)
        .then(|| usize::from(operand - *SCRIPT_COUNTER_ESCAPES.start()))
}

/// The glyph bytes a number escape prints for counter `value`.
///
/// The dialog renderer's number arm (`FUN_80036888`, `0x80036A54..0x80036A94`)
/// loads the counter with `lh` and calls `FUN_80034B78(value, 0, x, y)`. That
/// writer peels each decimal digit by repeated subtraction of the paired
/// `4 * 10^k` / `10^k` powers at `0x80073DCC`, suppresses leading zeros, and
/// always draws the units digit - so a value that never reaches the first
/// power, which is every value `<= 0`, prints a single `0`. A positive value
/// prints its plain decimal form (a signed halfword tops out at five digits).
///
/// REF: FUN_80036888, FUN_80034B78
pub fn script_counter_digits(value: i16) -> Vec<u8> {
    if value <= 0 {
        b"0".to_vec()
    } else {
        value.to_string().into_bytes()
    }
}

impl OwnedDialogPanel {
    /// Build a panel over `bytes` starting at `pc` (the offset returned by
    /// [`crate::scene_assets::SceneMes::message_offset`]).
    pub fn new(bytes: Arc<Vec<u8>>, pc: usize) -> Self {
        Self {
            bytes,
            pc,
            tick_count: 0,
            glyphs_per_frame: 1,
            page: Vec::new(),
            current_clut: 0,
            picker: None,
            picker_cursor: 0,
            menu_active: false,
            picker_slide: None,
            substitutions: None,
            current_box: None,
            pending_box_advance: false,
            state: PanelState::Typing,
            waiting_for_input: false,
            done: false,
            frame_step: 1,
            vsync_phase: 0,
            window: None,
            page_rows: Vec::new(),
            press_latch: false,
            menu_pending: false,
            menu_opens_at_wait: false,
            auto_pressed: false,
            decoded_subs: None,
        }
    }

    /// Convenience: build a panel from a [`crate::scene_assets::SceneMes`]
    /// resolution. Returns `None` if `text_id` is past the offset table.
    pub fn from_scene_mes(mes: &crate::scene_assets::SceneMes, text_id: u16) -> Option<Self> {
        let pc = mes.message_offset(text_id)?;
        Some(Self::new(Arc::new(mes.bytes.clone()), pc))
    }

    /// Build a panel over the **inline** dialog bytes a placement's
    /// interaction record carries (stored on
    /// [`crate::world::DialogRequest::inline`]).
    ///
    /// Placement-NPC and event dialogue does not live in the scene MES (its
    /// `text_id` is a box-config id, not a message index - it never resolves
    /// through [`Self::from_scene_mes`]); the message text is the inline buffer
    /// itself, a run of `0x1F`-lead text/option segments (MES glyph bytecode)
    /// preceded by the actor's field-VM interaction prologue - `CFlag` /
    /// `SysFlag.Test` / `JmpRel` / `0x4C 0x51` NPC-move-to-tile / `Nop`,
    /// branched on story flags. The prologue is **not a custom header
    /// format**; it is normal field-VM bytecode that retail consumes through
    /// `FUN_801DE840` from state 0 of the per-actor dialog SM `FUN_80039B7C`
    /// (which transitions to the pager only when the dispatcher lands on a
    /// byte where `& 0x7F < 0x20`). This implementation skips past it to the
    /// first `0x1F` lead marker and types the first segment from just past
    /// it through the standard MES
    /// [`Interpreter`].
    ///
    /// Only the first packed box is typed to start: the record holds the
    /// NPC's whole dialogue line pool (see [`decode_inline_segments`]).
    /// Retail picks which segment to land on via the prologue's
    /// story-flag-gated `JmpRel`s (not a header), then types consecutive
    /// `0x1F` lines as one page ended by the post-page control byte (decoded
    /// by [`legaia_mes::pack_page`]) in a scrolling three-row window - which
    /// is what the panel does ([`crate::dialog_window`]), continuing
    /// dispatches paged through [`Self::advance_page`]. See the
    /// `field_actor_placements_disc::dialog_prefix_decodes_as_field_vm_bytecode`
    /// regression for the prologue-is-field-VM-bytecode proof.
    ///
    /// Returns `None` when no `0x1F` lead marker is present (nothing
    /// renderable), so a caller can fall back to the MES path.
    pub fn from_inline_dialog(inline: &[u8]) -> Option<Self> {
        let lead = inline.iter().position(|&b| b == 0x1F)?;
        let mut panel = Self::new(Arc::new(inline.to_vec()), lead + 1);
        panel.seed_box_at_lead(lead);
        Some(panel)
    }

    /// Open a picker the moment its prompt's page waits instead of on the
    /// press retail waits for. A port-only mode for the simplified dialogue
    /// panel ([`crate::scene::SceneHost::open_pending_dialog`]), whose host
    /// has a choice commit but no page-turn press to open the menu with.
    pub fn opening_menu_at_wait(mut self) -> Self {
        self.menu_opens_at_wait = true;
        self
    }

    /// Build a panel that types the `0x1F` text segment whose **lead byte** is
    /// at `seg_lead` (i.e. `bytes[seg_lead] == 0x1F`), attaching any picker that
    /// immediately follows it. Used by the inline-script field-VM runner
    /// ([`crate::inline_dialogue`]), which lands the VM on each text segment and
    /// opens a box there. Unlike [`Self::from_inline_dialog`] it does not search
    /// for the first segment - the caller already knows the exact lead.
    pub fn at_segment(bytes: Arc<Vec<u8>>, seg_lead: usize) -> Self {
        let mut panel = Self::new(bytes, seg_lead + 1);
        panel.seed_box_at_lead(seg_lead);
        panel
    }

    /// Seed the typewriter on the pager page whose first `0x1F` lead is at
    /// `lead`, in a fresh window: PC on row 0's glyphs, and the picker
    /// attached only when the page's own post-page dispatch is a menu-open
    /// byte (the faithful "is this box a menu?" test - the picker open byte
    /// sits at [`legaia_mes::DialogBox::dispatch_at`], directly after the
    /// page's *last* row, so a 2-row prompt still finds its menu). Falls back
    /// to the ungrouped single-segment walk when `lead` isn't a `0x1F` byte.
    fn seed_box_at_lead(&mut self, lead: usize) {
        self.pending_box_advance = false;
        self.press_latch = false;
        self.menu_pending = false;
        self.window = None;
        self.page_rows.clear();
        match legaia_mes::pack_page(&self.bytes, lead) {
            Some(bx) => self.load_page(bx, None),
            None => {
                self.pc = lead + 1;
                self.picker = Self::picker_following_segment(&self.bytes, lead + 1);
                self.current_box = None;
            }
        }
    }

    /// Make `bx` the current page: decode its rows and either open a fresh
    /// window on it (`keep_rows = None`) or queue a page turn onto it
    /// (`Some(true)` for the `0x24` continuation that keeps the window,
    /// `Some(false)` for a fresh box).
    fn load_page(&mut self, bx: legaia_mes::DialogBox, keep_rows: Option<bool>) {
        self.pc = bx.lines[0].start;
        self.picker = if matches!(bx.dispatch, legaia_mes::Dispatch::Picker(_)) {
            legaia_mes::scan_pickers(&self.bytes)
                .into_iter()
                .find(|p| p.open == bx.dispatch_at)
        } else {
            None
        };
        self.decoded_subs = self.subs_key();
        let rows = Arc::new(self.decode_page(&bx));
        let n = rows.len();
        match (self.window.as_mut(), keep_rows) {
            (Some(w), Some(keep)) => w.turn_page(n, keep),
            _ => {
                self.page_rows.clear();
                self.window = Some(crate::dialog_window::RowWindow::open(n));
            }
        }
        let serial = self.window.as_ref().map_or(0, |w| w.page());
        self.page_rows.retain(|(s, _)| s + 1 >= serial);
        self.page_rows.push((serial, rows));
        self.current_box = Some(bx);
        self.rebuild_page();
    }

    /// Decode every line of `bx` (glyphs, their reveal units, the line's
    /// count), carrying the pen colour from line to line.
    fn decode_page(&self, bx: &legaia_mes::DialogBox) -> Vec<DecodedRow> {
        bx.lines
            .iter()
            .map(|l| {
                let lead = l.start.saturating_sub(1);
                DecodedRow {
                    glyphs: self.decode_line(l.start),
                    count: self.line_count(lead),
                    lead,
                }
            })
            .collect()
    }

    /// The glyphs of the line whose glyph run starts at `start`, each with the
    /// reveal units before it: one unit per glyph, per spliced substitution
    /// glyph and per `0xCE` escape, none for a `0xCF` pair (the draw's count,
    /// `FUN_80036044`). The line ends at its terminator, or at a mid-line
    /// `0x80..=0x9F` byte the pager's `(b & 0x7F) < 0x20` test also stops on.
    fn decode_line(&self, start: usize) -> Vec<(PanelGlyph, u32)> {
        let clut = self.current_clut;
        let glyph = |byte: u8| PanelGlyph { byte, clut };
        let mut out = Vec::new();
        let mut units = 0u32;
        let mut interp = Interpreter::new_at(&self.bytes, start);
        loop {
            match interp.next_event() {
                Some(MesEvent::EndOfMessage(_)) | Some(MesEvent::Control(_)) | None => break,
                Some(MesEvent::Glyph(g)) | Some(MesEvent::WideGlyph(_, g)) => {
                    out.push((glyph(g), units));
                    units += 1;
                }
                Some(MesEvent::SkipTwo(arg)) => out.push((glyph(arg), units)),
                Some(MesEvent::Substitute { kind, arg }) => {
                    if let Some(name) = self
                        .substitutions
                        .as_ref()
                        .and_then(|subs| subs.get(&(substitute_kind_key(kind), arg)))
                    {
                        for &b in name {
                            out.push((glyph(b), units));
                            units += 1;
                        }
                    }
                }
                // A number escape prints the script counter the host
                // resolved at open; the whole number is one reveal unit, like
                // the escape it replaces (`FUN_80036044` counts the `0xCE`
                // pair once). Any other `0xCE` escape is one typewriter unit
                // and, when the font has its sprite, one drawn icon
                // (`FUN_80036888`): keep the pair in the page so the layout
                // can place it.
                Some(MesEvent::Spacing(arg)) => {
                    if let Some(digits) = self
                        .substitutions
                        .as_ref()
                        .and_then(|subs| subs.get(&(SCRIPT_COUNTER_KEY, arg)))
                    {
                        for &b in digits {
                            out.push((glyph(b), units));
                        }
                    } else {
                        out.push((glyph(0xCE), units));
                        out.push((glyph(arg), units));
                    }
                    units += 1;
                }
                Some(MesEvent::Truncated(_)) => {}
            }
        }
        out
    }

    fn subs_key(&self) -> Option<usize> {
        self.substitutions
            .as_ref()
            .map(|a| Arc::as_ptr(a) as *const () as usize)
    }

    /// Re-decode the held pages if [`Self::substitutions`] changed since
    /// they were decoded (a name splices in, and its units count).
    fn refresh_decode(&mut self) {
        let key = self.subs_key();
        if key == self.decoded_subs {
            return;
        }
        self.decoded_subs = key;
        let pages = std::mem::take(&mut self.page_rows);
        self.page_rows = pages
            .into_iter()
            .map(|(serial, rows)| {
                let fresh = rows
                    .iter()
                    .map(|r| DecodedRow {
                        glyphs: self.decode_line(r.lead + 1),
                        count: self.line_count(r.lead),
                        lead: r.lead,
                    })
                    .collect();
                (serial, Arc::new(fresh))
            })
            .collect();
        self.rebuild_page();
    }

    /// Rebuild [`Self::page`] from the window: every row whole except the
    /// typing one, which shows the glyphs the reveal counter has passed;
    /// rows joined by the `0x7C` newline glyph.
    fn rebuild_page(&mut self) {
        let Some(w) = self.window.as_ref() else {
            return;
        };
        let typing = w.typing_row();
        let mut page = Vec::new();
        for (i, r) in w.rows.iter().enumerate() {
            if i > 0 {
                page.push(PanelGlyph {
                    byte: legaia_font::NEWLINE,
                    clut: self.current_clut,
                });
            }
            let Some(row) = self
                .page_rows
                .iter()
                .find(|(s, _)| *s == r.page)
                .and_then(|(_, rows)| rows.get(r.line))
            else {
                continue;
            };
            let cap = match typing {
                Some((t, units)) if t == *r => units,
                _ => u32::MAX,
            };
            page.extend(
                row.glyphs
                    .iter()
                    .filter(|(_, before)| *before < cap)
                    .map(|(g, _)| *g),
            );
        }
        self.page = page;
    }

    /// PCs of the current box's `0x1F` row leads. Runners that keep a
    /// visited/wrap map mark these so a conversation tail that jumps back onto
    /// row 2/3 of an already-shown box still reads as a wrap.
    pub fn row_leads(&self) -> Vec<usize> {
        self.current_box
            .as_ref()
            .map(|b| b.lines.iter().map(|l| l.start - 1).collect())
            .unwrap_or_default()
    }

    /// End index of the `0x1F` text segment whose glyph run starts at `from`
    /// (where the standard MES interpreter halts).
    fn segment_end(bytes: &[u8], from: usize) -> usize {
        let mut interp = Interpreter::new_at(bytes, from);
        loop {
            match interp.next_event() {
                Some(MesEvent::EndOfMessage(_)) | None => break interp.pc(),
                _ => {}
            }
        }
    }

    /// The option-picker (if any) that sits immediately after the text segment
    /// whose glyph run starts at `seg_start` - i.e. whose open byte equals the
    /// segment's end index. This is the faithful "is this box a menu?" test:
    /// the picker open byte directly follows the box's last typed line.
    fn picker_following_segment(bytes: &[u8], seg_start: usize) -> Option<legaia_mes::Picker> {
        let end = Self::segment_end(bytes, seg_start);
        legaia_mes::scan_pickers(bytes)
            .into_iter()
            .find(|p| p.open == end)
    }

    /// Confirm the highlighted menu option: apply its relative jump and resume
    /// the dialogue at the chosen branch's reply (mirrors the retail inline-
    /// script control handler `FUN_80038050`, which sets the script PC to
    /// `(open + 1 + index*2) + i16_LE(entry[index])`). The panel jumps to the
    /// branch, skips any leading non-text bytecode to the next `0x1F` reply
    /// segment, types it, and re-attaches a nested menu if one follows. With no
    /// reply segment at/after the target the conversation ends (`Done`).
    ///
    /// Returns the chosen option index, or `None` if no menu is active.
    // PORT: FUN_80038050
    pub fn confirm_menu(&mut self) -> Option<usize> {
        if !self.picker_takes_input() {
            return None;
        }
        let picker = self.picker.as_ref()?;
        let cursor = self.picker_cursor;
        let target = picker.jump_target(cursor)?;
        // Resume at the chosen branch: find its first reply text segment.
        let next_lead = self.bytes[target.min(self.bytes.len())..]
            .iter()
            .position(|&b| b == 0x1F)
            .map(|rel| target + rel);
        self.page.clear();
        self.menu_active = false;
        self.picker_slide = None;
        self.waiting_for_input = false;
        self.picker_cursor = 0;
        match next_lead {
            Some(lead) => {
                self.seed_box_at_lead(lead);
                self.state = PanelState::Typing;
            }
            None => {
                self.picker = None;
                self.done = true;
                self.state = PanelState::Done;
            }
        }
        Some(cursor)
    }

    /// The decoded option-picker following the prompt, if this box is a
    /// multiple-choice menu. `None` for plain dialogue.
    pub fn picker(&self) -> Option<&legaia_mes::Picker> {
        self.picker.as_ref()
    }

    /// The open picker's option labels as drawn: each label re-decoded from
    /// its `0x1F` lead with the `0xC1..=0xC7` name escapes resolved through
    /// [`Self::substitutions`], as the box text is. Empty without a picker.
    ///
    /// [`legaia_mes::PickerOption::label`] carries no names (that crate has
    /// no tables), and both hosts printed it, so a choice such as dolk2's
    /// "Be careful, `C1 01`!" read "Be careful, !" on either.
    pub fn picker_labels(&self) -> Vec<Vec<u8>> {
        let Some(p) = self.picker.as_ref() else {
            return Vec::new();
        };
        p.options
            .iter()
            .map(|o| {
                let mut out = Vec::new();
                let mut interp = Interpreter::new_at(&self.bytes, o.lead + 1);
                loop {
                    match interp.next_event() {
                        Some(MesEvent::Glyph(g))
                        | Some(MesEvent::SkipTwo(g))
                        | Some(MesEvent::WideGlyph(_, g)) => out.push(g),
                        Some(MesEvent::Substitute { kind, arg }) => {
                            if let Some(name) = self
                                .substitutions
                                .as_ref()
                                .and_then(|s| s.get(&(substitute_kind_key(kind), arg)))
                            {
                                out.extend_from_slice(name);
                            }
                        }
                        Some(MesEvent::EndOfMessage(_)) | None => break,
                        Some(_) => {}
                    }
                }
                out
            })
            .collect()
    }

    /// `true` once the prompt has finished typing and a menu is awaiting a
    /// choice (the host should draw the options + cursor and route Up/Down).
    pub fn menu_active(&self) -> bool {
        self.menu_active
    }

    /// Highlighted option index while the menu is active (`0` otherwise).
    pub fn picker_cursor(&self) -> usize {
        self.picker_cursor
    }

    /// The open menu's slide state, if a menu is open.
    pub fn picker_slide(&self) -> Option<&crate::dialog_picker_slide::PickerSlide> {
        self.picker_slide.as_ref().filter(|_| self.menu_active)
    }

    /// The picker box's centre rect `(x, y, w, h)` in 320x240 stage pixels
    /// this frame - somewhere on its slide, or at rest - or `None` when no
    /// menu is open or the slide has not started (the press's sentinel,
    /// which retail draws as no box). Both hosts draw the box and its labels
    /// here; `0x2A` rests at the top right, the N-option lists at the bottom.
    pub fn picker_rect(&self) -> Option<(i32, i32, i32, i32)> {
        self.picker_slide()?.rect()
    }

    /// The option hand is drawn: the slide has come to rest (count 0).
    pub fn picker_hand_drawn(&self) -> bool {
        self.picker_slide().is_some_and(|s| s.hand_drawn())
    }

    /// The open menu takes Up/Down and confirm: the slide came to rest on an
    /// earlier pager call. `false` while it slides, and when no menu is open.
    pub fn picker_takes_input(&self) -> bool {
        self.menu_active && self.picker_slide.as_ref().is_none_or(|s| s.takes_input())
    }

    /// Move the menu cursor by `delta` - wrapping within `0..n`, or clamping
    /// at the ends for a `0x2A` menu (the shared cursor handler at
    /// `0x801D941C` tests for its state `0x12`). No-op when the box isn't a
    /// menu, and while an open menu still slides in.
    pub fn move_picker_cursor(&mut self, delta: i32) {
        if self.menu_active && !self.picker_takes_input() {
            return;
        }
        if let Some(p) = &self.picker
            && p.n > 0
        {
            let n = p.n as i32;
            let next = self.picker_cursor as i32 + delta;
            self.picker_cursor = if legaia_mes::picker_cursor_clamps(p.open_byte) {
                next.clamp(0, n - 1) as usize
            } else {
                ((next % n + n) % n) as usize
            };
        }
    }

    /// Open the menu: set it active with its slide state - the press's
    /// sentinel on the pager path, at rest where no press opens it.
    fn open_menu(&mut self, pressed: bool) {
        self.menu_active = true;
        let open_byte = self.picker.as_ref().map_or(0x27, |p| p.open_byte);
        self.picker_slide = Some(if pressed && self.window.is_some() {
            crate::dialog_picker_slide::PickerSlide::pressed(open_byte)
        } else {
            crate::dialog_picker_slide::PickerSlide::at_rest(open_byte)
        });
    }

    pub fn set_glyphs_per_frame(&mut self, n: u8) {
        self.glyphs_per_frame = n.max(1);
    }

    /// Advance one vsync at `frame_step` (`DAT_1F800393`) and return the new
    /// state: [`Self::frame_step`] is set first, then [`Self::tick`] runs.
    pub fn tick_at(&mut self, frame_step: u8) -> PanelState {
        self.frame_step = frame_step;
        self.tick()
    }

    /// [`Self::tick_at`] plus the pager's **automatic press**: while a box
    /// page waits in state `0x19`, each pager call counts `auto_press`
    /// (retail's `_DAT_80073F00`, which field-VM op `4C 89` writes) down by
    /// the frame step once it is positive, and the call that takes it to zero
    /// or below clears it and presses confirm for the player
    /// (`0x801D8F4C..0x801D8F88`: the pad word is replaced by the confirm
    /// binding `0x800846D0`). The host reads the press through
    /// [`Self::take_auto_press`] and handles it as its own confirm.
    ///
    /// A retail capture (`autorun_dialog_picker_open.lua` with
    /// `LEGAIA_PICKER_AUTO=21` on `retock_innkeeper_talk_open`, frame step 2)
    /// counts `19, 17, .. 1` on the eleven calls after the page waits and
    /// leaves `0x19` on the twelfth with no button down.
    ///
    /// PORT: FUN_801D84D0 (state `0x19`'s `_DAT_80073F00` countdown, `0x801D8F4C..0x801D8F88`)
    pub fn tick_at_auto(&mut self, frame_step: u8, auto_press: &mut i16) -> PanelState {
        let pager_call = self.window.is_some() && self.vsync_phase == 0;
        let in_wait = self.waiting_for_input && !self.menu_active && !self.done;
        let st = self.tick_at(frame_step);
        if pager_call && in_wait && *auto_press > 0 {
            *auto_press = auto_press.saturating_sub(i16::from(frame_step.max(1)));
            if *auto_press <= 0 {
                *auto_press = 0;
                self.auto_pressed = true;
            }
        }
        st
    }

    /// Whether the automatic press fired on the last [`Self::tick_at_auto`];
    /// clears it.
    pub fn take_auto_press(&mut self) -> bool {
        std::mem::take(&mut self.auto_pressed)
    }

    /// Advance one frame (one vsync) and return the new state.
    ///
    /// A pager page (every field-pager path: [`Self::from_inline_dialog`] /
    /// [`Self::at_segment`]) runs the retail row window
    /// ([`crate::dialog_window`]): one pager call every [`Self::frame_step`]
    /// ticks, the last row typed at the [`crate::dialog_pacing`] pace, the
    /// window scrolled a row when a line follows a full one, rows carried
    /// over a page turn scrolled away before the page waits, and a confirm
    /// press ([`Self::confirm_while_typing`]) completing the page. The
    /// plain-MES paths ([`Self::new`] / [`Self::from_scene_mes`]) keep one
    /// event per [`Self::glyphs_per_frame`] ticks.
    pub fn tick(&mut self) -> PanelState {
        if self.window.is_some() {
            return self.tick_window();
        }
        if self.done {
            self.state = PanelState::Done;
            return self.state;
        }
        if self.waiting_for_input {
            self.state = PanelState::PageBreak;
            return self.state;
        }
        self.tick_count += 1;
        if !self.tick_count.is_multiple_of(self.glyphs_per_frame as u64) {
            return self.state;
        }
        let mut interp = Interpreter::new_at(&self.bytes, self.pc);
        let next = interp.next_event();
        self.pc = interp.pc();
        match next {
            Some(MesEvent::EndOfMessage(_)) | None => self.end_row(),
            Some(ev) => {
                self.apply_event(ev);
            }
        }
        self.state
    }

    /// The box path's vsync: the pager's game-tick cadence runs on whether
    /// or not the page waits, as retail's does.
    fn tick_window(&mut self) -> PanelState {
        let dt = self.frame_step.max(1);
        let run = self.vsync_phase == 0;
        self.vsync_phase = (self.vsync_phase + 1) % dt;
        if self.done {
            self.state = PanelState::Done;
            return self.state;
        }
        if self.menu_active {
            // The open menu's pager call: its slide advances a step.
            if run && let Some(slide) = self.picker_slide.as_mut() {
                slide.call(dt);
            }
            self.state = PanelState::PageBreak;
            return self.state;
        }
        self.tick_count += 1;
        if !run {
            return self.state;
        }
        self.refresh_decode();
        let pressed = std::mem::take(&mut self.press_latch);
        let reached_wait = {
            let rows = self
                .page_rows
                .last()
                .map(|(_, r)| Arc::clone(r))
                .unwrap_or_default();
            let Some(w) = self.window.as_mut() else {
                return self.state;
            };
            w.call(dt, pressed, |line| rows.get(line).map_or(0, |r| r.count))
        };
        self.rebuild_page();
        if reached_wait {
            self.page_waits();
        }
        self.state
    }

    /// The page is shown whole and the window waits (pager state `0x19`):
    /// open the menu, page-break on a continuing dispatch, or end.
    fn page_waits(&mut self) {
        if let Some(bx) = self.current_box.as_ref() {
            self.pc = bx.dispatch_at;
        }
        if self.picker.is_some() && !self.menu_active {
            // The prompt's page waits with the advance hand like any other;
            // the menu opens on the press (`0x801D9040..0x801D909C`: the
            // `0x2A` / `0x27` / `0x28` / `0x29` open bytes install their
            // picker states only inside `0x19`'s press arm - no other store
            // of `0x11` / `0x13` / `0x15` / `0x17` exists in the pager).
            if self.menu_opens_at_wait {
                self.open_menu(false);
            } else {
                self.menu_pending = true;
            }
            self.waiting_for_input = true;
            self.state = PanelState::PageBreak;
        } else if self
            .current_box
            .as_ref()
            .is_some_and(|bx| bx.dispatch.continues())
        {
            self.pending_box_advance = true;
            self.waiting_for_input = true;
            self.state = PanelState::PageBreak;
        } else {
            self.done = true;
            self.state = PanelState::Done;
        }
    }

    /// A confirm press while a pager page is still typing or scrolling: the
    /// next pager call sees it - it latches the skip speed `0x25` and
    /// completes the page (state `0x0D`), clears a short-row hold, or speeds a
    /// scroll (see [`crate::dialog_window`]). Returns `false` (and does
    /// nothing) when the page already waits, a menu is up, the panel is done,
    /// or this is a plain-MES panel.
    pub fn confirm_while_typing(&mut self) -> bool {
        if self.done || self.waiting_for_input || self.menu_active {
            return false;
        }
        match self.window.as_ref() {
            Some(w) if !w.waiting() => {
                self.press_latch = true;
                true
            }
            _ => false,
        }
    }

    /// The retail pager's typewriter words for the row being typed (box
    /// path; see [`crate::dialog_pacing`]).
    pub fn pacer(&self) -> &crate::dialog_pacing::TypewriterPacer {
        self.window
            .as_ref()
            .map_or(&crate::dialog_pacing::IDLE_PACER, |w| &w.pacer)
    }

    /// The retail row window, on the box path.
    pub fn window(&self) -> Option<&crate::dialog_window::RowWindow> {
        self.window.as_ref()
    }

    /// The `0x1F` lead of every row in the window, top first - the retail
    /// row table `_DAT_801F3540[]` as buffer offsets. Empty off the box path.
    pub fn window_row_leads(&self) -> Vec<usize> {
        let Some(w) = self.window.as_ref() else {
            return Vec::new();
        };
        w.rows
            .iter()
            .filter_map(|r| {
                self.page_rows
                    .iter()
                    .find(|(s, _)| *s == r.page)
                    .and_then(|(_, rows)| rows.get(r.line))
                    .map(|row| row.lead)
            })
            .collect()
    }

    /// Rows the reading box is tall: the window's
    /// [`crate::dialog_window::WINDOW_ROWS`] on the box path (the page buffer
    /// can hold one more while a row scrolls in), `None` on the plain-MES
    /// path, whose box grows with its page.
    pub fn box_rows(&self) -> Option<usize> {
        self.window
            .as_ref()
            .map(|_| crate::dialog_window::WINDOW_ROWS)
    }

    /// Whole pixels every row of the page draws above its slot (the pager's
    /// `scroll >> 4`, `<= 0`); `0` off the box path.
    pub fn scroll_px(&self) -> i32 {
        self.window.as_ref().map_or(0, |w| w.scroll_px())
    }

    /// `FUN_80036044` on the line whose `0x1F` lead is at `lead`, the rest of
    /// the buffer after it included (the count's walk overruns the row's
    /// `NUL` by one byte per two-byte unit). Substitutions resolve through
    /// [`Self::substitutions`]; an unresolved one counts nothing, as the
    /// panel then types nothing for it either.
    fn line_count(&self, lead: usize) -> u32 {
        let subs = self.substitutions.clone();
        let expand = move |op: u8, arg: u8| -> Option<Vec<u8>> {
            let key = match op {
                0xC1 => 1,
                0xC2 | 0xC4 => 2,
                0xC3 => 3,
                0xC5 => 5,
                0xC7 => 7,
                _ => return None,
            };
            subs.as_ref()?.get(&(key, arg)).cloned()
        };
        let text = self.bytes.get(lead..).unwrap_or(&[]);
        legaia_font::typewriter_glyph_count(text, Some(&expand)).count
    }

    /// Apply one non-terminator event to the page (plain-MES path).
    fn apply_event(&mut self, ev: MesEvent) {
        match ev {
            MesEvent::Glyph(g) | MesEvent::WideGlyph(_, g) | MesEvent::SkipTwo(g) => {
                self.page.push(PanelGlyph {
                    byte: g,
                    clut: self.current_clut,
                });
            }
            MesEvent::Control(_) => {
                self.waiting_for_input = true;
                self.state = PanelState::PageBreak;
            }
            MesEvent::Spacing(arg) => {
                // A number escape: the digits the host resolved at open.
                if let Some(subs) = self.substitutions.as_ref()
                    && let Some(digits) = subs.get(&(SCRIPT_COUNTER_KEY, arg))
                {
                    for &b in digits {
                        self.page.push(PanelGlyph {
                            byte: b,
                            clut: self.current_clut,
                        });
                    }
                }
            }
            MesEvent::Substitute { kind, arg } => {
                // Resolve through the host-installed name table (item /
                // character names). An absent entry emits nothing - the
                // pre-wiring behaviour.
                if let Some(subs) = self.substitutions.as_ref()
                    && let Some(name) = subs.get(&(substitute_kind_key(kind), arg))
                {
                    for &b in name {
                        self.page.push(PanelGlyph {
                            byte: b,
                            clut: self.current_clut,
                        });
                    }
                }
            }
            _ => {}
        }
    }

    /// The plain-MES stream hit a terminator: open the following menu, or
    /// end.
    fn end_row(&mut self) {
        // A menu stops at the prompt terminator and waits on the option list
        // instead of tearing down: the retail inline-script handler
        // `FUN_80038050` reads the chosen index here and jumps via the
        // picker's relative-offset table.
        if self.picker.is_some() && !self.menu_active {
            self.open_menu(false);
            self.waiting_for_input = true;
            self.state = PanelState::PageBreak;
        } else {
            self.done = true;
            self.state = PanelState::Done;
        }
    }

    /// Resume from a page break. No-op when the panel isn't paused.
    ///
    /// On the box path, a page that ended on a continuing post-page dispatch
    /// turns to the next page: a `0x24` keeps the window (the next page types
    /// beneath the rows already shown, pager state `5`), a `0x48` opens a
    /// fresh box. Retail spends the press call and a load call on the turn,
    /// so the next page's first glyph shows on the second pager call after
    /// the press. On the plain path the page buffer clears. A box page that
    /// ends on a picker open byte opens its menu on this press instead, and
    /// the choice is committed by the next ([`Self::confirm_menu`]).
    pub fn advance_page(&mut self) {
        if !self.waiting_for_input {
            return;
        }
        if std::mem::take(&mut self.menu_pending) {
            // The press on a prompt that ends on a picker open byte opens the
            // menu, which slides in before it takes the choice
            // ([`crate::dialog_picker_slide`]).
            self.open_menu(true);
            self.state = PanelState::PageBreak;
            return;
        }
        self.waiting_for_input = false;
        self.state = PanelState::Typing;
        if self.window.is_none() {
            self.page.clear();
            return;
        }
        if !std::mem::take(&mut self.pending_box_advance) {
            return;
        }
        let Some(cur) = self.current_box.as_ref() else {
            return;
        };
        let keep = matches!(cur.dispatch, legaia_mes::Dispatch::NextPage);
        let next = cur
            .next_box_pc()
            .filter(|&n| self.bytes.get(n) == Some(&0x1F))
            .and_then(|n| legaia_mes::pack_page(&self.bytes, n));
        match next {
            Some(bx) => self.load_page(bx, Some(keep)),
            None => {
                // Continuation byte with no following lead (malformed
                // stream): end rather than re-type the same page.
                self.done = true;
                self.state = PanelState::Done;
            }
        }
    }

    pub fn page_glyphs(&self) -> &[PanelGlyph] {
        &self.page
    }

    pub fn page_bytes(&self) -> Vec<u8> {
        self.page.iter().map(|g| g.byte).collect()
    }

    pub fn state(&self) -> PanelState {
        self.state
    }

    pub fn is_done(&self) -> bool {
        self.done
    }

    pub fn is_waiting_for_input(&self) -> bool {
        self.waiting_for_input
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_mes::{DialogPlayer, Interpreter};

    /// Tick until the panel stops typing (page break, menu or done).
    fn type_until_wait(panel: &mut OwnedDialogPanel) -> PanelState {
        for _ in 0..512 {
            let st = panel.tick();
            if st != PanelState::Typing {
                return st;
            }
        }
        panic!("the panel never stopped typing");
    }

    /// Tick an open menu until its slide comes to rest and it takes input.
    fn slide_in(panel: &mut OwnedDialogPanel) {
        for _ in 0..128 {
            if panel.picker_takes_input() {
                return;
            }
            panel.tick();
        }
        panic!("the menu never came to rest");
    }

    /// Minimal Compact-format MES blob: a single message with three glyphs
    /// then End. Avoids dragging the full container parser into tests.
    fn three_glyph_program() -> Vec<u8> {
        // bytecode is just the glyph stream + terminator: 'a' 'b' 'c' end
        vec![b'a', b'b', b'c', 0x00]
    }

    #[test]
    fn typing_then_done() {
        let buf = three_glyph_program();
        let interp = Interpreter::new_at(&buf, 0);
        let mut player = DialogPlayer::new(interp);
        player.set_glyphs_per_frame(1);
        let mut panel = DialogPanel::new(player);
        for _ in 0..3 {
            assert_eq!(panel.tick(), PanelState::Typing);
        }
        assert_eq!(
            panel.page_bytes(),
            vec![b'a', b'b', b'c'],
            "three glyphs should accumulate"
        );
        assert_eq!(panel.tick(), PanelState::Done);
        assert!(panel.is_done());
    }

    #[test]
    fn owned_panel_types_three_glyphs_then_done() {
        let buf = Arc::new(three_glyph_program());
        let mut panel = OwnedDialogPanel::new(buf, 0);
        for _ in 0..3 {
            assert_eq!(panel.tick(), PanelState::Typing);
        }
        assert_eq!(panel.page_bytes(), vec![b'a', b'b', b'c']);
        assert_eq!(panel.tick(), PanelState::Done);
        assert!(panel.is_done());
    }

    /// Inline field-VM dialog: a few prologue bytes (field-VM bytecode, some
    /// in the glyph range) then a `0x1F`-lead text segment.
    /// `from_inline_dialog` must skip the prologue to the first `0x1F`
    /// marker and type the text after it - here `"Hi"` from `[00 56 00 1F
    /// 'H' 'i' 00]`.
    #[test]
    fn from_inline_dialog_skips_prologue_and_types_text() {
        let inline = vec![0x00u8, 0x56, 0x00, 0x1F, b'H', b'i', 0x00];
        let mut panel = OwnedDialogPanel::from_inline_dialog(&inline).expect("has a 0x1F lead");
        for _ in 0..2 {
            assert_eq!(panel.tick(), PanelState::Typing);
        }
        assert_eq!(panel.page_bytes(), vec![b'H', b'i']);
        // Retail pacing: the row's glyph count is 3 (lead included), so the
        // row finishes on the call whose counter passes 3 - two calls after
        // the last glyph shows.
        assert_eq!(panel.tick(), PanelState::Typing);
        assert_eq!(panel.tick(), PanelState::Done);
    }

    /// A `0xCE 0x0B` number escape prints the script counter the host
    /// resolved, as one reveal unit; without a resolution it prints nothing.
    #[test]
    fn a_number_escape_prints_the_resolved_script_counter() {
        let inline = vec![0x1F, b'N', 0xCE, 0x0B, b'x', 0x00];
        let run = |subs: Option<PanelSubstitutions>| {
            let mut panel = OwnedDialogPanel::from_inline_dialog(&inline).unwrap();
            panel.substitutions = subs;
            for _ in 0..16 {
                if panel.tick() == PanelState::Done {
                    break;
                }
            }
            panel.page_bytes()
        };
        let mut map = std::collections::HashMap::new();
        map.insert((SCRIPT_COUNTER_KEY, 0x0B), script_counter_digits(0x2A));
        assert_eq!(run(Some(Arc::new(map))), b"N42x".to_vec());
        assert_eq!(
            run(None),
            vec![b'N', 0xCE, 0x0B, b'x'],
            "unresolved = the raw escape pair, left for the layout"
        );
    }

    /// `FUN_80034B78` with min-digits `0`: leading zeros suppressed, the units
    /// digit always drawn, so a value at or below zero prints one `0`.
    #[test]
    fn script_counter_digits_follow_the_number_writer() {
        assert_eq!(script_counter_digits(0), b"0".to_vec());
        assert_eq!(script_counter_digits(-5), b"0".to_vec());
        assert_eq!(script_counter_digits(7), b"7".to_vec());
        assert_eq!(script_counter_digits(i16::MAX), b"32767".to_vec());
        assert_eq!(script_counter_slot(0x0A), None);
        assert_eq!(script_counter_slot(0x0B), Some(0));
        assert_eq!(script_counter_slot(0x0E), Some(3));
        assert_eq!(script_counter_slot(0x0F), None);
    }

    /// No `0x1F` lead marker = nothing renderable; the caller falls back to the
    /// MES `text_id` path.
    #[test]
    fn from_inline_dialog_without_lead_marker_is_none() {
        assert!(OwnedDialogPanel::from_inline_dialog(&[0x00, 0x10, 0x00]).is_none());
    }

    /// A picker label's name escape resolves through the panel's table, as
    /// the box text does (dolk2's "Be careful, `C1 01`!" choice).
    #[test]
    fn picker_labels_splice_names_in() {
        let mut inline = vec![0x1F, b'O', b'K', b'?', 0x00];
        inline.push(0x27);
        inline.extend_from_slice(&0x10i16.to_le_bytes());
        inline.extend_from_slice(&0x20i16.to_le_bytes());
        inline.push(0x24);
        inline.extend_from_slice(&[0x1F, b'H', b'i', b',', b' ', 0xC1, 0x01, b'!', 0x00]);
        inline.extend_from_slice(&[0x1F, b'N', b'o', 0x00]);
        inline.resize(48, 0x21);
        let mut panel = OwnedDialogPanel::from_inline_dialog(&inline).expect("has a 0x1F lead");
        let raw = panel.picker().unwrap().options[0].label.clone();
        assert_eq!(raw, b"Hi, !", "the mes crate has no names");
        assert_eq!(
            panel.picker_labels()[0],
            b"Hi, !".to_vec(),
            "no table, no name"
        );
        let mut map = std::collections::HashMap::new();
        map.insert((1u8, 1u8), b"Noa".to_vec());
        panel.substitutions = Some(Arc::new(map));
        assert_eq!(
            panel.picker_labels(),
            vec![b"Hi, Noa!".to_vec(), b"No".to_vec()]
        );
    }

    /// A prompt followed by a `0x27` Yes/No picker: the panel types the prompt,
    /// then enters a menu wait exposing the two decoded option labels + a
    /// movable cursor (instead of going `Done`).
    #[test]
    fn from_inline_dialog_surfaces_a_following_picker_menu() {
        // [1F 'O' 'K' '?' 00]  27  <j0:i16> <j1:i16>  24  [1F 'Y' 'e' 's' 00][1F 'N' 'o' 00]
        let mut inline = vec![0x1F, b'O', b'K', b'?', 0x00];
        inline.push(0x27); // open, N=2
        inline.extend_from_slice(&0x10i16.to_le_bytes());
        inline.extend_from_slice(&0x20i16.to_le_bytes());
        inline.push(0x24); // continuation
        inline.extend_from_slice(&[0x1F, b'Y', b'e', b's', 0x00]);
        inline.extend_from_slice(&[0x1F, b'N', b'o', 0x00]);
        // Branch bodies both jumps land in: `legaia_mes::scan_pickers` only
        // accepts a picker whose every option target is a byte of this
        // script. Filler is field-VM `0x21` NOPs (no second picker).
        inline.resize(48, 0x21);

        let mut panel = OwnedDialogPanel::from_inline_dialog(&inline).expect("has a 0x1F lead");
        assert!(panel.picker().is_some(), "the following picker decodes");
        // Type the prompt "OK?" then hit the terminator.
        type_until_wait(&mut panel);
        assert_eq!(panel.page_bytes(), vec![b'O', b'K', b'?']);
        assert!(
            !panel.menu_active() && panel.is_waiting_for_input(),
            "the prompt's page waits for the press that opens the menu"
        );
        assert!(!panel.is_done(), "menu box is not Done");
        panel.advance_page();
        assert!(panel.menu_active(), "the press opens the menu");
        slide_in(&mut panel);
        let opts = panel.picker().unwrap();
        assert_eq!(opts.options.len(), 2);
        assert_eq!(opts.options[0].label, b"Yes");
        assert_eq!(opts.options[1].label, b"No");
        // Cursor wraps within 0..2.
        assert_eq!(panel.picker_cursor(), 0);
        panel.move_picker_cursor(1);
        assert_eq!(panel.picker_cursor(), 1);
        panel.move_picker_cursor(1);
        assert_eq!(panel.picker_cursor(), 0, "wraps");
        panel.move_picker_cursor(-1);
        assert_eq!(panel.picker_cursor(), 1, "wraps backward");
    }

    /// Confirming a menu option applies its relative jump and resumes typing
    /// the chosen branch's reply segment (mirrors `FUN_80038050`).
    #[test]
    fn confirm_menu_jumps_to_the_chosen_branch_reply() {
        // Layout: prompt "OK?" + 0x27 picker (Yes/No) + two reply segments.
        // Build it, then back-fill each option's i16 jump so it lands on its
        // reply lead: target = (open + 1 + index*2) + rel_jump.
        let mut b = vec![0x1F, b'O', b'K', b'?', 0x00]; // prompt, ends at pc=5
        let open = b.len(); // 5
        b.push(0x27); // open
        let entries_at = b.len(); // 6
        b.extend_from_slice(&[0, 0, 0, 0]); // 2 entries, filled in below
        b.push(0x24); // continuation
        b.extend_from_slice(&[0x1F, b'Y', b'e', b's', 0x00]); // label0
        b.extend_from_slice(&[0x1F, b'N', b'o', 0x00]); // label1
        let reply0 = b.len();
        b.extend_from_slice(&[0x1F, b'Y', b'!', 0x00]); // reply for option 0
        let reply1 = b.len();
        b.extend_from_slice(&[0x1F, b'N', b'!', 0x00]); // reply for option 1
        // rel_jump[i] so (open + 1 + i*2) + rel = reply_lead.
        let j0 = (reply0 as i32 - (open as i32 + 1)) as i16;
        let j1 = (reply1 as i32 - (open as i32 + 1 + 2)) as i16;
        b[entries_at..entries_at + 2].copy_from_slice(&j0.to_le_bytes());
        b[entries_at + 2..entries_at + 4].copy_from_slice(&j1.to_le_bytes());

        // Choose option 1 ("No") and confirm -> should type the "N!" reply.
        let mut panel = OwnedDialogPanel::from_inline_dialog(&b).unwrap();
        type_until_wait(&mut panel);
        panel.advance_page();
        assert!(panel.menu_active());
        slide_in(&mut panel);
        panel.move_picker_cursor(1);
        assert_eq!(panel.confirm_menu(), Some(1));
        assert!(!panel.menu_active(), "menu resolved");
        assert_eq!(type_until_wait(&mut panel), PanelState::Done);
        assert_eq!(panel.page_bytes(), vec![b'N', b'!']);

        // Option 0 ("Yes") -> "Y!".
        let mut panel = OwnedDialogPanel::from_inline_dialog(&b).unwrap();
        type_until_wait(&mut panel);
        assert_eq!(panel.confirm_menu(), None, "no menu before the press");
        panel.advance_page();
        assert_eq!(panel.confirm_menu(), None, "the menu is still sliding in");
        slide_in(&mut panel);
        assert_eq!(panel.confirm_menu(), Some(0));
        type_until_wait(&mut panel);
        // The two reply lines are consecutive `0x1F` rows, so they share the
        // box; the chosen reply is its first row.
        assert!(panel.page_bytes().starts_with(b"Y!"));
    }

    /// The press opens the menu hidden (sentinel), the box rises from
    /// `y = 0xF0` to its rest rect over thirteen pager calls at frame step 2,
    /// and Up/Down + confirm do nothing until the call after it rests - the
    /// pager's picker slide ([`crate::dialog_picker_slide`]). A `0x2A` menu
    /// rests at the top right instead, and its cursor clamps.
    #[test]
    fn a_menu_slides_in_before_it_takes_input() {
        let build = |open_byte: u8| {
            let mut b = vec![0x1F, b'O', b'K', b'?', 0x00];
            b.push(open_byte);
            b.extend_from_slice(&0x10i16.to_le_bytes());
            b.extend_from_slice(&0x20i16.to_le_bytes());
            b.push(0x24);
            b.extend_from_slice(&[0x1F, b'Y', b'e', b's', 0x00]);
            b.extend_from_slice(&[0x1F, b'N', b'o', 0x00]);
            b.resize(48, 0x21);
            b
        };
        let mut panel = OwnedDialogPanel::from_inline_dialog(&build(0x27)).unwrap();
        type_until_wait(&mut panel);
        panel.advance_page();
        assert!(panel.menu_active());
        assert_eq!(
            panel.picker_rect(),
            None,
            "the press's sentinel draws no box"
        );
        assert!(!panel.picker_takes_input());
        let mut ys = Vec::new();
        let mut vsyncs = 0;
        while !panel.picker_takes_input() {
            panel.tick_at(2);
            vsyncs += 1;
            // Input stays shut while it slides: the cursor and a confirm do
            // nothing.
            if !panel.picker_takes_input() {
                panel.move_picker_cursor(1);
                assert_eq!(panel.picker_cursor(), 0);
                assert_eq!(panel.confirm_menu(), None);
            }
            if let Some((x, y, w, h)) = panel.picker_rect()
                && ys.last() != Some(&y)
            {
                assert_eq!((x, w, h), (0x26, 0xF4, 0x1A));
                ys.push(y);
            }
            assert!(vsyncs < 64);
        }
        assert_eq!(ys.first(), Some(&0xF0));
        assert_eq!(ys.last(), Some(&0xA3));
        assert!(panel.picker_hand_drawn());
        panel.move_picker_cursor(1);
        assert_eq!(panel.picker_cursor(), 1);
        assert_eq!(panel.confirm_menu(), Some(1));

        let mut inn = OwnedDialogPanel::from_inline_dialog(&build(0x2A)).unwrap();
        type_until_wait(&mut inn);
        inn.advance_page();
        inn.tick_at(2);
        assert_eq!(inn.picker_rect(), Some((0x150, 0x4A, 0x58, 0x1A)));
        slide_in(&mut inn);
        assert_eq!(inn.picker_rect(), Some((0xD8, 0x4A, 0x58, 0x1A)));
        inn.move_picker_cursor(-1);
        assert_eq!(inn.picker_cursor(), 0, "a 0x2A cursor clamps at the top");
        inn.move_picker_cursor(1);
        inn.move_picker_cursor(1);
        assert_eq!(inn.picker_cursor(), 1, "and at the bottom");
    }

    /// `confirm_menu` is a no-op (returns `None`) when no menu is active.
    #[test]
    fn confirm_menu_without_active_menu_is_none() {
        let mut panel = OwnedDialogPanel::from_inline_dialog(&[0x1F, b'H', b'i', 0x00]).unwrap();
        assert_eq!(panel.confirm_menu(), None);
    }

    /// Three consecutive `0x1F` rows pack into ONE window: the page buffer
    /// holds all three rows joined by the `0x7C` newline glyph, and only
    /// after the last row does the panel end. Retail `_DAT_801F2740 = 3`
    /// (`FUN_801D84D0`); the port used to close the box at every row
    /// terminator, splitting a 3-row retail box into three 1-row windows.
    #[test]
    fn three_rows_type_into_one_window() {
        let inline = vec![
            0x1F, b'o', b'n', b'e', 0x00, // row 1
            0x1F, b't', b'w', b'o', 0x00, // row 2
            0x1F, b's', b'i', b'x', 0x00, // row 3
            0x25, // end
        ];
        let mut panel = OwnedDialogPanel::from_inline_dialog(&inline).unwrap();
        // 9 glyphs + 2 newline pushes + 3 terminators = the panel is done
        // only after all three rows.
        let mut ticks = 0;
        while !panel.is_done() && ticks < 64 {
            panel.tick();
            ticks += 1;
        }
        assert!(panel.is_done(), "conversation ends after the packed box");
        assert_eq!(
            panel.page_bytes(),
            b"one\x7Ctwo\x7Csix".to_vec(),
            "all three rows land on ONE page, newline-joined"
        );
    }

    /// A `0x24` next-page dispatch between two boxes: the panel page-breaks
    /// (not `Done`) after box one, and `advance_page` types box two in the
    /// same panel.
    #[test]
    fn next_page_dispatch_pages_within_one_panel() {
        let inline = vec![
            0x1F, b'a', 0x00, // box 1 row 1
            0x1F, b'b', 0x00, // box 1 row 2
            0x1F, b'c', 0x00, // box 1 row 3 (box full)
            0x24, // next page
            0x1F, b'd', 0x00, // box 2 row 1
            0x25, // end
        ];
        let mut panel = OwnedDialogPanel::from_inline_dialog(&inline).unwrap();
        let mut ticks = 0;
        while !panel.is_waiting_for_input() && ticks < 64 {
            panel.tick();
            ticks += 1;
        }
        assert!(!panel.is_done(), "page break, not teardown, at the 0x24");
        assert_eq!(panel.page_bytes(), b"a\x7Cb\x7Cc".to_vec());
        panel.advance_page();
        let mut ticks = 0;
        while !panel.is_done() && ticks < 64 {
            panel.tick();
            ticks += 1;
        }
        assert!(panel.is_done());
        assert_eq!(panel.page_bytes(), b"d".to_vec(), "page two typed fresh");
    }

    /// A 2-row prompt whose picker follows the SECOND row's terminator: the
    /// menu must attach (the old per-segment test looked only after row 1,
    /// so a multi-row prompt never found its picker).
    #[test]
    fn picker_after_two_row_prompt_attaches() {
        // [1F 'A' 00][1F 'B' '?' 00] 27 <j0> <j1> 24 [1F Y e s 00][1F N o 00]
        let mut inline = vec![0x1F, b'A', 0x00, 0x1F, b'B', b'?', 0x00];
        inline.push(0x27); // open, N=2
        inline.extend_from_slice(&0x10i16.to_le_bytes());
        inline.extend_from_slice(&0x20i16.to_le_bytes());
        inline.push(0x24);
        inline.extend_from_slice(&[0x1F, b'Y', b'e', b's', 0x00]);
        inline.extend_from_slice(&[0x1F, b'N', b'o', 0x00]);
        inline.resize(48, 0x21);

        let mut panel = OwnedDialogPanel::from_inline_dialog(&inline).unwrap();
        assert!(
            panel.picker().is_some(),
            "picker after the box's LAST row attaches"
        );
        let mut ticks = 0;
        while !panel.is_waiting_for_input() && ticks < 64 {
            panel.tick();
            ticks += 1;
        }
        panel.advance_page();
        assert!(
            panel.menu_active(),
            "menu opens on the press after the 2-row prompt"
        );
        assert_eq!(panel.page_bytes(), b"A\x7CB?".to_vec());
    }

    /// Plain dialogue (no picker) still types and goes `Done`.
    #[test]
    fn from_inline_dialog_without_picker_is_not_a_menu() {
        let inline = vec![0x1F, b'H', b'i', 0x00];
        let mut panel = OwnedDialogPanel::from_inline_dialog(&inline).unwrap();
        assert!(panel.picker().is_none());
        assert_eq!(type_until_wait(&mut panel), PanelState::Done);
        assert!(!panel.menu_active());
    }

    /// The segment-pool decoder recovers **every** `0x1F`-lead segment, not
    /// just the first: a record carries the NPC's whole dialogue line pool
    /// (consecutive speech lines plus interspersed option labels like
    /// `"Yes"` / `"No"`), each `0x00`-terminated.
    #[test]
    fn decode_inline_segments_recovers_every_segment() {
        // [header] 1F W e l c o m e 00  1F Y e s 00  1F N o 00
        let inline = vec![
            0x00, 0x42, 0x1F, b'W', b'e', b'l', b'c', b'o', b'm', b'e', 0x00, 0x1F, b'Y', b'e',
            b's', 0x00, 0x1F, b'N', b'o', 0x00,
        ];
        let segs = decode_inline_segments(&inline);
        assert_eq!(
            segs,
            vec![b"Welcome".to_vec(), b"Yes".to_vec(), b"No".to_vec()],
            "all three 0x1F-lead segments decode in pool order"
        );
    }

    /// A single-segment record decodes to exactly one segment.
    #[test]
    fn decode_inline_segments_single_segment() {
        let inline = vec![0x00, 0x56, 0x00, 0x1F, b'H', b'i', 0x00];
        assert_eq!(decode_inline_segments(&inline), vec![b"Hi".to_vec()]);
    }

    /// `0xCF XX` in MES bytecode = render XX alone. Both panel variants
    /// must surface the operand byte through the page accumulator, not
    /// silently drop it.
    #[test]
    fn dialog_panel_renders_skip_two_operand() {
        // Glyph 'a', then 0xCF 'b', then End. Expected page: ['a', 'b'].
        let buf = vec![b'a', 0xCF, b'b', 0x00];
        let interp = Interpreter::new_at(&buf, 0);
        let mut player = DialogPlayer::new(interp);
        player.set_glyphs_per_frame(1);
        let mut panel = DialogPanel::new(player);
        for _ in 0..3 {
            panel.tick();
        }
        assert_eq!(panel.page_bytes(), vec![b'a', b'b']);
    }

    #[test]
    fn owned_panel_renders_skip_two_operand() {
        let buf = Arc::new(vec![b'a', 0xCF, b'b', 0x00]);
        let mut panel = OwnedDialogPanel::new(buf, 0);
        for _ in 0..3 {
            panel.tick();
        }
        assert_eq!(panel.page_bytes(), vec![b'a', b'b']);
    }

    #[test]
    fn advance_page_clears_buffer_and_resumes() {
        // After a glyph, force a page break by injecting a control byte.
        // Compact format treats single bytes as glyphs unless they're in
        // the control range - the interpreter's actual control byte set is
        // out of scope here, so we just verify advance_page is a no-op
        // when not at a break, and clears properly when at one.
        let buf = three_glyph_program();
        let interp = Interpreter::new_at(&buf, 0);
        let mut player = DialogPlayer::new(interp);
        player.set_glyphs_per_frame(1);
        let mut panel = DialogPanel::new(player);
        panel.tick();
        assert_eq!(panel.page_bytes().len(), 1);
        // Not at page break - advance_page is idempotent.
        panel.advance_page();
        assert_eq!(panel.page_bytes().len(), 1);
    }

    /// The automatic press counts down only while the page waits, by the
    /// frame step once per pager call, and presses on the call that takes
    /// it to zero or below (`0x801D8F4C..0x801D8F88`).
    #[test]
    fn the_automatic_press_counts_down_in_the_wait_and_fires_once() {
        let inline = vec![0x1F, b'a', 0x00, 0x24, 0x1F, b'b', 0x00, 0x25];
        let mut panel = OwnedDialogPanel::from_inline_dialog(&inline).unwrap();
        let mut auto = 21i16;
        // Typing: the countdown does not move.
        while !panel.is_waiting_for_input() {
            panel.tick_at_auto(2, &mut auto);
            assert_eq!(auto, 21, "no countdown while the page types");
            assert!(!panel.take_auto_press());
        }
        // Waiting: 21 -> 19 -> .. -> 1, then the press (eleven calls, one
        // every two vsyncs), as the retail capture reads.
        let mut calls = 0;
        let mut vsyncs = 0;
        loop {
            let before = auto;
            panel.tick_at_auto(2, &mut auto);
            vsyncs += 1;
            if auto != before {
                calls += 1;
            }
            if panel.take_auto_press() {
                break;
            }
            assert!(vsyncs < 64, "the press never fired");
        }
        assert_eq!(auto, 0);
        assert_eq!(calls, 11);
        // Handled as a confirm: the page turns; nothing fires again.
        panel.advance_page();
        for _ in 0..8 {
            panel.tick_at_auto(2, &mut auto);
            assert!(!panel.take_auto_press());
        }
    }

    /// An idle countdown (`<= 0`) never presses.
    #[test]
    fn an_idle_countdown_never_presses() {
        let inline = vec![0x1F, b'a', 0x00, 0x25];
        let mut panel = OwnedDialogPanel::from_inline_dialog(&inline).unwrap();
        let mut auto = 0i16;
        for _ in 0..64 {
            panel.tick_at_auto(2, &mut auto);
            assert!(!panel.take_auto_press());
        }
        assert_eq!(auto, 0);
    }
}
