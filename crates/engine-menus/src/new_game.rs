//! New Game record seeding: turn the `SCUS_942.54` starting-party template
//! ([`legaia_game_tables::new_game`]) into live party records, plus the
//! cold-boot defaults a scene host seeds a fresh slate from.
//!
//! This is the `World`-free half of the New Game boot chain (see
//! `docs/subsystems/boot.md` and `docs/formats/new-game-table.md`): the record
//! a template row seeds ([`starting_record`]), the scene-picker party's
//! starter loadout ([`starter_loadout`]), the bundle a cold scene boot seeds
//! from ([`NewGameDefaults`]) and the game-state cold-reset values
//! ([`GAME_STATE_COLD_RESET`]). engine-core's `new_game` module applies them
//! to a `World` (`World::begin_new_game_seeded`, `World::seed_starting_party`,
//! `World::seed_cold_boot_defaults`).

use legaia_game_tables::new_game::{
    LIVE_RECORD_0_XP_OFFSET, StartingChar, StartingInventory, StartingParty, seed_live_records,
};
use legaia_save::CharacterRecord;

/// The retail new-game defaults a scene host seeds on a **cold** scene boot -
/// entering a scene directly (native `play-window --scene X`, the site play
/// page's scene picker) without a New Game confirm or a loaded save. Bundles
/// the `SCUS_942.54` starting-party template with the starting-inventory seed
/// so `World::seed_cold_boot_defaults` can stand up a faithful slate and the
/// pause menu always has valid party data to read.
#[derive(Debug, Clone)]
pub struct NewGameDefaults {
    /// Starting-party template (`legaia_game_tables::new_game::StartingParty`,
    /// SCUS `0x80078C4C`).
    pub party: StartingParty,
    /// Starting-inventory seed (`FUN_80034A6C`; vanilla = Healing Leaf ×5).
    /// `None` when the seed couldn't be decoded - the bag stays empty.
    pub inventory: Option<StartingInventory>,
    /// `true` on a **scene-picker** host (the browser play page, the native
    /// `play-window`): a cold entry seeds the full Vahn / Noa / Gala party
    /// (`World::seed_picker_party`) instead of retail's Vahn-alone New Game
    /// roster, so a scene picked mid-story is playable with the party the
    /// story has by then. `false` keeps the retail roster - what every
    /// headless harness that cold-boots `town01` as "the opening" expects.
    /// A true New Game never reaches this guard (it seeds first).
    pub picker_party: bool,
    /// The `SCUS_942.54` equipment stat-bonus table the picker party's
    /// starter loadout is chosen from ([`starter_loadout`]). `None` falls
    /// back to the world's installed table, and with neither the members
    /// stay unequipped (the battle file's default look).
    pub equip_stats: Option<legaia_game_tables::equip_stats::EquipStatTable>,
}

impl NewGameDefaults {
    /// Every cold-boot default the executable carries, parsed once: the
    /// starting-party template, the starting bag and the equipment table.
    /// `None` when the template does not parse (not a PSX-EXE).
    /// `picker_party` starts `false`; a picker host sets it.
    pub fn from_scus(scus: &[u8]) -> Option<Self> {
        Some(Self {
            party: StartingParty::from_scus(scus)?,
            inventory: StartingInventory::from_scus(scus),
            picker_party: false,
            equip_stats: legaia_game_tables::equip_stats::EquipStatTable::from_scus(scus),
        })
    }
}

/// The roster slots a scene-picker entry seeds: Vahn, Noa, Gala - the
/// three-member party the story carries from Noa's and Gala's joins onward
/// (Terra is a guest, never a battle member).
pub const PICKER_PARTY_SLOTS: [u8; 3] = [0, 1, 2];

/// Which equipment-slot byte holds each character's weapon - the
/// per-character table at `_DAT_8007B42C` (`2, 3, 2` for Vahn / Noa / Gala;
/// see [`legaia_save::EquipmentSlots`]). Body armour is byte 0, head byte 1
/// and footwear byte 4 for everyone.
pub const WEAPON_SLOT_BYTE: [usize; 3] = [2, 3, 2];

