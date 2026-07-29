//! Monster combat-stat randomizer: redistribute HP / MP / ATK / DEF / INT / SPD
//! across the enemy roster.
//!
//! Each monster's combat stats live as `u16` halfwords at fixed offsets in the
//! decoded `battle_data` block (PROT entry 867; offsets pinned in
//! [`legaia_asset::monster_archive`]). [`plan_stats`] collects each field's
//! value across the whole populated roster into a *column*, then either permutes
//! the column ([`StatMode::Shuffle`] - a 1:1 reassignment, so the multiset of
//! each stat is preserved) or draws each cell from the column pool with
//! replacement ([`StatMode::Random`]). Because every value that lands came from
//! a real monster, the global stat budget is preserved and no field is ever
//! pushed outside the game's own range - a tanky enemy may inherit a weakling's
//! HP while keeping its own attack, scrambling difficulty without producing
//! impossible records.
//!
//! AGL (`+0x0E`, the agility / action gauge) is **deliberately left alone** - it
//! gates the enemy AI's action economy (how many actions / casts it can afford
//! per round) rather than player-facing difficulty, and shuffling it would
//! mostly perturb how often enemies act, not how hard the fight is.
//!
//! A set of scripted enemies ([`PROTECTED_MONSTER_IDS`]) is left untouched so
//! their fights stay coherent. Two kinds qualify. **Early tutorial enemies** -
//! the scripted Rim Elm sparring partner fights the player in a teaching battle
//! the game never expects the player to lose (there is no game-over branch out
//! of it), so giving it a different monster's attack can let it one-shot the
//! party and soft-lock a brand-new game; the first wild enemies are similarly
//! fragile by design, and a late-game monster's stats can wall a fresh save.
//! **Story bosses** - set-piece fights tuned around scripted HP/phase triggers
//! and a specific difficulty; scrambling their stats can make a mandatory fight
//! unwinnable (or trivial), and donating a boss's extreme stats to a random
//! trash mob is its own kind of soft-lock. Every version of each protected boss
//! is pinned. Their combat stats are always kept as the disc ships them, both as
//! a randomization source and target. The encounter randomizer already keeps
//! scripted formations fixed (`crate::encounter`); this is the matching guard on
//! the stat side.
//!
//! Each edit re-packs the monster's slot through [`crate::monster::repack_slot`]:
//! the decoded block length is unchanged, so the slot stays its original
//! `0x14000`-byte footprint and every other monster's slot offset is fixed - a
//! same-size, in-place byte edit, exactly like the drop randomizer.
//!
//! ## Difficulty scale (the multiplier)
//!
//! [`plan_scale`] is the module's second, *seedless* pass: instead of moving
//! values between monsters it multiplies every monster's stats by a
//! [`StatScale`], so the whole roster gets weaker or stronger while every
//! monster keeps its own relative profile. It composes with the randomizer - run
//! the shuffle first and the scale multiplies the *shuffled* values.
//!
//! A [`StatScale`] carries one [`ScalePermille`] (`0.1x..=5x`) **per stat
//! field**, which gives the knob two spellings through a single
//! [`StatScale::parse`]:
//!
//! - **Uniform** - a bare multiplier, `"2.5"`, scales every field alike. This is
//!   the whole difficulty dial in one number.
//! - **Per-field** - a `key=value` list, `"hp=2,attack=1.5"`, scales only the
//!   named fields and leaves the rest at retail. Useful for shaping a fight
//!   rather than just hardening it: `"hp=3"` alone makes enemies spongy without
//!   making them lethal, and `"attack=2,defense=0.5"` makes them glass cannons.
//!
//! A [`ScaleProfile`] then carries **two** of those - one for random encounters
//! and one for bosses - so a run can soften the trash and harden the set-pieces
//! (or the reverse) instead of moving the whole roster together. It is the same
//! widening trick one level up: a single-scale profile is just the case where
//! both halves are equal, spelled `"2"`, and the split is spelled
//! `"regular:1|boss:2.5"`. Which monsters land in which half is read off the
//! disc's own encounter tables - see [`crate::monster_class`].
//!
//! One parser serves the CLI flag and the browser's simple/advanced slider modes,
//! so every front end accepts the same spellings and emits the same bytes.
//!
//! Two things about the scale differ from the randomizer above, and both are
//! deliberate:
//!
//! - **Bosses are scaled.** A difficulty knob that skipped the set-piece fights
//!   would leave the hardest fights in the game untouched, which is the opposite
//!   of what it is for. The shuffle's [`PROTECTED_MONSTER_IDS`] guard exists
//!   because *reassigning* a boss's stats breaks a scripted fight; multiplying
//!   them keeps every fight's shape and only moves its difficulty. The one
//!   carve-out is [`SCALE_PINNED_MONSTER_IDS`].
//! - **AGL is still untouched.** `+0x0E` is the action gauge, not a difficulty
//!   stat: scaling it multiplies how many actions an enemy gets per round, which
//!   turns a 5x run into a slideshow of enemy turns rather than a harder fight.
//!   [`STAT_FIELDS`] already excludes it, so the scale inherits the exclusion.
//!
//! Rewards (EXP `+0x46` / gold `+0x44` / the drop slot) are outside
//! [`STAT_FIELDS`] and never move, so a 5x run does not also pay out 5x.
//!
//! The scale lands on the **record**, and the battle loader applies its own
//! fixed boost on top when it copies a record into a live actor (`FUN_80054cb0`
//! multiplies ATK / DEF / INT - see [`legaia_asset::monster_archive`]). The two
//! compose multiplicatively, so an `Nx` record really is an `Nx` fight, up to
//! the loader's own integer rounding.

use crate::monster::repack_slot;
use crate::monster_class::{MonsterClass, MonsterClasses};
use crate::rng::SplitMix64;
use anyhow::Result;

/// How a randomizer reassigns values. Re-exported [`crate::drops::DropMode`] so
/// the monster-stat pass shares the CLI's `shuffle` / `random` vocabulary.
pub use crate::drops::DropMode as StatMode;

/// The combat-stat fields the randomizer touches, as `(label, decoded-record
/// byte offset)`. Each is a little-endian `u16` halfword in the monster's
/// decoded block. Order matches [`StatAssignment::stats`].
pub const STAT_FIELDS: [(&str, usize); 7] = [
    ("hp", 0x0C),
    ("mp", 0x10),
    ("attack", 0x12),
    ("defense_high", 0x14),
    ("defense_low", 0x16),
    ("intelligence", 0x18),
    ("speed", 0x1A),
];

/// Number of stat fields a [`StatAssignment`] carries.
pub const FIELD_COUNT: usize = STAT_FIELDS.len();

/// 1-based monster ids of the early **tutorial enemies** - the first fights a
/// fresh save meets, which must stay beatable at level 1.
///
/// One half of [`PROTECTED_MONSTER_IDS`]; the other is
/// [`STORY_BOSS_MONSTER_IDS`].
pub const TUTORIAL_MONSTER_IDS: &[u16] = &[
    19, 20, 21, // Red / Black / Blue Piura - the first wild enemies, deliberately weak.
    79, // Tetsu, the Rim Elm sparring partner (999/999, unwinnable by design).
];

