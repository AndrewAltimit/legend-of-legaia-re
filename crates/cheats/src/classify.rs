//! Semantic classifier for cheat addresses.
//!
//! Each cheat targets a single PSX RAM address. The classifier maps
//! that address to one of:
//!
//! - [`Category::CharacterRecord`] - inside one of the four party
//!   per-character `0x414`-byte records at `0x80084708 + n * 0x414`.
//!   The [`ClassifiedAddress::detail`] then names the field offset
//!   inside the record (e.g. `"hp_max_live(+0x106)"`).
//! - [`Category::PartyMoney`] - gold / coins / game time globals
//!   that sit between the inventory header and the per-character
//!   records.
//! - [`Category::Inventory`] - slots in the 2-byte-stride inventory
//!   array starting at `0x80085958`.
//! - [`Category::BattleActor`] - slots in the per-actor battle pool
//!   at `0x800EC9E8 + n * 0x2D4`.
//! - [`Category::ScriptVmGlobal`] - globals around `0x8007Bxxx`
//!   (BGM ID, story-flag word, debug menu trigger, encounter
//!   counter, save-anywhere flag).
//! - [`Category::PadInput`] - the per-frame button-mask cells.
//! - [`Category::CameraGlobal`] - camera mode / azimuth / zoom.
//! - [`Category::WorldStoryFlag`] - Door of Wind / town visited
//!   bitmaps that live outside the per-character records.
//! - [`Category::Minigame`] - fishing / baka / dance / slots scratch.
//! - [`Category::FieldVmCollision`] - walk-through-walls cells
//!   inside the field overlay.
//! - [`Category::ScratchActiveActor`] - the shared "currently-acting
//!   character" HP/MP scratch cell at `0x8007A6BC`.
//! - [`Category::CodePatch`] - addresses inside `SCUS_942.54` code
//!   that cheats patch to NOP (`0x2400`); these are usually
//!   "Maxed HP for All Characters" / "Remove Vahn's Chest" /
//!   "Infinite Items All Slots".
//! - [`Category::Unknown`] - none of the above.

use serde::{Deserialize, Serialize};

/// Coarse semantic bucket for a cheat address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Category {
    /// Per-character 0x414-byte record (one of four party slots).
    CharacterRecord,
    /// Party-wide money / gold / game-time / scene-name globals.
    PartyMoney,
    /// Inventory array at `0x80085958+`.
    Inventory,
    /// Battle actor pool at `0x800EC9E8+`.
    BattleActor,
    /// Scratch cells used by the script VM and overlays at `0x8007Bxxx`.
    ScriptVmGlobal,
    /// Pad input registers.
    PadInput,
    /// Camera-state globals.
    CameraGlobal,
    /// World-map / story-flag bitmaps outside the per-character records.
    WorldStoryFlag,
    /// Mini-game scratch RAM (fishing, baka, dance, slots).
    Minigame,
    /// Field overlay collision-state cells (walk-through-walls).
    FieldVmCollision,
    /// Shared "currently-acting character" HP/MP scratch.
    ScratchActiveActor,
    /// Code-patch sites inside `SCUS_942.54`.
    CodePatch,
    /// None of the above.
    Unknown,
}

/// Result of classifying a single address.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassifiedAddress {
    /// The address that was classified (canonicalised to `0x80xxxxxx`).
    pub addr: u32,
    /// Coarse bucket.
    pub category: Category,
    /// Specific subtype label - safe to render to JSON / display.
    /// Includes per-character offsets (e.g. `"vahn_record:level(+0x130)"`),
    /// inventory slot indices (`"inventory:slot[37]"`), etc.
    pub detail: String,
}

/// Per-character record base addresses (NTSC-U), stride 0x414.
pub const CHAR_RECORD_BASES: [(u32, &str); 4] = [
    (0x80084708, "vahn"),
    (0x80084B1C, "noa"),
    (0x80084F30, "gala"),
    (0x80085344, "slot3"),
];