/// The starter loadout a scene-picker party member wears: for each of the
/// four equip categories, the **weakest** item the disc's equipment table
/// lets `char_slot` wear - lowest positive stat bonus for the category
/// (attack for a weapon, defence + intelligence for the rest), ties broken by
/// the lower id. A weapon exclusive to the character (equip mask = their bit
/// alone) wins over a shared one. Ra-Seru story gear and items carrying an accessory passive
/// are never picked, so the result is ordinary shop-tier gear.
///
/// This is an engine choice, not a retail table: retail's New Game clears the
/// equipment bytes (the `0x1A18`-byte memset before `FUN_80034A6C`), and each
/// character's later gear comes from story scripts and shops. A scene picked
/// past those points would otherwise put three unarmed characters into a
/// fight. Accessory slots (5..7) stay empty.
pub fn starter_loadout(
    table: &legaia_game_tables::equip_stats::EquipStatTable,
    char_slot: u8,
) -> legaia_save::EquipmentSlots {
    use legaia_game_tables::equip_stats::EquipSlot as Cat;
    let mut eq = legaia_save::EquipmentSlots::default();
    let Some(&weapon_byte) = WEAPON_SLOT_BYTE.get(char_slot as usize) else {
        return eq;
    };
    for (cat, byte) in [
        (Cat::Body, 0usize),
        (Cat::Head, 1),
        (Cat::Weapon, weapon_byte),
        (Cat::Footwear, 4),
    ] {
        let best = (1u8..=255)
            .filter(|&id| table.is_equipment(id))
            .filter_map(|id| table.bonus(id).map(|b| (id, b)))
            .filter(|(_, b)| {
                b.slot() == cat
                    && b.equips_party_slot(char_slot)
                    && !b.is_ra_seru()
                    && !b.has_passive()
            })
            .map(|(id, b)| {
                let score = match cat {
                    Cat::Weapon => u32::from(b.attack()),
                    _ => u32::from(b.def_up()) + u32::from(b.def_down()) + u32::from(b.int_up()),
                };
                // A weapon only this character can hold is their own weapon
                // family; a shared one (knives) is off-class for most and
                // costs more on the arts gauge. Prefer the exclusive ones.
                let shared = cat == Cat::Weapon && b.equip_mask() != 1 << char_slot;
                (shared, score, id)
            })
            .filter(|&(_, score, _)| score > 0)
            .min();
        if let Some((_, _, id)) = best {
            eq.slots[byte] = id;
        }
    }
    eq
}

/// Build a live 0x414-byte character record from one starting-party template
/// row.
///
/// The stat halfwords are **not** re-derived here: they come from
/// [`seed_live_records`], the port of retail's seed routine `FUN_800560B4`,
/// applied at their record-relative offsets. That routine's stores are the
/// specification for which template field lands in which live cell, so routing
/// through it keeps the engine's record byte-identical to a retail New Game
/// record without this module re-stating the mapping.
///
/// The one cell where re-stating it went wrong is `+0x10C` (and its max-block
/// twin `+0x120`): retail writes the **literal** `100` there for every roster
/// slot (`800561b8 li v0,0x64` / `800561bc sh v0,0x6d4(s0)` /
/// `800561c0 sh v0,0x6e8(s0)`, inside the four-iteration loop), not the
/// template's agility field. Vahn's agility is also `100`, so a Vahn-only
/// capture cannot tell the two apart - Noa (`120`) and Gala (`80`) can, and
/// they seed `100`.
///
/// The seed routine's non-halfword writes stay here, because
/// [`seed_live_records`] deliberately does not carry them: the level /
/// magic-rank bytes, the XP pair, and the template-name `strcpy`.
pub fn starting_record(c: &StartingChar) -> CharacterRecord {
    let mut rec = CharacterRecord::zeroed();
    // Slot 0's live record base is `SC + LIVE_RECORD_0_XP_OFFSET`, so a
    // record-relative offset is the seed's SC offset less that base.
    let one = StartingParty::from_members(vec![c.clone()]);
    for store in seed_live_records(&one) {
        let off = (store.sc_offset - LIVE_RECORD_0_XP_OFFSET) as usize;
        rec.raw[off..off + 2].copy_from_slice(&store.value.to_le_bytes());
    }
    // Seru magic rank starts at 1 (Vahn, validated); gates the first spell tier.
    // (+0x130 doubles as the retail displayed level - LV 1 at a New Game.)
    rec.set_magic_rank(1);
    // Retail New Game records carry cumulative XP 0 (+0x0) and the L2
    // threshold in +0x4 - the Status menu's "Next Level 121" (base curve;
    // slots 1/2 hold the ± sin-divisor-corrected value: Noa 102 / Gala 140).
    rec.set_cumulative_xp(0);
    rec.set_next_level_xp(legaia_save::xp_for_level(2));
    // The template's 10-byte name field seeds the record's display name
    // (record +0x2A7) - retail shows it until the name-entry overlay
    // overwrites it.
    rec.set_name(&c.name);
    rec
}

