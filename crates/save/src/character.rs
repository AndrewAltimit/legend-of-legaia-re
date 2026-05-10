//! Per-character record at runtime offset `0x80084708 + n * 0x414`.
//!
//! Field offsets come from `docs/subsystems/battle.md`'s "Character record
//! layout" section, cross-referenced with the inventory / spell helpers
//! `FUN_80042558` / `FUN_80042DBC` / `FUN_800432BC` / `FUN_800431FC` /
//! `FUN_80043264`. Offsets that aren't documented are kept verbatim in
//! [`CharacterRecord::raw`] so a round-trip preserves them.

/// Byte size of one character record.
pub const CHARACTER_RECORD_SIZE: usize = 0x414;

/// Maximum spell entries the spell list at `+0x13C..0x160` can hold.
pub const MAX_SPELLS: usize = 36;

/// Length of the active-abilities bitfield at `+0xF4..0x100`.
pub const ABILITY_BITS_LEN: usize = 16;

/// Stride between active-spell-slot entries at `+0x2B0..0x380`.
const ACTIVE_SPELL_SLOT_STRIDE: usize = 0x14;

/// Maximum number of active-spell slots that fit in `+0x2B0..0x380`.
const MAX_ACTIVE_SPELL_SLOTS: usize = (0x380 - 0x2B0) / ACTIVE_SPELL_SLOT_STRIDE;

/// Equipment slot count at `+0x196..0x19D`.
const EQUIPMENT_SLOT_COUNT: usize = 8;

/// Maximum entries the displayed-skill list at `+0x185..+0x196` can hold.
/// The byte at `+0x185` is a count; the 16 bytes at `+0x186..+0x196` are
/// the parallel ID array. The menu reader at `0x801D4440` (in the menu
/// overlay) caps display at 7 entries; the on-record array is sized for
/// 16 to fill the gap to the equipment-slot field at `+0x196`.
pub const MAX_DISPLAYED_SKILLS: usize = 16;

/// Stat-cap clamp value the runtime applies via `FUN_80042558`.
pub const STAT_CAP: u16 = 0x3E7;

/// Per-stat record-side cap constant the engine writes to `+0x120`
/// during a level-up event. Empirically `100` for every captured
/// character and every captured save state. Documented at
/// `docs/formats/save-record.md` "+0x120 cap constant" section.
pub const RECORD_CAP_CONSTANT: u16 = 100;

/// Number of "live" stats stored at `+0x110..+0x11B` (six u16s LE).
/// Mirrored at `+0x122..+0x12D` as the record-side copy.
pub const LIVE_STAT_COUNT: usize = 6;

/// Number of summon-level bytes at `+0x161..+0x178` (16 bytes,
/// one per summon ID). Pinned by the GameShark "All Summons Level 9"
/// cheat which sets every byte in this window to `0x09`.
pub const SUMMON_SLOT_COUNT: usize = 16;

/// Number of magic-slot groups at `+0x13D..+0x160`. Each group is
/// 12 bytes (one byte per spell within the group). Pinned by the
/// "Magic Modifier N" cheat family which has three repeating
/// instances per character at offsets `+0x13D / +0x149 / +0x155`.
pub const MAGIC_GROUP_COUNT: usize = 3;
/// Byte stride between adjacent magic-slot groups.
pub const MAGIC_GROUP_STRIDE: usize = 12;

// --- Sub-structs -----------------------------------------------------------

/// HP / MP / SP triplet: each pair is (current, maximum) at `+0x104..0x110`.
///
/// Offsets verified against `FUN_80042558` (the per-frame stat aggregator
/// that caps each value at [`STAT_CAP`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct HpMpSp {
    /// Current HP.
    pub hp_cur: u16,
    /// Maximum HP.
    pub hp_max: u16,
    /// Current MP.
    pub mp_cur: u16,
    /// Maximum MP.
    pub mp_max: u16,
    /// Current SP.
    pub sp_cur: u16,
    /// Maximum SP.
    pub sp_max: u16,
}

/// Equipment-slot bytes at `+0x196..0x19D`. 8 slots - typically
/// (weapon, armour, helmet, ring, accessory_1..3, currency-slot).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EquipmentSlots {
    /// Raw 8 slot bytes; semantic mapping is engine-defined.
    pub slots: [u8; EQUIPMENT_SLOT_COUNT],
}

/// Spell list at `+0x13C..0x184`. The first byte at `+0x13C` is the count;
/// `+0x13D..+0x160` (36 bytes) is the spell-ID array; `+0x161..+0x184`
/// (36 bytes) is the parallel level / experience array.
///
/// Both arrays are kept full-length even when `count < MAX_SPELLS` so a
/// round-trip preserves any trailing bytes the runtime may have left
/// uninitialised.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpellList {
    /// Number of valid entries in `ids` / `levels` (range 0..=36).
    pub count: u8,
    /// Spell IDs (only `count` entries are semantically valid).
    pub ids: [u8; MAX_SPELLS],
    /// Parallel level / experience bytes.
    pub levels: [u8; MAX_SPELLS],
}

impl Default for SpellList {
    fn default() -> Self {
        Self {
            count: 0,
            ids: [0; MAX_SPELLS],
            levels: [0; MAX_SPELLS],
        }
    }
}

