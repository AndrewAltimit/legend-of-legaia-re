//! `OwnedDialogPanel`: the owning panel a host keeps across frames, and its constructors.

use super::*;

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
    pub(super) picker: Option<legaia_mes::Picker>,
    /// Highlighted option index `0..picker.n` while the menu is active.
    pub(super) picker_cursor: usize,
    /// `true` once the prompt has finished typing and the menu is awaiting a
    /// choice. Only meaningful when [`Self::picker`] is `Some`.
    pub(super) menu_active: bool,
    /// The open menu's slide onto the screen ([`crate::dialog_picker_slide`]):
    /// set by the press that opens it, advanced one step per pager call.
    /// `None` while no menu is open.
    pub(super) picker_slide: Option<crate::dialog_picker_slide::PickerSlide>,
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
    pub(super) current_box: Option<legaia_mes::DialogBox>,
    /// The page ended on a continuing post-page dispatch (`0x24` next-page /
    /// `0x48` new-box): [`Self::advance_page`] must turn to the next page
    /// instead of resuming the byte stream in place.
    pub(super) pending_box_advance: bool,
    pub(super) state: PanelState,
    pub(super) waiting_for_input: bool,
    pub(super) done: bool,
    /// Vsyncs per game tick (`DAT_1F800393`) the host runs the panel at. The
    /// box path runs one retail pager call every `frame_step` [`Self::tick`]s
    /// (one tick = one vsync); set it from
    /// `legaia_engine_vm::frame_clock::FrameClock::frame_step` before ticking (the World's
    /// dialog paths do, through [`Self::tick_at`]). `0` is treated as `1`.
    pub frame_step: u8,
    /// Vsync phase within the current game tick (`0` = a pager call runs).
    pub(super) vsync_phase: u8,
    /// The retail row window - which rows show, the scroll, the typing
    /// row's pace (box path; see [`crate::dialog_window`]).
    pub(super) window: Option<crate::dialog_window::RowWindow>,
    /// Decoded rows of the window's current page and the page before it
    /// (rows carried over a `0x24` turn still draw), keyed by page serial.
    pub(super) page_rows: Vec<(u32, Arc<Vec<DecodedRow>>)>,
    /// A confirm press the next pager call sees
    /// ([`Self::confirm_while_typing`]).
    pub(super) press_latch: bool,
    /// The page ended on a picker open byte and waits in state `0x19` for
    /// the press that opens the menu ([`Self::advance_page`]). Box path only.
    pub(super) menu_pending: bool,
    /// Open a picker as soon as its prompt's page waits, with no press: the
    /// simplified `--simple-dialogue` panel's contract
    /// ([`Self::opening_menu_at_wait`]), whose host commits a choice but never
    /// turns a page. `false` on every retail-faithful path.
    pub(super) menu_opens_at_wait: bool,
    /// The automatic press `_DAT_80073F00` fired on the last pager call
    /// ([`Self::tick_at_auto`]); drained by [`Self::take_auto_press`].
    pub(super) auto_pressed: bool,
    /// Identity of the [`Self::substitutions`] table [`Self::page_rows`] was
    /// decoded against (hosts install it after construction, so rows are
    /// re-decoded when it changes).
    pub(super) decoded_subs: Option<usize>,
    /// The page mark this pager call drew ([`Self::advance_icon`]): its strip
    /// frame, or `None` when the call drew none. Meaningful once
    /// [`Self::cursor_driven`] is up.
    pub(super) advance_icon: Option<u8>,
    /// The option hand's idle bob on this pager call ([`Self::picker_hand_bob`]).
    pub(super) hand_bob: i32,
    /// A pager call has run the cursor primitive for this panel
    /// ([`Self::tick_at_auto_drawn`]); until then the mark is the static
    /// frame 0 of a panel nobody animates.
    pub(super) cursor_driven: bool,
    /// Row capacity every window this panel opens takes
    /// ([`Self::set_window_format`]).
    pub(super) window_rows: usize,
    /// The instant-page latch waiting for the next window load.
    pub(super) window_instant: bool,
}

