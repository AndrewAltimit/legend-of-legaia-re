//! Single source-of-truth registry of well-known PSX RAM cells.
//!
//! Each [`RamCell`] is a (`address`, `width`, `category`, `target`,
//! `citation`) tuple where:
//!
//! - **address** is the canonical `0x80xxxxxx` PSX RAM address.
//! - **width** is the access width in bytes (1 / 2 / 4).
//! - **category** is the coarse [`Category`] taxonomy.
//! - **target** is the [`CellTarget`] - which engine cell the
//!   address maps to (for the runtime cheat applier).
//! - **citation** names the source(s) that pinned the cell - usually
//!   a GameShark cheat description plus a function dump symbol.
//!
//! The registry is built up at compile time via [`build_registry`].
//! It deliberately does NOT cover every byte in RAM - only the
//! globals and per-character offsets the cheat database has named
//! anchors for, plus a handful of well-known field-VM globals. Use
//! [`legaia_cheats::classify_address`] for finer-grained coverage of
//! the cheat corpus itself.

use legaia_cheats::Category;

/// One well-known RAM cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RamCell {
    /// PSX RAM address, normalised to `0x80xxxxxx`.
    pub addr: u32,
    /// Access width in bytes (1, 2, or 4).
    pub width: u8,
    /// Coarse semantic bucket.
    pub category: Category,
    /// Where this cell maps in the engine.
    pub target: CellTarget,
    /// Human description, used in docs generation.
    pub citation: &'static str,
}

/// Where a cell lives inside the engine. The runtime cheat applier
/// dispatches writes through this enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellTarget {
    /// World-level field (gold, play time, etc.). The variant
    /// names which field.
    World(WorldField),
    /// Per-character record at the given party slot (0..=3) and
    /// byte offset inside the 0x414-byte record.
    CharacterRecord {
        /// Party slot 0..=3 (Vahn / Noa / Gala / slot3).
        slot: u8,
        /// Byte offset inside the record.
        offset: u16,
        /// Access width inside the record (1 / 2).
        width: u8,
    },
    /// Inventory slot at the given index. `field` selects ID vs count.
    Inventory {
        /// Inventory slot index 0..=71.
        slot: u8,
        /// Which byte of the (id, count) pair this addresses.
        field: InventoryField,
    },
    /// A field the engine doesn't currently expose. Cheat applies
    /// to this target are recorded but not executed.
    Unmapped,
}

/// Which engine `World`-level field a [`CellTarget::World`] writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WorldField {
    /// Gold (`World::money`).
    Gold,
    /// Casino coins. Currently no engine field; recorded only.
    Coins,
    /// Game-time seconds (`World::play_time_seconds`). Mapped from
    /// the in-RAM frame counter at the cell address.
    PlayTimeSeconds,
    /// Party member count.
    PartyMemberCount,
    /// Encounter step counter.
    EncounterStepCounter,
    /// Camera mode word.
    CameraMode,
    /// Save-anywhere flag.
    SaveAnywhereFlag,
    /// Next game-mode register.
    NextGameMode,
    /// BGM ID register.
    BgmId,
    /// Active scene-name pool slot.
    SceneNamePool,
}

/// Which byte of an inventory `(id, count)` pair a cell addresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InventoryField {
    /// Item ID byte.
    Id,
    /// Quantity byte.
    Count,
}

