//! Curated game-data tables for *Legend of Legaia* (NTSC-U).
//!
//! Data files live under [`data/gamedata/`](../../../data/gamedata/) and are
//! baked into the library via `include_str!` + `toml::from_str`. The
//! [`Database`] type loads everything once and exposes typed accessors.
//!
//! See the crate-level `README.md` for the full table of contents and the
//! provenance of every entry.
//!
//! ## Cross-validation
//!
//! Compile-time tests in [`tests/`](../../../crates/gamedata/tests/) enforce:
//!
//! - every art name resolves to a canonical name in `legaia_art::tables`
//!   (or is annotated as a derived variant like `"Tornado Flame (Hyper)"`),
//! - every art `directions` byte is in `1..=4` and matches the `command`
//!   token list under the per-character mapping,
//! - every shop inventory `item` key resolves through one of `items`,
//!   `weapons`, `armor`, `accessories`,
//! - the magic table contains exactly 21 Seru + 8 Ra-Seru entries.

#![deny(unsafe_code)]
#![warn(missing_docs)]

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const ARTS_TOML: &str = include_str!("../../../data/gamedata/arts.toml");
const MAGIC_TOML: &str = include_str!("../../../data/gamedata/magic.toml");
const ITEMS_TOML: &str = include_str!("../../../data/gamedata/items.toml");
const WEAPONS_TOML: &str = include_str!("../../../data/gamedata/weapons.toml");
const ARMOR_TOML: &str = include_str!("../../../data/gamedata/armor.toml");
const ACCESSORIES_TOML: &str = include_str!("../../../data/gamedata/accessories.toml");
const ENEMIES_TOML: &str = include_str!("../../../data/gamedata/enemies.toml");
const BOSSES_TOML: &str = include_str!("../../../data/gamedata/bosses.toml");
const SHOPS_TOML: &str = include_str!("../../../data/gamedata/shops.toml");
const CASINO_TOML: &str = include_str!("../../../data/gamedata/casino.toml");
const FISHING_TOML: &str = include_str!("../../../data/gamedata/fishing.toml");
const CHARACTERS_TOML: &str = include_str!("../../../data/gamedata/characters.toml");

// ---------------------------------------------------------------------------
// Arts
// ---------------------------------------------------------------------------

/// One of the three playable characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Character {
    /// Vahn (Meta - fire).
    Vahn,
    /// Noa (Terra - wind). Left-handed; her Arms button maps to *right*.
    Noa,
    /// Gala (Ozma - thunder).
    Gala,
}

impl Character {
    /// Canonical short name used in TOML and CLI args.
    pub fn name(self) -> &'static str {
        match self {
            Character::Vahn => "Vahn",
            Character::Noa => "Noa",
            Character::Gala => "Gala",
        }
    }

    /// Parse a string name (case-insensitive).
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "vahn" => Some(Character::Vahn),
            "noa" => Some(Character::Noa),
            "gala" => Some(Character::Gala),
            _ => None,
        }
    }

    /// Map this character to a [`legaia_art::queue::Character`].
    pub fn to_art_character(self) -> legaia_art::Character {
        match self {
            Character::Vahn => legaia_art::Character::Vahn,
            Character::Noa => legaia_art::Character::Noa,
            Character::Gala => legaia_art::Character::Gala,
        }
    }

    /// Resolve the player-facing input token (`Arms` / `Ra-Seru` / `High`
    /// / `Low`) to the raw direction byte stored in an Art Record:
    /// `1=L, 2=R, 3=D, 4=U`.
    ///
    /// Mapping per [`docs/formats/art-data.md`](../../../docs/formats/art-data.md):
    ///
    /// | Token | Vahn / Gala | Noa |
    /// |---|---|---|
    /// | Arms    | L (1) | R (2) |
    /// | Ra-Seru | R (2) | L (1) |
    /// | High    | U (4) | U (4) |
    /// | Low     | D (3) | D (3) |
    pub fn token_to_byte(self, token: &str) -> Option<u8> {
        let normalised: String = token
            .trim()
            .chars()
            .map(|c| if c == '_' || c == '-' { ' ' } else { c })
            .collect::<String>()
            .to_ascii_lowercase();
        match (self, normalised.as_str()) {
            (Character::Vahn | Character::Gala, "arms") => Some(1),
            (Character::Vahn | Character::Gala, "ra seru" | "raseru") => Some(2),
            (Character::Noa, "arms") => Some(2),
            (Character::Noa, "ra seru" | "raseru") => Some(1),
            (_, "high" | "up" | "u") => Some(4),
            (_, "low" | "down" | "d") => Some(3),
            (_, "left" | "l") => Some(1),
            (_, "right" | "r") => Some(2),
            _ => None,
        }
    }
}