/// Displayed-skill list at `+0x185..+0x196` - the menu's spell / Hyper Art
/// display roster the per-character menu overlay (`FUN_801D33D8`) reads on
/// each frame.
///
/// `count` at `+0x185` is the number of valid entries; `ids[..count]` are
/// indices into the menu's static skill table at `0x801E472C` (stride 0x14;
/// each record carries a name pointer at `+0xC` and a sort key at `+0x0`).
///
/// The Fire Book I capture (`vahn_fire_book_use` in
/// [`legaia_engine_core::capture_observations`]) shows this field is the
/// learn signal the runtime updates when an item teaches a Hyper Art:
/// `count` increments by one and the new ID is inserted at position 0
/// (head insert). The menu reader caps rendering at 7 entries, but the
/// on-record array is sized for `MAX_DISPLAYED_SKILLS = 16` to fill the
/// gap to the equipment slots at `+0x196`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayedSkillList {
    /// Number of valid entries in `ids` (range `0..=MAX_DISPLAYED_SKILLS`).
    pub count: u8,
    /// Skill IDs (only `count` entries are semantically valid; the rest of
    /// the array is preserved verbatim on round-trip).
    pub ids: [u8; MAX_DISPLAYED_SKILLS],
}

impl Default for DisplayedSkillList {
    fn default() -> Self {
        Self {
            count: 0,
            ids: [0; MAX_DISPLAYED_SKILLS],
        }
    }
}

/// The six "live" stats at `+0x110..+0x11B` (six u16s LE), in
/// the order `(AGL, ATK, UDF, LDF, SPD, INT)`.
///
/// The runtime reads these for moment-to-moment battle resolution.
/// On level-up the engine writes the record-side copy at
/// `+0x122..+0x12D` first, then mirrors it into this window.
///
/// Pinned by the GameShark "Max AGL / Max ATK / ..." cheats which
/// each carry two address sites per character (one in this window,
/// one in the record-side window).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct LiveStats {
    /// Agility (`+0x110`).
    pub agl: u16,
    /// Attack (`+0x112`).
    pub atk: u16,
    /// Up-defence (`+0x114`).
    pub udf: u16,
    /// Down-defence (`+0x116`).
    pub ldf: u16,
    /// Speed (`+0x118`).
    pub spd: u16,
    /// Intelligence (`+0x11A`).
    pub int: u16,
}

/// The record-side stat window at `+0x11C..+0x12D` - a 9 × u16 view
/// the engine writes during level-up before mirroring into the live
/// window at `+0x110..+0x11B`.
///
/// Pinned across the captured Noa + Gala 4-level triplets and
/// cross-checked against the GameShark "Max HP / Max MP / Max ATK / ..."
/// cheats which target both the live AND the record copies.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct RecordStats {
    /// Maximum HP (`+0x11C`).
    pub hp_max: u16,
    /// Maximum MP (`+0x11E`).
    pub mp_max: u16,
    /// Per-stat cap constant. Always `100` in captured saves
    /// (`+0x120`). See [`RECORD_CAP_CONSTANT`].
    pub cap_constant: u16,
    /// Agility (`+0x122`).
    pub agl: u16,
    /// Attack (`+0x124`).
    pub atk: u16,
    /// Up-defence (`+0x126`).
    pub udf: u16,
    /// Down-defence (`+0x128`).
    pub ldf: u16,
    /// Speed (`+0x12A`).
    pub spd: u16,
    /// Intelligence (`+0x12C`).
    pub int: u16,
}

// --- Top-level character record -------------------------------------------

/// One character's runtime state - 0x414 bytes.
///
/// The struct exposes the documented fields as typed getters / setters
/// while keeping the full raw byte buffer in [`raw`]. Use [`parse`] to
/// read a buffer; mutate the typed fields; call [`write`] to get the
/// updated 0x414-byte buffer back.
///
/// [`parse`]: CharacterRecord::parse
/// [`write`]: CharacterRecord::write
/// [`raw`]: CharacterRecord::raw
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharacterRecord {
    /// Full 0x414-byte buffer. Typed accessors read/write through here.
    /// Round-trip preserves every byte not touched by typed setters.
    pub raw: Vec<u8>,
}

impl CharacterRecord {
    /// Build a fully-zero record. Typed fields all read as default values
    /// until the caller writes through them.
    pub fn zeroed() -> Self {
        Self {
            raw: vec![0u8; CHARACTER_RECORD_SIZE],
        }
    }

    /// Parse a 0x414-byte buffer. Errors if the buffer is the wrong size.
    pub fn parse(buf: &[u8]) -> anyhow::Result<Self> {
        if buf.len() != CHARACTER_RECORD_SIZE {
            anyhow::bail!(
                "character record must be {} bytes; got {}",
                CHARACTER_RECORD_SIZE,
                buf.len()
            );
        }
        Ok(Self { raw: buf.to_vec() })
    }

    /// Serialise the record back to its 0x414-byte representation.
    /// Returns the underlying buffer directly (allocating a copy).
    pub fn write(&self) -> Vec<u8> {
        debug_assert_eq!(self.raw.len(), CHARACTER_RECORD_SIZE);
        self.raw.clone()
    }

    // --- Typed views -----------------------------------------------------

