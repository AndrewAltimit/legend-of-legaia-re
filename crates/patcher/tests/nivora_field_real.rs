//! Disc oracle for the nilboa field-mesh mirror of `--delilas-party`:
//! apply [`legaia_patcher::nivora_field::apply_nivora_field`] on a
//! scratch copy and prove:
//!
//! - the PROT 0639 pack re-parses with every untouched member
//!   byte-identical and the three swapped members inside their retail
//!   byte span;
//! - each swapped member is a 10-part rig whose parts, posed by the
//!   scene's own idle ANM record, stand where the sibling's parts stood
//!   (per-part posed-centroid check + whole-rig height band);
//! - the head prims keep the sibling's texture conventions (texpage 5,
//!   row-481 CLUT columns) so the scene's VRAM uploads feed them;
//! - the PROT 0638 bundle rebuild touches ONLY the three sibling head
//!   TIMs' pixel/CLUT bytes in the decoded TIM list (the MAN section's
//!   raw span survives byte-identical);
//! - the mapping is honored, not hardcoded (a rotated permutation
//!   produces different member bytes and reports);
//! - a fixed mapping is byte-deterministic across independent applies;
//! - every touched sector re-validates (EDC/ECC) on re-open.
//!
//! Skips (and passes) when `LEGAIA_DISC_BIN` is unset.

use legaia_asset::monster_archive::PartPose;
use legaia_patcher::delilas_party::PartyMapping;
use legaia_patcher::disc::DiscPatcher;
use legaia_patcher::nivora_field::apply_nivora_field;

const PACK_ENTRY: usize = 639;
const BUNDLE_ENTRY: usize = 638;
/// nilboa coordinates: (sibling monster id, pack member, idle ANM record).
const SIBLINGS: [(u16, usize, usize); 3] = [(162, 106, 55), (163, 107, 68), (164, 108, 78)];

fn load_disc() -> Option<Vec<u8>> {
    let path = std::env::var("LEGAIA_DISC_BIN").ok()?;
    if path.is_empty() {
        return None;
    }
    std::fs::read(path).ok()
}

fn rot(p: &PartPose) -> [[f32; 3]; 3] {
    let rad = |r: u16| (r as f32) * std::f32::consts::TAU / 4096.0;
    let (sx, cx) = rad(p.rx).sin_cos();
    let (sy, cy) = rad(p.ry).sin_cos();
    let (sz, cz) = rad(p.rz).sin_cos();
    [
        [cy * cz, sx * sy * cz - cx * sz, cx * sy * cz + sx * sz],
        [cy * sz, sx * sy * sz + cx * cz, cx * sy * sz - sx * cz],
        [-sy, sx * cy, cx * cy],
    ]
}

/// Per-part posed world centroids + whole-rig posed height of one pack
/// member under one scene ANM record's first frame.
fn posed_stats(
    pack_entry: &[u8],
    bundle_entry: &[u8],
    member: usize,
    record: usize,
) -> (Vec<[f32; 3]>, f32) {
    let pack = &pack_entry[4..];
    let entries = legaia_asset::pack::parse_pack(pack).expect("pack parses");
    let e = &entries[member];
    let bytes = &pack[e.byte_offset..e.byte_offset + e.size];
    let tmd = legaia_tmd::parse(bytes).expect("member TMD parses");
    let model = legaia_tmd::encode::decode_model(&tmd, bytes).expect("member decodes");
    let anm = legaia_asset::player_anm::find_in_entry(bundle_entry, 5)
        .into_iter()
        .next()
        .expect("scene ANM bundle");
    let pose = anm
        .record_to_monster_animation(record)
        .expect("idle record");
    let frame = &pose.frames[0];
    assert_eq!(model.len(), frame.len().min(pose.part_count), "part count");
    let mut centroids = Vec::new();
    let (mut min_y, mut max_y) = (f32::MAX, f32::MIN);
    for (i, o) in model.iter().enumerate() {
        let m = rot(&frame[i]);
        let t = [frame[i].tx as f32, frame[i].ty as f32, frame[i].tz as f32];
        let mut c = [0f32; 3];
        for v in &o.vertices {
            let v = [v[0] as f32, v[1] as f32, v[2] as f32];
            for k in 0..3 {
                let w = m[k][0] * v[0] + m[k][1] * v[1] + m[k][2] * v[2] + t[k];
                c[k] += w;
                if k == 1 {
                    min_y = min_y.min(w);
                    max_y = max_y.max(w);
                }
            }
        }
        let n = o.vertices.len().max(1) as f32;
        centroids.push([c[0] / n, c[1] / n, c[2] / n]);
    }
    (centroids, max_y - min_y)
}