/// The four art kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArtKind {
    /// Regular Tactical Art (no preconditions; learned by trying input
    /// patterns in battle).
    Regular,
    /// Hyper Art (taught by an art Book item).
    Hyper,
    /// Super Art (combo of regular arts; not on the Arts List).
    Super,
    /// Miracle Art (one per character, 99 AP).
    Miracle,
}

/// One arts-table entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Art {
    /// Owner.
    pub character: Character,
    /// Display name.
    pub name: String,
    /// Art kind.
    pub kind: ArtKind,
    /// AP cost when fully expressed in the action queue.
    pub ap: u32,
    /// Player-facing input tokens (e.g. `["Arms", "Ra-Seru", "High"]`).
    pub command: Vec<String>,
    /// Raw direction bytes that end up in the Art Record (`1=L, 2=R, 3=D, 4=U`).
    pub directions: Vec<u8>,
    /// Action constant `0x1B..=0x32` if this art has a slot in the
    /// per-character action constant table. None for art entries that
    /// share an action constant with another entry (e.g. all three
    /// Hurricane Kick on-disc levels share `0x1C`).
    pub action_constant: Option<u8>,
}

#[derive(Debug, Deserialize)]
struct ArtsFile {
    arts: Vec<Art>,
}

// ---------------------------------------------------------------------------
// Magic
// ---------------------------------------------------------------------------

/// Spell family: regular Seru spell vs. egg-derived Ra-Seru summon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SpellFamily {
    /// Standard elemental Seru spell.
    Seru,
    /// Egg-derived character-bound summon (200-255 MP, talisman-gated).
    RaSeru,
}

/// One spell.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Spell {
    /// Display name (e.g. `"Nighto"`).
    pub name: String,
    /// Spell family.
    pub family: SpellFamily,
    /// Element (lowercase: `fire`, `water`, `earth`, `wind`, `thunder`,
    /// `light`, `dark`, `evil`).
    pub element: String,
    /// MP cost.
    pub mp: u16,
    /// Display attack name.
    pub attack: String,
    /// AoE shape.
    pub target: String,
    /// For Ra-Seru summons, the canonically-bound character (None for
    /// later-game summons that are character-agnostic via Talisman).
    #[serde(default)]
    pub character: Option<String>,
    /// Per-encounter absorption probability (%) against the Lv1 form of
    /// the source Seru. Source: Meth962 v1.10 Seru-magic table. Only
    /// populated for `family = "seru"` entries.
    #[serde(default)]
    pub absorb_lv1: Option<u8>,
    /// Per-encounter absorption probability (%) against the Lv2 form.
    #[serde(default)]
    pub absorb_lv2: Option<u8>,
    /// Per-encounter absorption probability (%) against the Lv3 form.
    #[serde(default)]
    pub absorb_lv3: Option<u8>,
}

#[derive(Debug, Deserialize)]
struct MagicFile {
    #[serde(rename = "spell")]
    spells: Vec<Spell>,
}

// ---------------------------------------------------------------------------
// Items
// ---------------------------------------------------------------------------

/// One item entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    /// Stable snake_case id used by `shops.toml` etc.
    pub key: String,
    /// Display name.
    pub name: String,
    /// One of: `consumable`, `permanent_stat`, `key`, `art_book`,
    /// `fishing_lure`.
    pub category: String,
    /// Shop price in Gold; `None` for non-purchasable key items.
    #[serde(default)]
    pub price: Option<u32>,
    /// Effect description.
    pub effect: String,
}

#[derive(Debug, Deserialize)]
struct ItemsFile {
    item: Vec<Item>,
}

// ---------------------------------------------------------------------------
// Weapons
// ---------------------------------------------------------------------------

/// One weapon entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Weapon {
    /// Stable id.
    pub key: String,
    /// Display name.
    pub name: String,
    /// Shop price in Gold; `None` for quest-only weapons.
    #[serde(default)]
    pub price: Option<u32>,
    /// Attack stat.
    pub attack: u16,
    /// Character with the favorite-weapon class match.
    pub equip_best: String,
    /// Other characters that can equip this weapon.
    #[serde(default)]
    pub equip_others: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct WeaponsFile {
    weapon: Vec<Weapon>,
}

// ---------------------------------------------------------------------------
// Armor
// ---------------------------------------------------------------------------

