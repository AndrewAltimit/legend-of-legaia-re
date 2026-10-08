//! Retail-model **Arts command input**: the per-press directional entry
//! session a party member's Arts command opens in battle.
//!
//! Retail flow (`FUN_801D0748` state `0x50` gauge-input arm, accounting in
//! `FUN_801D388C` case `9` / `0xB`; the Muscle Dome runs the same states
//! verbatim - `docs/subsystems/minigame-muscle-dome.md` § Arts command
//! input): each d-pad press appends one directional command to the acting
//! actor's buffer (`actor+0x1DF`) and debits the command's per-(character,
//! weapon) AP cost (`DAT_801C9360[char][cmd] + 0x74`) from the turn pool
//! (`ctx+0x6DC`, seeded from the actor's AGL `+0x154`).
//!
//! Entry leaves state `0x50` three ways, and the pad drives two of them.
//! It **ends by itself** the moment no command is affordable - the
//! affordability scan at `801d2054`..`801d2078` walks the four costs at
//! `ctx+0x14` and `801d208c bne s0,zero,801d20ac` takes `0x5A` when none
//! fits. The **confirm** mask `_DAT_800846D0` ends it early
//! (`801d20a0 and v0,s2,v0`), and the **cancel** mask `_DAT_800846D4`
//! either restarts the entry or leaves it; both are gated on the committed
//! count `ctx+0x19` and both are detailed at their sites in
//! [`ArtsCommandInputSession::input`].
//!
//! The review screen's next press picks the art's target and commits it.
//! **Begin | Reselect** (`0x6E`) is not this session's: retail raises it once
//! for the whole party, after the last member that can act has committed
//! (`crate::battle_input::CommandPhase::CommitConfirm`), and only a party of
//! one reaches it straight off an arts entry. **Triangle** cycles the learned
//! arts list (closed -> page 1 -> ... -> closed) and is inert when the
//! character has no learned art.
//!
//! The four accepted presses are **d-pad directions**. `FUN_801D0748` tests
//! them against `s2 = _DAT_8007B874 | _DAT_8007B938`, which is the
//! **packed** pad word (`crate::world_map_panel_host::packed_pad` - the
//! byte halves are swapped against the raw BIOS word), so the literals
//! `0x8000 / 0x1000 / 0x4000 / 0x2000` at `801d1e60`..`801d1f38` are Left /
//! Up / Down / Right and *not* the face buttons a raw reading makes them.
//!
//! The entered sequence resolves to arts through the matcher family in
//! `legaia-art`: an exact Miracle string replaces the whole queue, a
//! recognized named-art sequence ending on a Super combination replaces
//! the tail, and otherwise each recognized named art contributes its
//! record's strike profile while unmatched directions stay plain swings
//! ([`resolve_entered_commands`]).
//!
//! **Disclosed divergences from retail** (see
//! `docs/subsystems/arts-command-gauge.md`):
//! - **Disc-free fallback, not a behavioural divergence.** The pool seeds
//!   from the roster record's AGL exactly as retail does
//!   (`801d3a28 lhu v0,0x154(v0)` -> `801d3a30 sh v0,0x6(s6)`); it falls
//!   back to [`DEFAULT_POOL`] (the pinned input bar's 100-AP span) only
//!   when no roster is loaded, which retail never is.
//! - **Charge point, not amount.** The art body *is* charged from the
//!   Spirit gauge (`World::charge_art_spirit`, off
//!   [`crate::ap_gauge::arts_turn_spirit_cost`]), but at the commit
//!   rather than through retail's accumulator `actor[+0x224]`, which the
//!   battle-action cleanup arm spends once at `0x801E5D74`. Same turn,
//!   same total; the difference is only observable by reading `+0x170`
//!   mid-action. The actor's `0x800` halving flag has no carrier here, so
//!   the full-price multiplier is the one that runs.
//!
//! Two entries previously disclosed here as engine conveniences - "Cross
//! confirms early" and "Circle backs out" - were **not** divergences: both
//! are retail behaviour, driven by the configurable confirm / cancel masks,
//! and the sites are cited in [`ArtsCommandInputSession::input`]. The claim
//! they rested on ("retail entry only auto-ends", "retail's Arts command
//! cannot be backed out of at all") is falsified by the disassembly.
//!
//! PORT: FUN_801D0748 (state 0x50 / 0x5A flow)
//! PORT: FUN_801D388C (case 9 cost read + case 0xB pool debit)

use crate::target_picker::{
    CursorRow, PickerInput, PickerOutcome, SlotState, TargetKind, TargetPickerSession,
};
use legaia_art::power::PowerByte;
use legaia_art::queue::Command;
use legaia_art::{ArtRecord, EnemyEffect};

/// The `+0x16E` Rot limb bit that disables an arts direction.
///
/// `FUN_801D0748`'s entry state tests each pressed direction against the
/// acting actor's status word before it records the pick (`ctx+9`): Left
/// against `0x08` (`0x801D1E8C`), Up and Down both against `0x20`
/// (`0x801D1ED4` / `0x801D1F1C`), Right against `0x10` (`0x801D1F64`). A
/// blocked press records nothing and fires cue [`ROT_REFUSED_CUE`] instead.
/// The same three bits choose where the entry frame draws its crosses
/// (`0x801D1DA8..0x801D1E54`: one beside the left label, one beside the right
/// label, and two - above and below - for `0x20`).
///
/// PORT: FUN_801D0748 (the Rot direction gate, `0x801D1E60..0x801D1F7C`)
pub fn rot_blocks(status: u16, cmd: Command) -> bool {
    let bit = match cmd {
        Command::Left => 0x08,
        Command::Right => 0x10,
        Command::Up | Command::Down => 0x20,
    };
    status & bit != 0
}

/// The cue a Rot-blocked direction fires (`FUN_8004FCC8(0x23)`).
pub const ROT_REFUSED_CUE: u16 = 0x23;

