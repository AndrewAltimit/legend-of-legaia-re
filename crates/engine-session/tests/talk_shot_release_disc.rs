//! Disc-gated executing census of every placement talk that stages an
//! op-`0x45` shot: run the talk through the inline dialogue runner on a live
//! session, then walk the player, and check the two halves of a scripted
//! shot's life on the port.
//!
//! - **Held.** While a talk shows a shot it staged, the enhanced follow
//!   knobs (the `Far` distance preset here) stay out: the frame the follow
//!   arm resolves with the preset equals the one it resolves at retail's
//!   distance (`World::scripted_shot_holds_camera`, #610 / #615).
//! - **Released.** Once the talk ends, a shot it never handed back (no
//!   APPLY, no `[4C 39]` / `[4C 3E]`) goes back to the follow ease: retail's
//!   ease `FUN_801DB510` tests only the camera switch, the freeze bit and
//!   whether the player moved (`0x801DB550..0x801DB5A4`), so the first walk
//!   eases off the shot. The port's latch used to freeze the follow camera
//!   on it for the rest of the scene.
//!
//! `ropeway` P1[45] reaches the release the same way at present: the ride
//! stages its shot, the runner's spin-park net ends the talk on the
//! gondola's morph-lane wait (`AD 40 0B` at `+0x1F0`) before the ride's
//! `45 C0 00 00`, and the follow camera used to stay on the ride shot.
//!
//! The record list is the partition-1 rows of
//! `engine-core/tests/field_camera_shot_census_disc.rs` (every placement
//! record with a reachable CONFIGURE). A talk whose story gate keeps the
//! shot from running at the default flags stages nothing and passes both
//! halves trivially; the census reports how many staged one.
//!
//! Skips and passes without `LEGAIA_DISC_BIN` / `extracted/`.

use std::path::PathBuf;

use legaia_engine_camera::camera_distance::CameraDistance;
use legaia_engine_session::native::NativeSession;
use legaia_engine_session::session::{BootConfig, FieldLiveOpts};
use legaia_engine_system::input::PadButton;
use legaia_engine_world_views::camera_view::field_follow_view;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// Partition-1 records with a reachable op-`0x45` CONFIGURE.
const TALKS: &[(&str, u8)] = &[
    ("bubu1", 1),
    ("bubu1", 7),
    ("bubu1", 22),
    ("cave01", 2),
    ("conc2", 26),
    ("conc2", 35),
    ("conc2", 36),
    ("conc2", 39),
    ("deroa", 7),
    ("doman", 3),
    ("doman", 15),
    ("doman", 18),
    ("jagaroom", 8),
    ("jouinc", 8),
    ("jouinc", 9),
    ("koin1", 9),
    ("koin1b", 9),
    ("koin2", 19),
    ("koin3", 28),
    ("koin4", 1),
    ("rayman", 44),
    ("rayman", 47),
    ("rayman", 48),
    ("rayman2", 52),
    ("rikuroa", 3),
    ("ropeway", 44),
    ("ropeway", 45),
    ("station3", 8),
    ("suimon", 4),
    ("town01", 32),
    ("town01", 40),
    ("town01", 51),
    ("town0b", 36),
    ("town0b", 55),
    ("town0c", 4),
    ("town0c", 21),
    ("town0c", 24),
    ("tunnelc", 4),
    ("vell", 3),
    ("vozz", 1),
    ("vozz", 7),
];

/// Talks that end on a shot they staged and never released, at the
/// default flags. `koin4` P1[1] glides to its shot (`45 07 7B 28 00 ..`),
/// waits on the glide (`4C CD`), prints its line and ends on `21`;
/// `bubu1` P1[22] stages five shots and no APPLY. Before the release the
/// follow camera stayed frozen on the last of them after the talk.
const UNAPPLIED: &[(&str, u8)] = &[("bubu1", 22), ("koin4", 1)];