/// Build the static registry of well-known cells. Runs in O(N)
/// across the registry (a few hundred entries) - call once and
/// cache the result if you need repeated lookups.
#[allow(clippy::vec_init_then_push)]
pub fn build_registry() -> Vec<RamCell> {
    let mut out = Vec::new();

    // World-level globals.
    out.push(RamCell {
        addr: 0x80084540,
        width: 4,
        category: Category::PartyMoney,
        target: CellTarget::World(WorldField::SceneNamePool),
        citation: "GameShark `Map Modifier (Hold L2+R2)` + `View Credits`",
    });
    out.push(RamCell {
        addr: 0x80084570,
        width: 4,
        category: Category::PartyMoney,
        target: CellTarget::World(WorldField::PlayTimeSeconds),
        citation: "GameShark `Game Time 0:00:00`",
    });
    out.push(RamCell {
        addr: 0x80084594,
        width: 1,
        category: Category::PartyMoney,
        target: CellTarget::World(WorldField::PartyMemberCount),
        citation: "GameShark `Character Activator`",
    });
    out.push(RamCell {
        addr: 0x8008459C,
        width: 4,
        category: Category::PartyMoney,
        target: CellTarget::World(WorldField::Gold),
        citation: "GameShark `Infinite Gold (Never Glitchy)`",
    });
    out.push(RamCell {
        addr: 0x800845A4,
        width: 4,
        category: Category::PartyMoney,
        target: CellTarget::World(WorldField::Coins),
        citation: "GameShark `Infinite Coins`",
    });
    out.push(RamCell {
        addr: 0x8007B5FC,
        width: 2,
        category: Category::ScriptVmGlobal,
        target: CellTarget::World(WorldField::EncounterStepCounter),
        citation: "GameShark `No Random Battles` (writes 0x377 = max)",
    });
    out.push(RamCell {
        addr: 0x8007B6A8,
        width: 2,
        category: Category::ScriptVmGlobal,
        target: CellTarget::World(WorldField::SaveAnywhereFlag),
        citation: "GameShark `Save Anywhere (Press Select+X)`",
    });
    out.push(RamCell {
        addr: 0x8007B6F4,
        width: 2,
        category: Category::CameraGlobal,
        target: CellTarget::World(WorldField::CameraMode),
        citation: "GameShark `Control Camera` + `Small Maps`",
    });
    out.push(RamCell {
        addr: 0x8007B83C,
        width: 2,
        category: Category::ScriptVmGlobal,
        target: CellTarget::World(WorldField::NextGameMode),
        citation: "GameShark `Press R2 For Debug Menu`; FUN_801E30E4 sets to 0x1A for FMV",
    });
    out.push(RamCell {
        addr: 0x8007BAC8,
        width: 2,
        category: Category::ScriptVmGlobal,
        target: CellTarget::World(WorldField::BgmId),
        citation: "field-VM op 0x35 sub-1 sink; FUN_800243F0 reader",
    });

    // Per-character record offsets - 4 slots x cheat-pinned offsets.
    let char_bases: [(u8, u32); 4] = [
        (0, 0x80084708),
        (1, 0x80084B1C),
        (2, 0x80084F30),
        (3, 0x80085344),
    ];
    let char_offsets: &[(u16, u8, &str)] = &[
        (0x000, 4, "Max Exp / Quick Level Gain (XP low word)"),
        (0x004, 2, "captured XP cell at +0x004"),
        (0x10E, 2, "AP gauge"),
        (0x104, 2, "live HP curr"),
        (0x106, 2, "live HP max (Max HP cheat)"),
        (0x108, 2, "live MP curr (Max MP cheat)"),
        (0x10A, 2, "live MP max"),
        (0x110, 2, "live AGL"),
        (0x112, 2, "live ATK"),
        (0x114, 2, "live UDF"),
        (0x116, 2, "live LDF"),
        (0x118, 2, "live SPD"),
        (0x11A, 2, "live INT (also stat_cap accessor in legacy code)"),
        (0x11C, 2, "record HP_max"),
        (0x11E, 2, "record MP_max"),
        (0x120, 2, "per-stat cap constant 100"),
        (0x122, 2, "record AGL"),
        (0x124, 2, "record ATK"),
        (0x126, 2, "record UDF"),
        (0x128, 2, "record LDF"),
        (0x12A, 2, "record SPD"),
        (0x12C, 2, "record INT"),
        (0x130, 1, "Magic Rank (also `Level 99` cheat target)"),
        (0x13C, 1, "Magic Slot Activator"),
        (0x161, 1, "summon-level slot 0 (All Summons Level 9)"),
        (0x185, 1, "displayed-skills count (Has all Arts)"),
        (0x196, 1, "armor ID (Best Equipment)"),
        (0x197, 1, "head gear ID"),
        (0x198, 1, "weapon ID"),
        (0x19A, 1, "leg gear ID"),
        (0x19B, 1, "accessory 1 ID"),
        (0x19C, 1, "accessory 2 ID"),
        (0x19D, 1, "accessory 3 ID"),
    ];
    for (slot, base) in char_bases {
        for &(off, width, cite) in char_offsets {
            out.push(RamCell {
                addr: base + off as u32,
                width,
                category: Category::CharacterRecord,
                target: CellTarget::CharacterRecord {
                    slot,
                    offset: off,
                    width,
                },
                citation: cite,
            });
        }
    }

    // Inventory slots.
    for slot in 0..72u8 {
        let id_addr = 0x80085958 + (slot as u32) * 2;
        out.push(RamCell {
            addr: id_addr,
            width: 1,
            category: Category::Inventory,
            target: CellTarget::Inventory {
                slot,
                field: InventoryField::Id,
            },
            citation: "GameShark `Item Modifier`",
        });
        out.push(RamCell {
            addr: id_addr + 1,
            width: 1,
            category: Category::Inventory,
            target: CellTarget::Inventory {
                slot,
                field: InventoryField::Count,
            },
            citation: "GameShark `Have 99 Items` / `Have Max Items`",
        });
    }

    out
}

