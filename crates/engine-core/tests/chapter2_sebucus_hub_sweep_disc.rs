//! Disc-gated hub-sweep driver for the chapter-2 (Sebucus) arc: drive a live
//! `SceneHost` through the real scene topology - the `map02` overworld hub and
//! its dungeon spokes - and prove the progression gate spine advances
//! end-to-end as each beat EXECUTES (its script `SysFlag.Set` latching the
//! flag organically), across real `0x3F` scene transitions that preserve the
//! flag banks.
//!
//! This is the runtime companion to `chapter2_sebucus_spine_oracle` (which
//! drove the gate math with manual flag sets): here the flags latch by running
//! the records, and the arc is walked through the actual hub. The hub topology
//! (decoded from each scene's `0x3F` destination table):
//!
//! ```text
//!   map02 (Sebucus hub) --> geremi, balden, ropeway, jiji, ...
//!   geremi              --> tower
//!   tower               --> teien, geremi
//!   teien               --> tower, map02
//! ```
//! So the dungeon-arc route through the hub is
//! `map02 -> geremi -> tower -> teien` (run the teien beats) `-> tower` (run
//! the tower beat) `-> geremi`, then `map02 -> balden` for the balden leg.
//!
//! Skip-passes without `LEGAIA_DISC_BIN` / `extracted/` (CLAUDE.md convention).

use legaia_asset::man_section::{ManFile, parse as parse_man};
use legaia_engine_core::input::PadButton;
use legaia_engine_core::man_field_scripts::{partition2_record_gates, scene_destinations};
use legaia_engine_core::scene::{SceneHost, SceneTickEvent};
use std::collections::BTreeSet;
use std::path::PathBuf;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn open_host() -> Option<SceneHost> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let extracted = extracted_dir()?;
    Some(SceneHost::open_extracted(&extracted).expect("open SceneHost"))
}

/// The active scene's parsed MAN (bundle), for gate + seeder calls.
fn cur_man(host: &SceneHost) -> (ManFile, Vec<u8>) {
    let scene = host.scene.as_ref().expect("scene loaded");
    let man = scene
        .field_man_payload(&host.index)
        .expect("payload")
        .expect("MAN resolves");
    let mf = parse_man(&man).expect("parse MAN");
    (mf, man)
}

/// Named `0x3F`-style transition through the host: request it, tick, and
/// assert the host entered the destination.
fn hop(host: &mut SceneHost, dest: &str) {
    host.world.pending_named_scene_transition = Some((dest.to_string(), 10, 10, 0));
    match host.tick().expect("transition tick") {
        SceneTickEvent::SceneEntered { name } => assert_eq!(name, dest, "hop landed on {dest}"),
        other => panic!("expected SceneEntered({dest}), got {other:?}"),
    }
}

/// Spawn partition-2 record `rec` through the real gate seeder and run it to
/// its `target` flag SET by executing the cutscene timeline - mashing confirm
/// on alternate ticks so inline `0x1F` dialog parks advance (the proven
/// walk-on-beat drive). Tolerates a scene that already latched the flag on its
/// own entry.
fn play_beat(host: &mut SceneHost, mf: &ManFile, man: &[u8], rec: usize, target: u16) {
    if host.world.system_flag_test(target) {
        return; // scene entry already played it
    }
    assert!(
        host.world.install_gated_p2_record(mf, man, rec),
        "seeder spawns P2[{rec}] (its gate passes at this arc state)"
    );
    let mut n = 0u32;
    while host.world.cutscene_timeline_active() && !host.world.system_flag_test(target) && n < 8000
    {
        let pad = if n.is_multiple_of(2) {
            PadButton::Cross.mask()
        } else {
            0
        };
        host.world.set_pad(pad);
        host.tick().expect("beat tick");
        n += 1;
    }
    host.world.set_pad(0);
    assert!(
        host.world.system_flag_test(target),
        "P2[{rec}] latched flag 0x{target:X} by execution (ticked {n})"
    );
}