/// One armor entry (body / helmet / shoes).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Armor {
    /// Stable id.
    pub key: String,
    /// Display name.
    pub name: String,
    /// One of `armor`, `helmet`, `shoes`.
    pub slot: String,
    /// Shop price in Gold; `None` for endgame Ra-Seru gear.
    #[serde(default)]
    pub price: Option<u32>,
    /// Upper Defense Factor.
    pub udf: u16,
    /// Lower Defense Factor.
    pub ldf: u16,
    /// Equipment restriction: `Vahn` / `Noa` / `Gala` / `None` (None =
    /// quest-restricted, e.g. War God Plate).
    pub equip: String,
}

#[derive(Debug, Deserialize)]
struct ArmorFile {
    armor: Vec<Armor>,
}

// ---------------------------------------------------------------------------
// Accessories
// ---------------------------------------------------------------------------

/// One accessory entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Accessory {
    /// Stable id.
    pub key: String,
    /// Display name.
    pub name: String,
    /// Shop price in Gold; `None` for quest-only.
    #[serde(default)]
    pub price: Option<u32>,
    /// Free-text effect description.
    pub effect: String,
    /// Structured taxonomy class (see `accessories.toml` header for
    /// the full list). Optional - some accessories don't have a
    /// pinned class yet.
    #[serde(default)]
    pub effect_class: Option<String>,
    /// Numeric value paired with `effect_class` (percentage points,
    /// HP/turn, etc.). Sign matters: -50 means "-50%".
    #[serde(default)]
    pub effect_value: Option<i32>,
    /// Status name (for `effect_class = "protect_status"`).
    #[serde(default)]
    pub status: Option<String>,
    /// Element (for `effect_class = "elemental_def"`).
    #[serde(default)]
    pub element: Option<String>,
    /// Summoned spell name (for `effect_class = "summon_seru"`).
    #[serde(default)]
    pub summons: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AccessoriesFile {
    accessory: Vec<Accessory>,
}

// ---------------------------------------------------------------------------
// Enemies
// ---------------------------------------------------------------------------

/// One enemy entry.
///
/// The `hp` / `mp` / `exp` / `gold` / `atk` / `spd` / `udf` / `ldf` /
/// `intel` / `agl` columns are from Meth962's GameFAQs walkthrough
/// (v1.10 "Added all Enemy stats section"). They were extracted from
/// in-RAM memory at runtime, so they are fan-recorded values rather than
/// retail-binary-extracted constants - useful as labels for the monster
/// records being reverse-engineered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Enemy {
    /// Display name (e.g. `"Aluru Lv2"`).
    pub name: String,
    /// Zone the enemy is encountered in.
    pub location: String,
    /// Element (lowercase) - present for elemental Seru enemies only.
    #[serde(default)]
    pub element: Option<String>,
    /// True if this enemy is a boss / sub-boss.
    #[serde(default)]
    pub boss: bool,
    /// Item key dropped at end of battle (`None` if "Not Available").
    #[serde(default)]
    pub drop: Option<String>,
    /// Item key stolen with Evil God Icon.
    #[serde(default)]
    pub steal: Option<String>,
    /// Steal success chance, percent. Sourced from the static `SCUS_942.54`
    /// steal table (`DAT_80077828`) — byte-verified against the published steal
    /// table — so it is authoritative, not walkthrough-estimated. `None` for the
    /// few boss-name variants whose monster id couldn't be matched unambiguously.
    #[serde(default)]
    pub steal_chance: Option<u32>,
    /// Hit Points.
    #[serde(default)]
    pub hp: Option<u32>,
    /// Magic Points pool.
    #[serde(default)]
    pub mp: Option<u32>,
    /// Experience awarded on defeat.
    #[serde(default)]
    pub exp: Option<u32>,
    /// Gold awarded on defeat.
    #[serde(default)]
    pub gold: Option<u32>,
    /// Attack stat.
    #[serde(default)]
    pub atk: Option<u32>,
    /// Speed stat (initiative / block / escape contributor).
    #[serde(default)]
    pub spd: Option<u32>,
    /// Upper defense stat.
    #[serde(default)]
    pub udf: Option<u32>,
    /// Lower defense stat.
    #[serde(default)]
    pub ldf: Option<u32>,
    /// Intelligence stat (magical damage / magical defense).
    ///
    /// Field name in TOML is `int`; renamed here because `int` is a
    /// reserved keyword in Rust.
    #[serde(default, rename = "int")]
    pub intel: Option<u32>,
    /// Agility stat (AGL fuels the attack-input gauge: 30 AGL = 1 hit slot).
    #[serde(default)]
    pub agl: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct EnemiesFile {
    enemy: Vec<Enemy>,
}

// ---------------------------------------------------------------------------
// Bosses
// ---------------------------------------------------------------------------

