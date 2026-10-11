//! Per-character record at runtime offset `0x80084708 + n * 0x414`.
//!
//! PORT: FUN_80042558, FUN_80042DBC, FUN_800432BC, FUN_800431FC, FUN_80043264
//!
//! Field offsets come from `docs/subsystems/battle.md`'s "Character record
//! layout" section, cross-referenced with the inventory / spell helpers
//! `FUN_80042558` / `FUN_80042DBC` / `FUN_800432BC` / `FUN_800431FC` /
//! `FUN_80043264`. Offsets that aren't documented are kept verbatim in
//! [`CharacterRecord::raw`] so a round-trip preserves them.
//!
//! All offsets are relative to the record's base (`0x80084708 + n*0x414`).
//! Note the **display name** is at internal offset [`NAME_OFFSET`] (`+0x2A7`),
//! not the record start - the start holds an undocumented field. In the
//! retail SC save block the names are therefore visible at `game_data +
//! 0x66F + n*0x414`, which is *not* the record base; see
//! [`crate::card::RETAIL_CHAR_RECORD_HEADER_SIZE`].

/// Byte size of one character record.
pub const CHARACTER_RECORD_SIZE: usize = 0x414;

/// Maximum spell entries the spell list at `+0x13C..0x160` can hold.
pub const MAX_SPELLS: usize = 36;

/// Length of the active-abilities bitfield at `+0xF4..+0x104` (four u32 words).
pub const ABILITY_BITS_LEN: usize = 16;

/// Record offset of the character level byte (`+0x130`) - the retail
/// displayed level. See [`CharacterRecord::level`].
pub const LEVEL_OFFSET: usize = 0x130;

/// Stride between active-spell-slot entries at `+0x2B0..0x380`.
const ACTIVE_SPELL_SLOT_STRIDE: usize = 0x14;

/// Maximum number of active-spell slots that fit in `+0x2B0..0x380`.
const MAX_ACTIVE_SPELL_SLOTS: usize = (0x380 - 0x2B0) / ACTIVE_SPELL_SLOT_STRIDE;

/// Equipment slot count at `+0x196..0x19D`.
const EQUIPMENT_SLOT_COUNT: usize = 8;

/// Length of one auto command string (`FUN_801DA34C`'s `sltiu a0,0x10` copy
/// bound at `0x801DA408`).
pub const AUTO_COMMAND_STRING_LEN: usize = 16;

/// Record-relative offset of the primary auto command string
/// ([`AutoCommandBand::Primary`]).
pub const AUTO_COMMAND_STRING_A_OFFSET: usize = 0x1A7;

/// Record-relative offset of the secondary auto command string
/// ([`AutoCommandBand::Secondary`]).
pub const AUTO_COMMAND_STRING_B_OFFSET: usize = 0x1B7;

/// Maximum entries the displayed-skill list at `+0x185..+0x196` can hold.
/// The byte at `+0x185` is a count; the 16 bytes at `+0x186..+0x196` are
/// the parallel ID array. The menu reader at `0x801D4440` (in the menu
/// overlay) caps display at 7 entries; the on-record array is sized for
/// 16 to fill the gap to the equipment-slot field at `+0x196`.
pub const MAX_DISPLAYED_SKILLS: usize = 16;

/// Byte offset of the display-name field within a character record.
///
/// The NUL-padded ASCII name (`Vahn` / `Noa` / `Gala` / `Terra`, or the
/// player-entered lead name) sits here - pinned across six in-game RAM
/// captures at live RAM `0x80084708 + n*0x414 + 0x2A7` for all four roster
/// slots. The field is bounded by the active-spell-slot table at `+0x2B0`,
/// so it holds at most [`NAME_LEN`] bytes. In the retail SC save block this
/// lands at `game_data + 0x66F + n*0x414` (SC `+0x86F` for slot 0), which is
/// where the names are visible - see [`crate::card::RETAIL_CHAR_RECORD_HEADER_SIZE`].
pub const NAME_OFFSET: usize = 0x2A7;