/// Base per-press cost of a favored-class direction command (`0x1E`).
pub const FAVORED_COST: u16 = 0x1E;
/// Disc-free fallback AP pool - the pinned input bar maps `x 0..128` at a
/// 100-AP pool, so 100 keeps the bar geometry meaningful without an AGL.
pub const DEFAULT_POOL: u16 = 100;
/// Rows per page of the Triangle arts-list window (retail draws five,
/// `y = 36 + 30n`).
pub const ARTS_LIST_ROWS_PER_PAGE: usize = 5;

/// Per-frame, edge-triggered pad bundle for the input session.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ArtsCommandPad {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    /// Confirm (Cross).
    pub cross: bool,
    /// Cancel / back (Circle).
    pub circle: bool,
    /// Arts-list toggle (Triangle).
    pub triangle: bool,
}

/// Sub-phase of the input session.
#[derive(Debug, Clone)]
pub enum ArtsInputPhase {
    /// Accepting directional presses (retail `0x50`).
    Entering,
    /// The committed bar review (retail `0x5A`) - any press but the cancel
    /// opens the target picker.
    Review,
    /// Picking the art's target (retail's `0x5A` target cursor, with the
    /// committed bar still up).
    Targeting { picker: TargetPickerSession },
    /// Resolved: run the entered sequence against the target.
    Confirmed {
        target_row: CursorRow,
        target_slot: u8,
    },
    /// Backed out with an empty buffer - the command menu reopens.
    Aborted,
}

/// One party member's per-press arts entry, driven a frame at a time.
#[derive(Debug, Clone)]
pub struct ArtsCommandInputSession {
    /// Actor-table index of the acting party member.
    pub actor: u8,
    /// Party-row index (0..=2).
    pub party_slot: u8,
    /// Seeded AP pool (retail `ctx+0x6DC` seed = actor AGL `+0x154`).
    pub pool_max: u16,
    /// Remaining AP.
    pub pool: u16,
    /// Per-direction press cost, indexed `Command::as_byte() - 1`
    /// (Left, Right, Down, Up). Left is action `0x0C` - the **arm**
    /// command whose cost carries the weapon-specialty byte; the other
    /// three stay at [`FAVORED_COST`] in retail.
    pub costs: [u16; 4],
    /// Entered command bytes (`Command::as_byte()` values, in order).
    pub buffer: Vec<u8>,
    /// Cost paid per entered command (drives the pennant x seats:
    /// slot `n` sits at `7 + spent-before`).
    pub spent: Vec<u16>,
    /// Pages available to the Triangle arts list (0 = the toggle is
    /// inert, retail's no-learned-art case).
    pub list_pages: u8,
    /// Open arts-list page (`None` = closed).
    pub list_page: Option<u8>,
    /// The character's learned-arts count (`record[+0x185]`, the byte
    /// `FUN_801D3748` reads at `0x80084140 + char * 0x414 + 0x74D`), when the
    /// host supplies it. `Some` routes Triangle through the retail pager
    /// ([`arts_list_pager`]); `None` keeps the plain page cycle over
    /// [`Self::list_pages`].
    pub list_rows: Option<u8>,
    /// The character's saved auto command string, loaded when the entry
    /// opened (`FUN_801DA34C` at `0x801D1734`, beside the `0x50` phase
    /// store), as `Command::as_byte()` values. Empty when the record held
    /// none. The first accepted direction press wipes it - `FUN_801D388C`
    /// case `0xB` zeroes the sixteen queue bytes when the committed count
    /// is `0` (`0x801D3BE4..0x801D3C24`) - and so does the review's cancel.
    pub preseed: Vec<u8>,
    /// AP each [`Self::preseed`] command would cost, parallel to it - the
    /// pennant seats of the preseeded bar ([`Self::bar_spent`]).
    pub preseed_spent: Vec<u16>,
    /// Set when the confirm on an empty entry took [`Self::preseed`] as the
    /// turn's string (`0x801D1FA0..0x801D2044`). The resolved session then
    /// commits the preseed rather than [`Self::buffer`].
    pub replay: bool,
    pub phase: ArtsInputPhase,
}

/// Outcome of a resolved session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtsInputResolution {
    /// Begin confirmed against a target: execute the entered sequence.
    Confirmed {
        target_row: CursorRow,
        target_slot: u8,
    },
    /// Backed out; the live loop reopens the command menu.
    Aborted,
}

/// One Triangle press on the battle arts list, as `FUN_801D3748` steps it:
/// the page a press leaves open (`None` = closed), given the page open before
/// it and the character's learned-arts count.
///
/// Retail keeps the page as a row offset in `_DAT_8007B458` and the open bit
/// in `0x801F4E09`. With no learned art the press does nothing
/// (`beqz v1` at `0x801D3794`). A closed list opens on the first page
/// (`0x801D3848..0x801D3878`). An open one steps to the second page only from
/// the first and only with six or more arts (`sltiu v1, 6`), to the third
/// only from the second and only with eleven or more (`sltiu v0, 0xB`), and
/// any other press closes it - so retail shows at most three pages of five,
/// however many arts are learned.
///
/// PORT: overlay_battle_action_0898_801d3748
pub fn arts_list_pager(open: Option<u8>, learned: u8) -> Option<u8> {
    if learned == 0 {
        return open;
    }
    match open {
        None => Some(0),
        Some(0) if learned >= 6 => Some(1),
        Some(1) if learned >= 11 => Some(2),
        Some(_) => None,
    }
}

impl ArtsCommandInputSession {
    /// The entry with the retail pager over `learned` arts
    /// ([`Self::list_rows`]); the page count follows the pager's three-page
    /// cap.
    pub fn with_list_rows(mut self, learned: u8) -> Self {
        self.list_rows = Some(learned);
        self.list_pages = match learned {
            0 => 0,
            1..=5 => 1,
            6..=10 => 2,
            _ => 3,
        };
        self
    }

    /// Open a fresh entry. `pool` seeds both the live pool and its
    /// maximum; `costs` are the four per-direction press costs
    /// (Left / Right / Down / Up); `list_pages` sizes the Triangle list.
    pub fn new(actor: u8, party_slot: u8, pool: u16, costs: [u16; 4], list_pages: u8) -> Self {
        Self {
            actor,
            party_slot,
            pool_max: pool,
            pool,
            costs,
            buffer: Vec::new(),
            spent: Vec::new(),
            list_pages,
            list_page: None,
            list_rows: None,
            preseed: Vec::new(),
            preseed_spent: Vec::new(),
            replay: false,
            phase: ArtsInputPhase::Entering,
        }
    }

