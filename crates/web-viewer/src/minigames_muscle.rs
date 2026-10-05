//! Muscle Dome methods of [`LegaiaMinigames`] - the browser twin of the
//! play-window's `start_muscle_minigame` (`window/minigames.rs`).
//!
//! The dome is not a card battle, and it is not turn-limited either: each
//! round stages one real monster off the course ladder into the ordinary
//! battle formation cell and is fought to a knockout. The
//! `"Turns Left: N   HP Left: P%"` strip belongs to the one fight whose
//! formation slot 0 is monster `0xB6` (Koru); the dome ladder tops out at
//! `0xAA`, so no dome round raises it.
//!
//! The rules are the ported [`legaia_engine_core::muscle_dome`] engine (the
//! four-direction deal, the AP-budget commit into the fighter's action queue,
//! the turn counter, the opponent-HP-left readout and the
//! win/lose bookkeeping). Damage resolves through the **shared retail
//! kernel** [`legaia_engine_core::muscle_dome::DomeDamageModel`], which the
//! native play-window host installs on its session too - the move-power
//! record via the `0x801F4E63` id → index map, the arts/physical damage roll
//! (`FUN_801dd0ac`), the element-affinity scale (`FUN_801dd864`) and the
//! damage finisher (`FUN_801ddb30`), on a PsyQ `rand()` stream with retail
//! draw order. This module holds no damage rule of its own.
//!
//! Fighter stats come from the visitor's own disc records:
//!
//! - the **opponent** is a monster of the PROT 867 archive
//!   ([`legaia_asset::monster_archive`]); its battle stats are the record's
//!   `battle_stats()` boosted profile (the `FUN_80054CB0` load-time boost),
//!   its round budget the record's AGL (the `+0x154` pool the dome's
//!   `ctx+0x6dc` budget seeds from), its element the record's `+0x1D` byte.
//! - the **player** fighter's record is built from the `SCUS_942.54`
//!   new-game starting-party template ([`legaia_asset::new_game`]) leveled
//!   through the per-level stat-growth curves
//!   ([`legaia_asset::level_up_tables`], jitter-free core gains - see the
//!   documented approximation on [`LegaiaMinigames::muscle_start_vs`]), then
//!   seeded into a battle-actor stat block by the ported battle-load init
//!   (`init_party_battle_stats`, `FUN_80053CB8`, no equipment). Card costs
//!   are the character's own player-battle-file swing records (`+0x74`, the
//!   bytes the Arts gauge reads), section defaults.
//!
//! Documented host models / approximations, each surfaced to the page instead
//! of silently invented: the opponent AI (greedy in-order commit - retail has
//! no dome-specific AI table), the player level (retail uses your save's
//! party; the page exposes a level control over the disc's own growth
//! tables), the jitter-free growth core (retail adds a `rand()` spread of
//! mean 0), and the awarded Seru index (the arena init's `ctx+0x269` write is
//! not table-pinned; a default of 1 is used and named via the SCUS spell
//! table).

use super::*;

use legaia_art::queue::{Character as ArtCharacter, Command as ArtCommand};
use legaia_asset::battle_char_assembly as bca;
use legaia_asset::element_affinity::ElementAffinity;
use legaia_asset::monster_archive;
use legaia_asset::move_power;
use legaia_asset::muscle_dome as md;
use legaia_asset::scene_tmd_stream;
use legaia_asset::sfx_table;
use legaia_engine_core::muscle_dome::{
    DomeCombatant, DomeDamageModel, MuscleCard, MuscleDomeSession, MusclePhase,
};
use legaia_engine_vm::battle_formulas::{RecordStats, init_party_battle_stats};

#[path = "minigames_muscle/exports.rs"]
mod exports;
#[path = "minigames_muscle/hud.rs"]
mod hud;
#[path = "minigames_muscle/render.rs"]
mod render;
#[path = "minigames_muscle/session.rs"]
mod session;
#[path = "minigames_muscle/surface.rs"]
mod surface;

/// PROT entry of the monster stat archive (`0867_battle_data`).
const MONSTER_ARCHIVE_PROT_INDEX: u32 = 867;

/// PROT entry of the first player battle file (`data\battle\PLAYER1`,
/// extraction 863; `+ char_slot` for Noa / Gala / Terra).
const PLAYER_BATTLE_FILE_BASE: u32 = 863;

