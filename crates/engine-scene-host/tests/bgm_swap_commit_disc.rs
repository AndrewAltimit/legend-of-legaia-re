//! Disc-gated: a sub-op `9` track change sounds from its sub-op `0xA` commit,
//! not from the op that selects it.
//!
//! Sub-op `9`'s arm (`0x801E0224`) stores the id into `_DAT_8007BAC8` and
//! raises sound-flag bit `0` of `_DAT_8007B750`. The swap poller
//! `FUN_800243F0` reads that bit twice: its teardown stage leaves the field-BGM
//! source `0x8007052C` alone while the bit is up (`0x80024630..0x80024640`),
//! and its load-settle stage, having raised bit `3`, returns without
//! installing while bit `0` is up and the release-ack bit `4` is not
//! (`0x800246D4..0x800246FC`). Only the commit (`0x801E0264`) raises bit `4`;
//! the MAN loader `FUN_8003AEB0` clearing bit `0` is the other way out. So
//! between a sub-op `9` and its commit the outgoing track keeps the slot and
//! the incoming one is not heard. Two retail states sit inside that window
//! (`chapter2_garmel_pre_songi`, `chapter2_garmel_pre_zeto`: flags `0x9`, the
//! slot still holding `2043`, the requested `2033` / `2052` not attached).
//!
//! Skip-pass (disc-gated convention): `LEGAIA_DISC_BIN` unset or `extracted/`
//! missing.

use std::path::PathBuf;

use legaia_engine_field::field_events::FieldEvent;
use legaia_engine_scene_host::scene_host::{BgmDirector, SceneHost};

fn extracted_dir() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return None;
    }
    ["extracted", "../extracted", "../../extracted"]
        .iter()
        .map(PathBuf::from)
        .find(|d| d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists())
}

/// `(record kind, bgm ops)` for every field-VM record of a scene's MAN, in
/// partition 1 and 2, decoded with the opcode-aware walker.
fn scene_bgm_records(host: &SceneHost) -> Vec<Vec<(u16, u8)>> {
    use legaia_engine_field::man_field_scripts::partition_record_span;
    use legaia_engine_scene::man_census::scene_man_carriers;
    use legaia_scene_formats::field_disasm::{InsnInfo, LinearWalker};

    let Some(scene) = host.scene.as_ref() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for carrier in scene_man_carriers(&host.index, scene) {
        let man = carrier.payload.clone();
        let Ok(man_file) = legaia_scene_formats::man_section::parse(&man) else {
            continue;
        };
        for part in [1usize, 2] {
            let n = *man_file.header.partition_counts.get(part).unwrap_or(&0) as usize;
            for record in 0..n {
                let Some((start, pc0, len)) = partition_record_span(&man_file, &man, part, record)
                else {
                    continue;
                };
                let Some(body) = man.get(start..start + len) else {
                    continue;
                };
                let ops: Vec<_> = LinearWalker::new(body, pc0)
                    .flatten()
                    .filter_map(|insn| match insn.info {
                        InsnInfo::Bgm { text_id, sub_op } => Some((text_id, sub_op)),
                        _ => None,
                    })
                    .collect();
                if !ops.is_empty() {
                    out.push(ops);
                }
            }
        }
    }
    out
}

/// The disc's own shape: nearly every sub-op `9` is committed by a sub-op
/// `0xA` later in its own record - the shape that makes the commit, not the
/// select, the moment the new score is heard.
#[test]
fn sub_op_9_is_committed_inside_its_own_record() {
    let Some(extracted) = extracted_dir() else {
        return;
    };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    let names = host.index.cdname_scene_names();
    let (mut starts, mut committed, mut with_release) = (0usize, 0usize, 0usize);
    let mut uncommitted = Vec::new();
    for name in &names {
        if host.load_scene(name).is_err() {
            continue;
        }
        for ops in scene_bgm_records(&host) {
            for (i, &(id, sub)) in ops.iter().enumerate() {
                if sub != 9 {
                    continue;
                }
                starts += 1;
                let rest = &ops[i + 1..];
                let commit = rest.iter().position(|&(_, s)| s == 0xA);
                // A later start before the commit re-selects the track; the
                // commit then attaches that one.
                let restart = rest.iter().position(|&(_, s)| s == 9 || s == 1);
                match commit {
                    Some(c) if restart.is_none_or(|r| c < r) => {
                        committed += 1;
                        if rest[..c].iter().any(|&(_, s)| s == 5) {
                            with_release += 1;
                        }
                    }
                    _ => uncommitted.push(format!("{name}: {id}")),
                }
            }
        }
    }
    eprintln!(
        "[census] {starts} sub-op 9 starts in {} scenes: {committed} committed in their record \
         ({with_release} with a timed release before the commit), {} not: {uncommitted:?}",
        names.len(),
        uncommitted.len()
    );
    assert!(
        starts > 0,
        "no sub-op 9 on the disc - the census lost its subject"
    );
    assert!(
        committed * 10 >= starts * 9,
        "fewer than nine in ten sub-op 9 starts are committed in their record"
    );
}

