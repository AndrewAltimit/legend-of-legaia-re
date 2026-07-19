//! Disc-gated: `town01`'s opening record (`P2[3]`) stages the retail arrival
//! camera as three op-`0x45` beats - the ground truth a frame-exact per-frame
//! capture of the retail camera globals pins (see
//! `docs/subsystems/cutscene.md`, the camera mover law):
//!
//! - beat `+0x0091` (`apply 0`) **snaps** the establishing shot: pitch 250,
//!   H 412, eye depth 32100 - lands in a single frame;
//! - beat `+0x00A6` (`apply 460`, **mode 4** = ease-in-out on all slots)
//!   pans the eye X 1735 -> -165;
//! - beat `+0x00C4` (`apply 600`, `op0 0x13` -> **mode 4**) is the arrival
//!   H glide - H 412 -> 512 participating in the glide like every other
//!   slot, easing in-out (NOT mode 2 / ease-out).
//!
//! The test installs the record as the cutscene timeline and executes it by
//! execution (the field VM decodes `curve = op0 >> 2` from the disc bytes),
//! asserting each beat's decoded `(apply, mode)` + key targets from the
//! CameraConfigure event stream. Frame-level curve parity is covered by the
//! env-gated `camera_mover_recomp_oracle` in `legaia-engine-vm`.
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

fn skip_or_host() -> Option<SceneHost> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return None;
    };
    Some(SceneHost::open_extracted(&extracted).expect("open SceneHost"))
}

#[test]
fn town01_arrival_stages_snap_then_two_mode4_glides() {
    let Some(mut host) = skip_or_host() else {
        return;
    };
    host.world.begin_new_game();
    host.enter_field_scene(legaia_asset::new_game::OPENING_SCENE, 0)
        .expect("enter town01");

    let man_bytes = host
        .scene
        .as_ref()
        .expect("town01 scene")
        .field_man_payload(&host.index)
        .expect("man payload result")
        .expect("town01 has a field MAN");
    let man_file = legaia_asset::man_section::parse(&man_bytes).expect("parse town01 MAN");
    assert!(
        host.world
            .install_cutscene_timeline_record(&man_file, &man_bytes, 2, 3, true),
        "town01 P2[3] (the opening record) installs as a cutscene timeline"
    );

    // Execute the record, collecting every Camera Configure the VM emits.
    // The snap + pan run in the record's first slice; the H glide commits
    // after the pan's 460-display-frame wait drains (~770 sim ticks on the
    // 60 Hz sub-clock), so give the drive a comfortable budget.
    /// One decoded Configure beat: `(apply_trigger, mode, [(slot, value)])`.
    type ConfigureBeat = (u16, u8, Vec<(u8, u16)>);
    let mut beats: Vec<ConfigureBeat> = Vec::new();
    for _ in 0..4_000u32 {
        let _ = host.tick();
        for ev in host.world.drain_field_events() {
            if let legaia_engine_core::field_events::FieldEvent::CameraConfigure {
                params,
                apply_trigger,
                mode,
            } = ev
            {
                beats.push((
                    apply_trigger,
                    mode,
                    params.iter().map(|p| (p.slot, p.value)).collect(),
                ));
            }
        }
        if beats.iter().any(|(apply, _, _)| *apply == 600) {
            break;
        }
    }
    let slot =
        |params: &[(u8, u16)], s: u8| params.iter().find(|(ps, _)| *ps == s).map(|&(_, v)| v);

    // Beat +0x0091: the apply-0 establishing snap.
    let snap = beats
        .iter()
        .find(|(apply, _, p)| *apply == 0 && slot(p, 5) == Some(32100))
        .expect("the arrival establishing snap (apply 0, eye depth 32100) fires");
    assert_eq!(slot(&snap.2, 0), Some(250), "snap pitch");
    assert_eq!(slot(&snap.2, 9), Some(412), "snap H");

    // Beat +0x00A6: the apply-460 pan is MODE 4 (ease-in-out on all slots).
    let pan = beats
        .iter()
        .find(|(apply, _, _)| *apply == 460)
        .expect("the arrival pan (apply 460) fires");
    assert_eq!(pan.1, 4, "pan curve mode (op0 0x13 >> 2)");
    assert_eq!(
        slot(&pan.2, 3).map(|v| v as i16),
        Some(-165),
        "pan eye X target"
    );
    assert_eq!(slot(&pan.2, 9), Some(412), "pan holds H 412");

    // Beat +0x00C4: the arrival H glide is MODE 4, apply 600, H 412 -> 512.
    let h_glide = beats
        .iter()
        .find(|(apply, _, _)| *apply == 600)
        .expect("the arrival H glide (apply 600) fires");
    assert_eq!(
        h_glide.1, 4,
        "H-glide curve mode (op0 0x13 >> 2, NOT mode 2)"
    );
    assert_eq!(slot(&h_glide.2, 9), Some(512), "H glide target");
    assert_eq!(slot(&h_glide.2, 0), Some(186), "H-glide pitch target");
    assert_eq!(
        slot(&h_glide.2, 5).map(|v| v as i16),
        Some(-380),
        "H-glide eye depth target"
    );

    // The merged staging ends the H-glide beat holding mode 4 / apply 600.
    assert_eq!(host.world.camera_state.mode, 4, "staged curve mode");
    assert_eq!(host.world.camera_state.apply_trigger, 600, "staged apply");
}
