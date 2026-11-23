//! Disc-gated: Tieg's Fire Droplet beat in Uru Mais (`uru` `P2[36]`).
//!
//! Tieg's talk (`P1[7]`) spawns the record with `44 54` once the three
//! dreams are behind the party (`0x39C`, the record's C2 gate), so it runs as
//! a helper context. Tieg's parting words are five `CC F8 89` cards - `4C 89`
//! on the record's context, the pager's automatic press - and the box after
//! them is Gala's waking line, which the player turns.
//!
//! Retail's runner parks any engaged context on its text (`FUN_80039B7C`),
//! so each card holds the record until its automatic press lands, and a card
//! whose page ends on anything but a `0x24` continuation closes on the press.
//! The port used to park only the modal timeline on a card: the helper ran
//! straight past all five, the last one's text stayed up as the title-card
//! overlay, and the play hosts (`BootSession::claim_tick`) handed the field a
//! neutral pad for as long as a card was up - so Gala's box waited for a
//! press no button could deliver.
//!
//! The run is driven through `claim_tick`, as the play window and the browser
//! page drive it. It is seeded from the `PRO-11` card save (before the Usha
//! chain) with the chain's flags up to the third waking (`0x39C`). Skip-passes
//! without `LEGAIA_DISC_BIN`, an extracted tree or the save library.

use std::path::{Path, PathBuf};

use legaia_engine_save_state::WorldSaveState;
use legaia_engine_session::native::NativeSession;
use legaia_engine_session::session::{BootConfig, FieldLiveOpts, TickClaims, TickOwner};
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

/// `44 54`: the global record index of `P2[36]` (partition counts 21, 27).
const TIEG_RECORD_GLOBAL: u8 = 0x54;
// Record body offsets (`P2[36]`, measured from the record start).
/// Tieg's five cards: `CC F8 89 B4 00`, a 180-frame automatic press each.
const CARDS: [usize; 5] = [0xA0E, 0xAB9, 0xAEE, 0xB3A, 0xB7D];
/// Gala's waking line, the box after the cards.
const WAKING_LINE: usize = 0xBF1;
/// `39 BC`: the Fire Droplet.
const GIVE_ITEM: usize = 0xDD0;
/// `0x36F`, which the record raises on its way out.
const DROPLET_FLAG: u16 = 0x36F;

#[derive(Debug, Default)]
struct Run {
    /// Frames the record's PC sat on each card's text.
    card_frames: [u32; 5],
    /// Frames Gala's box was up with a card still drawn.
    waking_under_card: u32,
    /// Frames Gala's box was up on a neutral field pad.
    waking_neutral: u32,
    reached_give_item: bool,
    retired: bool,
}

fn play() -> Option<(Run, bool)> {
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
    let Some(sc) = card_sc(&lib, "playthrough-ladder-pro00-14.mcr", "PRO-11") else {
        eprintln!("[skip] save library card PRO-11 missing");
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
    // The Usha chain up to the third waking: the password and lift, Usha's
    // talk and quiz, the statues, the three dreams.
    for f in [
        0x36Du16, 0x36E, 0x371, 0x379, 0x4FF, 0x3FC, 0x39A, 0x39B, 0x39C,
    ] {
        save.ext.story_flag_bits[0x158 + (f as usize >> 3)] |= 0x80 >> (f & 7);
    }
    s.host.world.load_full(save.clone());
    let landing = s.resume_save(save, "uru", &FieldLiveOpts::default());
    assert!(landing.entered_scene(), "resume onto uru: {landing:?}");
    for _ in 0..120 {
        s.host.world.set_pad(0);
        s.tick().expect("tick");
    }
    // Tieg's talk spawns the record; install it as that spawn does.
    let man = s
        .host
        .scene
        .as_ref()
        .unwrap()
        .field_man_payload(&s.host.index)
        .expect("MAN payload read")
        .expect("uru carries a MAN");
    let man_file = legaia_scene_formats::man_section::parse(&man).expect("MAN parses");
    assert!(
        s.host
            .world
            .install_spawned_helper_record(&man_file, &man, TIEG_RECORD_GLOBAL),
        "P2[36] installs once 0x39C is up"
    );
    if let Some(tl) = s.host.world.field_vm.helper_contexts.last_mut() {
        tl.held_from_install = true;
    }
    let is_record = |b: &[u8]| b.get(GIVE_ITEM..GIVE_ITEM + 2) == Some(&[0x39, 0xBC][..]);
    assert!(
        s.host
            .world
            .field_vm
            .helper_contexts
            .iter()
            .any(|tl| is_record(&tl.bytecode)),
        "the installed helper is P2[36]"
    );

    let mut run = Run::default();
    let mut held = 0u16;
    for t in 0..12_000u32 {
        let Some(tl) = s
            .host
            .world
            .field_vm
            .helper_contexts
            .iter()
            .find(|tl| is_record(&tl.bytecode))
        else {
            run.retired = true;
            break;
        };
        let pc = tl.pc;
        for (i, &op) in CARDS.iter().enumerate() {
            // The card's lines start past its 5-byte op.
            if pc == op + 5 && tl.card_pager.is_some() {
                run.card_frames[i] += 1;
            }
        }
        if pc >= GIVE_ITEM {
            run.reached_give_item = true;
        }
        let waking = pc == WAKING_LINE && tl.dialog.is_some();
        if waking && s.host.world.cutscene.card.is_some() {
            run.waking_under_card += 1;
        }
        // The player taps Cross every eighth tick, as the hosts read it.
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
            TickOwner::Field(true) => {
                if waking {
                    run.waking_neutral += 1;
                }
                0
            }
            other => panic!("tick {t} went to {other:?}"),
        };
        s.host.world.set_pad(field_pad);
        s.tick().expect("tick");
    }
    let flag = s.host.world.system_flag_test(DROPLET_FLAG);
    Some((run, flag))
}

#[test]
fn tieg_cards_hold_the_record_and_gala_wakes_on_a_press() {
    let Some((run, flag)) = play() else {
        return;
    };
    eprintln!("{run:?} 0x36F={flag}");
    for (i, &n) in run.card_frames.iter().enumerate() {
        assert!(
            n > 180,
            "card {i} holds the record while it types and for its 180-frame automatic press ({n} frames)"
        );
    }
    assert_eq!(
        run.waking_under_card, 0,
        "the last card closes on its press; Gala's box is not drawn under it"
    );
    assert_eq!(
        run.waking_neutral, 0,
        "the field reads the pad while Gala's box waits for it"
    );
    assert!(
        run.reached_give_item,
        "a press turns Gala's box: the Fire Droplet"
    );
    assert!(run.retired && flag, "the record runs out and raises 0x36F");
}
