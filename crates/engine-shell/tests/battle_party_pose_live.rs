//! Disc-gated pins for the **assembled battle party pose** - the play-window
//! battle render socketing fix.
//!
//! The assembled battle mesh (one TMD spliced per character from their
//! player battle file's equipment sections, objects sorted by bone tag) is
//! posed by the character's **own idle keyframe stream** from record[0] of
//! the same player file (`legaia_asset::battle_char_assembly::
//! idle_battle_animation` - monster-format `[parts][frames][9-byte TRS]`,
//! `parts` = skeleton bones, channel `i` driving object `i`; equipment
//! extras ride their attach bone via `anm_bones`). PROT 1203 is NOT that
//! source: its banks are authored against PROT 1204's own object order,
//! which differs from the assembled tag order per character, so posing the
//! assembled mesh from 1203 mis-sockets joints and splits the duplicate
//! equipment pieces apart (the play-window bug this pins).
//!
//! Two oracles:
//!
//! 1. [`assembled_party_pose_matches_baka_reference`] (disc only): the
//!    assembled mesh posed by its own idle frame 0 must land each
//!    byte-shared piece at the same world centroid as the **reference
//!    pipeline** (the site's character viewer): the PROT 1204 pack posed
//!    by PROT 1203's per-character idle record, bone `i` -> 1204 object
//!    `i`. Also pins that every duplicate equipment extra coincides
//!    exactly with its attach piece (no visible duplication).
//! 2. [`assembled_party_matches_live_battle`] (disc + save library): the
//!    catalogued mid-battle capture's registered blobs, side tables and
//!    the live anim context's idle stream byte-match the disc decode -
//!    pinning both the assembly and the pose **source** against retail
//!    RAM.
//!
//! Skip-passes without `LEGAIA_DISC_BIN` / `extracted/PROT` (and, for the
//! live test, `scripts/scenarios.toml` + `saves/library`).

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

use legaia_asset::battle_char_assembly;
use legaia_asset::monster_archive::MonsterAnimation;
use legaia_mednafen::{SaveState, ScenarioManifest};

const PLAYER_FILES: [&str; 3] = [
    "0863_edstati3.BIN",
    "0864_edstati3.BIN",
    "0865_battle_data.BIN",
];

fn prot_dir() -> Option<PathBuf> {
    [
        PathBuf::from("extracted/PROT"),
        PathBuf::from("../../extracted/PROT"),
    ]
    .into_iter()
    .find(|p| p.is_dir())
}

/// Hash an object's vertex (x, y, z) triplets (the byte-sharing key used to
/// match assembled pieces against the PROT 1204 pack / the live blob).
fn vert_hash(obj: &legaia_tmd::Object) -> (usize, u64) {
    let mut h = DefaultHasher::new();
    for v in &obj.vertices {
        (v.x, v.y, v.z).hash(&mut h);
    }
    (obj.vertices.len(), h.finish())
}

/// World centroid of an object's vertices under the rigid pose
/// `Rz·Ry·Rx · v + t` (the retail per-object GTE composition, 12-bit
/// angles) - the same math as `legaia_tmd::mesh::tmd_to_vram_mesh_posed_rot`.
fn posed_centroid(obj: &legaia_tmd::Object, t: [i16; 3], r: [i16; 3]) -> [f32; 3] {
    const A2R: f32 = std::f32::consts::TAU / 4096.0;
    let (sx, cx) = (r[0] as f32 * A2R).sin_cos();
    let (sy, cy) = (r[1] as f32 * A2R).sin_cos();
    let (sz, cz) = (r[2] as f32 * A2R).sin_cos();
    let mut acc = [0.0f64; 3];
    for v in &obj.vertices {
        let (mut x, mut y, mut z) = (v.x as f32, v.y as f32, v.z as f32);
        let (ny, nz) = (y * cx - z * sx, y * sx + z * cx);
        y = ny;
        z = nz;
        let (nx, nz) = (x * cy + z * sy, -x * sy + z * cy);
        x = nx;
        z = nz;
        let (nx, ny) = (x * cz - y * sz, x * sz + y * cz);
        acc[0] += (nx + t[0] as f32) as f64;
        acc[1] += (ny + t[1] as f32) as f64;
        acc[2] += (z + t[2] as f32) as f64;
    }
    let n = obj.vertices.len().max(1) as f64;
    [
        (acc[0] / n) as f32,
        (acc[1] / n) as f32,
        (acc[2] / n) as f32,
    ]
}

