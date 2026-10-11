//! Building a space report from a pack and a disc: the dry-run import, then the
//! code-image, spare-space and per-category rows.

use super::super::code_strings::CodeStrings;
use super::super::export::SceneManText;
use super::super::import::{ImportReport, Key, WritePath, parse_key};
use super::super::monster_names;
use super::super::name_pool::{Layout, NamePool};
use super::super::pack::{Entry, LanguagePack};
use super::super::stream_man::{MAX_GROWN_FOOTPRINT, StreamManText};
use super::super::ui;
use super::report::{
    CarrierSpace, CategorySpace, CodeImageSpace, EntrySpace, MonsterSpace, NAME_SECTIONS,
    NameRegion, NameSpace, Outcome, PoolSpace, RoomKind, SPACE_SCHEMA, SceneSpace, SpaceReport,
    SpaceSummary, SpareSpace,
};
use super::{Outcomes, all_pools, english_len, pool_index, scene_name, section_of_pool};
use anyhow::{Context, Result};
use legaia_disc_patch::disc::DiscPatcher;
use legaia_disc_patch::space_ledger;
use legaia_scene_formats::man_edit::{self, TextSite};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

pub(super) fn build_report(
    patcher: &DiscPatcher,
    english: &LanguagePack,
    pack: Option<&LanguagePack>,
    dry: Option<&ImportReport>,
    relayout: bool,
) -> Result<SpaceReport> {
    let cdname = patcher.cdname();
    // Keys staged for (and landed by) a relayout: the relayout scenes' and
    // carriers' applied keys already sit in `applied_keys`.
    let outcomes = dry.map(|r| Outcomes::new(r, HashSet::new()));
    let filled: BTreeMap<&str, &Entry> = pack
        .map(|p| {
            p.sections
                .iter()
                .flat_map(|(_, es)| es)
                .filter(|e| e.is_filled())
                .map(|e| (e.key.as_str(), e))
                .collect()
        })
        .unwrap_or_default();

    // --- SCUS names -------------------------------------------------------
    let scus = patcher
        .read_named_file("SCUS_942.54")
        .context("SCUS_942.54 not found in disc image")?;
    let pool = NamePool::build(&scus);
    let english_layout = pool.layout(&scus, &BTreeMap::new());
    let pack_layout: Option<&Layout> = dry.and_then(|r| r.trace.name_layout.as_ref());
    let name_regions: Vec<NameRegion> = english_layout
        .regions
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let after = pack_layout.and_then(|l| l.regions.get(i));
            NameRegion {
                index: i,
                start_va: r.start_va,
                end_va: r.end_va,
                total: r.total(),
                strings: r.vas.len(),
                english_used: r.used,
                english_free: r.free,
                pack_used: after.map(|a| a.used).or(dry.map(|_| r.used)),
                pack_free: after.map(|a| a.free).or(dry.map(|_| r.free)),
            }
        })
        .collect();
    let mut names = Vec::new();
    let mut name_group: HashMap<String, Option<usize>> = HashMap::new();
    for (section, entries) in english.sections.iter() {
        if !NAME_SECTIONS.contains(&section) {
            continue;
        }
        for e in entries {
            let Some(Key::ScusStr { va }) = parse_key(&e.key) else {
                continue;
            };
            let pin = pool.pin_reason(va);
            let region = english_layout.region_of(va);
            name_group.insert(e.key.clone(), region);
            names.push(NameSpace {
                key: e.key.clone(),
                va,
                movable: pin.is_none(),
                pin,
                region,
                span: pool.span(va).map(|(_, len)| len),
                moved_to: dry.and_then(|r| r.trace.moved.get(&e.key).map(|m| m.1)),
            });
        }
    }

    // --- Code images (ui_menu / system_text) ----------------------------------
    // The executable and each overlay a `ui_menu` pool lives in, measured
    // the way import measures them before writing.
    let mut code: CodeImages = BTreeMap::new();
    code.insert(
        usize::MAX,
        (
            scus.clone(),
            ui::SCUS_POOL_BASE_VA,
            super::super::import::system_strings(patcher, &scus),
        ),
    );
    let ui_prots: BTreeSet<usize> = ui::UI_STRING_POOLS.iter().map(|p| p.prot_index).collect();
    for prot in ui_prots {
        let (Ok(buf), Some(base)) = (patcher.read_entry(prot), ui::overlay_base_va(prot)) else {
            continue;
        };
        let cs = super::super::import::overlay_strings(patcher, prot, &buf, base);
        code.insert(prot, (buf, base, cs));
    }
    let movable_code: HashSet<(usize, u32)> = code
        .iter()
        .flat_map(|(&id, (_, _, cs))| {
            cs.vas()
                .filter(|&va| cs.is_movable(va))
                .map(move |va| (id, va))
        })
        .collect();

    // --- Monsters -----------------------------------------------------------
    let mut monsters = Vec::new();
    if let Ok(archive) = patcher.read_entry(legaia_disc_patch::disc::MONSTER_ARCHIVE_ENTRY) {
        let fields = monster_names::fields(&archive);
        for ((id, f), (_, room)) in fields.iter().zip(monster_names::budgets(&fields)) {
            let Ok(Some(block)) =
                legaia_actor_formats::monster_archive::decode_block(&archive, *id)
            else {
                continue;
            };
            let kept = block
                .get(8..12)
                .map_or(0, |w| u32::from_le_bytes(w.try_into().unwrap()) as usize);
            // The longest name `rewrite_slot` accepts: in the record's own
            // room, or by `grow_block` (monotone in the length, so the first
            // length that works counting down is the cap).
            let longest = monster_names::RETAIL_LONGEST_NAME;
            let cap = (1..=longest)
                .rev()
                .find(|&l| l <= f.room || monster_names::grow_block(&block, f, l).is_ok())
                .unwrap_or(0);
            monsters.push(MonsterSpace {
                key: format!("mon:{id}"),
                id: *id,
                room,
                cap,
                longest,
                block_len: block.len(),
                kept_len: kept,
                max_block: monster_names::RETAIL_MAX_BLOCK,
                max_kept: monster_names::RETAIL_MAX_KEPT,
            });
        }
    }

    // --- Flat index (built alongside the pools / scenes / carriers) ---------
    let mut entries: Vec<EntrySpace> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut scene_lines: BTreeMap<usize, usize> = BTreeMap::new();
    let mut carrier_lines: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    let mut decoded_cache: HashMap<usize, Option<SceneManText>> = HashMap::new();
    let mut stream_cache: HashMap<usize, Option<StreamManText>> = HashMap::new();
    let mut push = |section: &str, key: &str, room_default: usize, eng: Option<usize>| {
        let parsed = parse_key(key);
        let t = dry.map(|r| &r.trace);
        let room = t
            .and_then(|t| t.rooms.get(key).copied())
            .unwrap_or(room_default);
        let (room_kind, group) = match &parsed {
            Some(Key::ScusStr { va }) if NAME_SECTIONS.contains(&section) => {
                match name_group.get(key).copied().flatten() {
                    Some(r) if pool.is_movable(*va) => {
                        (RoomKind::NameMovable, Some(format!("region:{r}")))
                    }
                    _ => (RoomKind::StringFixed, None),
                }
            }
            Some(k @ (Key::ScusStr { .. } | Key::Ui { .. })) => {
                let id = match *k {
                    Key::Ui { prot, va } => (prot, va),
                    Key::ScusStr { va } => (usize::MAX, va),
                    _ => unreachable!(),
                };
                (
                    if movable_code.contains(&id) {
                        RoomKind::StringMovable
                    } else {
                        RoomKind::StringFixed
                    },
                    pool_index(k).map(|i| format!("pool:{i}")),
                )
            }
            Some(Key::ScusParty { .. }) => (RoomKind::Field, Some("party".to_string())),
            Some(Key::ScusCell { .. }) => (RoomKind::Field, Some("place".to_string())),
            Some(Key::Mon { .. }) => (RoomKind::Monster, Some("monster".to_string())),
            Some(Key::Man { entry, off }) => {
                *scene_lines.entry(*entry).or_default() += 1;
                let man = decoded_cache.entry(*entry).or_insert_with(|| {
                    patcher
                        .read_entry(*entry)
                        .ok()
                        .and_then(|b| SceneManText::locate(&b))
                });
                let walked = man
                    .as_ref()
                    .is_some_and(|m| man_edit::text_site(&m.decoded, *off) == TextSite::Segment);
                (
                    if walked {
                        RoomKind::DialogGrowable
                    } else {
                        RoomKind::DialogFixed
                    },
                    Some(format!("scene:{entry}")),
                )
            }
            Some(Key::Raw { entry, off }) => {
                carrier_lines.entry(*entry).or_default().push(*off);
                let sm = stream_cache.entry(*entry).or_insert_with(|| {
                    patcher
                        .read_entry(*entry)
                        .ok()
                        .and_then(|b| StreamManText::locate(&b))
                });
                let walked = sm.as_ref().is_some_and(|sm| {
                    off.checked_sub(sm.man_range().start)
                        .is_some_and(|o| man_edit::text_site(&sm.man, o) == TextSite::Segment)
                });
                (
                    if walked {
                        RoomKind::DialogGrowable
                    } else {
                        RoomKind::DialogFixed
                    },
                    Some(format!("carrier:{entry}")),
                )
            }
            None => (RoomKind::StringFixed, None),
        };
        let fe = filled.get(key).copied();
        entries.push(EntrySpace {
            key: key.to_string(),
            section: section.to_string(),
            room_kind,
            room,
            group,
            english_len: eng,
            pack_len: t.and_then(|t| t.encoded.get(key).copied()),
            outcome: outcomes.as_ref().map(|o| o.of(key, fe.is_some())),
            issue_kind: t.and_then(|t| t.issue_kinds.get(key).copied()),
            issue: outcomes.as_ref().and_then(|o| o.issue(key)),
        });
    };
    for (section, es) in english.sections.iter() {
        for e in es {
            if seen.insert(e.key.clone()) {
                push(section, &e.key, e.budget, english_len(e));
            }
        }
    }
    // Pack keys the disc's export does not carry (a stale or foreign key).
    if let Some(p) = pack {
        for (section, es) in p.sections.iter() {
            for e in es {
                if seen.insert(e.key.clone()) {
                    push(section, &e.key, e.budget, None);
                }
            }
        }
    }

    // --- Fixed-room pools ----------------------------------------------------
    let mut pools: Vec<PoolSpace> = all_pools()
        .enumerate()
        .map(|(i, p)| PoolSpace {
            index: i,
            section: section_of_pool(p).to_string(),
            label: p.label.to_string(),
            prot: (p.prot_index != usize::MAX).then_some(p.prot_index),
            va_start: p.va_start,
            va_end: p.va_end,
            total: (p.va_end - p.va_start) as usize,
            strict: p.strict,
            strings: 0,
            english_bytes: 0,
            room_bytes: 0,
            slack: 0,
            pack_bytes: dry.map(|_| 0),
        })
        .collect();
    for e in &entries {
        let Some(i) = e
            .group
            .as_deref()
            .and_then(|g| g.strip_prefix("pool:"))
            .and_then(|i| i.parse::<usize>().ok())
        else {
            continue;
        };
        let p = &mut pools[i];
        let eng = e.english_len.unwrap_or(e.room);
        p.strings += 1;
        p.english_bytes += eng + 1;
        p.room_bytes += e.room + 1;
        if let Some(pb) = p.pack_bytes.as_mut() {
            let lands = e
                .outcome
                .is_some_and(|o| o.lands() && o != Outcome::AlreadyApplied);
            *pb += if lands {
                e.pack_len.unwrap_or(eng)
            } else {
                eng
            } + 1;
        }
    }
    for p in &mut pools {
        p.slack = p.room_bytes.saturating_sub(p.english_bytes);
    }

    // --- Scenes ----------------------------------------------------------------
    let filled_in = |prefix: &str, prot: usize| -> usize {
        filled
            .keys()
            .filter(|k| {
                k.strip_prefix(prefix)
                    .and_then(|r| r.split(':').next())
                    .and_then(|n| n.parse::<usize>().ok())
                    == Some(prot)
            })
            .count()
    };
    let mut scenes = Vec::new();
    for (&prot, &lines) in &scene_lines {
        let Some(Some(man)) = decoded_cache.get(&prot) else {
            continue;
        };
        let t = dry.and_then(|r| r.trace.scenes.get(&prot));
        scenes.push(SceneSpace {
            prot,
            scene: scene_name(&cdname, prot),
            lines,
            footprint: man.compressed_budget,
            disc_len: man.compressed_len,
            slack: man.compressed_budget.saturating_sub(man.compressed_len),
            filled: dry.map(|_| filled_in("man:", prot)),
            path: dry.map(|_| t.map_or(WritePath::None, |t| t.path)),
            written_len: t.and_then(|t| t.written_len),
            full_overflow: t.and_then(|t| t.full_overflow),
            padded_overflow: t.and_then(|t| t.padded_overflow),
            refused: t.is_some_and(|t| t.refused),
            rolled_back: t.map(|t| t.rolled_back.clone()).unwrap_or_default(),
            relayout_sectors: t.and_then(|t| t.relayout_sectors),
            relayout_would_add: t.and_then(|t| t.relayout_would_add),
        });
    }

    // --- Raw carriers ------------------------------------------------------------
    let mut carriers = Vec::new();
    for (&prot, offs) in &carrier_lines {
        let sm = stream_cache.get(&prot).and_then(Option::as_ref);
        let footprint = patcher
            .entry_true_footprint_sectors(prot)
            .map_or(0, |n| n as usize * 2048);
        let sector_slack = sm.and_then(|sm| {
            patcher
                .read_entry_footprint(prot)
                .ok()
                .map(|foot| sm.sector_slack(&foot))
        });
        let t = dry.and_then(|r| r.trace.carriers.get(&prot));
        carriers.push(CarrierSpace {
            prot,
            scene: scene_name(&cdname, prot),
            streaming: sm.is_some(),
            lines: offs.len(),
            footprint,
            sector_slack,
            max_footprint: sm.map(|_| MAX_GROWN_FOOTPRINT),
            filled: dry.map(|_| filled_in("raw:", prot)),
            path: dry.map(|_| t.map_or(WritePath::None, |t| t.path)),
            grown_sectors: t.and_then(|t| t.grown_sectors),
        });
    }

    // --- Summary -------------------------------------------------------------------
    let mut summary = SpaceSummary {
        entries: entries.len(),
        filled: filled.len(),
        name_bytes: name_regions.iter().map(|r| r.total).sum(),
        name_free_english: name_regions.iter().map(|r| r.english_free).sum(),
        name_free_pack: dry.map(|_| name_regions.iter().filter_map(|r| r.pack_free).sum()),
        scenes: scenes.len(),
        scenes_rolled_back: scenes.iter().filter(|s| !s.rolled_back.is_empty()).count(),
        relayout_entries: dry.map_or(0, |r| r.relayout_entries),
        relayout_sectors: dry.map_or(0, |r| r.relayout_sectors_added),
        ..Default::default()
    };
    if dry.is_some() {
        for e in &entries {
            if let Some(o) = e.outcome.filter(|o| *o != Outcome::Untranslated) {
                *summary.outcomes.entry(o.as_str().to_string()).or_default() += 1;
            }
        }
    }

    let code_images = code_image_rows(&code, dry, &summary);
    let spare_regions = spare_rows(&code);
    let categories = category_rows(&entries, &names, &monsters, &code_images, &summary);
    Ok(SpaceReport {
        schema: SPACE_SCHEMA.to_string(),
        language: pack.map(|p| p.language.clone()),
        relayout,
        summary,
        name_regions,
        names,
        monsters,
        pools,
        scenes,
        carriers,
        entries,
        characters: Vec::new(),
        categories,
        code_images,
        spare_regions,
    })
}

