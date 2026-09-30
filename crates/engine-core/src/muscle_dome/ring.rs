//! The command ring (which chip a fighter may pick), the magic arm, and the per-turn selection types.
//! Split out of `muscle_dome.rs`.

use super::*;

// ---------------------------------------------------------------------------
// The command ring: which chip a dome fighter may pick, and the magic arm
// ---------------------------------------------------------------------------

/// Bit of the **special-battle word** `0x8007BAC0` that forbids the Item
/// chip.
///
/// The arena **does** own this one. `FUN_801CEA6C` seeds the whole word on
/// fresh entry from three story-flag tests in order (`jal 0x8003CE64`): flag
/// `0x536` writes `0x101` (`0x801CEBA0`), `0x537` writes `0x111`
/// (`0x801CEBB4`), `0x538` writes `0x321` (`0x801CEBC8`), the last match
/// winning. The pre-test seed is **`1`**, not `0`: `0x801CEB8C` stores `$s2`,
/// which `0x801CEAF8` loaded with `1` - the same constant
/// [`CONTEST_ENTRY_WORD_DEFAULT`] already carried. All three *flagged* seeds
/// carry this bit, so **every dome visit with a course unlocked forbids the
/// Item chip**, while an arena with no course unlocked (word `1`) forbids
/// nothing. Thereafter the
/// arena stamps only the low byte (`FUN_801D0088` at
/// `0x801D00B8..0x801D00E4` writes `(old & ~0xFF) + (course << 4) + round + 1`,
/// preserving the high bits), and the battle round driver reads the high bits
/// as command restrictions. The low byte is also where the course comes from:
/// `((word - 1) & 0xFF) >> 4` at `0x801CEBD4..0x801CEBE8` is `0` / `1` / `2`
/// for the three seeds.
///
/// Both hosts seed the word at dome entry from those three story flags
/// ([`contest_entry_word`]) and hand it to
/// [`MuscleDomeSession::set_special_word`], so the chips gate exactly as the
/// arena's own entry makes them.
///
/// REF: FUN_801d0748 (`0x801D12C0..0x801D12D8` the mark; `0x801D1370..0x801D137C`
/// the arm's refusal)
pub const SPECIAL_ITEM_FORBIDDEN: u32 = 0x100;

/// Bit of the same word that forbids the **Ra-Seru (magic)** chip - the one
/// that draws the red X over it and makes the ring's Right arm refuse.
///
/// It has **three** raisers, not two. Two are in `SCUS_942.54`'s battle init
/// and key on the **first enemy's monster id**, not on a course:
/// `0x800519DC..0x80051A04` raises it for monster `0xAF`, and
/// `0x8005200C..0x8005205C` for a first enemy in `0x3D..=0x3F` while the mode
/// word `0x80084540` is `0xC` or `0x15`. The dome ladder tops out at monster
/// `0xAA`, so neither of those fires here.
///
/// The third is the arena's **own entry seed**, in PROT 0977: `FUN_801CEA6C`
/// stores `0x321` at `0x801CEBC8` when the story-flag test at `0x801CEBBC`
/// (`jal 0x8003CE64`, flag `0x538`) returns non-zero, beside `0x101` for flag
/// `0x536` and `0x111` for flag `0x537`. `0x321` decodes to course `2`
/// (Master) **plus this bit**, and every later write preserves the high bits,
/// so once the Master course is unlocked every dome round in that visit
/// crosses the Ra-Seru chip out - the bit
/// [`MuscleDomeSession::set_special_word`] carries into the ring. The earlier
/// "no dome round raises it"
/// reading came from a `gp`-relative sweep that caps `lui`-to-use pairing at
/// 24 instructions; the four seed stores sit 34..49 instructions past their
/// `lui` and were invisible to it.
///
/// REF: FUN_801d0748 (`0x801D12DC..0x801D12F4` the mark; `0x801D1448..0x801D1454`
/// the arm's refusal)
pub const SPECIAL_MAGIC_FORBIDDEN: u32 = 0x200;

/// The three `actor+0x16E` status bits that must **all** be set for the
/// Attack chip to be refused (`andi 0x38` then a compare against `0x38`).
///
/// REF: FUN_801d0748 (`0x801D12F8..0x801D132C`, `0x801D1560..0x801D156C`)
pub const STATUS_ATTACK_BLOCKED: u16 = 0x38;

/// The `actor+0x16E` status bit that seals magic: the Ra-Seru chip keeps its
/// plate, wears the sealed mark, and the ring's Right arm refuses.
///
/// REF: FUN_801d0748 (`0x801D1330..0x801D1360`, `0x801D1434..0x801D1440`)
pub const STATUS_MAGIC_SEALED: u16 = 0x1000;