/// Length of the display-name field (`+0x2A7..+0x2B0`).
pub const NAME_LEN: usize = 9;

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

/// HP / MP / SP triplet: each pair is (**maximum, current**) at
/// `+0x104..0x110` - `+0x104/+0x108/+0x10C` hold the effective maxima and
/// `+0x106/+0x10A/+0x10E` the current values.
///
/// Three independent sources pin the order: the walk-regen `FUN_801D0B90`
/// bumps `+0x106` clamping at `+0x104`; the per-frame stat aggregator
/// `FUN_80042558` rewrites `+0x104` from base stats + %-passives (a
/// per-frame recompute cannot be current HP) and caps each value at
/// [`STAT_CAP`]; GameShark "Infinite HP" codes write `+0x106` at every
/// character stride. (Fresh-save fixtures mask the distinction - current ==
/// maximum at seed - which is how the reversed labels survived initially.)
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct HpMpSp {
    /// Current HP (`+0x106`).
    pub hp_cur: u16,
    /// Maximum HP (`+0x104`).
    pub hp_max: u16,
    /// Current MP (`+0x10A`).
    pub mp_cur: u16,
    /// Maximum MP (`+0x108`).
    pub mp_max: u16,
    /// Current SP (`+0x10E`).
    pub sp_cur: u16,
    /// Maximum SP (`+0x10C`).
    pub sp_max: u16,
}

/// Equipment-slot bytes at `+0x196..0x19D`, 8 slots.
///
/// Retail's order is **body armour, head, then a per-character weapon
/// byte**: the hub equip sub-panel resolves a slot as `(byte +7 & 0x60) >> 5`
/// and the equip screen's row map `DAT_801E43E8` reads `00 01 00 04 05 06 07`,
/// which together put armour at 0, head at 1 and footwear at 4. The weapon
/// sits at byte 2 or 3 per `_DAT_8007B42C` (`2, 3, 2` for Vahn / Noa / Gala).
/// It is *not* weapon-first - `legaia_engine_menus::equipment::EquipSlot` uses
/// a different, engine-defined order and re-maps before walking a record.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EquipmentSlots {
    /// Raw 8 slot bytes; semantic mapping is engine-defined.
    pub slots: [u8; EQUIPMENT_SLOT_COUNT],
}

/// The two 16-byte **auto command strings** a character record carries at
/// `+0x1A7` and `+0x1B7`.
///
/// These are the battle command menu's "Auto" source: the arts-input string
/// the player last confirmed, replayed verbatim the next time the character
/// attacks without entering arrows. Retail addresses them off the live-state
/// window rather than the record base - `0x80084140 + slot*0x414 + 0x76F` and
/// `+0x77F` - and the record base sits `0x5C8` bytes into that window, which
/// is what makes them record-relative `+0x1A7` / `+0x1B7`.
///
/// There are two because the string is banded by the character's action
/// gauge: the reader picks between them on `actor[+0x156] < actor[+0x154]`
/// (base gauge below live gauge), so a combo recorded with a topped-up gauge
/// is not replayed into a turn that cannot pay for it.
///
/// Loaded by `FUN_801DA34C` and written back by `FUN_801DA59C`
/// (`see ghidra/scripts/funcs/overlay_battle_action_801da34c.txt`,
/// `overlay_battle_action_801da59c.txt`); both copy exactly
/// [`AUTO_COMMAND_STRING_LEN`] bytes into / out of battle-actor `+0x1DF`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoCommandBand {
    /// Record `+0x1A7` - the slot the reader takes when the base action gauge
    /// is strictly below the live one.
    Primary,
    /// Record `+0x1B7` - the slot the reader takes otherwise. Retail's
    /// asymmetry lives here: the primary leg falls back to this slot when its
    /// own head byte is zero, but this leg has **no** fallback to the primary
    /// one and zero-fills instead.
    Secondary,
}

