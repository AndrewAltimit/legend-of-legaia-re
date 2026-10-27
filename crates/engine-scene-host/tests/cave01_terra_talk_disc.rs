//! Disc-gated: Terra's last two cave01 talks (`cave01` `P1[2]`) run to
//! their ends under the field-VM talk runner.
//!
//! - The stage-`0x15C` hand-off (`+0x14A4`): `51 5D`, then the talker's own
//!   budgeted turn `38 04 10` and its `33 0A` verify, her own compass walk,
//!   and the seat `23 47 43` that stands her on `(71, 67)` - where the
//!   stage-`0x15D` entry branch seats her too (`+0x1521`).
//! - The stage-`0x15E` join (`+0x1546`): the turn `38 02 16` / `33 0A`, the
//!   pep talk, `50 13` (Terra rides with the party) and `51 69`.
//!
//! The halt bit a talker's own `38` raises belongs to the walk kernel's
//! RotateToAngle leg, whose terminal frame clears it; with the op run on the
//! stand-in context alone, the `33 0A` behind it ended both talks, and the
//! own `23` seat moved nobody.
//!
//! Skip-passes without disc data / extracted assets (CLAUDE.md convention).

use legaia_engine_scene_host::scene_host::SceneHost;
use std::path::PathBuf;

const TERRA: u8 = 2;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn set_flag(host: &mut SceneHost, f: usize) {
    let flags = &mut host.world.flags.system_flags;
    if flags.len() <= f / 8 {
        flags.resize(f / 8 + 1, 0);
    }
    flags[f / 8] |= 0x80 >> (f % 8);
}

fn flag(host: &SceneHost, f: usize) -> bool {
    host.world
        .flags
        .system_flags
        .get(f / 8)
        .copied()
        .unwrap_or(0)
        & (0x80 >> (f % 8))
        != 0
}

fn tile(p: (i16, i16)) -> (i16, i16) {
    (p.0 >> 7, p.1 >> 7)
}

/// Enter cave01 with `flags` raised, stand the player beside Terra facing
/// her, press Cross and page the talk for `frames` frames.
fn talk_to_terra(flags: &[usize], frames: usize) -> Option<SceneHost> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return None;
    };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    for &f in flags {
        set_flag(&mut host, f);
    }
    host.world.toggles.use_vm_dialogue = true;
    host.enter_field_scene("cave01", 0).expect("enter cave01");
    for _ in 0..90 {
        let _ = host.world.tick();
    }
    let (tx, tz) = *host.world.npcs.positions.get(&TERRA).expect("Terra placed");
    let slot = usize::from(host.world.player_actor_slot.expect("player"));
    let faced = [(128i16, 0i16), (-128, 0), (0, 128), (0, -128)]
        .into_iter()
        .any(|(dx, dz)| {
            (0..0x1000).step_by(0x200).any(|r| {
                let ms = &mut host.world.actors[slot].move_state;
                ms.world_x = tx + dx;
                ms.world_z = tz + dz;
                ms.render_26 = r as _;
                host.world.field_interact_probe_slot() == Some(TERRA)
            })
        });
    assert!(
        faced,
        "a tile beside Terra at {:?} faces her",
        tile((tx, tz))
    );
    host.world.set_pad(0);
    let _ = host.world.tick();
    host.world.set_pad(0x4000);
    let _ = host.world.tick();
    // Page with a press every 20 frames; once the talk ends, stop pressing
    // so it is not started again.
    for i in 0..frames {
        let talking = host.world.dialogue_owns_input();
        host.world
            .set_pad(if talking && i % 20 == 10 { 0x4000 } else { 0 });
        let _ = host.world.tick();
    }
    Some(host)
}

#[test]
fn cave01_terra_stage_15c_hand_off_seats_her_on_her_next_spot() {
    let mut flags: Vec<usize> = (0x158..=0x15C).collect();
    flags.extend([0x166, 0x167, 0x168]);
    let Some(host) = talk_to_terra(&flags, 900) else {
        return;
    };
    assert!(flag(&host, 0x15D), "the hand-off raises 0x15D");
    assert!(!host.world.dialogue_owns_input(), "the talk ends");
    let terra = *host.world.npcs.positions.get(&TERRA).unwrap();
    assert_eq!(tile(terra), (71, 67), "`23 47 43` seats Terra on (71, 67)");
    eprintln!("[ok] cave01 stage 0x15C: 0x15D raised, Terra on (71, 67)");
}

#[test]
fn cave01_terra_stage_15e_talk_joins_her_to_the_party() {
    let flags: Vec<usize> = (0x158..=0x15E).collect();
    let Some(host) = talk_to_terra(&flags, 900) else {
        return;
    };
    assert!(flag(&host, 0x13), "`50 13`: Terra rides with the party");
    assert!(flag(&host, 0x169), "`51 69`: the join is recorded");
    assert!(!host.world.dialogue_owns_input(), "the talk ends");
    eprintln!("[ok] cave01 stage 0x15E: 0x13 and 0x169 raised");
}
