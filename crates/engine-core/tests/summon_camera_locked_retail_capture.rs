//! Camera-locked (`+0x52 & 0x400`) move-VM parts sit where **their own
//! program** puts them, not at the cast target.
//!
//! `FUN_8001CF50`'s `0x400` arm (tested first, `0x8001CF7C`) loads the base
//! matrix alone and writes `+0x2C = S_b * (+0x14)` through `FUN_8003D344`,
//! so a camera-locked node's `+0x14` trio is an **eye-space offset**. The
//! engine's summon scene used to run every part through its translation glide,
//! which snaps the position to `origin + anim bank` - the cast target's world
//! coordinates - so both hosts locked those parts to the eye at the target's
//! coordinates read as an offset. Retail's part tick has no glide: its
//! motion block (`FUN_80021DF4` `0x800228A0..0x80022B90`) integrates
//! `+0x3C..+0x40` as velocities, and the part's op `0x07` WORLD_SET places it.
//!
//! The oracle is three retail mid-cast states whose actor lists carry
//! camera-locked parts. For each, every distinct `+0x14` trio of a
//! camera-locked part-tick node (`+0x0C == FUN_80021DF4`) in the retail RAM is
//! checked against the engine's summon scene for the same stager, spawned at a
//! deliberately non-zero cast-target origin:
//!
//! | state | stager | retail camera-locked `+0x14` |
//! |---|---|---|
//! | `cort_mystic_circle_mid_cast` | PROT 0938 | `(0, -192, 1536)` |
//! | `cort_evolved_ultra_charge_mid_cast` | PROT 0962 | `(0, 0, 256)` |
//! | `horn_summon_mid_cast` | PROT 0930 | twelve points on the `z = 2048` plane |
//!
//! The first two must be reached exactly. Horn's ring is spawned several times
//! by its stager with per-spawn velocities the scene does not model, so its
//! leg checks the plane the whole ring lives on - the WORLD_SET start.
//!
//! Needs the extracted `PROT.DAT` (`LEGAIA_EXTRACTED_DIR`, else
//! `extracted/`) and the mednafen save library (`LEGAIA_SAVES_LIBRARY`, else
//! `saves/library`); skips and passes without either.

use legaia_asset::summon_overlay::{self, SUMMON_OVERLAY_LINK_BASE};
use legaia_engine_core::summon::{SummonScene, is_camera_locked};
use legaia_engine_vm::move_vm::MoveHost;
use legaia_mednafen::SaveState;
use legaia_prot::archive::Archive;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

struct LutHost;
impl MoveHost for LutHost {
    fn rotation_lut(&self, index: u16) -> (i16, i16) {
        let a = (index as f64) * std::f64::consts::TAU / 4096.0;
        ((a.sin() * 4096.0) as i16, (a.cos() * 4096.0) as i16)
    }
}

/// The battle actor-list heads the frame passes walk (next pointer at `+0`).
const LIST_HEADS: [u32; 6] = [
    0x8007_C34C,
    0x8007_C350,
    0x8007_C354,
    0x8007_C358,
    0x8007_C35C,
    0x8007_C360,
];
/// A move-VM part's per-frame tick, in its `+0x0C` callback word.
const PART_TICK: u32 = 0x8002_1DF4;
/// A cast target well away from the eye, so an origin that leaks into a
/// camera-locked position cannot coincide with the retail offset.
const ORIGIN: [i16; 3] = [700, -100, -900];

fn prot_dat() -> Option<PathBuf> {
    if let Ok(d) = std::env::var("LEGAIA_EXTRACTED_DIR") {
        let p = PathBuf::from(d).join("PROT.DAT");
        if p.is_file() {
            return Some(p);
        }
    }
    ["extracted", "../extracted", "../../extracted"]
        .iter()
        .map(|b| PathBuf::from(b).join("PROT.DAT"))
        .find(|p| p.is_file())
}

fn library() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("LEGAIA_SAVES_LIBRARY") {
        let p = PathBuf::from(p).join("mednafen");
        if p.is_dir() {
            return Some(p);
        }
    }
    ["", "../", "../../"]
        .iter()
        .map(|p| PathBuf::from(format!("{p}saves/library/mednafen")))
        .find(|p| p.is_dir())
}

