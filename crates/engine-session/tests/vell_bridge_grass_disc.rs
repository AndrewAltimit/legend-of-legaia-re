//! `vell`'s Bridge Grass (MAN partition-0 record 1, the object on tile
//! `(16, 37)`): with the Fertilizer offered (`0x078`) and the first-visit beat
//! seen (`0x460`), examining it asks "Do you wish to use the Fertilizer?".
//! Yes sets `0x2AE`, takes the item, re-arms the object's VDF morph
//! (`4B 07 00 ..`), drops the pulse's recycle and hold bits (`AC 01 0C`,
//! `AC 01 0A`), primes the envelope (`AB 01 0D`) and spins on its ADVANCE
//! latch (`AC 01 0B` / `AD 01 0B`) while the grass grows across the river.
//!
//! The spin reads the record's own context and the envelope ran on the
//! object's channel, so the latch never reached the spin: the examine parked
//! on `AD 01 0B` for good and the player stood frozen at the riverbank.
//!
//! The grass is two models. The bind prologue swaps the object to the sprout
//! (`4C 50 29 00`, model `41`) and sways it with lanes `7..=10`; the Yes
//! branch swaps it to the bridge (`4C 50 28 00`, model `40`, its `.MAP`
//! slot) before the growth lanes `0..=6`, which the envelope primes to full
//! and drains one by one. Run on the record's own context, neither op
//! reached the object, and the sprout stayed where the bridge should have
//! grown. `vell_dead_genesis_tree` (a retail state walked on from the beat)
//! holds the object at `+0x64 = 45` (model `40` over the bank base `5`),
//! seven lanes all at `0` and `+0x62 = 0x4815`.
//!
//! The first examine's cutscene turns Vahn toward the grass
//! (`CC F8 85 14 00 01`) and later toward Noa (`CC F8 85 1A 00 09`); the
//! growth shot turns him to compass `4` (`B8 F8 84 34`). The prop runner
//! stepped all three on a stand-in context, and Vahn kept the heading the
//! walk-in left him with.
//!
//! Disc-gated: skips without `LEGAIA_DISC_BIN` or an extracted tree.

use std::path::PathBuf;

use legaia_engine_session::native::NativeSession;
use legaia_engine_session::session::{BootConfig, FieldLiveOpts};
use legaia_engine_system::input::PadButton;

const GRASS: (u8, u8) = (16, 37);

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

#[test]
fn the_fertilizer_grows_the_grass_and_hands_the_player_back() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let cfg = BootConfig {
        scene: "town01".into(),
        enable_audio: false,
    };
    let mut s = NativeSession::open(&extracted, &cfg).expect("open boot session");
    let save = legaia_save::SaveFile {
        party: legaia_save::Party::zeroed(1),
        ..Default::default()
    };
    s.resume_save(save, "vell", &FieldLiveOpts::default());
    for f in [0x460, 0x078] {
        s.host.world.system_flag_set(f);
    }
    for _ in 0..60 {
        s.tick().expect("tick");
    }
    assert!(
        s.host.world.props.bank.props.contains_key(&GRASS),
        "the Bridge Grass is a banked prop"
    );
    assert!(
        s.host.world.start_prop_interaction(GRASS),
        "the grass's record starts"
    );
    // Page the text and take the picker's first row (Yes).
    let mut used_at = None;
    let mut done_at = None;
    for t in 0..3000u32 {
        let pad = if t % 2 == 0 {
            PadButton::Cross.mask()
        } else {
            0
        };
        s.host.world.set_pad(pad);
        s.tick().expect("tick");
        if used_at.is_none() && s.host.world.system_flag_test(0x2AE) {
            used_at = Some(t);
        }
        if used_at.is_some() && s.host.world.dialog.inline.is_none() {
            done_at = Some(t);
            break;
        }
    }
    let used = used_at.expect("Yes on the Fertilizer picker sets 0x2AE");
    let done = done_at.unwrap_or_else(|| {
        let pc = s.host.world.dialog.inline.as_ref().map(|id| id.pc);
        panic!("the grass record never handed the player back (parked at {pc:x?})")
    });
    // The lanes unwind over a few hundred frames: the hand-back waits on
    // them rather than stepping past the spin.
    assert!(
        done > used + 60,
        "the record waited on the grass's envelope (used {used}, done {done})"
    );
}

fn open_vell(flags: &[u16]) -> Option<NativeSession> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return None;
    };
    let cfg = BootConfig {
        scene: "town01".into(),
        enable_audio: false,
    };
    let mut s = NativeSession::open(&extracted, &cfg).expect("open boot session");
    let save = legaia_save::SaveFile {
        party: legaia_save::Party::zeroed(1),
        ..Default::default()
    };
    s.resume_save(save, "vell", &FieldLiveOpts::default());
    for &f in flags {
        s.host.world.system_flag_set(f);
    }
    for _ in 0..60 {
        s.tick().expect("tick");
    }
    // Beside the grass, where the examine's own walk-in starts.
    s.host.world.seat_player_at_tile(17, 38);
    for _ in 0..10 {
        s.tick().expect("tick");
    }
    Some(s)
}