/// Inventory base address. Stride 2 bytes (id, count).
pub const INVENTORY_BASE: u32 = 0x80085958;
/// Inventory slot count (number of (id, count) pairs).
pub const INVENTORY_SLOTS: u32 = 72;

/// Battle actor pool base. Stride 0x2D4 across party slots.
pub const BATTLE_ACTOR_BASE: u32 = 0x800EC9E8;
/// Stride between adjacent battle actor records.
pub const BATTLE_ACTOR_STRIDE: u32 = 0x2D4;

/// Classify one address. The result is always populated - unknown
/// addresses get `Category::Unknown` and a `"<addr>"` detail string.
pub fn classify_address(addr: u32) -> ClassifiedAddress {
    if let Some(c) = classify_character_record(addr) {
        return c;
    }
    if let Some(c) = classify_party_money(addr) {
        return c;
    }
    if let Some(c) = classify_inventory(addr) {
        return c;
    }
    if let Some(c) = classify_battle_actor(addr) {
        return c;
    }
    if let Some(c) = classify_script_vm_global(addr) {
        return c;
    }
    if let Some(c) = classify_camera(addr) {
        return c;
    }
    if let Some(c) = classify_pad(addr) {
        return c;
    }
    if let Some(c) = classify_world_story_flag(addr) {
        return c;
    }
    if let Some(c) = classify_minigame(addr) {
        return c;
    }
    if let Some(c) = classify_field_collision(addr) {
        return c;
    }
    if let Some(c) = classify_scratch_active_actor(addr) {
        return c;
    }
    if let Some(c) = classify_code_patch(addr) {
        return c;
    }
    ClassifiedAddress {
        addr,
        category: Category::Unknown,
        detail: format!("0x{addr:08X}"),
    }
}

fn classify_character_record(addr: u32) -> Option<ClassifiedAddress> {
    for &(base, name) in &CHAR_RECORD_BASES {
        let end = base + 0x414;
        if (base..end).contains(&addr) {
            let off = addr - base;
            let field = field_name_for_offset(off);
            return Some(ClassifiedAddress {
                addr,
                category: Category::CharacterRecord,
                detail: format!("{name}_record:{field}(+0x{off:03X})"),
            });
        }
    }
    None
}

