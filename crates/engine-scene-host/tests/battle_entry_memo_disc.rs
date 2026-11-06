//! Disc-gated: the battle edge's memos hand back what a fresh build makes.
//!
//! A battle form is a pure function of the disc, the present-party ordinal,
//! the roster slot and five equipment bytes; a stage's resources of the block,
//! the stage stream and a pack's tint. The party half walks every character in
//! every band through a memo too small to hold them, so a key too narrow would
//! serve one member another's form: a rig swap (`cave01` hands the party to
//! Noa) and an equipment change are in the walk. The form and the VRAM its log
//! replays must equal a fresh build's, hit or miss.
//!
//! Skips without `LEGAIA_DISC_BIN` or without an `extracted/` tree (PROT.DAT + CDNAME.TXT).

use std::path::PathBuf;

use legaia_engine_battle::battle_party_form::{
    PartyFormDisc, PartyFormKey, PartyFormMemo, PartyFormSources, VramWriteLog,
    build_party_battle_form,
};
use legaia_engine_core::battle_party_form::ProtFormDisc;
use legaia_engine_scene::scene::ProtIndex;
use legaia_engine_scene_host::scene_host::SceneHost;

fn extracted_dir() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    ["extracted", "../../extracted"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.join("PROT.DAT").exists() && p.join("CDNAME.TXT").exists())
}

/// A form as text (every field carries `Debug`) and the VRAM its log leaves.
fn fresh(
    index: &ProtIndex,
    sources: &PartyFormSources,
    key: PartyFormKey,
) -> Option<(String, Vec<u8>, Vec<u8>)> {
    let mut log = VramWriteLog::default();
    let form = build_party_battle_form(
        &ProtFormDisc(index),
        sources,
        &mut log,
        key.member,
        key.cslot,
        &key.equipped,
    )?;
    let mut vram = legaia_tim::Vram::new();
    log.replay(&mut vram);
    Some((
        format!("{form:?}"),
        vram.as_bytes().to_vec(),
        vram.written_bytes().to_vec(),
    ))
}

fn through_memo(
    index: &ProtIndex,
    sources: &PartyFormSources,
    memo: &mut PartyFormMemo,
    key: PartyFormKey,
) -> Option<(String, Vec<u8>, Vec<u8>)> {
    let mut log = VramWriteLog::default();
    let form = memo.form(&ProtFormDisc(index), sources, &mut log, key)?;
    let mut vram = legaia_tim::Vram::new();
    log.replay(&mut vram);
    Some((
        format!("{form:?}"),
        vram.as_bytes().to_vec(),
        vram.written_bytes().to_vec(),
    ))
}

/// The equipment a walk tries: bare, then a spread of item ids through each
/// slot, as a player's would change between fights.
fn outfits() -> Vec<[u8; 5]> {
    let mut out = vec![[0u8; 5]];
    for slot in 0..5 {
        for id in (1u8..=u8::MAX).step_by(9) {
            let mut e = [0u8; 5];
            e[slot] = id;
            out.push(e);
        }
    }
    out
}

#[test]
fn a_memoised_battle_form_is_what_a_fresh_build_makes() {
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    let index = ProtIndex::open_extracted(&dir).expect("open extracted tree");
    let sources = PartyFormSources::load(&ProtFormDisc(&index)).expect("fallback pack");
    let mut memo = PartyFormMemo::default();
    let mut keys = Vec::new();
    for cslot in 0..4 {
        for member in 0..3 {
            for equipped in outfits() {
                // Every band bare, and the equipment spread in the first.
                if member == 0 || equipped == [0; 5] {
                    keys.push(PartyFormKey {
                        member,
                        cslot,
                        equipped,
                    });
                }
            }
        }
    }
    let mut differ = 0;
    let mut last = None;
    for &key in &keys {
        let want = fresh(&index, &sources, key);
        // Miss, then hit, each against the fresh build.
        assert_eq!(
            through_memo(&index, &sources, &mut memo, key),
            want,
            "miss: {key:?}"
        );
        assert_eq!(
            through_memo(&index, &sources, &mut memo, key),
            want,
            "hit: {key:?}"
        );
        if last.as_ref().is_some_and(|l| *l != want) {
            differ += 1;
        }
        last = Some(want);
    }
    // The walk is only worth anything if the keys it crosses build different
    // forms: each character in each band, and real equipment among the ids.
    eprintln!("{differ} of {} steps built another form", keys.len());
    assert!(differ >= 24, "{differ} of {}", keys.len());
    // A key revisited after others filled the memo: the rig swap's shape, the
    // same band on Vahn, then Noa, then Vahn in new clothes, then Vahn again.
    let vahn = PartyFormKey {
        member: 0,
        cslot: 0,
        equipped: [0; 5],
    };
    let noa = PartyFormKey { cslot: 1, ..vahn };
    let dressed = PartyFormKey {
        equipped: [1, 0, 0, 0, 0],
        ..vahn
    };
    for key in [vahn, noa, dressed, vahn, noa] {
        assert_eq!(
            through_memo(&index, &sources, &mut memo, key),
            fresh(&index, &sources, key),
            "{key:?}"
        );
    }
    assert_ne!(
        fresh(&index, &sources, vahn).map(|f| f.0),
        fresh(&index, &sources, noa).map(|f| f.0),
        "a rig swap builds another form"
    );
}