    /// Active-abilities bitfield at `+0xF4..0x100`. The runtime ORs this
    /// into the global 4×u32 mask at `0x80074358..0x80074368` per-frame
    /// via `FUN_80042558`.
    pub fn ability_bits(&self) -> [u8; ABILITY_BITS_LEN] {
        let mut out = [0u8; ABILITY_BITS_LEN];
        out.copy_from_slice(&self.raw[0xF4..0xF4 + ABILITY_BITS_LEN]);
        out
    }

    /// Replace the active-abilities bitfield at `+0xF4..0x100`.
    pub fn set_ability_bits(&mut self, bits: [u8; ABILITY_BITS_LEN]) {
        self.raw[0xF4..0xF4 + ABILITY_BITS_LEN].copy_from_slice(&bits);
    }

    /// HP / MP / SP triplet at `+0x104..0x110` (six u16s LE).
    pub fn hp_mp_sp(&self) -> HpMpSp {
        let r = |off: usize| u16::from_le_bytes([self.raw[off], self.raw[off + 1]]);
        HpMpSp {
            hp_cur: r(0x104),
            hp_max: r(0x106),
            mp_cur: r(0x108),
            mp_max: r(0x10A),
            sp_cur: r(0x10C),
            sp_max: r(0x10E),
        }
    }

    /// Replace the HP / MP / SP triplet.
    pub fn set_hp_mp_sp(&mut self, hms: HpMpSp) {
        let mut w = |off: usize, v: u16| {
            self.raw[off..off + 2].copy_from_slice(&v.to_le_bytes());
        };
        w(0x104, hms.hp_cur);
        w(0x106, hms.hp_max);
        w(0x108, hms.mp_cur);
        w(0x10A, hms.mp_max);
        w(0x10C, hms.sp_cur);
        w(0x10E, hms.sp_max);
    }

    /// Stat-cap field at `+0x11A` (u16). The runtime clamps several
    /// computed stats to this value.
    pub fn stat_cap(&self) -> u16 {
        u16::from_le_bytes([self.raw[0x11A], self.raw[0x11B]])
    }

    /// Replace the stat-cap field.
    pub fn set_stat_cap(&mut self, value: u16) {
        self.raw[0x11A..0x11C].copy_from_slice(&value.to_le_bytes());
    }

    /// Spell list at `+0x13C..0x184`.
    pub fn spell_list(&self) -> SpellList {
        let count = self.raw[0x13C];
        let mut ids = [0u8; MAX_SPELLS];
        let mut levels = [0u8; MAX_SPELLS];
        ids.copy_from_slice(&self.raw[0x13D..0x13D + MAX_SPELLS]);
        levels.copy_from_slice(&self.raw[0x161..0x161 + MAX_SPELLS]);
        SpellList { count, ids, levels }
    }

    /// Replace the spell list. The count clamps at [`MAX_SPELLS`].
    pub fn set_spell_list(&mut self, list: SpellList) {
        self.raw[0x13C] = list.count.min(MAX_SPELLS as u8);
        self.raw[0x13D..0x13D + MAX_SPELLS].copy_from_slice(&list.ids);
        self.raw[0x161..0x161 + MAX_SPELLS].copy_from_slice(&list.levels);
    }

    /// Displayed-skill list at `+0x185..0x196`. Read by the menu overlay's
    /// per-character skill renderer (loop at `0x801D4440`); written by the
    /// item-use battle event (Fire Book I and friends) when an item teaches
    /// a Hyper Art / spell.
    pub fn displayed_skills(&self) -> DisplayedSkillList {
        let count = self.raw[0x185];
        let mut ids = [0u8; MAX_DISPLAYED_SKILLS];
        ids.copy_from_slice(&self.raw[0x186..0x186 + MAX_DISPLAYED_SKILLS]);
        DisplayedSkillList { count, ids }
    }

    /// Replace the displayed-skill list. The count clamps at
    /// [`MAX_DISPLAYED_SKILLS`].
    pub fn set_displayed_skills(&mut self, list: DisplayedSkillList) {
        self.raw[0x185] = list.count.min(MAX_DISPLAYED_SKILLS as u8);
        self.raw[0x186..0x186 + MAX_DISPLAYED_SKILLS].copy_from_slice(&list.ids);
    }

    /// Equipment-slot bytes at `+0x196..0x19D`.
    pub fn equipment(&self) -> EquipmentSlots {
        let mut slots = [0u8; EQUIPMENT_SLOT_COUNT];
        slots.copy_from_slice(&self.raw[0x196..0x196 + EQUIPMENT_SLOT_COUNT]);
        EquipmentSlots { slots }
    }

    /// Replace the equipment-slot bytes.
    pub fn set_equipment(&mut self, eq: EquipmentSlots) {
        self.raw[0x196..0x196 + EQUIPMENT_SLOT_COUNT].copy_from_slice(&eq.slots);
    }

    /// Read one active-spell slot at `+0x2B0 + slot * 0x14`. Returns the
    /// raw 0x14 bytes; semantic interpretation (which byte is spell ID,
    /// which is duration, etc.) is engine-defined.
    pub fn active_spell_slot(&self, slot: usize) -> Option<[u8; ACTIVE_SPELL_SLOT_STRIDE]> {
        if slot >= MAX_ACTIVE_SPELL_SLOTS {
            return None;
        }
        let base = 0x2B0 + slot * ACTIVE_SPELL_SLOT_STRIDE;
        let mut out = [0u8; ACTIVE_SPELL_SLOT_STRIDE];
        out.copy_from_slice(&self.raw[base..base + ACTIVE_SPELL_SLOT_STRIDE]);
        Some(out)
    }