/// Maps a record-relative byte offset to a human-readable field
/// label drawn from cheat-database evidence + existing decompilation
/// memory.
///
/// See `docs/formats/save-record.md` for the source of every offset.
pub fn field_name_for_offset(off: u32) -> &'static str {
    match off {
        0x000..=0x003 => "xp_low_word_alt(+0x000)",
        0x004..=0x005 => "xp_cumulative_u16(+0x004)",
        0x006..=0x00F => "header_tail",
        0x010..=0x0F3 => "stat_block_unmapped",
        0x0F4..=0x103 => "ability_bits[16]",
        0x104..=0x105 => "hp_curr_live(+0x104)",
        0x106..=0x107 => "hp_max_live(+0x106)",
        0x108..=0x109 => "mp_curr_live(+0x108)",
        0x10A..=0x10B => "mp_max_live(+0x10A)",
        0x10C..=0x10D => "sp_curr_live(+0x10C)",
        0x10E..=0x10F => "sp_max_live(+0x10E)",
        0x110..=0x111 => "agl_live(+0x110)",
        0x112..=0x113 => "atk_live(+0x112)",
        0x114..=0x115 => "udf_live(+0x114)",
        0x116..=0x117 => "ldf_live(+0x116)",
        0x118..=0x119 => "spd_live(+0x118)",
        0x11A..=0x11B => "int_live(+0x11A)",
        0x11C..=0x11D => "hp_max_record(+0x11C)",
        0x11E..=0x11F => "mp_max_record(+0x11E)",
        0x120..=0x121 => "stat_cap_constant_100(+0x120)",
        0x122..=0x123 => "agl_record(+0x122)",
        0x124..=0x125 => "atk_record(+0x124)",
        0x126..=0x127 => "udf_record(+0x126)",
        0x128..=0x129 => "ldf_record(+0x128)",
        0x12A..=0x12B => "spd_record(+0x12A)",
        0x12C..=0x12D => "int_record(+0x12C)",
        0x12E..=0x12F => "stat_window_tail",
        0x130 => "level_or_magic_rank(+0x130)",
        0x131..=0x13B => "post_level_unmapped",
        0x13C => "magic_slot_activator(+0x13C)",
        0x13D..=0x148 => "magic_group_0_slots(+0x13D..)",
        0x149..=0x154 => "magic_group_1_slots(+0x149..)",
        0x155..=0x160 => "magic_group_2_slots(+0x155..)",
        0x161..=0x178 => "summon_levels[16]",
        0x179..=0x184 => "post_summon_unmapped",
        0x185 => "displayed_skill_count(+0x185)",
        0x186..=0x195 => "displayed_skill_ids[16]",
        0x196 => "armor_id(+0x196)",
        0x197 => "head_gear_id(+0x197)",
        0x198 => "weapon_id(+0x198)",
        0x199 => "accessory_or_seru_lock(+0x199)",
        0x19A => "leg_gear_id(+0x19A)",
        0x19B => "accessory_1_id(+0x19B)",
        0x19C => "accessory_2_id(+0x19C)",
        0x19D => "accessory_3_id(+0x19D)",
        0x19E..=0x2AF => "post_equipment_unmapped",
        0x2B0..=0x37F => "active_spell_slots[14]",
        _ => "tail_unmapped",
    }
}

fn classify_party_money(addr: u32) -> Option<ClassifiedAddress> {
    let label = match addr {
        0x80084540 => "scene_name_pool",
        0x80084570 => "game_time_u32",
        0x80084594 => "party_member_count",
        0x80084598 => "party_member_ids",
        0x80084599 => "party_noa_activator",
        0x8008459A => "party_gala_activator",
        0x8008459C => "gold_u32",
        0x800845A4 => "coins_u32",
        0x800845A6 => "coins_high_word",
        0x800845DC => "scene_name_mirror",
        _ => return None,
    };
    Some(ClassifiedAddress {
        addr,
        category: Category::PartyMoney,
        detail: label.into(),
    })
}

fn classify_inventory(addr: u32) -> Option<ClassifiedAddress> {
    let last = INVENTORY_BASE + INVENTORY_SLOTS * 2;
    if (INVENTORY_BASE..last).contains(&addr) {
        let slot = (addr - INVENTORY_BASE) / 2;
        let field = if (addr - INVENTORY_BASE).is_multiple_of(2) {
            "id"
        } else {
            "count"
        };
        return Some(ClassifiedAddress {
            addr,
            category: Category::Inventory,
            detail: format!("inventory:slot[{slot}].{field}"),
        });
    }
    None
}

fn classify_battle_actor(addr: u32) -> Option<ClassifiedAddress> {
    // Cheats touch slots 0..3. Pool may extend further but we only
    // classify what the corpus exercises.
    for slot in 0..4u32 {
        let base = BATTLE_ACTOR_BASE + slot * BATTLE_ACTOR_STRIDE;
        let end = base + BATTLE_ACTOR_STRIDE;
        if (base..end).contains(&addr) {
            let off = addr - base;
            let field = match off {
                0x14C => "hp_curr",
                0x14E => "hp_max",
                0x150 => "mp_curr",
                0x152 => "mp_max",
                0x172 => "hp_max_settled",
                0x174 => "mp_max_settled",
                _ => "actor_field",
            };
            return Some(ClassifiedAddress {
                addr,
                category: Category::BattleActor,
                detail: format!("battle_actor[{slot}]:{field}(+0x{off:03X})"),
            });
        }
    }
    None
}

