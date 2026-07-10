//! Disc-gated runtime oracle: the Drake-castle **door-choreography record
//! families** run organically through the engine's gated partition-2 record
//! machinery - the `0x00F` busy-mutex family (jouinc `P2[2..=59]`, door ids
//! J01..J58) and the jouind per-visit door band `0x4BE..=0x4C2`.
//!
//! Decode (see `docs/subsystems/script-vm.md`, door-choreography families):
//!
//! - Every jouinc door record is gated `C1=[0x00F]` and brackets its body
//!   with `50 0F` (first op, mutex acquire) .. `60 0F` (mutex release) before
//!   parking on a `JmpRel -2`. The C1 polarity makes `0x00F` a
//!   mutual-exclusion lock: no door record spawns while another is
//!   mid-flight, and a running record cannot re-trigger itself. The body is
//!   walk-through choreography (door-actor Animate + player `ExecMove`
//!   sequence) around `36 00 80 xx` / `36 04 80 00` SceneFade pairs - an
//!   intra-scene reposition, not a `0x3F` scene change.
//! - jouind `P2[10..=13]` are gated on the per-visit band: `P2[10]`/`P2[11]`
//!   (`C1=[0x4C1]`) SET `0x4BE`/`0x4BF` and share the first-use latch
//!   `0x4C2`; `P2[14]` SETs `0x4C1`, retiring the family for the visit; and
//!   `jouina P1[0]` (the castle entry script) CLEARs all five flags - so the
//!   band is per-castle-visit door/lift state, reset on every entry.
//!
//! The oracle proves the engine runs the whole lifecycle from disc bytes:
//! mutex acquire -> exclusion -> choreography -> release (a force-capped or
//! stalled record would leave `0x00F` latched and every later door dead),
//! across ALL 58 jouinc doors, plus the jouind band's latch/retire/reset
//! cycle with `jouina P1[0]`'s clears landing by entry-script execution.
//!
//! Skip-passes without disc data / extracted assets (CLAUDE.md convention).

use legaia_engine_core::scene::SceneHost;
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

fn gated() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return None;
    };
    Some(extracted)
}

/// The transient door busy-mutex flag (the `0xB`/`0xC`/`0x18` interaction-lock
/// band).
const DOOR_MUTEX_FLAG: u16 = 0x00F;
/// jouinc's door-choreography records: `P2[2..=59]` = door ids J01..J58.
const JOUINC_DOOR_RECORDS: std::ops::RangeInclusive<usize> = 2..=59;
/// The jouind per-visit door/lift state band, reset by `jouina P1[0]`.
const JOUIND_VISIT_BAND: [u16; 5] = [0x4BE, 0x4BF, 0x4C0, 0x4C1, 0x4C2];

fn man_of(host: &SceneHost) -> (Vec<u8>, legaia_asset::man_section::ManFile) {
    let man_bytes = host
        .scene
        .as_ref()
        .unwrap()
        .field_man_payload(&host.index)
        .expect("MAN payload read")
        .expect("scene resolves its bundle MAN");
    let man_file = legaia_asset::man_section::parse(&man_bytes).expect("MAN parses");
    (man_bytes, man_file)
}

/// Static disc census: all 58 jouinc door records carry the busy-mutex shape -
/// `C1=[0x00F]`, first op `50 0F` (acquire), a `60 0F` release in the body,
/// and at least one SceneFade pair (the intra-scene reposition).
#[test]
fn jouinc_door_records_carry_the_busy_mutex_shape() {
    let Some(extracted) = gated() else { return };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.enter_field_scene("jouinc", 0).expect("enter jouinc");
    let (man_bytes, man_file) = man_of(&host);
    for record in JOUINC_DOOR_RECORDS {
        let (c1, c2) = legaia_engine_core::man_field_scripts::partition2_record_gates(
            &man_file, &man_bytes, record,
        )
        .unwrap_or_else(|| panic!("P2[{record}] gates decode"));
        assert_eq!(c1, vec![DOOR_MUTEX_FLAG], "P2[{record}] C1 is the mutex");
        assert!(c2.is_empty(), "P2[{record}] has no C2 gate");
        let (start, pc0, len) = legaia_engine_core::man_field_scripts::partition_record_span(
            &man_file, &man_bytes, 2, record,
        )
        .unwrap_or_else(|| panic!("P2[{record}] span resolves"));
        let body = &man_bytes[start..start + len];
        assert_eq!(
            &body[pc0..pc0 + 2],
            &[0x50, 0x0F],
            "P2[{record}] first op acquires the mutex"
        );
        let releases = body.windows(2).filter(|w| *w == [0x60, 0x0F]).count();
        assert!(releases >= 1, "P2[{record}] releases the mutex in-body");
        let fades = body.windows(3).filter(|w| *w == [0x36, 0x00, 0x80]).count();
        assert!(
            fades >= 1,
            "P2[{record}] carries a SceneFade intra-scene reposition"
        );
    }
    eprintln!("[jouinc] all 58 door records carry acquire/release + SceneFade");
}

