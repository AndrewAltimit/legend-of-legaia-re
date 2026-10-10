//! Slot ids, the op-`0x49` sub-op slot table, the panel window table and the painter each window names.

use super::*;

// ---------------------------------------------------------------------------
// the two tables
// ---------------------------------------------------------------------------

/// Slots of `PTR_FUN_801F33B4`, the 52-entry handler table the dispatcher
/// indexes by `+0x50`.
///
/// Read out of the field overlay's own bytes (PROT 0897 at base `0x801CE818`,
/// table VA `0x801F33B4`), not inferred: each constant is the index whose word
/// holds that handler's entry VA. Slot `0` is what a freshly spawned submode
/// driver carries, and it is [`close_tick`] - so an actor nobody has given a
/// screen to closes itself on its second frame instead of sitting there.
pub mod slot {
    /// `FUN_801F2134` - also slots `0x14..=0x18`.
    pub const CLOSE_TICK: u16 = 0x00;
    /// `FUN_801F1D90`.
    pub const DEACTIVATE: u16 = 0x13;
    /// `FUN_801F20B0`. Equal to [`super::HUB_RETURN_STATE`]: a hand-back
    /// re-arms `+0x50` here, and this slot is what then clears the completion
    /// gate so the dispatcher can retire the actor.
    pub const DRAW_TICK: u16 = 0x1A;
    /// `FUN_801F0ADC` - the casino coin counter.
    pub const COIN_COUNTER: u16 = 0x25;
    /// `FUN_801F1138`.
    pub const START_MENU: u16 = 0x27;
    /// `FUN_801F1FDC`.
    pub const PROMPT: u16 = 0x28;
    /// `FUN_801F1E48` - the Incense wear-off notice (entry kind `0x0B`).
    pub const SUBMENU: u16 = 0x32;
    /// Slots in the table.
    pub const COUNT: usize = 52;
}

/// The op-`0x49` **sub-op to handler-slot table** at `0x801F33A4`: 14 signed
/// bytes, one per sub-op, `-1` where the sub-op opens no submode screen.
///
/// This is what selects which of the 52 [`slot`] state machines a script's
/// `49 <sub_op>` runs. The enter half reads it directly off the parked
/// operand pointer:
///
/// ```text
/// 801f1454  lw   a1,-0x4bb0(v0)    ; a1 = _DAT_8007B450, the operand pointer
/// 801f145c  lbu  v0,0x0(a1)        ; sub_op = *operand
/// 801f1460  addiu a2,v1,0x33a4     ; a2 = 0x801F33A4
/// 801f1464  addu v0,v0,a2
/// 801f1468  lb   v0,0x0(v0)        ; slot = (i8)table[sub_op]
/// 801f146c  li   a0,-0x1
/// 801f1470  beq  v0,a0,0x801f14b0  ; -1 -> keep +0x50 = 7 (0x801F140C)
/// 801f14ac  sh   v0,0x50(s4)       ; else install the handler
/// ```
///
/// (`overlay_baka_fighter_801f1278.txt`, `FUN_801F1278`.) The length is the
/// field VM's own bound - op `0x49` rejects `sub_op > 0xD` (`sltiu v0,v0,0xe`
/// at `0x801E098C`) - and the table sits immediately before
/// `PTR_FUN_801F33B4`, whose first entry is the close tick.
///
/// Two rows cross-validate the read against facts pinned by other routes:
/// sub-op `3` selects slot `0x22` = `FUN_801F03F0`, the **name-entry**
/// overlay, and sub-op `5` selects slot `0x24` = `FUN_801EF2B0`, the
/// **tile-board** walk state machine. Those are exactly the two sub-ops the
/// engine already resolves through dedicated host paths, identified without
/// this table.
pub const OP49_SUBOP_SLOT_TABLE: u32 = 0x801F_33A4;

/// The table itself - see [`OP49_SUBOP_SLOT_TABLE`].
pub const OP49_SUBOP_SLOTS: [i8; 14] = [
    -1, -1, 0x21, 0x22, 0x23, 0x24, 0x25, -1, 0x27, 0x28, 0x31, 0x32, 0x33, -1,
];

