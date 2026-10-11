//! The handler family's data model: the actor record, the environment views, the draw list, the actions and the per-frame result.

use super::*;

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
    /// `_DAT_800845B4` - the Point Card bank the `0xFE` caption arm prints.
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
    /// `FUN_80035A4C()` - begin closing every text-actor panel on the
    /// `gp+0x148` list. No sound: an earlier reading named it a close sting.
    CloseAllPanels,
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
    /// The start panel's height and top, written into **window record 8** -
    /// `0x801F2B98 + 8*0x1C + 0xE` and `+0xA`, the record the start menu's own
    /// descriptor then installs.
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
    pub(super) fn draw(&mut self, d: HubDraw) {
        self.draws.push(d);
    }
    pub(super) fn act(&mut self, a: HubAction) {
        self.actions.push(a);
    }
}
