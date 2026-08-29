//! Disc oracle for the event-scene field mirrors of `--delilas-party`
//! (`stone` map-stone confrontation, `taiku2` floating castle, `conc2`
//! past Conkram): apply
//! [`legaia_patcher::nivora_field::apply_event_field`] on a scratch
//! copy and prove, per scene:
//!
//! - the rebuilt bundle keeps its footprint and every section decodes
//!   to its retail decoded size (MAN / MES / MOVE / VDF byte-identical);
//! - the TMD section's untouched members are byte-identical, the
//!   sibling members changed inside their retail span, and taiku2's
//!   four shading copies of a sibling carry one identical bake;
//! - each swapped member is a 10-part rig whose parts, posed by the
//!   scene's own anchor ANM record, stand where the sibling's parts
//!   stood (posed-centroid check + whole-rig height band);
//! - the TIM carrier (bundle `TIM_LIST` section, or conc2's external
//!   `tim_pack` entry 625) changed ONLY inside the sibling head TIMs;
//! - the mapping is honored, not hardcoded, and a fixed mapping is
//!   byte-deterministic.
//!
//! Skips (and passes) when `LEGAIA_DISC_BIN` is unset.

use legaia_asset::monster_archive::PartPose;
use legaia_asset::party_swap::event_field::{EVENT_SCENES, EventSceneSpec};
use legaia_patcher::delilas_party::PartyMapping;
use legaia_patcher::disc::DiscPatcher;
use legaia_patcher::nivora_field::apply_event_field;

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

/// Decode one bundle section by asset-type byte.
fn section(bundle: &[u8], type_byte: u8) -> Vec<u8> {
    let c = legaia_asset::parse_player_lzs(bundle, 7).expect("bundle container");
    let d = c
        .descriptors
        .iter()
        .find(|d| d.type_byte == type_byte)
        .expect("section present");
    legaia_asset::decode(bundle, d, legaia_asset::DecodeMode::Lzs).expect("section decodes")
}