/// The [`slot`] a `49 <sub_op>` installs, or `None` when the table says the
/// sub-op opens no screen (`-1`) or the sub-op is out of the VM's range.
pub fn slot_for_sub_op(sub_op: u8) -> Option<u16> {
    let entry = *OP49_SUBOP_SLOTS.get(sub_op as usize)?;
    (entry >= 0).then_some(entry as u16)
}

/// The panel-window records a panel descriptor installs.
///
/// A descriptor is a list of 8-byte entries `[i16 op][i16 window][u32]`
/// terminated by a zero op; `FUN_801E9B3C` walks it and, for every entry,
/// indexes [`PANEL_WINDOW_TABLE`] by the window halfword. So the descriptor -
/// not the caller - is what names the painter a state machine's screen draws
/// through, and the mapping below is those lists read off the five programs
/// the ported state machines install (plus the coin counter's two).
///
/// Only the windows are kept: the entry op selects which of the installer's
/// thirteen sub-handlers runs (show, hide, resize, ...), and none of them
/// changes *which* record the entry names. The lists are verbatim, including
/// the repeats and the `0` entries - record `0` is a `0x2A`-kind record with
/// no painter, so a program that names only it draws nothing.
pub fn panel_windows(descriptor: u32) -> &'static [usize] {
    match descriptor {
        PANEL_COIN_IDLE => &[0, 10, 10],
        PANEL_COIN_CONFIRM => &[window::THREE_LINE],
        PANEL_START => &[0, 8],
        PANEL_SUBMENU_IDLE | PANEL_SUBMENU_CONFIRM => &[window::SINGLE_LABEL],
        PANEL_PROMPT => &[0, 9],
        PANEL_DRAW_TICK => &[0],
        _ => &[],
    }
}

/// The **panel-window record table** at `0x801F2B98`: 17 records of
/// [`PANEL_WINDOW_STRIDE`] bytes, each
/// `[u32 0][u32 kind][3 geometry words][u32 0x0C][u32 painter VA]`.
///
/// This is a second table, distinct from `PTR_FUN_801F33B4`: the seven panel
/// painters below are **not** `+0x50` handler slots, they are the
/// [`PANEL_WINDOW_PAINTER`] callback of a window record, reached when a state
/// machine installs a panel descriptor through `FUN_801E9B3C`.
///
/// The frame is the installer's own arithmetic, not an inference from where
/// the painter VAs land - `FUN_801E9B3C` indexes the array by the descriptor
/// entry's window halfword and reads the record's geometry at `+8` / `+0xA`:
///
/// ```text
/// 801e9b70  addiu s4,v0,0x2b98   ; s4 = 0x801F2B98, record 0
/// 801e9b78  lh    s0,-0x2(s3)    ; s0 = the descriptor entry's window index
/// 801e9b80  sll   v0,s0,0x3      ; v0 = s0 * 28  (x8 - x1, then x4)
/// 801e9b84  subu  v0,v0,s0
/// 801e9b88  sll   v0,v0,0x2
/// 801e9b8c  addu  v0,v0,s4       ; record = base + index * 0x1C
/// 801e9b90  lh    s1,0x8(v0)
/// 801e9b94  lh    s2,0xa(v0)
/// ```
///
/// (`overlay_baka_fighter_801e9b3c.txt`.) An earlier reading put the base at
/// `0x801F2C0C` with the painter at `+0x14`. That window holds exactly the 13
/// painter-bearing records and reads each painter correctly - the layout is
/// periodic, so a frame shifted by one word still lands on a painter - but it
/// starts four records late, so **every index it names is four low**. The
/// records the old reading cross-validated against (the name-entry renderer
/// `FUN_801E6B34`, `FUN_801E6984`) are records `13` and `14` here, not `9` and
/// `10`; the check could not catch the shift because it was taken inside the
/// shifted frame. Records `0..=3` are a `0x2A` kind with no painter, which is
/// why the shift is invisible from the painter column alone.
pub const PANEL_WINDOW_TABLE: u32 = 0x801F_2B98;

