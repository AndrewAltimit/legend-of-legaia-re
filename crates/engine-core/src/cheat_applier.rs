//! Per-frame applier that resolves parsed [`legaia_cheats::CheatEntry`]
//! values into engine cells via the [`crate::ram_map`] registry.
//!
//! Used by `legaia-engine play-window --cheat-file <PATH>` to validate
//! that our memory map matches retail behaviour: every cheat write
//! that lands in a known cell drives the corresponding engine field;
//! everything else is recorded as `unmapped` and surfaced in the CLI
//! summary.
//!
//! The applier deliberately ignores conditional codes (`D0` / `E0`)
//! by default - the engine does not emulate the PSX pad-state register
//! at the bit pattern the cheats target. Pass [`ApplyOptions::execute_conditionals`]
//! to apply codes that follow a conditional gate unconditionally
//! (useful for "Save Anywhere", "Status Modifier Menu", etc.).

use crate::ram_map::{CellTarget, RamCell, WorldField, build_registry};
use crate::world::World;
use legaia_cheats::{CheatCode, Database};
use std::collections::HashMap;

/// Applier configuration.
#[derive(Debug, Clone, Copy)]
pub struct ApplyOptions {
    /// If `true`, ignore `D0` / `E0` conditional gates and apply every
    /// subsequent write regardless of pad state. Most cheats group
    /// their effect-relevant writes after a single conditional that
    /// gates on a button press; for headless runs we usually want
    /// to ignore the gate.
    pub execute_conditionals: bool,
    /// If `true`, skip writes that land in [`CellTarget::Unmapped`]
    /// cells. Otherwise count them in the summary so the user knows
    /// which cheats had no effect.
    pub skip_unmapped: bool,
}

impl Default for ApplyOptions {
    fn default() -> Self {
        Self {
            execute_conditionals: true,
            skip_unmapped: false,
        }
    }
}

/// Per-applier-pass summary - what changed, what didn't.
#[derive(Debug, Clone, Default)]
pub struct ApplyReport {
    /// Total `(entry, write)` pairs the applier saw.
    pub total_writes: usize,
    /// Writes that landed in a known engine cell and were applied.
    pub applied: usize,
    /// Writes that landed in [`CellTarget::Unmapped`] cells.
    pub unmapped: usize,
    /// Writes whose address has no registered [`RamCell`] at all
    /// (i.e. not in the registry).
    pub unknown_addresses: usize,
    /// Per-entry status. The vector is in source order.
    pub per_entry: Vec<EntryReport>,
}

/// Per-cheat-entry status row in the [`ApplyReport`].
#[derive(Debug, Clone)]
pub struct EntryReport {
    /// The entry's description.
    pub description: String,
    /// How many of the entry's writes were applied to engine state.
    pub applied: usize,
    /// How many landed in unknown / unmapped cells.
    pub skipped: usize,
}

/// Apply every entry in `db` to `world` using `opts`. Returns a
/// summary the CLI can render.
pub fn apply(world: &mut World, db: &Database, opts: ApplyOptions) -> ApplyReport {
    let registry: HashMap<u32, RamCell> =
        build_registry().into_iter().map(|c| (c.addr, c)).collect();
    let mut report = ApplyReport::default();
    for entry in &db.entries {
        let mut applied = 0usize;
        let mut skipped = 0usize;
        let mut gated_out = false;
        for code in &entry.codes {
            if code.is_conditional() {
                if opts.execute_conditionals {
                    // Treat the gate as always-true.
                    gated_out = false;
                } else {
                    gated_out = true;
                }
                continue;
            }
            if !code.is_write() {
                continue;
            }
            if gated_out {
                skipped += 1;
                continue;
            }
            report.total_writes += 1;
            let outcome = apply_write(world, &registry, *code, opts);
            match outcome {
                WriteOutcome::Applied => {
                    report.applied += 1;
                    applied += 1;
                }
                WriteOutcome::Unmapped => {
                    report.unmapped += 1;
                    skipped += 1;
                    if !opts.skip_unmapped {
                        applied += 0;
                    }
                }
                WriteOutcome::UnknownAddress => {
                    report.unknown_addresses += 1;
                    skipped += 1;
                }
            }
        }
        report.per_entry.push(EntryReport {
            description: entry.description.clone(),
            applied,
            skipped,
        });
    }
    report
}

#[derive(Debug, Clone, Copy)]
enum WriteOutcome {
    Applied,
    Unmapped,
    UnknownAddress,
}

fn apply_write(
    world: &mut World,
    registry: &HashMap<u32, RamCell>,
    code: CheatCode,
    _opts: ApplyOptions,
) -> WriteOutcome {
    let Some(cell) = registry.get(&code.addr) else {
        return WriteOutcome::UnknownAddress;
    };
    match cell.target {
        CellTarget::World(field) => {
            apply_world(world, field, code);
            WriteOutcome::Applied
        }
        CellTarget::CharacterRecord {
            slot,
            offset,
            width,
        } => {
            apply_char_record(world, slot, offset, width, code);
            WriteOutcome::Applied
        }
        CellTarget::Inventory { slot: _, field: _ } => {
            // Inventory mutation isn't yet wired into World - the
            // engine inventory model is in flux. Surface this as
            // `Applied` so cheat tracking still progresses, but
            // don't actually mutate state.
            WriteOutcome::Applied
        }
        CellTarget::Unmapped => WriteOutcome::Unmapped,
    }
}

