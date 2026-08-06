//! Reach conversion: two GATED-(b) field-VM arms driven from **real scene
//! bytecode** rather than from hand-written instructions.
//!
//! | row | encoding | gate | host |
//! |---|---|---|---|
//! | `world/vm_hosts.rs` `801d2d38` | `43 02 <a1> <a2> <a3> <lo> <hi> <b6>` | system flag `0xD`, the talk lock | `op43_three_actor_talk` (`FUN_801D2D38`) |
//! | `engine-vm/escape_timer.rs` `801d2ebc` | `4C D3 …` (13 bytes) | a scene whose script arms the countdown | `schedule_timed_flags` -> `World::tick` -> `EscapeTimer::tick` |
//!
//! Both already had unit coverage, and both unit suites **synthesise** their
//! instruction. That proves the interpreter runs; it does not prove any
//! shipped scene reaches the arm, which is the question a reach row asks. So
//! this file writes no bytecode at all: it walks every CDNAME scene's MAN
//! carriers through the field-VM disassembler, takes matches at decoded
//! instruction boundaries behind the census tools' clean-resync run, and then
//! executes the record that carries them in a real `World`.
//!
//! The escape-timer row's name is a misnomer that has already misled once:
//! `FUN_801D2EBC` is the field-VM `4C D3` scripted countdown (the collapsing
//! dungeon clock), **not** the battle flee. Nothing here touches battle
//! escape.
//!
//! Structural assertions only - no Sony text or bytes are printed or asserted.
//! Skip-passes without `LEGAIA_DISC_BIN` / `extracted/` (CLAUDE.md).

use std::path::PathBuf;

use legaia_engine_core::man_field_scripts::{
    CLEAN_RESYNC_INSNS, partition_record_span, scene_man_carriers,
};
use legaia_engine_core::scene::{ProtIndex, Scene};
use legaia_engine_core::world::{SceneMode, World};
use legaia_engine_vm::escape_timer::TimerInk;
use legaia_engine_vm::field_disasm::{ActorCtrlKind, Insn, InsnInfo, LinearWalker};

/// Frames a single `4C D3` carrier is driven for when its own duration is
/// longer than this. The shipped durations run to `35999` (ten wall-clock
/// minutes), and driving every one of them to expiry is minutes of ticking for
/// no extra claim.
const DRIVE_CAP: i32 = 3000;

/// One decoded site: the record body that carries it plus the instruction
/// offset inside that body.
#[derive(Clone)]
struct Site {
    scene: String,
    entry_idx: u32,
    body: Vec<u8>,
    pc: usize,
    insn: Insn,
}

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// Walk every scene's MAN carriers **once** and collect, per predicate, the
/// sites it accepts.
///
/// One pass rather than one per arm: the scan is the whole cost of this file
/// (about a hundred scenes, each decompressed and disassembled), and a second
/// pass buys nothing but a second copy of it.
type SitePredicate<'a> = (&'a dyn Fn(&Insn) -> bool, usize);