/// One boss-fight summary.
///
/// `hp_min` / `hp_max` and the per-enemy stat columns in `enemies.toml`
/// overlap for main-story bosses; this struct adds the *fight-specific*
/// layer (attack moveset, victory rewards, recommended party level).
/// Muscle Dome tournament rounds use the HP-only legacy shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Boss {
    /// Display name (mirrors `enemies.toml`).
    pub name: String,
    /// Zone.
    pub location: String,
    /// Lower bound of HP estimate.
    pub hp_min: u32,
    /// Upper bound of HP estimate.
    pub hp_max: u32,
    /// Muscle Dome course this estimate was measured in (if applicable).
    #[serde(default)]
    pub tournament: Option<String>,
    /// Meth962 FAQ boss ID (`B0001`-`B0014`, or `s0005` for Lapis).
    #[serde(default)]
    pub meth_id: Option<String>,
    /// MP pool at battle start (FAQ stat block).
    #[serde(default)]
    pub mp: Option<u32>,
    /// Named special attacks; auto-attacks aren't enumerated.
    #[serde(default)]
    pub attacks: Vec<BossAttack>,
    /// Experience awarded on victory.
    #[serde(default)]
    pub exp_reward: Option<u32>,
    /// Gold awarded on victory.
    #[serde(default)]
    pub gold_reward: Option<u32>,
    /// Item key dropped on victory (resolves through `items.toml`).
    #[serde(default)]
    pub item_reward: Option<String>,
    /// FAQ's recommended party level range, free-text ("6-7", "34" etc.).
    #[serde(default)]
    pub recommended_level: Option<String>,
    /// Free-text notes (strategy hint, mechanic gotcha).
    #[serde(default)]
    pub notes: Option<String>,
}

/// One special attack in a boss's moveset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BossAttack {
    /// Move name as displayed in the battle log.
    pub name: String,
    /// MP cost (0 = free).
    pub mp: u32,
}

#[derive(Debug, Deserialize)]
struct BossesFile {
    boss: Vec<Boss>,
}

// ---------------------------------------------------------------------------
// Shops
// ---------------------------------------------------------------------------

/// One shop / merchant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shop {
    /// Town name.
    pub town: String,
    /// Optional shop / merchant name.
    #[serde(default)]
    pub name: Option<String>,
    /// Optional merchant name.
    #[serde(default)]
    pub merchant: Option<String>,
    /// Optional phase tag (`"before mist"` / `"after mist"`).
    #[serde(default)]
    pub phase: Option<String>,
    /// Item keys in display order.
    pub inventory: Vec<String>,
    /// Items the walkthrough flagged as "new in this town".
    #[serde(default)]
    pub featured: Vec<String>,
    /// Free-text notes.
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ShopsFile {
    shop: Vec<Shop>,
}

/// A resolved (priced + categorised) entry from a shop's inventory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResolvedShopEntry<'a> {
    /// Item key.
    pub key: &'a str,
    /// Display name.
    pub name: &'a str,
    /// Price (None = quest-only).
    pub price: Option<u32>,
    /// Category bucket.
    pub category: ShopEntryCategory,
}

/// Which gamedata table a shop entry came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ShopEntryCategory {
    /// `items.toml`.
    Item,
    /// `weapons.toml`.
    Weapon,
    /// `armor.toml`.
    Armor,
    /// `accessories.toml`.
    Accessory,
}

// ---------------------------------------------------------------------------
// Casino + fishing + characters
// ---------------------------------------------------------------------------

/// One slot-machine prize entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlotPrize {
    /// `"Vidna"` or `"Sol"`.
    pub location: String,
    /// Item key.
    pub item: String,
    /// Coin cost.
    pub cost_coins: u32,
    /// Optional notes.
    #[serde(default)]
    pub notes: Option<String>,
}

/// One Muscle Dome course.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MuscleDomeCourse {
    /// `"Beginner"` / `"Expert"` / `"Master"`.
    pub name: String,
    /// Per-attempt fee in coins.
    pub entry_fee: u32,
    /// Coin payout for winning.
    pub reward_coins: u32,
    /// First-clear bonus item (Master gives a War God Icon once).
    #[serde(default)]
    pub reward_first_clear: Option<String>,
    /// Enemy line-up in encounter order.
    pub enemies: Vec<String>,
}

/// One Baka Fighter round pattern.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BakaFighterRound {
    /// 1-indexed round number.
    pub round: u32,
    /// Button sequence ("Square", "X", "O" tokens).
    pub buttons: Vec<String>,
    /// Optional notes (e.g. "Songi" boss round).
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CasinoFile {
    #[serde(rename = "slot_prize", default)]
    slot_prizes: Vec<SlotPrize>,
    #[serde(rename = "muscle_dome_course", default)]
    courses: Vec<MuscleDomeCourse>,
    #[serde(rename = "baka_fighter", default)]
    baka_fighter: Vec<BakaFighterRound>,
}

