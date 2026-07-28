//! The field overlay's **op-`0x49` submode system-actor handler family** in
//! the `0x801F0000+` band: one dispatcher plus the per-state handlers it
//! indexes.
//!
//! ## Which image this code belongs to
//!
//! The corpus holds these VAs under six program names (`baka_fighter`,
//! `dance`, `fishing`, `slot_machine`, `debug_menu`, `overlay_0897`) and the
//! five minigame-named dumps are **byte-identical**. That is not five copies -
//! it is the same resident field-overlay code seen through five RAM-derived
//! captures, the artifact [`docs/tooling/dump-corpus-integrity.md`] catalogues
//! and that the sibling port [`crate::field_party_cursor`] already diagnoses
//! at `FUN_801F1278`. The statically extracted Baka Fighter overlay is PROT
//! 976 at base `0x801CE818` and `0xE000` bytes long, so it does not reach
//! `0x801F0ADC` at all; PROT 0897 (`0x4F800` bytes at the same base) does.
//!
//! This matters beyond attribution, because the minigame-named dumps are also
//! **truncated**: `overlay_baka_fighter_801f0adc.txt` stops after 46
//! instructions, while `overlay_0897_801f0adc.txt` carries the whole
//! discontiguous 264-instruction body. The port of [`coin_exchange`] below is
//! written from the long one.
//!
//! ## What the family is
//!
//! The dispatcher `FUN_801F159C` is the **resume / close half of the field
//! VM's op-`0x49` submode** ([`docs/subsystems/script-vm.md`]); the enter half
//! is `FUN_801F1278` ([`crate::field_party_cursor`]). It indexes the 52-entry
//! table `PTR_FUN_801F33B4` by the actor's `+0x50` word.
//!
//! The routines here come from **two** tables, and conflating them is the
//! error to avoid. The state machines ([`slot`]) are `PTR_FUN_801F33B4`
//! entries: read `+0x54`, install a panel, and on a confirm hand the actor
//! back by stashing `+0x50` into the submode cursor context and re-arming
//! `+0x50` to [`HUB_RETURN_STATE`]. The panel painters ([`HubPainter`]) are
//! **not** in that table at all - they are the `+0x14` callback of a
//! [`PANEL_WINDOW_TABLE`] record, reached only once a state machine has
//! installed the descriptor that names the window.
//!
//! The op-`0x49` contract is what makes the dispatcher load-bearing: the
//! script parks on the same PC until this family drops the busy flag
//! `_DAT_8007B450` to `1`, which is the VM's "Done" signal.
//!
//! ## Actor fields
//!
//! | Field | Role |
//! |---|---|
//! | `+0x0A` / `+0x0C` | panel origin x / y |
//! | `+0x0E` | panel width (the right-edge cursor anchor) |
//! | `+0x10` | actor flag word; bit `3` (`\|= 8`) retires the actor this frame |
//! | `+0x1A` | dispatcher gate: non-zero suppresses the re-arm pass |
//! | `+0x50` | handler id - the `PTR_FUN_801F33B4` index |
//! | `+0x54` | the handler's own sub-state |
//!
//! ## Globals
//!
//! Named here by VA because the family is pure glue over them. `0x801C6EA4`
//! is the submode **cursor context**, whose `+0x2E` is the "hand-back"
//! sentinel, `+0x3E` the completion gate the dispatcher polls and `+0x40` the
//! stashed handler id. `0x8007B450` is the op-`0x49` operand pointer doubling
//! as the busy flag, `0x8007B454` the text palette index, `0x8007B458` a
//! frame-paced hold timer, `0x8007BB80` a suppression flag that blocks every
//! confirm, `0x8007BB88` / `0x8007BB98` the two cursor rows, `0x8007BB90` the
//! published coin ceiling, `0x8007BB9C` the coin counter's digit cursor,
//! `0x801F35F0..+7` its eight digit cells, `0x8008459C` party gold and
//! `0x800845A4` the casino coin bank.
//!
//! `0x80084594` / `0x80084598` are the **party roster** - member count and
//! member ids - not a generic entry table; [`docs/subsystems/script-vm.md`]
//! pins them from the enter half's portrait seeding.
//!
//! Read from `overlay_0897_801f0adc.txt` and `overlay_baka_fighter_801f{1138,
//! 159c,16c0,17d8,1890,1950,1a1c,1ab0,1b64,1d90,1e48,1fdc,20b0,2134}.txt`
//! plus `overlay_baka_fighter_801f90dc.txt`.
//!
//! Host: `legaia_engine_core::field_submode_screen` runs the dispatcher over
//! the live `SubmodeDriver` pool actor every field frame.

use crate::world_map_overlay::{
    EquipPanelDraw, EquipPanelInput, EquipProps, ItemProps, equip_stat_panel,
};

/// One system actor as this family sees it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HubActor {
    /// `+0x0A`
    pub x: i16,
    /// `+0x0C`
    pub y: i16,
    /// `+0x0E`
    pub width: i16,
    /// `+0x10`
    pub flags: u32,
    /// `+0x1A`
    pub gate: i16,
    /// `+0x50`
    pub state: u16,
    /// `+0x54`
    pub sub: i16,
}

/// The submode cursor context at `DAT_801C6EA4`, in the three fields this
/// family touches plus the per-column byte row `+0x54..`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HubGrid {
    /// `+0x2E` - set to `-1` when a handler hands the actor back.
    pub handback: i16,
    /// `+0x3E` - the dispatcher's completion gate.
    pub done_gate: i16,
    /// `+0x40` - the handler id a hand-back stashes.
    pub stashed_state: u16,
    /// `+0x54..` - one byte per drawn column.
    pub columns: Vec<u8>,
}

/// The globals the handlers read, gathered so a host can supply them without
/// a RAM image.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HubEnv {
    /// `DAT_801F2734` - the dispatcher's submode gate.
    pub submode: i32,
    /// `_DAT_1F800394` - the pad latch word; bit `0x8000` suspends the actor.
    pub pad_latch: u32,
    /// `_DAT_8007B874` - this frame's edge-triggered pad word.
    pub pad_edge: u32,
    /// `_DAT_8007B850` - this frame's held pad word.
    pub pad_held: u32,
    /// `_DAT_800846D0` / `_DAT_800846D4` - the two confirm masks, OR-ed.
    pub confirm_mask: u32,
    /// `_DAT_800846D8` - the cancel mask.
    pub cancel_mask: u32,
    /// `_DAT_800846D0` on its own. The coin counter tests the two masks
    /// **separately** rather than OR-ed: `0x800846D0` is its accept edge and
    /// `0x800846D4` its back-out edge (`0x801F0BF8..0x801F0CEC`).
    pub accept_mask: u32,
    /// `_DAT_800846D4` on its own - see [`HubEnv::accept_mask`].
    pub back_mask: u32,
    /// `_DAT_8007BB84` - the auto-repeating directional pad word the coin
    /// counter reads (distinct from the one-shot edge word `pad_edge`).
    pub pad_repeat: u32,
    /// `DAT_1F800393` - this frame's cadence scalar, subtracted from the
    /// hold timer.
    pub frame_delta: i32,
    /// What `FUN_801E9DC8(&_DAT_8007BB98, 2, 1)` returned this frame: `1`
    /// confirm, `2` cancel, anything else "still picking". Supplied by the
    /// host because the two-option picker itself is not ported here.
    ///
    /// REF: FUN_801E9DC8
    pub picker_result: i32,
    /// `_DAT_8007BB80` - non-zero blocks every confirm test.
    pub input_blocked: i32,
    /// `_DAT_8007BB88` - the primary cursor row.
    pub cursor_row: i32,
    /// `_DAT_8007BB98` - the three-line panel's cursor row.
    pub cursor_row_alt: i32,
    /// `_DAT_8007B450` - the field / tile-board busy flag, read as a value.
    pub board_flag: i32,
    /// The three bytes at `_DAT_8007B450 + 1..=3` the start menu counts.
    pub board_entries: [u8; 3],
    /// `_DAT_8007C364 + 0x10` - the busy word whose bit `0x80000` the
    /// dispatcher clears.
    pub busy_word: u32,
    /// `DAT_80084594` - the party roster member count.
    pub entry_count: u8,
    /// `DAT_80084598..` - one member id per roster entry.
    pub entry_codes: Vec<u8>,
    /// `DAT_8008459C` - party gold.
    pub gold: i32,
    /// `DAT_800845A4` - the casino coin bank.
    pub coin_bank: i32,
    /// `_DAT_8007B868` / `_DAT_8007B98C` - the two hub progress flags.
    pub progress_a: i32,
    pub progress_b: i32,
    /// `DAT_801E46B0` - the item id the acquisition caption names.
    pub caption_item: i32,
    /// `_DAT_800845B4` - the amount the money pseudo-item prints.
    pub caption_amount: i32,
    /// Everything the per-entry sub-panel `FUN_801E5B4C` reads that is not
    /// already on this struct: the per-entry character records and the two
    /// static resolver tables.
    pub equip: HubEquipEnv,
}