    /// The entry with the character's auto command string preseeded
    /// ([`Self::preseed`]).
    ///
    /// The gauge build that opens the entry (`FUN_801D388C` case `0x2C`,
    /// `0x801D4DC8..0x801D5070`) walks the preseeded window when its first
    /// byte is non-zero: from a pool seeded off `+0x154` it draws one
    /// pennant per command (window `0x20 + i` through `FUN_801D8DE8`, its
    /// chip's word `+ 6`, seated `cost` further right each time) and debits
    /// the command's cost, and at the first command the remaining pool
    /// cannot pay (`slt` at `0x801D4E90`) it zeroes that byte
    /// (`sb zero,0x1df(a1)` at `0x801D4DC4`) and stops - so the string
    /// keeps only the prefix one full pool affords, for the bar and for a
    /// replay alike. The pool and the pennant seat are then reset to the
    /// full gauge (`0x801D503C..0x801D5068`): drawing the preseed charges
    /// nothing.
    pub fn with_preseed(mut self, mut preseed: Vec<u8>) -> Self {
        let mut pool = self.pool_max;
        let mut spent = Vec::with_capacity(preseed.len());
        for &b in &preseed {
            let cost = Command::from_byte(b).map_or(u16::MAX, |c| self.cost_of(c));
            if cost > pool {
                break;
            }
            pool -= cost;
            spent.push(cost);
        }
        preseed.truncate(spent.len());
        self.preseed = preseed;
        self.preseed_spent = spent;
        self
    }

    /// Drop the preseeded string (and its pennants).
    fn clear_preseed(&mut self) {
        self.preseed.clear();
        self.preseed_spent.clear();
    }

    /// The commands the pennant bar shows: the entered buffer, or - while
    /// nothing is entered - the preseeded string the gauge build drew. The
    /// first press closes those pennants with the wipe (case `0xB`'s count-`0`
    /// arm retires the bar's text windows, kinds `5..=0xD`, at
    /// `0x801D3C2C..0x801D3C94`).
    pub fn bar_commands(&self) -> &[u8] {
        if self.buffer.is_empty() {
            &self.preseed
        } else {
            &self.buffer
        }
    }

    /// AP per [`Self::bar_commands`] entry - the pennant seats and widths.
    pub fn bar_spent(&self) -> &[u16] {
        if self.buffer.is_empty() {
            &self.preseed_spent
        } else {
            &self.spent
        }
    }

    /// The command string a resolved session commits: the replayed preseed
    /// when the entry was confirmed empty, else the entered buffer.
    pub fn committed_string(&self) -> &[u8] {
        if self.replay {
            &self.preseed
        } else {
            &self.buffer
        }
    }

    /// `true` while at least one direction command is still affordable.
    /// The moment this goes false the entry auto-ends (retail
    /// `0x50 -> 0x5A` on the exhausting press).
    pub fn any_affordable(&self) -> bool {
        self.costs.iter().any(|&c| c <= self.pool)
    }

    /// Cost of one direction press (the retail `+0x74` byte for that
    /// command).
    pub fn cost_of(&self, cmd: Command) -> u16 {
        self.costs[(cmd.as_byte() - 1) as usize]
    }

    /// The resolved execution / abort, or `None` while still entering.
    pub fn resolved(&self) -> Option<ArtsInputResolution> {
        match &self.phase {
            ArtsInputPhase::Confirmed {
                target_row,
                target_slot,
            } => Some(ArtsInputResolution::Confirmed {
                target_row: *target_row,
                target_slot: *target_slot,
            }),
            ArtsInputPhase::Aborted => Some(ArtsInputResolution::Aborted),
            _ => None,
        }
    }

    /// The active target picker, while one is open.
    pub fn picker(&self) -> Option<&TargetPickerSession> {
        match &self.phase {
            ArtsInputPhase::Targeting { picker } => Some(picker),
            _ => None,
        }
    }