/// One chip of the battle command ring, in the seats the dome draws them.
///
/// The ring is **direction-selected**, and the pad bit each arm tests is the
/// Legaia mask's, not the PSX pad's: Up `0x1000` picks Item, Right `0x2000`
/// picks Ra-Seru, Down `0x4000` picks Spirit, and Attack is taken by the
/// *configured confirm button* (`0x800846D0`) rather than by Left.
///
/// REF: FUN_801d0748 (`0x801D1364` Item, `0x801D1400` Ra-Seru, `0x801D1534`
/// Attack, `0x801D1670` Spirit)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DomeRingChip {
    Item,
    Attack,
    RaSeru,
    Spirit,
}

impl DomeRingChip {
    /// The ring in its seat order (the SCUS screen-element table's records
    /// 8 / 9 / `0xA` / `0xB`).
    pub const RING: [DomeRingChip; 4] = [
        DomeRingChip::Item,
        DomeRingChip::Attack,
        DomeRingChip::RaSeru,
        DomeRingChip::Spirit,
    ];

    /// The chip's arrived screen anchor - the `(x, y)` the mark emitters are
    /// called with, which is the element table's second glide endpoint.
    pub fn anchor(self) -> (i16, i16) {
        match self {
            DomeRingChip::Item => (204, 34),
            DomeRingChip::Attack => (160, 66),
            DomeRingChip::RaSeru => (248, 66),
            DomeRingChip::Spirit => (204, 98),
        }
    }

    /// The action-state byte the arm writes into `actor+0x1DE` when the chip
    /// is taken (`1` Item, `2` Ra-Seru, `3` Attack, `4` Spirit).
    ///
    /// REF: FUN_801d0748 (`0x801D13CC`, `0x801D14A4`, `0x801D15CC`, `0x801D1690`)
    pub fn action_state(self) -> u8 {
        match self {
            DomeRingChip::Item => 1,
            DomeRingChip::RaSeru => 2,
            DomeRingChip::Attack => 3,
            DomeRingChip::Spirit => 4,
        }
    }
}

/// The mark retail lays over a chip the fighter cannot take. All three are
/// the same 64x16 screen quad at `(anchor.x - 8, anchor.y - 4)` off the
/// `etim` page (tpage `7`); they differ only in source rect and palette.
///
/// REF: FUN_801dbc30, FUN_801dbd04, FUN_801dbec4
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChipMark {
    /// The red cross-out X (`FUN_801DBC30`) - the course restriction.
    Forbidden,
    /// `FUN_801DBD04`'s 32x24 mark - the Attack chip under
    /// [`STATUS_ATTACK_BLOCKED`].
    Blocked,
    /// `FUN_801DBEC4`'s 64x16 mark - the Ra-Seru chip under
    /// [`STATUS_MAGIC_SEALED`].
    Sealed,
}

impl ChipMark {
    /// Source rect `(u, v, w, h)` on the `etim` page.
    pub fn source_rect(self) -> (u16, u16, u16, u16) {
        match self {
            ChipMark::Forbidden => (0, 96, 64, 16),
            ChipMark::Blocked => (80, 96, 32, 24),
            ChipMark::Sealed => (120, 96, 64, 16),
        }
    }

    /// The packet's CLUT word.
    pub fn clut(self) -> u16 {
        match self {
            ChipMark::Forbidden => 0x7704,
            ChipMark::Blocked => 0x770B,
            ChipMark::Sealed => 0x7700,
        }
    }
}

/// Everything the command ring gates a chip on for one fighter.
///
/// Retail keeps the three inputs apart, and so does this: the **word** is
/// per battle, the **status** is per actor, and the Ra-Seru marker is the
/// per-member gate `ctx[+0x25F + member]` the party battle-actor init writes
/// (`FUN_80053CB8`, mirrored by
/// [`crate::battle_hud::battle_member_has_raseru`]).
///
/// A missing Ra-Seru is **not** a mark: the chip's label becomes a lone `-`
/// (`FUN_801D8DE8` record `0xA`) and the arm refuses silently. Only the
/// three [`ChipMark`] conditions draw anything.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DomeRing {
    /// The special-battle word `0x8007BAC0`.
    pub special: u32,
    /// The fighter's `actor+0x16E` status halfword.
    pub status: u16,
    /// The per-member Ra-Seru gate `ctx[+0x25F + member]`.
    pub has_raseru: bool,
}