/// The equipment context [`entry_list`]'s sub-draw resolves against.
///
/// Retail reads all of this out of RAM the sub-draw addresses directly - the
/// save block at `0x80084140`, the item property table `DAT_80074368` and the
/// equipment table `DAT_80074F68`. The port hands them in, because
/// `engine-vm` owns neither the records nor the static tables.
///
/// The **default is a zero loadout with mode `0`**, on which
/// [`equip_stat_panel`] takes retail's own no-candidate arm and prints three
/// zero rows. That is a host gap, not a decode gap: every field below has a
/// counterpart the engine already holds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HubEquipEnv {
    /// Per entry **code** (not per list position): the five equip slot ids at
    /// record `+0x196` and the three base stats at `+0x112` / `+0x114` /
    /// `+0x116`. An entry code with no row here draws the zero loadout.
    pub records: Vec<HubEquipRecord>,
    /// `DAT_80074368` rows, indexed by item id.
    pub item_props: Vec<ItemProps>,
    /// `DAT_80074F68` rows, indexed by an item row's `stat_index`.
    pub equip_props: Vec<EquipProps>,
    /// The word `FUN_801E5B4C` reads at `0x8007BB9C` to select its candidate
    /// source. That cell is scratch shared with other consumers - this same
    /// family reads it as the coin counter's digit cursor - so it is a host
    /// input rather than something the hub state can be asked for.
    pub mode: u32,
    /// The inventory id list at `0x80084140 + 0x1818` the inventory modes
    /// index with [`HubEnv::cursor_row`].
    pub inventory: Vec<u8>,
    /// `0x8007B42C` - the per-character weapon slot table.
    pub weapon_slots: Vec<i16>,
}

/// One character's contribution to [`HubEquipEnv`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HubEquipRecord {
    /// The entry code this row answers for.
    pub code: u8,
    /// Record `+0x196..+0x19B` - the five slots the aggregator walks.
    pub slots: [u8; 5],
    /// Record `+0x112` / `+0x114` / `+0x116` - ATK / UDF / LDF.
    pub base_stats: [i32; 3],
}

/// A string slot in the family's rodata, named by the VA of the pointer the
/// handler loads. The bytes are Sony-owned and are not reproduced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HubString {
    /// One of the five `&DAT_801F29B0[code + 2]` per-entry labels.
    EntryLabel(u8),
    /// A literal string pointer, by its rodata VA.
    Literal(u32),
    /// The SCUS item table's name pointer for an item id
    /// (`*(0x8007436C + id * 0x0C)`).
    ItemName(i32),
    /// The item record's second word (`*(0x80074370 + id * 0x0C)`).
    ItemDetail(i32),
}

/// One draw the family emits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HubDraw {
    /// `FUN_80036888(str, 0, 0, x, y)` - a glyph string at the given palette.
    Text {
        text: HubString,
        x: i16,
        y: i16,
        palette: i32,
    },
    /// `FUN_8003CD00(str, x, y)` - the three-argument glyph string.
    ShortText {
        text: HubString,
        x: i16,
        y: i16,
        palette: i32,
    },
    /// `FUN_8003CC98(str, 0, 0, x, y)` - the header string form.
    HeaderText { text: HubString, x: i16, y: i16 },
    /// `FUN_8002B994(a, b, x, y)` - a cursor / marker sprite.
    Sprite { a: i32, b: i32, x: i16, y: i16 },
    /// `FUN_8002C488(x, y, cell)` - one indexed sprite cell.
    Cell { x: i16, y: i16, cell: i32 },
    /// `FUN_800337B0(str, id, x, y)` - the item-detail line.
    Detail {
        text: HubString,
        id: i32,
        x: i16,
        y: i16,
    },
    /// `FUN_80034B78(value, digits, x, y)` - a right-aligned decimal.
    Number {
        value: i32,
        digits: i32,
        x: i16,
        y: i16,
    },
    /// One draw of `FUN_801E5B4C(actor)`, the per-entry equipment stat
    /// sub-panel the list paints under each label. Retail's `jal 0x801F1778`
    /// expands to a run of these; the inner enum keeps each draw's own retail
    /// emitter.
    EntrySubPanel(EquipPanelDraw),
    /// `FUN_80024EE4(3, 0, 0)` - the panel's screen effect push.
    Effect(i32),
}

/// One side effect other than a draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HubAction {
    /// `FUN_801E9B3C(desc)` - install the panel descriptor at this VA.
    InstallPanel(u32),
    /// `FUN_80035BD0(id)` - the confirm sting.
    ConfirmCue(u8),
    /// `FUN_80035B50(id)` - the entry sting.
    EntryCue(u8),
    /// `FUN_80035A4C()` - the close sting.
    CloseCue,
    /// `FUN_80031D00()` - the per-actor draw pump.
    DrawPump,
    /// `FUN_801F1278(actor)` - the submode re-arm the dispatcher calls.
    RearmSubmode,
    /// The dispatcher's pad-latch release (`&= ~0x8000`).
    ReleasePadLatch,
    /// The dispatcher's busy-bit clear (`busy_word &= ~0x80000`).
    ClearBusyBit,
    /// The dispatcher's `_DAT_8007B450 = 1`.
    SetBoardFlag,
    /// `_DAT_8007B450 = 0` (the sub-menu SM's state-0 clear).
    ClearBoardFlag,
    /// `DAT_8007BB90 = n` - the clamped coin amount.
    SetCoinAmount(i32),
    /// The coin counter's commit (`0x801F1080..0x801F109C`): credit `coins`
    /// to the **casino coin bank** `DAT_800845A4` and debit `gold_cost` from
    /// **party gold** `DAT_8008459C`. Two different destinations - the credit
    /// never lands in gold.
    BuyCoins { coins: i32, gold_cost: i32 },
    /// `DAT_8007BB88 = 0`.
    ClearCursorRow,
    /// `DAT_801F2C86` / `DAT_801F2C82` - the start panel's height and top.
    SizePanel { height: i16, top: i16 },
    /// `DAT_8007B469 = code` - the per-entry code the sub-draw reads, and the
    /// only thing that tells `FUN_801E5B4C` which character it is drawing.
    SetEntryCode(u8),
}

/// What one handler call produced.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HubFrame {
    pub draws: Vec<HubDraw>,
    pub actions: Vec<HubAction>,
}

impl HubFrame {
    fn draw(&mut self, d: HubDraw) {
        self.draws.push(d);
    }
    fn act(&mut self, a: HubAction) {
        self.actions.push(a);
    }
}

/// Text palette the panel handlers select before a string
/// (`_DAT_8007B454`).
pub const PALETTE_PANEL: i32 = 7;
/// The alternate palette the three-line panel uses for its lower two lines.
pub const PALETTE_DIM: i32 = 5;
/// The palette the acquisition caption uses for the item name.
pub const PALETTE_CAPTION: i32 = 6;

/// The handler id every hand-back re-arms `+0x50` to.
pub const HUB_RETURN_STATE: u16 = 0x1A;
/// The alternate re-arm id `FUN_801F1D90` picks.
pub const HUB_DEACTIVATE_STATE: u16 = 0x2C;
/// The other id it picks.
pub const HUB_SKIP_STATE: u16 = 0x02;

/// Confirm sting id.
pub const CUE_CONFIRM: u8 = 0x20;
/// Entry sting id.
pub const CUE_ENTRY: u8 = 0x26;

/// Bit of the pad latch word that suspends the actor.
pub const PAD_LATCH_SUSPEND: u32 = 0x8000;
/// Bit of `_DAT_8007C364 + 0x10` the dispatcher clears.
pub const BUSY_BIT: u32 = 0x0008_0000;
/// The actor flag bit that retires the actor.
pub const ACTOR_RETIRE: u32 = 0x8;

/// The three submode values the dispatcher runs under (`DAT_801F2734`).
pub const ACTIVE_SUBMODES: [i32; 3] = [1, 4, 7];

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
    /// `FUN_801F1E48`.
    pub const SUBMENU: u16 = 0x32;
    /// Slots in the table.
    pub const COUNT: usize = 52;
}

/// The **panel-window record table** at `0x801F2C0C`: 13 records of
/// [`PANEL_WINDOW_STRIDE`] bytes, each `[u32 kind = 0x00030000][3 geometry
/// words][u32 0x0C][u32 painter VA][u32 0]`.
///
/// This is a second table, distinct from `PTR_FUN_801F33B4`: the seven panel
/// painters below are **not** `+0x50` handler slots, they are the `+0x14`
/// callback of a window record, reached when a state machine installs a panel
/// descriptor through `FUN_801E9B3C`. Two records cross-validate the read -
/// index `9` is the name-entry renderer `FUN_801E6B34` and index `10` is
/// `FUN_801E6984`, both already pinned elsewhere
/// ([`docs/subsystems/script-vm.md`], `field_submode::submode_panel_rows`).
pub const PANEL_WINDOW_TABLE: u32 = 0x801F_2C0C;
/// Bytes per panel-window record.
pub const PANEL_WINDOW_STRIDE: u32 = 0x1C;
/// Offset of the painter VA inside a panel-window record.
pub const PANEL_WINDOW_PAINTER: u32 = 0x14;
/// Records in the panel-window table.
pub const PANEL_WINDOW_COUNT: usize = 13;

/// The panel painters of [`PANEL_WINDOW_TABLE`] that have a body here, by
/// record index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HubPainter {
    /// Record `1` - `FUN_801F1950`.
    TwoOption,
    /// Record `2` - `FUN_801F1A1C`.
    CountGatedLabel,
    /// Record `3` - `FUN_801F16C0`.
    EntryList,
    /// Record `7` - `FUN_801F1890`.
    ThreeLine,
    /// Record `8` - `FUN_801F17D8`.
    ColumnRow,
    /// Record `11` - `FUN_801F1AB0`.
    TwoLine,
    /// Record `12` - `FUN_801F1B64`.
    SingleLabel,
}