/// One fishing-pond prize entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FishingPrize {
    /// `"Vidna"` or `"Buma"`.
    pub location: String,
    /// Item key.
    pub item: String,
    /// Cost in fishing points.
    pub cost_points: u32,
    /// Optional notes.
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FishingFile {
    #[serde(rename = "fishing_prize")]
    prizes: Vec<FishingPrize>,
}

/// One playable character profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CharacterProfile {
    /// Stable id.
    pub key: String,
    /// Display name.
    pub name: String,
    /// Bound Ra-Seru.
    pub ra_seru: String,
    /// Ra-Seru element (lowercase).
    pub ra_seru_element: String,
    /// Strong against (resistance) elements.
    pub affinity_strong: Vec<String>,
    /// Weak against elements.
    pub affinity_weak: Vec<String>,
    /// Favorite weapon classes.
    pub weapon_classes: Vec<String>,
    /// True if the character is left-handed (Noa only).
    #[serde(default)]
    pub left_handed: bool,
    /// Free-text notes.
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CharactersFile {
    #[serde(rename = "character")]
    characters: Vec<CharacterProfile>,
}

// ---------------------------------------------------------------------------
// Database
// ---------------------------------------------------------------------------

/// In-memory view of every gamedata TOML file.
///
/// Construct with [`Database::load`]; the data is baked in at compile time
/// so this never fails at runtime.
#[derive(Debug, Clone)]
pub struct Database {
    arts: Vec<Art>,
    spells: Vec<Spell>,
    items: Vec<Item>,
    weapons: Vec<Weapon>,
    armor: Vec<Armor>,
    accessories: Vec<Accessory>,
    enemies: Vec<Enemy>,
    bosses: Vec<Boss>,
    shops: Vec<Shop>,
    slot_prizes: Vec<SlotPrize>,
    muscle_dome: Vec<MuscleDomeCourse>,
    baka_fighter: Vec<BakaFighterRound>,
    fishing: Vec<FishingPrize>,
    characters: Vec<CharacterProfile>,
    item_index: BTreeMap<String, usize>,
    weapon_index: BTreeMap<String, usize>,
    armor_index: BTreeMap<String, usize>,
    accessory_index: BTreeMap<String, usize>,
}

impl Database {
    /// Parse every embedded TOML file and return the populated database.
    ///
    /// # Panics
    ///
    /// Panics if any of the embedded TOML files fails to parse. This
    /// would only happen if the data files were edited in a way that
    /// broke the schema; the test suite catches that before commit.
    pub fn load() -> Self {
        let arts: ArtsFile = toml::from_str(ARTS_TOML).expect("arts.toml");
        let magic: MagicFile = toml::from_str(MAGIC_TOML).expect("magic.toml");
        let items: ItemsFile = toml::from_str(ITEMS_TOML).expect("items.toml");
        let weapons: WeaponsFile = toml::from_str(WEAPONS_TOML).expect("weapons.toml");
        let armor: ArmorFile = toml::from_str(ARMOR_TOML).expect("armor.toml");
        let accessories: AccessoriesFile =
            toml::from_str(ACCESSORIES_TOML).expect("accessories.toml");
        let enemies: EnemiesFile = toml::from_str(ENEMIES_TOML).expect("enemies.toml");
        let bosses: BossesFile = toml::from_str(BOSSES_TOML).expect("bosses.toml");
        let shops: ShopsFile = toml::from_str(SHOPS_TOML).expect("shops.toml");
        let casino: CasinoFile = toml::from_str(CASINO_TOML).expect("casino.toml");
        let fishing: FishingFile = toml::from_str(FISHING_TOML).expect("fishing.toml");
        let characters: CharactersFile = toml::from_str(CHARACTERS_TOML).expect("characters.toml");

        let mut db = Self {
            arts: arts.arts,
            spells: magic.spells,
            items: items.item,
            weapons: weapons.weapon,
            armor: armor.armor,
            accessories: accessories.accessory,
            enemies: enemies.enemy,
            bosses: bosses.boss,
            shops: shops.shop,
            slot_prizes: casino.slot_prizes,
            muscle_dome: casino.courses,
            baka_fighter: casino.baka_fighter,
            fishing: fishing.prizes,
            characters: characters.characters,
            item_index: BTreeMap::new(),
            weapon_index: BTreeMap::new(),
            armor_index: BTreeMap::new(),
            accessory_index: BTreeMap::new(),
        };
        db.rebuild_indexes();
        db
    }