/// Runtime breadth: every jouinc door record runs to organic completion
/// through the gated record machinery - the mutex is held while the
/// choreography plays (a competing door install refuses), and the record's
/// own trailing `60 0F` releases it (a force-capped/stalled record would
/// leave it latched and fail the release assert).
#[test]
fn jouinc_doors_hold_and_release_the_busy_mutex_organically() {
    let Some(extracted) = gated() else { return };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.enter_field_scene("jouinc", 0).expect("enter jouinc");
    let (man_bytes, man_file) = man_of(&host);

    for record in JOUINC_DOOR_RECORDS {
        assert!(
            !host.world.system_flag_test(DOOR_MUTEX_FLAG),
            "mutex clear before P2[{record}] spawns"
        );
        assert!(
            host.world
                .install_gated_p2_record(&man_file, &man_bytes, record),
            "the gated dispatch admits P2[{record}] with the mutex clear"
        );
        // First slice: the record's own `50 0F` acquires the mutex.
        host.tick().expect("tick");
        assert!(
            host.world.system_flag_test(DOOR_MUTEX_FLAG),
            "P2[{record}]'s `50 0F` holds the mutex while in flight"
        );
        // Exclusion: any competing door record refuses through its C1 gate.
        let competitor = if record == 2 { 3 } else { 2 };
        assert!(
            !host
                .world
                .install_gated_p2_record(&man_file, &man_bytes, competitor),
            "P2[{competitor}] refuses while P2[{record}] holds the mutex"
        );
        // Run the choreography out. Completion is organic: the record's
        // trailing `60 0F` must land before its park-loop wraps.
        let mut ticks = 0u32;
        while host.world.cutscene_timeline_active() && ticks < 4000 {
            host.tick().expect("tick");
            ticks += 1;
        }
        assert!(
            !host.world.cutscene_timeline_active(),
            "P2[{record}] completes within {ticks} ticks"
        );
        assert!(
            !host.world.system_flag_test(DOOR_MUTEX_FLAG),
            "P2[{record}]'s `60 0F` released the mutex (not force-capped)"
        );
    }
    eprintln!("[jouinc] all 58 doors held + released the mutex organically");
}

/// The jouind per-visit band lifecycle: `jouina P1[0]`'s entry script clears
/// the band by execution, the door record latches its state + first-use flag
/// from its own bytes, `P2[14]` retires the family, and a fresh `jouina`
/// entry resets it - the full per-castle-visit cycle.
#[test]
fn jouind_visit_band_latches_retires_and_resets_by_script_execution() {
    let Some(extracted) = gated() else { return };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");

    // Seed the whole band SET (the state a previous castle visit leaves).
    for f in JOUIND_VISIT_BAND {
        host.world.system_flag_set(f);
    }
    // jouina's entry script P1[0] clears all five - organically, by the
    // field VM executing the scene-entry script (`64 BE` .. `64 C2`).
    host.enter_field_scene("jouina", 0).expect("enter jouina");
    let mut ticks = 0u32;
    while JOUIND_VISIT_BAND
        .iter()
        .any(|&f| host.world.system_flag_test(f))
        && ticks < 2000
    {
        host.tick().expect("tick");
        ticks += 1;
    }
    for f in JOUIND_VISIT_BAND {
        assert!(
            !host.world.system_flag_test(f),
            "jouina P1[0] cleared {f:#x} by entry-script execution (waited {ticks} ticks)"
        );
    }
    eprintln!("[jouina] entry script cleared the visit band at tick {ticks}");

    // Into jouind: door record P2[10] admits while 0x4C1 is clear, and its
    // own bytes SET the door state 0x4BE + the first-use latch 0x4C2.
    host.enter_field_scene("jouind", 0).expect("enter jouind");
    let (man_bytes, man_file) = man_of(&host);
    assert!(
        host.world
            .install_gated_p2_record(&man_file, &man_bytes, 10),
        "P2[10] admits while 0x4C1 is clear"
    );
    let mut ticks = 0u32;
    while host.world.cutscene_timeline_active() && ticks < 4000 {
        host.tick().expect("tick");
        ticks += 1;
    }
    assert!(
        host.world.system_flag_test(0x4BE),
        "P2[10]'s `54 BE` latched the door-open state"
    );
    assert!(
        host.world.system_flag_test(0x4C2),
        "P2[10]'s `54 C2` latched the first-use flag"
    );

    // P2[14] retires the family for the visit (`54 C1`).
    assert!(
        host.world
            .install_gated_p2_record(&man_file, &man_bytes, 14),
        "P2[14] admits (no gate)"
    );
    let mut ticks = 0u32;
    while host.world.cutscene_timeline_active() && ticks < 4000 {
        host.tick().expect("tick");
        ticks += 1;
    }
    assert!(
        host.world.system_flag_test(0x4C1),
        "P2[14]'s `54 C1` retired the family (waited {ticks} ticks)"
    );
    for record in [10usize, 11, 12] {
        assert!(
            !host
                .world
                .install_gated_p2_record(&man_file, &man_bytes, record),
            "P2[{record}] refuses once 0x4C1 is set"
        );
    }

    // A fresh castle entry resets the band: jouina P1[0]'s clears land again,
    // and the door family is live for the new visit.
    host.enter_field_scene("jouina", 0)
        .expect("re-enter jouina");
    let mut ticks = 0u32;
    while host.world.system_flag_test(0x4C1) && ticks < 2000 {
        host.tick().expect("tick");
        ticks += 1;
    }
    assert!(
        !host.world.system_flag_test(0x4C1),
        "the re-entry cleared 0x4C1 (per-visit reset)"
    );
    host.enter_field_scene("jouind", 0)
        .expect("re-enter jouind");
    let (man_bytes, man_file) = man_of(&host);
    assert!(
        host.world
            .install_gated_p2_record(&man_file, &man_bytes, 10),
        "P2[10] is live again on the fresh visit"
    );
    eprintln!("[jouind] visit band latched, retired, and reset by script execution");
}
