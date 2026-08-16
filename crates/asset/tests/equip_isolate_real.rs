//! Disc-gated sweep of [`battle_char_assembly::equip_isolate`] - the
//! **item-alone** cut - over every equipment record on the disc (132), plus
//! the integrity of the committed rule table against those records.
//!
//! The cut is a policy, not a disc fact, so what is asserted is the shape of
//! the result rather than its exact primitive set: every record keeps
//! something and drops something it should (a held item never keeps the
//! whole hand object; a weapon keeps at least its palette-cut item), the cut
//! is a subset of the section's contribution, and every committed override
//! names a record, an object and primitives that exist - a rule for a bone
//! tag or ordinal the record does not have is an authoring error that would
//! otherwise silently do nothing.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use legaia_asset::battle_char_assembly::equip_isolate::{self, IsolationMode};
use legaia_asset::battle_char_assembly::equip_item;
use legaia_asset::{battle_char_assembly as bca, battle_data_pack};

fn extracted_prot_dir() -> Option<PathBuf> {
    [
        PathBuf::from("extracted/PROT"),
        PathBuf::from("../../extracted/PROT"),
    ]
    .into_iter()
    .find(|p| p.is_dir())
}

fn section_ids(pack: &battle_data_pack::BattleDataPack) -> Vec<Vec<u32>> {
    let mut out: Vec<Vec<u32>> = vec![Vec::new(); bca::SECTION_COUNT];
    let mut slot = 0usize;
    for r in &pack.records {
        if slot >= bca::SECTION_COUNT {
            break;
        }
        if r.id == 0 {
            slot += 1;
        } else {
            out[slot].push(r.id);
        }
    }
    out
}

fn load(dir: &Path, file: &str) -> Option<(Vec<u8>, battle_data_pack::BattleDataPack)> {
    let path = dir.join(file);
    if !path.exists() {
        eprintln!("[skip] {} missing", path.display());
        return None;
    }
    let raw = std::fs::read(&path).ok()?;
    let pack = battle_data_pack::parse(&raw).ok()?;
    Some((raw, pack))
}

fn vram_for(
    raw: &[u8],
    pack: &battle_data_pack::BattleDataPack,
    load: &[u8; bca::SECTION_COUNT],
) -> legaia_tim::Vram {
    let mut vram = legaia_tim::Vram::new();
    for u in &bca::character_texture_uploads(raw, pack, load, 0).expect("texture pool") {
        vram.write_block(u.fb_x(), u.fb_y(), u.rect.w, u.rect.h, &u.pixels);
        if !u.clut.is_empty() {
            vram.write_clut_row(u.clut_x, u.clut_row(), &u.clut_bytes());
        }
    }
    vram
}

const FILES: [(&str, &str, usize); 3] = [
    ("0863_edstati3.BIN", "vahn", 0),
    ("0864_edstati3.BIN", "noa", 1),
    ("0865_battle_data.BIN", "gala", 2),
];