impl HubPainter {
    /// The painter a panel-window record index selects, or `None` for the six
    /// records whose painter lives outside this module.
    pub fn for_window(index: usize) -> Option<Self> {
        Some(match index {
            1 => HubPainter::TwoOption,
            2 => HubPainter::CountGatedLabel,
            3 => HubPainter::EntryList,
            7 => HubPainter::ThreeLine,
            8 => HubPainter::ColumnRow,
            11 => HubPainter::TwoLine,
            12 => HubPainter::SingleLabel,
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

/// Ceiling the coin exchange clamps the bank to.
pub const COIN_BANK_MAX: i32 = 0x0098_967F;
/// Gold per coin.
pub const GOLD_PER_COIN: i32 = 100;

/// The item id that means "money" rather than an inventory item.
pub const CAPTION_MONEY_ID: i32 = 0xFE;

// ---------------------------------------------------------------------------
// dispatcher
// ---------------------------------------------------------------------------

/// PORT: FUN_801f159c - the hub system-actor dispatcher.
///
/// Active only while the submode gate `DAT_801F2734` is one of
/// [`ACTIVE_SUBMODES`]. It forces the text layer byte `DAT_80073F20` to `0x0C`,
/// re-arms the submode through `FUN_801F1278` unless the actor's own gate
/// `+0x1A` is set (and retires the actor outright when the pad latch's
/// [`PAD_LATCH_SUSPEND`] bit is up), runs the `+0x50` handler, and then - once
/// the grid actor's completion gate `+0x3E` reads `0` - retires the actor,
/// releases the pad latch and drops the board busy state.
///
/// `handler` is the caller's view of `PTR_FUN_801F33B4[actor.state]`. It takes
/// the cursor context mutably because retail re-reads `+0x3E` **after** the
/// `jalr`, so a handler that clears the completion gate this frame is retired
/// this frame - which is exactly how [`draw_tick`] closes a screen.
pub fn hub_dispatch(
    actor: &mut HubActor,
    env: &HubEnv,
    grid: &mut HubGrid,
    handler: impl FnOnce(&mut HubActor, &mut HubGrid) -> HubFrame,
) -> HubFrame {
    let mut out = HubFrame::default();
    if !ACTIVE_SUBMODES.contains(&env.submode) {
        return out;
    }
    // `DAT_80073F20 = 0x0C` is a plain store with no engine-side reader.
    if actor.gate == 0 {
        if env.pad_latch & PAD_LATCH_SUSPEND != 0 {
            actor.flags |= ACTOR_RETIRE;
            return out;
        }
        out.act(HubAction::RearmSubmode);
    }
    let inner = handler(actor, grid);
    out.draws.extend(inner.draws);
    out.actions.extend(inner.actions);
    if grid.done_gate == 0 {
        actor.flags |= ACTOR_RETIRE;
        out.act(HubAction::ReleasePadLatch);
        if env.board_flag == 0 {
            out.act(HubAction::ClearBusyBit);
        } else {
            out.act(HubAction::SetBoardFlag);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// coin exchange
// ---------------------------------------------------------------------------

/// The coin counter's own globals: the eight decimal cells at `DAT_801F35F0`,
/// the digit cursor `_DAT_8007BB9C`, the Yes/No row `_DAT_8007BB98` and the
/// frame-paced hold timer `_DAT_8007B458`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoinCounter {
    /// `DAT_801F35F0..+7`, **least significant cell first**: the entered
    /// amount is `sum(digits[i] * 10^i)`. Cells are signed bytes because
    /// retail sign-extends them into the multiply.
    pub digits: [i8; COIN_ENTRY_DIGITS],
    /// `_DAT_8007BB9C` - which cell the up/down edits hit. Wraps over
    /// [`COIN_CURSOR_CELLS`], not over all eight cells.
    pub cursor: i32,
    /// `_DAT_8007BB98` - the confirm panel's Yes/No row, seeded to `1`.
    pub yes_no: i32,
    /// `_DAT_8007B458` - the post-commit hold timer.
    pub hold: i32,
    /// `_DAT_8007BB90` - the ceiling the head republishes every frame.
    pub ceiling: i32,
}

impl Default for CoinCounter {
    fn default() -> Self {
        Self {
            digits: [0; COIN_ENTRY_DIGITS],
            cursor: 0,
            yes_no: 0,
            hold: 0,
            ceiling: 0,
        }
    }
}

impl CoinCounter {
    /// The amount the eight cells spell.
    ///
    /// PORT: FUN_801f0adc (`0x801F0C20..0x801F0C4C`, and the two identical
    /// copies at `0x801F0E3C` / `0x801F102C`)
    pub fn entered(&self) -> i32 {
        let mut total: i32 = 0;
        let mut power: i32 = 1;
        for d in self.digits {
            total = total.wrapping_add(power.wrapping_mul(d as i32));
            power = power.wrapping_mul(10);
        }
        total
    }

    /// Rewrite the cells as the decimal expansion of `value`, most
    /// significant cell last.
    ///
    /// PORT: FUN_801f0adc (`0x801F0EDC..0x801F0F5C`)
    pub fn set_entered(&mut self, value: i32) {
        let mut rest = value;
        let mut place: i32 = 10_000_000;
        for cell in (0..COIN_ENTRY_DIGITS).rev() {
            let digit = (rest / place) as i8;
            self.digits[cell] = digit;
            rest = rest.wrapping_sub((digit as i32).wrapping_mul(place));
            place /= 10;
        }
    }

    /// Zero every cell (`0x801F0BA4` loop, repeated at `0x801F10A0`).
    pub fn clear_digits(&mut self) {
        self.digits = [0; COIN_ENTRY_DIGITS];
    }
}

/// Cells in the coin counter's decimal entry field.
pub const COIN_ENTRY_DIGITS: usize = 8;
/// Cells the left/right cursor actually visits - retail wraps at `5`, so the
/// top two cells are only ever written by the affordability clamp.
pub const COIN_CURSOR_CELLS: i32 = 6;

/// Idle panel descriptor the counter installs on entry and on a cancelled
/// confirm.
pub const PANEL_COIN_IDLE: u32 = 0x801F_3340;
/// Confirm ("buy this many?") panel descriptor.
pub const PANEL_COIN_CONFIRM: u32 = 0x801F_3360;

/// Cursor-move sting (`FUN_80035B50(0x21)`).
pub const CUE_CURSOR: u8 = 0x21;
/// Accept sting (`FUN_80035B50(0x36)`).
pub const CUE_ACCEPT: u8 = 0x36;
/// Back-out sting (`FUN_80035B50(0x37)`).
pub const CUE_BACK: u8 = 0x37;
/// Coin-jingle sting the commit plays (`FUN_80035B50(0x25)`).
pub const CUE_COINS: u8 = 0x25;
/// Refusal buzz (`FUN_80035BD0(0x23)`) - unaffordable or over the ceiling.
pub const CUE_REFUSE: u8 = 0x23;
/// The picker's own accept sting (`FUN_80035BD0(0)`).
pub const CUE_PICK: u8 = 0x00;

/// Hold frames the commit state waits before closing (`0x801F0FD0`).
pub const COIN_COMMIT_HOLD: i32 = 0x14;
/// Hold frames the post-purchase state waits (`0x801F10BC`).
pub const COIN_CLOSE_HOLD: i32 = 0x10;

/// `FUN_801E9DC8` return meaning "the player accepted the row".
pub const PICK_ACCEPT: i32 = 1;
/// `FUN_801E9DC8` return meaning "the player backed out".
pub const PICK_CANCEL: i32 = 2;

/// PORT: FUN_801f0adc - the casino coin counter: buy coins with gold.
///
/// Handler slot `0x25` of `PTR_FUN_801F33B4`. The head runs every frame
/// regardless of sub-state: it converts party gold into buyable coins at
/// [`GOLD_PER_COIN`] gold each (a signed divide truncating toward zero,
/// spelled in retail as the `0x51EB851F` reciprocal multiply plus the sign
/// fixup), publishes it to `DAT_8007BB90`, and clamps it so the bank cannot
/// pass [`COIN_BANK_MAX`]. It then tail-jumps the sub-handler `[actor+0x54]`
/// out of the five-entry table at `0x801CF734`; the five arms share this
/// frame and fall into its epilogue, which is why the dumped body is
/// discontiguous.
///
/// The five arms, from `overlay_0897_801f0adc.txt` (264 instructions - the
/// minigame-named dumps of this VA stop at 46 and carry only the head, which
/// is why an earlier reading of this routine had no transaction in it at all):
///
/// | `+0x54` | What it does |
/// |---|---|
/// | `0` | Zero the cells + both cursors, install [`PANEL_COIN_IDLE`], advance. |
/// | `1` | Digit entry: cursor moves, digit edits, affordability clamp, accept / back. |
/// | `2` | Yes/No confirm over `FUN_801E9DC8`; accept arms the commit hold. |
/// | `3` | **Commit**: coins to `DAT_800845A4`, gold out of `DAT_8008459C`. |
/// | `4` | Count the hold timer down by the cadence scalar, then hand back. |
///
/// `bank` is read-only here: the commit is reported as
/// [`HubAction::BuyCoins`] so the host applies it to its own money model.
pub fn coin_exchange(
    actor: &mut HubActor,
    env: &HubEnv,
    counter: &mut CoinCounter,
    grid: &mut HubGrid,
) -> HubFrame {
    let mut out = HubFrame::default();
    counter.ceiling = coin_exchange_amount(env.gold, env.coin_bank);
    out.act(HubAction::SetCoinAmount(counter.ceiling));

    // `sltiu v1, 5` - an out-of-range `+0x54` skips straight to the pump.
    let Some(slot) = coin_exchange_slot(actor.sub) else {
        out.act(HubAction::DrawPump);
        return out;
    };

    // Every arm that finishes the screen falls into the shared hand-back
    // tail at `0x801F10F4`; the arms that stay open jump past it.
    let mut hand_back_now = false;
    match slot {
        0 => {
            counter.clear_digits();
            counter.cursor = 0;
            out.act(HubAction::ClearCursorRow);
            out.act(HubAction::InstallPanel(PANEL_COIN_IDLE));
            actor.sub = 1;
        }
        1 => {
            if env.input_blocked != 0 {
                out.act(HubAction::DrawPump);
                return out;
            }
            if env.pad_edge & env.accept_mask != 0 {
                let want = counter.entered();
                if !coin_purchase_affordable(want, env.gold, counter.ceiling) {
                    out.act(HubAction::ConfirmCue(CUE_REFUSE));
                } else if want != 0 {
                    out.act(HubAction::EntryCue(CUE_ACCEPT));
                    out.act(HubAction::InstallPanel(PANEL_COIN_CONFIRM));
                    counter.yes_no = 1;
                    actor.sub = 2;
                } else {
                    // Zero coins: the accept behaves as a back-out.
                    out.act(HubAction::EntryCue(CUE_BACK));
                    hand_back_now = true;
                }
            } else if env.pad_edge & env.back_mask != 0 {
                out.act(HubAction::EntryCue(CUE_BACK));
                hand_back_now = true;
            } else if env.pad_repeat & PAD_CURSOR_RIGHT != 0 {
                out.act(HubAction::EntryCue(CUE_CURSOR));
                counter.cursor = if counter.cursor == 0 {
                    COIN_CURSOR_CELLS - 1
                } else {
                    counter.cursor - 1
                };
            } else if env.pad_repeat & PAD_CURSOR_LEFT != 0 {
                out.act(HubAction::EntryCue(CUE_CURSOR));
                counter.cursor = if counter.cursor == COIN_CURSOR_CELLS - 1 {
                    0
                } else {
                    counter.cursor + 1
                };
            } else {
                if env.pad_repeat & PAD_CURSOR_UP != 0 {
                    out.act(HubAction::EntryCue(CUE_CURSOR));
                    bump_digit(counter, 1);
                }
                if env.pad_repeat & PAD_CURSOR_DOWN != 0 {
                    out.act(HubAction::EntryCue(CUE_CURSOR));
                    bump_digit(counter, -1);
                }
                // Whatever the cells now spell, an unaffordable total is
                // rewritten to the best the player can actually buy.
                let want = counter.entered();
                if !coin_purchase_affordable(want, env.gold, counter.ceiling) {
                    let afford = (env.gold / GOLD_PER_COIN).min(counter.ceiling);
                    counter.set_entered(afford);
                }
            }
        }
        2 => {
            if env.input_blocked != 0 {
                out.act(HubAction::DrawPump);
                return out;
            }
            match env.picker_result {
                PICK_ACCEPT => {
                    out.act(HubAction::ConfirmCue(CUE_PICK));
                    if counter.yes_no != 1 {
                        out.act(HubAction::EntryCue(CUE_COINS));
                        counter.hold = COIN_COMMIT_HOLD;
                        actor.sub = 3;
                    } else {
                        out.act(HubAction::EntryCue(CUE_BACK));
                        out.act(HubAction::InstallPanel(PANEL_COIN_IDLE));
                        actor.sub = 1;
                    }
                }
                PICK_CANCEL => {
                    out.act(HubAction::EntryCue(CUE_BACK));
                    out.act(HubAction::InstallPanel(PANEL_COIN_IDLE));
                    actor.sub = 1;
                }
                _ => {}
            }
        }
        3 => {
            let coins = counter.entered();
            out.act(HubAction::BuyCoins {
                coins,
                gold_cost: coins.wrapping_mul(GOLD_PER_COIN),
            });
            counter.clear_digits();
            out.act(HubAction::ClearCursorRow);
            counter.hold = COIN_CLOSE_HOLD;
            actor.sub = 4;
        }
        _ => {
            counter.hold = counter.hold.wrapping_sub(env.frame_delta);
            if counter.hold <= 0 {
                hand_back_now = true;
            }
        }
    }

    if hand_back_now {
        hand_back(actor, grid);
    }
    out.act(HubAction::DrawPump);
    out
}

/// D-pad bits of the auto-repeating word `_DAT_8007BB84`, in the **packed**
/// Legaia layout (`crate::retail_pad`-shaped, the same bits
/// `legaia_engine_core::dev_menu::PACK_*` names).
///
/// The cell index counts **right to left** - cell `0` is the units digit, the
/// rightmost on screen - so pressing right walks the index down and wraps it
/// to [`COIN_CURSOR_CELLS`]` - 1`, which is why the two arms look inverted
/// against their bit names.
pub const PAD_CURSOR_RIGHT: u32 = 0x2000;
/// See [`PAD_CURSOR_RIGHT`].
pub const PAD_CURSOR_LEFT: u32 = 0x8000;
/// Digit `+1` on the selected cell.
pub const PAD_CURSOR_UP: u32 = 0x1000;
/// Digit `-1` on the selected cell.
pub const PAD_CURSOR_DOWN: u32 = 0x4000;

/// One up/down edit of the selected cell, wrapping `0..=9` (`0x801F0D80` /
/// `0x801F0DE8`).
fn bump_digit(counter: &mut CoinCounter, delta: i8) {
    let Some(cell) = counter.digits.get_mut(counter.cursor.max(0) as usize) else {
        return;
    };
    let next = cell.wrapping_add(delta);
    *cell = if next == 10 {
        0
    } else if next < 0 {
        9
    } else {
        next
    };
}

/// The purchase gate: retail refuses when the gold cost exceeds party gold
/// **or** the amount exceeds the published ceiling (`0x801F0C64..0x801F0C88`).
pub fn coin_purchase_affordable(coins: i32, gold: i32, ceiling: i32) -> bool {
    gold >= coins.wrapping_mul(GOLD_PER_COIN) && ceiling >= coins
}

/// The clamped coin amount `FUN_801F0ADC` publishes to `DAT_8007BB90`.
///
/// PORT: FUN_801f0adc (`0x801F0AE8..0x801F0B44`)
pub fn coin_exchange_amount(gold: i32, coin_bank: i32) -> i32 {
    let coins = gold / GOLD_PER_COIN;
    if COIN_BANK_MAX < coin_bank.wrapping_add(coins) {
        COIN_BANK_MAX - coin_bank
    } else {
        coins
    }
}

/// Sub-handler index, or `None` when `+0x54` is outside the five-slot table
/// (retail's `sltiu v1, 5` guard, which is unsigned - a negative `+0x54`
/// therefore also falls through).
pub fn coin_exchange_slot(sub: i16) -> Option<usize> {
    let s = sub as u16 as u32;
    (s < 5).then_some(s as usize)
}

// ---------------------------------------------------------------------------
// panel state machines
// ---------------------------------------------------------------------------

/// Panel descriptor VAs the state machines install through `FUN_801E9B3C`.
pub const PANEL_START: u32 = 0x801F_3370;
pub const PANEL_SUBMENU_IDLE: u32 = 0x801F_3294;
pub const PANEL_SUBMENU_CONFIRM: u32 = 0x801F_32A4;
pub const PANEL_PROMPT: u32 = 0x801F_3388;
pub const PANEL_DRAW_TICK: u32 = 0x801F_2A88;

/// Whether a confirm press lands this frame: the suppression flag must be
/// clear and the edge-triggered pad must intersect the confirm mask.
fn confirm_pressed(env: &HubEnv) -> bool {
    env.input_blocked == 0 && env.pad_edge & env.confirm_mask != 0
}

/// Hand the actor back to the hub: stash `+0x50` into the grid actor and
/// re-arm to [`HUB_RETURN_STATE`].
fn hand_back(actor: &mut HubActor, grid: &mut HubGrid) {
    grid.handback = -1;
    grid.stashed_state = actor.state;
    actor.state = HUB_RETURN_STATE;
    actor.sub = 0;
}

/// PORT: FUN_801f1138 - the start / confirm menu tick.
///
/// State `0` counts the active entries in the three bytes at
/// `_DAT_8007B450 + 1..=3` (one plus however many are non-zero), sizes the
/// panel to `n * 14 - 2` pixels tall, centres it by writing the top edge as
/// `0x2C - (height >> 1)` (an arithmetic shift on the 16-bit height), installs
/// the panel and advances. State `1` waits for a confirm, plays
/// [`CUE_CONFIRM`] and hands the actor back.
pub fn start_menu(actor: &mut HubActor, env: &HubEnv, grid: &mut HubGrid) -> HubFrame {
    let mut out = HubFrame::default();
    match actor.sub {
        0 => {
            let mut n: i16 = 1;
            if env.board_entries[0] != 0 {
                n = 2;
            }
            if env.board_entries[1] != 0 {
                n += 1;
            }
            if env.board_entries[2] != 0 {
                n += 1;
            }
            let height = n.wrapping_mul(14).wrapping_sub(2);
            out.act(HubAction::SizePanel {
                height,
                top: 0x2C - (height >> 1),
            });
            out.act(HubAction::InstallPanel(PANEL_START));
            actor.sub = actor.sub.wrapping_add(1);
        }
        1 if confirm_pressed(env) => {
            out.act(HubAction::ConfirmCue(CUE_CONFIRM));
            hand_back(actor, grid);
        }
        _ => {}
    }
    out.act(HubAction::DrawPump);
    out
}

/// PORT: FUN_801f1e48 - the hub sub-menu state machine.
///
/// Three states: `0` clears the cursor row and installs the idle panel, `1`
/// waits for a confirm and swaps to the confirm panel, `2` clears the board
/// flag and hands the actor back.
pub fn submenu(actor: &mut HubActor, env: &HubEnv, grid: &mut HubGrid) -> HubFrame {
    let mut out = HubFrame::default();
    match actor.sub {
        0 => {
            out.act(HubAction::ClearCursorRow);
            out.act(HubAction::InstallPanel(PANEL_SUBMENU_IDLE));
            actor.sub = actor.sub.wrapping_add(1);
        }
        1 if confirm_pressed(env) => {
            out.act(HubAction::ConfirmCue(CUE_CONFIRM));
            out.act(HubAction::InstallPanel(PANEL_SUBMENU_CONFIRM));
            actor.sub = 2;
        }
        2 => {
            out.act(HubAction::ClearBoardFlag);
            hand_back(actor, grid);
        }
        _ => {}
    }
    out.act(HubAction::DrawPump);
    out
}

/// PORT: FUN_801f1fdc - the hub prompt state machine.
///
/// State `0` plays the entry sting [`CUE_ENTRY`] and installs the prompt
/// panel; state `1` waits for a confirm, plays [`CUE_CONFIRM`] and hands the
/// actor back. Unlike [`submenu`] it never clears the board flag.
pub fn hub_prompt(actor: &mut HubActor, env: &HubEnv, grid: &mut HubGrid) -> HubFrame {
    let mut out = HubFrame::default();
    match actor.sub {
        0 => {
            out.act(HubAction::EntryCue(CUE_ENTRY));
            out.act(HubAction::InstallPanel(PANEL_PROMPT));
            actor.sub = actor.sub.wrapping_add(1);
        }
        1 if confirm_pressed(env) => {
            out.act(HubAction::ConfirmCue(CUE_CONFIRM));
            hand_back(actor, grid);
        }
        _ => {}
    }
    out.act(HubAction::DrawPump);
    out
}

/// PORT: FUN_801f20b0 - the panel-install draw tick.
///
/// State `0` installs [`PANEL_DRAW_TICK`] and advances; any state above `1`
/// returns before the pump. Otherwise it pumps and, while the suppression
/// flag is clear, clears the grid actor's completion gate - which is what
/// lets [`hub_dispatch`] retire the actor on the following frame.
pub fn draw_tick(actor: &mut HubActor, env: &HubEnv, grid: &mut HubGrid) -> HubFrame {
    let mut out = HubFrame::default();
    match actor.sub {
        0 => {
            out.act(HubAction::InstallPanel(PANEL_DRAW_TICK));
            actor.sub = actor.sub.wrapping_add(1);
        }
        1 => {}
        _ => return out,
    }
    out.act(HubAction::DrawPump);
    if env.input_blocked == 0 {
        grid.done_gate = 0;
    }
    out
}

/// PORT: FUN_801f2134 - the close-sting draw tick.
///
/// [`draw_tick`]'s twin: identical but for state `0`, which plays the close
/// sting instead of installing a panel.
pub fn close_tick(actor: &mut HubActor, env: &HubEnv, grid: &mut HubGrid) -> HubFrame {
    let mut out = HubFrame::default();
    match actor.sub {
        0 => {
            out.act(HubAction::CloseCue);
            actor.sub = actor.sub.wrapping_add(1);
        }
        1 => {}
        _ => return out,
    }
    out.act(HubAction::DrawPump);
    if env.input_blocked == 0 {
        grid.done_gate = 0;
    }
    out
}

/// PORT: FUN_801f1d90 - the actor deactivate with a chosen re-arm state.
///
/// Plays the close sting, pumps, then hands the actor back with `+0x50` set
/// to [`HUB_SKIP_STATE`] when the first progress flag is set or the second is
/// set *and* the cancel button is held, and to [`HUB_DEACTIVATE_STATE`]
/// otherwise. The two arms are otherwise identical: retail writes the grid
/// hand-back fields in both.
pub fn deactivate(actor: &mut HubActor, env: &HubEnv, grid: &mut HubGrid) -> HubFrame {
    let mut out = HubFrame::default();
    out.act(HubAction::CloseCue);
    out.act(HubAction::DrawPump);
    grid.handback = -1;
    grid.stashed_state = actor.state;
    let skip = env.progress_a != 0 || (env.progress_b != 0 && env.pad_held & env.cancel_mask != 0);
    actor.state = if skip {
        HUB_SKIP_STATE
    } else {
        HUB_DEACTIVATE_STATE
    };
    actor.sub = 0;
    out
}

// ---------------------------------------------------------------------------
// panel draws
// ---------------------------------------------------------------------------

/// Rodata VAs of the literal strings the panel draws load.
pub const STR_ROW_HEADER: u32 = 0x801C_F09C;
pub const STR_THREE_LINE: [u32; 3] = [0x801C_F108, 0x801C_F10C, 0x801C_F110];
pub const STR_TWO_OPTION: [u32; 2] = [0x801C_F138, 0x801C_F140];
pub const STR_COUNT_GATED: [u32; 2] = [0x801C_F14C, 0x801C_F170];
pub const STR_TWO_LINE: [u32; 2] = [0x801C_F190, 0x801C_F198];
pub const STR_SINGLE: u32 = 0x801C_F1A4;
pub const STR_CAPTION: u32 = 0x801C_EA30;

/// PORT: FUN_801f16c0 - the stacked per-entry label list.
///
/// Walks the `DAT_80084594` entries, publishing each entry's code byte to
/// `DAT_8007B469` first because the per-entry sub-draw `FUN_801E5B4C` reads
/// it. Codes at or above `3` draw nothing at all, but still cost a loop step.
/// Each drawn entry prints its label at the running `y`, advances `y` by
/// `0x0D` for the sub-draw and by a further `0x2A` afterwards; the actor's own
/// `+0x0C` is restored at the end, as is the previous entry code.
///
/// The sub-draw is [`equip_stat_panel`], and the `jal 0x801F1778` that reaches
/// it is that routine's only reference in the corpus - so the equipment
/// stat panel exists for this list and nothing else. Its rows are spliced in
/// as [`HubDraw::EntrySubPanel`] at the point the `jal` sits, between the
/// `0x0D` and the `0x2A` pen advances.
pub fn entry_list(actor: &mut HubActor, env: &HubEnv) -> HubFrame {
    let mut out = HubFrame::default();
    let saved_y = actor.y;
    for i in 0..env.entry_count as usize {
        let code = env.entry_codes.get(i).copied().unwrap_or(0);
        out.act(HubAction::SetEntryCode(code));
        if code < 3 {
            out.draw(HubDraw::Text {
                text: HubString::EntryLabel(code),
                x: actor.x,
                y: actor.y,
                palette: PALETTE_PANEL,
            });
            actor.y = actor.y.wrapping_add(0x0D);
            for d in entry_sub_panel(actor, env, code) {
                out.draw(HubDraw::EntrySubPanel(d));
            }
            actor.y = actor.y.wrapping_add(0x2A);
        }
    }
    actor.y = saved_y;
    out
}

/// Build the `FUN_801E5B4C` call [`entry_list`] makes for one entry.
///
/// Retail passes only the actor; everything else the sub-draw needs it reads
/// out of globals - the entry code the caller just published to
/// `DAT_8007B469`, the save block, the two static tables and the menu mode
/// word. This assembles the same inputs out of [`HubEnv::equip`].
fn entry_sub_panel(actor: &HubActor, env: &HubEnv, code: u8) -> Vec<EquipPanelDraw> {
    let eq = &env.equip;
    let record = eq
        .records
        .iter()
        .find(|r| r.code == code)
        .copied()
        .unwrap_or_default();
    let input = EquipPanelInput {
        x: actor.x,
        y: actor.y,
        char_index: usize::from(code),
        slots: record.slots,
        base_stats: record.base_stats,
        mode: eq.mode,
        cursor: env.cursor_row,
        inventory: eq.inventory.clone(),
        weapon_slots: eq.weapon_slots.clone(),
        // `_DAT_8007B450` is the same op-`0x49` descriptor cell `HubEnv`
        // already carries as a truth value.
        tight_rows: env.board_flag != 0,
    };
    equip_stat_panel(
        &input,
        |id| {
            eq.item_props
                .get(usize::from(id))
                .copied()
                .unwrap_or_default()
        },
        |idx| {
            eq.equip_props
                .get(usize::from(idx))
                .copied()
                .unwrap_or_default()
        },
    )
}

/// PORT: FUN_801f17d8 - the header string plus one sprite cell per grid column.
///
/// The header prints at the actor origin; the cell row starts `0x10` in on
/// both axes and steps `0x20` per column, its cell id being the grid actor's
/// `+0x54 + i` byte biased by `0x37`. The row length is `_DAT_8007BB88`, not
/// the entry count.
pub fn column_row(actor: &HubActor, env: &HubEnv, grid: &HubGrid) -> HubFrame {
    let mut out = HubFrame::default();
    out.draw(HubDraw::HeaderText {
        text: HubString::Literal(STR_ROW_HEADER),
        x: actor.x,
        y: actor.y,
    });
    let mut x = actor.x.wrapping_add(0x10);
    let y = actor.y.wrapping_add(0x10);
    for i in 0..env.cursor_row.max(0) as usize {
        let cell = grid.columns.get(i).copied().unwrap_or(0) as i32 + 0x37;
        out.draw(HubDraw::Cell { x, y, cell });
        x = x.wrapping_add(0x20);
    }
    out
}

/// PORT: FUN_801f1890 - the three-line panel with its own cursor row.
///
/// The first line uses [`PALETTE_PANEL`], the lower two [`PALETTE_DIM`]; the
/// cursor sits at `x + 0x38` and steps `0x0E` per `_DAT_8007BB98` row -
/// retail spells that pitch as `(n * 8 - n) << 1`.
pub fn three_line_panel(actor: &HubActor, env: &HubEnv) -> HubFrame {
    let mut out = HubFrame::default();
    let (x, y) = (actor.x, actor.y);
    out.draw(HubDraw::ShortText {
        text: HubString::Literal(STR_THREE_LINE[0]),
        x: x.wrapping_add(0x24),
        y,
        palette: PALETTE_PANEL,
    });
    out.draw(HubDraw::ShortText {
        text: HubString::Literal(STR_THREE_LINE[1]),
        x: x.wrapping_add(0x4C),
        y: y.wrapping_add(0x10),
        palette: PALETTE_DIM,
    });
    out.draw(HubDraw::ShortText {
        text: HubString::Literal(STR_THREE_LINE[2]),
        x: x.wrapping_add(0x4C),
        y: y.wrapping_add(0x1E),
        palette: PALETTE_DIM,
    });
    out.draw(HubDraw::Sprite {
        a: 0,
        b: 1,
        x: x.wrapping_add(0x38),
        y: y.wrapping_add(0x10)
            .wrapping_add((env.cursor_row_alt * 0x0E) as i16),
    });
    out
}

/// PORT: FUN_801f1950 - the two-option panel.
///
/// Each option draws its cursor *before* its label and only when
/// `_DAT_8007BB88` selects that row, so the two tests read the global twice
/// with the labels between them.
pub fn two_option_panel(actor: &HubActor, env: &HubEnv) -> HubFrame {
    let mut out = HubFrame::default();
    let (x, y) = (actor.x, actor.y);
    if env.cursor_row == 0 {
        out.draw(HubDraw::Sprite { a: 0, b: 1, x, y });
    }
    out.draw(HubDraw::Text {
        text: HubString::Literal(STR_TWO_OPTION[0]),
        x: x.wrapping_add(0x14),
        y,
        palette: PALETTE_PANEL,
    });
    if env.cursor_row == 1 {
        out.draw(HubDraw::Sprite {
            a: 0,
            b: 1,
            x,
            y: y.wrapping_add(0x0E),
        });
    }
    out.draw(HubDraw::Text {
        text: HubString::Literal(STR_TWO_OPTION[1]),
        x: x.wrapping_add(0x14),
        y: y.wrapping_add(0x0E),
        palette: PALETTE_PANEL,
    });
    out
}

/// The right-edge cursor both single-label panels place: `x + width - 0x10`,
/// `y - 2`, sprite `(1, 1)`.
fn edge_cursor(actor: &HubActor) -> HubDraw {
    HubDraw::Sprite {
        a: 1,
        b: 1,
        x: actor.x.wrapping_add(actor.width).wrapping_sub(0x10),
        y: actor.y.wrapping_sub(2),
    }
}

/// PORT: FUN_801f1a1c - the count-gated single label.
///
/// Picks the alternate string when the entry count `DAT_80084594` is below
/// `2` (an unsigned byte test), draws it `0x0C` in from the panel origin, and
/// finishes with the shared right-edge cursor.
pub fn count_gated_label(actor: &HubActor, env: &HubEnv) -> HubFrame {
    let mut out = HubFrame::default();
    let text = if env.entry_count < 2 {
        STR_COUNT_GATED[1]
    } else {
        STR_COUNT_GATED[0]
    };
    out.draw(HubDraw::Text {
        text: HubString::Literal(text),
        x: actor.x.wrapping_add(0x0C),
        y: actor.y,
        palette: PALETTE_PANEL,
    });
    out.draw(edge_cursor(actor));
    out
}

/// PORT: FUN_801f1b64 - the single label plus the right-edge cursor.
///
/// [`count_gated_label`] without the count test.
pub fn single_label(actor: &HubActor) -> HubFrame {
    let mut out = HubFrame::default();
    out.draw(HubDraw::Text {
        text: HubString::Literal(STR_SINGLE),
        x: actor.x.wrapping_add(0x0C),
        y: actor.y,
        palette: PALETTE_PANEL,
    });
    out.draw(edge_cursor(actor));
    out
}

/// PORT: FUN_801f1ab0 - the two-line panel with the screen-effect push.
///
/// Both lines start `0x0C` in and are `0x10` apart; the cursor sits `8` left
/// of the origin and steps `0x10` per `_DAT_8007BB88` row. The trailing
/// `FUN_80024EE4(3, 0, 0)` is the only screen-effect push in the family.
pub fn two_line_panel(actor: &HubActor, env: &HubEnv) -> HubFrame {
    let mut out = HubFrame::default();
    let (x, y) = (actor.x, actor.y);
    out.draw(HubDraw::Text {
        text: HubString::Literal(STR_TWO_LINE[0]),
        x: x.wrapping_add(0x0C),
        y,
        palette: PALETTE_PANEL,
    });
    out.draw(HubDraw::Text {
        text: HubString::Literal(STR_TWO_LINE[1]),
        x: x.wrapping_add(0x0C),
        y: y.wrapping_add(0x10),
        palette: PALETTE_PANEL,
    });
    out.draw(HubDraw::Sprite {
        a: 0,
        b: 1,
        x: x.wrapping_sub(8),
        y: y.wrapping_add((env.cursor_row * 0x10) as i16),
    });
    out.draw(HubDraw::Effect(3));
    out
}

// NOT WIRED: unlike the other seven painters this VA has **no reference
// anywhere in the field overlay's bytes** - it is in neither
// `PTR_FUN_801F33B4` nor `PANEL_WINDOW_TABLE` - and it sits in the resident
// slot-B band whose widget descriptors `engine-core::screen_fx` pins at
// `0x801F8FE4..0x801F902C`. Its owning image is therefore unsettled, and its
// only dump ends on `Control flow encountered bad instruction data`. What has
// to exist first is a base-confirmed dump of the image that really owns
// `0x801F90DC`, so a host knows which subsystem's caller to attach it to.
/// PORT: FUN_801f90dc - the item-acquisition caption.
///
/// `DAT_801E46B0` is an **item id**, and the two strings come from the static
/// `SCUS_942.54` item table (`0x8007436C + id * 0x0C`, see
/// [`docs/formats/item-table.md`]) - the name at the record's word `0` and the
/// detail line at word `1`. The special id [`CAPTION_MONEY_ID`] is the money
/// pseudo-item: it adds a fixed caption and prints the eight-digit amount from
/// `_DAT_800845B4`.
///
/// The Ghidra dump of this body stops after the money arm with no epilogue
/// (`Control flow encountered bad instruction data`), so anything past the
/// number draw is unrecovered; what is here is the whole disassembled extent.
pub fn acquisition_caption(actor: &HubActor, env: &HubEnv) -> HubFrame {
    let mut out = HubFrame::default();
    let (x, y) = (actor.x, actor.y);
    let id = env.caption_item;
    out.draw(HubDraw::Text {
        text: HubString::ItemName(id),
        x,
        y,
        palette: PALETTE_CAPTION,
    });
    out.draw(HubDraw::Detail {
        text: HubString::ItemDetail(id),
        id,
        x,
        y: y.wrapping_add(0x10),
        // palette 7 is selected before this call
    });
    if id == CAPTION_MONEY_ID {
        out.draw(HubDraw::Text {
            text: HubString::Literal(STR_CAPTION),
            x: x.wrapping_add(0x18),
            y: y.wrapping_add(0x41),
            palette: PALETTE_PANEL,
        });
        out.draw(HubDraw::Number {
            value: env.caption_amount,
            digits: 8,
            x: x.wrapping_add(0x38),
            y: y.wrapping_add(0x4E),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env() -> HubEnv {
        HubEnv {
            submode: 1,
            confirm_mask: 0x0060,
            cancel_mask: 0x0010,
            accept_mask: 0x0040,
            back_mask: 0x0020,
            frame_delta: 1,
            ..HubEnv::default()
        }
    }

    /// Walk the counter from a fresh open to the commit with `coins` typed in,
    /// returning every action the run emitted.
    fn run_purchase(env0: &HubEnv, coins: i32) -> (Vec<HubAction>, CoinCounter, HubActor) {
        let mut a = HubActor {
            state: 0x25,
            ..HubActor::default()
        };
        let mut c = CoinCounter::default();
        let mut g = HubGrid::default();
        let mut acts = Vec::new();
        let mut e = env0.clone();

        // State 0 -> 1.
        acts.extend(coin_exchange(&mut a, &e, &mut c, &mut g).actions);
        // Type the amount straight into the cells the way the clamp does.
        c.set_entered(coins);
        // Accept -> confirm panel.
        e.pad_edge = e.accept_mask;
        acts.extend(coin_exchange(&mut a, &e, &mut c, &mut g).actions);
        // Pick "Yes" (row 0) and accept.
        e.pad_edge = 0;
        c.yes_no = 0;
        e.picker_result = PICK_ACCEPT;
        acts.extend(coin_exchange(&mut a, &e, &mut c, &mut g).actions);
        // Commit.
        e.picker_result = 0;
        acts.extend(coin_exchange(&mut a, &e, &mut c, &mut g).actions);
        (acts, c, a)
    }

    #[test]
    fn coin_counter_credits_coins_and_debits_gold() {
        // The regression this test exists for: the credit must land in the
        // casino coin bank, never in gold, and the debit is 100x the coins.
        let mut e = env();
        e.gold = 12_345;
        let (acts, _, a) = run_purchase(&e, 100);
        assert!(acts.contains(&HubAction::BuyCoins {
            coins: 100,
            gold_cost: 10_000,
        }));
        // The commit arms the close hold rather than handing back at once.
        assert_eq!(a.sub, 4);
    }

    #[test]
    fn coin_counter_hold_runs_out_before_the_hand_back() {
        let mut e = env();
        e.gold = 1_000;
        let (_, mut c, mut a) = run_purchase(&e, 5);
        let mut g = HubGrid::default();
        assert_eq!(c.hold, COIN_CLOSE_HOLD);
        for _ in 0..COIN_CLOSE_HOLD - 1 {
            coin_exchange(&mut a, &e, &mut c, &mut g);
            assert_eq!(a.state, 0x25, "still on its own slot while the hold runs");
        }
        coin_exchange(&mut a, &e, &mut c, &mut g);
        assert_eq!(a.state, HUB_RETURN_STATE);
        assert_eq!(g.handback, -1);
        assert_eq!(g.stashed_state, 0x25);
    }

    #[test]
    fn coin_counter_digit_cells_are_little_endian() {
        let mut c = CoinCounter::default();
        c.set_entered(1_203);
        assert_eq!(c.digits, [3, 0, 2, 1, 0, 0, 0, 0]);
        assert_eq!(c.entered(), 1_203);
        c.set_entered(9_999_999);
        assert_eq!(c.entered(), 9_999_999);
    }

    #[test]
    fn coin_counter_cursor_wraps_over_six_cells_not_eight() {
        let mut e = env();
        e.gold = 10_000_000;
        let mut a = HubActor {
            sub: 1,
            ..HubActor::default()
        };
        let mut c = CoinCounter::default();
        let mut g = HubGrid::default();
        e.pad_repeat = PAD_CURSOR_RIGHT;
        coin_exchange(&mut a, &e, &mut c, &mut g);
        assert_eq!(
            c.cursor,
            COIN_CURSOR_CELLS - 1,
            "right off the units cell wraps to the top cell"
        );
        e.pad_repeat = PAD_CURSOR_LEFT;
        coin_exchange(&mut a, &e, &mut c, &mut g);
        assert_eq!(c.cursor, 0);
    }

    #[test]
    fn coin_counter_rewrites_an_unaffordable_amount() {
        // 250 gold buys two coins; typing nine into the tens cell asks for 90.
        let mut e = env();
        e.gold = 250;
        let mut a = HubActor {
            sub: 1,
            ..HubActor::default()
        };
        let mut c = CoinCounter {
            cursor: 1,
            ..CoinCounter::default()
        };
        let mut g = HubGrid::default();
        e.pad_repeat = PAD_CURSOR_DOWN; // 0 -> 9 in cell 1
        coin_exchange(&mut a, &e, &mut c, &mut g);
        assert_eq!(c.entered(), 2, "clamped to what 250 gold can pay for");
        assert_eq!(a.sub, 1, "the clamp keeps the screen open");
    }

    #[test]
    fn coin_counter_refuses_an_over_ceiling_accept_without_leaving_entry() {
        let mut e = env();
        e.gold = 9_999_999;
        e.coin_bank = COIN_BANK_MAX; // no headroom at all
        let mut a = HubActor {
            sub: 1,
            ..HubActor::default()
        };
        let mut c = CoinCounter::default();
        c.set_entered(1);
        let mut g = HubGrid::default();
        e.pad_edge = e.accept_mask;
        let f = coin_exchange(&mut a, &e, &mut c, &mut g);
        assert!(f.actions.contains(&HubAction::ConfirmCue(CUE_REFUSE)));
        assert_eq!(a.sub, 1);
        assert!(
            !f.actions
                .iter()
                .any(|a| matches!(a, HubAction::BuyCoins { .. }))
        );
    }

    #[test]
    fn coin_counter_back_edge_hands_back_without_buying() {
        let mut e = env();
        e.gold = 5_000;
        let mut a = HubActor {
            state: 0x25,
            sub: 1,
            ..HubActor::default()
        };
        let mut c = CoinCounter::default();
        c.set_entered(10);
        let mut g = HubGrid::default();
        e.pad_edge = e.back_mask;
        let f = coin_exchange(&mut a, &e, &mut c, &mut g);
        assert!(
            !f.actions
                .iter()
                .any(|a| matches!(a, HubAction::BuyCoins { .. }))
        );
        assert_eq!(a.state, HUB_RETURN_STATE);
    }

    #[test]
    fn coin_counter_zero_accept_is_a_back_out() {
        let mut e = env();
        e.gold = 5_000;
        let mut a = HubActor {
            state: 0x25,
            sub: 1,
            ..HubActor::default()
        };
        let mut c = CoinCounter::default();
        let mut g = HubGrid::default();
        e.pad_edge = e.accept_mask;
        let f = coin_exchange(&mut a, &e, &mut c, &mut g);
        assert!(f.actions.contains(&HubAction::EntryCue(CUE_BACK)));
        assert_eq!(a.state, HUB_RETURN_STATE);
    }

    #[test]
    fn coin_counter_confirm_panel_no_returns_to_entry() {
        let mut e = env();
        e.gold = 5_000;
        let mut a = HubActor {
            sub: 2,
            ..HubActor::default()
        };
        let mut c = CoinCounter {
            yes_no: 1, // the seeded row is "No"
            ..CoinCounter::default()
        };
        let mut g = HubGrid::default();
        e.picker_result = PICK_ACCEPT;
        let f = coin_exchange(&mut a, &e, &mut c, &mut g);
        assert!(
            f.actions
                .contains(&HubAction::InstallPanel(PANEL_COIN_IDLE))
        );
        assert_eq!(a.sub, 1);
    }

    #[test]
    fn coin_exchange_buys_at_a_hundred_gold_each() {
        assert_eq!(coin_exchange_amount(0, 0), 0);
        assert_eq!(coin_exchange_amount(99, 0), 0);
        assert_eq!(coin_exchange_amount(100, 0), 1);
        assert_eq!(coin_exchange_amount(12_345, 0), 123);
    }

    #[test]
    fn coin_exchange_clamps_against_the_bank_ceiling() {
        // Buying would overflow the bank: the amount becomes the headroom.
        let bank = COIN_BANK_MAX - 5;
        assert_eq!(coin_exchange_amount(1_000_000, bank), 5);
        assert_eq!(coin_exchange_amount(1_000_000, COIN_BANK_MAX), 0);
    }

    #[test]
    fn coin_exchange_divide_truncates_toward_zero() {
        // Retail's reciprocal multiply subtracts the sign, so a debt rounds
        // toward zero rather than down.
        assert_eq!(coin_exchange_amount(-150, 0), -1);
    }

    #[test]
    fn coin_exchange_slot_guard_is_unsigned() {
        assert_eq!(coin_exchange_slot(0), Some(0));
        assert_eq!(coin_exchange_slot(4), Some(4));
        assert_eq!(coin_exchange_slot(5), None);
        assert_eq!(coin_exchange_slot(-1), None);
    }

    #[test]
    fn dispatcher_is_inert_outside_the_three_submodes() {
        let mut a = HubActor::default();
        let mut grid = HubGrid::default();
        let mut e = env();
        e.submode = 2;
        let f = hub_dispatch(&mut a, &e, &mut grid, |_, _| HubFrame::default());
        assert!(f.draws.is_empty() && f.actions.is_empty());
        assert_eq!(a.flags, 0);
    }

    #[test]
    fn dispatcher_retires_on_the_pad_latch_without_running_the_handler() {
        let mut a = HubActor::default();
        let mut grid = HubGrid::default();
        let mut e = env();
        e.pad_latch = PAD_LATCH_SUSPEND;
        let mut ran = false;
        let f = hub_dispatch(&mut a, &e, &mut grid, |_, _| {
            ran = true;
            HubFrame::default()
        });
        assert!(!ran);
        assert_eq!(a.flags & ACTOR_RETIRE, ACTOR_RETIRE);
        assert!(f.actions.is_empty());
    }

    #[test]
    fn dispatcher_release_arm_picks_by_the_board_flag() {
        let mut a = HubActor::default();
        let mut grid = HubGrid::default();
        let mut e = env();
        e.board_flag = 0;
        let f = hub_dispatch(&mut a, &e, &mut grid, |_, _| HubFrame::default());
        assert!(f.actions.contains(&HubAction::ClearBusyBit));
        e.board_flag = 1;
        let mut a = HubActor::default();
        let f = hub_dispatch(&mut a, &e, &mut grid, |_, _| HubFrame::default());
        assert!(f.actions.contains(&HubAction::SetBoardFlag));
    }

    #[test]
    fn start_menu_sizes_and_centres_the_panel() {
        let mut a = HubActor::default();
        let mut g = HubGrid::default();
        let mut e = env();
        e.board_entries = [1, 1, 0];
        let f = start_menu(&mut a, &e, &mut g);
        // one base row plus two active entries = 3 rows.
        assert!(f.actions.contains(&HubAction::SizePanel {
            height: 3 * 14 - 2,
            top: 0x2C - ((3 * 14 - 2) >> 1),
        }));
        assert_eq!(a.sub, 1);
    }

    #[test]
    fn start_menu_confirm_hands_the_actor_back() {
        let mut a = HubActor {
            state: 0x11,
            sub: 1,
            ..HubActor::default()
        };
        let mut g = HubGrid::default();
        let mut e = env();
        e.pad_edge = e.confirm_mask;
        let f = start_menu(&mut a, &e, &mut g);
        assert!(f.actions.contains(&HubAction::ConfirmCue(CUE_CONFIRM)));
        assert_eq!(g.stashed_state, 0x11);
        assert_eq!(g.handback, -1);
        assert_eq!(a.state, HUB_RETURN_STATE);
        assert_eq!(a.sub, 0);
    }

    #[test]
    fn a_blocked_frame_swallows_the_confirm() {
        let mut a = HubActor {
            sub: 1,
            ..HubActor::default()
        };
        let mut g = HubGrid::default();
        let mut e = env();
        e.pad_edge = e.confirm_mask;
        e.input_blocked = 1;
        let f = hub_prompt(&mut a, &e, &mut g);
        assert!(!f.actions.contains(&HubAction::ConfirmCue(CUE_CONFIRM)));
        assert_eq!(a.sub, 1);
    }

    #[test]
    fn submenu_walks_its_three_states() {
        let mut a = HubActor::default();
        let mut g = HubGrid::default();
        let mut e = env();
        let f = submenu(&mut a, &e, &mut g);
        assert!(
            f.actions
                .contains(&HubAction::InstallPanel(PANEL_SUBMENU_IDLE))
        );
        e.pad_edge = e.confirm_mask;
        let f = submenu(&mut a, &e, &mut g);
        assert!(
            f.actions
                .contains(&HubAction::InstallPanel(PANEL_SUBMENU_CONFIRM))
        );
        assert_eq!(a.sub, 2);
        let f = submenu(&mut a, &e, &mut g);
        assert!(f.actions.contains(&HubAction::ClearBoardFlag));
        assert_eq!(a.state, HUB_RETURN_STATE);
    }

    #[test]
    fn draw_tick_clears_the_completion_gate_only_while_unblocked() {
        let mut a = HubActor {
            sub: 1,
            ..HubActor::default()
        };
        let mut g = HubGrid {
            done_gate: 5,
            ..HubGrid::default()
        };
        let mut e = env();
        e.input_blocked = 1;
        draw_tick(&mut a, &e, &mut g);
        assert_eq!(g.done_gate, 5);
        e.input_blocked = 0;
        draw_tick(&mut a, &e, &mut g);
        assert_eq!(g.done_gate, 0);
    }

    #[test]
    fn draw_tick_returns_before_the_pump_past_state_one() {
        let mut a = HubActor {
            sub: 2,
            ..HubActor::default()
        };
        let mut g = HubGrid {
            done_gate: 5,
            ..HubGrid::default()
        };
        let f = draw_tick(&mut a, &env(), &mut g);
        assert!(f.actions.is_empty());
        assert_eq!(g.done_gate, 5);
    }

    #[test]
    fn close_tick_differs_from_draw_tick_only_in_state_zero() {
        let mut a = HubActor::default();
        let mut g = HubGrid::default();
        let f = close_tick(&mut a, &env(), &mut g);
        assert_eq!(f.actions[0], HubAction::CloseCue);
    }

    #[test]
    fn deactivate_picks_the_skip_state_from_the_progress_flags() {
        let mut g = HubGrid::default();
        let mut e = env();
        let mut a = HubActor::default();
        deactivate(&mut a, &e, &mut g);
        assert_eq!(a.state, HUB_DEACTIVATE_STATE);

        e.progress_a = 1;
        let mut a = HubActor::default();
        deactivate(&mut a, &e, &mut g);
        assert_eq!(a.state, HUB_SKIP_STATE);

        e.progress_a = 0;
        e.progress_b = 1;
        e.pad_held = e.cancel_mask;
        let mut a = HubActor::default();
        deactivate(&mut a, &e, &mut g);
        assert_eq!(a.state, HUB_SKIP_STATE);

        // The second flag alone is not enough - the cancel button must be held.
        e.pad_held = 0;
        let mut a = HubActor::default();
        deactivate(&mut a, &e, &mut g);
        assert_eq!(a.state, HUB_DEACTIVATE_STATE);
    }

    #[test]
    fn entry_list_skips_codes_at_or_above_three_and_restores_y() {
        let mut a = HubActor {
            x: 10,
            y: 20,
            ..HubActor::default()
        };
        let mut e = env();
        e.entry_count = 3;
        e.entry_codes = vec![0, 5, 2];
        let f = entry_list(&mut a, &e);
        assert_eq!(a.y, 20);
        let labels: Vec<_> = f
            .draws
            .iter()
            .filter_map(|d| match d {
                HubDraw::Text { text, y, .. } => Some((*text, *y)),
                _ => None,
            })
            .collect();
        assert_eq!(
            labels,
            vec![
                (HubString::EntryLabel(0), 20),
                (HubString::EntryLabel(2), 20 + 0x0D + 0x2A),
            ]
        );
        // Every entry publishes its code, drawn or not.
        assert_eq!(
            f.actions,
            vec![
                HubAction::SetEntryCode(0),
                HubAction::SetEntryCode(5),
                HubAction::SetEntryCode(2),
            ]
        );
    }

    #[test]
    fn column_row_steps_thirty_two_pixels_and_biases_the_cell() {
        let a = HubActor {
            x: 0,
            y: 0,
            ..HubActor::default()
        };
        let mut e = env();
        e.cursor_row = 2;
        let g = HubGrid {
            columns: vec![1, 4],
            ..HubGrid::default()
        };
        let f = column_row(&a, &e, &g);
        assert_eq!(
            f.draws[1..],
            [
                HubDraw::Cell {
                    x: 0x10,
                    y: 0x10,
                    cell: 0x38
                },
                HubDraw::Cell {
                    x: 0x30,
                    y: 0x10,
                    cell: 0x3B
                },
            ]
        );
    }

    #[test]
    fn two_option_panel_draws_the_cursor_before_its_own_label() {
        let a = HubActor::default();
        let mut e = env();
        e.cursor_row = 1;
        let f = two_option_panel(&a, &e);
        assert!(matches!(f.draws[0], HubDraw::Text { .. }));
        assert!(matches!(f.draws[1], HubDraw::Sprite { .. }));
        assert!(matches!(f.draws[2], HubDraw::Text { .. }));
    }

    #[test]
    fn count_gated_label_picks_the_alternate_below_two() {
        let a = HubActor::default();
        let mut e = env();
        e.entry_count = 1;
        let f = count_gated_label(&a, &e);
        assert!(matches!(
            f.draws[0],
            HubDraw::Text { text: HubString::Literal(v), .. } if v == STR_COUNT_GATED[1]
        ));
        e.entry_count = 2;
        let f = count_gated_label(&a, &e);
        assert!(matches!(
            f.draws[0],
            HubDraw::Text { text: HubString::Literal(v), .. } if v == STR_COUNT_GATED[0]
        ));
    }

    #[test]
    fn edge_cursor_anchors_on_the_panel_right_edge() {
        let a = HubActor {
            x: 0x20,
            y: 0x30,
            width: 0x60,
            ..HubActor::default()
        };
        assert_eq!(
            single_label(&a).draws[1],
            HubDraw::Sprite {
                a: 1,
                b: 1,
                x: 0x20 + 0x60 - 0x10,
                y: 0x2E
            }
        );
    }

    #[test]
    fn acquisition_caption_adds_the_amount_only_for_the_money_id() {
        let a = HubActor::default();
        let mut e = env();
        e.caption_item = 3;
        assert_eq!(acquisition_caption(&a, &e).draws.len(), 2);
        e.caption_item = CAPTION_MONEY_ID;
        e.caption_amount = 1234;
        let f = acquisition_caption(&a, &e);
        assert_eq!(f.draws.len(), 4);
        assert_eq!(
            f.draws[3],
            HubDraw::Number {
                value: 1234,
                digits: 8,
                x: 0x38,
                y: 0x4E
            }
        );
    }
}
