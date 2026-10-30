//! The monster-id safety lists every randomizer pass respects.
//!
//! A randomized run must stay completable: the tutorial enemies stay beatable
//! at level 1, the story bosses keep the stats their set pieces are scripted
//! around, and the unwinnable sparring partner stays unwinnable in both
//! directions. The disc patcher's stat, scale and encounter passes and the
//! engine's run-rule randomizer read the same lists from here.

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
/// fight scripted around them. The difficulty *scale*
/// needs it for the opposite reason: it is the curated floor under
/// `legaia-patcher`'s `monster_class`'s disc-derived split, covering the boss forms the
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

/// 1-based monster ids the **difficulty scale** must never touch.
///
/// Much shorter than [`PROTECTED_MONSTER_IDS`], because a uniform multiplier
/// keeps every fight's shape  - story bosses are scaled on
/// purpose. What a multiplier *can* break is a fight whose script depends on the
/// player being unable to end it: the Rim Elm sparring partner is unwinnable by
/// design and has no branch for the player winning, so scaling it *down* turns
/// the tutorial into a soft-lock. Its stats are pinned in both directions rather
/// than only below `1x`, so the fight is byte-identical at every setting.
pub const SCALE_PINNED_MONSTER_IDS: &[u16] = &[
    79, // Tetsu, the Rim Elm sparring partner (999/999, unwinnable by design).
];