/// 1-based monster ids of the **story bosses**, every version of each.
///
/// Hand-curated from the game's set-piece fights, which makes it a provenance
/// the disc's encounter tables don't share - and it earns its keep twice over.
/// The stat *shuffle* needs it because reassigning a boss's stats breaks a
/// fight scripted around them (see the module docs). The difficulty *scale*
/// needs it for the opposite reason: it is the curated floor under
/// [`crate::monster_class`]'s disc-derived split, covering the boss forms the
/// game swaps in mid-battle, which no formation record names and no scan can
/// therefore find.
///
/// The other half of [`PROTECTED_MONSTER_IDS`]; see [`TUTORIAL_MONSTER_IDS`].
pub const STORY_BOSS_MONSTER_IDS: &[u16] = &[
    10, // Gimard - the early scripted Seru-boss fight (also guarded on the encounter side).
    73, 171, 172, // Caruban
    75,  // Zeto
    76, 136, 179, // Songi
    77, 173, 174, // Berserker
    175, // Tetsu (boss form; 79 in TUTORIAL_MONSTER_IDS is the tutorial form)
    138, // Dohati
    139, // Xain
    162, 163, 164, // Gi / Che / Lu Delilas
    165, 166, // Gaza
    169, // Zora
    170, // Jette
    182, // Koru (the muscle-dome strip arm's 0xB6 gate names it; scripted nilboa fight)
    180, 181, 183, 184, 185, 186, // Cort
];

/// 1-based monster ids the stat randomizer must never modify.
///
/// 1-based monster ids pinned to their disc stats - never modified, and never a
/// donor into another monster's stats. The union of the two groups the module
/// docs describe: [`TUTORIAL_MONSTER_IDS`] (must stay beatable on a fresh save)
/// and [`STORY_BOSS_MONSTER_IDS`] (set-piece fights randomized stats could
/// break, or whose extreme stats would wreck balance if leaked to a trash mob).
/// Spelled out rather than concatenated so the guard stays a `const` the shuffle
/// can test against directly; `protected_ids_are_the_two_groups` pins the
/// partition.
pub const PROTECTED_MONSTER_IDS: &[u16] = &[
    // Early tutorial enemies.
    19, 20, 21, // Red / Black / Blue Piura - the first wild enemies, deliberately weak.
    79, // Tetsu, the Rim Elm sparring partner (999/999, unwinnable by design).
    // Story bosses (all versions of each).
    10, // Gimard - the early scripted Seru-boss fight (also guarded on the encounter side).
    73, 171, 172, // Caruban
    75,  // Zeto
    76, 136, 179, // Songi
    77, 173, 174, // Berserker
    175, // Tetsu (boss form; 79 above is the tutorial form)
    138, // Dohati
    139, // Xain
    162, 163, 164, // Gi / Che / Lu Delilas
    165, 166, // Gaza
    169, // Zora
    170, // Jette
    182, // Koru (the muscle-dome strip arm's 0xB6 gate names it; scripted nilboa fight)
    180, 181, 183, 184, 185, 186, // Cort
];

/// One monster's stat values, in [`STAT_FIELDS`] order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatAssignment {
    /// 1-based monster id (the `battle_data` archive slot index + 1).
    pub monster_id: u16,
    /// The randomized stat halfwords, in [`STAT_FIELDS`] order
    /// (`[hp, mp, attack, defense_high, defense_low, intelligence, speed]`).
    pub stats: [u16; FIELD_COUNT],
}

/// A monster's overall **combat power**: the sum of its damage-relevant combat
/// stats - every [`STAT_FIELDS`] entry **except MP**
/// (`hp + attack + defense_high + defense_low + agility + speed`). A single
/// scalar standing in for a monster's whole stat budget, so a swapped-in monster
/// can be compared against an area's native average (the
/// solo-strong-encounter option). MP is excluded for the same reason the AGL
/// gauge is left out of the stat shuffle: it gates the enemy's action economy,
/// not raw danger. Saturating, so a degenerate record can never overflow.
pub fn combat_power(stats: &[u16; FIELD_COUNT]) -> u32 {
    // STAT_FIELDS order: hp, mp, attack, defense_high, defense_low, intelligence, speed.
    let [hp, _mp, atk, dhi, dlo, intl, spd] = *stats;
    [hp, atk, dhi, dlo, intl, spd]
        .into_iter()
        .fold(0u32, |acc, v| acc.saturating_add(v as u32))
}

/// Read the [`STAT_FIELDS`] halfwords out of a decoded monster block. Returns
/// `None` if the block is too short to hold the last field.
pub fn read_stats(block: &[u8]) -> Option<[u16; FIELD_COUNT]> {
    let mut out = [0u16; FIELD_COUNT];
    for (i, (_, off)) in STAT_FIELDS.iter().enumerate() {
        let b = block.get(*off..*off + 2)?;
        out[i] = u16::from_le_bytes([b[0], b[1]]);
    }
    Some(out)
}

/// Re-pack a monster slot with new stat values. Writes each [`STAT_FIELDS`]
/// halfword into the decoded block and recompresses into a fresh `0x14000`-byte
/// slot. Same-size, in place; errors only on the [`repack_slot`] guards
/// (empty/filler slot, LZS failure, re-packed stream overflows the slot).
pub fn set_stats(slot_bytes: &[u8], stats: &[u16; FIELD_COUNT]) -> Result<Vec<u8>> {
    repack_slot(slot_bytes, |block| {
        for (i, (_, off)) in STAT_FIELDS.iter().enumerate() {
            if let Some(dst) = block.get_mut(*off..*off + 2) {
                dst.copy_from_slice(&stats[i].to_le_bytes());
            }
        }
    })
}

/// Plan a column-wise stat randomization. `current` holds `(id, stats)` for
/// every populated monster, in roster order; the returned plan is the same
/// monsters with reassigned stats. Deterministic in `(current, seed, mode)`.
///
/// Each field is randomized independently across the roster: [`StatMode::Shuffle`]
/// permutes the column (so the multiset of, say, every monster's HP is exactly
/// preserved); [`StatMode::Random`] draws each cell from the column pool with
/// replacement (the multiset is no longer preserved, but every value is still a
/// real in-game stat).
///
/// Monsters in [`PROTECTED_MONSTER_IDS`] (the scripted tutorial fight) are passed
/// through unchanged and excluded from every column pool, so they keep their own
/// stats and never donate them to another monster. Under `Shuffle` this still
/// preserves each column's full multiset: a protected monster contributes the
/// same value before and after, and the rest are a permutation among themselves.
pub fn plan_stats(current: &[StatAssignment], seed: u64, mode: StatMode) -> Vec<StatAssignment> {
    let mut out = current.to_vec();
    // Indices of the monsters eligible for randomization (everything but the
    // protected scripted-fight ids). Protected entries stay byte-identical.
    let free: Vec<usize> = (0..current.len())
        .filter(|&i| !PROTECTED_MONSTER_IDS.contains(&current[i].monster_id))
        .collect();
    if free.is_empty() {
        return out;
    }
    let mut rng = SplitMix64::new(seed);
    for field in 0..FIELD_COUNT {
        let column: Vec<u16> = free.iter().map(|&i| current[i].stats[field]).collect();
        match mode {
            StatMode::Shuffle => {
                let mut bag = column;
                rng.shuffle(&mut bag);
                for (&i, value) in free.iter().zip(bag) {
                    out[i].stats[field] = value;
                }
            }
            StatMode::Random => {
                for &i in &free {
                    out[i].stats[field] = column[rng.below(column.len())];
                }
            }
        }
    }
    out
}

/// 1-based monster ids the **difficulty scale** must never touch.
///
/// Much shorter than [`PROTECTED_MONSTER_IDS`], because a uniform multiplier
/// keeps every fight's shape (see the module docs) - story bosses are scaled on
/// purpose. What a multiplier *can* break is a fight whose script depends on the
/// player being unable to end it: the Rim Elm sparring partner is unwinnable by
/// design and has no branch for the player winning, so scaling it *down* turns
/// the tutorial into a soft-lock. Its stats are pinned in both directions rather
/// than only below `1x`, so the fight is byte-identical at every setting.
pub const SCALE_PINNED_MONSTER_IDS: &[u16] = &[
    79, // Tetsu, the Rim Elm sparring partner (999/999, unwinnable by design).
];