impl AutoCommandBand {
    /// Record-relative byte offset of this band's string.
    pub const fn offset(self) -> usize {
        match self {
            AutoCommandBand::Primary => AUTO_COMMAND_STRING_A_OFFSET,
            AutoCommandBand::Secondary => AUTO_COMMAND_STRING_B_OFFSET,
        }
    }

    /// The band `FUN_801DA34C` / `FUN_801DA59C` select for an actor whose
    /// live action gauge is `agl` (`+0x154`) over a base of `agl_base`
    /// (`+0x156`): `sltu (base, live)` at `0x801DA3A4` / `0x801DA5E0`.
    pub const fn for_gauge(agl: u16, agl_base: u16) -> Self {
        if agl_base < agl {
            AutoCommandBand::Primary
        } else {
            AutoCommandBand::Secondary
        }
    }
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
/// [`legaia_parity::capture_observations`]) shows this field is the
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

    /// Active-abilities bitfield at `+0xF4..+0x104` (four u32 words). The runtime ORs this
    /// into the global 4×u32 mask at `0x80074358..0x80074368` per-frame
    /// via `FUN_80042558`.
    pub fn ability_bits(&self) -> [u8; ABILITY_BITS_LEN] {
        let mut out = [0u8; ABILITY_BITS_LEN];
        out.copy_from_slice(&self.raw[0xF4..0xF4 + ABILITY_BITS_LEN]);
        out
    }

    /// Replace the active-abilities bitfield at `+0xF4..+0x104`.
    pub fn set_ability_bits(&mut self, bits: [u8; ABILITY_BITS_LEN]) {
        self.raw[0xF4..0xF4 + ABILITY_BITS_LEN].copy_from_slice(&bits);
    }

    /// HP / MP / SP triplet at `+0x104..0x110` (six u16s LE; each pair is
    /// `(max, cur)` - see [`HpMpSp`] for the three-source provenance).
    pub fn hp_mp_sp(&self) -> HpMpSp {
        let r = |off: usize| u16::from_le_bytes([self.raw[off], self.raw[off + 1]]);
        HpMpSp {
            hp_max: r(0x104),
            hp_cur: r(0x106),
            mp_max: r(0x108),
            mp_cur: r(0x10A),
            sp_max: r(0x10C),
            sp_cur: r(0x10E),
        }
    }

    /// Replace the HP / MP / SP triplet.
    pub fn set_hp_mp_sp(&mut self, hms: HpMpSp) {
        let mut w = |off: usize, v: u16| {
            self.raw[off..off + 2].copy_from_slice(&v.to_le_bytes());
        };
        w(0x104, hms.hp_max);
        w(0x106, hms.hp_cur);
        w(0x108, hms.mp_max);
        w(0x10A, hms.mp_cur);
        w(0x10C, hms.sp_max);
        w(0x10E, hms.sp_cur);
    }

    /// Stat-cap field at `+0x120` (u16 LE) - the record-side per-stat cap
    /// constant, the same field [`RecordStats::cap_constant`] exposes.
    /// Retail holds [`RECORD_CAP_CONSTANT`] (`100`) here in every captured
    /// save; the cheat classifier pins the offset as
    /// `stat_cap_constant_100(+0x120)`. The `999` clamp the runtime applies
    /// per frame is the *code* constant [`STAT_CAP`] in `FUN_80042558`, not
    /// this field. (An earlier revision of this accessor read `+0x11A`,
    /// which is the live INT stat - see [`LiveStats::int`].)
    pub fn stat_cap(&self) -> u16 {
        u16::from_le_bytes([self.raw[0x120], self.raw[0x121]])
    }

    /// Replace the stat-cap field at `+0x120`.
    pub fn set_stat_cap(&mut self, value: u16) {
        self.raw[0x120..0x122].copy_from_slice(&value.to_le_bytes());
    }

