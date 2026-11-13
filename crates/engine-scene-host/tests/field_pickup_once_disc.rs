//! Disc-gated: a chest or a floor pickup grants its item once per opening,
//! and the second touch reads the record's "already taken" arm.
//!
//! Retail's dialog SM resumes a placement's own context at `actor[+0x9E]`,
//! and an executed `0x21` leaves that cursor one past it (`FUN_80039B7C`).
//! Both pickup shapes in `dolk2` (Drake Castle) put the grant before such a
//! `0x21`:
//!
//! * a chest (`P1[62]`, the Healing Bloom): `39 88`, `50 43`, `21`, then
//!   `26 06 00` to the "The treasure chest is empty!" loop;
//! * the Platinum Card spot (`P1[70]`): `51 14`, `39 81`, the `36 00 80 2C 00`
//!   item-get cue, the box, `21`, then a `21` / `26 FE FF` park.
//!
//! The interaction entry is past the spawn section's flag test, so a talk that
//! restarts there instead of at the resumed cursor grants the item again on
//! every touch.
//!
//! Skips without `LEGAIA_DISC_BIN` / `extracted/` (disc-gated convention).

use std::path::PathBuf;

use legaia_engine_scene_host::scene_host::SceneHost;
use legaia_engine_system::input::PadButton;
use legaia_engine_system::sound_state::SfxRingOp;

const SCENE: &str = "dolk2";
/// The Healing Bloom chest and its item id.
const CHEST_SLOT: u8 = 62;
const HEALING_BLOOM: u8 = 0x88;
/// The Platinum Card spot and its item id.
const CARD_SLOT: u8 = 70;
const PLATINUM_CARD: u8 = 0x81;
/// The item-get cue both records sound before their "now has" box.
const ITEM_GET_CUE: i16 = 0x2C;

fn open_host() -> Option<SceneHost> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return SceneHost::open_extracted(&d).ok();
        }
    }
    eprintln!("[skip] extracted/ missing");
    None
}

fn enter(host: &mut SceneHost) {
    enter_scene(host, SCENE).expect("enter scene");
}

fn enter_scene(host: &mut SceneHost, scene: &str) -> anyhow::Result<()> {
    host.enter_field_scene(scene, 0)?;
    host.world.toggles.use_vm_dialogue = true;
    let scene = host.scene.as_ref().expect("scene");
    let scene_anm = scene.entries.iter().find_map(|e| {
        [3usize, 5, 6, 7].into_iter().find_map(|d| {
            legaia_actor_formats::player_anm::find_in_entry(&e.bytes, d)
                .into_iter()
                .next()
        })
    });
    let loco = host
        .index
        .entry_bytes(legaia_actor_formats::character_pack::PROT_ENTRY_INDEX)
        .ok()
        .and_then(|b| legaia_actor_formats::character_pack::field_locomotion_anm(&b).ok());
    host.world
        .drain_field_anim_cues(scene_anm.as_ref(), loco.as_ref(), |_| Some(true));
    for _ in 0..30 {
        let _ = host.world.tick();
    }
    let _ = host.world.take_sfx_ring_ops();
    Ok(())
}

/// One touch on `slot`, paged through with Cross until the talk ends.
/// Returns the item-get cues the talk pushed to the SFX ring and whether a
/// box opened.
fn touch(host: &mut SceneHost, slot: u8) -> (usize, bool) {
    host.world.trigger_field_interact(0, slot);
    let mut cues = 0;
    let mut boxed = false;
    let mut started = false;
    for f in 0..2_000 {
        host.world.input.set_pad(if f % 4 == 0 {
            PadButton::Cross.mask()
        } else {
            0
        });
        let _ = host.world.tick();
        cues += host
            .world
            .take_sfx_ring_ops()
            .iter()
            .filter(|op| matches!(op, SfxRingOp::Push(ITEM_GET_CUE)))
            .count();
        boxed |= host
            .world
            .dialog
            .inline
            .as_ref()
            .is_some_and(|id| id.panel.is_some());
        let live = host.world.dialog.inline.is_some() || host.world.dialog.current.is_some();
        started |= live;
        if started && !live {
            break;
        }
    }
    host.world.input.set_pad(0);
    for _ in 0..10 {
        let _ = host.world.tick();
    }
    (cues, boxed)
}

#[test]
fn chest_grants_its_item_once() {
    let Some(mut host) = open_host() else {
        return;
    };
    enter(&mut host);
    let before = host.world.held_count_by_id(HEALING_BLOOM);
    let (cues, _) = touch(&mut host, CHEST_SLOT);
    assert_eq!(
        host.world.held_count_by_id(HEALING_BLOOM),
        before + 1,
        "the first touch grants the chest's item"
    );
    assert_eq!(cues, 1, "and sounds the item-get cue once");
    for n in 0..3 {
        let (cues, boxed) = touch(&mut host, CHEST_SLOT);
        assert!(boxed, "an opened chest still says it is empty");
        assert_eq!(
            host.world.held_count_by_id(HEALING_BLOOM),
            before + 1,
            "touch {} of an opened chest grants nothing",
            n + 2
        );
        assert_eq!(cues, 0, "and sounds no item-get cue");
    }
}

