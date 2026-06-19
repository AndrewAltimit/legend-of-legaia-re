//! Disc + library gated: does the ENEMY Gimard "Fire Tail" boss move run the
//! PROT 0900 screen-WIDGET path, or a move-VM part-actor?
//!
//! Background. Unlike the player summons and the Cort/Delilas/Zeto boss
//! specials - which page a per-spell *stager* into slot B and seat its part
//! records (`enemy_stager_binding`, `summon_render_mode_node`) - the enemy
//! Gimard "Fire Tail" mid-cast frames hold loader-B current-id `5` →
//! extraction PROT 0900, the move-FX / screen-widget module (the same overlay
//! whose mask/sprite/panel/letterbox widget family the ending sequences drive
//! through field-VM op `0x43`; `screen_widgets_live`). The open question was
//! whether Fire Tail runs that 0900 widget path with live parts.
//!
//! FINDING (both catalogued frames, `battle_gimard_tail_fire_a/_b`):
//!
//!   - PROT 0900 IS slot-B resident - byte-exact at the residency pin
//!     (file `0x1628` ↔ VA `0x801F8000`, the move-FX code region; the whole-
//!     file figure is lower only because of the over-read tail + mutated bss).
//!   - The 0900 screen-widget family has **zero** live actors. Fire Tail is
//!     NOT the widget path; the widget family stays ending-scene-exclusive.
//!   - The live effect is a **single** move-VM part-actor in the part pool
//!     `DAT_801C90F0`, ticked per frame by the SCUS actor tick
//!     **`FUN_80021DF4`** (`actor[+0xC]`; the tick that steps the move VM
//!     `FUN_80023070`) - a live capture of that render-tail driver.
//!   - That part's `[i16 model_sel][u16 flags][move-VM bytecode @+4]` record
//!     (`actor[+0x48]`) sits in the **battle overlay (0898)** resident data
//!     (`0x801F5xxx`, below the 0900 slot-B link base `0x801F69D8`), not in a
//!     0900 record. `model_sel` reads `-1` (transform/pivot node) / `5`
//!     (library mesh `DAT_8007C018[5 + base]`) - the summon part-record format.
//!
//! Net: the enemy Fire Tail renders through a move-VM part-actor sourced from
//! battle-overlay data and the generic SCUS part tick, while PROT 0900 is
//! resident but its screen-widget path is dormant. The 0900 widget reading of
//! Fire Tail is falsified.
//!
//! Skip-passes without `LEGAIA_DISC_BIN` / `scripts/scenarios.toml` /
//! `saves/library` / `extracted`.

use std::path::PathBuf;

use legaia_asset::summon_overlay::SUMMON_OVERLAY_LINK_BASE;
use legaia_engine_core::scene::ProtIndex;
use legaia_mednafen::{SaveState, ScenarioManifest, extract::ram_slice};

const FIRETAIL: &[&str] = &["battle_gimard_tail_fire_a", "battle_gimard_tail_fire_b"];

/// Battle overlay loader-B current-id (`*DAT_8007BC4C`).
const LOADER_B_VA: u32 = 0x8007_BC4C;
/// loader-B id 5 → extraction 5 + 895 = PROT 0900.
const FIRETAIL_LOADER_B: u16 = 5;
const FIRETAIL_PROT: u32 = 900;

/// Part-actor pool base (`DAT_801C90F0`, 0x60 u32 slots).
const PART_POOL_VA: u32 = 0x801C_90F0;
const PART_POOL_SLOTS: u32 = 0x60;

/// The per-frame actor tick that steps the move VM (`FUN_80021DF4`).
const ACTOR_TICK: u32 = 0x8002_1DF4;

/// Main-RAM virtual-address range (used to sanity-check actor pointers).
const RAM_RANGE: std::ops::Range<u32> = 0x8000_0000..0x8020_0000;

/// Battle overlay (PROT 0898) resident span `[base, base + 0x29800)`.
const BATTLE_OVL_BASE: u32 = 0x801C_E818;
const BATTLE_OVL_END: u32 = BATTLE_OVL_BASE + 0x2_9800;

/// PROT 0900 screen-widget handler descriptors (`actor[+0xC]`).
const SPRITE: u32 = 0x801F_7A9C;
const MASK: u32 = 0x801F_811C;
const PANEL: u32 = 0x801F_849C;
const LETTERBOX: u32 = 0x801F_8A34;
const LIST_HEADS: u32 = 0x8007_C34C;

fn manifest_path() -> Option<PathBuf> {
    [
        "scripts/scenarios.toml",
        "../scripts/scenarios.toml",
        "../../scripts/scenarios.toml",
    ]
    .into_iter()
    .map(PathBuf::from)
    .find(|p| p.exists())
}
fn library_dir() -> Option<PathBuf> {
    ["saves/library", "../saves/library", "../../saves/library"]
        .into_iter()
        .map(PathBuf::from)
        .find(|d| d.is_dir())
}
fn extracted_root() -> Option<PathBuf> {
    ["extracted", "../extracted", "../../extracted"]
        .into_iter()
        .map(PathBuf::from)
        .find(|d| d.join("PROT.DAT").is_file())
}

fn ru32(ram: &[u8], va: u32) -> u32 {
    let off = (va as usize - 0x8000_0000) & 0x1F_FFFF;
    u32::from_le_bytes(ram[off..off + 4].try_into().unwrap())
}
fn ri16(ram: &[u8], va: u32) -> i16 {
    let off = (va as usize - 0x8000_0000) & 0x1F_FFFF;
    i16::from_le_bytes(ram[off..off + 2].try_into().unwrap())
}