/// PORT: FUN_8001FFA4
///
/// Game-state **cold-reset** defaults - the boot-time initializer one layer
/// above the new-game data-init `FUN_80034A6C`. Retail:
///
/// 1. zero-fills the `0x1A18`-byte live game-state block at `0x80084140`
///    (inventory pages, party fields, story flags - everything the save
///    block snapshots),
/// 2. seeds the non-zero boot defaults tabulated below,
/// 3. chains into `FUN_80034A6C` (new-game data-init: gold = 500,
///    starting-party template expansion) and `FUN_8002614C(0)` (audio-context
///    volume re-apply).
///
/// | Field | Retail global | Boot value |
/// |---|---|---|
/// | `flag_at_0x434` | `_DAT_80084574` | `1` |
/// | `brightness_ref` | `_DAT_8008457C` | `0xD7` (full-brightness reference the battle fades clamp against) |
/// | `voice_volume` | `_DAT_80084580` | `200` (voice/SFX volume config) |
/// | `audio_level` | `_DAT_8007B910` | `0xD7` - the live **audio level**: `FUN_800267A8` reads it at `0x800267B0` and passes `(level << 15) >> 16` to the libsnd volume shim `FUN_80062004`. The field was called `screen_brightness`, which the cell never was |
/// | `bgm_volume_raw` | `DAT_8007B6EC` | `-1` (field-BGM volume; see [`legaia_engine_scene_host::scene_host::bgm_reattach_volume`]) |
///
/// REF: FUN_800267A8, FUN_80062004
///
/// `DAT_8007B750` and `_DAT_8007BAD0` are also cleared to `0` (already
/// covered by the engine's default state). The engine analog of steps 1 + 3
/// is `World::begin_new_game` + `World::seed_cold_boot_defaults`; this
/// struct carries the step-2 values so hosts seed them from one place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameStateColdReset {
    /// `_DAT_80084574` boot value.
    pub flag_at_0x434: u32,
    /// `_DAT_8008457C` - full-brightness reference.
    pub brightness_ref: i32,
    /// `_DAT_80084580` - voice/SFX volume config.
    pub voice_volume: i32,
    /// `_DAT_8007B910` - the live audio level (a volume operand, not a
    /// brightness; see the table on [`GameStateColdReset`]).
    pub audio_level: i32,
    /// `DAT_8007B6EC` - field-BGM volume raw global.
    pub bgm_volume_raw: i32,
}