/// A monster-stat difficulty multiplier, held as **permille** (thousandths) so
/// the plan is exact integer arithmetic and a given setting always reproduces
/// byte-identically - no float ever reaches the disc.
///
/// Range [`MIN`](Self::MIN)`..=`[`MAX`](Self::MAX) (`0.1x..=5x`);
/// [`RETAIL`](Self::RETAIL) (`1x`) is the identity and applies no edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ScalePermille(u32);

impl ScalePermille {
    /// Weakest setting: `0.1x`.
    pub const MIN: u32 = 100;
    /// Strongest setting: `5x`.
    pub const MAX: u32 = 5000;
    /// The identity multiplier - retail stats, no edit.
    pub const RETAIL: u32 = 1000;

    /// Build from a raw permille value, rejecting anything outside
    /// [`MIN`](Self::MIN)`..=`[`MAX`](Self::MAX).
    pub fn from_permille(permille: u32) -> Result<Self, String> {
        if !(Self::MIN..=Self::MAX).contains(&permille) {
            return Err(format!(
                "enemy stat scale {} is out of range (want {}..={})",
                Self(permille),
                Self(Self::MIN),
                Self(Self::MAX),
            ));
        }
        Ok(Self(permille))
    }

    /// Parse a user-facing multiplier: `"2.5"`, `"2.5x"`, `"0.1"`, `"1"`.
    /// Rounded to the nearest permille, then range-checked. The shared entry
    /// point for the CLI flag and the browser slider, so both accept exactly the
    /// same values and produce the same bytes.
    pub fn parse(text: &str) -> Result<Self, String> {
        let t = text.trim().trim_end_matches(['x', 'X']).trim();
        let value: f64 = t
            .parse()
            .map_err(|_| format!("{text:?} is not a number (want a multiplier like 2.5)"))?;
        if !value.is_finite() || value < 0.0 {
            return Err(format!("{text:?} is not a usable multiplier"));
        }
        Self::from_permille((value * 1000.0).round() as u32)
    }

    /// The multiplier in thousandths (`2500` = `2.5x`).
    pub fn permille(self) -> u32 {
        self.0
    }

    /// Whether this is the identity multiplier (retail stats).
    pub fn is_retail(self) -> bool {
        self.0 == Self::RETAIL
    }
}

impl std::fmt::Display for ScalePermille {
    /// `2500` -> `2.5x`, `1000` -> `1x` (trailing zeros trimmed).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (whole, frac) = (self.0 / 1000, self.0 % 1000);
        if frac == 0 {
            write!(f, "{whole}x")
        } else {
            write!(f, "{whole}.{}x", format!("{frac:03}").trim_end_matches('0'))
        }
    }
}

/// A difficulty multiplier **per stat field** - one [`ScalePermille`] for each
/// [`STAT_FIELDS`] entry, in that order.
///
/// This is the whole of the difficulty knob. A uniform scale is not a separate
/// mode, just the case where every field holds the same multiplier
/// ([`uniform`](Self::uniform)), which is why one type backs both the CLI's bare
/// `--enemy-stat-scale 2` and the browser's advanced per-stat sliders. Keeping a
/// single representation means the two spellings cannot drift apart: there is
/// exactly one planner, one clamp rule and one set of bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatScale([ScalePermille; FIELD_COUNT]);

/// The per-field keys [`StatScale::parse`] accepts, for error messages. Kept
/// next to [`fields_for_key`] so the two cannot fall out of step.
const SCALE_KEYS: &str = "hp, mp, attack, defense, defense_high, defense_low, intelligence, speed";

/// Resolve a per-field key to the [`STAT_FIELDS`] indices it sets.
///
/// Generous about spelling because the names have three provenances that all
/// reach a user: this module's own field labels, the runtime's `UDF`/`LDF`
/// (upper/lower defence - Legaia splits defence by attack height), and the plain
/// words a player would type. `defense` deliberately resolves to **both**
/// defence halves, since a player asking for "half defence" means the stat, not
/// one of its two internal halfwords.
fn fields_for_key(key: &str) -> Option<&'static [usize]> {
    // STAT_FIELDS order: hp, mp, attack, defense_high, defense_low,
    // intelligence, speed.
    Some(match key {
        "hp" | "health" => &[0],
        "mp" | "magic_points" => &[1],
        "attack" | "atk" => &[2],
        "defense_high" | "def_high" | "udf" | "upper_defense" => &[3],
        "defense_low" | "def_low" | "ldf" | "lower_defense" => &[4],
        "defense" | "def" | "defence" => &[3, 4],
        "intelligence" | "int" | "magic" => &[5],
        "speed" | "spd" => &[6],
        _ => return None,
    })
}

impl StatScale {
    /// Retail stats - every field at `1x`. Applies no edit.
    pub fn retail() -> Self {
        Self([ScalePermille(ScalePermille::RETAIL); FIELD_COUNT])
    }

    /// One multiplier across every field (the simple difficulty dial).
    pub fn uniform(scale: ScalePermille) -> Self {
        Self([scale; FIELD_COUNT])
    }

    /// Build from an explicit per-field array, in [`STAT_FIELDS`] order.
    pub fn from_fields(fields: [ScalePermille; FIELD_COUNT]) -> Self {
        Self(fields)
    }

    /// This field's multiplier. `field` indexes [`STAT_FIELDS`].
    pub fn get(self, field: usize) -> ScalePermille {
        self.0[field]
    }

    /// The per-field multipliers, in [`STAT_FIELDS`] order.
    pub fn fields(self) -> [ScalePermille; FIELD_COUNT] {
        self.0
    }

    /// Whether every field is retail, i.e. the whole scale is a no-op.
    pub fn is_retail(self) -> bool {
        self.0.iter().all(|s| s.is_retail())
    }

    /// The single multiplier this scale applies, when every field shares one.
    /// `None` for a genuinely per-field scale.
    pub fn uniform_value(self) -> Option<ScalePermille> {
        let first = self.0[0];
        self.0.iter().all(|s| *s == first).then_some(first)
    }

    /// Parse either spelling of the knob:
    ///
    /// - no `=` in the text -> a **uniform** multiplier (`"2.5"`, `"2.5x"`).
    /// - otherwise a **per-field** `key=value` list, separated by commas and/or
    ///   whitespace (`"hp=2,attack=1.5"`, `"hp=2 defense=0.5"`). Fields not named
    ///   stay at retail.
    ///
    /// Every value goes through [`ScalePermille::parse`], so both spellings share
    /// one range check and one rounding rule. Errors rather than clamps or
    /// ignores: an unknown stat name or a field set twice is a typo, and silently
    /// applying a *different* difficulty than the one asked for is the failure
    /// mode worth being loud about.
    pub fn parse(text: &str) -> Result<Self, String> {
        let t = text.trim();
        if t.is_empty() {
            return Err(
                "no enemy stat scale given (want a multiplier like 2.5, or a \
                        per-stat list like hp=2,attack=1.5)"
                    .to_string(),
            );
        }
        if !t.contains('=') {
            return Ok(Self::uniform(ScalePermille::parse(t)?));
        }

        let mut out = Self::retail();
        let mut seen = [false; FIELD_COUNT];
        for token in t
            .split([',', ';', ' ', '\t', '\n', '\r'])
            .filter(|s| !s.trim().is_empty())
        {
            let (key, value) = token.trim().split_once('=').ok_or_else(|| {
                format!("{token:?} is not a stat=multiplier pair (want e.g. hp=2)")
            })?;
            let norm = key.trim().to_ascii_lowercase().replace('-', "_");
            let fields = fields_for_key(&norm).ok_or_else(|| {
                format!("unknown stat {:?} (want one of: {SCALE_KEYS})", key.trim())
            })?;
            let scale = ScalePermille::parse(value)?;
            for &f in fields {
                if seen[f] {
                    return Err(format!("stat {:?} is set more than once", STAT_FIELDS[f].0));
                }
                seen[f] = true;
                out.0[f] = scale;
            }
        }
        Ok(out)
    }
}