/// Pose `(t, r)` for object `i` from frame 0 of an object-expanded idle
/// animation.
fn frame0_pose(anim: &MonsterAnimation, i: usize) -> ([i16; 3], [i16; 3]) {
    let p = &anim.frames[0][i];
    ([p.tx, p.ty, p.tz], [p.rx as i16, p.ry as i16, p.rz as i16])
}

#[test]
fn assembled_party_pose_matches_baka_reference() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let Some(dir) = prot_dir() else {
        eprintln!("[skip] extracted/PROT missing");
        return;
    };
    // Reference pipeline inputs: the PROT 1204 pack + the PROT 1203 battle
    // ANM bundle (per-character banks; the first record of each bank is the
    // idle whose frame 0 the reference poses with, bone i -> 1204 object i).
    let pack1204 = std::fs::read(dir.join("1204_other5.BIN")).expect("read 1204");
    let bcp = legaia_asset::battle_char_pack::parse(&pack1204).expect("parse 1204");
    let raw1203 = std::fs::read(dir.join("1203_other5.BIN")).expect("read 1203");
    let anm1203 = [6usize, 3, 5, 7]
        .iter()
        .find_map(|&dc| {
            legaia_asset::player_anm::find_in_entry(&raw1203, dc)
                .into_iter()
                .next()
        })
        .expect("PROT 1203 ANM bundle");
    const BANK_START: [usize; 3] = [0, 9, 18];

    // The two streams quantize rotations differently (the player-file TRS
    // stream carries full 12-bit angles; 1203 stores u8 << 4), so allow a
    // small per-axis drift. A mis-socketed mapping (e.g. the old
    // tag-as-1203-bone bug) displaces pieces by whole limb offsets
    // (tens-hundreds of units), far past this.
    const TOL: f32 = 4.0;

    for (cslot, file) in PLAYER_FILES.iter().enumerate() {
        let raw = std::fs::read(dir.join(file)).expect("read player file");
        let pack = legaia_asset::battle_data_pack::parse(&raw).expect("parse pack");
        let asm = battle_char_assembly::assemble_character(&raw, &pack, &[0; 5])
            .expect("assemble defaults");
        let tmd = legaia_tmd::parse(&asm.tmd).expect("parse assembled TMD");
        let idle = battle_char_assembly::idle_battle_animation(&raw)
            .expect("idle decode")
            .expect("idle present");
        let expanded = battle_char_assembly::expand_animation_for_objects(&idle, &asm.anm_bones);
        assert_eq!(expanded.part_count, tmd.objects.len(), "{file}: expansion");

        // Engine-path frame-0 centroid per assembled object.
        let engine_centroids: Vec<[f32; 3]> = tmd
            .objects
            .iter()
            .enumerate()
            .map(|(i, o)| {
                let (t, r) = frame0_pose(&expanded, i);
                posed_centroid(o, t, r)
            })
            .collect();

        // Reference-path centroid per 1204 object (identity bone mapping).
        let slot = bcp.slot(cslot).expect("1204 slot");
        let rtmd = legaia_tmd::parse(&slot.tmd_bytes).expect("parse 1204 TMD");
        let rec = BANK_START[cslot];
        let ref_centroids: Vec<[f32; 3]> = rtmd
            .objects
            .iter()
            .enumerate()
            .map(|(j, o)| {
                let bt = anm1203
                    .bone_transform(rec, 0, j)
                    .expect("1203 bone transform");
                posed_centroid(
                    o,
                    [bt.t_x as i16, bt.t_y as i16, bt.t_z as i16],
                    [bt.r_x as i16, bt.r_y as i16, bt.r_z as i16],
                )
            })
            .collect();

        // Match byte-shared pieces and compare world placement across the
        // two pipelines.
        let rhashes: Vec<(usize, u64)> = rtmd.objects.iter().map(vert_hash).collect();
        let mut shared = 0usize;
        for (i, o) in tmd.objects.iter().enumerate() {
            let key = vert_hash(o);
            let matches: Vec<usize> = rhashes
                .iter()
                .enumerate()
                .filter(|&(_, &k)| k == key)
                .map(|(j, _)| j)
                .collect();
            let [j] = matches[..] else {
                continue; // not byte-shared (equipped variant / extra), or ambiguous
            };
            shared += 1;
            let (e, r) = (engine_centroids[i], ref_centroids[j]);
            for k in 0..3 {
                assert!(
                    (e[k] - r[k]).abs() < TOL,
                    "{file}: object {i} (tag {}) centroid {e:?} != reference 1204 obj {j} {r:?}",
                    asm.bone_tags[i]
                );
            }
        }
        let skeleton = asm.bone_tags.iter().filter(|&&t| t < 100).count();
        assert!(
            shared >= skeleton,
            "{file}: only {shared} byte-shared pieces matched (skeleton = {skeleton})"
        );

        // No visible duplication: every 200+ equipment extra whose vertex
        // pool byte-matches its attach piece must land at exactly the same
        // world centroid (same pose channel => coincident copies).
        for (i, &tag) in asm.bone_tags.iter().enumerate() {
            if tag < 200 {
                continue;
            }
            let attach = asm.attach_bones[(tag - 200) as usize];
            let attach_idx = asm
                .bone_tags
                .iter()
                .position(|&t| t == attach)
                .expect("attach piece present");
            if vert_hash(&tmd.objects[i]) == vert_hash(&tmd.objects[attach_idx]) {
                assert_eq!(
                    engine_centroids[i], engine_centroids[attach_idx],
                    "{file}: duplicate extra {i} must coincide with attach piece {attach_idx}"
                );
            }
        }
        eprintln!(
            "{file}: {} objects, {shared} byte-shared pieces within {TOL} of the reference pose",
            tmd.objects.len()
        );
    }
}