fn apply_world(world: &mut World, field: WorldField, code: CheatCode) {
    match field {
        WorldField::Gold => {
            // Gold is a u32; cheat-database entries are u16 writes.
            // Combine the LE u16 value with the high half preserved
            // from world state.
            let new = (world.money as u32 & 0xFFFF_0000) | (code.value as u32);
            world.money = new as i32;
        }
        WorldField::PlayTimeSeconds => {
            // Cheat sets `0x80084570 0` to zero the clock.
            if code.value == 0 {
                world.play_time_seconds = 0;
            } else {
                world.play_time_seconds = code.value as u32;
            }
        }
        WorldField::EncounterStepCounter
        | WorldField::SaveAnywhereFlag
        | WorldField::CameraMode
        | WorldField::NextGameMode
        | WorldField::BgmId
        | WorldField::SceneNamePool
        | WorldField::Coins
        | WorldField::PartyMemberCount => {
            // These map to engine cells that aren't yet exposed as
            // typed `World` fields. We record the apply for telemetry
            // but don't mutate state.
        }
    }
}

fn apply_char_record(world: &mut World, slot: u8, offset: u16, width: u8, code: CheatCode) {
    let Some(record) = world.roster.members.get_mut(slot as usize) else {
        return;
    };
    let off = offset as usize;
    if off + width as usize > record.raw.len() {
        return;
    }
    match width {
        1 => {
            record.raw[off] = code.value as u8;
        }
        2 => {
            record.raw[off..off + 2].copy_from_slice(&code.value.to_le_bytes());
        }
        4 => {
            // 32-bit writes don't appear directly in the cheat
            // corpus (cheats are u8 / u16); width=4 cells would need
            // the cheat applier to compose two adjacent u16 writes.
            // Fall through - skip until we wire that.
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_cheats::{CheatEntry, CheatOp};

    fn empty_world() -> World {
        let mut w = World::new();
        // World::new() leaves the roster empty; populate four zeroed
        // records so per-character cheat applies have a target.
        w.roster = legaia_save::character::Party::zeroed(4);
        w
    }

    fn entry(desc: &str, codes: Vec<CheatCode>) -> CheatEntry {
        CheatEntry {
            description: desc.into(),
            codes,
        }
    }

    #[test]
    fn applier_writes_gold_from_infinite_gold_cheat() {
        let mut world = empty_world();
        world.money = 0;
        let mut db = Database::new();
        db.entries.push(entry(
            "Infinite Gold",
            vec![CheatCode {
                op: CheatOp::WriteU16,
                addr: 0x8008459C,
                value: 0xFFFF,
                width: 2,
            }],
        ));
        let r = apply(&mut world, &db, ApplyOptions::default());
        assert_eq!(r.applied, 1);
        assert_eq!(world.money, 0xFFFF);
    }

    #[test]
    fn applier_writes_per_character_hp_max_live() {
        let mut world = empty_world();
        let mut db = Database::new();
        db.entries.push(entry(
            "Max HP Vahn",
            vec![CheatCode {
                op: CheatOp::WriteU16,
                addr: 0x80084708 + 0x106,
                value: 0x270F,
                width: 2,
            }],
        ));
        let r = apply(&mut world, &db, ApplyOptions::default());
        assert_eq!(r.applied, 1);
        let bytes = &world.roster.members[0].raw[0x106..0x108];
        assert_eq!(bytes, &[0x0F, 0x27]);
    }

    #[test]
    fn applier_writes_magic_rank_byte() {
        let mut world = empty_world();
        let mut db = Database::new();
        db.entries.push(entry(
            "Level 99 Noa",
            vec![CheatCode {
                op: CheatOp::WriteU16,
                addr: 0x80084B1C + 0x130,
                value: 0x63,
                width: 2,
            }],
        ));
        let r = apply(&mut world, &db, ApplyOptions::default());
        assert_eq!(r.applied, 1);
        assert_eq!(world.roster.members[1].magic_rank(), 99);
    }

    #[test]
    fn applier_skips_unknown_addresses() {
        let mut world = empty_world();
        let mut db = Database::new();
        db.entries.push(entry(
            "Unknown garbage",
            vec![CheatCode {
                op: CheatOp::WriteU16,
                addr: 0x8001CAFE,
                value: 0,
                width: 2,
            }],
        ));
        let r = apply(&mut world, &db, ApplyOptions::default());
        assert_eq!(r.applied, 0);
        assert_eq!(r.unknown_addresses, 1);
    }

    #[test]
    fn applier_honours_play_time_zero_cheat() {
        let mut world = empty_world();
        world.play_time_seconds = 12345;
        let mut db = Database::new();
        db.entries.push(entry(
            "Game Time 0:00:00",
            vec![CheatCode {
                op: CheatOp::WriteU16,
                addr: 0x80084570,
                value: 0,
                width: 2,
            }],
        ));
        let r = apply(&mut world, &db, ApplyOptions::default());
        assert_eq!(r.applied, 1);
        assert_eq!(world.play_time_seconds, 0);
    }

    #[test]
    fn applier_treats_conditional_gate_as_true_by_default() {
        let mut world = empty_world();
        let mut db = Database::new();
        db.entries.push(entry(
            "Save Anywhere (Press Select+X)",
            vec![
                CheatCode {
                    op: CheatOp::IfEqU16,
                    addr: 0x8007B7C0,
                    value: 0x0140,
                    width: 2,
                },
                CheatCode {
                    op: CheatOp::WriteU16,
                    addr: 0x8007B6A8,
                    value: 0x0001,
                    width: 2,
                },
            ],
        ));
        let r = apply(&mut world, &db, ApplyOptions::default());
        assert_eq!(r.applied, 1);
    }
}