/// The three head-TIM byte ranges inside the decoded §0 TIM list (each
/// from its 0x10 magic to the end of its image block), keyed by the
/// CLUT block position row 481 / columns 128/160/192.
fn head_tim_ranges(sec0: &[u8]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut off = 0usize;
    while off + 8 <= sec0.len() {
        if u32::from_le_bytes(sec0[off..off + 4].try_into().unwrap()) == 0x10
            && let Ok(tim) = legaia_tim::parse(&sec0[off..])
            && let Some(c) = tim.clut.as_ref()
            && c.fb_y == 481
            && [128u16, 160, 192].contains(&c.fb_x)
        {
            // TIM extent: 8-byte header + CLUT block + image block.
            let clut_len = u32::from_le_bytes(sec0[off + 8..off + 12].try_into().unwrap()) as usize;
            let img = off + 8 + clut_len;
            let img_len = u32::from_le_bytes(sec0[img..img + 4].try_into().unwrap()) as usize;
            out.push((off, img + img_len));
        }
        off += 4;
    }
    out
}

fn decoded_sec0(bundle_entry: &[u8]) -> Vec<u8> {
    let container = legaia_asset::parse_player_lzs(bundle_entry, 3).expect("0638 container");
    legaia_asset::decode(
        bundle_entry,
        &container.descriptors[0],
        legaia_asset::DecodeMode::Lzs,
    )
    .expect("0638 TIM list decodes")
}

