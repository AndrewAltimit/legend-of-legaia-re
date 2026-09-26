//! Retail capture of the battle tint pass `FUN_8004A908`
//! (`legaia_engine_vm::battle_actor_tint`), the colour word and blend weight
//! both play hosts now stage for every battle body through
//! `World::battle_actor_draw_plan`.
//!
//! For every battle-phase state in `scripts/scenarios.toml` that the save
//! library holds, walk the seven render lists (`0x8007C348 + {4..0x18}`,
//! `0x8007C36C`), take every render-mode-2 node (`+0x56 == 2`) the dispatcher
//! would hand the draw tick (`+0x10 & 0xA` clear, view depth `>= 0xA1`), feed
//! the kernel the seated actor's fields (`*(0x801C9370 + seat*4)`) plus the
//! node's stored view depth, and compare its output with the node's stored
//! `+0x74` / `+0x78`.
//!
//! The pass is deterministic over those inputs, so a mismatch is either a
//! port defect or a node a later writer touched after the frame's draw (a
//! hit flash, a battle still loading). The test pins the match rate rather
//! than every node, and pins that the distance-fade arm is exercised.
//!
//! Capture-gated: skip-passes without `scripts/scenarios.toml` or
//! `saves/library`.

use legaia_engine_vm::battle_actor_tint::{BattleTintInputs, TintArm, battle_actor_tint};
use std::path::PathBuf;

const SEAT_TABLE: u32 = 0x801C_9370;
const LIST_CELLS: [u32; 7] = [
    0x8007_C34C,
    0x8007_C350,
    0x8007_C354,
    0x8007_C358,
    0x8007_C35C,
    0x8007_C360,
    0x8007_C36C,
];
/// `gp[+0xA0C]` - the battle context pointer.
const CTX_PTR: u32 = 0x8007_BD24;
/// `DAT_8007BDA8` - the outdoor-stage flag.
const OUTDOOR: u32 = 0x8007_BDA8;
/// `0x8007BD09 + seat` - the per-seat formation cell.
const FORMATION: u32 = 0x8007_BD09;
/// `DAT_8007BD58` - the battle effect pool is up.
const BATTLE_LIVE: u32 = 0x8007_BD58;

fn find(rel: &str) -> Option<PathBuf> {
    ["", "../", "../../"]
        .iter()
        .map(|p| PathBuf::from(format!("{p}{rel}")))
        .find(|p| p.exists())
}

struct Ram(Vec<u8>);

impl Ram {
    fn ok(a: u32) -> bool {
        (0x8000_0000..0x8020_0000 - 4).contains(&a)
    }
    fn b(&self, a: u32) -> &[u8] {
        &self.0[(a - 0x8000_0000) as usize..]
    }
    fn u8(&self, a: u32) -> u8 {
        self.b(a)[0]
    }
    fn u16(&self, a: u32) -> u16 {
        u16::from_le_bytes([self.b(a)[0], self.b(a)[1]])
    }
    fn i16(&self, a: u32) -> i16 {
        self.u16(a) as i16
    }
    fn u32(&self, a: u32) -> u32 {
        let s = self.b(a);
        u32::from_le_bytes([s[0], s[1], s[2], s[3]])
    }
}

fn battle_nodes(ram: &Ram) -> Vec<u32> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for cell in LIST_CELLS {
        let head = ram.u32(cell);
        if !Ram::ok(head) {
            continue;
        }
        let mut a = ram.u32(head);
        while Ram::ok(a) && seen.insert(a) && out.len() < 4000 {
            if ram.u16(a + 0x56) == 2 {
                out.push(a);
            }
            a = ram.u32(a);
        }
    }
    out
}

#[test]
fn the_tint_pass_reproduces_the_captured_render_words() {
    let (Some(manifest), Some(library)) = (find("scripts/scenarios.toml"), find("saves/library"))
    else {
        eprintln!("[skip] scripts/scenarios.toml or saves/library missing (capture-gated)");
        return;
    };
    let manifest = legaia_mednafen::ScenarioManifest::from_path(&manifest).expect("manifest");
    let (mut states, mut bodies, mut exact, mut depth_arm) = (0, 0, 0, 0);
    for scn in &manifest.scenarios {
        if scn.phase.as_deref() != Some("battle") {
            continue;
        }
        let Some(path) = manifest.library_save_path(scn, library.as_path()) else {
            continue;
        };
        let ram = match path.extension().and_then(|e| e.to_str()) {
            Some("sstate") => legaia_pcsxr::SaveState::from_path(&path)
                .ok()
                .map(|s| s.main_ram().to_vec()),
            _ => legaia_mednafen::SaveState::from_path(&path)
                .ok()
                .and_then(|s| s.main_ram().ok().map(<[u8]>::to_vec)),
        };
        let Some(ram) = ram.map(Ram) else { continue };
        if ram.u8(BATTLE_LIVE) == 0 {
            continue;
        }
        states += 1;
        let ctx = ram.u32(CTX_PTR);
        for node in battle_nodes(&ram) {
            let view_z = ram.u32(node + 0x34) as i32;
            if ram.u32(node + 0x10) & 0xA != 0 || view_z < 0xA1 {
                continue;
            }
            let seat = ram.i16(node + 0x5A);
            let s1 = ram.u32(SEAT_TABLE.wrapping_add((seat as u32).wrapping_mul(4)));
            if !Ram::ok(s1) || !Ram::ok(ram.u32(s1 + 0x22C)) {
                continue;
            }
            let t = battle_actor_tint(&BattleTintInputs {
                lanes: ram.u32(s1 + 0x04),
                top: ram.u32(s1 + 0x08) & 0xFF00_0000,
                blend: ram.u32(s1 + 0x0C),
                render_flag: ram.u8(s1 + 0x21C),
                status: ram.u16(s1 + 0x16E),
                fade: ram.u8(s1 + 0x226),
                radius: ram.i16(ram.u32(s1 + 0x22C) + 0x58),
                view_z,
                seat,
                formation_cell: ram.u8(FORMATION.wrapping_add(seat as u32)),
                outdoor: ram.u8(OUTDOOR) != 0,
                ctx_243: Ram::ok(ctx) && ram.u8(ctx + 0x243) != 0,
                prev_colour: ram.u32(node + 0x74),
                prev_weight: ram.u16(node + 0x78),
            });
            bodies += 1;
            if t.arm == TintArm::DepthCue {
                depth_arm += 1;
            }
            if t.colour == ram.u32(node + 0x74) && t.weight == ram.u16(node + 0x78) {
                exact += 1;
            } else {
                eprintln!(
                    "  {} node {node:#x} seat {seat}: port {:#010x}/{:#06x} retail {:#010x}/{:#06x}",
                    scn.label,
                    t.colour,
                    t.weight,
                    ram.u32(node + 0x74),
                    ram.u16(node + 0x78)
                );
            }
        }
    }
    if states == 0 {
        eprintln!("[skip] no battle state in the library");
        return;
    }
    eprintln!(
        "[ok] {states} battle states, {bodies} drawn bodies, {exact} exact, {depth_arm} on the distance-fade arm"
    );
    assert!(bodies >= 50, "too few bodies to judge ({bodies})");
    assert!(
        exact * 100 >= bodies * 95,
        "tint pass matched {exact} of {bodies} captured bodies"
    );
    assert!(
        depth_arm >= 10,
        "distance-fade arm barely exercised ({depth_arm})"
    );
}