    /// Display name, decoded from the NUL-padded ASCII field at
    /// [`NAME_OFFSET`]. Bytes from the first NUL on are dropped; non-ASCII
    /// bytes are passed through as their raw `char` value.
    pub fn name(&self) -> String {
        let field = &self.raw[NAME_OFFSET..NAME_OFFSET + NAME_LEN];
        let end = field.iter().position(|&b| b == 0).unwrap_or(field.len());
        field[..end].iter().map(|&b| b as char).collect()
    }

    /// Write the display name into [`NAME_OFFSET`], truncated to [`NAME_LEN`]
    /// bytes and NUL-padded.
    pub fn set_name(&mut self, name: &str) {
        let field = &mut self.raw[NAME_OFFSET..NAME_OFFSET + NAME_LEN];
        field.fill(0);
        for (dst, b) in field.iter_mut().zip(name.bytes()) {
            *dst = b;
        }
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

    /// One of the two [`AutoCommandBand`] strings (`+0x1A7` / `+0x1B7`,
    /// [`AUTO_COMMAND_STRING_LEN`] bytes each).
    ///
    /// The bytes are battle-action command constants exactly as the actor's
    /// `+0x1DF` stream holds them; a leading `0` means the band has never
    /// been written, which is the condition `FUN_801DA34C` branches on.
    pub fn auto_command_string(&self, band: AutoCommandBand) -> [u8; AUTO_COMMAND_STRING_LEN] {
        let base = band.offset();
        let mut out = [0u8; AUTO_COMMAND_STRING_LEN];
        out.copy_from_slice(&self.raw[base..base + AUTO_COMMAND_STRING_LEN]);
        out
    }

    /// Overwrite one [`AutoCommandBand`] string - the write-back half
    /// (`FUN_801DA59C`, `sb` loops at `0x801DA638` / `0x801DA69C`). Retail
    /// overwrites exactly one band per call and never clears the other.
    pub fn set_auto_command_string(
        &mut self,
        band: AutoCommandBand,
        bytes: [u8; AUTO_COMMAND_STRING_LEN],
    ) {
        let base = band.offset();
        self.raw[base..base + AUTO_COMMAND_STRING_LEN].copy_from_slice(&bytes);
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

    /// Cumulative XP at `+0x00..+0x04` (u32 LE).
    ///
    /// The status renderer `FUN_801D33D8` draws the "Experience" line from
    /// this word verbatim (`0x80084708 + slot*0x414 + 0x0`), and the level-up
    /// applier `FUN_801E9504` compares it against the per-level thresholds.
    /// Validated across the save-state library (New Game 0 → L37 `518_462` →
    /// L99 `9_664_511`). An earlier reading placed a u16 XP field at `+0x04` -
    /// that offset is actually the low half of [`Self::next_level_xp`].
    /// Feed into [`crate::level_for_cumulative_xp`] to derive the (base-curve)
    /// character level.
    pub fn cumulative_xp(&self) -> u32 {
        u32::from_le_bytes(self.raw[0x00..0x04].try_into().unwrap())
    }

    /// Replace the cumulative-XP field.
    pub fn set_cumulative_xp(&mut self, xp: u32) {
        self.raw[0x00..0x04].copy_from_slice(&xp.to_le_bytes());
    }

    /// Next-level XP threshold at `+0x04..+0x08` (u32 LE) - the cumulative XP
    /// total at which the character reaches the next level.
    ///
    /// Written by the level-up applier `FUN_801E9504` (base curve for slots
    /// 0/3, ± sin-divisor correction for Noa/Gala) and drawn **verbatim** as
    /// the Status-menu "Next Level" number by `FUN_801D33D8`
    /// (`0x80084708 + slot*0x414 + 0x4`; New Game Vahn = 121, Noa = 102,
    /// Gala = 140). Zero at L99 (no next level).
    pub fn next_level_xp(&self) -> u32 {
        u32::from_le_bytes(self.raw[0x04..0x08].try_into().unwrap())
    }

    /// Replace the next-level XP threshold field.
    pub fn set_next_level_xp(&mut self, threshold: u32) {
        self.raw[0x04..0x08].copy_from_slice(&threshold.to_le_bytes());
    }

    /// The character level - the retail displayed-level byte at `+0x130`
    /// ([`LEVEL_OFFSET`]).
    ///
    /// This is the one level cell: the status screen's "LV", the GameShark
    /// `Level 99` target, and what the engine's level-up sync, cheats and AP
    /// gauge read and write. [`Self::magic_rank`] is a legacy alias of the
    /// same byte. An earlier revision kept a separate engine-only level at
    /// `+0x100`, but that byte is word 3 of the ability bitfield
    /// (`+0xF4..+0x103`, see [`Self::ability_bits`]), which retail's
    /// aggregator - and the engine's port of it - zeroes and rebuilds from
    /// equipment on every pass, so a level stored there read back as `0`.
    pub fn level(&self) -> u8 {
        self.raw[LEVEL_OFFSET]
    }

    /// Replace the character level (`+0x130`; see [`Self::level`]).
    pub fn set_level(&mut self, level: u8) {
        self.raw[LEVEL_OFFSET] = level;
    }

    /// Byte at `+0x130` (u8) - the **retail displayed character level** (legacy
    /// accessor name).
    ///
    /// Boot-confirmed via the starting-level randomizer: this is the byte the status
    /// screen reads as "LV" and the GameShark `Level 99` code targets - a New Game
    /// record with level-10 cumulative experience (`+0x0`), level-10 stats, and the
    /// right next-level threshold (`+0x4`) but `+0x130 == 1` still shows LV 1, and
    /// `+0x130 = 10` shows LV 10. The retail level-up applier maintains it `+1` per
    /// level-up event (the captured Noa/Gala four-level jumps bumped it by one, so it
    /// can lag the XP-derived level after a rare multi-level grant). The earlier
    /// "Magic Rank, level derived from XP" reading is superseded for the level
    /// question, and the adjacent `+0x131` is not the magic-rank byte: the new-game
    /// seed's `sb $v0, 0x6f9($s0)` at `0x800561C8` is its only writer on the disc and
    /// nothing reads it, while the magic-rank counter is capture-pinned at record
    /// `+0x9C`. See `docs/formats/save-record.md`.
    pub fn magic_rank(&self) -> u8 {
        self.level()
    }

    /// Replace the `+0x130` level byte (legacy alias of [`Self::set_level`]).
    pub fn set_magic_rank(&mut self, rank: u8) {
        self.set_level(rank);
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
    /// `legaia_engine_system::ram_map` for the engine-side cell.
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
    /// Stat-cap constant at `+0x120` (the same field as
    /// `record_stats.cap_constant`; retail value 100).
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
    /// Equipment slots at `+0x196` (length `EQUIPMENT_SLOT_COUNT`).
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

    /// Build a [`Party`] from a retail SC save block (the 8 KiB block whose
    /// first two bytes are [`crate::SAVE_BLOCK_MAGIC`]).
    ///
    /// Walks [`crate::card::read_retail_char_records`] and parses each
    /// returned record. Stops at the first all-zero slot, so a partial
    /// roster (e.g. Vahn alone in the prologue) returns members in the
    /// order they appear on disc.
    ///
    /// `max_records` caps the walk - retail saves never hold more than 4
    /// active records (Vahn / Noa / Gala / Terra).
    pub fn from_retail_sc_block(sc_block: &[u8], max_records: usize) -> anyhow::Result<Self> {
        let raw = crate::card::read_retail_char_records(sc_block, max_records)
            .ok_or_else(|| anyhow::anyhow!("SC block too small to hold the record header"))?;
        let mut members = Vec::with_capacity(raw.len());
        for (i, bytes) in raw.iter().enumerate() {
            members.push(
                CharacterRecord::parse(bytes)
                    .map_err(|e| anyhow::anyhow!("character record {i}: {e}"))?,
            );
        }
        Ok(Self { members })
    }
}

#[cfg(test)]
mod tests;