/// Look up a cell by its canonical address. Returns the first match
/// (the registry has no duplicates by address).
pub fn lookup(addr: u32) -> Option<RamCell> {
    build_registry().into_iter().find(|c| c.addr == addr)
}

/// Iterate over every per-character offset the registry knows about,
/// for the given party slot. Useful for the offset-table doc.
pub fn character_record_cells(slot: u8) -> Vec<RamCell> {
    build_registry()
        .into_iter()
        .filter(|c| matches!(c.target, CellTarget::CharacterRecord { slot: s, .. } if s == slot))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_has_global_cells_for_every_world_field() {
        let r = build_registry();
        let world_targets: std::collections::HashSet<WorldField> = r
            .iter()
            .filter_map(|c| match c.target {
                CellTarget::World(w) => Some(w),
                _ => None,
            })
            .collect();
        assert!(world_targets.contains(&WorldField::Gold));
        assert!(world_targets.contains(&WorldField::PlayTimeSeconds));
        assert!(world_targets.contains(&WorldField::EncounterStepCounter));
        assert!(world_targets.contains(&WorldField::NextGameMode));
        assert!(world_targets.contains(&WorldField::CameraMode));
    }

    #[test]
    fn character_record_cells_cover_all_four_slots() {
        for slot in 0..4 {
            let cells = character_record_cells(slot);
            assert!(
                cells.len() >= 25,
                "slot {slot} only has {} cells",
                cells.len()
            );
        }
    }

    #[test]
    fn inventory_registry_covers_all_72_slots() {
        let r = build_registry();
        let inv_count = r
            .iter()
            .filter(|c| matches!(c.target, CellTarget::Inventory { .. }))
            .count();
        assert_eq!(inv_count, 72 * 2);
    }

    #[test]
    fn lookup_finds_known_addresses() {
        // Vahn HP_max live = +0x106
        let cell = lookup(0x80084708 + 0x106).unwrap();
        assert!(matches!(
            cell.target,
            CellTarget::CharacterRecord {
                slot: 0,
                offset: 0x106,
                ..
            }
        ));
        // Gold global.
        let cell = lookup(0x8008459C).unwrap();
        assert!(matches!(cell.target, CellTarget::World(WorldField::Gold)));
        // Inventory slot 5 count.
        let cell = lookup(0x80085958 + 5 * 2 + 1).unwrap();
        assert!(matches!(
            cell.target,
            CellTarget::Inventory {
                slot: 5,
                field: InventoryField::Count,
            }
        ));
    }

    #[test]
    fn lookup_returns_none_for_unknown_address() {
        assert!(lookup(0xCAFEBABE).is_none());
    }
}