/// Distinct `+0x14` trios of the camera-locked part nodes on the retail
/// actor lists.
fn retail_locked_positions(lib: &Path, prefix: &str) -> Option<BTreeSet<[i16; 3]>> {
    let path = std::fs::read_dir(lib)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .find(|p| {
            p.file_name()
                .is_some_and(|f| f.to_string_lossy().starts_with(prefix))
        })?;
    let state = SaveState::from_path(&path).ok()?;
    let ram = state.main_ram().ok()?;
    let o = |va: u32| (va & 0x001F_FFFF) as usize;
    let u32_at = |va: u32| u32::from_le_bytes(ram[o(va)..o(va) + 4].try_into().unwrap());
    let u16_at = |va: u32| u16::from_le_bytes(ram[o(va)..o(va) + 2].try_into().unwrap());
    let mut out = BTreeSet::new();
    for head in LIST_HEADS {
        let mut a = u32_at(head);
        let mut n = 0;
        while a != 0 && (0x8000_0000..0x8020_0000).contains(&a) && n < 500 {
            if u32_at(a + 0x0C) == PART_TICK && u16_at(a + 0x52) & 0x400 != 0 {
                out.insert([0x14, 0x16, 0x18].map(|k| u16_at(a + k) as i16));
            }
            a = u32_at(a);
            n += 1;
        }
    }
    Some(out)
}

/// Every position a camera-locked part of stager `idx` takes over `frames`
/// ticks of the engine's summon scene, spawned at [`ORIGIN`].
fn engine_locked_positions(prot: &Path, idx: usize, frames: u32) -> BTreeSet<[i16; 3]> {
    let mut archive = Archive::open(prot).expect("open PROT.DAT");
    let entry = archive.entries[idx].clone();
    let mut bytes = Vec::new();
    archive.read_entry(&entry, &mut bytes).expect("read stager");
    let overlay = summon_overlay::parse(&bytes, SUMMON_OVERLAY_LINK_BASE);
    let mut scene = SummonScene::spawn(&overlay, &bytes, 0, ORIGIN);
    let mut out = BTreeSet::new();
    for _ in 0..frames {
        scene.tick(&mut LutHost, 8);
        for p in &scene.parts {
            if is_camera_locked(&p.state) {
                out.insert([p.state.world_x, p.state.world_y, p.state.world_z]);
            }
        }
    }
    out
}

#[test]
fn camera_locked_parts_reach_the_retail_eye_offsets() {
    let (Some(prot), Some(lib)) = (prot_dat(), library()) else {
        eprintln!(
            "[skip] needs extracted/PROT.DAT and saves/library (LEGAIA_EXTRACTED_DIR / LEGAIA_SAVES_LIBRARY)"
        );
        return;
    };
    // (state backup-fingerprint prefix, label, stager PROT entry)
    let exact = [
        ("0ba3c2ba", "cort_mystic_circle_mid_cast", 938usize),
        ("baf51cc2", "cort_evolved_ultra_charge_mid_cast", 962),
    ];
    let mut ran = 0;
    for (prefix, label, idx) in exact {
        let Some(retail) = retail_locked_positions(&lib, prefix) else {
            eprintln!("[skip] {label} not in the library");
            continue;
        };
        assert!(
            !retail.is_empty(),
            "{label}: retail carries camera-locked parts"
        );
        let engine = engine_locked_positions(&prot, idx, 2000);
        println!(
            "{label} (PROT {idx:04}): retail {retail:?}; engine reaches {} positions",
            engine.len()
        );
        for r in &retail {
            assert!(
                engine.contains(r),
                "{label}: the camera-locked part never reaches the retail eye offset {r:?} \
                 (engine positions include the cast-target origin {ORIGIN:?}?)"
            );
        }
        ran += 1;
    }

    // Horn: the retail ring lies on one eye-space plane, and the engine's
    // camera-locked parts start on it.
    if let Some(retail) = retail_locked_positions(&lib, "0df8c64d") {
        let planes: BTreeSet<i16> = retail.iter().map(|p| p[2]).collect();
        assert_eq!(planes.len(), 1, "Horn's ring is one eye-space plane");
        let z = *planes.first().unwrap();
        let engine = engine_locked_positions(&prot, 930, 2000);
        println!(
            "horn_summon_mid_cast (PROT 0930): retail ring {} points on z = {z}; engine {:?}",
            retail.len(),
            engine
        );
        assert!(
            engine.contains(&[0, 0, z]),
            "Horn's camera-locked parts start at the WORLD_SET point (0, 0, {z})"
        );
        ran += 1;
    }
    assert!(ran > 0, "no library state was found to compare against");
}