/// The player's position and heading (engine convention, `0` = `+Z`).
fn player(s: &NativeSession) -> (i16, i16, i16) {
    let w = &s.host.world;
    let a = w
        .player_actor_slot
        .and_then(|p| w.actors.get(usize::from(p)))
        .expect("player actor");
    (
        a.move_state.world_x,
        a.move_state.world_z,
        a.move_state.render_26,
    )
}

/// The engine heading from `from` toward `to` (`0` = `+Z`, `0x400` = `+X`).
fn bearing(from: (i16, i16), to: (i32, i32)) -> i32 {
    let (dx, dz) = (
        f64::from(to.0) - f64::from(from.0),
        f64::from(to.1) - f64::from(from.1),
    );
    (dx.atan2(dz).to_degrees() * 4096.0 / 360.0).rem_euclid(4096.0) as i32
}

fn arc(a: i32, b: i32) -> i32 {
    let d = (a - b).rem_euclid(4096);
    d.min(4096 - d)
}

#[test]
fn the_fertilizer_swaps_in_the_bridge_and_grows_it() {
    use legaia_engine_core::world::MorphOwner;
    let Some(mut s) = open_vell(&[0x460, 0x078]) else {
        return;
    };
    assert_eq!(
        s.host.world.object_live_models().get(&1),
        Some(&41),
        "the bind prologue draws the sprout"
    );
    assert!(s.host.world.start_prop_interaction(GRASS));
    let mut grew = false;
    let mut swapped_dirty = false;
    for t in 0..3000u32 {
        s.host.world.set_pad(if t % 2 == 0 {
            PadButton::Cross.mask()
        } else {
            0
        });
        s.tick().expect("tick");
        let dirty = s.host.world.take_morph_dirty_slots();
        if s.host.world.object_live_models().get(&1) == Some(&40) {
            swapped_dirty |= dirty.iter().any(|&(slot, _)| slot == 40);
            let lanes = s
                .host
                .world
                .field_morph_lanes(MorphOwner::Object(1))
                .unwrap_or_default();
            // The growth lanes, primed to full and draining.
            grew |= lanes.len() == 7
                && lanes.iter().map(|&(i, _)| i).eq(0..7)
                && lanes.iter().any(|&(_, w)| w == 0x1000);
        }
        if s.host.world.dialog.inline.is_none() && s.host.world.system_flag_test(0x2AE) {
            break;
        }
    }
    assert!(s.host.world.system_flag_test(0x2AE), "Yes was taken");
    assert_eq!(
        s.host.world.object_live_models().get(&1),
        Some(&40),
        "the grown grass draws the bridge model (retail +0x64 = 45)"
    );
    assert!(grew, "the growth lanes 0..=6 ran on the object");
    assert!(swapped_dirty, "the bridge mesh re-staged under the lanes");
    let lanes = s
        .host
        .world
        .field_morph_lanes(MorphOwner::Object(1))
        .expect("lanes");
    assert!(
        lanes.iter().all(|&(_, w)| w == 0),
        "grown, every lane at 0 as in the retail state: {lanes:?}"
    );
}

#[test]
fn the_first_examine_turns_vahn_toward_the_grass_then_noa() {
    let Some(mut s) = open_vell(&[0x078]) else {
        return;
    };
    let grass = s
        .host
        .world
        .props
        .bank
        .props
        .get(&GRASS)
        .map(|p| p.world)
        .expect("the grass prop");
    assert!(s.host.world.start_prop_interaction(GRASS));
    // The acquire on the grass sits at record offset 0xB1, the one on Noa at
    // 0x474; sample the heading the record holds just after each.
    let mut at_grass = None;
    let mut at_noa = None;
    for t in 0..4000u32 {
        s.host.world.set_pad(if t % 2 == 0 {
            PadButton::Cross.mask()
        } else {
            0
        });
        s.tick().expect("tick");
        let Some(pc) = s.host.world.dialog.inline.as_ref().map(|i| i.pc) else {
            break;
        };
        if at_grass.is_none() && (0x0E0..0x150).contains(&pc) {
            at_grass = Some(player(&s));
        }
        if at_noa.is_none() && (0x4EA..0x600).contains(&pc) {
            at_noa = Some(player(&s));
        }
    }
    let (x, z, h) = at_grass.expect("the record reached the grass shot");
    let want = bearing((x, z), grass);
    assert!(
        arc(i32::from(h), want) < 0x40,
        "Vahn faces the grass: heading {h:#x}, bearing {want:#x}"
    );
    let (_, _, h2) = at_noa.expect("the record reached Noa's line");
    assert!(
        arc(i32::from(h2), i32::from(h)) > 0x100,
        "Vahn turns again toward Noa: {h:#x} -> {h2:#x}"
    );
}