    /// Advance one frame. `party` / `monsters` describe slot occupancy for
    /// the Begin target picker. A no-op once the session has resolved.
    pub fn input(&mut self, ev: ArtsCommandPad, party: [SlotState; 3], monsters: [SlotState; 5]) {
        // Triangle cycles the learned-arts list in the entry phases
        // (closed -> page 0 -> .. -> last page -> closed); inert with no
        // pages, matching retail's no-learned-art case.
        if ev.triangle
            && self.list_pages > 0
            && matches!(
                self.phase,
                ArtsInputPhase::Entering | ArtsInputPhase::Review
            )
        {
            self.list_page = match self.list_rows {
                Some(rows) => arts_list_pager(self.list_page, rows),
                None => match self.list_page {
                    None => Some(0),
                    Some(p) if p + 1 < self.list_pages => Some(p + 1),
                    Some(_) => None,
                },
            };
            return;
        }
        match std::mem::replace(&mut self.phase, ArtsInputPhase::Aborted) {
            ArtsInputPhase::Entering => {
                let dir = if ev.up {
                    Some(Command::Up)
                } else if ev.down {
                    Some(Command::Down)
                } else if ev.left {
                    Some(Command::Left)
                } else if ev.right {
                    Some(Command::Right)
                } else {
                    None
                };
                if let Some(cmd) = dir {
                    // The first press of an entry wipes the preseeded string
                    // (case `0xB`'s count-`0` arm zeroes all sixteen queue
                    // bytes before the affordability test).
                    if self.buffer.is_empty() {
                        self.clear_preseed();
                    }
                    let cost = self.cost_of(cmd);
                    if cost <= self.pool {
                        self.pool -= cost;
                        self.buffer.push(cmd.as_byte());
                        self.spent.push(cost);
                    }
                    // Auto-end on the exhausting press (retail 0x50 ->
                    // 0x5A: entry ends by itself, no confirm).
                    self.phase = if self.any_affordable() {
                        ArtsInputPhase::Entering
                    } else {
                        ArtsInputPhase::Review
                    };
                } else if ev.cross && self.buffer.is_empty() && !self.preseed.is_empty() {
                    // **Replay.** With nothing entered and a preseeded string
                    // in the window, the confirm mask takes that string as
                    // the turn (`0x801D1FA0..0x801D2044`): gated on the
                    // staging byte `DAT_8007BD04`, a zero committed count and
                    // a non-zero `+0x1DF[0]`, it measures the string, stores
                    // its length as the count (`0x801D2028`) and enters
                    // `0x5A` through `FUN_801D388C` case `0xC`, exactly as a
                    // typed confirm does. **No press is charged**: case `0xB`
                    // never runs, so the entry pool `ctx+0x6DC` is neither
                    // debited nor tested against the string's cost.
                    self.replay = true;
                    self.phase = ArtsInputPhase::Review;
                } else if ev.cross && !self.buffer.is_empty() {
                    // Retail's configurable confirm mask `_DAT_800846D0`
                    // ends the entry, gated on the committed count
                    // `ctx+0x19`: `801d207c lbu v0,0x8(s1)` /
                    // `801d2084 beq v0,zero,..` skips the mask test with an
                    // empty buffer, and `801d20a0 and v0,s2,v0` /
                    // `801d20ac sb v0,0x0(s3)` writes state `0x5A`.
                    self.phase = ArtsInputPhase::Review;
                } else if ev.circle {
                    // Retail's cancel mask `_DAT_800846D4`
                    // (`801d20ec lw v0,0x46d4(v0)` / `801d20f4 and v0,s2,v0`)
                    // forks on the same committed count at
                    // `801d210c lbu v0,0x8(s1)`:
                    //
                    // - buffer **non-empty** (`801d2114 bne v0,zero,801d21a8`)
                    //   calls `FUN_801D388C` case `0x26`, which wipes all
                    //   sixteen queue bytes (`801d52d4 sb zero,0x1df(v0)`
                    //   under `801d52d8 sltiu v0,s3,0x10`), re-seeds the pool
                    //   from the actor's AGL (`801d535c lhu v0,0x154(v0)` ->
                    //   `801d5364 sh v0,0x6(s6)`) and zeros the count
                    //   (`801d536c sb zero,0x8(s4)`) - the entry restarts
                    //   clean and `ctx+0x06` is never written, so the flow
                    //   stays in `0x50`.
                    // - buffer **empty** leaves the entry entirely, to the
                    //   attack-mode prompt `0x78` (`801d219c`/`801d21a0`) or
                    //   the command ring `0x28` (`801d218c`) when
                    //   `_DAT_800846C4` is set.
                    if self.buffer.is_empty() {
                        self.phase = ArtsInputPhase::Aborted;
                    } else {
                        self.buffer.clear();
                        self.spent.clear();
                        self.pool = self.pool_max;
                        self.phase = ArtsInputPhase::Entering;
                    }
                } else {
                    self.phase = ArtsInputPhase::Entering;
                }
            }
            ArtsInputPhase::Review => {
                if ev.circle {
                    // Retail's `0x5A` cancel (`0x801D2304..0x801D23DC`) wipes
                    // the sixteen queue bytes and, for an entry opened from
                    // the arts input, returns to it (`0x50`, case `0xF`).
                    // The wipe takes a replayed preseed with it.
                    self.clear_preseed();
                    self.replay = false;
                    self.buffer.clear();
                    self.spent.clear();
                    self.pool = self.pool_max;
                    self.phase = ArtsInputPhase::Entering;
                } else if ev.cross || ev.up || ev.down || ev.left || ev.right {
                    // Any other press opens the target picker; the commit it
                    // resolves to walks the ring on, and the last member's
                    // commit raises the party's Begin | Reselect.
                    let picker = TargetPickerSession::new(
                        TargetKind::SingleEnemy,
                        self.party_slot,
                        party,
                        monsters,
                    );
                    self.phase = match picker.outcome() {
                        Some(PickerOutcome::Single { slot, row }) => ArtsInputPhase::Confirmed {
                            target_row: row,
                            target_slot: slot,
                        },
                        Some(PickerOutcome::Sweep { row }) => ArtsInputPhase::Confirmed {
                            target_row: row,
                            target_slot: 0,
                        },
                        Some(PickerOutcome::NoCandidates) => ArtsInputPhase::Aborted,
                        _ => ArtsInputPhase::Targeting { picker },
                    };
                } else {
                    self.phase = ArtsInputPhase::Review;
                }
            }
            ArtsInputPhase::Targeting { mut picker } => {
                picker.input(PickerInput {
                    up: ev.up,
                    down: ev.down,
                    left: ev.left,
                    right: ev.right,
                    cross: ev.cross,
                    circle: ev.circle,
                });
                self.phase = match picker.outcome() {
                    Some(PickerOutcome::Single { slot, row }) => ArtsInputPhase::Confirmed {
                        target_row: row,
                        target_slot: slot,
                    },
                    Some(PickerOutcome::Sweep { row }) => ArtsInputPhase::Confirmed {
                        target_row: row,
                        target_slot: 0,
                    },
                    Some(PickerOutcome::Cancelled) => ArtsInputPhase::Review,
                    Some(PickerOutcome::NoCandidates) => ArtsInputPhase::Aborted,
                    None => ArtsInputPhase::Targeting { picker },
                };
            }
            other => self.phase = other,
        }
    }
}

/// Which surface the input session is showing, flattened for the hosts
/// (the live [`ArtsInputPhase`] carries a target picker the chrome
/// builders have no use for).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtsInputScreen {
    /// Direction chips + D-pad live; presses append.
    Entering,
    /// Committed-bar review - the chips are gone, the bar stays.
    Review,
    /// Picking the art's target; the bar stays up behind the picker.
    Targeting,
}