#[test]
fn default_mapping_heroizes_nilboa_and_survives_reparse() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let patcher = DiscPatcher::open(original.clone()).expect("open disc");
    let base_pack = patcher.read_entry_footprint(PACK_ENTRY).expect("0639");
    let base_bundle = patcher.read_entry_footprint(BUNDLE_ENTRY).expect("0638");
    let base_entries = legaia_asset::pack::parse_pack(&base_pack[4..]).expect("baseline pack");
    let member_count = base_entries.len();
    let base_span: usize = SIBLINGS.iter().map(|&(_, m, _)| base_entries[m].size).sum();
    let mut base_stats = Vec::new();
    for &(_, member, record) in &SIBLINGS {
        base_stats.push(posed_stats(&base_pack, &base_bundle, member, record));
    }

    let mut patcher = DiscPatcher::open(original.clone()).expect("open disc");
    let prot_0874 = patcher.read_entry_footprint(874).expect("0874");
    let mapping = PartyMapping::default(); // vahn=Gi, noa=Lu, gala=Che
    let report = apply_nivora_field(&mut patcher, &mapping, &prot_0874).expect("apply");

    // The report names the mapped hero per sibling: default mapping is
    // Gi<-Vahn(0), Lu<-Noa(1), Che<-Gala(2).
    let hero_of = |id: u16| {
        report
            .slots
            .iter()
            .find(|s| s.monster_id == id)
            .expect("slot report")
            .hero_slot
    };
    assert_eq!(hero_of(162), 0, "Gi wears Vahn");
    assert_eq!(hero_of(164), 1, "Lu wears Noa");
    assert_eq!(hero_of(163), 2, "Che wears Gala");

    let patched = patcher.into_image();
    assert_eq!(patched.len(), original.len(), "image length preserved");
    // Re-open validates EDC/ECC on every touched sector.
    let reopened = DiscPatcher::open(patched).expect("re-open patched");
    let new_pack = reopened.read_entry_footprint(PACK_ENTRY).expect("0639");
    let new_bundle = reopened.read_entry_footprint(BUNDLE_ENTRY).expect("0638");
    assert_eq!(new_pack.len(), base_pack.len(), "0639 footprint preserved");
    assert_eq!(
        new_bundle.len(),
        base_bundle.len(),
        "0638 footprint preserved"
    );

    // Pack re-parses with the same shape; untouched members identical.
    let new_entries = legaia_asset::pack::parse_pack(&new_pack[4..]).expect("patched pack");
    assert_eq!(new_entries.len(), member_count, "member count");
    let swapped: Vec<usize> = SIBLINGS.iter().map(|&(_, m, _)| m).collect();
    for m in 0..member_count {
        if swapped.contains(&m) {
            continue;
        }
        let (a, b) = (&base_entries[m], &new_entries[m]);
        assert_eq!(a.byte_offset, b.byte_offset, "member {m} offset");
        assert_eq!(
            &base_pack[4 + a.byte_offset..4 + a.byte_offset + a.size],
            &new_pack[4 + b.byte_offset..4 + b.byte_offset + b.size],
            "member {m} bytes"
        );
    }
    // The swapped members changed (non-vacuous) and stay in the span.
    let new_span: usize = swapped
        .iter()
        .map(|&m| new_entries[m].byte_offset + new_entries[m].size)
        .max()
        .unwrap()
        - swapped
            .iter()
            .map(|&m| new_entries[m].byte_offset)
            .min()
            .unwrap();
    assert!(new_span <= base_span, "members exceed the retail span");
    for &(_, member, _) in &SIBLINGS {
        let (a, b) = (&base_entries[member], &new_entries[member]);
        assert_ne!(
            &base_pack[4 + a.byte_offset..4 + a.byte_offset + a.size],
            &new_pack[4 + b.byte_offset..4 + b.byte_offset + b.size],
            "member {member} unchanged - the swap did nothing"
        );
    }

    // Each swapped member: 10 parts posed by the scene idle record land
    // where the sibling's parts stood (the bake anchors at the same
    // channel pivots), at a comparable standing height, and the head
    // prims keep the sibling's texture conventions.
    for (k, &(id, member, record)) in SIBLINGS.iter().enumerate() {
        let (centroids, height) = posed_stats(&new_pack, &new_bundle, member, record);
        let (base_centroids, base_height) = &base_stats[k];
        assert_eq!(centroids.len(), 10, "sibling {id} part count");
        let mut worst = 0f32;
        for (i, (c, b)) in centroids.iter().zip(base_centroids).enumerate() {
            let d = ((c[0] - b[0]).powi(2) + (c[1] - b[1]).powi(2) + (c[2] - b[2]).powi(2)).sqrt();
            worst = worst.max(d);
            assert!(
                d < 60.0,
                "sibling {id} part {i} posed centroid {d:.1} units from the sibling's"
            );
        }
        eprintln!("[nivora] sibling {id}: worst part-centroid delta {worst:.1} units");
        let ratio = height / base_height;
        assert!(
            (0.6..=1.5).contains(&ratio),
            "sibling {id} posed height ratio {ratio:.2} out of band"
        );

        // Texture conventions: every textured prim samples texpage 5
        // through a row-481 CLUT in the sibling's own column pair.
        let pack = &new_pack[4..];
        let e = &new_entries[member];
        let bytes = &pack[e.byte_offset..e.byte_offset + e.size];
        let tmd = legaia_tmd::parse(bytes).expect("swapped TMD");
        let model = legaia_tmd::encode::decode_model(&tmd, bytes).expect("swapped model");
        let base_col = match id {
            162 => 8u16,
            163 => 10,
            _ => 12,
        };
        let mut textured = 0usize;
        for o in &model {
            for g in &o.groups {
                if !g.shape.is_textured() {
                    continue;
                }
                for p in &g.prims {
                    textured += 1;
                    assert_eq!(p.tsb & 0x1F, 5, "sibling {id} texpage");
                    assert_eq!(p.cba >> 6, 481, "sibling {id} CLUT row");
                    let col = p.cba & 0x3F;
                    assert!(
                        col == base_col || col == base_col + 1,
                        "sibling {id} CLUT column {col} outside its pair"
                    );
                }
            }
        }
        assert!(textured > 0, "sibling {id} kept no textured face prims");
    }

    // Bundle: the decoded TIM list changed ONLY inside the three head
    // TIMs; the MAN section's raw byte span survives byte-identical.
    let base_sec0 = decoded_sec0(&base_bundle);
    let new_sec0 = decoded_sec0(&new_bundle);
    assert_eq!(base_sec0.len(), new_sec0.len(), "TIM list length");
    let ranges = head_tim_ranges(&base_sec0);
    assert_eq!(ranges.len(), 3, "three sibling head TIMs");
    let mut changed_outside = 0usize;
    let mut changed_inside = 0usize;
    for (i, (a, b)) in base_sec0.iter().zip(&new_sec0).enumerate() {
        if a != b {
            if ranges.iter().any(|&(s, e)| i >= s && i < e) {
                changed_inside += 1;
            } else {
                changed_outside += 1;
            }
        }
    }
    assert_eq!(
        changed_outside, 0,
        "TIM-list bytes changed outside the head TIMs"
    );
    assert!(
        changed_inside > 0,
        "head TIMs unchanged - repaint did nothing"
    );
    let base_container = legaia_asset::parse_player_lzs(&base_bundle, 3).expect("base container");
    let new_container = legaia_asset::parse_player_lzs(&new_bundle, 3).expect("new container");
    let (b1, b2) = (
        base_container.descriptors[1].data_offset as usize,
        base_container.descriptors[2].data_offset as usize,
    );
    let (n1, n2) = (
        new_container.descriptors[1].data_offset as usize,
        new_container.descriptors[2].data_offset as usize,
    );
    assert_eq!(
        &base_bundle[b1..b2],
        &new_bundle[n1..n2],
        "MAN section raw bytes changed"
    );
}

