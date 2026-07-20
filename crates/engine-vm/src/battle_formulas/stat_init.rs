//! Battle-load stat initialisation: the party actor's `+0x14C..+0x176` block
//! (`FUN_80053CB8`) and the equipment bonus fold it applies. Split out of
//! `battle_formulas.rs`.
//!
//! The monster-side sibling is `FUN_80054CB0`; its hit-reaction tag map is
//! ported in `legaia_asset::monster_archive::reaction_map`.

/// The five equipment slots a party character carries (character record
/// `+0x196..+0x19A`), in slot order.
pub const EQUIP_SLOTS: usize = 5;

/// The equipment stat-bonus record (`DAT_80074F68`, 8-byte stride) as far as
/// battle-load init reads it.
///
/// The record's first two bytes are the INT (`+0`) and ATK (`+1`) bonuses.
/// **Battle-load init does not read them** - see [`equip_stat_bonuses`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EquipBonus {
    /// `+2` - upper-body defence bonus.
    pub udf: u8,
    /// `+3` - lower-body defence bonus.
    pub ldf: u8,
    /// `+4` - speed bonus.
    pub spd: u8,
}

/// The three stat bonuses battle-load folds out of a character's five equipped
/// items, as `(udf, ldf, spd)` deltas.
///
/// **The trap:** the equipment table also carries INT (`+0`) and ATK (`+1`)
/// bonuses, and `FUN_80053CB8` folds **neither**. A weapon's attack bonus never
/// reaches the battle actor through this path - the actor's ATK base
/// (`+0x15A`) stays the raw character-record value. A port that folds all five
/// fields "for symmetry" inflates every party member's damage.
///
/// Retail also applies **no** `kind == 1` item-class guard here, though the
/// menu's stat-preview path (`FUN_801CF650`) does. So a slot holding a
/// non-equipment id would show one number in the menu and apply another in
/// battle. Retail data appears never to reach that state, but the asymmetry is
/// real and a randomizer that loosens equip validity can expose it.
///
/// Adds wrap like retail (`u16`, no clamp).
///
/// NOT WIRED: no engine caller. `engine-core` seeds party battle stats through
/// its own `seed_party_battle_stats`, which has not been moved onto this kernel;
/// until it is, this is a verified transcription with no runtime effect.
///
/// PORT: FUN_80053cb8 (the equipment-bonus loop)
pub fn equip_stat_bonuses(equipped: &[Option<EquipBonus>; EQUIP_SLOTS]) -> (u16, u16, u16) {
    let mut udf: u16 = 0;
    let mut ldf: u16 = 0;
    let mut spd: u16 = 0;
    for slot in equipped.iter().flatten() {
        udf = udf.wrapping_add(u16::from(slot.udf));
        ldf = ldf.wrapping_add(u16::from(slot.ldf));
        spd = spd.wrapping_add(u16::from(slot.spd));
    }
    (udf, ldf, spd)
}

/// The character-record stats battle-load reads, at their record-relative
/// offsets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RecordStats {
    /// `+0x104` HP max.
    pub hp_max: u16,
    /// `+0x106` HP current.
    pub hp_cur: u16,
    /// `+0x108` MP max.
    pub mp_max: u16,
    /// `+0x10A` MP current.
    pub mp_cur: u16,
    /// `+0x10E` spirit / SP.
    pub spirit: u16,
    /// `+0x110` AGL.
    pub agl: u16,
    /// `+0x112` ATK.
    pub atk: u16,
    /// `+0x114` UDF (upper defence).
    pub udf: u16,
    /// `+0x116` LDF (lower defence).
    pub ldf: u16,
    /// `+0x118` SPD.
    pub spd: u16,
    /// `+0x11A` INT.
    pub int: u16,
}

/// The battle actor's stat block (`+0x14C..+0x176`) as battle-load leaves it.
///
/// Every buffable stat is stored as a **pair** of adjacent halfwords: the lower
/// offset is the working value the formulas read, the higher is the base a buff
/// restores to. Battle-load seeds both to the same value, so `working == base`
/// at round zero.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BattleActorStats {
    /// `+0x14C` HP current.
    pub hp: u16,
    /// `+0x14E` HP max.
    pub hp_max: u16,
    /// `+0x150` MP current.
    pub mp: u16,
    /// `+0x152` MP max.
    pub mp_max: u16,
    /// `+0x154` / `+0x156` AGL working / base.
    pub agl: u16,
    pub agl_base: u16,
    /// `+0x158` / `+0x15A` ATK working / base.
    pub atk: u16,
    pub atk_base: u16,
    /// `+0x15C` / `+0x15E` UDF working / base.
    pub udf: u16,
    pub udf_base: u16,
    /// `+0x160` / `+0x162` LDF working / base.
    pub ldf: u16,
    pub ldf_base: u16,
    /// `+0x164` / `+0x166` SPD working / base.
    pub spd: u16,
    pub spd_base: u16,
    /// `+0x168` / `+0x16A` INT working / base.
    pub int: u16,
    pub int_base: u16,
    /// `+0x170` spirit / SP.
    pub spirit: u16,
    /// `+0x172` / `+0x174` battle-entry HP / MP snapshots (the values the
    /// results screen diffs against).
    pub hp_at_entry: u16,
    pub mp_at_entry: u16,
}

/// Seed a party battle actor's stat block from its character record plus the
/// equipment bonuses (`FUN_80053CB8`).
///
/// Order matters and is retail's: the equipment bonuses are folded into the
/// **base** halfwords first, then every working halfword is mirrored from its
/// base - so the working copies include the bonuses. Folding after the mirror
/// (an easy transposition) would leave the working stats un-equipped for the
/// first round.
///
/// The hit-reaction map this function also writes (`+0x1EF..+0x1F3`) is the
/// constant `[2, 3, 4, 5, 0x0B]` - the player battle files store that family
/// identity-ordered, so no search is needed. Monster actors take the scanning
/// path instead; see `legaia_asset::monster_archive::reaction_map`.
///
/// NOT WIRED: no engine caller - see [`equip_stat_bonuses`].
///
/// PORT: FUN_80053cb8 (the stat-block half)
pub fn init_party_battle_stats(
    record: &RecordStats,
    equipped: &[Option<EquipBonus>; EQUIP_SLOTS],
) -> BattleActorStats {
    let (udf_bonus, ldf_bonus, spd_bonus) = equip_stat_bonuses(equipped);
    // Bases first: only UDF / LDF / SPD take an equipment bonus.
    let agl_base = record.agl;
    let atk_base = record.atk;
    let udf_base = record.udf.wrapping_add(udf_bonus);
    let ldf_base = record.ldf.wrapping_add(ldf_bonus);
    let spd_base = record.spd.wrapping_add(spd_bonus);
    let int_base = record.int;
    BattleActorStats {
        hp: record.hp_cur,
        hp_max: record.hp_max,
        mp: record.mp_cur,
        mp_max: record.mp_max,
        // Working copies mirror the (already bonused) bases.
        agl: agl_base,
        agl_base,
        atk: atk_base,
        atk_base,
        udf: udf_base,
        udf_base,
        ldf: ldf_base,
        ldf_base,
        spd: spd_base,
        spd_base,
        int: int_base,
        int_base,
        spirit: record.spirit,
        hp_at_entry: record.hp_cur,
        mp_at_entry: record.mp_cur,
    }
}

/// The constant hit-reaction tag map `FUN_80053CB8` writes to a party actor's
/// `+0x1EF..+0x1F3`.
pub const PARTY_REACTION_MAP: [u8; 5] = [2, 3, 4, 5, 0x0B];