impl Default for StatScale {
    fn default() -> Self {
        Self::retail()
    }
}

impl From<ScalePermille> for StatScale {
    fn from(scale: ScalePermille) -> Self {
        Self::uniform(scale)
    }
}

impl std::fmt::Display for StatScale {
    /// A uniform scale prints as the bare multiplier (`2.5x`); a per-field one
    /// lists only the fields that actually move (`hp=2x attack=1.5x`), so a
    /// manifest line stays readable when one stat out of seven is touched.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(u) = self.uniform_value() {
            return write!(f, "{u}");
        }
        let mut first = true;
        for (i, (label, _)) in STAT_FIELDS.iter().enumerate() {
            if self.0[i].is_retail() {
                continue;
            }
            if !first {
                write!(f, " ")?;
            }
            first = false;
            write!(f, "{label}={}", self.0[i])?;
        }
        Ok(())
    }
}

/// A difficulty scale **per enemy class** - one [`StatScale`] for random
/// encounters and one for bosses.
///
/// The same widening as [`StatScale`] itself, one level up: a whole-roster scale
/// is not a separate mode, just the case where both halves hold the same value
/// ([`uniform`](Self::uniform)). That is what lets the CLI flag, the wasm
/// boundary and the browser's four slider panes all stay one string through one
/// [`parse`](Self::parse) - adding the split needed no new argument anywhere.
///
/// Which half a monster is scaled by comes from
/// [`crate::monster_class::MonsterClasses`], read off the disc's encounter
/// tables. A uniform profile never consults it, so the classification scan is
/// only paid for when the two halves actually differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScaleProfile {
    regular: StatScale,
    boss: StatScale,
}

/// The enemy-group keys [`ScaleProfile::parse`] accepts, for error messages.
const PROFILE_KEYS: &str = "regular, boss, all";

impl ScaleProfile {
    /// Retail stats on both halves. Applies no edit.
    pub fn retail() -> Self {
        Self::uniform(StatScale::retail())
    }

    /// One scale across the whole roster - bosses and trash alike. The classic
    /// single-dial difficulty knob, and the shape every pre-split setting takes.
    pub fn uniform(scale: StatScale) -> Self {
        Self {
            regular: scale,
            boss: scale,
        }
    }

    /// An explicit split.
    pub fn new(regular: StatScale, boss: StatScale) -> Self {
        Self { regular, boss }
    }

    /// The random-encounter half.
    pub fn regular(self) -> StatScale {
        self.regular
    }

    /// The boss half.
    pub fn boss(self) -> StatScale {
        self.boss
    }

    /// The half that scales `class`.
    pub fn get(self, class: MonsterClass) -> StatScale {
        match class {
            MonsterClass::Regular => self.regular,
            MonsterClass::Boss => self.boss,
        }
    }

    /// Whether both halves are retail, i.e. the whole profile is a no-op.
    pub fn is_retail(self) -> bool {
        self.regular.is_retail() && self.boss.is_retail()
    }

    /// Whether both halves are the same scale - a whole-roster dial rather than
    /// a split. The caller's cue that no monster classification is needed.
    pub fn is_uniform(self) -> bool {
        self.regular == self.boss
    }

    /// Parse either spelling of the knob:
    ///
    /// - **no `:`** -> the text is one [`StatScale`] applied to the whole roster
    ///   (`"2.5"`, `"hp=2,attack=1.5"`). Every setting written before the split
    ///   existed still means exactly what it did.
    /// - **scoped** -> `|`-separated `group:scale` segments
    ///   (`"regular:1|boss:2.5"`, `"regular:hp=2|boss:hp=4,attack=2"`). Groups
    ///   are `regular` (aliases `normal`, `random`, `common`), `boss` (`bosses`)
    ///   and `all` (`both`, `every`). An unscoped segment among scoped ones is
    ///   read as `all`, so `"2|boss:4"` is "everything 2x, bosses 4x".
    ///
    /// `|` is the segment separator precisely because [`StatScale::parse`] never
    /// uses it: a segment's own body keeps the full `key=value` grammar,
    /// separators and all, so the two parsers compose without escaping.
    ///
    /// A group left unnamed falls back to the `all` segment, or to retail when
    /// there isn't one - `"boss:3"` hardens bosses and leaves random encounters
    /// exactly as shipped. Like [`StatScale::parse`] this errors rather than
    /// ignoring: an unknown group or a group set twice is a typo, and quietly
    /// applying a different difficulty is the failure worth being loud about.
    pub fn parse(text: &str) -> Result<Self, String> {
        let t = text.trim();
        if t.is_empty() {
            return Err("no enemy stat scale given (want a multiplier like 2.5, a \
                        per-stat list like hp=2,attack=1.5, or a per-group split \
                        like regular:1|boss:2.5)"
                .to_string());
        }
        if !t.contains(':') {
            return Ok(Self::uniform(StatScale::parse(t)?));
        }

        let (mut regular, mut boss, mut all) = (None, None, None);
        for seg in t.split('|').filter(|s| !s.trim().is_empty()) {
            let seg = seg.trim();
            // A segment with no `:` among scoped ones is the `all` base.
            let (group, spec) = match seg.split_once(':') {
                Some((g, v)) => (g.trim().to_ascii_lowercase().replace('-', "_"), v.trim()),
                None => ("all".to_string(), seg),
            };
            let slot = match group.as_str() {
                "regular" | "normal" | "random" | "common" => &mut regular,
                "boss" | "bosses" => &mut boss,
                "all" | "both" | "every" => &mut all,
                _ => {
                    return Err(format!(
                        "unknown enemy group {group:?} (want one of: {PROFILE_KEYS})"
                    ));
                }
            };
            if slot.is_some() {
                return Err(format!("enemy group {group:?} is set more than once"));
            }
            *slot = Some(StatScale::parse(spec)?);
        }
        let base = all.unwrap_or_else(StatScale::retail);
        Ok(Self {
            regular: regular.unwrap_or(base),
            boss: boss.unwrap_or(base),
        })
    }
}

impl Default for ScaleProfile {
    fn default() -> Self {
        Self::retail()
    }
}

impl From<StatScale> for ScaleProfile {
    fn from(scale: StatScale) -> Self {
        Self::uniform(scale)
    }
}

impl std::fmt::Display for ScaleProfile {
    /// A uniform profile prints as the bare scale (`2.5x`, `hp=2x`), so a
    /// single-dial run's manifest line is unchanged by the split existing; a
    /// genuine split prints the scoped spelling its own [`parse`](Self::parse)
    /// accepts back.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_uniform() {
            return write!(f, "{}", self.regular);
        }
        write!(f, "regular:{}|boss:{}", self.regular, self.boss)
    }
}

