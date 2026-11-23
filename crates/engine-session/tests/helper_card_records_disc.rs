//! Disc-gated: every helper-run record that carries a `CC F8 89` card
//! plays its cards as retail's pager does, on the pad both play hosts feed.
//!
//! The disc's cards outside the opening chain sit in records the field VM
//! spawns with op `0x44`, which the engine runs as helper contexts:
//! `concend` P2[0], `dream` P2[1..4], `jouine` P2[16], `noaru` P2[31],
//! `urudre2` P2[9] and `uru` P2[36] (Tieg, pinned on its own in
//! `uru_tieg_fire_droplet_disc.rs`). A card is `4C 89` on the record's
//! context, the pager's automatic press over a text segment, and the runner
//! parks any engaged context on its text (`FUN_80039B7C`). Before the fix
//! for #653 a helper ran straight past its cards and the last one stayed up
//! as the title-card overlay, which holds the field's pad neutral on both
//! hosts (`BootSession::claim_tick`).
//!
//! Each record is installed as its op-`0x44` spawn installs it and driven
//! through `claim_tick` with a Cross tap every eighth tick. Per record:
//!
//! - every card shows and holds its record for at least its automatic press;
//! - no frame draws a dialog box under a card (retail has one box, and the
//!   card's lines load into it: a `0x24` box before a card gives it its rows);
//! - the record ends (it retires, or its scene change lands);
//! - once it has, no card is left up and the field reads the pad.
//!
//! Seeded from the `PRO-11` card save; skip-passes without
//! `LEGAIA_DISC_BIN`, an extracted tree or the save library.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use legaia_engine_save_state::WorldSaveState;
use legaia_engine_session::native::NativeSession;
use legaia_engine_session::session::{BootConfig, FieldLiveOpts, TickClaims, TickOwner};
use legaia_engine_system::input::PadButton;
use legaia_scene_formats::cutscene_text::NarrationKind;

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

/// One helper record with cards: its scene, its op-`0x44` global index and
/// the story flags its C1 gate needs clear.
struct Beat {
    scene: &'static str,
    global: u8,
    clear: &'static [u16],
}

const BEATS: &[Beat] = &[
    // The Noaru Valley ending's entry script spawns it (`44 09`).
    Beat {
        scene: "concend",
        global: 0x09,
        clear: &[],
    },
    // The inn dreams, spawned by the dispatcher `P2[0]` on their own
    // one-shot flags.
    Beat {
        scene: "dream",
        global: 0x0D,
        clear: &[0x35B],
    },
    Beat {
        scene: "dream",
        global: 0x0E,
        clear: &[0x35C],
    },
    Beat {
        scene: "dream",
        global: 0x0F,
        clear: &[0x35D],
    },
    Beat {
        scene: "dream",
        global: 0x10,
        clear: &[0x35E],
    },
    // Spawned by the entry script (`44 35`).
    Beat {
        scene: "jouine",
        global: 0x35,
        clear: &[],
    },
    // Songi's fall, spawned by the entry script (`44 39`).
    Beat {
        scene: "noaru",
        global: 0x39,
        clear: &[],
    },
    // King Nebular's dream, spawned by the entry script (`44 37`).
    Beat {
        scene: "urudre2",
        global: 0x37,
        clear: &[],
    },
];

#[derive(Debug, Default)]
struct Run {
    /// Frames each card (by op offset) held its record, and its press.
    cards: BTreeMap<usize, (u32, i16)>,
    /// Frames a card was drawn while the record parked on it.
    card_drawn: u32,
    /// Frames a dialog box was drawn under a card (the box the hosts draw,
    /// `World::script_dialog_panel`): retail has one box, and a card is it.
    box_under_card: u32,
    /// How the record ended: `retired` or `scene <name>`.
    ended: Option<String>,
    /// After the end: a card still up, and the field's pad still neutral.
    card_after: bool,
    neutral_after: bool,
}