    fn rebuild_indexes(&mut self) {
        self.item_index = self
            .items
            .iter()
            .enumerate()
            .map(|(i, e)| (e.key.clone(), i))
            .collect();
        self.weapon_index = self
            .weapons
            .iter()
            .enumerate()
            .map(|(i, e)| (e.key.clone(), i))
            .collect();
        self.armor_index = self
            .armor
            .iter()
            .enumerate()
            .map(|(i, e)| (e.key.clone(), i))
            .collect();
        self.accessory_index = self
            .accessories
            .iter()
            .enumerate()
            .map(|(i, e)| (e.key.clone(), i))
            .collect();
    }

    /// All arts.
    pub fn arts(&self) -> &[Art] {
        &self.arts
    }

    /// Arts owned by a specific character.
    pub fn arts_for(&self, character: Character) -> impl Iterator<Item = &Art> {
        self.arts.iter().filter(move |a| a.character == character)
    }

    /// Find an art by exact direction-byte sequence.
    pub fn find_art_by_directions(&self, character: Character, dirs: &[u8]) -> Option<&Art> {
        self.arts
            .iter()
            .find(|a| a.character == character && a.directions == dirs)
    }

    /// Find an art by case-insensitive name match.
    pub fn find_art_by_name(&self, name: &str) -> Option<&Art> {
        let name = name.trim().to_ascii_lowercase();
        self.arts
            .iter()
            .find(|a| a.name.to_ascii_lowercase() == name)
    }

    /// AP cost for an art identified by character + action constant.
    /// `None` if no art is registered for this slot.
    pub fn ap_cost_for_action(&self, character: Character, action_constant: u8) -> Option<u32> {
        self.arts
            .iter()
            .find(|a| a.character == character && a.action_constant == Some(action_constant))
            .map(|a| a.ap)
    }

    /// All spells.
    pub fn spells(&self) -> &[Spell] {
        &self.spells
    }

    /// Lookup a spell by case-insensitive name match.
    pub fn spell_by_name(&self, name: &str) -> Option<&Spell> {
        let name = name.trim().to_ascii_lowercase();
        self.spells
            .iter()
            .find(|s| s.name.to_ascii_lowercase() == name)
    }

    /// Filter spells by element (lowercase).
    pub fn spells_by_element(&self, element: &str) -> impl Iterator<Item = &Spell> {
        let elem = element.trim().to_ascii_lowercase();
        self.spells.iter().filter(move |s| s.element == elem)
    }

    /// All items.
    pub fn items(&self) -> &[Item] {
        &self.items
    }

    /// Look up an item by stable key.
    pub fn item(&self, key: &str) -> Option<&Item> {
        self.item_index.get(key).map(|&i| &self.items[i])
    }

    /// All weapons.
    pub fn weapons(&self) -> &[Weapon] {
        &self.weapons
    }

    /// Look up a weapon by key.
    pub fn weapon(&self, key: &str) -> Option<&Weapon> {
        self.weapon_index.get(key).map(|&i| &self.weapons[i])
    }

    /// All armor.
    pub fn armor(&self) -> &[Armor] {
        &self.armor
    }

    /// Look up armor by key.
    pub fn armor_piece(&self, key: &str) -> Option<&Armor> {
        self.armor_index.get(key).map(|&i| &self.armor[i])
    }

    /// All accessories.
    pub fn accessories(&self) -> &[Accessory] {
        &self.accessories
    }

    /// Look up an accessory by key.
    pub fn accessory(&self, key: &str) -> Option<&Accessory> {
        self.accessory_index.get(key).map(|&i| &self.accessories[i])
    }

    /// All enemies.
    pub fn enemies(&self) -> &[Enemy] {
        &self.enemies
    }

    /// Find an enemy by exact name match (case-insensitive, ignores Lv suffix differences).
    pub fn enemy_by_name(&self, name: &str) -> Option<&Enemy> {
        let name = name.trim().to_ascii_lowercase();
        self.enemies
            .iter()
            .find(|e| e.name.to_ascii_lowercase() == name)
    }

    /// All boss-HP estimates.
    pub fn bosses(&self) -> &[Boss] {
        &self.bosses
    }

    /// All shops.
    pub fn shops(&self) -> &[Shop] {
        &self.shops
    }

    /// All shops in a specific town.
    pub fn shops_in(&self, town: &str) -> impl Iterator<Item = &Shop> {
        self.shops.iter().filter(move |s| s.town == town)
    }