impl From<&ArtsInputPhase> for ArtsInputScreen {
    fn from(p: &ArtsInputPhase) -> Self {
        match p {
            ArtsInputPhase::Entering => Self::Entering,
            ArtsInputPhase::Review => Self::Review,
            ArtsInputPhase::Targeting { .. } => Self::Targeting,
            // A resolved session is torn down the same frame; nothing
            // draws from it.
            _ => Self::Review,
        }
    }
}

/// Chip-label icon ids of the arts-entry direction chips: an index into the
/// SCUS window-icon table `0x800732A4` (12-byte records, sheet `(u, v)` at
/// `+4`) that every chip's window record carries at `+0x0E` / `+0x0F`.
/// `0x0C` = `RaSeru`, `0x0D` = `Arms`, `0x0E` = `Right`, `0x0F` = `Left`,
/// `0x10` = `High`, `0x11` = `Low`; a committed pennant takes its chip's id
/// `+ 6` (`0x801D3D1C..0x801D3D38`), the same word on the pennant variant.
pub mod chip_icon {
    pub const RASERU: u8 = 0x0C;
    pub const ARMS: u8 = 0x0D;
    pub const RIGHT: u8 = 0x0E;
    pub const LEFT: u8 = 0x0F;
    pub const HIGH: u8 = 0x10;
    pub const LOW: u8 = 0x11;
}

/// The plain direction words (Left, Right, Down, Up order) - what the chips
/// read for a fighter with neither arm slot filled.
pub const PLAIN_CHIP_ICONS: [u8; 4] = [
    chip_icon::LEFT,
    chip_icon::RIGHT,
    chip_icon::LOW,
    chip_icon::HIGH,
];

/// The retail chip words for one caster, in Command-byte order (Left,
/// Right, Down, Up).
///
/// The entry opener's per-seat loop (`FUN_801D388C`, `0x801D3A48..0x801D3BCC`)
/// stamps each chip record's icon from the seat table `DAT_801F4B94 =
/// [0x0D, 0x10, 0x11, 0x0C]` (seats Left, High, Low, Right): Left reads
/// `Arms`, Right `RaSeru`. Character id `2` (Noa) swaps the two arm seats,
/// because her record carries the Ra-Seru in equipment index 2 and the
/// weapon in index 3. An arm whose equipment byte is empty - index 2
/// (`+0x198`) for the Left seat, index 3 (`+0x199`) for the Right, keyed by
/// the **unswapped** seat command `DAT_801F4B8C` - instead reads the plain
/// direction word, the seat's own id `+ 2` (`Left` / `Right`).
///
/// `equip` is the record's equipment bytes from `+0x196`.
///
/// PORT: FUN_801D388C (arts-entry chip-icon seat loop, `0x801D3A48..0x801D3B08`)
pub fn retail_chip_icons(character_id: u8, equip: &[u8]) -> [u8; 4] {
    let swap = character_id == 2;
    let filled = |i: usize| equip.get(i).is_some_and(|&b| b != 0);
    let left = if !filled(2) {
        chip_icon::LEFT
    } else if swap {
        chip_icon::RASERU
    } else {
        chip_icon::ARMS
    };
    let right = if !filled(3) {
        chip_icon::RIGHT
    } else if swap {
        chip_icon::ARMS
    } else {
        chip_icon::RASERU
    };
    [left, right, chip_icon::LOW, chip_icon::HIGH]
}

/// Renderer-agnostic snapshot of an open input session - everything the
/// pinned chrome needs and nothing else. Built by
/// `World::arts_input_view`, consumed by
/// `legaia_engine_ui::arts_input`.
#[derive(Debug, Clone, Copy)]
pub struct ArtsInputView<'a> {
    /// Entered command bytes, in order (`Command::as_byte()` values).
    pub buffer: &'a [u8],
    /// AP paid per entered command - the pennant seat law is
    /// `x = 7 + sum(spent[..n])`.
    pub spent: &'a [u16],
    /// The commands the pennant bar draws - [`Self::buffer`], or the
    /// preseeded auto command string while nothing is entered
    /// ([`ArtsCommandInputSession::bar_commands`]).
    pub pennants: &'a [u8],
    /// AP per [`Self::pennants`] entry ([`ArtsCommandInputSession::bar_spent`]).
    pub pennant_spent: &'a [u16],
    /// Remaining / seeded entry pool (drives the bar length).
    pub pool: u16,
    pub pool_max: u16,
    /// Per-direction press costs (Left, Right, Down, Up).
    pub costs: [u16; 4],
    /// Per-direction chip-label icon ids (Left, Right, Down, Up) -
    /// [`retail_chip_icons`]; the pennants reuse their chip's word.
    pub chip_icons: [u8; 4],
    /// Value the right-hand AP plate shows. Retail reads the caster's
    /// Spirit gauge here and it does **not** drain during entry.
    pub plate_value: u8,
    /// Open Triangle arts-list page (`None` = closed).
    pub list_page: Option<u8>,
    /// Pages the Triangle list can cycle (`0` = the toggle is inert).
    pub list_pages: u8,
    pub phase: ArtsInputScreen,
    /// The caster's `+0x16E` status word. Its Rot limb bits (`0x38`) pick
    /// which chips wear the Rot stamp during entry ([`rot_blocks`] refuses
    /// the same directions).
    pub status: u16,
}

impl ArtsInputView<'_> {
    /// `true` while the four direction chips + D-pad glyph are up (retail
    /// draws them only in the entry phase).
    pub fn chips_visible(&self) -> bool {
        self.phase == ArtsInputScreen::Entering
    }
}

/// The strike profile an entered command sequence resolves to.
#[derive(Debug, Clone, Default)]
pub struct ResolvedEntry {
    /// Per-strike power bytes, in strike order.
    pub power: Vec<PowerByte>,
    /// Status effect the resolved arts inflict (first non-`None` among
    /// the matched records).
    pub enemy_effect: EnemyEffect,
    /// Shout-cue key: the first matched art's action constant.
    pub action: Option<legaia_art::ActionConstant>,
    /// Recognized named arts, in performed order.
    pub matched: Vec<legaia_art::ActionConstant>,
}

