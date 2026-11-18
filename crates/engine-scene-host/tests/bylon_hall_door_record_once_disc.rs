//! Disc-gated: back from West Voz, Biron's hall door plays Gala's beat once.
//!
//! After the attack (`0x2A1` up, `0x2A2` clear) the hall door `P0[31]`
//! fades out, seats the player on tile `(23, 115)` (`A3 F8 17 73`), fades
//! back in and spawns `P2[6]` (`44 61`) - the beat where the party steps
//! out of Vahn, Gala smells the Mist and runs ahead - then ends on its
//! `0x21`. The seat lands on `P2[6]`'s own walk-on tile, a fallback-table
//! kind-1 record for the same `P2[6]`.
//!
//! Retail plays the beat once. The spawn bumps the script runner's modal
//! depth and seeds the new context's share (`FUN_8003BDE0`,
//! `0x8003C080..0x8003C0CC`), so the door's closing `0x21` takes back only
//! its own share and the player's engaged bit stays up
//! (`FUN_80039B7C`, `0x80039ECC..0x80039F14`): the crossing onto the tile is
//! spent under the lock (`FUN_801D1EC4`, `0x801D214C`). The port counted a
//! spawned record as holding the player only from its first slice, so on the
//! door's last frame nothing held him, the crossing ran the tile's record as
//! well, and `P2[6]` played twice at once - the party stepped out twice,
//! every line ran twice and the second copy left the player locked while it
//! waited on actors the first had moved (issue #633).
//!
//! REF: FUN_8003BDE0, FUN_80039B7C, FUN_801D1EC4
//!
//! Skip-passes without disc data (CLAUDE.md convention).

use legaia_engine_scene_host::scene_host::SceneHost;
use legaia_engine_system::input::PadButton;

const ANCHOR_FLAGS: &str = include_str!("fixtures/spine_anchor_flags.txt");

/// `P2[6]`'s one-shot latch: its first op raises it.
const BEAT_DONE: u16 = 0x2A2;
/// The attack happened: the hall door spawns the beat.
const ATTACKED: u16 = 0x2A1;
/// Standing in front of the hall door, `P0[31]`, on the far side.
const BEFORE_DOOR: (i16, i16) = (4416, 5116);

fn open_host() -> Option<SceneHost> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = std::path::PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return SceneHost::open_extracted(&d).ok();
        }
    }
    let disc = std::env::var_os("LEGAIA_DISC_BIN")?;
    SceneHost::open_disc(std::path::PathBuf::from(disc)).ok()
}

#[test]
fn the_hall_door_plays_gala_s_beat_once() {
    let Some(mut host) = open_host() else {
        return;
    };
    let flags = ANCHOR_FLAGS
        .lines()
        .find_map(|l| l.strip_prefix("west_voz_forest "))
        .and_then(|l| l.split_whitespace().nth(1))
        .expect("west_voz_forest milestone");
    for f in flags.split(',').filter_map(|f| f.parse().ok()) {
        host.world.system_flag_set(f);
    }
    for f in 0x297..=ATTACKED {
        host.world.system_flag_set(f);
    }
    host.world.toggles.use_vm_dialogue = true;
    host.world.install_present_party_list(vec![0, 1, 2]);
    host.enter_field_scene("bylon", 0).expect("enter bylon");
    for _ in 0..60 {
        host.tick().expect("tick");
    }
    assert!(host.debug_seat_standing(BEFORE_DOOR.0, BEFORE_DOOR.1));
    let beat_len = {
        let man = host.world.field_vm.channels_man.clone().expect("bylon MAN");
        let mf = legaia_scene_formats::man_section::parse(&man).expect("MAN");
        legaia_engine_field::man_field_scripts::partition_record_span(&mf, &man, 2, 6)
            .expect("P2[6]")
            .2
    };
    let (mut copies_max, mut boxes, mut started, mut released) = (0usize, 0u32, false, false);
    let mut was_boxed = false;
    for f in 0..3000u32 {
        let engaged =
            host.world.dialogue_owns_input() || host.world.script_context_engages_player();
        // Walk into the door, then page every box.
        let pad = if engaged {
            if f % 8 == 0 {
                PadButton::Cross.mask()
            } else {
                0
            }
        } else if !started {
            PadButton::Down.mask()
        } else {
            0
        };
        host.world.set_pad(pad);
        host.tick().expect("tick");
        let timeline = host
            .world
            .cutscene
            .timeline
            .iter()
            .filter(|t| !t.is_done() && t.bytecode.len() == beat_len);
        let helpers = host
            .world
            .field_vm
            .helper_contexts
            .iter()
            .filter(|t| !t.is_done() && t.bytecode.len() == beat_len);
        let copies = timeline.count() + helpers.count();
        copies_max = copies_max.max(copies);
        started |= copies > 0;
        let boxed = host
            .world
            .field_vm
            .helper_contexts
            .iter()
            .any(|t| t.dialog.is_some())
            || host
                .world
                .cutscene
                .timeline
                .as_ref()
                .is_some_and(|t| t.dialog.is_some());
        if boxed && !was_boxed {
            boxes += 1;
        }
        was_boxed = boxed;
        if started && copies == 0 && !host.world.script_context_engages_player() {
            released = true;
            break;
        }
    }
    eprintln!("[bylon hall door] copies {copies_max} boxes {boxes} released {released}");
    assert!(started, "the door spawns P2[6]");
    assert_eq!(copies_max, 1, "P2[6] runs once, not twice at once");
    assert_eq!(boxes, 3, "Gala's two lines and Noa's line, once each");
    assert!(
        host.world.system_flag_test(BEAT_DONE),
        "the beat latches 0x2A2"
    );
    assert!(released, "the beat ends and lets the player go");
}