    /// Resolve a shop's inventory keys against the unified item lookup.
    /// Unknown keys are silently dropped (the `data_files_resolve_all_shop_keys`
    /// test catches that).
    pub fn shop_inventory(&self, town: &str, name: &str) -> Option<Vec<ResolvedShopEntry<'_>>> {
        let shop = self.shops.iter().find(|s| {
            s.town == town
                && (s.name.as_deref() == Some(name) || s.merchant.as_deref() == Some(name))
        })?;
        Some(self.resolve_inventory(&shop.inventory))
    }

    /// Resolve a list of item keys against the unified item lookup.
    pub fn resolve_inventory(&self, keys: &[String]) -> Vec<ResolvedShopEntry<'_>> {
        keys.iter().filter_map(|k| self.resolve_key(k)).collect()
    }

    /// Resolve a single item key.
    pub fn resolve_key(&self, key: &str) -> Option<ResolvedShopEntry<'_>> {
        if let Some(it) = self.item(key) {
            return Some(ResolvedShopEntry {
                key: &it.key,
                name: &it.name,
                price: it.price,
                category: ShopEntryCategory::Item,
            });
        }
        if let Some(w) = self.weapon(key) {
            return Some(ResolvedShopEntry {
                key: &w.key,
                name: &w.name,
                price: w.price,
                category: ShopEntryCategory::Weapon,
            });
        }
        if let Some(a) = self.armor_piece(key) {
            return Some(ResolvedShopEntry {
                key: &a.key,
                name: &a.name,
                price: a.price,
                category: ShopEntryCategory::Armor,
            });
        }
        if let Some(ac) = self.accessory(key) {
            return Some(ResolvedShopEntry {
                key: &ac.key,
                name: &ac.name,
                price: ac.price,
                category: ShopEntryCategory::Accessory,
            });
        }
        None
    }

    /// All slot prizes.
    pub fn slot_prizes(&self) -> &[SlotPrize] {
        &self.slot_prizes
    }

    /// All Muscle Dome courses.
    pub fn muscle_dome(&self) -> &[MuscleDomeCourse] {
        &self.muscle_dome
    }

    /// All Baka Fighter rounds.
    pub fn baka_fighter(&self) -> &[BakaFighterRound] {
        &self.baka_fighter
    }

    /// All fishing prizes.
    pub fn fishing_prizes(&self) -> &[FishingPrize] {
        &self.fishing
    }

    /// All character profiles.
    pub fn characters(&self) -> &[CharacterProfile] {
        &self.characters
    }
}

impl Default for Database {
    fn default() -> Self {
        Self::load()
    }
}

/// Magic-leveling tables (per-spell XP curve + damage scaling).
///
/// Spells level up with use: each cast awards `0..=12` xp (single-target)
/// or `0..=4` per target (multi-target), scaled by what fraction of the
/// target's max HP the cast inflicted. Hitting a level threshold below
/// permanently upgrades the spell. Source: Meth962 v1.10 "Magic Levels
/// & Experience" section.
pub mod magic_leveling {
    /// Cumulative XP required to *reach* level `N` (N = 2..=9).
    ///
    /// `XP_TO_LEVEL[0]` = cost of Lv1 -> Lv2 cumulative threshold (= 18).
    /// Spells start at Lv1 with 0 xp.
    pub const XP_TO_LEVEL: [u32; 8] = [18, 51, 93, 145, 209, 289, 393, 537];

    /// Damage / heal scaling multiplier per spell level.
    ///
    /// Lv1 = 1.0 (base). Lv9 = 2.0 (+100%). Stored as basis points
    /// (1/100 of a percent) for exact integer math: 10000 = 100%.
    ///
    /// Lv1 entry (10000 = no bonus) is included so the index matches
    /// the spell level directly (`SCALING_BP[level - 1]`).
    pub const SCALING_BP: [u32; 9] = [
        10000, // Lv1: base 100%
        11248, // Lv2: +12.48%
        12499, // Lv3: +24.99%
        13750, // Lv4: +37.50%
        15000, // Lv5: +50.00%
        16250, // Lv6: +62.50%
        17500, // Lv7: +75.00%
        18750, // Lv8: +87.50%
        20000, // Lv9: +100%
    ];

    /// Vera per-level heal amount (HP restored on cast).
    pub const VERA_HEAL: [u32; 9] = [256, 288, 320, 352, 384, 416, 448, 480, 512];

    /// Orb per-level heal amount (AoE).
    pub const ORB_HEAL: [u32; 9] = [512, 576, 640, 704, 768, 832, 896, 960, 1024];

    /// Spoon per-level heal amount (AoE, post-Sol).
    pub const SPOON_HEAL: [u32; 9] = [1024, 1152, 1280, 1408, 1536, 1664, 1792, 1920, 2048];

