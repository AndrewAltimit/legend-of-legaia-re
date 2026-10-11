//! The incremental fit checks an editor calls per keystroke: one scene's dialog
//! against its footprint, and the name tables against their pools.

use super::super::export::SceneManText;
use super::super::import::{ImportReport, Key, WritePath, parse_key, plan_scene_man};
use super::super::name_pool::NamePool;
use super::super::pack::{Entry, LanguagePack};
use super::report::{EntrySpace, NameRegion, RoomKind, SceneSpace};
use super::{Outcomes, scene_name};
use anyhow::{Context, Result};
use legaia_disc_patch::disc::DiscPatcher;
use legaia_scene_formats::man_edit::{self, TextSite};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

/// [`scene_fit`]'s answer: the scene's row and one row per pack key in it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneFit {
    /// The scene (its `lines` = the pack's keys for it).
    pub scene: SceneSpace,
    /// One row per pack key in the scene (`english_len` is `None`: no
    /// export runs).
    pub entries: Vec<EntrySpace>,
}

/// Re-plan scene MAN `prot` with `pack`'s lines for it through the
/// importer's scene planner (`import::plan_scene_man`): nothing is written,
/// and no other scene or section is touched. The fast path for a live
/// editor; it costs one decompress, the relocator, and one or more
/// recompressions of this scene only.
pub fn scene_fit(
    patcher: &DiscPatcher,
    pack: &LanguagePack,
    prot: usize,
    relayout: bool,
) -> SceneFit {
    let (prepared, _) = super::super::accents::prepared(pack);
    let pack: &LanguagePack = &prepared;
    let mut keyed: Vec<(&str, &Entry)> = Vec::new();
    let mut edits: Vec<(usize, &Entry)> = Vec::new();
    for (section, es) in pack.sections.iter() {
        for e in es {
            if let Some(Key::Man { entry, off }) = parse_key(&e.key)
                && entry == prot
            {
                keyed.push((section, e));
                if e.is_filled() {
                    edits.push((off, e));
                }
            }
        }
    }
    let mut report = ImportReport::default();
    let plan = plan_scene_man(patcher, prot, &edits, relayout, &mut report);
    let staged: HashSet<&str> = plan
        .growth
        .as_ref()
        .map(|(_, keys, _)| keys.iter().map(String::as_str).collect())
        .unwrap_or_default();
    report.applied_keys = plan.applied.clone();
    let outcomes = Outcomes::new(&report, staged);
    let man = patcher
        .read_entry(prot)
        .ok()
        .and_then(|b| SceneManText::locate(&b));
    let t = report.trace.scenes.get(&prot);
    let entries = keyed
        .iter()
        .map(|(section, e)| {
            let walked = man.as_ref().is_some_and(|m| {
                matches!(parse_key(&e.key), Some(Key::Man { off, .. })
                    if man_edit::text_site(&m.decoded, off) == TextSite::Segment)
            });
            EntrySpace {
                key: e.key.clone(),
                section: section.to_string(),
                room_kind: if walked {
                    RoomKind::DialogGrowable
                } else {
                    RoomKind::DialogFixed
                },
                room: report.trace.rooms.get(&e.key).copied().unwrap_or(e.budget),
                group: Some(format!("scene:{prot}")),
                english_len: None,
                pack_len: report.trace.encoded.get(&e.key).copied(),
                outcome: Some(outcomes.of(&e.key, e.is_filled())),
                issue_kind: report.trace.issue_kinds.get(&e.key).copied(),
                issue: outcomes.issue(&e.key),
            }
        })
        .collect();
    let scene = SceneSpace {
        prot,
        scene: scene_name(&patcher.cdname(), prot),
        lines: keyed.len(),
        footprint: man.as_ref().map_or(0, |m| m.compressed_budget),
        disc_len: man.as_ref().map_or(0, |m| m.compressed_len),
        slack: man
            .as_ref()
            .map_or(0, |m| m.compressed_budget.saturating_sub(m.compressed_len)),
        filled: Some(edits.len()),
        path: Some(t.map_or(WritePath::None, |t| t.path)),
        written_len: t.and_then(|t| t.written_len),
        full_overflow: t.and_then(|t| t.full_overflow),
        padded_overflow: t.and_then(|t| t.padded_overflow),
        refused: t.is_some_and(|t| t.refused),
        rolled_back: t.map(|t| t.rolled_back.clone()).unwrap_or_default(),
        relayout_sectors: t.and_then(|t| t.relayout_sectors),
        relayout_would_add: t.and_then(|t| t.relayout_would_add),
    };
    SceneFit { scene, entries }
}

