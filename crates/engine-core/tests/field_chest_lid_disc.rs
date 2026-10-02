//! Disc-gated: a treasure chest's lid is shut until it is opened, plays open
//! exactly once on the touch, and stays open - including after the scene is
//! re-entered.
//!
//! A chest is not a placed `.MAP` prop but a MAN partition-1 actor (`宝箱N`,
//! model `0xF4` from the PROT 0874 player bank) whose clip is bound by its
//! own spawn script (`22 17` ExecMove). The same spawn script then branches
//! on the chest's opened story flag: `4C 35` (restart at frame 0, one-shot,
//! hold - shut) or `4C 36` (restart at the last frame, reverse, hold - open).
//! The touch body is `2C 07` / `2C 01` / `2B 03` / `2C 08` (forward, unhold,
//! clamp, clear the end latch) then the `2D 08` spin until the anim tick
//! latches the end, and only then the "There is ... in the treasure chest!"
//! box. All of that is the actor's `+0x62` word, which the world's NPC clip
//! cursor ticks under (`World::tick_npc_clips`, retail `FUN_800204F8`).
//!
//! Before the cursor honoured `+0x62`, every chest looped its open clip
//! forever, and the talk fell straight through the spin to the box.
//!
//! Skips without `LEGAIA_DISC_BIN` / `extracted/` (disc-gated convention).

use std::path::PathBuf;

use legaia_engine_core::input::PadButton;
use legaia_engine_core::scene::SceneHost;

/// West Voz Forest - nine chests in one scene.
const SCENE: &str = "vell";
/// Partition-1 placement slot of one of its chests (`宝箱１`).
const CHEST_SLOT: u8 = 8;

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

/// Enter the scene and do what a host does at entry: drain the spawn
/// scripts' clip cues against the scene + locomotion bundles, which binds
/// each NPC's world-owned clip cursor.
fn enter(host: &mut SceneHost) {
    host.enter_field_scene(SCENE, 0).expect("enter scene");
    host.world.toggles.use_vm_dialogue = true;
    let scene = host.scene.as_ref().expect("scene");
    let scene_anm = scene.entries.iter().find_map(|e| {
        [3usize, 5, 6, 7].into_iter().find_map(|d| {
            legaia_asset::player_anm::find_in_entry(&e.bytes, d)
                .into_iter()
                .next()
        })
    });
    let loco = host
        .index
        .entry_bytes(legaia_asset::character_pack::PROT_ENTRY_INDEX)
        .ok()
        .and_then(|b| legaia_asset::character_pack::field_locomotion_anm(&b).ok());
    host.world
        .drain_field_anim_cues(scene_anm.as_ref(), loco.as_ref(), |_| Some(true));
    assert!(
        host.world.npc_clip_cursor_bound(CHEST_SLOT),
        "the chest's spawn script binds its lid clip"
    );
}

fn cursor(host: &SceneHost) -> (i16, u16, u16) {
    let c = host.world.npcs.clip_cursors[&CHEST_SLOT];
    (c.cursor, c.flags, c.frames)
}

#[test]
fn chest_lid_is_shut_then_opens_once_and_stays_open() {
    let Some(mut host) = open_host() else {
        return;
    };
    enter(&mut host);

    // Shut: held at frame 0, however long the scene runs.
    for f in 0..90 {
        let _ = host.world.tick();
        assert_eq!(
            cursor(&host).0,
            0,
            "an unopened chest holds its lid shut (f{f})"
        );
    }
    let (_, _, frames) = cursor(&host);
    let last = (i32::from(frames) * 16 - 1) as i16;

    // Touch: the lid plays forward before the box opens, then clamps.
    host.world.trigger_field_interact(0, CHEST_SLOT);
    let mut lid_moving_before_box = false;
    let mut box_opened_at_end = false;
    for _ in 0..120 {
        let _ = host.world.tick();
        let (cur, _, _) = cursor(&host);
        let box_open = host
            .world
            .dialog
            .inline
            .as_ref()
            .is_some_and(|id| id.panel.is_some());
        if !box_open && cur > 0 && cur < last {
            lid_moving_before_box = true;
        }
        if box_open {
            box_opened_at_end = cur == last;
            break;
        }
    }
    assert!(lid_moving_before_box, "the lid swings before the item box");
    assert!(box_opened_at_end, "the box waits for the lid's end latch");

    // Dismiss, then keep running: the lid never loops back.
    for f in 0..400 {
        host.world.input.set_pad(if f % 4 == 0 {
            PadButton::Cross.mask()
        } else {
            0
        });
        let _ = host.world.tick();
        assert_eq!(cursor(&host).0, last, "an opened lid stays open (f{f})");
    }

    // Re-entry: the opened flag routes the spawn script to `4C 36`, the
    // already-open snap - last frame, held.
    enter(&mut host);
    for _ in 0..30 {
        let _ = host.world.tick();
    }
    let (cur, flags, frames) = cursor(&host);
    assert_eq!(
        cur >> 4,
        i16::try_from(frames).unwrap() - 1,
        "a re-entered opened chest poses open"
    );
    assert_ne!(
        flags & legaia_engine_core::field_env::ANIM_HOLD,
        0,
        "and holds there"
    );
}