#[test]
fn mapping_permutation_is_honored_and_deterministic() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let member_bytes = |image: &[u8], member: usize| -> Vec<u8> {
        let p = DiscPatcher::open(image.to_vec()).expect("open");
        let pack = p.read_entry_footprint(PACK_ENTRY).expect("0639");
        let entries = legaia_asset::pack::parse_pack(&pack[4..]).expect("pack");
        let e = &entries[member];
        pack[4 + e.byte_offset..4 + e.byte_offset + e.size].to_vec()
    };
    let apply = |mapping: &str| -> (Vec<u8>, usize) {
        let mut p = DiscPatcher::open(original.clone()).expect("open");
        let prot_0874 = p.read_entry_footprint(874).expect("0874");
        let mapping = PartyMapping::parse(mapping).expect("mapping");
        let report = apply_nivora_field(&mut p, &mapping, &prot_0874).expect("apply");
        let gi_hero = report
            .slots
            .iter()
            .find(|s| s.monster_id == 162)
            .expect("Gi slot")
            .hero_slot;
        (p.into_image(), gi_hero)
    };

    let (img_a, gi_a) = apply("gi,lu,che");
    let (img_b, gi_b) = apply("lu,che,gi");
    let (img_a2, _) = apply("gi,lu,che");

    // Determinism: same mapping, byte-identical image.
    assert_eq!(img_a, img_a2, "same mapping must be byte-deterministic");
    // Permutation honored: Gi wears Vahn under the default, Gala under
    // the rotation - different hero, different bytes.
    assert_eq!(gi_a, 0, "default: Gi <- Vahn");
    assert_eq!(gi_b, 2, "rotated: Gi <- Gala");
    assert_ne!(
        member_bytes(&img_a, 106),
        member_bytes(&img_b, 106),
        "member 106 identical under different mappings - the mapping is not honored"
    );
}