fn image_name(id: usize) -> String {
    if id == usize::MAX {
        "SCUS_942.54".to_string()
    } else {
        format!("PROT {id:04}")
    }
}

/// Code image id (a PROT entry, `usize::MAX` for the executable) -> its
/// bytes, load base and code strings.
type CodeImages = BTreeMap<usize, (Vec<u8>, u32, CodeStrings)>;

/// One row per code image: movable / pinned strings, the compaction's free
/// bytes with English, the spare room outside the pools, and what the pack
/// leaves.
fn code_image_rows(
    code: &CodeImages,
    dry: Option<&ImportReport>,
    summary: &SpaceSummary,
) -> Vec<CodeImageSpace> {
    let mut out = Vec::new();
    for (&id, (buf, base, cs)) in code {
        let extra = if id == usize::MAX {
            Vec::new()
        } else {
            space_ledger::translation_spans(space_ledger::Image::Prot(id), buf, *base)
        };
        let lay = cs.layout(buf, &BTreeMap::new(), &extra);
        let english_free: usize = lay.regions.iter().map(|r| r.free).sum();
        let spare = if id == usize::MAX {
            summary.name_free_english
        } else {
            extra.iter().map(|(s, e)| (e - s) as usize).sum()
        };
        let mut pinned: BTreeMap<String, usize> = BTreeMap::new();
        let mut movable = 0;
        let mut strings = 0;
        for va in cs.vas() {
            strings += 1;
            match cs.pin_reason(va) {
                None => movable += 1,
                Some(p) => {
                    let name = serde_json::to_value(p)
                        .ok()
                        .and_then(|v| v.as_str().map(str::to_string))
                        .unwrap_or_default();
                    *pinned.entry(name).or_default() += 1;
                }
            }
        }
        let pack_free = dry.map(|r| match r.trace.code_layouts.get(&id) {
            Some(l) => l.free(),
            None if id == usize::MAX => {
                english_free + summary.name_free_pack.unwrap_or(summary.name_free_english)
            }
            None => english_free + spare,
        });
        out.push(CodeImageSpace {
            image: image_name(id),
            prot: (id != usize::MAX).then_some(id),
            section: if id == usize::MAX {
                "system_text"
            } else {
                "ui_menu"
            }
            .to_string(),
            strings,
            movable,
            pinned,
            english_free,
            spare,
            pack_free,
        });
    }
    out
}