fn play(s: &mut NativeSession, beat: &Beat) -> Run {
    let man = s
        .host
        .scene
        .as_ref()
        .unwrap()
        .field_man_payload(&s.host.index)
        .expect("MAN payload read")
        .expect("the scene carries a MAN");
    let man_file = legaia_scene_formats::man_section::parse(&man).expect("MAN parses");
    let before = s.host.world.field_vm.helper_contexts.len();
    assert!(
        s.host
            .world
            .install_spawned_helper_record(&man_file, &man, beat.global),
        "{} global {:#04x} installs",
        beat.scene,
        beat.global
    );
    assert_eq!(s.host.world.field_vm.helper_contexts.len(), before + 1);
    let tl = s.host.world.field_vm.helper_contexts.last_mut().unwrap();
    tl.held_from_install = true;
    let body = tl.bytecode.clone();
    let cards: Vec<(usize, i16)> = tl
        .narration_blocks
        .iter()
        .filter(|b| b.kind == NarrationKind::Card)
        .map(|b| {
            let auto = body
                .get(b.op_offset + 3..b.op_offset + 5)
                .map_or(0, |w| i16::from_le_bytes([w[0], w[1]]));
            (b.op_offset, auto)
        })
        .collect();
    assert!(
        !cards.is_empty(),
        "{}: the record carries a card",
        beat.scene
    );
    let mut run = Run::default();
    for &(op, auto) in &cards {
        run.cards.insert(op, (0, auto));
    }
    let scene = beat.scene.to_string();
    let mut held = 0u16;
    let mut after = None::<u32>;
    for t in 0..30_000u32 {
        if after.is_none() {
            let here = s.host.world.active_scene_label.clone();
            match s
                .host
                .world
                .field_vm
                .helper_contexts
                .iter()
                .find(|tl| *tl.bytecode == *body)
            {
                None if here != scene => {
                    run.ended = Some(format!("scene {here}"));
                    after = Some(0);
                }
                None => {
                    run.ended = Some("retired".into());
                    after = Some(0);
                }
                Some(tl) => {
                    if tl.card_pager.is_some() {
                        if let Some((_, (n, _))) = run.cards.range_mut(..=tl.pc).next_back() {
                            *n += 1;
                        }
                        if s.host.world.cutscene.card.is_some() {
                            run.card_drawn += 1;
                        }
                    }
                }
            }
        }
        if s.host.world.cutscene.card.is_some() && s.host.world.script_dialog_panel().is_some() {
            run.box_under_card += 1;
        }
        if let Some(n) = after.as_mut() {
            *n += 1;
            // What follows the record (the inn a dream returns to, a
            // record the scene change's arrival plays) settles first.
            if *n > 600 && s.host.world.field_vm.helper_contexts.is_empty() {
                break;
            }
            if *n > 6000 {
                break;
            }
        }
        let pad = if t % 8 == 0 {
            PadButton::Cross.mask()
        } else {
            0
        };
        let edge = pad & !held;
        held = pad;
        let claims = TickClaims {
            pause_menu_ready: true,
            held: pad,
            ..TickClaims::default()
        };
        let field_pad = match s.claim_tick(claims, edge) {
            TickOwner::Field(false) => pad,
            TickOwner::Field(true) => 0,
            other => panic!("{}: tick {t} went to {other:?}", beat.scene),
        };
        s.host.world.set_pad(field_pad);
        s.tick().expect("tick");
    }
    run.card_after = s.host.world.cutscene.card.is_some();
    run.neutral_after = s.world_holds_field_pad();
    run
}

#[test]
fn helper_record_cards_hold_close_and_hand_the_pad_back() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let lib = std::env::var_os("LEGAIA_SAVES_LIBRARY")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../saves/library"));
    let Some(sc) = card_sc(&lib, "playthrough-ladder-pro00-14.mcr", "PRO-11") else {
        eprintln!("[skip] save library card PRO-11 missing");
        return;
    };
    let mut failures = Vec::new();
    for beat in BEATS {
        let cfg = BootConfig {
            scene: "town01".into(),
            enable_audio: false,
        };
        let mut s = NativeSession::open(&extracted, &cfg).expect("open boot session");
        s.host.world.toggles.use_vm_dialogue = true;
        let mut save =
            legaia_save::SaveFile::from_retail_sc_block(&sc, legaia_save::RETAIL_SC_PARTY_RECORDS)
                .expect("lift SC block");
        for &f in beat.clear {
            save.ext.story_flag_bits[0x158 + (f as usize >> 3)] &= !(0x80 >> (f & 7));
        }
        s.host.world.load_full(save.clone());
        let landing = s.resume_save(save, beat.scene, &FieldLiveOpts::default());
        assert!(
            landing.entered_scene(),
            "resume onto {}: {landing:?}",
            beat.scene
        );
        // Let the entry script run; a record it spawns itself is retired
        // (or plays) before the one under test installs.
        for _ in 0..30 {
            s.host.world.set_pad(0);
            s.tick().expect("tick");
        }
        s.host.world.field_vm.helper_contexts.clear();
        s.host.world.cutscene.card = None;
        let run = play(&mut s, beat);
        eprintln!("{} {:#04x}: {run:?}", beat.scene, beat.global);
        let label = format!("{} {:#04x}", beat.scene, beat.global);
        for (op, (n, auto)) in &run.cards {
            if i32::try_from(*n).unwrap_or(i32::MAX) < i32::from(*auto) {
                failures.push(format!(
                    "{label}: card at {op:#x} held {n} frames, press {auto}"
                ));
            }
        }
        if run.card_drawn == 0 {
            failures.push(format!("{label}: no card drawn"));
        }
        if run.box_under_card > 0 {
            failures.push(format!(
                "{label}: a box was drawn under a card for {} frames",
                run.box_under_card
            ));
        }
        if run.ended.is_none() {
            failures.push(format!("{label}: the record never ended"));
        }
        if run.card_after || run.neutral_after {
            failures.push(format!(
                "{label}: after it, card up {} / pad neutral {}",
                run.card_after, run.neutral_after
            ));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