/// RAM offsets of the live battle globals (see `docs/reference/memory-map.md`
/// and `docs/subsystems/battle.md`).
const PARTY_BASE_INDEX: u32 = 0x8007_B824;
const TMD_POINTER_TABLE: u32 = 0x8007_C018;
const PARTY_IDS: u32 = 0x8007_BD10;
const CHAR_RECORDS: u32 = 0x8008_4708;
const CHAR_STRIDE: u32 = 0x414;
const EQUIP_OFF: u32 = 0x196;
/// Per-party-slot pointer to the character's decoded record[0] image (the
/// battle loader's per-character work area; `+0x50` holds the assembled
/// blob base, which this test cross-checks before trusting the table).
const RECORD0_BASES: u32 = 0x801C_9360;

const RAM_BASE: u32 = 0x8000_0000;

fn ru32(ram: &[u8], va: u32) -> u32 {
    let off = (va - RAM_BASE) as usize & 0x1F_FFFF;
    u32::from_le_bytes(ram[off..off + 4].try_into().unwrap())
}
fn rbytes(ram: &[u8], va: u32, n: usize) -> &[u8] {
    let off = (va - RAM_BASE) as usize & 0x1F_FFFF;
    &ram[off..off + n]
}

#[test]
fn assembled_party_matches_live_battle() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let Some(dir) = prot_dir() else {
        eprintln!("[skip] extracted/PROT missing");
        return;
    };
    let Some(manifest_path) = ["scripts/scenarios.toml", "../../scripts/scenarios.toml"]
        .into_iter()
        .map(PathBuf::from)
        .find(|p| p.exists())
    else {
        eprintln!("[skip] scenarios.toml missing");
        return;
    };
    let Some(library) = ["saves/library", "../../saves/library"]
        .into_iter()
        .map(PathBuf::from)
        .find(|p| p.is_dir())
    else {
        eprintln!("[skip] saves/library missing");
        return;
    };
    let manifest = ScenarioManifest::from_path(&manifest_path).expect("parse manifest");
    let Some(sc) = manifest
        .scenarios
        .iter()
        .find(|s| s.label == "party_battle_gobu_gobu")
    else {
        eprintln!("[skip] party_battle_gobu_gobu scenario missing");
        return;
    };
    let Ok(path) = manifest.mednafen_save_path(sc, Some(library.as_path())) else {
        eprintln!("[skip] no save path");
        return;
    };
    if !path.exists() {
        eprintln!("[skip] {} missing", path.display());
        return;
    }
    let save = SaveState::from_path(&path).expect("load savestate");
    let ram = save.main_ram().expect("main ram");

    let base = ru32(ram, PARTY_BASE_INDEX);
    for (slot, file) in PLAYER_FILES.iter().enumerate() {
        let blob = ru32(ram, TMD_POINTER_TABLE + (base + slot as u32) * 4);
        assert_eq!(blob & 0xFF00_0000, 0x8000_0000, "{file}: live blob ptr");
        assert_eq!(ru32(ram, blob), 0x8000_0002, "{file}: live blob magic");
        let live_nobj = ru32(ram, blob + 8) as usize;

        // Assemble from the disc with the capture's own equipped ids.
        let raw = std::fs::read(dir.join(file)).expect("read player file");
        let pack = legaia_asset::battle_data_pack::parse(&raw).expect("parse pack");
        let pid = rbytes(ram, PARTY_IDS, 3)[slot];
        assert!(pid > 0, "{file}: party slot {slot} empty in capture");
        let rec = CHAR_RECORDS + (pid as u32 - 1) * CHAR_STRIDE;
        let mut equipped = [0u8; 5];
        equipped.copy_from_slice(rbytes(ram, rec + EQUIP_OFF, 5));
        let asm = battle_char_assembly::assemble_character(&raw, &pack, &equipped)
            .expect("assemble live loadout");
        let tmd = legaia_tmd::parse(&asm.tmd).expect("parse assembled TMD");
        assert_eq!(tmd.objects.len(), live_nobj, "{file}: nobj vs live blob");

        // Per-index vertex pools byte-match the live registered blob: the
        // assembled object ORDER is the retail order.
        for (i, o) in tmd.objects.iter().enumerate() {
            let e = blob + 12 + i as u32 * 0x1C;
            let vert_top = ru32(ram, e);
            let n_vert = ru32(ram, e + 4) as usize;
            assert_eq!(n_vert, o.vertices.len(), "{file}: obj {i} vert count");
            let live = rbytes(ram, vert_top, n_vert * 8);
            for (k, v) in o.vertices.iter().enumerate() {
                let lx = i16::from_le_bytes([live[k * 8], live[k * 8 + 1]]);
                let ly = i16::from_le_bytes([live[k * 8 + 2], live[k * 8 + 3]]);
                let lz = i16::from_le_bytes([live[k * 8 + 4], live[k * 8 + 5]]);
                assert_eq!((v.x, v.y, v.z), (lx, ly, lz), "{file}: obj {i} vert {k}");
            }
        }

        // The blob header's side tables (bone tags at blob_base, attach
        // bones right after) match the assembler's.
        let hdr_base = blob - 0x18;
        let live_tags = rbytes(ram, hdr_base, live_nobj);
        assert_eq!(live_tags, &asm.bone_tags[..], "{file}: live bone tags");
        let extras = asm.attach_bones.len();
        let live_attach = rbytes(ram, hdr_base + live_nobj as u32, extras);
        assert_eq!(live_attach, &asm.attach_bones[..], "{file}: live attach");
        // ... and the channel map they imply equals `anm_bones` (skeleton
        // rides its own bone, 200+ extras ride their attach bone).
        for (i, &tag) in asm.bone_tags.iter().enumerate() {
            if tag < 100 {
                assert_eq!(asm.anm_bones[i], tag, "{file}: obj {i} channel");
            } else if tag >= 200 {
                assert_eq!(
                    asm.anm_bones[i],
                    live_attach[(tag - 200) as usize],
                    "{file}: extra {i} channel"
                );
            }
        }

        // The live anim context's idle stream == the disc decode: the
        // record[0] image base per party slot (cross-checked via its +0x50
        // assembled-blob backpointer), idle entry at table[0], packed
        // stream at entry + 0xAC.
        let rec0_base = ru32(ram, RECORD0_BASES + slot as u32 * 4);
        assert_eq!(
            ru32(ram, rec0_base + 0x50),
            hdr_base,
            "{file}: record[0] work area cross-check"
        );
        let decoded = battle_char_assembly::decode_record0(&raw).expect("decode record[0]");
        // (The loader rewrites the in-RAM action table to absolute pointers,
        // so the idle entry offset comes from the disc-side decode.)
        let entry_off = u32::from_le_bytes(decoded[0..4].try_into().unwrap());
        let stream_off = entry_off as usize + battle_char_assembly::PLAYER_ANIM_STREAM_OFFSET;
        let parts = decoded[stream_off] as usize;
        let frames = decoded[stream_off + 1] as usize;
        let stream_len = 2 + parts * frames * 9;
        let live_stream = rbytes(ram, rec0_base + stream_off as u32, stream_len);
        assert_eq!(
            live_stream,
            &decoded[stream_off..stream_off + stream_len],
            "{file}: live idle stream bytes"
        );
        let idle = battle_char_assembly::idle_battle_animation(&raw)
            .expect("idle decode")
            .expect("idle present");
        assert_eq!(idle.part_count, parts, "{file}: idle parts");
        assert_eq!(idle.frame_count, frames, "{file}: idle frames");
        eprintln!(
            "{file}: live blob ({live_nobj} objects) + side tables + idle stream \
             ({parts} parts x {frames} frames) byte-match the disc decode"
        );
    }
}
