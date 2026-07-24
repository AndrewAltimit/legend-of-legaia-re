//! The **minigame-hub system-actor handler family** in the `0x801F0000+` band
//! of every hub-family overlay (Baka Fighter, dance, fishing, slot machine and
//! the debug menu all carry byte-identical copies; the field overlay PROT 0897
//! carries the same code at the same VAs).
//!
//! The family is one dispatcher plus a set of small per-state handlers. The
//! dispatcher `FUN_801F159C` indexes the 52-entry table `PTR_FUN_801F33B4` by
//! the actor's `+0x50` word, so every routine here is one of that table's
//! slots - which is why they share a shape: read `+0x54` (the handler's own
//! sub-state), draw a panel through the glyph/sprite kernel, and on a confirm
//! press hand the actor back to the hub by stashing `+0x50` into the grid
//! actor and re-arming `+0x50` to the hub's own slot.
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
//! is the grid / tile-board actor ([`docs/subsystems/tile-board.md`]), whose
//! `+0x2E` is the "hand-back" sentinel, `+0x3E` the completion gate the
//! dispatcher polls and `+0x40` the stashed handler id. `0x8007B450` is the
//! field/tile-board busy flag, `0x8007B454` the text palette index,
//! `0x8007BB80` a suppression flag that blocks every confirm, `0x8007BB88` /
//! `0x8007BB98` the two cursor rows, `0x80084594` / `0x80084598` an entry
//! count and its parallel per-entry code array, `0x8008459C` party gold and
//! `0x800845A4` the casino coin bank.
//!
//! Read from `overlay_baka_fighter_801f{0adc,1138,159c,16c0,17d8,1890,1950,
//! 1a1c,1ab0,1b64,1d90,1e48,1fdc,20b0,2134}.txt` and
//! `overlay_baka_fighter_801f90dc.txt`; byte-identical dumps exist under the
//! sibling hub overlays. Documented in
//! [`docs/subsystems/minigame-baka-fighter.md`].
//!
//! Nothing here is reachable from a host root: the engine has no field
//! system-actor pool, so no code path produces an actor with a `+0x50`
//! handler id for [`hub_dispatch`] to index. Every entry point carries the
//! `NOT WIRED` disclosure naming that one blocker.

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

/// The grid / tile-board actor at `DAT_801C6EA4`, in the three fields this
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
    /// `DAT_80084594` - the entry count.
    pub entry_count: u8,
    /// `DAT_80084598..` - one code byte per entry.
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
    /// `FUN_801E5B4C(actor)` - the per-entry sub-draw between labels.
    EntrySubDraw,
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
    /// `DAT_8007BB88 = 0`.
    ClearCursorRow,
    /// `DAT_801F2C86` / `DAT_801F2C82` - the start panel's height and top.
    SizePanel { height: i16, top: i16 },
    /// `DAT_8007B469 = code` - the per-entry code the sub-draw reads.
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

/// Ceiling the coin exchange clamps the bank to.
pub const COIN_BANK_MAX: i32 = 0x0098_967F;
/// Gold per coin.
pub const GOLD_PER_COIN: i32 = 100;

/// The item id that means "money" rather than an inventory item.
pub const CAPTION_MONEY_ID: i32 = 0xFE;

// ---------------------------------------------------------------------------
// dispatcher
// ---------------------------------------------------------------------------