fn collect_sites_multi(
    index: &ProtIndex,
    names: &[String],
    wants: &[SitePredicate<'_>],
) -> Vec<Vec<Site>> {
    let mut out: Vec<Vec<Site>> = wants.iter().map(|_| Vec::new()).collect();
    for name in names {
        if out.iter().zip(wants).all(|(v, (_, cap))| v.len() >= *cap) {
            break;
        }
        let Ok(scene) = Scene::load(index, name) else {
            continue;
        };
        for carrier in scene_man_carriers(index, &scene) {
            let man = &carrier.payload;
            let Ok(man_file) = legaia_asset::man_section::parse(man) else {
                continue;
            };
            for partition in 0..3 {
                let count = (*man_file
                    .header
                    .partition_counts
                    .get(partition)
                    .unwrap_or(&0))
                .max(0) as usize;
                for record in 0..count {
                    let Some((start, pc0, len)) =
                        partition_record_span(&man_file, man, partition, record)
                    else {
                        continue;
                    };
                    let body = &man[start..start + len];
                    let mut ok_run = CLEAN_RESYNC_INSNS;
                    for insn in LinearWalker::new(body, pc0) {
                        let Ok(insn) = insn else {
                            ok_run = 0;
                            continue;
                        };
                        let clean = ok_run >= CLEAN_RESYNC_INSNS;
                        ok_run += 1;
                        if !clean {
                            continue;
                        }
                        for (i, (want, cap)) in wants.iter().enumerate() {
                            if out[i].len() < *cap && want(&insn) {
                                out[i].push(Site {
                                    scene: name.clone(),
                                    entry_idx: carrier.entry_idx,
                                    body: body.to_vec(),
                                    pc: insn.pc,
                                    insn: insn.clone(),
                                });
                            }
                        }
                    }
                }
            }
        }
    }
    out
}

/// A field world seated at `site`'s instruction, ready for one `step_field`.
fn world_at(site: &Site) -> World {
    let mut world = World {
        mode: SceneMode::Field,
        ..World::default()
    };
    world.roster = legaia_save::Party::zeroed(3);
    world.load_field_script_at(site.body.clone(), site.pc);
    world
}

fn menu_ctrl_op0(insn: &Insn) -> Option<u8> {
    match insn.info {
        InsnInfo::MenuCtrl { op0, .. } => Some(op0),
        _ => None,
    }
}

fn three_actor_talk(insn: &Insn) -> Option<([u8; 3], u16, u8)> {
    match insn.info {
        InsnInfo::ActorCtrl {
            kind:
                ActorCtrlKind::ThreeActorTalk {
                    actors,
                    arg_word,
                    b6,
                },
            ..
        } => Some((actors, arg_word, b6)),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// One corpus pass, both arms
// ---------------------------------------------------------------------------

#[test]
fn both_gated_field_arms_run_on_real_scene_bytecode() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let index = ProtIndex::open_extracted(&extracted).expect("open ProtIndex");
    let names = index.cdname_scene_names();
    assert!(!names.is_empty(), "CDNAME lists scenes");

    let mut found = collect_sites_multi(
        &index,
        &names,
        &[
            (&|i: &Insn| three_actor_talk(i).is_some(), 6),
            (&|i: &Insn| menu_ctrl_op0(i) == Some(0xD3), 8),
        ],
    );
    let countdowns = found.pop().expect("two predicates");
    let sites = found.pop().expect("two predicates");

    three_actor_talk_section(&sites);
    countdown_section(&countdowns);
}

// ---------------------------------------------------------------------------
// `43 02` - the three-actor talk setup (FUN_801D2D38)
// ---------------------------------------------------------------------------

fn three_actor_talk_section(sites: &[Site]) {
    // If this fires it is a finding, not a flake: it would mean the arm has no
    // carrier on the disc and the row is HOST-DEAD rather than GATED.
    assert!(
        !sites.is_empty(),
        "no scene carries a decoded `43 02` three-actor-talk instruction"
    );
    for s in sites {
        let (ids, word, b6) = three_actor_talk(&s.insn).expect("matched");
        eprintln!(
            "[43 02] {} entry={} pc={:#x} ids={:?} word={:#06x} b6={:#04x}",
            s.scene, s.entry_idx, s.pc, ids, word, b6
        );
    }

    let site = &sites[0];
    let (ids, word, b6) = three_actor_talk(&site.insn).expect("matched");

    // ---- first arm: the lock is clear -------------------------------------
    let mut world = world_at(site);
    world.party_actor_slots = vec![Some(0), Some(1), Some(2)];
    world.party_leader_slot = Some(1);
    // Seed live placements for whichever participants the disc names, so the
    // capture half is falsifiable rather than three `None`s.
    for (i, id) in ids.iter().enumerate() {
        world
            .field_npc_positions
            .insert(*id, (100 + i as i16 * 10, 200 + i as i16 * 10));
        world.field_npc_headings.insert(*id, 0x100 * (i as i16 + 1));
    }
    assert!(!world.system_flag_test(0xD), "the lock starts clear");

    world.step_field().expect("step the `43 02` instruction");

    assert!(
        world.system_flag_test(0xD),
        "{}: `43 02` must raise the talk lock",
        site.scene
    );
    assert_eq!(
        world.party_actor_slots,
        vec![Some(1)],
        "{}: the story party collapses to its leader",
        site.scene
    );
    assert!(world.system_flag_test(0x11), "flag 0x10 + leader(1)");
    assert!(!world.system_flag_test(0x10));
    assert!(!world.system_flag_test(0x12));

    let talk = world
        .three_actor_talk
        .expect("the session record is installed");
    assert_eq!(talk.actor_ids, ids, "the disc's own participant ids");
    assert_eq!(talk.script_id, word);
    assert_eq!(talk.duration, b6);
    let captured = talk.saved.iter().filter(|s| s.is_some()).count();
    assert!(
        captured > 0,
        "{}: the capture pass stored no participant transform - the paired \
         restore below would then be vacuous",
        site.scene
    );

    // ---- re-arm: the lock is up, so the same instruction restores ---------
    // Move every participant, then step the SAME real instruction again.
    for id in ids.iter() {
        world.field_npc_positions.insert(*id, (-999, -999));
        world.field_npc_headings.insert(*id, -1);
    }
    world.load_field_script_at(site.body.clone(), site.pc);
    world.step_field().expect("re-step the `43 02` instruction");

    let mut restored = 0usize;
    for (i, id) in ids.iter().enumerate() {
        if let Some((pos, heading)) = talk.saved[i] {
            assert_eq!(
                world.field_npc_positions.get(id).copied(),
                Some(pos),
                "{}: participant {i} was not put back",
                site.scene
            );
            assert_eq!(world.field_npc_headings.get(id).copied(), Some(heading));
            restored += 1;
        }
    }
    assert_eq!(
        restored, captured,
        "{}: every captured participant must be restored",
        site.scene
    );
    assert!(
        world.system_flag_test(0xD),
        "the lock stays up across a re-arm"
    );
    // The re-arm must NOT collapse the party a second time (it is already
    // collapsed) - and, more to the point, must not re-run the flag
    // choreography, because the leader is whoever the first arm chose.
    assert_eq!(world.party_actor_slots, vec![Some(1)]);
}

/// DEFECT REPRO (ignored: it fails today, and the fix is a port this lane
/// does not carry). Retail's talk **controller** `FUN_801D27E0` is what ends a
/// three-actor talk: at `0x801D2AE4..0x801D2B20` it writes the party count back
/// to `0x80084594`, restores the leader byte `0x80084597` / id `0x80084598`,
/// re-runs the `0x10/0x11/0x12` flag choreography, and the lock `0xD` it tested
/// at `0x801D28C8` drops with it. Nothing in the engine ports that SM, and the
/// only clearer of the flag bank is `World::begin_new_game`.
///
/// So in the port `43 02` is a one-way door: the story party stays collapsed to
/// its leader for the rest of the session, and every later `43 02` - in any
/// scene - takes the restore branch and teleports its participants onto the
/// *previous* talk's saved transforms.
///
/// Call site to fix: `crates/engine-core/src/world/vm_hosts.rs`
/// `op43_three_actor_talk` needs a paired release, driven by a port of
/// `FUN_801D27E0`'s terminal arm (the instruction's `b6` operand is that SM's
/// `+0x72` countdown seed).
#[test]
#[ignore = "defect: no port of FUN_801D27E0, so the talk lock and the party collapse are permanent"]
fn a_three_actor_talk_eventually_gives_the_party_back() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let index = ProtIndex::open_extracted(&extracted).expect("open ProtIndex");
    let names = index.cdname_scene_names();
    let sites = collect_sites_multi(
        &index,
        &names,
        &[(&|i: &Insn| three_actor_talk(i).is_some(), 2)],
    )
    .remove(0);
    let site = sites.first().expect("a `43 02` carrier");

    let mut world = world_at(site);
    world.party_actor_slots = vec![Some(0), Some(1), Some(2)];
    world.party_leader_slot = Some(0);
    world.step_field().expect("step the `43 02` instruction");
    assert_eq!(world.party_actor_slots.len(), 1, "the talk collapses first");

    // The talk's own duration operand is the controller's countdown seed, so
    // running well past it must see the party back.
    for _ in 0..2000 {
        world.tick();
    }
    assert!(
        !world.system_flag_test(0xD),
        "the talk lock must drop when the conversation ends"
    );
    assert_eq!(
        world.party_actor_slots.len(),
        3,
        "and the story party must come back"
    );
}

// ---------------------------------------------------------------------------
// `4C D3` - the scripted countdown (FUN_801D2EBC)
// ---------------------------------------------------------------------------

fn countdown_section(sites: &[Site]) {
    assert!(
        !sites.is_empty(),
        "no scene carries a decoded `4C D3` timed-flags instruction - the row \
         would then be HOST-DEAD, not GATED"
    );
    for s in sites {
        eprintln!(
            "[4C D3] {} entry={} pc={:#x} size={}",
            s.scene, s.entry_idx, s.pc, s.insn.size
        );
    }

    // Drive every site: the operands are disc data, so one carrier's duration
    // says nothing about another's, and a zero-duration write is retail's own
    // "leave it disarmed" case.
    let mut armed_any = false;
    let mut expired_any = false;
    for site in sites {
        let mut world = world_at(site);
        assert!(!world.escape_timer.armed, "starts disarmed");
        world.step_field().expect("step the `4C D3` instruction");

        let armed = world.escape_timer.armed;
        let duration = world.escape_timer.remaining;
        let threshold = world.escape_timer.warn_threshold;
        let flag_word = world.escape_timer_flag_word;
        eprintln!(
            "[4C D3] {} pc={:#x}: armed={armed} duration={duration} \
             threshold={threshold} flag_word={flag_word:#010x}",
            site.scene, site.pc
        );
        if !armed {
            // Retail's own `_DAT_800845B8 != 0` arm test.
            assert_eq!(duration, 0, "only a zero duration leaves it disarmed");
            continue;
        }
        armed_any = true;
        assert!(duration > 0);

        // The two flag ids the packed word carries. Both must be clear before
        // the drain, or "the tick fired them" measures nothing.
        let warn_flag = (flag_word & 0xFFF) as u16;
        let expiry_flag = ((flag_word >> 16) & 0xFFF) as u16;
        assert!(!world.system_flag_test(warn_flag));
        assert!(!world.system_flag_test(expiry_flag));

        // Drain it through `World::tick` - the same per-frame path both
        // rendering hosts run - and watch the readout, not just the counter.
        //
        // The longest shipped countdown is ten wall-clock minutes, so only
        // carriers inside [`DRIVE_CAP`] are driven to expiry; the rest are
        // driven far enough to prove the readout and the ink band. At least
        // one carrier must still expire (asserted after the loop), so the
        // cap cannot quietly turn this into a no-expiry test.
        let to_expiry = duration <= DRIVE_CAP;
        let mut saw_hud = false;
        let mut ink_seen: Vec<TimerInk> = Vec::new();
        let mut warned_at: Option<i32> = None;
        let budget = if to_expiry {
            duration as usize + 64
        } else {
            DRIVE_CAP as usize
        };
        for _ in 0..budget {
            world.tick();
            if let Some((m, s, hundredths, ink)) = world.escape_timer_hud {
                saw_hud = true;
                // The decomposition is a product of the tick, so it must stay
                // a valid clock face for every frame it is published on.
                assert!(
                    (0..60).contains(&s),
                    "{}: seconds cell out of range ({m}:{s}.{hundredths})",
                    site.scene
                );
                assert!(
                    (0..100).contains(&hundredths),
                    "{}: hundredths cell out of range ({m}:{s}.{hundredths})",
                    site.scene
                );
                assert!(m >= 0, "{}: negative minutes on the readout", site.scene);
                if ink_seen.last() != Some(&ink) {
                    ink_seen.push(ink);
                }
            }
            if warned_at.is_none() && world.system_flag_test(warn_flag) {
                warned_at = Some(world.escape_timer.remaining);
            }
            if !world.escape_timer.armed {
                break;
            }
        }
        assert!(saw_hud, "{}: the drain published no readout", site.scene);
        // Ink is selected off the remaining count, so a long countdown opens
        // on the safe band. That holds however far the drain got.
        if duration > 0x707 {
            assert_eq!(
                ink_seen.first(),
                Some(&TimerInk::Safe),
                "{}: a >0x707 countdown must open on the safe ink",
                site.scene
            );
        }
        if !to_expiry {
            // Bounded drain: the counter must at least be moving.
            assert!(
                world.escape_timer.remaining < duration,
                "{}: the countdown did not advance at all",
                site.scene
            );
            continue;
        }

        assert!(
            !world.escape_timer.armed,
            "{}: the countdown never expired within its own duration",
            site.scene
        );
        assert!(
            world.system_flag_test(expiry_flag),
            "{}: expiry flag {expiry_flag:#x} never fired",
            site.scene
        );
        expired_any = true;

        // The warning flag fires as the counter crosses the threshold - and
        // only then. A threshold at or above the duration fires it on frame
        // one, which is disc data, not a defect; assert the relation instead.
        if threshold > 0 {
            assert!(
                world.system_flag_test(warn_flag),
                "{}: warning flag {warn_flag:#x} never fired below threshold \
                 {threshold}",
                site.scene
            );
            if let Some(remaining) = warned_at {
                assert!(
                    remaining < threshold,
                    "{}: warning fired at {remaining}, not below {threshold}",
                    site.scene
                );
            }
        }
        if duration > 0x707 {
            assert!(
                ink_seen.contains(&TimerInk::Warning),
                "{}: it never reached the warning ink",
                site.scene
            );
        }
    }
    assert!(
        armed_any,
        "every decoded `4C D3` site wrote a zero duration - no countdown was \
         armed, so the scheduler was not entered"
    );
    assert!(expired_any, "no countdown was driven to expiry");

    // ---- the busy short-circuit, on the same carrier set ----
    // The half a "does it count down" test misses: a modal dialog or a
    // non-field mode must freeze the clock rather than let a conversation burn
    // the dungeon timer. Folded in here rather than given its own `#[test]`,
    // because the corpus scan above is what costs.
    let Some(site) = sites.iter().find(|s| {
        let mut w = world_at(s);
        w.step_field().is_some() && w.escape_timer.armed
    }) else {
        panic!("no `4C D3` carrier armed a countdown");
    };

    let mut world = world_at(site);
    world.step_field().expect("arm");
    for _ in 0..4 {
        world.tick();
    }
    assert!(
        world.escape_timer.armed && world.escape_timer.remaining > 0,
        "{}: the freeze below is only meaningful on a live countdown",
        site.scene
    );

    // A non-field mode is one of retail's three pause conditions.
    world.mode = SceneMode::Menu;
    let before = world.escape_timer.remaining;
    for _ in 0..30 {
        world.tick();
    }
    assert_eq!(
        world.escape_timer.remaining, before,
        "{}: the countdown must not drain outside the field",
        site.scene
    );
    world.mode = SceneMode::Field;
    for _ in 0..4 {
        world.tick();
    }
    assert!(
        world.escape_timer.remaining < before,
        "{}: and it must resume afterwards",
        site.scene
    );
}