impl DomeRing {
    /// The mark this chip wears, if any.
    ///
    /// PORT: FUN_801d0748 (`0x801D12C0..0x801D1364`, the four mark tests of
    /// the phase-`0x28` arm)
    pub fn mark(&self, chip: DomeRingChip) -> Option<ChipMark> {
        match chip {
            DomeRingChip::Item => {
                (self.special & SPECIAL_ITEM_FORBIDDEN != 0).then_some(ChipMark::Forbidden)
            }
            DomeRingChip::RaSeru => {
                if self.special & SPECIAL_MAGIC_FORBIDDEN != 0 {
                    Some(ChipMark::Forbidden)
                } else if self.status & STATUS_MAGIC_SEALED != 0 {
                    Some(ChipMark::Sealed)
                } else {
                    None
                }
            }
            DomeRingChip::Attack => (self.status & STATUS_ATTACK_BLOCKED == STATUS_ATTACK_BLOCKED)
                .then_some(ChipMark::Blocked),
            DomeRingChip::Spirit => None,
        }
    }

    /// Whether the ring's arm for this chip commits rather than refusing.
    ///
    /// The Ra-Seru arm's three refusals are tested in retail's own order:
    /// the member gate first, then the sealed status, then the forbidden
    /// bit. So a fighter with no Ra-Seru is refused even on a course that
    /// allows magic.
    ///
    /// PORT: FUN_801d0748 (`0x801D1370..0x801D137C` Item,
    /// `0x801D1408..0x801D1454` Ra-Seru, `0x801D1560..0x801D156C` Attack)
    pub fn enabled(&self, chip: DomeRingChip) -> bool {
        match chip {
            DomeRingChip::Item => self.special & SPECIAL_ITEM_FORBIDDEN == 0,
            DomeRingChip::RaSeru => {
                self.has_raseru
                    && self.status & STATUS_MAGIC_SEALED == 0
                    && self.special & SPECIAL_MAGIC_FORBIDDEN == 0
            }
            DomeRingChip::Attack => self.status & STATUS_ATTACK_BLOCKED != STATUS_ATTACK_BLOCKED,
            DomeRingChip::Spirit => true,
        }
    }
}

/// One fighter's magic loadout: the ring gates, the live MP gauge and the
/// spells the Ra-Seru arm offers.
///
/// The spell list is the caster's **learned** block, which retail reads out
/// of the character record at live `+0x13D` (32 ids) with the per-spell
/// level at `+0x161`; the MP cost is the static spell table's `+3` byte
/// discounted by the accessory ability bits at record `+0xF4` (bit `0x20`
/// halves it, bit `0x10` takes a quarter off).
///
/// REF: FUN_801d0748 (`0x801D1A38..0x801D1B70`, the phase-`0x46` cost read)
#[derive(Debug, Clone, Default)]
pub struct DomeMagic {
    /// The fighter's command-ring gates.
    pub ring: DomeRing,
    /// Live MP (`actor+0x150`).
    pub mp: u16,
    /// Max MP (`actor+0x152`).
    pub mp_max: u16,
    /// The character record's ability bitfield `+0xF4` low byte - the MP-saver
    /// accessory bits the arm discounts with.
    pub ability_bits: u8,
    /// The caster's magic-power column, the `caster_mag` the shared cast
    /// kernel takes.
    pub magic_power: u16,
    /// The learned spells, in list order.
    pub spells: Vec<crate::spells::SpellDef>,
}

/// One selectable row of the dome's Ra-Seru list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomeSpellRow {
    pub id: u8,
    pub name: String,
    /// The cost **after** the ability-bit discount - the number the arm
    /// compares against MP.
    pub mp_cost: u16,
    pub affordable: bool,
}

/// Why the ring's Ra-Seru arm refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DomeCastRefusal {
    /// No magic loadout installed for this fighter at all.
    NoLoadout,
    /// `ctx[+0x25F + member] == 0` - the member carries no Ra-Seru.
    NoRaSeru,
    /// `actor+0x16E & 0x1000`.
    Sealed,
    /// The special-battle word's magic bit.
    Forbidden,
    /// The list carries no such spell.
    UnknownSpell,
    /// `actor+0x150 < cost` - retail clears the menu result and stays on the
    /// list rather than committing.
    NotEnoughMp,
    /// The session is not taking a selection.
    WrongPhase,
}