/// Multiply one monster's stats by `scale`, field by field.
///
/// Each [`STAT_FIELDS`] halfword is scaled independently with
/// round-half-up integer arithmetic, then held inside the record's own `u16`
/// range. Two clamps matter:
///
/// - **A zero stays zero.** A monster with no MP has no MP at any multiplier;
///   scaling would otherwise invent a resource it never had.
/// - **A non-zero never reaches zero.** At `0.1x` a 5-HP enemy floors at `1`
///   rather than becoming a zero-HP actor the battle code never expects.
///
/// The top end saturates at [`u16::MAX`], so a `5x` run on a boss already past
/// `13107` HP simply pins that stat at the record's ceiling.
///
/// A field left at `1x` is exactly the identity, not a rounding of it:
/// `(v * 1000 + 500) / 1000 == v` for every `u16`, so a per-field scale writes
/// only the fields it names.
pub fn scale_stats(stats: &[u16; FIELD_COUNT], scale: StatScale) -> [u16; FIELD_COUNT] {
    let mut out = *stats;
    for (i, v) in out.iter_mut().enumerate() {
        if *v == 0 {
            continue;
        }
        // Round half up. `u16::MAX * 5000 + 500` stays well inside u32.
        let scaled = (*v as u32 * scale.get(i).permille() + 500) / 1000;
        *v = scaled.clamp(1, u16::MAX as u32) as u16;
    }
    out
}

/// Plan a uniform difficulty scale over the roster. `current` holds
/// `(id, stats)` for every populated monster; the returned plan is the same
/// monsters with each stat multiplied by `scale`.
///
/// Seedless and total: the result depends only on `(current, scale)`.
/// Monsters in [`SCALE_PINNED_MONSTER_IDS`] pass through untouched; everything
/// else - story bosses included - is scaled. An all-retail scale is the identity,
/// so the caller writes nothing.
pub fn plan_scale(current: &[StatAssignment], scale: StatScale) -> Vec<StatAssignment> {
    plan_scale_profile(
        current,
        ScaleProfile::uniform(scale),
        &MonsterClasses::all_regular(),
    )
}