#[derive(Default)]
struct Log(Vec<String>);

impl BgmDirector for Log {
    fn start_owned_vab(&mut self, bgm_id: u16, _entry: &[u8]) {
        self.0.push(format!("start {bgm_id}"));
    }
    fn pause(&mut self) {
        self.0.push("pause".into());
    }
    fn stop(&mut self) {
        self.0.push("stop".into());
    }
    fn unhalt_pause(&mut self) {
        self.0.push("unhalt".into());
    }
}

fn route(host: &mut SceneHost, d: &mut Log, events: &[(u16, u8)]) {
    for &(text_id, sub_op) in events {
        host.world
            .pending_field_events
            .push(FieldEvent::Bgm { text_id, sub_op });
    }
    host.route_bgm_events(d).expect("route");
}

/// `garmel`'s pre-Songi record shape, `9 (2033) · 5 · 7 · ... · 0xA`: the
/// select selects (the word moves), the commit starts.
#[test]
fn the_commit_starts_the_track_the_select_chose() {
    let Some(extracted) = extracted_dir() else {
        return;
    };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.load_scene("garmel").expect("load garmel");
    let mut d = Log::default();
    route(&mut host, &mut d, &[(2043, 1)]);
    assert_eq!(d.0, ["start 2043"]);
    // The select raises sound-flag bit 0 (the field VM's arm does this
    // when it runs the op; the commit and the MAN loader end it).
    host.world.audio.set_start_pending_commit(true);
    route(&mut host, &mut d, &[(2033, 9)]);
    assert_eq!(
        host.bgm_track_word,
        Some(2033),
        "the select writes the word"
    );
    assert_eq!(
        d.0,
        ["start 2043"],
        "the outgoing track keeps the slot until the commit"
    );
    host.world.audio.set_start_pending_commit(false);
    route(&mut host, &mut d, &[(0, 0xA)]);
    assert_eq!(
        d.0,
        ["start 2043", "stop", "start 2033"],
        "the commit releases the outgoing track and attaches the selected one"
    );
    // A commit with no select pending is the plain unhalt.
    route(&mut host, &mut d, &[(0, 2), (0, 0xA)]);
    assert_eq!(&d.0[3..], ["pause", "unhalt"]);
}

/// A select its scene never commits is attached when the window ends -
/// the MAN loader clears bit `0` and the poller installs what it holds -
/// unless the next scene's own start replaces it first.
#[test]
fn an_uncommitted_select_is_attached_when_its_window_ends() {
    let Some(extracted) = extracted_dir() else {
        return;
    };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.load_scene("garmel").expect("load garmel");
    let mut d = Log::default();
    host.world.audio.set_start_pending_commit(true);
    route(&mut host, &mut d, &[(2033, 9)]);
    assert!(d.0.is_empty(), "held behind the commit: {:?}", d.0);
    // The MAN loader ends the window.
    host.world.audio.set_start_pending_commit(false);
    host.route_bgm_events(&mut d).expect("route");
    assert_eq!(d.0, ["start 2033"]);
    // A held select the next scene's own start replaces is never heard.
    let mut d = Log::default();
    host.world.audio.set_start_pending_commit(true);
    route(&mut host, &mut d, &[(2033, 9)]);
    host.world.audio.set_start_pending_commit(false);
    route(&mut host, &mut d, &[(2043, 1)]);
    assert_eq!(d.0, ["start 2043"]);
}