/// Every record on the disc yields a non-empty item-alone cut that is a
/// strict subset of the section's contribution wherever a limb exists to
/// leave behind, and the held-item cut never keeps less than the exact
/// palette item.
#[test]
fn every_record_isolates_to_something_and_leaves_something() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let Some(dir) = extracted_prot_dir() else {
        eprintln!("[skip] extracted/PROT missing");
        return;
    };
    let rules = equip_isolate::rules();
    let mut total = 0usize;
    let mut curated = 0usize;
    let mut by_mode: BTreeMap<&str, usize> = BTreeMap::new();
    for (file, who, cslot) in FILES {
        let Some((raw, pack)) = load(&dir, file) else {
            return;
        };
        let ids = section_ids(&pack);
        let mut bare = bca::assemble_character(&raw, &pack, &[0; bca::SECTION_COUNT])
            .unwrap_or_else(|e| panic!("{who}: bare assembly: {e:#}"));
        bca::relocate_tsb_cba(&mut bare.tmd, 0).expect("bare relocate");
        let bare_tmd = legaia_tmd::parse(&bare.tmd).expect("bare TMD");
        let bare_vram = vram_for(&raw, &pack, &[0; bca::SECTION_COUNT]);
        let own = equip_isolate::skin_colours(&bare, &bare_tmd, &bare_vram);
        for section in 0..bca::SECTION_COUNT {
            for &id in &ids[section] {
                total += 1;
                let mut load = [0u8; bca::SECTION_COUNT];
                load[section] = id as u8;
                let mut eq = bca::assemble_character(&raw, &pack, &load)
                    .unwrap_or_else(|e| panic!("{who} {id:#x}: assembly: {e:#}"));
                bca::relocate_tsb_cba(&mut eq.tmd, 0).expect("relocate");
                let eq_tmd = legaia_tmd::parse(&eq.tmd).expect("equipped TMD");
                let vram = vram_for(&raw, &pack, &load);
                let partition = equip_item::item_partition(section, &bare, &bare_tmd, &eq, &eq_tmd)
                    .unwrap_or_else(|| panic!("{who} {id:#x}: no partition"));
                let rule = rules.rule_for(cslot, id);
                let iso = equip_isolate::isolate_item(
                    &equip_isolate::IsolationInputs {
                        section,
                        bare: &bare,
                        bare_tmd: &bare_tmd,
                        bare_vram: &bare_vram,
                        equipped: &eq,
                        equipped_tmd: &eq_tmd,
                        vram: &vram,
                        partition: &partition,
                    },
                    rule,
                );
                *by_mode.entry(iso.mode.tag()).or_default() += 1;
                if iso.curated {
                    curated += 1;
                }
                assert_eq!(iso.curated, rule.is_some(), "{who} {id:#x}: curated flag");
                assert!(
                    iso.kept_primitives > 0,
                    "{who} s{section} {id:#x}: the item-alone cut kept nothing"
                );
                assert_eq!(
                    iso.kept_primitives,
                    iso.keep.len(),
                    "{who} {id:#x}: kept count vs set"
                );
                // Every kept primitive belongs to one of the section's objects.
                for &(obj, _) in &iso.keep {
                    assert!(
                        iso.objects.contains(&obj),
                        "{who} {id:#x}: kept prim on object {obj} outside the section"
                    );
                }
                // A held item (sections 2 / 3) always has a hand to leave
                // behind: something is dropped, and everything the exact
                // palette cut calls item is kept unless a rule says
                // otherwise (the palette cut is the floor of the reading).
                if equip_item::ITEM_SECTIONS.contains(&section) {
                    assert!(
                        iso.dropped_primitives > 0,
                        "{who} s{section} {id:#x}: a held item kept the whole hand"
                    );
                    if rule.is_none() && iso.mode == IsolationMode::ColourDiff {
                        for &obj in &iso.objects {
                            for p in equip_isolate::object_prim_refs(&eq_tmd, &eq.tmd, obj) {
                                if partition.claims(obj, p.cba) && !iso.claims(obj, p.ordinal) {
                                    // Allowed only when the palette item prim
                                    // is itself body-coloured somewhere - the
                                    // wrist band Vahn's Great Axe claims as
                                    // item, the skin a Ra-Seru paints between
                                    // its plates in its own palette column.
                                    let tex = equip_isolate::prim_texels(&p, &vram);
                                    let bare_obj = eq
                                        .bone_tags
                                        .get(obj)
                                        .and_then(|t| bare.bone_tags.iter().position(|b| b == t));
                                    let bare_set: std::collections::HashSet<u16> = bare_obj
                                        .map(|bi| {
                                            equip_isolate::object_prim_refs(
                                                &bare_tmd, &bare.tmd, bi,
                                            )
                                            .iter()
                                            .flat_map(|bp| {
                                                equip_isolate::prim_texels(bp, &bare_vram)
                                            })
                                            .collect()
                                        })
                                        .unwrap_or_default();
                                    let body_like = tex.iter().any(|w| {
                                        equip_isolate::near(&bare_set, *w)
                                            || equip_isolate::skin_like(*w)
                                            || equip_isolate::near_within(&own, *w, 2)
                                    });
                                    assert!(
                                        body_like,
                                        "{who} s{section} {id:#x}: dropped palette-item prim {}:{} that is not body-coloured",
                                        eq.bone_tags[obj], p.ordinal
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    assert_eq!(total, 132, "equipment records on the disc");
    assert!(curated > 0, "the committed rule table touched no record");
    // The two section defaults both occur; whole / palette only by rule.
    assert!(by_mode.contains_key("colour-diff") && by_mode.contains_key("identity"));
}

/// Every committed rule names a record on the disc, and every object / prim
/// it addresses exists in that record's assembly - so no override is inert.
#[test]
fn every_committed_rule_addresses_a_real_record() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let Some(dir) = extracted_prot_dir() else {
        eprintln!("[skip] extracted/PROT missing");
        return;
    };
    let rules = equip_isolate::rules();
    let mut seen: BTreeSet<(String, u32)> = BTreeSet::new();
    for r in &rules.record {
        assert!(
            seen.insert((r.character.to_ascii_lowercase(), r.id)),
            "duplicate rule for {} {:#x}",
            r.character,
            r.id
        );
    }
    let mut matched = 0usize;
    for (file, who, cslot) in FILES {
        let Some((raw, pack)) = load(&dir, file) else {
            return;
        };
        let ids = section_ids(&pack);
        for section in 0..bca::SECTION_COUNT {
            for &id in &ids[section] {
                let Some(rule) = rules.rule_for(cslot, id) else {
                    continue;
                };
                matched += 1;
                let mut load = [0u8; bca::SECTION_COUNT];
                load[section] = id as u8;
                let eq = bca::assemble_character(&raw, &pack, &load)
                    .unwrap_or_else(|e| panic!("{who} {id:#x}: assembly: {e:#}"));
                let eq_tmd = legaia_tmd::parse(&eq.tmd).expect("equipped TMD");
                let section_tags: BTreeSet<u8> = eq
                    .section_of
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| usize::from(**s) == section)
                    .map(|(ei, _)| eq.bone_tags[ei])
                    .collect();
                for tag in rule.keep_objects.iter().chain(rule.drop_objects.iter()) {
                    assert!(
                        section_tags.contains(tag),
                        "{who} {id:#x}: rule names bone tag {tag}, section has {section_tags:?}"
                    );
                }
                let mut columns: BTreeSet<u16> = BTreeSet::new();
                let mut ordinals: BTreeMap<u8, BTreeSet<u32>> = BTreeMap::new();
                for (ei, &s) in eq.section_of.iter().enumerate() {
                    if usize::from(s) != section {
                        continue;
                    }
                    for p in equip_isolate::object_prim_refs(&eq_tmd, &eq.tmd, ei) {
                        columns.insert(p.column);
                        ordinals
                            .entry(eq.bone_tags[ei])
                            .or_default()
                            .insert(p.ordinal);
                    }
                }
                for c in rule.keep_columns.iter().chain(rule.drop_columns.iter()) {
                    assert!(
                        columns.contains(c),
                        "{who} {id:#x}: rule names palette column {c}, section draws {columns:?}"
                    );
                }
                for s in rule.keep.iter().chain(rule.drop.iter()) {
                    let (t, o) = s.split_once(':').expect("tag:ordinal");
                    let t: u8 = t.trim().parse().unwrap();
                    let o: u32 = o.trim().parse().unwrap();
                    assert!(
                        ordinals.get(&t).is_some_and(|set| set.contains(&o)),
                        "{who} {id:#x}: rule names prim {t}:{o}, which the record does not have"
                    );
                }
            }
        }
    }
    assert_eq!(
        matched,
        rules.record.len(),
        "every rule must match a (character, id) on the disc"
    );
}