/// Installing from a shared build must leave it as it was: an actor holds the
/// clips while the memo holds them for the next fight.
#[test]
fn a_memo_hands_out_clips_it_keeps_unchanged() {
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    let index = ProtIndex::open_extracted(&dir).expect("open extracted tree");
    let sources = PartyFormSources::load(&ProtFormDisc(&index)).expect("fallback pack");
    let mut memo = PartyFormMemo::default();
    let key = PartyFormKey {
        member: 0,
        cslot: 0,
        equipped: [0; 5],
    };
    let mut log = VramWriteLog::default();
    let mut first = memo
        .form(&ProtFormDisc(&index), &sources, &mut log, key)
        .expect("Vahn builds");
    assert!(first.action_clips.take().is_some());
    first.art_bank = Default::default();
    let second = memo
        .form(&ProtFormDisc(&index), &sources, &mut log, key)
        .expect("Vahn builds");
    assert!(second.action_clips.is_some(), "the memo kept its clips");
    assert!(!second.art_bank.is_empty(), "the memo kept its art bank");
}

/// A disc whose player files move under the memo: Vahn's file reads as Noa's
/// once `swapped` is up, so a held build no longer matches a fresh one.
struct MovingDisc<'a> {
    index: &'a ProtIndex,
    swapped: std::cell::Cell<bool>,
}

impl PartyFormDisc for MovingDisc<'_> {
    fn entry(&self, prot: u32) -> Result<std::sync::Arc<Vec<u8>>, String> {
        let prot = if self.swapped.get() && prot == 863 {
            864
        } else {
            prot
        };
        ProtFormDisc(self.index).entry(prot)
    }
}

/// The verify switch fires on a hit that differs from a fresh build, and only
/// with the switch up: the ladder's run under it would prove nothing otherwise.
#[test]
fn the_verify_switch_catches_a_party_form_hit_that_differs() {
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    let index = ProtIndex::open_extracted(&dir).expect("open extracted tree");
    let disc = MovingDisc {
        index: &index,
        swapped: std::cell::Cell::new(false),
    };
    let sources = PartyFormSources::load(&disc).expect("fallback pack");
    let key = PartyFormKey {
        member: 0,
        cslot: 0,
        equipped: [0; 5],
    };
    let mut memo = PartyFormMemo::default();
    let mut log = VramWriteLog::default();
    memo.form(&disc, &sources, &mut log, key).expect("builds");
    memo.verify = true;
    // A hit that still matches passes.
    memo.form(&disc, &sources, &mut log, key).expect("builds");
    disc.swapped.set(true);
    // With the switch down the differing hit is served, unexamined.
    memo.verify = false;
    memo.form(&disc, &sources, &mut log, key).expect("builds");
    memo.verify = true;
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        memo.form(&disc, &sources, &mut log, key)
    }));
    let message = caught.expect_err("the verify switch fires");
    let text = message
        .downcast_ref::<String>()
        .map(String::as_str)
        .unwrap_or_default();
    assert!(text.contains("differs from a fresh build"), "{text}");
}

#[test]
fn the_verify_switch_catches_a_stage_hit_that_differs() {
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    let mut host = SceneHost::open_extracted(&dir).expect("open extracted tree");
    host.enter_field_scene("town01", 0).expect("scene enters");
    host.world.toggles.battle_memo_verify = true;
    host.battle_stage_resources().expect("town01 has a stage");
    // A hit that still matches passes.
    host.battle_stage_resources().expect("town01 has a stage");
    assert!(host.corrupt_battle_stage_memo(), "a stage is held");
    // With the switch down the differing hit is served, unexamined.
    host.world.toggles.battle_memo_verify = false;
    host.battle_stage_resources().expect("town01 has a stage");
    host.world.toggles.battle_memo_verify = true;
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        host.battle_stage_resources().map(|_| ())
    }));
    let message = caught.expect_err("the verify switch fires");
    let text = message
        .downcast_ref::<String>()
        .map(String::as_str)
        .unwrap_or_default();
    assert!(text.contains("differs from a fresh build"), "{text}");
}

