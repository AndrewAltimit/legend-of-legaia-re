//! Disc-gated: the length of `edson`'s aerial shot in the credits.
//!
//! `edson` `P2[0]` opens on an aerial glide (`45 07 FB 00 00 ..` then
//! `45 07 FB 40 01 ..`, 320 frames, at `+0xC9` / `+0xE0`), flies the two
//! Seru across it on long arcs (`C3 0C 0A .. D6 01 ..`, 470 frames;
//! `C3 0D 0A .. 72 01 ..`, 370 frames), waits for the glide (`4C CD` at
//! `+0x110`), then clears both arcs' halt bits (`B2 0C 0A` at `+0x127`,
//! `B2 0D 0A` at `+0x146`) and re-stages the Seru with ops aimed at them
//! while they are still in the air, before cutting to the ground shot
//! (`45 07 FB 00 00 ..` at `+0x1B9`). The busy gate reads the halt bit
//! (`+0x10 & 0x400`, `0x801DE90C..0x801DE944`), not the arc, so retail's
//! aerial shot lasts the glide: the retail credits film shows it for about
//! 300 vsyncs (first aerial frame at film vsync 14430, first ground frame at
//! 14730, sampled every 30). Holding the ops behind the arcs stretched it to
//! the 470-frame arc.
//!
//! REF: FUN_801DE840 (0x801DE8E0..0x801DE944, op 0x32), FUN_801D5D60
//!
//! Skip-passes without disc data (CLAUDE.md convention).

use legaia_engine_scene_host::scene_host::SceneHost;
use std::path::PathBuf;

fn open_host() -> Option<SceneHost> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return SceneHost::open_extracted(&d).ok();
        }
    }
    let disc = std::env::var_os("LEGAIA_DISC_BIN")?;
    SceneHost::open_disc(PathBuf::from(disc)).ok()
}

/// `P2[0]`'s aerial glide op and its ground-shot op (record-relative PCs).
const AERIAL_SHOT_PC: usize = 0xC9;
const GROUND_SHOT_PC: usize = 0x1B9;

#[test]
fn edson_aerial_shot_lasts_the_glide_not_the_arcs() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(mut host) = open_host() else {
        eprintln!("[skip] no extracted/ tree and disc open failed");
        return;
    };
    host.enter_field_scene("edson", 0).expect("enter edson");
    let (mut aerial, mut ground) = (None, None);
    for t in 0..2000u32 {
        let _ = host.tick();
        let pc = host
            .world
            .field_vm
            .helper_contexts
            .iter()
            .map(|c| c.pc)
            .max();
        if let Some(pc) = pc {
            if aerial.is_none() && pc > AERIAL_SHOT_PC {
                aerial = Some(t);
            }
            if ground.is_none() && pc > GROUND_SHOT_PC {
                ground = Some(t);
                break;
            }
        }
    }
    eprintln!("[edson] aerial shot op on tick {aerial:?}, ground shot op on tick {ground:?}");
    let (Some(a), Some(g)) = (aerial, ground) else {
        panic!("P2[0] reaches both shots (aerial {aerial:?}, ground {ground:?})");
    };
    let held = g - a;
    assert!(
        (300..=360).contains(&held),
        "the aerial shot lasts the 320-frame glide (retail film ~300 vsyncs), not the \
         470-frame arc: held {held} ticks"
    );
}
