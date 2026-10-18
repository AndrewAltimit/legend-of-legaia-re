//! Disc-gated sweep of the battle mesh-variant swap's inputs
//! ([`legaia_engine_battle::battle_attach_swap`]) over every single-section
//! equipment assembly of the four player files.
//!
//! What it pins:
//!
//! - every assembly carries exactly the two `0xFF` variants
//!   `FUN_800513F0` snapshots (so [`AttachPairs::from_assembly`] never
//!   declines on retail data), sorted to `nobj - 2` / `nobj - 1`;
//! - the pinned ordinal is non-zero exactly where an assembly carries a
//!   `0xFE` surplus, and a swing stream with more parts than the idle stream
//!   exists only there (the extra-channel escape's precondition);
//! - live `+0xA4` windows exist only in Noa's file, and on her default
//!   assembly a window does put a geometrically different object in the
//!   attach channel - the case the model table exists to draw.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset or `extracted/PROT` is
//! missing.

use std::path::{Path, PathBuf};

use legaia_actor_formats::{battle_char_assembly as bca, battle_data_pack};
use legaia_engine_battle::battle_attach_swap::{AttachPairs, BattleModelTable};

const FILES: [(&str, &str); 4] = [
    ("0863_edstati3.BIN", "Vahn"),
    ("0864_edstati3.BIN", "Noa"),
    ("0865_battle_data.BIN", "Gala"),
    ("0866_battle_data.BIN", "Terra"),
];

fn prot_dir() -> Option<PathBuf> {
    [
        PathBuf::from("extracted/PROT"),
        PathBuf::from("../../extracted/PROT"),
    ]
    .into_iter()
    .find(|p| p.is_dir())
}

/// The equipment ids each of the five sections offers, in table order.
fn section_ids(pack: &battle_data_pack::BattleDataPack) -> Vec<Vec<u8>> {
    let mut out: Vec<Vec<u8>> = vec![Vec::new(); bca::SECTION_COUNT];
    let mut slot = 0usize;
    for r in &pack.records {
        if slot >= bca::SECTION_COUNT {
            break;
        }
        if r.id == 0 {
            slot += 1;
        } else {
            out[slot].push(r.id as u8);
        }
    }
    out
}

fn find_file(dir: &Path, prefix: &str) -> Option<PathBuf> {
    let want = &prefix[..4];
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(want))
        })
}