/// Bytes per panel-window record.
pub const PANEL_WINDOW_STRIDE: u32 = 0x1C;

/// Offset of the painter VA inside a panel-window record.
pub const PANEL_WINDOW_PAINTER: u32 = 0x18;

/// Offset of the record's kind word.
pub const PANEL_WINDOW_KIND: u32 = 0x04;

/// Records in the panel-window table (`0..=16`; `17` onward is zero fill).
pub const PANEL_WINDOW_COUNT: usize = 17;

/// The record indices of [`PANEL_WINDOW_TABLE`] whose painter is ported here.
///
/// Named rather than spelled inline because the four-record shift above was
/// invisible precisely while every call site carried its own literal.
pub mod window {
    /// `FUN_801F1950` - the Yes/No panel.
    pub const TWO_OPTION: usize = 5;
    /// `FUN_801F1A1C` - the count-gated single label.
    pub const COUNT_GATED_LABEL: usize = 6;
    /// `FUN_801F16C0` - the stacked per-entry list.
    pub const ENTRY_LIST: usize = 7;
    /// `FUN_801F1890` - the three-line panel, which is what the **coin
    /// counter's** confirm descriptor installs.
    pub const THREE_LINE: usize = 11;
    /// `FUN_801F17D8` - the header row plus one cell per grid column.
    pub const COLUMN_ROW: usize = 12;
    /// `FUN_801F1AB0` - the two-line panel with the screen-effect push.
    pub const TWO_LINE: usize = 15;
    /// `FUN_801F1B64` - the single label plus right-edge cursor, which is what
    /// the **sub-menu's** idle and confirm descriptors install.
    pub const SINGLE_LABEL: usize = 16;
}

/// The panel painters of [`PANEL_WINDOW_TABLE`] that have a body here, by
/// record index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HubPainter {
    /// Record [`window::TWO_OPTION`] - `FUN_801F1950`.
    TwoOption,
    /// Record [`window::COUNT_GATED_LABEL`] - `FUN_801F1A1C`.
    CountGatedLabel,
    /// Record [`window::ENTRY_LIST`] - `FUN_801F16C0`.
    EntryList,
    /// Record [`window::THREE_LINE`] - `FUN_801F1890`.
    ThreeLine,
    /// Record [`window::COLUMN_ROW`] - `FUN_801F17D8`.
    ColumnRow,
    /// Record [`window::TWO_LINE`] - `FUN_801F1AB0`.
    TwoLine,
    /// Record [`window::SINGLE_LABEL`] - `FUN_801F1B64`.
    SingleLabel,
}

impl HubPainter {
    /// The painter a panel-window record index selects, or `None` for the ten
    /// records whose painter lives outside this module (or is absent).
    pub fn for_window(index: usize) -> Option<Self> {
        Some(match index {
            window::TWO_OPTION => HubPainter::TwoOption,
            window::COUNT_GATED_LABEL => HubPainter::CountGatedLabel,
            window::ENTRY_LIST => HubPainter::EntryList,
            window::THREE_LINE => HubPainter::ThreeLine,
            window::COLUMN_ROW => HubPainter::ColumnRow,
            window::TWO_LINE => HubPainter::TwoLine,
            window::SINGLE_LABEL => HubPainter::SingleLabel,
            _ => return None,
        })
    }

    /// Run the painter. [`HubPainter::EntryList`] is the only one that walks
    /// the actor's `+0x0C` and therefore needs it mutable.
    pub fn paint(self, actor: &mut HubActor, env: &HubEnv, grid: &HubGrid) -> HubFrame {
        match self {
            HubPainter::TwoOption => two_option_panel(actor, env),
            HubPainter::CountGatedLabel => count_gated_label(actor, env),
            HubPainter::EntryList => entry_list(actor, env),
            HubPainter::ThreeLine => three_line_panel(actor, env),
            HubPainter::ColumnRow => column_row(actor, env, grid),
            HubPainter::TwoLine => two_line_panel(actor, env),
            HubPainter::SingleLabel => single_label(actor),
        }
    }
}
