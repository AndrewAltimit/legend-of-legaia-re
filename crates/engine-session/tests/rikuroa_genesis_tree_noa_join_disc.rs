//! Disc-gated: the Genesis Tree beat atop Mt. Rikuroa, Terra joining Vahn
//! and Noa asking to come along (`rikuroa` `P2[54]`).
//!
//! The scene's entry script spawns the record with `44 60` once `0x2C9` is
//! up, so it runs as a helper context. Two retail rules decide how it plays:
//!
//! - Terra's speech is a narration crawl opened on actor `0x05`
//!   (`CC 05 80 0B`). The crawl op spawns its roller from whichever context
//!   runs it (`0x801E1ECC..0x801E1FBC`), so the helper opens it as a modal
//!   timeline would, and the record waits on `B3 05 0A` until it retires.
//! - An executed `0x21` ends the record (`FUN_80039B7C` clears the engaged
//!   bit at `0x80039E68..0x80039E7C`, and the context's tick `FUN_8003BC08`
//!   no longer steps it). The Yes arm plays the join and stops on one; the
//!   bytes after it are the No arm's brooding and second ask, reached only
//!   by the first picker's No jump, which asks again until the answer is Yes.
//!
//! The record is seeded from the `PRO-00` card save (Mt. Rikuroa, before
//! Caruban) with the four story flags the beats before it raise: `0x142`
//! (Caruban), `0x28A` and `0x2C2` (the tree revived), `0x2C9` (the return
//! that spawns the record). Skip-passes without `LEGAIA_DISC_BIN`, an
//! extracted tree or the save library.

use std::path::{Path, PathBuf};

use legaia_engine_save_state::WorldSaveState;
use legaia_engine_session::native::NativeSession;
use legaia_engine_session::session::{BootConfig, FieldLiveOpts};
use legaia_engine_system::input::PadButton;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

/// The SC block of the save named `save` on the library card `card`.
fn card_sc(lib: &Path, card: &str, save: &str) -> Option<Vec<u8>> {
    let mounted = legaia_save::emu::MountedCard::open(&lib.join("cards").join(card)).ok()?;
    (1..=15u8).find_map(|block| {
        let frame = mounted.dir_frame(block)?;
        let name: String = frame[0x0A..0x0A + 20]
            .iter()
            .take_while(|&&b| b != 0)
            .map(|&b| b as char)
            .collect();
        if !name.ends_with(save) {
            return None;
        }
        mounted.sc_block(block).map(<[u8]>::to_vec)
    })
}

// Record body offsets (`P2[54]`, measured from the record start).
/// Terra's crawl: `CC 05 80 0B`.
const CRAWL_OP: usize = 0x2B8;
/// The join both answers reach: Noa's "Good." line follows its BGM op.
const JOIN: usize = 0x8FE;
/// The `0x21` the join stops on.
const JOIN_STOP: usize = 0xAB9;
/// The third ask, past the join's stop: only a second No reaches it.
const THIRD_ASK: usize = 0xC07;

/// What one play of the record did.
#[derive(Debug, Default)]
struct Run {
    crawl_ticks: u32,
    asks: usize,
    visited_join: bool,
    past_join_stop: bool,
    ended_at: Option<usize>,
}

/// Play `P2[54]`, answering each ask with the next of `answers` (`true` =
/// Yes), and record what the record ran.
fn play(answers: &[bool]) -> Option<Run> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return None;
    };
    let lib = std::env::var_os("LEGAIA_SAVES_LIBRARY")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../saves/library"));
    let Some(sc) = card_sc(&lib, "playthrough-ladder-pro00-14.mcr", "PRO-00") else {
        eprintln!("[skip] save library card PRO-00 missing");
        return None;
    };
    let cfg = BootConfig {
        scene: "town01".into(),
        enable_audio: false,
    };
    let mut s = NativeSession::open(&extracted, &cfg).expect("open boot session");
    s.host.world.toggles.use_vm_dialogue = true;
    let mut save =
        legaia_save::SaveFile::from_retail_sc_block(&sc, legaia_save::RETAIL_SC_PARTY_RECORDS)
            .expect("lift SC block");
    for f in [0x142u16, 0x28A, 0x2C2, 0x2C9] {
        save.ext.story_flag_bits[0x158 + (f as usize >> 3)] |= 0x80 >> (f & 7);
    }
    s.host.world.load_full(save.clone());
    let landing = s.resume_save(save, "rikuroa", &FieldLiveOpts::default());
    assert!(landing.entered_scene(), "resume onto rikuroa: {landing:?}");

    // The record is the helper whose body opens Terra's crawl there.
    let is_record = |b: &[u8]| b.get(CRAWL_OP..CRAWL_OP + 3) == Some(&[0xCC, 0x05, 0x80][..]);
    let mut run = Run::default();
    let mut answers = answers.iter().copied();
    let mut answer = None;
    let mut seen = false;
    for t in 0..20_000u32 {
        let tl = s
            .host
            .world
            .field_vm
            .helper_contexts
            .iter()
            .find(|tl| is_record(&tl.bytecode));
        let Some(tl) = tl else {
            if seen {
                break;
            }
            s.host.world.set_pad(0);
            s.tick().expect("tick");
            continue;
        };
        seen = true;
        let pc = tl.pc;
        run.ended_at = Some(pc);
        if (JOIN..JOIN + 0x20).contains(&pc) {
            run.visited_join = true;
        }
        if pc > JOIN_STOP && pc < THIRD_ASK && run.visited_join {
            run.past_join_stop = true;
        }
        if s.host.world.cutscene.narration.is_some() {
            run.crawl_ticks += 1;
        }
        let menu = tl.dialog.as_ref().filter(|d| d.picker_takes_input());
        let pad = match menu {
            Some(d) => {
                let yes = *answer.get_or_insert_with(|| {
                    run.asks += 1;
                    answers.next().unwrap_or(true)
                });
                match (yes, d.picker_cursor(), t % 4) {
                    (false, 0, 0) => PadButton::Down.mask(),
                    (_, c, 2) if c == usize::from(!yes) => PadButton::Cross.mask(),
                    _ => 0,
                }
            }
            None => {
                answer = None;
                if t % 8 == 0 {
                    PadButton::Cross.mask()
                } else {
                    0
                }
            }
        };
        s.host.world.set_pad(pad);
        s.tick().expect("tick");
    }
    assert!(seen, "the entry script spawns P2[54] on 0x2C9");
    Some(run)
}

#[test]
fn terra_crawl_draws_and_yes_ends_on_the_join() {
    let Some(run) = play(&[true]) else {
        return;
    };
    eprintln!("{run:?}");
    assert!(
        run.crawl_ticks > 600,
        "Terra's crawl scrolls on the helper context ({} ticks)",
        run.crawl_ticks
    );
    assert_eq!(run.asks, 1, "Noa asks once");
    assert!(run.visited_join, "Yes plays the join");
    assert!(
        !run.past_join_stop,
        "the join stops on its 0x21; the No arm after it does not play"
    );
}

#[test]
fn no_asks_again_until_yes() {
    let Some(run) = play(&[false, false, true]) else {
        return;
    };
    eprintln!("{run:?}");
    assert_eq!(run.asks, 3, "No, No, then Yes");
    assert!(run.visited_join, "the final Yes plays the join");
    assert!(
        run.ended_at
            .is_some_and(|p| (JOIN..=JOIN_STOP + 1).contains(&p)),
        "the record ends on the join's stop: {:?}",
        run.ended_at
    );
}