// NOT WIRED: the engine has no field system-actor pool, so nothing ever holds
// an actor whose `+0x50` is a `PTR_FUN_801F33B4` slot for this to index. The
// missing host is the field overlay's actor tick (the caller that walks the
// pool and invokes each actor's `+0x50` handler); until that exists this
// family has no entry point.
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
/// `handler` is the caller's view of `PTR_FUN_801F33B4[actor.state]`.
pub fn hub_dispatch(
    actor: &mut HubActor,
    env: &HubEnv,
    grid: &HubGrid,
    handler: impl FnOnce(&mut HubActor) -> HubFrame,
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
    let inner = handler(actor);
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

// NOT WIRED: same blocker as [`hub_dispatch`] - this is handler slot `0x25` of
// `PTR_FUN_801F33B4` and only that dispatcher reaches it.
/// PORT: FUN_801f0adc - the coin-exchange head plus its five-slot sub-dispatch.
///
/// The head converts party gold into buyable coins at [`GOLD_PER_COIN`] gold
/// each (a signed divide truncating toward zero, spelled in retail as the
/// `0x51EB851F` reciprocal multiply plus the sign fixup), publishes it to
/// `DAT_8007BB90`, and clamps it so the bank cannot pass [`COIN_BANK_MAX`].
/// It then tail-jumps the sub-handler `[actor+0x54]` out of the five-entry
/// table at `0x801CF734` - the handlers share this frame and fall into its
/// epilogue, which is why the dumped body is discontiguous - and finishes on
/// the draw pump.
pub fn coin_exchange(actor: &mut HubActor, env: &HubEnv) -> HubFrame {
    let mut out = HubFrame::default();
    out.act(HubAction::SetCoinAmount(coin_exchange_amount(
        env.gold,
        env.coin_bank,
    )));
    // The five sub-handlers live behind `jr v0` and are separate rows.
    let _slot = coin_exchange_slot(actor.sub);
    out.act(HubAction::DrawPump);
    out
}

/// The clamped coin amount `FUN_801F0ADC` publishes to `DAT_8007BB90`.
///
/// PORT: FUN_801f0adc (`0x801F0AE8..0x801F0B44`)
///
/// NOT WIRED: the arithmetic half of [`coin_exchange`], which is itself only
/// reachable through [`hub_dispatch`] - same blocker.
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

// NOT WIRED: same blocker as [`hub_dispatch`].
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

// NOT WIRED: same blocker as [`hub_dispatch`].
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

// NOT WIRED: same blocker as [`hub_dispatch`].
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

// NOT WIRED: same blocker as [`hub_dispatch`].
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

// NOT WIRED: same blocker as [`hub_dispatch`].
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

// NOT WIRED: same blocker as [`hub_dispatch`].
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

// NOT WIRED: same blocker as [`hub_dispatch`].
/// PORT: FUN_801f16c0 - the stacked per-entry label list.
///
/// Walks the `DAT_80084594` entries, publishing each entry's code byte to
/// `DAT_8007B469` first because the per-entry sub-draw `FUN_801E5B4C` reads
/// it. Codes at or above `3` draw nothing at all, but still cost a loop step.
/// Each drawn entry prints its label at the running `y`, advances `y` by
/// `0x0D` for the sub-draw and by a further `0x2A` afterwards; the actor's own
/// `+0x0C` is restored at the end, as is the previous entry code.
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
            out.draw(HubDraw::EntrySubDraw);
            actor.y = actor.y.wrapping_add(0x2A);
        }
    }
    actor.y = saved_y;
    out
}

// NOT WIRED: same blocker as [`hub_dispatch`].
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

// NOT WIRED: same blocker as [`hub_dispatch`].
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

// NOT WIRED: same blocker as [`hub_dispatch`].
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

// NOT WIRED: same blocker as [`hub_dispatch`].
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

// NOT WIRED: same blocker as [`hub_dispatch`].
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

// NOT WIRED: same blocker as [`hub_dispatch`].
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

// NOT WIRED: same blocker as [`hub_dispatch`].
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
            ..HubEnv::default()
        }
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
        let grid = HubGrid::default();
        let mut e = env();
        e.submode = 2;
        let f = hub_dispatch(&mut a, &e, &grid, |_| HubFrame::default());
        assert!(f.draws.is_empty() && f.actions.is_empty());
        assert_eq!(a.flags, 0);
    }

    #[test]
    fn dispatcher_retires_on_the_pad_latch_without_running_the_handler() {
        let mut a = HubActor::default();
        let grid = HubGrid::default();
        let mut e = env();
        e.pad_latch = PAD_LATCH_SUSPEND;
        let mut ran = false;
        let f = hub_dispatch(&mut a, &e, &grid, |_| {
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
        let grid = HubGrid::default();
        let mut e = env();
        e.board_flag = 0;
        let f = hub_dispatch(&mut a, &e, &grid, |_| HubFrame::default());
        assert!(f.actions.contains(&HubAction::ClearBusyBit));
        e.board_flag = 1;
        let mut a = HubActor::default();
        let f = hub_dispatch(&mut a, &e, &grid, |_| HubFrame::default());
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