#[test]
fn a_memoised_stage_is_what_a_fresh_build_makes() {
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    let mut host = SceneHost::open_extracted(&dir).expect("open extracted tree");
    let scenes: Vec<String> = ["town01", "map01", "jouina", "cave01", "koin1"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let mut seen = Vec::new();
    // Each scene's stage fresh, across every other scene's in turn: a stage
    // resolved after another must not differ from one resolved first.
    for pass in 0..2 {
        for name in &scenes {
            host.enter_field_scene(name, 0).expect("scene enters");
            let Some(built) = host.battle_stage_resources() else {
                continue;
            };
            let again = host.battle_stage_resources().expect("the stage resolves");
            assert!(
                std::sync::Arc::ptr_eq(&built, &again),
                "{name}: the same stage twice is a hit"
            );
            if pass == 0 {
                seen.push((name.clone(), built));
            } else {
                let (_, first) = seen
                    .iter()
                    .find(|(n, _)| n == name)
                    .expect("seen on the first pass");
                assert!(
                    !std::sync::Arc::ptr_eq(first, &built),
                    "{name}: rebuilt, not held"
                );
                assert_eq!(first.stage_entry, built.stage_entry, "{name}");
                assert_eq!(first.dome.1, built.dome.1, "{name}: dome bytes");
                assert_eq!(
                    first.vram.as_bytes(),
                    built.vram.as_bytes(),
                    "{name}: stage VRAM"
                );
                assert_eq!(
                    first.vram.written_bytes(),
                    built.vram.written_bytes(),
                    "{name}: stage VRAM's written mask"
                );
            }
        }
    }
    assert!(seen.len() >= 3, "only {} scenes had a stage", seen.len());
    // Between the two scenes' stages the entries differ, or the walk proved
    // nothing about the key.
    assert!(
        seen.windows(2)
            .any(|w| w[0].1.stage_entry != w[1].1.stage_entry)
    );
}

/// The one-walk monster mesh is the two walks it replaced, for every monster
/// id in every texture slot, and leaves the same VRAM.
#[test]
fn one_monster_walk_is_the_mesh_and_the_ids_of_two() {
    let Some(dir) = extracted_dir() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/ incomplete");
        return;
    };
    use legaia_actor_formats::monster_archive as ma;
    let index = ProtIndex::open_extracted(&dir).expect("open extracted tree");
    let archive = index.entry_bytes(867).expect("monster archive");
    let mut checked = 0;
    let slots = (archive.len() / ma::SLOT_STRIDE) as u16;
    for id in 1..=slots {
        let Ok(Some(mesh)) = ma::mesh(&archive, id) else {
            continue;
        };
        let Ok(tmd) = legaia_tmd::parse(mesh.tmd_bytes()) else {
            continue;
        };
        let ids = legaia_tmd::mesh::tmd_to_vram_mesh_with_object_ids(&tmd, mesh.tmd_bytes()).1;
        for slot in 0..5u8 {
            let mut a = legaia_tim::Vram::new();
            let mut b = legaia_tim::Vram::new();
            let old = mesh.battle_render_mesh(slot, &mut a);
            let new = mesh.battle_render_mesh_with_object_ids(slot, &mut b);
            assert_eq!(a.as_bytes(), b.as_bytes(), "monster {id} slot {slot}: VRAM");
            match (old, new) {
                (Some(old), Some((new, new_ids))) => {
                    assert_eq!(old.positions, new.positions, "monster {id} slot {slot}");
                    assert_eq!(old.uvs, new.uvs, "monster {id} slot {slot}");
                    assert_eq!(old.cba_tsb, new.cba_tsb, "monster {id} slot {slot}");
                    assert_eq!(old.indices, new.indices, "monster {id} slot {slot}");
                    assert_eq!(ids, new_ids, "monster {id} slot {slot}");
                }
                (None, None) => {}
                _ => panic!("monster {id} slot {slot}: one walk built and one did not"),
            }
        }
        checked += 1;
    }
    assert!(checked > 100, "only {checked} monsters decoded");
}