fn classify_script_vm_global(addr: u32) -> Option<ClassifiedAddress> {
    let label = match addr {
        0x8007B450 => "menu_request_register",
        0x8007B5FC => "encounter_step_counter",
        0x8007B6A8 => "save_anywhere_flag",
        0x8007B7C0 => "pad_state_word",
        0x8007B83C => "next_game_mode",
        0x8007BAC8 => "bgm_id",
        0x8008575C => "story_flag_door_of_wind_lo",
        0x8008575E => "story_flag_door_of_wind_hi",
        _ => return None,
    };
    Some(ClassifiedAddress {
        addr,
        category: Category::ScriptVmGlobal,
        detail: label.into(),
    })
}

fn classify_camera(addr: u32) -> Option<ClassifiedAddress> {
    let label = match addr {
        0x8007B6F4 => "camera_mode_word",
        0x8007B790 => "camera_zoom_state",
        _ => return None,
    };
    Some(ClassifiedAddress {
        addr,
        category: Category::CameraGlobal,
        detail: label.into(),
    })
}

fn classify_pad(addr: u32) -> Option<ClassifiedAddress> {
    let label = match addr {
        0x8007B850 => "pad_per_frame_mask",
        0x8007B874 => "pad_newly_pressed",
        _ => return None,
    };
    Some(ClassifiedAddress {
        addr,
        category: Category::PadInput,
        detail: label.into(),
    })
}

fn classify_world_story_flag(addr: u32) -> Option<ClassifiedAddress> {
    // The Door-of-Wind town list lives between 0x8008575C and 0x80085800.
    if (0x80085600..0x80085800).contains(&addr) {
        return Some(ClassifiedAddress {
            addr,
            category: Category::WorldStoryFlag,
            detail: format!("story_flag_word(+0x{:03X})", addr - 0x80085600),
        });
    }
    None
}

fn classify_minigame(addr: u32) -> Option<ClassifiedAddress> {
    let label = match addr {
        0x8008444C => "fishing_points_u16",
        0x801D078C | 0x801D071C | 0x801D065C | 0x801D06BC => return None,
        0x801D3CAC => "slot_machine_punch_mode",
        0x801D53CC => "dance_points",
        0x801D9168 => "fishing_tension",
        0x801D91CC => "fishing_fish_id",
        0x801D9274 => "fishing_casting_power",
        0x801D9298 => "fishing_life",
        0x801DBFC4 => "baka_player_life",
        0x801DBFF0 => "baka_rounds_won",
        0x801DC06C => "baka_computer_life",
        _ => return None,
    };
    Some(ClassifiedAddress {
        addr,
        category: Category::Minigame,
        detail: label.into(),
    })
}

fn classify_field_collision(addr: u32) -> Option<ClassifiedAddress> {
    let label = match addr {
        0x801D078C => "field_collision_a",
        0x801D071C => "field_collision_b",
        0x801D065C => "field_collision_c",
        0x801D06BC => "field_collision_d",
        _ => return None,
    };
    Some(ClassifiedAddress {
        addr,
        category: Category::FieldVmCollision,
        detail: label.into(),
    })
}

fn classify_scratch_active_actor(addr: u32) -> Option<ClassifiedAddress> {
    let label = match addr {
        0x8007A6BC => "active_actor_hp_or_mp",
        0x8007A894 => "frame_pacing_logic_timer",
        _ => return None,
    };
    Some(ClassifiedAddress {
        addr,
        category: Category::ScratchActiveActor,
        detail: label.into(),
    })
}