/// What a fighter does with its turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DomeTurnAction {
    /// The queued direction string, already run through the tokenizer.
    Commands(Vec<u8>),
    /// One cast of this spell id. Retail writes the id at `actor+0x1DF[0]`
    /// with `actor+0x1DE = 2` and `actor+0x1E7 = 9`, and never touches the AP
    /// accounting: a cast costs **MP, not AP**.
    ///
    /// REF: FUN_801d0748 (`0x801D1A14..0x801D1A34` the queue store,
    /// `0x801D14A4` / `0x801D14C0` the two action bytes)
    Cast(u8),
}

/// One frame of edge-triggered pad for a dome selection, in the shape both
/// hosts can build.
///
/// `magic` is the surface that opens the Ra-Seru list. Retail takes it off
/// the ring's Right chip; the port's selection has no ring screen, so each
/// host binds it separately (native `play-window`: Triangle; the browser
/// minigames page: the ring's Right chip) and both reach
/// [`MuscleDomeSession::select_input`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DomeSelectPad {
    pub left: bool,
    pub right: bool,
    pub up: bool,
    pub down: bool,
    /// Confirm (Cross).
    pub confirm: bool,
    /// Cancel / back (Circle).
    pub cancel: bool,
    /// Open the Ra-Seru list.
    pub magic: bool,
}

/// One dealt slot: a direction-command id + its per-fighter AP cost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MuscleCard {
    /// Command id (`0xC..=0xF`, from the deck table `DAT_801f4b8c`).
    pub command_id: u8,
    /// AP cost (the fighter's per-command record `+0x74` byte).
    pub cost: u16,
}

/// Match phase, host view of the retail `ctx+6` loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MusclePhase {
    /// Directions are being committed under the turn's AP budget.
    Select,
    /// Both queues are built; the turn is ready to play out.
    Resolve,
    /// The turn played out; HP updated, another turn or a decision.
    TurnOver,
    /// The player's fighter won (reward available).
    Won,
    /// The player's fighter lost.
    Lost,
}

impl MusclePhase {
    /// A **turn** inside an open leg just resolved - and nothing else.
    ///
    /// Retail's turn boundary is entirely inside the battle: the
    /// battle-action SM writes `ctx[6] = 0x14` (the round driver's turn-top
    /// arm) and bumps the turn counter `ctx+0x28a`, then the driver re-enters
    /// its own command cluster (`ctx+6 = 0x28`). The arena's hub state machine
    /// is not running at all - the game is in battle mode - so **no hub screen
    /// is raised between turns**. A host that puts one there is inventing a
    /// beat retail does not have.
    ///
    /// REF: FUN_801e295c (`0x801E67E8..0x801E6810`)
    pub fn ends_turn(self) -> bool {
        matches!(self, Self::TurnOver)
    }

    /// The **leg** is over - the fight ended on a KO, which is the only thing
    /// that ends one.
    ///
    /// This is the boundary the arena hub sees: the `0x5A` end-of-action scan
    /// of `FUN_801E295C` raises the battle-end signal `DAT_8007BD71 = 0xFE`
    /// (party wipe at `0x801E65D8`, cause `5`; monster wipe at `0x801E6674`,
    /// cause `0`), the exit selector routes back to arena mode `0x18`, and only
    /// then does the hub decide between another leg and settlement.
    ///
    /// REF: FUN_801e295c (`0x801E65D8`, `0x801E6674`)
    pub fn ends_leg(self) -> bool {
        matches!(self, Self::Won | Self::Lost)
    }
}

/// One fighter's dome state.
#[derive(Debug, Clone)]
pub(super) struct DomeFighter {
    pub(super) hand: [MuscleCard; HAND_SLOTS],
    /// Remaining turn budget (`ctx+0x6dc`), reseeded each turn.
    pub(super) budget: u16,
    /// Points spent this turn (`ctx+0x6d8`).
    pub(super) spent: u16,
    /// The `+0x1df` action queue: committed command ids this turn.
    pub(super) queue: Vec<u8>,
    pub(super) hp: i32,
    pub(super) max_hp: i32,
    /// The `+0x154` pool the budget reseeds from each turn.
    pub(super) budget_pool: u16,
}

impl DomeFighter {
    pub(super) fn new(hand: [MuscleCard; HAND_SLOTS], budget_pool: u16, hp: i32) -> Self {
        Self {
            hand,
            budget: budget_pool,
            spent: 0,
            queue: Vec::new(),
            hp,
            max_hp: hp.max(1),
            budget_pool,
        }
    }

    pub(super) fn reset_turn(&mut self) {
        self.budget = self.budget_pool;
        self.spent = 0;
        self.queue.clear();
    }
}
