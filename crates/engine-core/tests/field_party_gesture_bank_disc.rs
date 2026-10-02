//! Disc-gated: a party member's story gestures bind from the bank its
//! **live** party-bank bit names.
//!
//! Vahn, Noa and Gala appear in field events as MAN placements with a party
//! model (`>= 0xF0`). The placement seater `FUN_8003A1E4` raises the actor's
//! party-bank bit `0x01000000` for such a model, and `FUN_800204F8` binds the
//! actor's clip ids through the party locomotion bank (PROT 0874 §1) while it
//! is up. The story scripts drop the bit (`B2 <id> 18`) to play a gesture out
//! of the scene's own ANM bundle and raise it again after (`B1 <id> 18`).
//! Binding by the spawn model instead resolved every gesture out of the
//! locomotion bank: the wrong clip, no clip at all for an id past its end, or
//! a two-bone save-crystal record on a ten-bone hero - the body came apart.
//!
//! The test spawns every partition-2 story record that drops a party
//! member's bit, ticks the scene host through it, and checks each party
//! re-target. Skip-passes without `LEGAIA_DISC_BIN`.

use legaia_asset::field_disasm::{FlagKind, InsnInfo, LinearWalker, man_script_spans};
use legaia_engine_core::field_anim::FieldClipPlayer;
use legaia_engine_core::scene::SceneHost;

/// Bones of every party-member field clip (the hero meshes carry 12 objects,
/// the last two equipment templates no clip poses).
const HERO_BONES: usize = 10;

#[test]
fn party_gestures_bind_from_the_live_party_bank() {
    let Some(disc) = std::env::var("LEGAIA_DISC_BIN")
        .ok()
        .filter(|p| std::path::Path::new(p).exists())
    else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    };
    let mut host = SceneHost::open_disc(&disc).expect("open disc");
    let loco = legaia_asset::character_pack::field_locomotion_anm(
        &host
            .index
            .entry_bytes(legaia_asset::character_pack::PROT_ENTRY_INDEX)
            .expect("PROT 0874"),
    )
    .expect("party locomotion bundle");
    let mut names = host.index.cdname_scene_names();
    names.sort();
    names.dedup();

    let (mut records, mut scene_bank_cues, mut rebanked) = (0usize, 0usize, 0usize);
    for name in &names {
        if host.enter_field_scene(name, 0).is_err() {
            continue;
        }
        let Some(scene) = host.scene.as_ref() else {
            continue;
        };
        let Ok(Some(man)) = scene.field_man_payload(&host.index) else {
            continue;
        };
        let Ok(mf) = legaia_asset::man_section::parse(&man) else {
            continue;
        };
        let p0 = mf.header.partition_counts[0].max(0) as usize;
        let n1 = mf.header.partition_counts[1].max(0) as usize;
        let party: Vec<u8> = mf
            .actor_placements(&man)
            .into_iter()
            .filter(|p| (0xF0..=0xF2).contains(&p.model_index))
            .map(|p| p.index as u8)
            .collect();
        // The seater raises the bit for every party model it seats.
        for &slot in &party {
            assert_eq!(
                host.world.npc_party_bank(slot),
                Some(true),
                "{name} P1[{slot}]: a party model spawns on the party bank"
            );
        }
        let ids: Vec<u8> = party.iter().map(|&s| (p0 + usize::from(s)) as u8).collect();
        let gesture_records: Vec<usize> = man_script_spans(&mf, &man)
            .into_iter()
            .filter(|&(part, _, start, pc0, len)| {
                part == 2
                    && LinearWalker::new(&man[start..start + len], pc0)
                        .flatten()
                        .any(|i| {
                            matches!(
                                i.info,
                                InsnInfo::CFlag {
                                    kind: FlagKind::Clear,
                                    bit: 24
                                }
                            ) && i.extended.is_some_and(|t| ids.contains(&t))
                        })
            })
            .map(|(_, rec, ..)| p0 + n1 + rec)
            .filter(|&flat| flat <= 0xFF)
            .collect();
        for flat in gesture_records {
            host.enter_field_scene(name, 0).expect("re-enter");
            records += 1;
            let scene_anm =
                legaia_engine_core::npc_catalog::scene_anm_bundle(host.scene.as_ref().unwrap());
            // What a host does at scene entry: bind each party member's
            // spawn clip, which fixes the bone count its mesh is cut to.
            for &slot in &party {
                if let Some(a) = host.world.field_npc_live_anim(usize::from(slot))
                    && let Some(p) = FieldClipPlayer::from_record(&loco, usize::from(a.max(1)) - 1)
                {
                    host.world.bind_npc_clip_cursor(slot, a, &p);
                }
            }
            host.world.field_vm.pending_record_spawns.push(flat as u8);
            for _ in 0..2400 {
                let cues: Vec<(u8, u8)> = host
                    .world
                    .npcs
                    .anim_cues
                    .iter()
                    .filter(|(s, _)| party.contains(s))
                    .map(|(s, c)| (*s, c.1))
                    .collect();
                let out = host
                    .world
                    .drain_field_anim_cues(scene_anm.as_ref(), Some(&loco), |s| {
                        Some(party.contains(&s))
                    });
                for (slot, id) in cues {
                    let party_bit = host.world.npc_party_bank(slot).unwrap_or(true);
                    let bound = out.iter().find(|r| r.slot == slot).map(|r| &r.player);
                    if let Some(b) = bound {
                        assert_eq!(
                            b.bone_count(),
                            HERO_BONES,
                            "{name} record {flat}: P1[{slot}] clip {id} is a hero pose"
                        );
                    }
                    if !party_bit {
                        scene_bank_cues += 1;
                        let spawn_rule =
                            FieldClipPlayer::from_record(&loco, usize::from(id.max(1)) - 1)
                                .map(|c| (c.frame_count(), c.bone_count()));
                        if bound.map(|b| (b.frame_count(), b.bone_count())) != spawn_rule {
                            rebanked += 1;
                        }
                    }
                }
                let _ = host.tick();
            }
        }
    }
    println!(
        "{records} story records, {scene_bank_cues} scene-bank party cues, {rebanked} bound a different clip than the spawn-model rule"
    );
    assert!(records > 0, "the disc carries party gesture records");
    assert!(
        scene_bank_cues > 0 && rebanked > 0,
        "non-vacuous: some gesture plays off the scene bank, and the spawn-model rule got it wrong"
    );
}