/// Synthetic tier-0 (x12) UDF hit for an unmatched high/side swing.
const SYNTH_UDF_X12: u8 = 0x16;
/// Synthetic tier-0 (x12) LDF hit for an unmatched low swing.
const SYNTH_LDF_X12: u8 = 0x1B;

/// Resolve an entered command sequence against a caster's art catalog:
/// greedy longest-match left to right (the retail queue-builder's
/// recognition order - REF: FUN_801EED1C; same walk as
/// [`legaia_art::recognize_art_sequence`], kept local so unmatched
/// positions are visible). Each matched art contributes its record's
/// damaging power bytes in place; an unmatched direction stays a plain
/// swing (one synthetic tier-0 hit, Down low / others high).
///
/// Miracle / Super replacement is the caller's job (the World holds the
/// finisher profiles); this is the plain path.
pub fn resolve_entered_commands(
    records: &[(legaia_art::ActionConstant, ArtRecord)],
    buffer: &[u8],
) -> ResolvedEntry {
    let commands: Vec<Command> = buffer
        .iter()
        .filter_map(|&b| Command::from_byte(b))
        .collect();
    let mut out = ResolvedEntry::default();
    let mut i = 0usize;
    while i < commands.len() {
        let mut best: Option<(usize, usize)> = None; // (record idx, len)
        for (ri, (_, rec)) in records.iter().enumerate() {
            if rec.commands.is_empty() || !commands[i..].starts_with(&rec.commands) {
                continue;
            }
            if best.is_none_or(|(_, len)| len < rec.commands.len()) {
                best = Some((ri, rec.commands.len()));
            }
        }
        match best {
            Some((ri, len)) => {
                let (action, rec) = &records[ri];
                out.matched.push(*action);
                if out.action.is_none() {
                    out.action = Some(*action);
                }
                if out.enemy_effect == EnemyEffect::None {
                    out.enemy_effect = rec.enemy_effect;
                }
                out.power
                    .extend(rec.power.iter().copied().filter(|p| p.is_damage()));
                i += len;
            }
            None => {
                // Plain swing: one synthetic tier-0 hit.
                let byte = match commands[i] {
                    Command::Down => SYNTH_LDF_X12,
                    _ => SYNTH_UDF_X12,
                };
                out.power.push(PowerByte::from_byte(byte));
                i += 1;
            }
        }
    }
    out.power
        .truncate(crate::battle_arts::MAX_ART_HITS as usize);
    if out.power.is_empty() {
        out.power.push(PowerByte::from_byte(SYNTH_UDF_X12));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The seat loop's table `[0x0D, 0x10, 0x11, 0x0C]` plus Noa's swap and
    /// the empty-arm `+ 2`: Vahn / Gala read `Arms | RaSeru`, Noa
    /// `RaSeru | Arms` - the `arts_bar_*` captures - and an empty arm its
    /// direction word.
    #[test]
    fn chip_icons_follow_the_seat_table_the_noa_swap_and_empty_arms() {
        use chip_icon::*;
        let full = [0, 0, 1, 1, 0];
        assert_eq!(retail_chip_icons(1, &full), [ARMS, RASERU, LOW, HIGH]);
        assert_eq!(retail_chip_icons(3, &full), [ARMS, RASERU, LOW, HIGH]);
        assert_eq!(retail_chip_icons(2, &full), [RASERU, ARMS, LOW, HIGH]);
        // The empty check keys on the unswapped seat: index 2 for Left.
        assert_eq!(
            retail_chip_icons(1, &[0, 0, 0, 1]),
            [LEFT, RASERU, LOW, HIGH]
        );
        assert_eq!(
            retail_chip_icons(2, &[0, 0, 1, 0]),
            [RASERU, RIGHT, LOW, HIGH]
        );
        assert_eq!(retail_chip_icons(1, &[]), PLAIN_CHIP_ICONS);
    }

    #[test]
    fn rot_blocks_each_limb_s_directions() {
        assert!(rot_blocks(0x08, Command::Left));
        assert!(!rot_blocks(0x08, Command::Right));
        assert!(rot_blocks(0x10, Command::Right));
        assert!(rot_blocks(0x20, Command::Up) && rot_blocks(0x20, Command::Down));
        assert!(!rot_blocks(0x20, Command::Left));
        let all = 0x38;
        for c in [Command::Left, Command::Right, Command::Up, Command::Down] {
            assert!(rot_blocks(all, c));
            assert!(!rot_blocks(0x1007, c), "non-Rot bits gate nothing");
        }
    }
    use legaia_art::queue::ActionConstant;

    fn alive(present: bool) -> SlotState {
        SlotState::alive(present, true)
    }
    fn party3() -> [SlotState; 3] {
        [alive(true), alive(true), alive(true)]
    }
    fn one_monster() -> [SlotState; 5] {
        [
            alive(true),
            SlotState::default(),
            SlotState::default(),
            SlotState::default(),
            SlotState::default(),
        ]
    }
    fn press(b: &str) -> ArtsCommandPad {
        ArtsCommandPad {
            up: b == "U",
            down: b == "D",
            left: b == "L",
            right: b == "R",
            cross: b == "c",
            circle: b == "o",
            triangle: b == "t",
        }
    }
    fn rec(action: u8, cmds: &[Command], power: &[u8]) -> (ActionConstant, ArtRecord) {
        (
            ActionConstant::from_byte(action).unwrap(),
            ArtRecord {
                action: ActionConstant::from_byte(action).unwrap(),
                commands: cmds.to_vec(),
                anim_index: 0,
                anim_extra: vec![],
                name: None,
                power: power.iter().map(|&b| PowerByte::from_byte(b)).collect(),
                dmg_timing: vec![],
                effect_cues: Default::default(),
                hit_cues: vec![],
                identifier: 0,
                anim_speed: 0,
                enemy_effect: EnemyEffect::None,
                repeat_frames: Default::default(),
                background: 0,
                runtime_address: None,
            },
        )
    }

    #[test]
    fn press_appends_and_debits_per_command_cost() {
        // Off-class arm: Left costs 42, the others 30. Pool 120 leaves 48
        // after the two presses, so the entry is still live and the test
        // measures the debit rather than the auto-end.
        let mut s = ArtsCommandInputSession::new(0, 0, 120, [42, 30, 30, 30], 0);
        s.input(press("U"), party3(), one_monster());
        s.input(press("L"), party3(), one_monster());
        assert_eq!(s.buffer, vec![4, 1]);
        assert_eq!(s.spent, vec![30, 42], "each press debits its own cost");
        assert_eq!(s.pool, 48);
        assert!(matches!(s.phase, ArtsInputPhase::Entering));
    }

    #[test]
    fn entry_auto_ends_when_nothing_is_affordable() {
        // Pool 90 at flat cost 30: the third press exhausts the pool and
        // the entry ends by itself (retail 0x50 -> 0x5A, no confirm).
        let mut s = ArtsCommandInputSession::new(0, 0, 90, [30; 4], 0);
        s.input(press("U"), party3(), one_monster());
        s.input(press("D"), party3(), one_monster());
        assert!(matches!(s.phase, ArtsInputPhase::Entering));
        s.input(press("U"), party3(), one_monster());
        assert_eq!(s.buffer.len(), 3);
        assert_eq!(s.pool, 0);
        assert!(matches!(s.phase, ArtsInputPhase::Review), "auto-ended");
    }

    #[test]
    fn unaffordable_press_is_refused_and_pool_untouched() {
        // Pool 116, arm (Left) 42: two Lefts leave 32 - too little for a
        // third arm (42) but enough for a plain swing (30), so the entry
        // stays live and the unaffordable press is simply dropped.
        let mut s = ArtsCommandInputSession::new(0, 0, 116, [42, 30, 30, 30], 0);
        s.input(press("L"), party3(), one_monster());
        s.input(press("L"), party3(), one_monster());
        assert_eq!(s.pool, 32);
        assert!(matches!(s.phase, ArtsInputPhase::Entering));
        s.input(press("L"), party3(), one_monster());
        assert_eq!(s.buffer.len(), 2, "third arm press refused");
        assert_eq!(s.pool, 32, "a refused press does not debit");
        assert!(
            matches!(s.phase, ArtsInputPhase::Entering),
            "entry stays live"
        );
        // An affordable swing still lands, and exhausts the pool.
        s.input(press("U"), party3(), one_monster());
        assert_eq!(s.buffer.len(), 3);
        assert_eq!(s.pool, 2);
        assert!(matches!(s.phase, ArtsInputPhase::Review), "auto-ended");
    }

    #[test]
    fn circle_on_the_review_restarts_the_entry() {
        let mut s = ArtsCommandInputSession::new(0, 0, 60, [30; 4], 0);
        s.input(press("U"), party3(), one_monster());
        s.input(press("D"), party3(), one_monster());
        assert!(matches!(s.phase, ArtsInputPhase::Review));
        s.input(press("o"), party3(), one_monster());
        assert!(matches!(s.phase, ArtsInputPhase::Entering));
        assert!(s.buffer.is_empty());
        assert_eq!(s.pool, 60, "the cancel re-seeds the pool");
    }

    /// The review's press goes straight to the target: there is no per-member
    /// Begin | Reselect in the session - that screen is the party's, raised by
    /// the World after the last member commits.
    #[test]
    fn the_review_press_resolves_through_the_target_picker() {
        let mut s = ArtsCommandInputSession::new(0, 0, 60, [30; 4], 0);
        s.input(press("U"), party3(), one_monster());
        s.input(press("U"), party3(), one_monster());
        assert!(matches!(s.phase, ArtsInputPhase::Review));
        s.input(press("c"), party3(), one_monster()); // review -> target
        // One monster: the picker may resolve immediately or need one
        // confirm.
        if s.resolved().is_none() {
            s.input(press("c"), party3(), one_monster());
        }
        assert_eq!(
            s.resolved(),
            Some(ArtsInputResolution::Confirmed {
                target_row: CursorRow::Enemy,
                target_slot: 0,
            })
        );
    }

    /// The open-time preseed replays on a bare confirm, costs nothing from
    /// the entry pool, and survives nothing but an empty entry: the first
    /// press wipes it, and so does the review's cancel.
    #[test]
    fn a_bare_confirm_replays_the_preseed_without_a_charge() {
        let mut s = ArtsCommandInputSession::new(0, 0, 60, [30; 4], 0).with_preseed(vec![4, 3]);
        s.input(press("c"), party3(), one_monster());
        assert!(matches!(s.phase, ArtsInputPhase::Review));
        assert!(s.replay);
        assert_eq!(s.committed_string(), &[4, 3]);
        assert_eq!(s.pool, 60, "no press was charged");
        assert!(s.spent.is_empty());

        // The review's cancel wipes the window: back to a plain entry.
        s.input(press("o"), party3(), one_monster());
        assert!(matches!(s.phase, ArtsInputPhase::Entering));
        assert!(!s.replay && s.preseed.is_empty());
        s.input(press("c"), party3(), one_monster());
        assert!(
            matches!(s.phase, ArtsInputPhase::Entering),
            "nothing left to replay"
        );

        // The first press wipes it too.
        let mut t = ArtsCommandInputSession::new(0, 0, 90, [30; 4], 0).with_preseed(vec![4, 3]);
        t.input(press("L"), party3(), one_monster());
        assert!(t.preseed.is_empty());
        t.input(press("c"), party3(), one_monster());
        assert!(matches!(t.phase, ArtsInputPhase::Review));
        assert!(!t.replay);
        assert_eq!(t.committed_string(), &[1]);
    }

    /// The gauge build draws the preseed as pennants and cuts it at the
    /// first command one full pool cannot pay (case `0x2C`,
    /// `0x801D4E90` / `0x801D4DC4`) - Gala's off-class capture: a `42`-AP
    /// arm, a `197` pool, and a saved `Right Right Left Left Left Up` that
    /// opens as five pennants.
    #[test]
    fn the_gauge_build_draws_the_affordable_preseed_prefix() {
        let s = ArtsCommandInputSession::new(0, 0, 197, [42, 30, 30, 30], 0)
            .with_preseed(vec![2, 2, 1, 1, 1, 4]);
        assert_eq!(s.preseed, [2, 2, 1, 1, 1], "the unaffordable Up is cut");
        assert_eq!(s.bar_commands(), [2, 2, 1, 1, 1]);
        assert_eq!(s.bar_spent(), [30, 30, 42, 42, 42]);
        assert_eq!(s.pool, 197, "drawing the preseed charges nothing");

        // A bare confirm replays the cut string.
        let mut r = s.clone();
        r.input(press("c"), party3(), one_monster());
        assert_eq!(r.committed_string(), &[2, 2, 1, 1, 1]);
        assert_eq!(r.bar_commands(), [2, 2, 1, 1, 1], "the review keeps them");

        // The first press closes the preseeded pennants.
        let mut t = s;
        t.input(press("U"), party3(), one_monster());
        assert_eq!(t.bar_commands(), [4]);
        assert_eq!(t.bar_spent(), [30]);
    }

    #[test]
    fn circle_on_empty_buffer_aborts() {
        let mut s = ArtsCommandInputSession::new(0, 0, 60, [30; 4], 0);
        s.input(press("o"), party3(), one_monster());
        assert_eq!(s.resolved(), Some(ArtsInputResolution::Aborted));
    }

    /// Retail's cancel mask on a **non-empty** buffer is `FUN_801D388C`
    /// case `0x26`: the sixteen queue bytes are wiped, the pool is re-seeded
    /// from AGL and the committed count is zeroed, with `ctx+0x06` never
    /// written - so the entry restarts in place instead of ending.
    ///
    /// The distinction from [`circle_on_empty_buffer_aborts`] is the whole
    /// point: the same button leaves the entry only when there is nothing
    /// to clear, which is why one press cannot be read as the other.
    #[test]
    fn circle_on_a_typed_buffer_resets_the_entry_instead_of_leaving_it() {
        let mut s = ArtsCommandInputSession::new(0, 0, 60, [30; 4], 0);
        s.input(press("U"), party3(), one_monster());
        assert_eq!(s.buffer.len(), 1, "the direction was accepted");
        assert_eq!(s.pool, 30, "and debited its cost");

        s.input(press("o"), party3(), one_monster());

        assert_eq!(s.resolved(), None, "the entry is still open, not aborted");
        assert!(matches!(s.phase, ArtsInputPhase::Entering), "still in 0x50");
        assert!(s.buffer.is_empty(), "the queue is wiped");
        assert!(s.spent.is_empty());
        assert_eq!(s.pool, s.pool_max, "the pool is re-seeded in full");

        // And a second Circle - now on the empty buffer it just made - is
        // the leave press, so the reset is not a one-way trap.
        s.input(press("o"), party3(), one_monster());
        assert_eq!(s.resolved(), Some(ArtsInputResolution::Aborted));
    }

    #[test]
    fn the_retail_pager_steps_at_most_three_pages() {
        assert_eq!(arts_list_pager(None, 0), None, "no art: inert");
        assert_eq!(arts_list_pager(Some(0), 0), Some(0));
        assert_eq!(arts_list_pager(None, 3), Some(0));
        assert_eq!(arts_list_pager(Some(0), 5), None, "one page closes");
        assert_eq!(arts_list_pager(Some(0), 6), Some(1));
        assert_eq!(arts_list_pager(Some(1), 10), None);
        assert_eq!(arts_list_pager(Some(1), 11), Some(2));
        assert_eq!(arts_list_pager(Some(2), 16), None, "the third page closes");
        let mut s = ArtsCommandInputSession::new(0, 0, 60, [30; 4], 9).with_list_rows(7);
        assert_eq!(s.list_pages, 2);
        s.input(press("t"), party3(), one_monster());
        s.input(press("t"), party3(), one_monster());
        assert_eq!(s.list_page, Some(1));
        s.input(press("t"), party3(), one_monster());
        assert_eq!(s.list_page, None);
    }

    #[test]
    fn triangle_cycles_the_arts_list_and_is_inert_without_pages() {
        let mut s = ArtsCommandInputSession::new(0, 0, 60, [30; 4], 2);
        assert_eq!(s.list_page, None);
        s.input(press("t"), party3(), one_monster());
        assert_eq!(s.list_page, Some(0));
        s.input(press("t"), party3(), one_monster());
        assert_eq!(s.list_page, Some(1));
        s.input(press("t"), party3(), one_monster());
        assert_eq!(s.list_page, None, "last page closes");
        // No pages: the toggle is inert (retail's no-learned-art case).
        let mut none = ArtsCommandInputSession::new(0, 0, 60, [30; 4], 0);
        none.input(press("t"), party3(), one_monster());
        assert_eq!(none.list_page, None);
    }

    #[test]
    fn resolver_matches_arts_and_leaves_swings_synthetic() {
        use Command::{Down, Up};
        // Art 0x1B = [Up, Up] with two damage bytes; art 0x1C = [Down].
        let records = vec![
            rec(0x1B, &[Up, Up], &[0x1A, 0x1A]),
            rec(0x1C, &[Down], &[0x1B]),
        ];
        // Left (unmatched swing) + Up Up (art) + Down (art).
        let entry = resolve_entered_commands(&records, &[1, 4, 4, 3]);
        assert_eq!(entry.matched.len(), 2);
        assert_eq!(entry.matched[0].as_byte(), 0x1B);
        assert_eq!(entry.matched[1].as_byte(), 0x1C);
        // 1 synthetic + 2 (art 0x1B) + 1 (art 0x1C) strikes.
        assert_eq!(entry.power.len(), 4);
        assert_eq!(entry.action.map(|a| a.as_byte()), Some(0x1B));
    }

    #[test]
    fn resolver_with_no_catalog_is_all_synthetic() {
        let entry = resolve_entered_commands(&[], &[4, 3, 1]);
        assert!(entry.matched.is_empty());
        assert_eq!(entry.power.len(), 3);
        assert_eq!(entry.action, None);
        // Empty buffer floors at one hit.
        assert_eq!(resolve_entered_commands(&[], &[]).power.len(), 1);
    }
}