    /// Write one active-spell slot. Returns `false` if `slot` is out of
    /// range.
    pub fn set_active_spell_slot(
        &mut self,
        slot: usize,
        bytes: [u8; ACTIVE_SPELL_SLOT_STRIDE],
    ) -> bool {
        if slot >= MAX_ACTIVE_SPELL_SLOTS {
            return false;
        }
        let base = 0x2B0 + slot * ACTIVE_SPELL_SLOT_STRIDE;
        self.raw[base..base + ACTIVE_SPELL_SLOT_STRIDE].copy_from_slice(&bytes);
        true
    }

    /// Cumulative XP at `+0x04..+0x06` (u16 LE).
    ///
    /// Pinned by the captured magic-rank-up + character-level-up save
    /// triplet: the level-up event at L9 → L10 grew the value at this
    /// offset by `+365`, exactly the per-level XP increment for L9→10
    /// in the retail XP table at
    /// `SCUS_942.54 0x8007123C`. Returns the cumulative XP value the engine
    /// can feed into [`crate::level_for_cumulative_xp`] to derive the
    /// character level.
    pub fn cumulative_xp(&self) -> u16 {
        u16::from_le_bytes([self.raw[0x04], self.raw[0x05]])
    }

    /// Replace the cumulative-XP field.
    pub fn set_cumulative_xp(&mut self, xp: u16) {
        self.raw[0x04..0x06].copy_from_slice(&xp.to_le_bytes());
    }

    /// "Magic Rank" / level field at `+0x130` (u8).
    ///
    /// Captured Noa + Gala level-up triplets show this byte ticking
    /// `+1` per level-up event regardless of how many character
    /// levels were granted; the GameShark `Level 99` cheat for each
    /// character sets the same byte to `0x63`. The two readings are
    /// reconciled by treating this byte as the **Magic Rank** (not
    /// the character level): single-level XP gains tick rank +1
    /// per event, and the cheat-set value of 99 unlocks every
    /// magic / summon at maximum power without affecting the
    /// character's Tactical Arts level. The character's combat
    /// level is derived from cumulative XP via
    /// [`crate::level_for_cumulative_xp`].
    pub fn magic_rank(&self) -> u8 {
        self.raw[0x130]
    }

    /// Replace the magic-rank field.
    pub fn set_magic_rank(&mut self, rank: u8) {
        self.raw[0x130] = rank;
    }

    /// Live stats at `+0x110..+0x11B`, in `(AGL, ATK, UDF, LDF,
    /// SPD, INT)` order.
    pub fn live_stats(&self) -> LiveStats {
        let r = |off: usize| u16::from_le_bytes([self.raw[off], self.raw[off + 1]]);
        LiveStats {
            agl: r(0x110),
            atk: r(0x112),
            udf: r(0x114),
            ldf: r(0x116),
            spd: r(0x118),
            int: r(0x11A),
        }
    }

    /// Replace the live-stats window.
    pub fn set_live_stats(&mut self, s: LiveStats) {
        let mut w = |off: usize, v: u16| {
            self.raw[off..off + 2].copy_from_slice(&v.to_le_bytes());
        };
        w(0x110, s.agl);
        w(0x112, s.atk);
        w(0x114, s.udf);
        w(0x116, s.ldf);
        w(0x118, s.spd);
        w(0x11A, s.int);
    }

    /// Record-side stats at `+0x11C..+0x12D` (9 × u16 LE).
    /// Engine writes these first on level-up, then mirrors into
    /// [`Self::live_stats`].
    pub fn record_stats(&self) -> RecordStats {
        let r = |off: usize| u16::from_le_bytes([self.raw[off], self.raw[off + 1]]);
        RecordStats {
            hp_max: r(0x11C),
            mp_max: r(0x11E),
            cap_constant: r(0x120),
            agl: r(0x122),
            atk: r(0x124),
            udf: r(0x126),
            ldf: r(0x128),
            spd: r(0x12A),
            int: r(0x12C),
        }
    }

    /// Replace the record-side stats window.
    pub fn set_record_stats(&mut self, s: RecordStats) {
        let mut w = |off: usize, v: u16| {
            self.raw[off..off + 2].copy_from_slice(&v.to_le_bytes());
        };
        w(0x11C, s.hp_max);
        w(0x11E, s.mp_max);
        w(0x120, s.cap_constant);
        w(0x122, s.agl);
        w(0x124, s.atk);
        w(0x126, s.udf);
        w(0x128, s.ldf);
        w(0x12A, s.spd);
        w(0x12C, s.int);
    }

    /// Per-character summon-level bytes at `+0x161..+0x178` (16
    /// bytes; one byte per summon ID 0..=15).
    ///
    /// Pinned by the GameShark "All Summons Level 9" cheat which
    /// stamps `0x09` into every byte in this window. Distinct from
    /// the per-character spell-level array (which the prior
    /// `SpellList::levels` field misidentified as).
    pub fn summon_levels(&self) -> [u8; SUMMON_SLOT_COUNT] {
        let mut out = [0u8; SUMMON_SLOT_COUNT];
        out.copy_from_slice(&self.raw[0x161..0x161 + SUMMON_SLOT_COUNT]);
        out
    }