/// PROT entry (extraction space) of the Sol Muscle Dome **arena backdrop**
/// stream - the tail slot of the dome's `data\field\other6.lzs` file (CDNAME
/// `other6` = raw TOC 1222 -> extraction block 1220..=1225, loaded by the
/// arena door/init overlay at extraction 0977). It is the block's only
/// `scene_tmd_stream` - the battle-backdrop carrier format the battle init
/// walker `FUN_8001FE70` records into `_DAT_8007B864`: a leading arena-shell
/// TMD plus two type-0x01 TIM pages at framebuffer `(768, 0)` / `(832, 0)`
/// (CLUT rows 473 / 479) - `(832, 0)` + CLUT `(0, 479)` being exactly the
/// address the battle ground-grid renderer `func_0x801d02c0` samples. See
/// `docs/subsystems/minigame-muscle-dome.md` (Arena backdrop).
const ARENA_BACKDROP_PROT_INDEX: u32 = 1225;

/// The dome match SM's own UI cue ids as **called** (`FUN_801d0748` passes
/// these to the one-arg cue funnel `FUN_8004fcc8`, whose `< 0x40` leg
/// enqueues `id - 1` as the static descriptor row). 34 immediate call sites:
/// `0x21` x13, `0x22` x7, `0x23` x14.
const MUSCLE_UI_CUE_CALL_IDS: [u8; 3] = [0x21, 0x22, 0x23];

/// The physical-impact static cue row of the shared battle/duel bank
/// (descriptor row `0x09`: program 0, tones 9..=10, category 2 -> the PROT
/// 0869 VAB). Pinned as the melee hit at the top of the Baka duel damage
/// kernel (`FUN_801D3B18`); the dome resolves its card plays through the same
/// shared battle-action path and bank.
const MUSCLE_HIT_CUE_ROW: u8 = 0x09;

/// Flat per-card cost fallback (the native launcher's `FAVORED_COST`), used
/// when the character's swing records don't decode.
const FAVORED_COST: u16 = 0x1E;

/// The Seru index the victory **caption** names (`ctx+0x269`); the captioned
/// spell id is `REWARD_SPELL_ID_BASE + index`.
///
/// It is a string index, not a prize. The table it reaches (`0x801F4DFC`) is
/// the shared battle-family cast-caption label table, resident in every
/// battle overlay and read by any cast; the arena grants no Seru. A contest's
/// reward is casino coins - see
/// [`legaia_engine_core::muscle_dome::DomeContest`].
const WEB_CAPTION_SERU: u8 = 1;

/// PROT entry the arena roster/init overlay lives in - the contest layer's
/// course ladder and score table both come off it.
const ARENA_OVERLAY_PROT_INDEX: u32 =
    legaia_engine_core::muscle_dome::ARENA_OVERLAY_PROT_INDEX as u32;

/// Lift the page's two flag bitmasks into the shared
/// [`ContestFlags`](legaia_engine_core::muscle_dome::ContestFlags) the rules
/// kernel takes. The browser has no save file to read a flag bank out of, so
/// the page states what it wants open and the kernel applies the same rule to
/// it that the native host's real bank gets.
fn web_contest_flags(
    unlock: u32,
    gates: u32,
    prize_awarded: bool,
) -> legaia_engine_core::muscle_dome::ContestFlags {
    let bit = |m: u32, i: usize| m & (1 << i) != 0;
    legaia_engine_core::muscle_dome::ContestFlags {
        course_unlock: [bit(unlock, 0), bit(unlock, 1), bit(unlock, 2)],
        master_gates: [bit(gates, 0), bit(gates, 1), bit(gates, 2)],
        prize_awarded,
    }
}

/// Non-elemental element id (`element_affinity` id space).
const ELEMENT_NEUTRAL: u8 = 7;

/// The curated gamedata tables (baked-in TOML), parsed once per session -
/// the arts **kind** labels (regular / hyper / super / miracle) the banner
/// classifier joins onto the disc's own arts rows.
fn gamedata_db() -> &'static legaia_gamedata::Database {
    static DB: std::sync::OnceLock<legaia_gamedata::Database> = std::sync::OnceLock::new();
    DB.get_or_init(legaia_gamedata::Database::load)
}