/// The importer's SCUS pass, reusable per keystroke: holds the retail
/// executable and its [`NamePool`] (the expensive part to build), and runs
/// the import's own SCUS planner over a copy for each [`Self::fit`].
pub struct NameFitter {
    scus: Vec<u8>,
    pool: NamePool,
    /// The executable's `system_text` strings, pinned against the overlays
    /// the import reads ([`super::super::import::system_strings`]).
    sys: Option<super::super::code_strings::CodeStrings>,
}

/// [`NameFitter::fit`]'s answer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamesFit {
    /// Every compaction region after the pack (same indices as
    /// [`SpaceReport::name_regions`]).
    pub regions: Vec<NameRegion>,
    /// One row per filled SCUS key (`scus:str` / `scus:party` /
    /// `scus:cell`): `room`, `pack_len`, `outcome` and `issue`; `group` and
    /// `english_len` are not filled in.
    pub entries: Vec<EntrySpace>,
}

impl NameFitter {
    /// Read the disc's executable and measure its pools.
    pub fn new(patcher: &DiscPatcher) -> Result<Self> {
        let scus = patcher
            .read_named_file("SCUS_942.54")
            .context("SCUS_942.54 not found in disc image")?;
        let sys = super::super::import::system_strings(patcher, &scus);
        let mut this = Self::from_scus(scus);
        this.sys = Some(sys);
        Ok(this)
    }

    /// Measure the pools of a retail executable. Without the overlays a
    /// `system_text` string cannot be proven movable, so this fitter
    /// predicts none moving; [`Self::new`] reads them and predicts what
    /// import does.
    pub fn from_scus(scus: Vec<u8>) -> Self {
        let pool = NamePool::build(&scus);
        Self {
            scus,
            pool,
            sys: None,
        }
    }

    /// Plan every filled SCUS entry of `pack` exactly as import does, on a
    /// copy of the executable.
    pub fn fit(&self, pack: &LanguagePack) -> NamesFit {
        let (prepared, _) = super::super::accents::prepared(pack);
        let pack: &LanguagePack = &prepared;
        let english = self.pool.layout(&self.scus, &BTreeMap::new());
        let mut work: Vec<&Entry> = Vec::new();
        let mut sections: HashMap<&str, &str> = HashMap::new();
        for (section, es) in pack.sections.iter() {
            for e in es.iter().filter(|e| e.is_filled()) {
                if matches!(
                    parse_key(&e.key),
                    Some(Key::ScusStr { .. } | Key::ScusParty { .. } | Key::ScusCell { .. })
                ) {
                    work.push(e);
                    sections.insert(&e.key, section);
                }
            }
        }
        let mut scus = self.scus.clone();
        let mut report = ImportReport::default();
        super::super::import::apply_scus_work(
            &mut scus,
            &self.pool,
            self.sys.as_ref(),
            &work,
            &mut report,
        );
        let outcomes = Outcomes::new(&report, HashSet::new());
        let after = report.trace.name_layout.as_ref();
        let regions = english
            .regions
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let a = after.and_then(|l| l.regions.get(i));
                NameRegion {
                    index: i,
                    start_va: r.start_va,
                    end_va: r.end_va,
                    total: r.total(),
                    strings: r.vas.len(),
                    english_used: r.used,
                    english_free: r.free,
                    pack_used: Some(a.map_or(r.used, |a| a.used)),
                    pack_free: Some(a.map_or(r.free, |a| a.free)),
                }
            })
            .collect();
        let entries = work
            .iter()
            .map(|e| {
                let movable = matches!(parse_key(&e.key), Some(Key::ScusStr { va }) if self.pool.is_movable(va));
                EntrySpace {
                    key: e.key.clone(),
                    section: sections[e.key.as_str()].to_string(),
                    room_kind: match parse_key(&e.key) {
                        Some(Key::ScusStr { .. }) if movable => RoomKind::NameMovable,
                        Some(Key::ScusStr { .. }) => RoomKind::StringFixed,
                        _ => RoomKind::Field,
                    },
                    room: report.trace.rooms.get(&e.key).copied().unwrap_or(e.budget),
                    group: None,
                    english_len: None,
                    pack_len: report.trace.encoded.get(&e.key).copied(),
                    outcome: Some(outcomes.of(&e.key, true)),
                    issue_kind: report.trace.issue_kinds.get(&e.key).copied(),
                    issue: outcomes.issue(&e.key),
                }
            })
            .collect();
        NamesFit { regions, entries }
    }
}