    /// Replace the per-summon-level array.
    pub fn set_summon_levels(&mut self, levels: [u8; SUMMON_SLOT_COUNT]) {
        self.raw[0x161..0x161 + SUMMON_SLOT_COUNT].copy_from_slice(&levels);
    }

    /// Per-magic-group spell IDs at `+0x13D..+0x160` - three
    /// 12-byte groups in source order. The byte at `+0x13C` is the
    /// "Magic Slot Activator" gate (pinned by the cheat of the same
    /// name; setting it to `0x24` enables every spell slot).
    pub fn magic_groups(&self) -> [[u8; MAGIC_GROUP_STRIDE]; MAGIC_GROUP_COUNT] {
        let mut out = [[0u8; MAGIC_GROUP_STRIDE]; MAGIC_GROUP_COUNT];
        for (i, group) in out.iter_mut().enumerate() {
            let base = 0x13D + i * MAGIC_GROUP_STRIDE;
            group.copy_from_slice(&self.raw[base..base + MAGIC_GROUP_STRIDE]);
        }
        out
    }

    /// Replace the magic-group spell IDs.
    pub fn set_magic_groups(&mut self, groups: [[u8; MAGIC_GROUP_STRIDE]; MAGIC_GROUP_COUNT]) {
        for (i, group) in groups.iter().enumerate() {
            let base = 0x13D + i * MAGIC_GROUP_STRIDE;
            self.raw[base..base + MAGIC_GROUP_STRIDE].copy_from_slice(group);
        }
    }

    /// "Magic Slot Activator" byte at `+0x13C`. The cheat database
    /// sets this to `0x24` to enable the magic-slot system; the
    /// engine reads it as a count / gate when assembling the menu.
    pub fn magic_slot_activator(&self) -> u8 {
        self.raw[0x13C]
    }

    /// Replace the magic-slot-activator byte.
    pub fn set_magic_slot_activator(&mut self, value: u8) {
        self.raw[0x13C] = value;
    }

    /// Game-time field accessor lives outside this record (game time
    /// is a party-wide global at `0x80084570`); see
    /// [`legaia_engine_core::ram_map`] for the engine-side cell.
    /// Provided here as a doc anchor for cross-reference.
    pub const GAME_TIME_GLOBAL_ADDR: u32 = 0x80084570;

    /// Typed snapshot of every documented field - convenient for JSON
    /// dumps. Fields not in [`Snapshot`] still pass through `write` via
    /// the underlying [`Self::raw`] buffer.
    pub fn snapshot(&self) -> Snapshot {
        let spells = self.spell_list();
        let displayed = self.displayed_skills();
        let equip = self.equipment();
        Snapshot {
            ability_bits: self.ability_bits().to_vec(),
            hp_mp_sp: self.hp_mp_sp(),
            stat_cap: self.stat_cap(),
            live_stats: self.live_stats(),
            record_stats: self.record_stats(),
            magic_rank: self.magic_rank(),
            magic_slot_activator: self.magic_slot_activator(),
            summon_levels: self.summon_levels().to_vec(),
            spell_count: spells.count,
            spell_ids: spells.ids.to_vec(),
            spell_levels: spells.levels.to_vec(),
            displayed_skill_count: displayed.count,
            displayed_skill_ids: displayed.ids.to_vec(),
            equipment_slots: equip.slots.to_vec(),
        }
    }
}

/// JSON-friendly snapshot of every documented character-record field.
/// Round-trip is via [`CharacterRecord::write`] from the underlying
/// raw bytes - this struct is for diagnostics, not serialisation.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Snapshot {
    /// Active-abilities bitfield at `+0xF4`.
    pub ability_bits: Vec<u8>,
    /// HP / MP / SP triplet at `+0x104`.
    pub hp_mp_sp: HpMpSp,
    /// Stat cap at `+0x11A`. NB: this offset overlaps the live INT
    /// stat in the cheat-derived layout; treat with care.
    pub stat_cap: u16,
    /// Live stats at `+0x110..+0x11B` (AGL, ATK, UDF, LDF, SPD, INT).
    pub live_stats: LiveStats,
    /// Record-side stats at `+0x11C..+0x12D`.
    pub record_stats: RecordStats,
    /// Magic-rank byte at `+0x130`.
    pub magic_rank: u8,
    /// Magic-slot-activator byte at `+0x13C`.
    pub magic_slot_activator: u8,
    /// Per-summon-level bytes at `+0x161..+0x178`.
    pub summon_levels: Vec<u8>,
    /// Spell-list count at `+0x13C`.
    pub spell_count: u8,
    /// Spell IDs at `+0x13D` (length [`MAX_SPELLS`]).
    pub spell_ids: Vec<u8>,
    /// Spell levels at `+0x161` (length [`MAX_SPELLS`]).
    pub spell_levels: Vec<u8>,
    /// Displayed-skill-list count at `+0x185`.
    pub displayed_skill_count: u8,
    /// Displayed-skill IDs at `+0x186` (length [`MAX_DISPLAYED_SKILLS`]).
    pub displayed_skill_ids: Vec<u8>,
    /// Equipment slots at `+0x196` (length [`EQUIPMENT_SLOT_COUNT`]).
    pub equipment_slots: Vec<u8>,
}

