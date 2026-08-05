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
//! The review screen's next press reaches the **Begin | Reselect** menu
//! (`0x6E`): Begin plays the round out, Reselect returns to a clean input.
//! **Triangle** cycles the learned arts list (closed -> page 1 -> ... ->
//! closed) and is inert when the character has no learned art.
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
//! - **Open gap.** The art itself is not additionally charged from the
//!   Spirit gauge, so the swing costs above are the whole price of the
//!   turn. Retail pays the art body out of `actor[+0x170]` through the
//!   accumulator `actor[+0x224]`; the mechanism is decoded in
//!   `arts-command-gauge.md` § What an art costs in AP but is not wired
//!   here.
//!
//! Two entries previously disclosed here as engine conveniences - "Cross
//! confirms early" and "Circle backs out" - were **not** divergences: both
//! are retail behaviour, driven by the configurable confirm / cancel masks,
//! and the sites are cited in [`ArtsCommandInputSession::input`]. The claim
//! they rested on ("retail entry only auto-ends", "retail's Arts command
//! cannot be backed out of at all") is falsified by the disassembly.
//!
//! PORT: FUN_801D0748 (state 0x50 / 0x5A / 0x6E flow)
//! PORT: FUN_801D388C (case 9 cost read + case 0xB pool debit)

use crate::target_picker::{
    CursorRow, PickerInput, PickerOutcome, SlotState, TargetKind, TargetPickerSession,
};
use legaia_art::power::PowerByte;
use legaia_art::queue::Command;
use legaia_art::{ArtRecord, EnemyEffect};

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
    /// The committed bar review (retail `0x5A`) - any press advances.
    Review,
    /// The Begin | Reselect menu (retail `0x6E`). `cursor` 0 = Begin,
    /// 1 = Reselect.
    BeginMenu { cursor: u8 },
    /// Begin chosen; picking the art's target (engine flow - retail
    /// pre-picks the target with the Attack command).
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

impl ArtsCommandInputSession {
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
            phase: ArtsInputPhase::Entering,
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
            self.list_page = match self.list_page {
                None => Some(0),
                Some(p) if p + 1 < self.list_pages => Some(p + 1),
                Some(_) => None,
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
                // Any press reaches the Begin | Reselect menu.
                self.phase = if ev.cross || ev.circle || ev.up || ev.down || ev.left || ev.right {
                    ArtsInputPhase::BeginMenu { cursor: 0 }
                } else {
                    ArtsInputPhase::Review
                };
            }
            ArtsInputPhase::BeginMenu { cursor } => {
                // Spatial seating on the drawn pair: the menu is two stacked
                // rows (`Begin` above `Reselect` -
                // `legaia_engine_ui::arts_input::BEGIN_MENU_SEAT` + pitch), so
                // Up is always the top row and Down the bottom one - and the
                // direction press itself commits the row, like every other
                // battle prompt. Cross commits the cursor's row.
                let (cursor, pressed) = if ev.up {
                    (0, true)
                } else if ev.down {
                    (1, true)
                } else {
                    (cursor, false)
                };
                if pressed || ev.cross {
                    if cursor == 0 {
                        // Begin: pick the target, then run.
                        let picker = TargetPickerSession::new(
                            TargetKind::SingleEnemy,
                            self.party_slot,
                            party,
                            monsters,
                        );
                        self.phase = match picker.outcome() {
                            Some(PickerOutcome::Single { slot, row }) => {
                                ArtsInputPhase::Confirmed {
                                    target_row: row,
                                    target_slot: slot,
                                }
                            }
                            Some(PickerOutcome::Sweep { row }) => ArtsInputPhase::Confirmed {
                                target_row: row,
                                target_slot: 0,
                            },
                            Some(PickerOutcome::NoCandidates) => ArtsInputPhase::Aborted,
                            _ => ArtsInputPhase::Targeting { picker },
                        };
                    } else {
                        // Reselect: clean input, full pool (retail: the
                        // previous round's pennants clear on the first
                        // fresh press; the port clears on entry).
                        self.buffer.clear();
                        self.spent.clear();
                        self.pool = self.pool_max;
                        self.phase = ArtsInputPhase::Entering;
                    }
                } else if ev.circle {
                    self.phase = ArtsInputPhase::Review;
                } else {
                    self.phase = ArtsInputPhase::BeginMenu { cursor };
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
                    Some(PickerOutcome::Cancelled) => ArtsInputPhase::BeginMenu { cursor: 0 },
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
    /// The Begin | Reselect pick (`cursor` 0 = Begin).
    BeginMenu { cursor: u8 },
    /// Picking the Begin target; the bar stays up behind the picker.
    Targeting,
}

impl From<&ArtsInputPhase> for ArtsInputScreen {
    fn from(p: &ArtsInputPhase) -> Self {
        match p {
            ArtsInputPhase::Entering => Self::Entering,
            ArtsInputPhase::Review => Self::Review,
            ArtsInputPhase::BeginMenu { cursor } => Self::BeginMenu { cursor: *cursor },
            ArtsInputPhase::Targeting { .. } => Self::Targeting,
            // A resolved session is torn down the same frame; nothing
            // draws from it.
            _ => Self::Review,
        }
    }
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
    /// Remaining / seeded entry pool (drives the bar length).
    pub pool: u16,
    pub pool_max: u16,
    /// Per-direction press costs (Left, Right, Down, Up).
    pub costs: [u16; 4],
    /// Value the right-hand AP plate shows. Retail reads the caster's
    /// Spirit gauge here and it does **not** drain during entry.
    pub plate_value: u8,
    /// Open Triangle arts-list page (`None` = closed).
    pub list_page: Option<u8>,
    /// Pages the Triangle list can cycle (`0` = the toggle is inert).
    pub list_pages: u8,
    pub phase: ArtsInputScreen,
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
    fn begin_reselect_round_trip_restores_the_pool() {
        let mut s = ArtsCommandInputSession::new(0, 0, 60, [30; 4], 0);
        s.input(press("U"), party3(), one_monster());
        s.input(press("D"), party3(), one_monster());
        assert!(matches!(s.phase, ArtsInputPhase::Review));
        // Any press reaches Begin | Reselect.
        s.input(press("c"), party3(), one_monster());
        assert!(matches!(s.phase, ArtsInputPhase::BeginMenu { cursor: 0 }));
        // One Down press takes Reselect (the bottom drawn row) on the press
        // itself - spatial seating with retail's direct commit, same as
        // every other battle prompt.
        s.input(press("D"), party3(), one_monster());
        assert!(matches!(s.phase, ArtsInputPhase::Entering));
        assert!(s.buffer.is_empty());
        assert_eq!(s.pool, 60, "Reselect restores the pool");
    }

    #[test]
    fn begin_resolves_through_the_target_picker() {
        let mut s = ArtsCommandInputSession::new(0, 0, 60, [30; 4], 0);
        s.input(press("U"), party3(), one_monster());
        s.input(press("U"), party3(), one_monster());
        s.input(press("c"), party3(), one_monster()); // review -> menu
        s.input(press("c"), party3(), one_monster()); // Begin
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