#[derive(Debug, Default)]
struct Outcome {
    staged: bool,
    /// Frames a talk-owned shot framed differently under the `Far` preset.
    knob_leaks: u32,
    talk_ended: bool,
    /// The latch survived the talk's end and a walk.
    frozen_after_walk: bool,
    left_scene: bool,
}

fn run_talk(extracted: &std::path::Path, scene: &str, slot: u8) -> Option<Outcome> {
    let cfg = BootConfig {
        scene: scene.into(),
        enable_audio: false,
    };
    let mut s = NativeSession::open(extracted, &cfg).ok()?;
    s.host.world.toggles.use_vm_dialogue = true;
    s.enter_field_live(scene, &FieldLiveOpts::default()).ok()?;
    s.set_host_drains_queues(true);
    for _ in 0..20 {
        s.tick().ok()?;
    }
    let &(nx, nz) = s.host.world.npcs.positions.get(&slot)?;
    let (tx, tz) = ((nx >> 7) as u8, (nz >> 7) as u8);
    s.host.world.seat_player_at_tile(tx, tz.saturating_add(1));
    for _ in 0..4 {
        s.tick().ok()?;
    }
    s.camera.distance = CameraDistance::Far;
    let mut out = Outcome::default();
    s.host.world.trigger_field_interact(0, slot);
    let mut talked = false;
    let label = s.host.world.active_scene_label.clone();
    for t in 0..4000u32 {
        let busy = s.host.world.dialogue_owns_input();
        talked |= busy;
        if s.host.world.active_scene_label != label {
            out.left_scene = true;
            return Some(out);
        }
        if talked && !busy {
            out.talk_ended = true;
            break;
        }
        if busy && s.camera.script_owns_focus() {
            out.staged = true;
            let far = field_follow_view(&s.camera, &s.host.world);
            let mut retail = s.camera.clone();
            retail.distance = CameraDistance::Retail;
            let ret = field_follow_view(&retail, &s.host.world);
            if let (Some(a), Some(b)) = (far, ret)
                && a.eye() != b.eye()
            {
                out.knob_leaks += 1;
            }
        }
        let pad = if busy && t % 8 == 0 {
            PadButton::Cross.mask()
        } else {
            0
        };
        s.host.world.set_pad(pad);
        s.tick().ok()?;
    }
    if !out.talk_ended || !out.staged {
        return Some(out);
    }
    for _ in 0..90 {
        s.host.world.set_pad(PadButton::Down.mask());
        s.tick().ok()?;
    }
    s.host.world.set_pad(0);
    out.frozen_after_walk = s.camera.script_owns_focus()
        && s.camera.mover.is_none()
        && !s.host.world.dialogue_owns_input()
        && !s.host.world.script_context_engages_player();
    Some(out)
}

#[test]
fn talk_shots_hold_and_release() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let mut staged = 0;
    let mut bad = Vec::new();
    let mut unapplied_seen = Vec::new();
    for &(scene, slot) in TALKS {
        let Some(o) = run_talk(&extracted, scene, slot) else {
            eprintln!("TALK {scene} P1[{slot}] - no session / no placement");
            continue;
        };
        eprintln!("TALK {scene} P1[{slot}] {o:?}");
        staged += usize::from(o.staged);
        if UNAPPLIED.contains(&(scene, slot)) && o.staged && o.talk_ended {
            unapplied_seen.push((scene, slot));
        }
        if o.knob_leaks > 0 || o.frozen_after_walk {
            bad.push((scene, slot, o));
        }
    }
    eprintln!("[talks] {} records, {staged} staged a shot", TALKS.len());
    assert_eq!(
        unapplied_seen.len(),
        UNAPPLIED.len(),
        "the talks that end on an unreleased shot must stage it to test anything: {unapplied_seen:?}"
    );
    assert!(bad.is_empty(), "talk shots leaked or froze: {bad:?}");
}