/// Plan a **split** difficulty scale: each monster is multiplied by the half of
/// `profile` its [`MonsterClass`] selects, per `classes`.
///
/// The general form of [`plan_scale`], which is this with a uniform profile and
/// a classification that cannot matter. Seedless and total, same as the uniform
/// pass: the result depends only on `(current, profile, classes)`.
/// [`SCALE_PINNED_MONSTER_IDS`] still passes through untouched **whichever class
/// it lands in** - the tutorial fight is unwinnable by design and a boss slider
/// must not resurrect it any more than a regular one may.
pub fn plan_scale_profile(
    current: &[StatAssignment],
    profile: ScaleProfile,
    classes: &MonsterClasses,
) -> Vec<StatAssignment> {
    current
        .iter()
        .map(|a| {
            if SCALE_PINNED_MONSTER_IDS.contains(&a.monster_id) {
                *a
            } else {
                StatAssignment {
                    monster_id: a.monster_id,
                    stats: scale_stats(&a.stats, profile.get(classes.class_of(a.monster_id))),
                }
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_asset::monster_archive::SLOT_STRIDE;

    /// `[u32 size][LZS]` slot padded to `SLOT_STRIDE`, like a real archive slot.
    fn fake_slot(block: &[u8]) -> Vec<u8> {
        let stream = legaia_lzs::compress(block);
        let mut slot = Vec::with_capacity(SLOT_STRIDE);
        slot.extend_from_slice(&(block.len() as u32).to_le_bytes());
        slot.extend_from_slice(&stream);
        slot.resize(SLOT_STRIDE, 0);
        slot
    }

    fn decode_block(slot: &[u8]) -> Vec<u8> {
        let declared = u32::from_le_bytes(slot[0..4].try_into().unwrap()) as usize;
        legaia_lzs::decompress(&slot[4..], declared).unwrap()
    }

    #[test]
    fn set_stats_is_surgical() {
        // Recognisable, non-zero content at every byte.
        let mut block: Vec<u8> = (0..256u32).map(|i| (i * 5 + 3) as u8).collect();
        // Known starting stats.
        let start = [10u16, 20, 30, 40, 50, 60, 70];
        for (i, (_, off)) in STAT_FIELDS.iter().enumerate() {
            block[*off..*off + 2].copy_from_slice(&start[i].to_le_bytes());
        }
        let slot = fake_slot(&block);

        let new = [111u16, 222, 333, 444, 555, 666, 777];
        let patched = set_stats(&slot, &new).expect("re-pack");
        assert_eq!(patched.len(), SLOT_STRIDE, "slot size preserved");

        let out = decode_block(&patched);
        assert_eq!(out.len(), block.len(), "decoded length preserved");
        assert_eq!(read_stats(&out).unwrap(), new, "stats applied");

        // Every byte outside the seven stat halfwords is untouched.
        let mut expected = block.clone();
        for (i, (_, off)) in STAT_FIELDS.iter().enumerate() {
            expected[*off..*off + 2].copy_from_slice(&new[i].to_le_bytes());
        }
        assert_eq!(out, expected, "only the stat halfwords changed");
    }

    fn sample(n: usize) -> Vec<StatAssignment> {
        // Base ids at 100 so the synthetic roster never overlaps the real
        // PROTECTED_MONSTER_IDS (the tutorial enemies) - a test that wants a
        // protected monster sets one id explicitly.
        (0..n)
            .map(|i| StatAssignment {
                monster_id: i as u16 + 100,
                stats: [
                    i as u16,
                    i as u16 + 100,
                    i as u16 + 200,
                    i as u16 + 300,
                    i as u16 + 400,
                    i as u16 + 500,
                    i as u16 + 600,
                ],
            })
            .collect()
    }

    /// Shuffle preserves each column's multiset and is a 1:1 reassignment.
    #[test]
    fn shuffle_preserves_each_column_multiset() {
        let current = sample(24);
        let plan = plan_stats(&current, 0xABCD_1234, StatMode::Shuffle);
        assert_eq!(plan.len(), current.len());
        for field in 0..FIELD_COUNT {
            let mut before: Vec<u16> = current.iter().map(|a| a.stats[field]).collect();
            let mut after: Vec<u16> = plan.iter().map(|a| a.stats[field]).collect();
            before.sort_unstable();
            after.sort_unstable();
            assert_eq!(before, after, "column {field} multiset must be preserved");
        }
        // ids are unchanged (the plan re-skins monsters in place).
        for (c, p) in current.iter().zip(&plan) {
            assert_eq!(c.monster_id, p.monster_id);
        }
    }

    /// Random draws stay within the column's value set (no invented stats).
    #[test]
    fn random_draws_from_column_pool() {
        let current = sample(16);
        let plan = plan_stats(&current, 7, StatMode::Random);
        for field in 0..FIELD_COUNT {
            let pool: std::collections::HashSet<u16> =
                current.iter().map(|a| a.stats[field]).collect();
            for a in &plan {
                assert!(pool.contains(&a.stats[field]), "drew an out-of-pool value");
            }
        }
    }

    #[test]
    fn plan_is_deterministic() {
        let current = sample(20);
        let a = plan_stats(&current, 99, StatMode::Shuffle);
        let b = plan_stats(&current, 99, StatMode::Shuffle);
        assert_eq!(a, b, "same seed must reproduce the plan");
    }

    #[test]
    fn empty_roster_is_noop() {
        assert!(plan_stats(&[], 1, StatMode::Shuffle).is_empty());
    }

    /// A protected monster keeps its exact stats and never leaks them into the
    /// pool, while the rest of the roster is still randomized.
    #[test]
    fn protected_monster_is_pinned() {
        let protected = PROTECTED_MONSTER_IDS[0];
        // A roster that includes the protected id, with a recognisable, unique
        // stat block on the protected monster.
        let mut current = sample(24);
        let pidx = 5;
        current[pidx].monster_id = protected;
        let pinned = [4242u16, 4243, 4244, 4245, 4246, 4247, 4248];
        current[pidx].stats = pinned;

        for mode in [StatMode::Shuffle, StatMode::Random] {
            let plan = plan_stats(&current, 0x1234_5678, mode);
            let p = plan.iter().find(|a| a.monster_id == protected).unwrap();
            assert_eq!(
                p.stats, pinned,
                "{mode:?}: protected monster must be pinned"
            );
            // Its unique values never appear on any other monster.
            for a in &plan {
                if a.monster_id == protected {
                    continue;
                }
                for (field, &p) in pinned.iter().enumerate() {
                    assert_ne!(
                        a.stats[field], p,
                        "{mode:?}: protected monster's stats leaked to id {}",
                        a.monster_id
                    );
                }
            }
            // The rest of the roster is actually randomized (not a no-op).
            let moved = current
                .iter()
                .zip(&plan)
                .filter(|(c, p)| c.monster_id != protected && c.stats != p.stats)
                .count();
            assert!(moved > 0, "{mode:?}: non-protected monsters should change");
        }
    }

    fn scale(text: &str) -> ScalePermille {
        ScalePermille::parse(text).expect("valid scale")
    }

    /// A whole-roster scale parsed from either spelling.
    fn uni(text: &str) -> StatScale {
        StatScale::parse(text).expect("valid scale")
    }

    /// The exact strings the browser page emits must parse here.
    ///
    /// The page fixes every slider to one decimal, because a `0.1` range step can
    /// land on `2.5000000000000004`. So the wire format is `"2.0"`, not `"2"`,
    /// and the advanced mode's keys are these seven spellings specifically - this
    /// pins that contract, since a rename in `fields_for_key` would otherwise
    /// only surface as a silently skipped slider in a browser.
    #[test]
    fn parses_the_strings_the_browser_emits() {
        // Simple mode at rest, and at both ends of its range.
        assert!(uni("1.0").is_retail());
        assert_eq!(uni("2.5").uniform_value(), Some(scale("2.5")));
        assert_eq!(uni("0.1").uniform_value(), Some(scale("0.1")));
        assert_eq!(uni("5.0").uniform_value(), Some(scale("5")));

        // Advanced mode joins only the stats that moved, comma-separated.
        let adv = uni("hp=3.0,defense_high=0.5,defense_low=0.5");
        assert_eq!(adv.get(0), scale("3"), "hp");
        assert_eq!(adv.get(3), scale("0.5"), "defense_high");
        assert_eq!(adv.get(4), scale("0.5"), "defense_low");
        for untouched in [1, 2, 5, 6] {
            assert!(
                adv.get(untouched).is_retail(),
                "{} must stay retail",
                STAT_FIELDS[untouched].0
            );
        }

        // Every key the advanced panel can emit - the `SCALE_STATS` list in
        // site/js/rom-patcher-app.js, which mirrors STAT_FIELDS.
        for (label, _) in STAT_FIELDS {
            assert!(
                StatScale::parse(&format!("{label}=2.0")).is_ok(),
                "the page can emit {label}=, so it must parse"
            );
        }
    }

    /// The split spellings the page emits, exactly as its collect block builds
    /// them. Pinned here for the same reason as the per-stat keys above: the
    /// browser sends one opaque string across the wasm boundary, so a grammar
    /// change that this parser stopped accepting would surface only as a patch
    /// silently skipped in a browser.
    #[test]
    fn parses_the_split_strings_the_browser_emits() {
        // Both groups equal collapse to an unscoped scale - the page never emits
        // `regular:2.0|boss:2.0`, and this is why the pre-split wire format is
        // still what a whole-roster run sends.
        assert!(prof("2.0").is_uniform());

        // A genuine split, simple mode. A group at rest is spelled `1.0`, not
        // left empty, because an empty segment is not a scale.
        let p = prof("regular:0.5|boss:3.0");
        assert_eq!(p.regular(), uni("0.5"));
        assert_eq!(p.boss(), uni("3.0"));
        let only_boss = prof("regular:1.0|boss:3.0");
        assert!(only_boss.regular().is_retail());
        assert_eq!(only_boss.boss(), uni("3"));

        // Advanced mode: each group's body is the same comma-joined list the
        // single-group advanced pane already emitted, including one group at
        // rest while the other shapes stats.
        let adv = prof("regular:1.0|boss:hp=3.0,defense_high=0.5,defense_low=0.5");
        assert!(adv.regular().is_retail());
        assert_eq!(adv.boss().get(0), scale("3"));
        assert_eq!(adv.boss().get(3), scale("0.5"));
        assert_eq!(adv.boss().get(4), scale("0.5"));

        // Every (group, stat) pair the advanced panel can emit - the
        // `SCALE_GROUPS` x `SCALE_STATS` grid in site/js/rom-patcher-app.js.
        for group in ["regular", "boss"] {
            for (label, _) in STAT_FIELDS {
                let text = format!("{group}:{label}=2.0|{}:1.0", other_group(group));
                assert!(
                    ScaleProfile::parse(&text).is_ok(),
                    "the page can emit {text:?}, so it must parse"
                );
            }
        }
    }

    /// The other half of the two-group grid, for the emit-every-pair loop above.
    fn other_group(g: &str) -> &'static str {
        if g == "boss" { "regular" } else { "boss" }
    }

    /// The user-facing multiplier round-trips through permille and back to a
    /// display string, and the accepted spellings agree.
    #[test]
    fn scale_parses_and_displays() {
        assert_eq!(scale("1").permille(), 1000);
        assert_eq!(scale("2.5").permille(), 2500);
        assert_eq!(scale("2.5x").permille(), 2500);
        assert_eq!(scale(" 0.1 ").permille(), 100);
        assert_eq!(scale("5").permille(), 5000);
        assert_eq!(scale("1").to_string(), "1x");
        assert_eq!(scale("2.5").to_string(), "2.5x");
        assert_eq!(scale("0.1").to_string(), "0.1x");
        assert!(scale("1").is_retail());
        assert!(!scale("1.1").is_retail());
    }

    /// Out-of-range and non-numeric settings are refused, not clamped - a typo
    /// must not silently become a different difficulty.
    #[test]
    fn scale_rejects_out_of_range() {
        for bad in ["0", "0.05", "5.1", "10", "-2", "abc", ""] {
            assert!(
                ScalePermille::parse(bad).is_err(),
                "{bad:?} should be refused"
            );
        }
    }

    /// A multiplier scales every stat field, rounding half up.
    #[test]
    fn scale_multiplies_every_field() {
        let stats = [100u16, 40, 25, 30, 35, 45, 55];
        assert_eq!(scale_stats(&stats, uni("1")), stats, "1x is the identity");
        assert_eq!(
            scale_stats(&stats, uni("2")),
            [200, 80, 50, 60, 70, 90, 110]
        );
        // 25 * 0.5 = 12.5 -> 13 (half up); 35 * 0.5 = 17.5 -> 18.
        assert_eq!(
            scale_stats(&stats, uni("0.5")),
            [50, 20, 13, 15, 18, 23, 28]
        );
    }

    /// The per-field spelling: named stats move, every other field is left
    /// byte-identical rather than round-tripped through the scale arithmetic.
    #[test]
    fn per_field_scale_touches_only_named_stats() {
        let stats = [100u16, 40, 25, 30, 35, 45, 55];
        // STAT_FIELDS order: hp, mp, attack, defense_high, defense_low, int, speed.
        assert_eq!(
            scale_stats(&stats, uni("hp=2")),
            [200, 40, 25, 30, 35, 45, 55],
            "only HP moves"
        );
        assert_eq!(
            scale_stats(&stats, uni("hp=3,attack=2")),
            [300, 40, 50, 30, 35, 45, 55]
        );
        // `defense` is the stat, so it covers both internal halfwords.
        assert_eq!(
            scale_stats(&stats, uni("defense=0.5")),
            [100, 40, 25, 15, 18, 45, 55],
            "defense reaches both halves"
        );
        // Whitespace separation and the runtime's UDF/LDF names both work.
        assert_eq!(
            scale_stats(&stats, uni("udf=2 ldf=3")),
            [100, 40, 25, 60, 105, 45, 55]
        );
        // A per-field list naming nothing but 1x is still the identity.
        assert_eq!(scale_stats(&stats, uni("hp=1")), stats);
    }

    /// Uniform and per-field are one representation: spelling a uniform scale as
    /// an exhaustive per-field list must produce the identical result.
    #[test]
    fn uniform_and_exhaustive_per_field_agree() {
        let stats = [100u16, 40, 25, 30, 35, 45, 55];
        let spelled = uni("hp=2,mp=2,attack=2,defense=2,intelligence=2,speed=2");
        assert_eq!(spelled, uni("2"), "the two spellings are the same value");
        assert_eq!(scale_stats(&stats, spelled), scale_stats(&stats, uni("2")));
        assert_eq!(spelled.uniform_value(), Some(scale("2")));
    }

    /// Display collapses a uniform scale to the bare multiplier and lists only
    /// the moving fields otherwise - that string lands in the run manifest.
    #[test]
    fn stat_scale_displays_both_spellings() {
        assert_eq!(uni("2").to_string(), "2x");
        assert_eq!(uni("0.5").to_string(), "0.5x");
        assert_eq!(uni("hp=2").to_string(), "hp=2x");
        assert_eq!(
            uni("hp=3,attack=1.5").to_string(),
            "hp=3x attack=1.5x",
            "fields print in STAT_FIELDS order"
        );
        assert_eq!(
            uni("defense=0.5").to_string(),
            "defense_high=0.5x defense_low=0.5x"
        );
        // An all-retail scale is uniform, so it collapses rather than printing empty.
        assert_eq!(uni("hp=1").to_string(), "1x");
        assert!(uni("hp=1").is_retail());
        assert!(!uni("hp=2").is_retail());
        assert_eq!(StatScale::retail().to_string(), "1x");
        assert_eq!(StatScale::default(), StatScale::retail());
        assert_eq!(StatScale::from(scale("2")), uni("2"));
    }

    /// A per-field list refuses typos instead of ignoring them: an unknown stat
    /// name or a doubly-set field would otherwise silently apply a different
    /// difficulty than the one asked for.
    #[test]
    fn per_field_scale_rejects_bad_lists() {
        for bad in [
            "hp=2,hp=3",       // same field twice
            "defense=2,udf=3", // defense already covered def_high
            "agility=2",       // real stat, deliberately not scalable
            "hitpoints=2",     // not an accepted alias
            "hp=9",            // value out of range
            "hp=",             // no value
            "=2",              // no key
            "hp",              // no `=`, so read as a multiplier - and isn't one
            "",                // nothing at all
        ] {
            assert!(
                StatScale::parse(bad).is_err(),
                "{bad:?} should be refused, got {:?}",
                StatScale::parse(bad).map(|s| s.to_string())
            );
        }
        // AGL is excluded from STAT_FIELDS, so no alias can reach it.
        assert!(fields_for_key("agility").is_none());
        assert!(fields_for_key("agl").is_none());
    }

    /// The two clamps: a zero stat stays zero (no invented MP), a non-zero one
    /// never rounds down to zero, and the top saturates inside `u16`.
    #[test]
    fn scale_clamps_at_both_ends() {
        let sparse = [5u16, 0, 1, 0, 0, 2, 0];
        let down = scale_stats(&sparse, uni("0.1"));
        assert_eq!(
            down,
            [1, 0, 1, 0, 0, 1, 0],
            "zeros stay zero, non-zeros floor at 1"
        );

        let huge = [60000u16, 60000, 60000, 60000, 60000, 60000, 60000];
        assert_eq!(
            scale_stats(&huge, uni("5")),
            [u16::MAX; FIELD_COUNT],
            "the record's own u16 ceiling holds"
        );
    }

    /// The scale is seedless, total, and hits bosses - only the pinned tutorial
    /// fight passes through untouched.
    #[test]
    fn plan_scale_covers_bosses_but_pins_the_tutorial() {
        let mut current = sample(24);
        // A story boss (protected against the *shuffle*) and the pinned
        // tutorial partner, side by side.
        let boss = 138; // Dohati
        current[2].monster_id = boss;
        current[7].monster_id = SCALE_PINNED_MONSTER_IDS[0];
        let pinned_stats = current[7].stats;

        let plan = plan_scale(&current, uni("2"));
        assert_eq!(plan.len(), current.len());
        assert!(
            PROTECTED_MONSTER_IDS.contains(&boss),
            "the chosen id must be shuffle-protected, to prove the scale differs"
        );
        let b = plan.iter().find(|a| a.monster_id == boss).unwrap();
        let c = current.iter().find(|a| a.monster_id == boss).unwrap();
        assert_eq!(
            b.stats,
            scale_stats(&c.stats, uni("2")),
            "story bosses are scaled"
        );
        let p = plan
            .iter()
            .find(|a| a.monster_id == SCALE_PINNED_MONSTER_IDS[0])
            .unwrap();
        assert_eq!(p.stats, pinned_stats, "the tutorial fight is pinned");

        // Identity at 1x, and deterministic without a seed.
        assert_eq!(plan_scale(&current, uni("1")), current, "1x is a no-op");
        assert_eq!(
            plan_scale(&current, uni("0.4")),
            plan_scale(&current, uni("0.4"))
        );
        assert!(plan_scale(&[], uni("3")).is_empty());

        // A per-field scale is planned the same way: pinned monster untouched,
        // bosses scaled, and only the named field moves on anyone.
        let per = plan_scale(&current, uni("hp=2"));
        let pinned_now = per
            .iter()
            .find(|a| a.monster_id == SCALE_PINNED_MONSTER_IDS[0])
            .unwrap();
        assert_eq!(pinned_now.stats, pinned_stats, "pin holds per-field too");
        for (c, p) in current.iter().zip(&per) {
            if c.monster_id == SCALE_PINNED_MONSTER_IDS[0] {
                continue;
            }
            assert_eq!(&c.stats[1..], &p.stats[1..], "only HP may move");
        }
    }

    /// The two named groups exactly partition the shuffle's guard: no id is in
    /// both, none is in neither, and none is listed twice. Three lists that must
    /// agree, so the one a caller picks can't quietly be the stale one.
    #[test]
    fn protected_ids_are_the_two_groups() {
        use std::collections::HashSet;
        let tutorial: HashSet<u16> = TUTORIAL_MONSTER_IDS.iter().copied().collect();
        let bosses: HashSet<u16> = STORY_BOSS_MONSTER_IDS.iter().copied().collect();
        let protected: HashSet<u16> = PROTECTED_MONSTER_IDS.iter().copied().collect();

        assert_eq!(tutorial.len(), TUTORIAL_MONSTER_IDS.len(), "no duplicates");
        assert_eq!(bosses.len(), STORY_BOSS_MONSTER_IDS.len(), "no duplicates");
        assert_eq!(
            protected.len(),
            PROTECTED_MONSTER_IDS.len(),
            "no duplicates"
        );
        assert!(
            tutorial.is_disjoint(&bosses),
            "an id cannot be both a tutorial enemy and a story boss"
        );
        assert_eq!(
            &tutorial | &bosses,
            protected,
            "the two groups must be exactly the shuffle's guard"
        );
        // The pinned tutorial partner is a tutorial enemy, not a boss - the
        // scale's pin is about the fight being unwinnable, not about rank.
        for &id in SCALE_PINNED_MONSTER_IDS {
            assert!(tutorial.contains(&id), "id {id} should be a tutorial enemy");
        }
    }

    /// A profile parsed from the given text.
    fn prof(text: &str) -> ScaleProfile {
        ScaleProfile::parse(text).expect("valid profile")
    }

    /// Every pre-split spelling still means "the whole roster", so a saved
    /// setting, a preset or a manifest line written before the split cannot
    /// change meaning.
    #[test]
    fn unscoped_profile_is_the_whole_roster() {
        for text in ["2", "2.5x", "0.1", "hp=2,attack=1.5", "defense=0.5"] {
            let p = prof(text);
            assert!(p.is_uniform(), "{text:?} must apply to both classes");
            assert_eq!(p.regular(), uni(text));
            assert_eq!(p.boss(), uni(text));
            assert_eq!(
                p.to_string(),
                uni(text).to_string(),
                "{text:?} prints as the bare scale"
            );
        }
        assert!(prof("1").is_retail());
        assert!(ScaleProfile::retail().is_retail());
        assert_eq!(ScaleProfile::default(), ScaleProfile::retail());
        assert_eq!(
            ScaleProfile::from(uni("2")),
            ScaleProfile::uniform(uni("2"))
        );
    }

    /// The scoped spelling, including the group aliases and the `all` base.
    #[test]
    fn scoped_profile_splits_the_roster() {
        let p = prof("regular:0.5|boss:3");
        assert!(!p.is_uniform() && !p.is_retail());
        assert_eq!(p.get(MonsterClass::Regular), uni("0.5"));
        assert_eq!(p.get(MonsterClass::Boss), uni("3"));

        // A group left unnamed is retail, not a copy of the other one.
        let boss_only = prof("boss:3");
        assert!(boss_only.regular().is_retail(), "trash ships as authored");
        assert_eq!(boss_only.boss(), uni("3"));

        // An unscoped segment among scoped ones is the `all` base.
        let based = prof("2|boss:4");
        assert_eq!(based.regular(), uni("2"));
        assert_eq!(based.boss(), uni("4"));
        assert_eq!(prof("all:2|regular:0.5").boss(), uni("2"));

        // Aliases, whitespace and per-stat bodies inside a segment.
        assert_eq!(prof("normal:2|bosses:3"), prof("regular:2|boss:3"));
        assert_eq!(prof(" random : 2 | boss : 3 "), prof("regular:2|boss:3"));
        let per = prof("regular:hp=2|boss:hp=4,attack=2");
        assert_eq!(per.regular().get(0), scale("2"));
        assert_eq!(per.boss().get(0), scale("4"));
        assert_eq!(per.boss().get(2), scale("2"));
        assert!(per.regular().get(2).is_retail(), "only HP moves on trash");
    }

    /// A split profile prints the scoped spelling its own parser reads back, so
    /// a manifest line is a usable setting rather than a lossy summary.
    #[test]
    fn profile_display_round_trips() {
        for text in [
            "regular:0.5|boss:3",
            "boss:3",
            "regular:hp=2|boss:hp=4,attack=2",
            "2",
            "hp=2",
        ] {
            let p = prof(text);
            assert_eq!(
                ScaleProfile::parse(&p.to_string()),
                Ok(p),
                "{text:?} -> {} must parse back",
                p
            );
        }
        assert_eq!(
            prof("regular:0.5|boss:3").to_string(),
            "regular:0.5x|boss:3x"
        );
        assert_eq!(prof("boss:3").to_string(), "regular:1x|boss:3x");
        assert_eq!(prof("2|boss:2").to_string(), "2x", "equal halves collapse");
    }

    /// Bad group names and doubly-set groups are refused rather than ignored -
    /// same reason as the per-stat list.
    #[test]
    fn profile_rejects_bad_groups() {
        for bad in [
            "regular:2|regular:3",  // same group twice
            "boss:2|bosses:3",      // ...through an alias
            "2|all:3",              // the bare segment *is* `all`
            "elite:2|boss:3",       // no such group
            "regular:|boss:3",      // empty body
            "regular:9|boss:3",     // body out of range
            "regular:hp=2|boss:hp", // body isn't a scale
            ":2",                   // no group name
        ] {
            assert!(
                ScaleProfile::parse(bad).is_err(),
                "{bad:?} should be refused, got {:?}",
                ScaleProfile::parse(bad).map(|p| p.to_string())
            );
        }
    }

    /// The split planner: each monster takes the half its class selects, and the
    /// pinned tutorial fight is untouched whichever class it lands in.
    #[test]
    fn plan_scale_profile_scales_each_class_by_its_own_half() {
        let mut current = sample(24);
        let boss_id = 138; // Dohati
        current[2].monster_id = boss_id;
        current[7].monster_id = SCALE_PINNED_MONSTER_IDS[0];
        let pinned_stats = current[7].stats;

        // The pinned tutorial partner classified as a *boss*, to prove the pin
        // outranks the class rather than only holding on the regular half.
        let classes = MonsterClasses::from_boss_ids([boss_id, SCALE_PINNED_MONSTER_IDS[0]]);
        let profile = prof("regular:0.5|boss:3");
        let plan = plan_scale_profile(&current, profile, &classes);
        assert_eq!(plan.len(), current.len());
        for (c, p) in current.iter().zip(&plan) {
            if c.monster_id == SCALE_PINNED_MONSTER_IDS[0] {
                assert_eq!(p.stats, pinned_stats, "the tutorial fight is pinned");
            } else if c.monster_id == boss_id {
                assert_eq!(p.stats, scale_stats(&c.stats, uni("3")), "boss half");
            } else {
                assert_eq!(p.stats, scale_stats(&c.stats, uni("0.5")), "regular half");
            }
        }

        // A uniform profile is the old whole-roster pass, whatever the classes
        // say - so the split cannot perturb a single-dial run.
        assert_eq!(
            plan_scale_profile(&current, ScaleProfile::uniform(uni("2")), &classes),
            plan_scale(&current, uni("2")),
            "a uniform profile ignores the classification"
        );
        assert_eq!(
            plan_scale_profile(&current, ScaleProfile::retail(), &classes),
            current,
            "an all-retail profile is a no-op"
        );
    }

    /// Shuffle still preserves each column's full multiset even with a protected
    /// monster in the roster (the protected value is conserved in place).
    #[test]
    fn shuffle_with_protected_preserves_full_multiset() {
        let mut current = sample(24);
        current[3].monster_id = PROTECTED_MONSTER_IDS[0];
        let plan = plan_stats(&current, 0xFEED_BEEF, StatMode::Shuffle);
        for field in 0..FIELD_COUNT {
            let mut before: Vec<u16> = current.iter().map(|a| a.stats[field]).collect();
            let mut after: Vec<u16> = plan.iter().map(|a| a.stats[field]).collect();
            before.sort_unstable();
            after.sort_unstable();
            assert_eq!(before, after, "column {field} multiset must be preserved");
        }
    }
}