#[test]
fn every_assembly_feeds_the_variant_swap() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let Some(dir) = prot_dir() else {
        eprintln!("[skip] extracted/PROT missing");
        return;
    };
    let mut assemblies = 0usize;
    let mut escapes = 0usize;
    for (file, who) in FILES {
        let Some(path) = find_file(&dir, file) else {
            eprintln!("[skip] {file} missing");
            continue;
        };
        let raw = std::fs::read(&path).expect("read player file");
        let pack = battle_data_pack::parse(&raw).expect("parse player file");
        let idle = bca::idle_battle_animation(&raw)
            .expect("idle decode")
            .expect("idle stream");
        let idle_parts = idle.variant_track.stream_parts;
        assert_eq!(idle_parts as usize, idle.part_count, "{who}: idle parts");
        let anims = bca::battle_animations(&raw).expect("action streams");
        let windowed = anims.iter().any(|a| a.variant_track.windows != [0; 8]);
        assert_eq!(windowed, who == "Noa", "{who}: live +0xA4 windows");

        let ids = section_ids(&pack);
        let mut loadouts = vec![[0u8; 5]];
        for (s, list) in ids.iter().enumerate() {
            for &id in list {
                let mut e = [0u8; 5];
                e[s] = id;
                loadouts.push(e);
            }
        }
        for eq in loadouts {
            let Ok(asm) = bca::assemble_character(&raw, &pack, &eq) else {
                continue;
            };
            assemblies += 1;
            let nobj = asm.bone_tags.len();
            assert_eq!(asm.attach_bones.len(), 2, "{who} {eq:?}: two 0xFF variants");
            assert_eq!(asm.bone_tags[nobj - 2], 200, "{who} {eq:?}");
            assert_eq!(asm.bone_tags[nobj - 1], 201, "{who} {eq:?}");
            let pairs = AttachPairs::from_assembly(nobj, &asm.attach_bones, asm.pinned_pair)
                .expect("pairs");
            let has_fe = asm.bone_tags.iter().any(|&t| (100..200).contains(&t));
            assert_eq!(
                asm.pinned_pair != 0,
                has_fe,
                "{who} {eq:?}: pinned iff 0xFE"
            );
            let swings = bca::swing_battle_animations(&raw, &pack, &eq).unwrap_or_default();
            for s in &swings {
                let parts = s.anim.variant_track.stream_parts;
                if parts != idle_parts {
                    assert!(has_fe, "{who} {eq:?}: surplus-channel swing with no 0xFE");
                    escapes += 1;
                    let mut t = BattleModelTable::new(nobj, pairs, idle_parts);
                    t.step(&s.anim.variant_track.windows, 0, parts);
                    let drawn = t.drawn_objects(parts as usize);
                    let pin = pairs.pairs[asm.pinned_pair as usize - 1];
                    assert!(drawn[pin.variant_object as usize], "{who}: escape variant");
                    assert!(drawn[idle_parts as usize], "{who}: 0xFE extra drawn");
                }
            }
        }
    }
    eprintln!("{assemblies} assemblies, {escapes} surplus-channel swings");
    if assemblies > 0 {
        assert!(escapes > 0, "the disc carries surplus-channel swings");
    }
}

/// On Noa's default assembly a windowed clip draws the `0xFF` variant in the
/// attach channel, and that object is not her attach bone's own mesh.
#[test]
fn noa_window_draws_a_different_object() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let Some(dir) = prot_dir() else {
        eprintln!("[skip] extracted/PROT missing");
        return;
    };
    let Some(path) = find_file(&dir, "0864") else {
        eprintln!("[skip] 0864 missing");
        return;
    };
    let raw = std::fs::read(&path).expect("read");
    let pack = battle_data_pack::parse(&raw).expect("parse");
    let asm = bca::assemble_character(&raw, &pack, &[0; 5]).expect("assemble");
    let tmd = legaia_tmd::parse(&asm.tmd).expect("tmd");
    let nobj = asm.bone_tags.len();
    let idle = bca::idle_battle_animation(&raw).unwrap().unwrap();
    let pairs = AttachPairs::from_assembly(nobj, &asm.attach_bones, asm.pinned_pair).unwrap();
    let anims = bca::battle_animations(&raw).unwrap();
    let mut swapped = 0usize;
    for a in &anims {
        let parts = a.variant_track.stream_parts;
        for f in 0..a.frame_count as i16 {
            let mut t = BattleModelTable::new(nobj, pairs, idle.variant_track.stream_parts);
            let swap = t.step(&a.variant_track.windows, f, parts);
            for w in swap
                .writes
                .iter()
                .filter(|w| w.object == pairs.pairs[w.pair].variant_object)
            {
                let host = &tmd.objects[pairs.pairs[w.pair].default_object as usize];
                let var = &tmd.objects[w.object as usize];
                let same = host.vertices.len() == var.vertices.len()
                    && host
                        .vertices
                        .iter()
                        .zip(&var.vertices)
                        .all(|(x, y)| (x.x, x.y, x.z) == (y.x, y.y, y.z));
                if !same {
                    swapped += 1;
                }
                let drawn = t.drawn_objects(parts as usize);
                assert!(drawn[w.object as usize]);
                assert!(!drawn[w.channel as usize]);
            }
        }
    }
    eprintln!("Noa: {swapped} windowed frames draw a geometrically distinct variant");
    assert!(swapped > 0);
}