/// Walk the effect-actor list heads; return live (non-killed) 0900-widget
/// actors as `(handler, node)`.
fn live_widgets(ram: &[u8]) -> Vec<(u32, u32)> {
    let mut out: Vec<(u32, u32)> = Vec::new();
    for head in 0..9u32 {
        let mut node = ru32(ram, LIST_HEADS + head * 4);
        let mut hops = 0;
        while RAM_RANGE.contains(&node) && hops < 256 {
            hops += 1;
            let h = ru32(ram, node + 0xC);
            if [SPRITE, MASK, PANEL, LETTERBOX].contains(&h)
                && ru32(ram, node + 0x10) & 8 == 0
                && !out.iter().any(|&(_, n)| n == node)
            {
                out.push((h, node));
            }
            node = ru32(ram, node);
        }
    }
    out
}

#[test]
fn firetail_runs_move_vm_part_not_widget_path() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let (Some(mpath), Some(lib), Some(root)) = (manifest_path(), library_dir(), extracted_root())
    else {
        eprintln!("[skip] scenarios.toml / saves/library / extracted missing");
        return;
    };
    let manifest = ScenarioManifest::from_path(&mpath).expect("parse manifest");
    let index = ProtIndex::open_extracted(&root).expect("open ProtIndex");
    let prot0900 = index
        .entry_bytes_lba_footprint(FIRETAIL_PROT)
        .expect("read PROT 900 footprint");
    let slotb_end = SUMMON_OVERLAY_LINK_BASE + prot0900.len() as u32;

    let mut checked = 0usize;

    for label in FIRETAIL {
        let Some(scn) = manifest.scenarios.iter().find(|s| &s.label == label) else {
            eprintln!("[skip] scenario {label} absent");
            continue;
        };
        let Some(save_path) = manifest.library_save_path(scn, &lib) else {
            eprintln!("[skip] {label}: no mednafen library backup");
            continue;
        };
        let save = SaveState::from_path(&save_path).expect("parse save");
        let ram = save.main_ram().expect("main RAM");

        // (1) loader-B id pins PROT 0900 as the slot-B occupant.
        let lb = u16::from_le_bytes(
            ram_slice(ram, LOADER_B_VA, LOADER_B_VA + 2).unwrap()[..2]
                .try_into()
                .unwrap(),
        );
        assert_eq!(
            lb, FIRETAIL_LOADER_B,
            "{label}: loader-B id 0x{lb:X} != 0x{FIRETAIL_LOADER_B:X} (PROT 0900)",
        );

        // (2) PROT 0900 is byte-exact at the residency pin (code region, the
        // doc's file 0x1628 <-> VA 0x801F8000 anchor).
        let win = 0x400usize;
        let fa = 0x1628usize;
        let rb = ram_slice(ram, 0x801F_8000, 0x801F_8000 + win as u32).unwrap();
        let pinmatch = rb
            .iter()
            .zip(&prot0900[fa..fa + win])
            .filter(|(a, b)| a == b)
            .count();
        assert_eq!(
            pinmatch, win,
            "{label}: PROT 0900 not byte-exact at the residency pin ({pinmatch}/{win})",
        );

        // (3) Zero live 0900 screen-widgets - Fire Tail is not the widget path.
        let widgets = live_widgets(ram);
        assert!(
            widgets.is_empty(),
            "{label}: {} live 0900 widget(s) - Fire Tail unexpectedly drives the widget path",
            widgets.len(),
        );

        // (4) The live effect parts are move-VM part-actors ticked by the SCUS
        // FUN_80021DF4 render-tail, with records in the battle overlay (0898),
        // never the 0900 slot-B record table.
        let pool = ram_slice(ram, PART_POOL_VA, PART_POOL_VA + PART_POOL_SLOTS * 4).unwrap();
        let mut live_parts = 0usize;
        for k in 0..PART_POOL_SLOTS as usize {
            let actor = u32::from_le_bytes(pool[k * 4..k * 4 + 4].try_into().unwrap());
            if !RAM_RANGE.contains(&actor) {
                continue;
            }
            live_parts += 1;

            let handler = ru32(ram, actor + 0xC);
            assert_eq!(
                handler, ACTOR_TICK,
                "{label}: pool slot {k} handler 0x{handler:08X} != FUN_80021DF4 (the move-VM tick)",
            );

            let rec = ru32(ram, actor + 0x48);
            assert!(
                (BATTLE_OVL_BASE..BATTLE_OVL_END).contains(&rec),
                "{label}: slot {k} record 0x{rec:08X} not in the battle overlay (0898) span",
            );
            assert!(
                !(SUMMON_OVERLAY_LINK_BASE..slotb_end).contains(&rec),
                "{label}: slot {k} record 0x{rec:08X} lands in the 0900 slot-B window - \
                 Fire Tail DOES seat a 0900 record; re-open the thread",
            );

            // The record is the summon part-record format: model_sel is -1
            // (transform node) or a small library-mesh index.
            let model_sel = ri16(ram, rec);
            assert!(
                model_sel == -1 || (0..=0x40).contains(&model_sel),
                "{label}: slot {k} record model_sel {model_sel} is not a transform/library node",
            );

            eprintln!(
                "{label}: part actor 0x{actor:08X} tick=FUN_80021DF4, record 0x{rec:08X} \
                 (0898 data) model_sel={model_sel}",
            );
        }
        assert!(
            live_parts >= 1,
            "{label}: no live part-actor - expected the Fire Tail move-FX part",
        );

        eprintln!("{label}: PROT 0900 resident, 0 widgets, {live_parts} move-VM part(s)");
        checked += 1;
    }

    if checked == 0 {
        eprintln!("[skip] no Fire-Tail mid-cast states available");
    }
}