/// The retail boot values `FUN_8001FFA4` stores (see [`GameStateColdReset`]).
pub const GAME_STATE_COLD_RESET: GameStateColdReset = GameStateColdReset {
    flag_at_0x434: 1,
    brightness_ref: 0xD7,
    voice_volume: 200,
    audio_level: 0xD7,
    bgm_volume_raw: -1,
};

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_save::character::RECORD_CAP_CONSTANT;

    /// FUN_8001FFA4's five non-zero boot stores, as read off the
    /// disassembly (`li` + `sw` pairs at `0x8001FFD4..0x80020004`).
    #[test]
    fn cold_reset_defaults_match_retail_stores() {
        let d = GAME_STATE_COLD_RESET;
        assert_eq!(d.flag_at_0x434, 1);
        assert_eq!(d.brightness_ref, 0xD7);
        assert_eq!(d.voice_volume, 200);
        // Boot: the live audio level and the full-brightness reference
        // are stored from the same 0xD7 register, so they start equal
        // despite being unrelated cells.
        assert_eq!(d.audio_level, d.brightness_ref);
        assert_eq!(d.bgm_volume_raw, -1);
    }

    fn vahn() -> StartingChar {
        StartingChar {
            name: "Vahn".into(),
            hp_max: 180,
            mp_max: 20,
            agl: 100,
            atk: 24,
            udf: 16,
            ldf: 12,
            spd: 19,
            intel: 9,
        }
    }

    #[test]
    fn starting_record_maps_template_stats() {
        let rec = starting_record(&vahn());
        let hms = rec.hp_mp_sp();
        assert_eq!(hms.hp_cur, 180);
        assert_eq!(hms.hp_max, 180);
        assert_eq!(hms.mp_cur, 20);
        assert_eq!(hms.mp_max, 20);
        // +0x10C is the seed routine's literal 100, not Vahn's agility - which
        // happens to be 100 too. `cap_cells_take_the_retail_literal` is the
        // test that can actually tell those apart.
        assert_eq!(hms.sp_max, 100);
        assert_eq!(hms.sp_cur, 0, "status-page AP gauge (+0x10E) seeds to 0");
        let ls = rec.live_stats();
        assert_eq!(
            (ls.agl, ls.atk, ls.udf, ls.ldf, ls.spd, ls.int),
            (100, 24, 16, 12, 19, 9)
        );
        let rs = rec.record_stats();
        assert_eq!(rs.hp_max, 180);
        assert_eq!(rs.cap_constant, RECORD_CAP_CONSTANT);
        assert_eq!(rec.magic_rank(), 1);
        assert_eq!(rec.name(), "Vahn", "template name stamps the record field");
        // Retail New Game: Experience 0, Next Level 121 (Status-menu capture).
        assert_eq!(rec.cumulative_xp(), 0);
        assert_eq!(rec.next_level_xp(), 121);
    }

    /// Non-Vahn template rows are the only ones that can distinguish the
    /// seed routine's cap literal from the template's agility field, because
    /// Vahn's agility *is* `100`. Retail writes `100` into both cap cells for
    /// every roster slot (`FUN_800560B4`, `li v0,0x64` inside the loop), so
    /// Noa's record must read `100` at `+0x10C` / `+0x120` and `120` only in
    /// the agility cells.
    #[test]
    fn cap_cells_take_the_retail_literal_not_agility() {
        let noa = StartingChar {
            name: "Noa".into(),
            hp_max: 150,
            mp_max: 10,
            agl: 120,
            atk: 21,
            udf: 13,
            ldf: 11,
            spd: 30,
            intel: 3,
        };
        let rec = starting_record(&noa);
        assert_eq!(
            rec.hp_mp_sp().sp_max,
            legaia_game_tables::new_game::SEEDED_CAP_CONSTANT,
            "+0x10C is the seed literal, not the template agility"
        );
        assert_eq!(
            rec.stat_cap(),
            legaia_game_tables::new_game::SEEDED_CAP_CONSTANT,
            "+0x120 is the same literal"
        );
        assert_eq!(rec.live_stats().agl, 120, "agility still lands at +0x110");
        assert_eq!(rec.record_stats().agl, 120, "and at +0x122");
    }

    /// The record's stat halfwords must be exactly the seed model's stores -
    /// this is what makes [`seed_live_records`] the source of truth rather
    /// than a parallel description of it.
    #[test]
    fn every_seeded_halfword_lands_in_the_record() {
        let noa = StartingChar {
            name: "Noa".into(),
            hp_max: 150,
            mp_max: 10,
            agl: 120,
            atk: 21,
            udf: 13,
            ldf: 11,
            spd: 30,
            intel: 3,
        };
        for c in [vahn(), noa] {
            let rec = starting_record(&c);
            let one = StartingParty::from_members(vec![c.clone()]);
            let stores = seed_live_records(&one);
            assert!(!stores.is_empty());
            for s in stores {
                let off = (s.sc_offset - LIVE_RECORD_0_XP_OFFSET) as usize;
                let got = u16::from_le_bytes([rec.raw[off], rec.raw[off + 1]]);
                assert_eq!(got, s.value, "record +{off:#x} disagrees with the seed");
            }
        }
    }

    /// `+7` byte per category: Body `0x00`, Head `0x20`, Weapon `0x40`,
    /// Footwear `0x60`; `| 1` marks Ra-Seru gear.
    fn bonus(atk: u8, udf: u8, mask: u8, cat: u8) -> [u8; 8] {
        let none = legaia_game_tables::equip_stats::PASSIVE_NONE;
        [0, atk, udf, 0, 0, none, mask, cat]
    }

    fn synthetic_equip_table() -> legaia_game_tables::equip_stats::EquipStatTable {
        legaia_game_tables::equip_stats::EquipStatTable::from_entries(&[
            // Weapons: Vahn-only 10, Vahn-only 4 (weakest), Noa-only 6,
            // Gala-only 9, a Ra-Seru weapon with 1 that must never be picked.
            (0x10, bonus(10, 0, 1, 0x40)),
            (0x11, bonus(4, 0, 1, 0x40)),
            (0x12, bonus(6, 0, 2, 0x40)),
            (0x13, bonus(9, 0, 4, 0x40)),
            (0x14, bonus(1, 0, 7, 0x41)),
            // Armour for everyone, head for everyone, boots Vahn-only.
            (0x20, bonus(0, 5, 7, 0x00)),
            (0x21, bonus(0, 3, 7, 0x00)),
            (0x30, bonus(0, 2, 7, 0x20)),
            (0x40, bonus(0, 1, 1, 0x60)),
            // A zero-stat placeholder is never picked.
            (0x41, bonus(0, 0, 7, 0x60)),
        ])
    }

    #[test]
    fn starter_loadout_picks_the_weakest_wearable_per_category() {
        let t = synthetic_equip_table();
        // Vahn: body 0x21, head 0x30, weapon (byte 2) 0x11, boots 0x40.
        assert_eq!(
            starter_loadout(&t, 0).slots,
            [0x21, 0x30, 0x11, 0, 0x40, 0, 0, 0]
        );
        // Noa wears her weapon in byte 3 and has no wearable boots.
        assert_eq!(
            starter_loadout(&t, 1).slots,
            [0x21, 0x30, 0, 0x12, 0, 0, 0, 0]
        );
        // Gala: byte 2.
        assert_eq!(
            starter_loadout(&t, 2).slots,
            [0x21, 0x30, 0x13, 0, 0, 0, 0, 0]
        );
        // Terra is not a picker member: nothing.
        assert_eq!(starter_loadout(&t, 3).slots, [0; 8]);
    }

    #[test]
    fn the_boot_voice_volume_halves_to_one_hundred() {
        // The word this reads is the voice-volume config `_DAT_80084580`,
        // which the cold reset seeds at 200 - so a freshly booted game keys
        // the arena cue at 100 per channel (`other_game_overlay::cue_volume`,
        // the PROT 0977 arena cue). This is what settles the slot as a
        // volume rather than a coordinate.
        let boot = GAME_STATE_COLD_RESET.voice_volume;
        assert_eq!(boot, 200);
        assert_eq!(
            legaia_engine_minigames::other_game_overlay::cue_volume(boot as u32),
            100
        );
    }
}