/// One fighter's battle-formula inputs, resolved from disc records at contest
/// start. Field names follow the battle-actor offsets the damage kernel reads.
#[derive(Debug, Clone, Copy)]
pub(crate) struct MuscleFighter {
    /// Max HP (`+0x14e`); current HP lives in the rules session.
    hp_max: u16,
    /// Max MP (`+0x152`) - displayed on the retail battle status plate. The
    /// dome rules never spend it (the port has no cast path); `0` for a
    /// monster opponent, whose plate retail does not draw.
    mp_max: u16,
    /// AGL (`+0x154`) - the round-budget pool the dome seeds `ctx+0x6dc` from.
    budget_pool: u16,
    /// INT working value (`+0x168`) - the damage kernel's roll stat.
    int: u16,
    /// UDF (`+0x15c`) - defender roll term A.
    udf: u16,
    /// LDF (`+0x160`) - defender roll term B.
    ldf: u16,
    /// Element id (0..=7) for the affinity scale.
    element: u8,
}

impl MuscleFighter {
    /// The subset the shared retail damage kernel reads.
    fn combatant(&self) -> DomeCombatant {
        DomeCombatant {
            hp_max: self.hp_max,
            int: self.int,
            udf: self.udf,
            ldf: self.ldf,
            element: self.element,
        }
    }
}

/// Read a little-endian `u32` at a VA inside the as-loaded PROT 0898 image.
fn overlay_u32(image: &[u8], va: u32) -> Option<u32> {
    let off = va.checked_sub(md::MUSCLE_OVERLAY_BASE_VA)? as usize;
    Some(u32::from_le_bytes(
        image.get(off..off + 4)?.try_into().ok()?,
    ))
}

/// Read the NUL-terminated string at a VA inside the as-loaded PROT 0898
/// image. Bounded at 128 bytes; non-ASCII bytes are dropped, which keeps a
/// mis-resolved pointer from emitting binary into the page.
fn overlay_string(image: &[u8], va: u32) -> Option<String> {
    let off = va.checked_sub(md::MUSCLE_OVERLAY_BASE_VA)? as usize;
    let win = image.get(off..(off + 128).min(image.len()))?;
    let end = win.iter().position(|&b| b == 0)?;
    Some(
        win[..end]
            .iter()
            .filter(|&&b| (0x20..0x7F).contains(&b))
            .map(|&b| b as char)
            .collect(),
    )
}

/// The cached PROT 0898 battle tables the dome plays with.
pub(crate) struct MuscleTables {
    /// The four dealt hand command ids (deck table `DAT_801f4b8c`).
    pub hand: [u8; md::HAND_SLOTS],
    /// The 44-record move-power table (`0x801F4F5C`).
    move_power: Vec<move_power::MoveRecord>,
    /// The 128-byte move-id → power-index map (`0x801F4E63`).
    move_map: [u8; move_power::MOVE_ID_INDEX_MAP_LEN],
    /// The 8x8 element-affinity matrix + per-character elements
    /// (`0x801F53E8` / `0x801F5480`).
    affinity: Option<ElementAffinity>,
}

/// The static spell table's **player Seru-magic** block - the eleven ids a
/// character's Ra-Seru command can offer (`docs/formats/spell-table.md`).
const PLAYER_SERU_IDS: std::ops::RangeInclusive<u8> = 0x81..=0x8b;

/// The refusal name the page shows for a rejected Ra-Seru pick.
fn muscle_refusal_name(e: legaia_engine_core::muscle_dome::DomeCastRefusal) -> &'static str {
    use legaia_engine_core::muscle_dome::DomeCastRefusal as R;
    match e {
        R::NoLoadout => "no_loadout",
        R::NoRaSeru => "no_raseru",
        R::Sealed => "sealed",
        R::Forbidden => "forbidden",
        R::UnknownSpell => "unknown_spell",
        R::NotEnoughMp => "not_enough_mp",
        R::WrongPhase => "wrong_phase",
    }
}

/// A running contest: the rules session plus everything the battle-formula
/// resolution needs alongside it.
pub(crate) struct MuscleContest {
    session: MuscleDomeSession,
    fighters: [MuscleFighter; 2],
    names: [String; 2],
    /// `"disc"` when the player record came from the SCUS template + growth
    /// tables, `"fallback"` when no executable was available.
    stats_source: &'static str,
    monster_id: u16,
    char_slot: usize,
    level: u32,
    /// The contest's `rand()` stream. The native host lends the world's
    /// `rng_state` to [`MuscleDomeSession::resolve_turn_on_stream`]; this page
    /// has no world, so the contest holds the stream it lends instead.
    rng: u32,
}

#[cfg(test)]
#[path = "minigames_muscle/chip_mark_tests.rs"]
mod chip_mark_tests;
