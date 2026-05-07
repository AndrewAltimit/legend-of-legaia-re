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

/// Stat-cap clamp value the runtime applies via `FUN_80042558`.
pub const STAT_CAP: u16 = 0x3E7;

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

/// Equipment-slot bytes at `+0x196..0x19D`. 8 slots — typically
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

// --- Top-level character record -------------------------------------------

/// One character's runtime state — 0x414 bytes.
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

    /// Typed snapshot of every documented field — convenient for JSON
    /// dumps. Fields not in [`Snapshot`] still pass through `write` via
    /// the underlying [`Self::raw`] buffer.
    pub fn snapshot(&self) -> Snapshot {
        let spells = self.spell_list();
        let equip = self.equipment();
        Snapshot {
            ability_bits: self.ability_bits().to_vec(),
            hp_mp_sp: self.hp_mp_sp(),
            stat_cap: self.stat_cap(),
            spell_count: spells.count,
            spell_ids: spells.ids.to_vec(),
            spell_levels: spells.levels.to_vec(),
            equipment_slots: equip.slots.to_vec(),
        }
    }
}

/// JSON-friendly snapshot of every documented character-record field.
/// Round-trip is via [`CharacterRecord::write`] from the underlying
/// raw bytes — this struct is for diagnostics, not serialisation.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Snapshot {
    /// Active-abilities bitfield at `+0xF4`.
    pub ability_bits: Vec<u8>,
    /// HP / MP / SP triplet at `+0x104`.
    pub hp_mp_sp: HpMpSp,
    /// Stat cap at `+0x11A`.
    pub stat_cap: u16,
    /// Spell-list count at `+0x13C`.
    pub spell_count: u8,
    /// Spell IDs at `+0x13D` (length [`MAX_SPELLS`]).
    pub spell_ids: Vec<u8>,
    /// Spell levels at `+0x161` (length [`MAX_SPELLS`]).
    pub spell_levels: Vec<u8>,
    /// Equipment slots at `+0x196` (length [`EQUIPMENT_SLOT_COUNT`]).
    pub equipment_slots: Vec<u8>,
}

// --- Party wrapper --------------------------------------------------------

/// A roster of N character records — wraps the per-character offsets
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
        // typed field — every other byte must survive unchanged.
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
}