    /// Nighto per-level base success chance (%) for its Confuse / Death
    /// status roll. Confuse:Death ratio is fixed at 8:1 across all levels.
    pub const NIGHTO_HIT_PCT: [u32; 9] = [50, 53, 56, 60, 64, 69, 75, 82, 90];

    /// Single-target damage XP award table.
    ///
    /// Returns 0..=12 xp depending on what fraction of the target's max
    /// HP the cast inflicted. Buckets are 8.5% / 17.5% / 25% / 33.5% /
    /// 42% / 50.5% / 57.5% / 66% / 74.5% / 83% / 91.5% / 100% (with the
    /// last bucket also covering kill-shots on partially-damaged
    /// targets, for Gimard).
    pub fn xp_single_target(hp_fraction_pct: u32) -> u32 {
        const THRESHOLDS: [(u32, u32); 13] = [
            (100, 12),
            (92, 11), // >= 91.5
            (83, 10),
            (75, 9), // >= 74.5
            (66, 8),
            (58, 7), // >= 57.5
            (51, 6), // >= 50.5
            (42, 5),
            (34, 4), // >= 33.5
            (25, 3),
            (18, 2), // >= 17.5
            (9, 1),  // >= 8.5
            (0, 0),
        ];
        for &(threshold, xp) in &THRESHOLDS {
            if hp_fraction_pct >= threshold {
                return xp;
            }
        }
        0
    }

    /// Multi-target damage XP award (per enemy hit).
    pub fn xp_multi_target(hp_fraction_pct: u32) -> u32 {
        match hp_fraction_pct {
            100.. => 4,
            75..=99 => 3, // >= 74.5
            51..=74 => 2, // >= 50.5
            25..=50 => 1, // >= 24.99
            _ => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn database_loads() {
        let db = Database::load();
        assert!(!db.arts().is_empty());
        assert!(!db.spells().is_empty());
        assert!(!db.items().is_empty());
        assert!(!db.weapons().is_empty());
        assert!(!db.armor().is_empty());
        assert!(!db.accessories().is_empty());
        assert!(!db.enemies().is_empty());
        assert!(!db.shops().is_empty());
    }

    #[test]
    fn art_command_byte_mapping_round_trip() {
        let db = Database::load();
        for art in db.arts() {
            for (token, &expected) in art.command.iter().zip(art.directions.iter()) {
                let mapped = art
                    .character
                    .token_to_byte(token)
                    .unwrap_or_else(|| panic!("token {:?} for {:?}", token, art.character));
                assert_eq!(
                    mapped, expected,
                    "art {:?} ({:?}) command token {:?} maps to {} not {}",
                    art.name, art.character, token, mapped, expected
                );
            }
        }
    }

    #[test]
    fn vahn_hyper_elbow_lookup() {
        let db = Database::load();
        let art = db
            .find_art_by_directions(Character::Vahn, &[1, 2, 4])
            .expect("Hyper Elbow");
        assert_eq!(art.name, "Hyper Elbow");
        assert_eq!(art.ap, 18);
        assert_eq!(art.kind, ArtKind::Regular);
    }

    #[test]
    fn ap_cost_for_action_constant() {
        let db = Database::load();
        // Vahn 0x29 = Hyper Elbow = 18 AP
        assert_eq!(db.ap_cost_for_action(Character::Vahn, 0x29), Some(18));
        // Gala 0x1B = Biron Rage = 99 AP
        assert_eq!(db.ap_cost_for_action(Character::Gala, 0x1B), Some(99));
    }

    #[test]
    fn magic_counts() {
        let db = Database::load();
        let seru: Vec<_> = db
            .spells()
            .iter()
            .filter(|s| s.family == SpellFamily::Seru)
            .collect();
        let ra_seru: Vec<_> = db
            .spells()
            .iter()
            .filter(|s| s.family == SpellFamily::RaSeru)
            .collect();
        assert_eq!(seru.len(), 21, "expected 21 Seru spells");
        assert_eq!(ra_seru.len(), 8, "expected 8 Ra-Seru summons");
    }

    #[test]
    fn resolve_rim_elm_shop() {
        let db = Database::load();
        let entries = db
            .shop_inventory("Rim Elm", "Variety Shop")
            .expect("Rim Elm shop");
        let leaf = entries.iter().find(|e| e.key == "healing_leaf").unwrap();
        assert_eq!(leaf.name, "Healing Leaf");
        assert_eq!(leaf.price, Some(100));
        assert_eq!(leaf.category, ShopEntryCategory::Item);
    }
}