/// Every ledger region, with the bytes still zero on this disc.
fn spare_rows(code: &CodeImages) -> Vec<SpareSpace> {
    space_ledger::REGIONS
        .iter()
        .map(|r| {
            let id = match r.image {
                space_ledger::Image::Scus => usize::MAX,
                space_ledger::Image::Prot(p) => p,
            };
            let zero = code.get(&id).map_or(0, |(buf, base, _)| {
                space_ledger::zero_spans(buf, *base, &[(r.start_va, r.end_va)])
                    .iter()
                    .map(|(s, e)| (e - s) as usize)
                    .sum()
            });
            let (owner, mods) = match r.owner {
                space_ledger::Owner::Translation => ("translation", Vec::new()),
                space_ledger::Owner::Mods(m) => ("mods", m.iter().map(|s| s.to_string()).collect()),
            };
            SpareSpace {
                image: image_name(id),
                start_va: r.start_va,
                end_va: r.end_va,
                owner: owner.to_string(),
                mods,
                zero_on_disc: zero,
                why: r.why.to_string(),
            }
        })
        .collect()
}

/// The five categories a translator asks about, each summarised from the
/// rows above.
fn category_rows(
    entries: &[EntrySpace],
    names: &[NameSpace],
    monsters: &[MonsterSpace],
    code_images: &[CodeImageSpace],
    summary: &SpaceSummary,
) -> Vec<CategorySpace> {
    let sums = |f: &dyn Fn(&EntrySpace) -> bool| {
        let rows: Vec<&EntrySpace> = entries.iter().filter(|e| f(e)).collect();
        let english: usize = rows
            .iter()
            .map(|e| e.english_len.unwrap_or(e.room) + 1)
            .sum();
        let room: usize = rows.iter().map(|e| e.room + 1).sum();
        (rows.len(), english, room)
    };
    let landed_len = |e: &EntrySpace| match e.outcome {
        Some(o) if o.lands() => e.pack_len.or(e.english_len),
        _ => e.english_len,
    };
    let pack = summary.name_free_pack.is_some();
    let mut out = Vec::new();

    let (n, en, room) = sums(&|e| NAME_SECTIONS.contains(&e.section.as_str()));
    out.push(CategorySpace {
        category: "items".into(),
        sections: NAME_SECTIONS.iter().map(|s| s.to_string()).collect(),
        carrier: "SCUS_942.54 name pools (always resident)".into(),
        addressing: "a pointer word in each table record".into(),
        strings: n,
        growable: names.iter().filter(|x| x.movable).count(),
        english_bytes: en,
        room_bytes: room,
        free_english: summary.name_free_english,
        spare_bytes: 0,
        free_pack: summary.name_free_pack,
        growth: "moves into the free run of any name region; the pools are compacted and the \
                 table slots repointed"
            .into(),
        competing: vec![
            "system_text: a longer system string takes the free runs the names leave".into(),
            "--delilas-challenge: its three custom items point their name slots at code caves \
             outside the pools (composes)"
                .into(),
        ],
    });

    let (n, en, room) = sums(&|e| e.section == "monster_names");
    let head = |len: &dyn Fn(&EntrySpace) -> Option<usize>| -> usize {
        entries
            .iter()
            .filter(|e| e.section == "monster_names")
            .filter_map(|e| {
                let cap = monsters.iter().find(|m| m.key == e.key)?.cap;
                Some(cap.saturating_sub(len(e)?))
            })
            .sum()
    };
    out.push(CategorySpace {
        category: "monster_names".into(),
        sections: vec!["monster_names".into()],
        carrier: "PROT 0867 monster archive, one record per monster".into(),
        addressing: "the record's +0x00 block-relative offset word".into(),
        strings: n,
        growable: monsters.iter().filter(|m| m.cap > m.room).count(),
        english_bytes: en,
        room_bytes: room,
        free_english: head(&|e| e.english_len),
        spare_bytes: 0,
        free_pack: pack.then(|| head(&landed_len)),
        growth: "the record grows, up to the longest retail name".into(),
        competing: vec![
            "legaia-patcher monster-model: re-packs the same decoded block into the same slot"
                .into(),
            "--monster-stats / --enemy-stat-scale: same-size field writes in the record \
             (compose)"
                .into(),
        ],
    });

    let (n, en, room) = sums(&|e| e.section == "place_names");
    let cells = |len: &dyn Fn(&EntrySpace) -> Option<usize>| -> usize {
        entries
            .iter()
            .filter(|e| e.section == "place_names")
            .map(|e| e.room.saturating_sub(len(e).unwrap_or(e.room)))
            .sum()
    };
    out.push(CategorySpace {
        category: "place_names".into(),
        sections: vec!["place_names".into()],
        carrier: "SCUS_942.54 quick-travel cells (the world-map label table and each scene's \
                  entry banner are separate carriers)"
            .into(),
        addressing: "fixed 0x20-byte cells indexed by landmark".into(),
        strings: n,
        growable: 0,
        english_bytes: en,
        room_bytes: room,
        free_english: cells(&|e| e.english_len),
        spare_bytes: 0,
        free_pack: pack.then(|| cells(&landed_len)),
        growth: "none needed: each cell holds 31 bytes".into(),
        competing: vec![
            "--rename-location: writes the same cells, the world-map labels (23 characters) \
             and the entry banners"
                .into(),
        ],
    });

    for (cat, carrier, addressing, growth, competing) in [
        (
            "ui_menu",
            "overlay data segments (field 0897, battle 0898, menu 0899, battle tutorial 0967, \
             cast modules 0941 / 0954)",
            "lui pairs and pointer words in the overlay's own code",
            "moves with every reference into its image's compacted pools or the ledger's \
             translation region (menu overlay)",
            vec![
                "--seru-trade / --show-super-arts: menu overlay run-C and the description run \
                 (never used by translation)"
                    .to_string(),
                "--enemy-hp-bar / arts hooks: code in the battle overlay, outside the pools"
                    .to_string(),
            ],
        ),
        (
            "system_text",
            "SCUS_942.54 system pools (always resident)",
            "pointer words (screen-element records, message tables), lui pairs, gp-relative forms",
            "moves with every reference into its pools' compaction or the name pools' free runs",
            vec![
                "items: shares the name pools' free runs (names are placed first)".to_string(),
                "every SCUS mod arena (--shiny-seru, --equipment-drops, --flee-exp, ...) is \
                 off limits"
                    .to_string(),
            ],
        ),
    ] {
        let (n, en, room) = sums(&|e| e.section == cat);
        let rows: Vec<&CodeImageSpace> = code_images.iter().filter(|c| c.section == cat).collect();
        out.push(CategorySpace {
            category: cat.into(),
            sections: vec![cat.into()],
            carrier: carrier.into(),
            addressing: addressing.into(),
            strings: n,
            growable: rows.iter().map(|c| c.movable).sum(),
            english_bytes: en,
            room_bytes: room,
            free_english: rows.iter().map(|c| c.english_free).sum(),
            spare_bytes: rows.iter().map(|c| c.spare).sum(),
            free_pack: pack.then(|| rows.iter().filter_map(|c| c.pack_free).sum()),
            growth: growth.into(),
            competing,
        });
    }
    out
}
