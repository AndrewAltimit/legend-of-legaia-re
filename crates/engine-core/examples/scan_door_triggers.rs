//! Audit a scene's door/teleport trigger surface: every `.MAP` kind-1 tile
//! trigger (gate 0 = partition-0 object bind, gate 1 = partition-2 record
//! spawn) joined to the record it references and every **player-channel**
//! move op that record's script carries.
//!
//! The three op forms that reposition the player (all cross-context into the
//! `0xF8` player system channel):
//!
//! - `A3 F8 <xb> <zb>`             - op `0x23` MOVE_TO (instant teleport)
//! - `CC F8 51 <xb> <zb> <d> <m>`  - op `0x4C` nibble-5 sub-1 (teleport + move anim)
//! - `C7 F8 <xb> <zb> <mode>`      - op `0x47` walk-to-tile (animated walk)
//!
//! Run with:
//!   cargo run --release -p legaia-engine-core --example scan_door_triggers -- town01
//! With no args, sweeps every CDNAME scene and prints a class census.

use std::collections::BTreeMap;
use std::path::PathBuf;

use legaia_engine_core::man_field_scripts::{self as mfs, PlayerMoveKind};
use legaia_engine_core::scene::{ProtIndex, Scene};

fn main() -> anyhow::Result<()> {
    let extracted = PathBuf::from("extracted");
    let p = ProtIndex::open_extracted(&extracted)?;
    let cdname = legaia_prot::cdname::parse(&extracted.join("CDNAME.TXT"))?;
    let mut all: Vec<String> = cdname.values().cloned().collect();
    all.sort();
    all.dedup();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let verbose = !args.is_empty();
    let want: Vec<String> = if args.is_empty() { all } else { args };

    let mut totals: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut scenes_with_doors = 0usize;
    for name in &want {
        let Ok(scene) = Scene::load(&p, name) else {
            continue;
        };
        let Ok(Some(man)) = scene.field_man_payload(&p) else {
            continue;
        };
        let Ok(mf) = legaia_asset::man_section::parse(&man) else {
            continue;
        };
        let (primary, fallback) = scene.field_tile_triggers(&p).unwrap_or_default();
        let triggers: Vec<_> = primary.iter().chain(fallback.iter()).copied().collect();
        if triggers.is_empty() {
            continue;
        }
        if verbose {
            println!("\n=== {name} ===");
        }
        let mut any = false;
        let mut by: BTreeMap<(u8, u8), Vec<(u8, u8)>> = BTreeMap::new();
        for t in &triggers {
            by.entry((t.gate, t.record))
                .or_default()
                .push((t.tile_x, t.tile_z));
        }
        for ((gate, record), tiles) in &by {
            let moves = if *gate == 0 {
                mfs::p0_record_player_moves(&mf, &man, *record as usize)
            } else {
                mfs::p2_record_player_moves(&mf, &man, *record as usize)
            };
            let class = match moves.first().map(|m| m.kind) {
                Some(PlayerMoveKind::MoveTo) => "teleport-0x23",
                Some(PlayerMoveKind::NpcRun) => "teleport-4C51",
                Some(PlayerMoveKind::WalkTo) => "walk-0x47",
                None => {
                    if *gate == 0 {
                        "gate0-object"
                    } else {
                        "gate1-beat"
                    }
                }
            };
            if moves.is_empty() && !verbose {
                *totals.entry(class).or_default() += 1;
                continue;
            }
            any = true;
            *totals.entry(class).or_default() += 1;
            if verbose {
                let detail: Vec<String> = moves
                    .iter()
                    .map(|m| {
                        format!(
                            "{:?}({},{}){}",
                            m.kind,
                            m.world_x / 128,
                            m.world_z / 128,
                            m.facing.map(|f| format!(" f={f:#x}")).unwrap_or_default()
                        )
                    })
                    .collect();
                println!(
                    "  gate {gate} rec {record:>3} tiles {tiles:?}  {class:14} {}",
                    detail.join(" ")
                );
            }
        }
        if any {
            scenes_with_doors += 1;
        }
    }
    println!("\n=== class census (scenes with >=1 player-move door: {scenes_with_doors}) ===");
    for (k, v) in &totals {
        println!("  {k:20} {v}");
    }
    Ok(())
}