// --- Party wrapper --------------------------------------------------------

/// A roster of N character records - wraps the per-character offsets
/// computed from a base address (`0x80084708 + n * 0x414` in retail, but
/// the typed crate doesn't bake that in; engines hold the records however
/// they like).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Party {
    /// Members in slot order. Stride between two adjacent members'
    /// underlying buffers is exactly [`CHARACTER_RECORD_SIZE`].
    pub members: Vec<CharacterRecord>,
}

impl Party {
    /// Build a fresh party of `count` zeroed records.
    pub fn zeroed(count: usize) -> Self {
        Self {
            members: (0..count).map(|_| CharacterRecord::zeroed()).collect(),
        }
    }

    /// Parse a contiguous `count * 0x414`-byte buffer into a party.
    /// Errors if the buffer length isn't a multiple of [`CHARACTER_RECORD_SIZE`].
    pub fn parse(buf: &[u8]) -> anyhow::Result<Self> {
        if buf.is_empty() || !buf.len().is_multiple_of(CHARACTER_RECORD_SIZE) {
            anyhow::bail!(
                "party buffer must be a non-zero multiple of {} bytes; got {}",
                CHARACTER_RECORD_SIZE,
                buf.len()
            );
        }
        let count = buf.len() / CHARACTER_RECORD_SIZE;
        let mut members = Vec::with_capacity(count);
        for i in 0..count {
            let off = i * CHARACTER_RECORD_SIZE;
            members.push(CharacterRecord::parse(
                &buf[off..off + CHARACTER_RECORD_SIZE],
            )?);
        }
        Ok(Self { members })
    }