#[test]
fn platinum_card_grants_once_with_its_cue() {
    let Some(mut host) = open_host() else {
        return;
    };
    enter(&mut host);
    let before = host.world.held_count_by_id(PLATINUM_CARD);
    let (cues, boxed) = touch(&mut host, CARD_SLOT);
    assert!(boxed, "the first touch says what was found");
    assert_eq!(
        host.world.held_count_by_id(PLATINUM_CARD),
        before + 1,
        "the first touch grants the card"
    );
    assert_eq!(cues, 1, "and sounds the item-get cue (`36 00 80 2C 00`)");
    for n in 0..3 {
        let (cues, boxed) = touch(&mut host, CARD_SLOT);
        // The record parks on `21` / `26 FE FF` behind its grant: the dialog
        // SM executes the `0x21` and ends the talk with no box.
        assert!(!boxed, "touch {} of the taken spot opens no box", n + 2);
        assert_eq!(
            host.world.held_count_by_id(PLATINUM_CARD),
            before + 1,
            "touch {} of the taken spot grants nothing",
            n + 2
        );
        assert_eq!(cues, 0, "and sounds no item-get cue");
    }
}

/// A placement the talk probe cannot reach: retired, or parked in the
/// off-map hide box (`doman` `P1[32]` sends itself there with `23 7F 7F`
/// behind its grant, and its record loops back onto the grant).
fn out_of_reach(host: &SceneHost, slot: u8) -> bool {
    let hide = legaia_engine_field::man_field_scripts::FIELD_OFFMAP_HIDE_XZ;
    host.world.field_placement_retired(slot)
        || host.world.npcs.positions.get(&slot) == Some(&(hide, hide))
}

/// Every placement record that carries a `0x39` grant, in every scene: three
/// touches grant its item on at most one of them. Pins the chest shape and the
/// pickup-spot shape disc-wide, and the pickup spots' first-touch grant (a
/// floor on the sites that grant at all keeps the census non-vacuous).
///
/// A record whose grant sits behind a later talk (`korb2` `P1[3]`, the
/// bookshelf that hands over its book on the second read) still grants on one
/// touch. Retired placements (a taken pickup's story arm sets `31 03`) are out
/// of the talk probe's reach and are skipped, and so is a placement its own
/// talk parks in the hide box. Story flags carry from scene to scene in CDNAME
/// order, so a pickup taken in `dolk` is retired in `dolk2`.
#[test]
fn every_pickup_grants_at_most_once() {
    let Some(mut host) = open_host() else {
        return;
    };
    let cdname = ["extracted", "../extracted", "../../extracted"]
        .iter()
        .map(|d| PathBuf::from(d).join("CDNAME.TXT"))
        .find(|p| p.exists())
        .map(|p| legaia_prot::cdname::parse(&p).expect("parse CDNAME"))
        .expect("CDNAME.TXT");
    let mut names: Vec<String> = cdname.values().cloned().collect();
    names.sort();
    names.dedup();
    let mut granted = 0usize;
    let mut repeats = Vec::new();
    for n in &names {
        if enter_scene(&mut host, n).is_err() {
            continue;
        }
        let mut sites: Vec<(u8, u8)> = host
            .world
            .npcs
            .dialog_prologue
            .iter()
            .filter_map(|(&slot, p)| {
                legaia_scene_formats::field_disasm::LinearWalker::new(&p.body, p.entry_pc)
                    .flatten()
                    .find(|i| p.body.get(i.pc) == Some(&0x39))
                    .and_then(|i| p.body.get(i.pc + 1).copied())
                    .map(|item| (slot, item))
            })
            .collect();
        sites.sort_unstable();
        for (slot, item) in sites {
            let mut granting_touches = 0;
            for _ in 0..3 {
                if out_of_reach(&host, slot) {
                    break;
                }
                let before = host.world.held_count_by_id(item);
                touch(&mut host, slot);
                if host.world.held_count_by_id(item) > before {
                    granting_touches += 1;
                }
            }
            if granting_touches > 0 {
                granted += 1;
            }
            if granting_touches > 1 {
                repeats.push(format!(
                    "{n} P1[{slot}] item {item:#04x} on {granting_touches} touches"
                ));
            }
        }
    }
    assert!(
        repeats.is_empty(),
        "pickups that grant again on a later touch: {repeats:#?}"
    );
    assert!(granted >= 200, "only {granted} pickup sites granted at all");
}