/// Per-part posed world centroids + posed height of one TMD-section
/// member under one scene ANM record's first frame.
fn posed_stats(
    tmd_sec: &[u8],
    bundle: &[u8],
    member: usize,
    record: usize,
) -> (Vec<[f32; 3]>, f32) {
    let entries = legaia_asset::pack::parse_pack(tmd_sec).expect("pack parses");
    let e = &entries[member];
    let bytes = &tmd_sec[e.byte_offset..e.byte_offset + e.size];
    let tmd = legaia_tmd::parse(bytes).expect("member TMD parses");
    let model = legaia_tmd::encode::decode_model(&tmd, bytes).expect("member decodes");
    let anm = legaia_asset::player_anm::find_in_entry(bundle, 7)
        .into_iter()
        .next()
        .expect("scene ANM bundle");
    let pose = anm.record_to_monster_animation(record).expect("anchor rec");
    let frame = &pose.frames[0];
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

/// Byte ranges of the sibling head TIMs inside a raw-TIM buffer: the
/// CLUT'd TIMs whose CLUT block sits at one of the members' first CLUT
/// coordinates (derived from the members' own prims).
fn head_tim_ranges(tims: &[u8], cluts: &[(u16, u16)]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut off = 0usize;
    while off + 8 <= tims.len() {
        if u32::from_le_bytes(tims[off..off + 4].try_into().unwrap()) == 0x10
            && let Ok(tim) = legaia_tim::parse(&tims[off..])
            && let Some(c) = tim.clut.as_ref()
            && cluts.contains(&(c.fb_x, c.fb_y))
        {
            let clut_len = u32::from_le_bytes(tims[off + 8..off + 12].try_into().unwrap()) as usize;
            let img = off + 8 + clut_len;
            let img_len = u32::from_le_bytes(tims[img..img + 4].try_into().unwrap()) as usize;
            out.push((off, img + img_len));
        }
        off += 4;
    }
    out
}

/// The members' first-CLUT VRAM coordinates, read off their own prims.
fn member_clut_coords(tmd_sec: &[u8], members: &[usize]) -> Vec<(u16, u16)> {
    let entries = legaia_asset::pack::parse_pack(tmd_sec).expect("pack");
    let mut out = Vec::new();
    for &m in members {
        let e = &entries[m];
        let bytes = &tmd_sec[e.byte_offset..e.byte_offset + e.size];
        let tmd = legaia_tmd::parse(bytes).expect("TMD");
        let model = legaia_tmd::encode::decode_model(&tmd, bytes).expect("model");
        let mut cbas: Vec<u16> = Vec::new();
        for o in &model {
            for g in &o.groups {
                if !g.shape.is_textured() {
                    continue;
                }
                for p in &g.prims {
                    if !cbas.contains(&p.cba) {
                        cbas.push(p.cba);
                    }
                }
            }
        }
        cbas.sort_unstable();
        let cba = cbas[0];
        let coord = ((cba & 0x3F) * 16, cba >> 6);
        if !out.contains(&coord) {
            out.push(coord);
        }
    }
    out
}

fn verify_scene(spec: &EventSceneSpec, base_img: &[u8], patched_img: &[u8]) {
    let base = DiscPatcher::open(base_img.to_vec()).expect("open base");
    let new = DiscPatcher::open(patched_img.to_vec()).expect("open patched");
    let base_bundle = base
        .read_entry_footprint(spec.bundle_entry)
        .expect("bundle");
    let new_bundle = new.read_entry_footprint(spec.bundle_entry).expect("bundle");
    assert_eq!(
        base_bundle.len(),
        new_bundle.len(),
        "{}: bundle footprint",
        spec.scene
    );

    // Every non-TMD, non-TIM section decodes byte-identical.
    for tb in [0x03u8, 0x04, 0x05, 0x07] {
        assert_eq!(
            section(&base_bundle, tb),
            section(&new_bundle, tb),
            "{}: section type {tb:#x} changed",
            spec.scene
        );
    }

    let base_tmd = section(&base_bundle, 0x02);
    let new_tmd = section(&new_bundle, 0x02);
    assert_eq!(base_tmd.len(), new_tmd.len(), "{}: TMD size", spec.scene);
    let base_entries = legaia_asset::pack::parse_pack(&base_tmd).expect("base pack");
    let new_entries = legaia_asset::pack::parse_pack(&new_tmd).expect("new pack");
    assert_eq!(
        base_entries.len(),
        new_entries.len(),
        "{}: members",
        spec.scene
    );
    let replaced: Vec<usize> = spec
        .slots
        .iter()
        .flat_map(|s| s.members.iter().copied())
        .collect();
    for m in 0..base_entries.len() {
        if replaced.contains(&m) {
            continue;
        }
        let (a, b) = (&base_entries[m], &new_entries[m]);
        assert_eq!(
            a.byte_offset, b.byte_offset,
            "{}: member {m} offset",
            spec.scene
        );
        assert_eq!(
            &base_tmd[a.byte_offset..a.byte_offset + a.size],
            &new_tmd[b.byte_offset..b.byte_offset + b.size],
            "{}: member {m} bytes",
            spec.scene
        );
    }
    // Replaced members: changed, in-span, copies identical per sibling.
    let base_span: usize = replaced.iter().map(|&m| base_entries[m].size).sum();
    let new_span: usize = replaced
        .iter()
        .map(|&m| new_entries[m].byte_offset + new_entries[m].size)
        .max()
        .unwrap()
        - replaced
            .iter()
            .map(|&m| new_entries[m].byte_offset)
            .min()
            .unwrap();
    assert!(new_span <= base_span, "{}: members exceed span", spec.scene);
    for slot in &spec.slots {
        let bytes_of = |m: usize| {
            let e = &new_entries[m];
            new_tmd[e.byte_offset..e.byte_offset + e.size].to_vec()
        };
        let first = bytes_of(slot.members[0]);
        let e = &base_entries[slot.members[0]];
        assert_ne!(
            first,
            base_tmd[e.byte_offset..e.byte_offset + e.size].to_vec(),
            "{}: sibling {} member unchanged",
            spec.scene,
            slot.monster_id
        );
        // Copies carry one identical bake. The pack parser attributes
        // the region's trailing zero residue to the LAST member's size,
        // so compare the shared prefix and require a zero tail.
        for &m in &slot.members[1..] {
            let b = bytes_of(m);
            let n = first.len().min(b.len());
            assert_eq!(
                first[..n],
                b[..n],
                "{}: sibling {} copies diverge",
                spec.scene,
                slot.monster_id
            );
            assert!(
                first[n..].iter().all(|&x| x == 0) && b[n..].iter().all(|&x| x == 0),
                "{}: sibling {} copy tail not padding",
                spec.scene,
                slot.monster_id
            );
        }

        // Posed by the scene's own anchor record, the hero parts stand
        // where the sibling's parts stood.
        let (base_c, base_h) =
            posed_stats(&base_tmd, &base_bundle, slot.members[0], slot.anchor_record);
        let (new_c, new_h) =
            posed_stats(&new_tmd, &new_bundle, slot.members[0], slot.anchor_record);
        assert_eq!(new_c.len(), 10, "{}: part count", spec.scene);
        for (i, (c, b)) in new_c.iter().zip(&base_c).enumerate() {
            let d = ((c[0] - b[0]).powi(2) + (c[1] - b[1]).powi(2) + (c[2] - b[2]).powi(2)).sqrt();
            assert!(
                d < 60.0,
                "{}: sibling {} part {i} posed centroid {d:.1} units off",
                spec.scene,
                slot.monster_id
            );
        }
        let ratio = new_h / base_h;
        assert!(
            (0.6..=1.5).contains(&ratio),
            "{}: sibling {} posed height ratio {ratio:.2}",
            spec.scene,
            slot.monster_id
        );
    }

    // TIM carrier: only the sibling head TIMs changed.
    let members_first: Vec<usize> = spec.slots.iter().map(|s| s.members[0]).collect();
    let cluts = member_clut_coords(&base_tmd, &members_first);
    let (base_tims, new_tims) = match spec.tim_entry {
        Some(e) => (
            base.read_entry_footprint(e).expect("tim entry"),
            new.read_entry_footprint(e).expect("tim entry"),
        ),
        None => (section(&base_bundle, 0x01), section(&new_bundle, 0x01)),
    };
    assert_eq!(
        base_tims.len(),
        new_tims.len(),
        "{}: TIM carrier size",
        spec.scene
    );
    let ranges = head_tim_ranges(&base_tims, &cluts);
    assert_eq!(ranges.len(), 3, "{}: three sibling head TIMs", spec.scene);
    let mut inside = 0usize;
    for (i, (a, b)) in base_tims.iter().zip(&new_tims).enumerate() {
        if a != b {
            assert!(
                ranges.iter().any(|&(s, e)| i >= s && i < e),
                "{}: TIM byte {i:#x} changed outside the head TIMs",
                spec.scene
            );
            inside += 1;
        }
    }
    assert!(inside > 0, "{}: head TIMs unchanged", spec.scene);
}

#[test]
fn default_mapping_heroizes_every_event_scene() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut patcher = DiscPatcher::open(original.clone()).expect("open disc");
    let prot_0874 = patcher.read_entry_footprint(874).expect("0874");
    let mapping = PartyMapping::default();
    let report = apply_event_field(&mut patcher, &mapping, &prot_0874).expect("apply");
    // One slot report per sibling per scene; default mapping honored.
    assert_eq!(report.slots.len(), 3 * EVENT_SCENES.len(), "slot reports");
    for s in &report.slots {
        let hero = match s.monster_id {
            162 => 0, // Gi <- Vahn
            164 => 1, // Lu <- Noa
            _ => 2,   // Che <- Gala
        };
        assert_eq!(s.hero_slot, hero, "sibling {} hero", s.monster_id);
    }
    let patched = patcher.into_image();
    assert_eq!(patched.len(), original.len(), "image length");
    // Re-open validates EDC/ECC on every touched sector.
    DiscPatcher::open(patched.clone()).expect("re-open patched");
    for spec in EVENT_SCENES {
        verify_scene(spec, &original, &patched);
        eprintln!("[event-field] {} verified", spec.scene);
    }
}

#[test]
fn event_mapping_permutation_is_honored_and_deterministic() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let apply = |mapping: &str| -> Vec<u8> {
        let mut p = DiscPatcher::open(original.clone()).expect("open");
        let prot_0874 = p.read_entry_footprint(874).expect("0874");
        let mapping = PartyMapping::parse(mapping).expect("mapping");
        apply_event_field(&mut p, &mapping, &prot_0874).expect("apply");
        p.into_image()
    };
    let member_bytes = |image: &[u8]| -> Vec<u8> {
        let p = DiscPatcher::open(image.to_vec()).expect("open");
        let bundle = p.read_entry_footprint(175).expect("stone bundle");
        let sec = section(&bundle, 0x02);
        let entries = legaia_asset::pack::parse_pack(&sec).expect("pack");
        let e = &entries[33];
        sec[e.byte_offset..e.byte_offset + e.size].to_vec()
    };
    let img_a = apply("gi,lu,che");
    let img_b = apply("lu,che,gi");
    let img_a2 = apply("gi,lu,che");
    assert_eq!(img_a, img_a2, "same mapping must be byte-deterministic");
    assert_ne!(
        member_bytes(&img_a),
        member_bytes(&img_b),
        "stone Gi member identical under different mappings"
    );
}