    /// Serialise the party back to a contiguous buffer of length
    /// `members.len() * 0x414`.
    pub fn write(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.members.len() * CHARACTER_RECORD_SIZE);
        for m in &self.members {
            out.extend_from_slice(&m.raw);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cumulative_xp_round_trip() {
        let mut r = CharacterRecord::zeroed();
        r.set_cumulative_xp(365);
        assert_eq!(r.cumulative_xp(), 365);
        // Verify the bytes land at +0x04..+0x06 in LE.
        assert_eq!(r.raw[0x04], 0x6D);
        assert_eq!(r.raw[0x05], 0x01);
    }

    #[test]
    fn cumulative_xp_default_is_zero() {
        let r = CharacterRecord::zeroed();
        assert_eq!(r.cumulative_xp(), 0);
    }

    #[test]
    fn zeroed_record_round_trips_to_all_zeros() {
        let r = CharacterRecord::zeroed();
        let bytes = r.write();
        assert_eq!(bytes.len(), CHARACTER_RECORD_SIZE);
        assert!(bytes.iter().all(|&b| b == 0));
    }

    #[test]
    fn parse_rejects_wrong_size() {
        assert!(CharacterRecord::parse(&[0u8; 0x100]).is_err());
        assert!(CharacterRecord::parse(&[0u8; 0x500]).is_err());
        assert!(CharacterRecord::parse(&[]).is_err());
    }

    #[test]
    fn parse_then_write_round_trips_arbitrary_bytes() {
        // Synthesise a buffer with a recognisable pattern in every byte
        // so any field-level corruption shows up in the comparison.
        let buf: Vec<u8> = (0..CHARACTER_RECORD_SIZE)
            .map(|i| ((i * 37) ^ 0x5A) as u8)
            .collect();
        let r = CharacterRecord::parse(&buf).unwrap();
        let out = r.write();
        assert_eq!(buf, out);
    }

    #[test]
    fn typed_setters_round_trip_through_raw_bytes() {
        let mut r = CharacterRecord::zeroed();
        let hms = HpMpSp {
            hp_cur: 250,
            hp_max: 999,
            mp_cur: 30,
            mp_max: 80,
            sp_cur: 10,
            sp_max: 50,
        };
        r.set_hp_mp_sp(hms);
        r.set_stat_cap(STAT_CAP);
        let bits = [0xAB; ABILITY_BITS_LEN];
        r.set_ability_bits(bits);
        let eq = EquipmentSlots {
            slots: [0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17],
        };
        r.set_equipment(eq);
        let mut ids = [0u8; MAX_SPELLS];
        ids[..5].copy_from_slice(&[1, 2, 3, 4, 5]);
        let mut levels = [0u8; MAX_SPELLS];
        levels[..5].copy_from_slice(&[10, 20, 30, 40, 50]);
        let spells = SpellList {
            count: 5,
            ids,
            levels,
        };
        r.set_spell_list(spells);

        // Round-trip via parse/write.
        let bytes = r.write();
        let r2 = CharacterRecord::parse(&bytes).unwrap();
        assert_eq!(r2.hp_mp_sp(), hms);
        assert_eq!(r2.stat_cap(), STAT_CAP);
        assert_eq!(r2.ability_bits(), bits);
        assert_eq!(r2.equipment(), eq);
        let s2 = r2.spell_list();
        assert_eq!(s2.count, 5);
        assert_eq!(&s2.ids[..5], &[1, 2, 3, 4, 5]);
        assert_eq!(&s2.levels[..5], &[10, 20, 30, 40, 50]);
    }

    #[test]
    fn typed_setters_preserve_untouched_bytes() {
        // Fill the record with a non-zero pattern, then write only ONE
        // typed field - every other byte must survive unchanged.
        let mut buf = vec![0xCC; CHARACTER_RECORD_SIZE];
        // Pre-zero the byte we'll write through so the comparison is clean.
        buf[0x11A] = 0;
        buf[0x11B] = 0;
        let mut r = CharacterRecord::parse(&buf).unwrap();
        r.set_stat_cap(STAT_CAP);
        let out = r.write();
        // stat_cap bytes should reflect the new value.
        assert_eq!(&out[0x11A..0x11C], &STAT_CAP.to_le_bytes());
        // Every other byte should be the original 0xCC.
        for (i, &b) in out.iter().enumerate() {
            if !(0x11A..0x11C).contains(&i) {
                assert_eq!(b, 0xCC, "byte at offset 0x{i:X} corrupted by set_stat_cap");
            }
        }
    }

    /// The Fire Book I capture pair (battle command menu parked on Fire
    /// Book I → Fire Book I just used on Vahn) shows Vahn's record
    /// changing at `+0x185..+0x188` from `[0x01, 0x0C, 0x00]` to
    /// `[0x02, 0x03, 0x0C]`. The typed accessor must read those exact
    /// bytes back as the same list shape (count + head-inserted ID).
    #[test]
    fn displayed_skills_reads_fire_book_capture_pattern() {
        let mut r = CharacterRecord::zeroed();
        // Pre-event: count=1, ids=[0x0C, ...].
        r.raw[0x185] = 0x01;
        r.raw[0x186] = 0x0C;
        let before = r.displayed_skills();
        assert_eq!(before.count, 1);
        assert_eq!(before.ids[0], 0x0C);
        assert_eq!(before.ids[1], 0x00);

        // Post-event: count=2, ids=[0x03, 0x0C, ...] (head insert).
        r.raw[0x185] = 0x02;
        r.raw[0x186] = 0x03;
        r.raw[0x187] = 0x0C;
        let after = r.displayed_skills();
        assert_eq!(after.count, 2);
        assert_eq!(after.ids[0], 0x03);
        assert_eq!(after.ids[1], 0x0C);
        assert_eq!(after.count - before.count, 1);
    }

    /// Setter writes back to `+0x185..+0x196` and round-trips through `parse`.
    #[test]
    fn displayed_skills_round_trip() {
        let mut r = CharacterRecord::zeroed();
        let mut ids = [0u8; MAX_DISPLAYED_SKILLS];
        ids[..3].copy_from_slice(&[0x03, 0x0C, 0x1F]);
        let list = DisplayedSkillList { count: 3, ids };
        r.set_displayed_skills(list);
        assert_eq!(r.raw[0x185], 0x03);
        assert_eq!(&r.raw[0x186..0x186 + 3], &[0x03, 0x0C, 0x1F]);
        let bytes = r.write();
        let r2 = CharacterRecord::parse(&bytes).unwrap();
        let read_back = r2.displayed_skills();
        assert_eq!(read_back.count, 3);
        assert_eq!(read_back.ids[..3], [0x03, 0x0C, 0x1F]);
    }

    /// Count clamps at `MAX_DISPLAYED_SKILLS` to keep the byte field
    /// within the +0x185..+0x196 window before the equipment slots.
    #[test]
    fn displayed_skills_count_clamps_at_max() {
        let mut r = CharacterRecord::zeroed();
        let list = DisplayedSkillList {
            count: 200,
            ids: [0xAA; MAX_DISPLAYED_SKILLS],
        };
        r.set_displayed_skills(list);
        assert_eq!(r.raw[0x185], MAX_DISPLAYED_SKILLS as u8);
        // The equipment-slot byte at +0x196 must not be clobbered.
        assert_eq!(r.raw[0x196], 0x00);
    }

    #[test]
    fn active_spell_slot_round_trips_and_rejects_out_of_range() {
        let mut r = CharacterRecord::zeroed();
        let payload = [0x42u8; 0x14];
        assert!(r.set_active_spell_slot(0, payload));
        assert!(r.set_active_spell_slot(MAX_ACTIVE_SPELL_SLOTS - 1, payload));
        assert!(!r.set_active_spell_slot(MAX_ACTIVE_SPELL_SLOTS, payload));
        assert_eq!(r.active_spell_slot(0), Some(payload));
        assert_eq!(r.active_spell_slot(MAX_ACTIVE_SPELL_SLOTS), None);
    }

    #[test]
    fn party_round_trip_preserves_every_member() {
        let mut p = Party::zeroed(4);
        for (i, m) in p.members.iter_mut().enumerate() {
            m.set_stat_cap((i as u16 + 1) * 100);
            let spells = SpellList {
                count: i as u8,
                ..SpellList::default()
            };
            m.set_spell_list(spells);
        }
        let bytes = p.write();
        assert_eq!(bytes.len(), 4 * CHARACTER_RECORD_SIZE);
        let p2 = Party::parse(&bytes).unwrap();
        assert_eq!(p2.members.len(), 4);
        for (i, m) in p2.members.iter().enumerate() {
            assert_eq!(m.stat_cap(), (i as u16 + 1) * 100);
            assert_eq!(m.spell_list().count, i as u8);
        }
    }

    #[test]
    fn party_parse_rejects_misaligned_buffer() {
        assert!(Party::parse(&[]).is_err());
        assert!(Party::parse(&[0u8; 0x100]).is_err());
        assert!(Party::parse(&[0u8; 0x415]).is_err());
    }

    /// The "Max AGL / ATK / UDF / LDF / SPD / INT" GameShark cheats
    /// each have two address sites per character: one in the live
    /// window (`+0x110..+0x11B`) and one in the record window
    /// (`+0x122..+0x12D`). Verify that writing the live window and
    /// reading it back round-trips through the typed accessor.
    #[test]
    fn live_stats_round_trip() {
        let mut r = CharacterRecord::zeroed();
        let s = LiveStats {
            agl: 0x3E7,
            atk: 0x3E7,
            udf: 0x3E7,
            ldf: 0x3E7,
            spd: 0x3E7,
            int: 0x3E7,
        };
        r.set_live_stats(s);
        let read_back = r.live_stats();
        assert_eq!(read_back, s);
        // The bytes should land at the cheat-pinned offsets.
        assert_eq!(
            &r.raw[0x110..0x11C],
            &[
                0xE7, 0x03, 0xE7, 0x03, 0xE7, 0x03, 0xE7, 0x03, 0xE7, 0x03, 0xE7, 0x03,
            ]
        );
    }

    /// Record-side stats round-trip including the cap constant at
    /// `+0x120`.
    #[test]
    fn record_stats_round_trip_pins_cap_constant() {
        let mut r = CharacterRecord::zeroed();
        let s = RecordStats {
            hp_max: 0x270F,
            mp_max: 0x03E7,
            cap_constant: RECORD_CAP_CONSTANT,
            agl: 50,
            atk: 60,
            udf: 70,
            ldf: 80,
            spd: 90,
            int: 100,
        };
        r.set_record_stats(s);
        assert_eq!(r.record_stats(), s);
        // Cap constant lands at +0x120 LE.
        assert_eq!(&r.raw[0x120..0x122], &[100, 0]);
    }

    /// The "Level 99" GameShark cheat writes `0x63` to `+0x130`.
    /// Our accessor calls this byte `magic_rank`; the test asserts
    /// the read/write reaches the same bytes the cheat targets.
    #[test]
    fn magic_rank_writes_byte_at_offset_0x130() {
        let mut r = CharacterRecord::zeroed();
        r.set_magic_rank(99);
        assert_eq!(r.raw[0x130], 99);
        assert_eq!(r.magic_rank(), 99);
    }

    /// "All Summons Level 9" stamps `0x09` into 16 bytes at
    /// `+0x161..+0x178`.
    #[test]
    fn summon_levels_round_trip_16_bytes() {
        let mut r = CharacterRecord::zeroed();
        let levels = [0x09u8; SUMMON_SLOT_COUNT];
        r.set_summon_levels(levels);
        assert_eq!(r.summon_levels(), levels);
        assert!(
            r.raw[0x161..0x161 + SUMMON_SLOT_COUNT]
                .iter()
                .all(|&b| b == 0x09)
        );
        // Bytes outside the window must be untouched.
        assert_eq!(r.raw[0x160], 0);
        assert_eq!(r.raw[0x161 + SUMMON_SLOT_COUNT], 0);
    }

    /// Magic-slot-activator gate at `+0x13C`. Cheat sets to 0x24.
    #[test]
    fn magic_slot_activator_writes_byte_at_offset_0x13c() {
        let mut r = CharacterRecord::zeroed();
        r.set_magic_slot_activator(0x24);
        assert_eq!(r.raw[0x13C], 0x24);
        assert_eq!(r.magic_slot_activator(), 0x24);
    }

    /// Magic groups: three 12-byte windows starting at `+0x13D`.
    #[test]
    fn magic_groups_round_trip_three_windows() {
        let mut r = CharacterRecord::zeroed();
        let mut groups = [[0u8; MAGIC_GROUP_STRIDE]; MAGIC_GROUP_COUNT];
        for (i, group) in groups.iter_mut().enumerate() {
            for (j, slot) in group.iter_mut().enumerate() {
                *slot = (i as u8) * 16 + (j as u8) + 1;
            }
        }
        r.set_magic_groups(groups);
        let read_back = r.magic_groups();
        assert_eq!(read_back, groups);
        // First byte of group 0 lands at +0x13D.
        assert_eq!(r.raw[0x13D], 1);
        // First byte of group 1 lands at +0x149.
        assert_eq!(r.raw[0x149], 17);
        // First byte of group 2 lands at +0x155.
        assert_eq!(r.raw[0x155], 33);
        // Last byte of group 2 lands at +0x160.
        assert_eq!(r.raw[0x160], 33 + 11);
    }

    /// Snapshot now includes the cheat-derived fields.
    #[test]
    fn snapshot_includes_new_cheat_derived_fields() {
        let mut r = CharacterRecord::zeroed();
        r.set_magic_rank(42);
        r.set_magic_slot_activator(0x24);
        r.set_record_stats(RecordStats {
            cap_constant: RECORD_CAP_CONSTANT,
            ..RecordStats::default()
        });
        let snap = r.snapshot();
        assert_eq!(snap.magic_rank, 42);
        assert_eq!(snap.magic_slot_activator, 0x24);
        assert_eq!(snap.record_stats.cap_constant, RECORD_CAP_CONSTANT);
        assert_eq!(snap.summon_levels.len(), SUMMON_SLOT_COUNT);
    }
}