/// One line of a pager page, decoded once: each glyph with the reveal units
/// before it (it shows once the counter passes that), and the line's
/// `FUN_80036044` count.
#[derive(Debug, Clone, Default)]
pub(super) struct DecodedRow {
    pub(super) glyphs: Vec<(PanelGlyph, u32)>,
    pub(super) count: u32,
    /// Index of the line's `0x1F` lead in [`OwnedDialogPanel::bytes`].
    pub(super) lead: usize,
}

impl OwnedDialogPanel {
    /// Build a panel over `bytes` starting at `pc` (the offset returned by
    /// `crate::dialog::SceneMes::message_offset`).
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
            window_rows: crate::dialog_window::WINDOW_ROWS,
            window_instant: false,
            advance_icon: None,
            hand_bob: 0,
            cursor_driven: false,
        }
    }

    /// Convenience: build a panel from a [`SceneMes`]
    /// resolution. Returns `None` if `text_id` is past the offset table.
    pub fn from_scene_mes(mes: &SceneMes, text_id: u16) -> Option<Self> {
        let pc = mes.message_offset(text_id)?;
        Some(Self::new(Arc::new(mes.bytes.clone()), pc))
    }

    /// Build a panel over the **inline** dialog bytes a placement's
    /// interaction record carries (stored on
    /// `legaia_engine_field::dialog_state::DialogRequest::inline`).
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
    /// panel (`legaia_engine_scene_host::scene_host::SceneHost::open_pending_dialog`), whose host
    /// has a choice commit but no page-turn press to open the menu with.
    pub fn opening_menu_at_wait(mut self) -> Self {
        self.menu_opens_at_wait = true;
        self
    }

    /// Build a panel that types the `0x1F` text segment whose **lead byte** is
    /// at `seg_lead` (i.e. `bytes[seg_lead] == 0x1F`), attaching any picker that
    /// immediately follows it. Used by the inline-script field-VM runner
    /// (`legaia_engine_field::inline_dialogue`), which lands the VM on each text segment and
    /// opens a box there. Unlike [`Self::from_inline_dialog`] it does not search
    /// for the first segment - the caller already knows the exact lead.
    pub fn at_segment(bytes: Arc<Vec<u8>>, seg_lead: usize) -> Self {
        let mut panel = Self::new(bytes, seg_lead + 1);
        panel.seed_box_at_lead(seg_lead);
        panel
    }

    /// The box the pager goes on drawing after the press that left this
    /// panel: the same rows at the same scroll, with no page mark, no picker
    /// box and no option hand.
    ///
    /// `FUN_801D84D0` is a list-0 actor tick, so it draws every game tick
    /// whether or not a box is being typed, and its draw gate is the state
    /// alone: states `1` and `2` skip the window (`j 0x801D9868` at
    /// `0x801D968C`), every other state but `0x0B` takes the row loop at
    /// `0x801D9770` with the box flag up (`li s6,0x1`), which frames the box
    /// (`FUN_8002C69C` at `0x801D99E0`, behind `beq s6,zero` at `0x801D9938`)
    /// and draws each non-null row of `_DAT_801F3540`. The page mark is drawn
    /// only in state `0x19` (`0x801D9804..0x801D9858`) and the picker box only
    /// in the picker states. So the press that dispatches a box (the post-page
    /// dispatch in `0x19`, or the picker's commit at `0x801D9530..0x801D95A4`)
    /// draws the box once more in the state it stores (`0`, `3`, `6` or
    /// `9`), and a `0x24` continuation's state `3` becomes the idle state `4`,
    /// which keeps the rows (`0x801D8708`) and goes on drawing them until the
    /// dialog SM loads the next line.
    pub fn held_after_press(&self) -> Self {
        let mut held = self.clone();
        held.picker = None;
        held.menu_active = false;
        held.picker_slide = None;
        held.menu_pending = false;
        held.waiting_for_input = false;
        held.press_latch = false;
        held.cursor_driven = true;
        held.advance_icon = None;
        held
    }
}