#[test]
fn sebucus_hub_reaches_its_dungeon_spokes() {
    let Some(mut host) = open_host() else { return };
    host.enter_field_scene("map02", 0).expect("enter map02 hub");
    let (mf, man) = cur_man(&host);
    let dests: BTreeSet<String> = scene_destinations(&mf, &man)
        .into_iter()
        .map(|d| d.scene_name)
        .collect();
    eprintln!("[map02 hub] destinations: {dests:?}");
    for spoke in ["geremi", "balden", "ropeway", "jiji"] {
        assert!(
            dests.contains(spoke),
            "map02 hub must reach {spoke}; got {dests:?}"
        );
    }
    // `tower` is an inner scene reached from geremi; its own `0x3F` controller
    // decodes cleanly and lists the teien leg (geremi's dialog-heavy P1 records
    // desync the linear walk, so its inner destinations aren't statically
    // recoverable - the geremi->tower->teien routing is instead exercised live
    // by `sebucus_hub_sweep_advances_the_gate_spine`'s real hops).
    host.enter_field_scene("tower", 0).expect("enter tower");
    let (mf, man) = cur_man(&host);
    let d: BTreeSet<String> = scene_destinations(&mf, &man)
        .into_iter()
        .map(|x| x.scene_name)
        .collect();
    assert!(d.contains("teien"), "tower must reach teien; got {d:?}");
}

/// Walk the whole Sebucus arc through the real hub, latching each progression
/// flag by executing its beat, and prove downstream gates open only after
/// their predecessors run - across real transitions that keep the flag banks.
#[test]
fn sebucus_hub_sweep_advances_the_gate_spine() {
    let Some(mut host) = open_host() else { return };

    // Enter the hub and route into the dungeon sub-area: map02 -> geremi ->
    // tower -> teien.
    host.enter_field_scene("map02", 0).expect("enter hub");
    hop(&mut host, "geremi");
    hop(&mut host, "tower");

    // Before the arc runs, tower's own beat is blocked (needs the teien arc
    // flag 0x1C9, still clear).
    {
        let (tmf, tman) = cur_man(&host);
        let tower2 = partition2_record_gates(&tmf, &tman, 2).expect("tower gates");
        assert!(
            !host.world.p2_record_gates_pass(&tower2.0, &tower2.1),
            "tower beat blocked before the teien arc"
        );
    }

    // Into teien and run its beat chain: P2[1] -> 0x1C8, P2[2] -> 0x1C9,
    // P2[5] -> 0x332.
    hop(&mut host, "teien");
    let (teien_mf, teien) = cur_man(&host);
    play_beat(&mut host, &teien_mf, &teien, 1, 0x1C8);
    play_beat(&mut host, &teien_mf, &teien, 2, 0x1C9);
    play_beat(&mut host, &teien_mf, &teien, 5, 0x332);

    // Hop teien -> tower; now the teien arc (0x1C9) is reached, tower unlocks.
    hop(&mut host, "tower");
    let (tower_mf, tower) = cur_man(&host);
    let tower2 = partition2_record_gates(&tower_mf, &tower, 2).expect("tower gates");
    assert!(
        host.world.p2_record_gates_pass(&tower2.0, &tower2.1),
        "tower beat unlocked once the teien arc ran (0x1C9)"
    );
    play_beat(&mut host, &tower_mf, &tower, 2, 0x1C7);

    // Hop tower -> geremi; the post-tower geremi beat now passes (needs 0x1C7).
    hop(&mut host, "geremi");
    let (geremi_mf, geremi) = cur_man(&host);
    let geremi1 = partition2_record_gates(&geremi_mf, &geremi, 1).expect("geremi gates");
    assert!(
        host.world.p2_record_gates_pass(&geremi1.0, &geremi1.1),
        "geremi post-tower beat unlocked (requires tower-clear 0x1C7)"
    );

    // Return to the hub: every progression flag survived the transitions.
    hop(&mut host, "map02");
    for f in [0x1C8u16, 0x1C9, 0x332, 0x1C7] {
        assert!(
            host.world.system_flag_test(f),
            "flag 0x{f:X} persisted across the hub sweep"
        );
    }

    // Balden leg (independent self-latch): its successor P2[18] is blocked
    // until P2[19] runs and sets 0x5B3.
    hop(&mut host, "balden");
    let (balden_mf, balden) = cur_man(&host);
    let b18 = partition2_record_gates(&balden_mf, &balden, 18).expect("balden P2[18] gates");
    assert!(
        !host.world.p2_record_gates_pass(&b18.0, &b18.1),
        "balden successor blocked before its beat runs"
    );
    play_beat(&mut host, &balden_mf, &balden, 19, 0x5B3);
    assert!(
        host.world.p2_record_gates_pass(&b18.0, &b18.1),
        "balden successor unlocked once P2[19] ran (0x5B3)"
    );
}