fn classify_code_patch(addr: u32) -> Option<ClassifiedAddress> {
    let label = match addr {
        0x8004309E => "patch_infinite_items_all_slots",
        0x80043900..=0x80043920 => "patch_remove_vahn_chest",
        0x8007EA96 => "patch_max_hp_all_characters",
        0x800422F4 => "patch_99_quantity_pickup",
        _ => return None,
    };
    Some(ClassifiedAddress {
        addr,
        category: Category::CodePatch,
        detail: label.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_vahn_xp_offset() {
        let c = classify_address(0x80084708);
        assert_eq!(c.category, Category::CharacterRecord);
        assert!(c.detail.contains("vahn_record"));
        assert!(c.detail.contains("xp_low_word_alt"));
    }

    #[test]
    fn classifies_noa_level_offset() {
        // 0x80084C4C - 0x80084B1C = 0x130 → "Level 99 (Noa)"
        let c = classify_address(0x80084C4C);
        assert_eq!(c.category, Category::CharacterRecord);
        assert!(c.detail.starts_with("noa_record:"));
        assert!(c.detail.contains("level_or_magic_rank"));
    }

    #[test]
    fn classifies_inventory_slot() {
        // 0x80085959 = slot 0 count byte.
        let c = classify_address(0x80085959);
        assert_eq!(c.category, Category::Inventory);
        assert!(c.detail.contains("slot[0]"));
        assert!(c.detail.ends_with(".count"));
    }

    #[test]
    fn classifies_inventory_id() {
        let c = classify_address(0x80085958);
        assert_eq!(c.category, Category::Inventory);
        assert!(c.detail.ends_with(".id"));
    }

    #[test]
    fn classifies_battle_actor_hp_curr() {
        // Vahn slot: 0x800ECB34 - 0x800EC9E8 = 0x14C → slot 0, hp_curr.
        let vahn = classify_address(0x800ECB34);
        assert_eq!(vahn.category, Category::BattleActor);
        assert!(vahn.detail.contains("battle_actor[0]"));
        assert!(vahn.detail.contains("hp_curr"));
        // Noa slot: 0x800ECE08 - 0x800EC9E8 = 0x420 → slot 1, hp_curr (0x14C inside slot).
        let noa = classify_address(0x800ECE08);
        assert_eq!(noa.category, Category::BattleActor);
        assert!(
            noa.detail.contains("battle_actor[1]"),
            "expected slot 1, got `{}`",
            noa.detail
        );
        assert!(noa.detail.contains("hp_curr"));
    }

    #[test]
    fn classifies_known_globals() {
        assert_eq!(
            classify_address(0x8007B5FC).detail,
            "encounter_step_counter"
        );
        assert_eq!(classify_address(0x8008459C).detail, "gold_u32");
        assert_eq!(classify_address(0x80084570).detail, "game_time_u32");
        assert_eq!(classify_address(0x8007B83C).detail, "next_game_mode");
        assert_eq!(classify_address(0x8007B6F4).detail, "camera_mode_word");
    }

    #[test]
    fn classifies_minigame_cells() {
        assert_eq!(classify_address(0x801D9274).detail, "fishing_casting_power");
        assert_eq!(classify_address(0x801DC06C).detail, "baka_computer_life");
    }

    #[test]
    fn unknown_addresses_fall_through() {
        let c = classify_address(0xCAFEBABE);
        assert_eq!(c.category, Category::Unknown);
    }

    #[test]
    fn classifies_walk_thru_walls_collision() {
        let c = classify_address(0x801D078C);
        assert_eq!(c.category, Category::FieldVmCollision);
    }

    #[test]
    fn classifies_code_patch_sites() {
        assert_eq!(classify_address(0x8004309E).category, Category::CodePatch);
        assert_eq!(classify_address(0x8007EA96).category, Category::CodePatch);
    }

    #[test]
    fn equipment_block_offsets_named() {
        // Vahn weapon = +0x198
        let c = classify_address(0x800848A0);
        assert!(c.detail.contains("weapon_id"));
        // Noa armor = +0x196
        let c = classify_address(0x80084CB2);
        assert!(c.detail.contains("armor_id"));
    }
}
