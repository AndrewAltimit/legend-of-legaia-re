//! Disc-gated: an op aimed at another actor lands on that actor, whichever
//! runner steps it.
//!
//! Retail's dispatcher swaps an extended op's resolved target into `s5`
//! before the arm runs (`FUN_801DE840` `0x801DE894..0x801DE8B4`, the id
//! resolver `FUN_8003C83C`), so the scene system script, a talk and a
//! spawned record all write the named actor's words. Each case below is a
//! shipped script the routing census
//! (`field_route_census_disc.rs`) found stepping on the wrong context:
//!
//! - `koin1` `P1[0]` re-stages six objects with `CC 02..07 50 <model>`;
//! - `town0e` `P1[0]` installs an ambient tree on a villager (`B4 07 30 01`)
//!   once the villager's clip cursor reads `0` (`CC 07 8C 05 00`);
//! - `juui2` `P1[0]` drops the player's shadow (`B1 F8 15`);
//! - `bubu1` `P1[22]`'s talk plays its helper's gestures (`A2 4F 17`);
//! - `bubu1` `P1[7]`'s talk turns its speaker's head (`CC 1E 45 ..`) and
//!   steps Vahn aside (`C1 F8 02 82`);
//! - `suimon` `P1[4]`'s talk lowers the sluice objects (`CC 05..09 42 ..`);
//! - `bylon` `P1[47]`'s talk slows Vahn's clip (`CC F8 41 20 00 00 00`).
//!
//! Skips without `LEGAIA_DISC_BIN` / `extracted/` (disc-gated convention).

use std::path::PathBuf;

use legaia_engine_scene_host::scene_host::SceneHost;
use legaia_engine_system::input::PadButton;

const ANCHOR_FLAGS: &str = include_str!("fixtures/spine_anchor_flags.txt");

fn extracted_dir() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    eprintln!("[skip] extracted/ missing");
    None
}

fn milestone_flags(name: &str) -> Vec<u16> {
    ANCHOR_FLAGS
        .lines()
        .find_map(|line| {
            let mut it = line.split_whitespace();
            (it.next() == Some(name)).then(|| {
                it.nth(1)
                    .unwrap_or("")
                    .split(',')
                    .filter_map(|f| f.parse().ok())
                    .collect()
            })
        })
        .expect("milestone in the fixture")
}

fn open() -> Option<SceneHost> {
    let dir = extracted_dir()?;
    Some(SceneHost::open_extracted(&dir).expect("open the host"))
}

fn enter(host: &mut SceneHost, scene: &str, flags: &[u16], settle: u32) {
    host.world
        .flags
        .system_flags
        .iter_mut()
        .for_each(|b| *b = 0);
    for &f in flags {
        host.world.system_flag_set(f);
    }
    host.enter_field_scene(scene, 0).expect("enter the scene");
    host.world.toggles.use_vm_dialogue = true;
    for _ in 0..settle {
        let _ = host.world.tick();
    }
    assert_eq!(host.world.mode, legaia_engine_mode::mode::SceneMode::Field);
}

/// The channel index an id resolves to.
fn channel(host: &SceneHost, id: u8) -> usize {
    legaia_engine_field::field_channels::resolve_target(host.world.channel_view(), id)
        .unwrap_or_else(|| panic!("id {id:#x} resolves"))
}

fn player_xz(host: &SceneHost) -> (i16, i16) {
    let p = usize::from(host.world.player_actor_slot.expect("a player"));
    let ms = &host.world.actors[p].move_state;
    (ms.world_x, ms.world_z)
}

/// Stand the player beside placement `slot` and talk to it.
fn talk_to(host: &mut SceneHost, slot: u8) {
    let &(nx, nz) = host.world.npcs.positions.get(&slot).expect("the talker");
    let (tx, tz) = (i32::from(nx - 0x40) >> 7, i32::from(nz - 0x40) >> 7);
    let stand = [
        (0, 1),
        (1, 0),
        (0, -1),
        (-1, 0),
        (1, 1),
        (-1, -1),
        (1, -1),
        (-1, 1),
    ]
    .into_iter()
    .map(|(dx, dz)| (tx + dx, tz + dz))
    .find(|&(sx, sz)| {
        let (x, z) = ((sx * 128 + 0x40) as i16, (sz * 128 + 0x40) as i16);
        host.world.field_tile_is_walk_visible(x, z) && !host.world.field_tile_is_wall(x, z)
    })
    .expect("a standable tile beside the talker");
    let p = usize::from(host.world.player_actor_slot.expect("a player"));
    host.world.actors[p].move_state.world_x = (stand.0 * 128 + 0x40) as i16;
    host.world.actors[p].move_state.world_z = (stand.1 * 128 + 0x40) as i16;
    host.world.trigger_field_interact(0, slot);
}

/// Run the talk out, pressing confirm, calling `each` every frame.
fn run_talk(host: &mut SceneHost, mut each: impl FnMut(&SceneHost)) {
    for f in 0..4_000 {
        host.world.input.set_pad(if f % 4 == 0 {
            PadButton::Cross.mask()
        } else {
            0
        });
        let _ = host.world.tick();
        each(host);
        if host.world.dialog.inline.is_none() && f > 8 {
            return;
        }
    }
    panic!("the talk ends");
}

#[test]
fn koin1_entry_restages_the_six_objects() {
    let Some(mut host) = open() else {
        return;
    };
    enter(&mut host, "koin1", &[], 4);
    for (id, model) in (2u8..=7).zip([98i16, 99, 103, 100, 102, 101]) {
        let ci = channel(&host, id);
        let ch = &host.world.channel_view()[ci];
        assert!(ch.object_bind, "id {id} names an object");
        assert_eq!(
            host.world.object_live_models().get(&ch.placement_index),
            Some(&model),
            "`CC {id:02X} 50 {model:02X} 00` re-stages object {id}"
        );
    }
}

#[test]
fn town0e_installs_the_trees_on_the_villagers() {
    let Some(mut host) = open() else {
        return;
    };
    enter(&mut host, "town0e", &[], 0);
    // The loop's four installs, each behind its villager's cursor test:
    // `B4 07 30 01`, `B4 15 30 02`, `B4 0E 30 03`, `B4 13 30 03`.
    let villagers: Vec<u8> = [0x07u8, 0x15, 0x0E, 0x13]
        .into_iter()
        .map(|id| {
            let ci = channel(&host, id);
            host.world.channel_view()[ci].placement_index as u8
        })
        .collect();
    let mut seen = false;
    for _ in 0..600 {
        let before = host.world.ambient.fx.len();
        let _ = host.world.tick();
        if !host.world.system_flag_test(0x6A9) || seen {
            continue;
        }
        seen = true;
        let spots: Vec<(i16, i16)> = villagers
            .iter()
            .map(|s| host.world.npcs.positions[s])
            .collect();
        let player = player_xz(&host);
        let fresh: Vec<(i16, i16)> = host.world.ambient.fx
            [before.min(host.world.ambient.fx.len())..]
            .iter()
            .map(|p| (p.state.world_x, p.state.world_z))
            .collect();
        assert!(!fresh.is_empty(), "`B4 07 30 01` installed a tree");
        for part in &fresh {
            assert!(
                spots.iter().any(|&(x, z)| {
                    (i32::from(part.0) - i32::from(x)).abs() < 0x100
                        && (i32::from(part.1) - i32::from(z)).abs() < 0x100
                }),
                "tree part {part:?} stands at a villager {spots:?}, not the player {player:?}"
            );
        }
    }
    assert!(seen, "the loop reached the install");
}

#[test]
fn juui2_entry_drops_the_players_shadow() {
    let Some(mut host) = open() else {
        return;
    };
    enter(&mut host, "juui2", &[], 4);
    let p = usize::from(host.world.player_actor_slot.expect("a player"));
    assert_ne!(
        host.world.actors[p].move_state.flags & 0x0020_0000,
        0,
        "`B1 F8 15` lands on the player"
    );
    // The next scene's player starts with its shadow.
    enter(&mut host, "town01", &[], 4);
    let p = usize::from(host.world.player_actor_slot.expect("a player"));
    assert_eq!(host.world.actors[p].move_state.flags & 0x0020_0000, 0);
}

#[test]
fn bubu1_talk_plays_the_helpers_gesture() {
    let Some(mut host) = open() else {
        return;
    };
    enter(&mut host, "bubu1", &[], 4);
    let ci = channel(&host, 0x4F);
    let helper = host.world.channel_view()[ci].placement_index as u8;
    talk_to(&mut host, 22);
    let mut played = false;
    run_talk(&mut host, |h| {
        played |= h
            .world
            .npcs
            .clip_cursors
            .get(&helper)
            .is_some_and(|c| c.anim_id == 0x17);
    });
    assert!(played, "`A2 4F 17` re-points the helper's clip");
}

#[test]
fn bubu1_talk_turns_its_speaker_and_steps_vahn_aside() {
    let Some(mut host) = open() else {
        return;
    };
    enter(&mut host, "bubu1", &[], 4);
    talk_to(&mut host, 7);
    use legaia_engine_field::actor_look::LookKey;
    let (mut parked_moving, mut speaker_looked, mut player_looked) = (0, false, false);
    let mut last = player_xz(&host);
    run_talk(&mut host, |h| {
        let now = player_xz(h);
        // The talk holds past the op while the player walks the glide.
        if h.world
            .dialog
            .inline
            .as_ref()
            .is_some_and(|i| i.pc == 0xFB && i.player_glide.is_some())
            && now != last
        {
            parked_moving += 1;
        }
        last = now;
        speaker_looked |= h.world.actor_look(LookKey::Npc(7)).is_some();
        player_looked |= h.world.actor_look(LookKey::Player).is_some();
    });
    assert!(
        parked_moving > 1,
        "`C1 F8 02 82` glides the player with the talk parked on it"
    );
    assert!(speaker_looked, "`CC 1E 45 ..` turns the speaker's head");
    assert!(!player_looked, "and not the player's");
}

#[test]
fn suimon_water_gate_talk_lowers_the_sluices() {
    let Some(mut host) = open() else {
        return;
    };
    enter(&mut host, "suimon", &milestone_flags("rogue_tower"), 4);
    let sluices: Vec<usize> = (5u8..=9).map(|id| channel(&host, id)).collect();
    let before: Vec<i16> = sluices
        .iter()
        .map(|&ci| host.world.channel_view()[ci].ctx.field_8e)
        .collect();
    talk_to(&mut host, 4);
    run_talk(&mut host, |_| {});
    let after: Vec<i16> = sluices
        .iter()
        .map(|&ci| host.world.channel_view()[ci].ctx.field_8e)
        .collect();
    for (i, (b, a)) in before.iter().zip(&after).enumerate() {
        assert_ne!(b, a, "`CC {:02X} 42 ..` moves sluice {}", 5 + i, 5 + i);
    }
}

#[test]
fn bylon_talk_slows_vahns_clip() {
    let Some(mut host) = open() else {
        return;
    };
    enter(&mut host, "bylon", &[], 4);
    talk_to(&mut host, 47);
    let mut rates = Vec::new();
    run_talk(&mut host, |h| {
        let r = h.world.locomotion.player_clip_rate;
        if rates.last() != Some(&r) {
            rates.push(r);
        }
    });
    assert!(
        rates.contains(&0x10),
        "`CC F8 41 20 ..` halves to rate 0x10: {rates:x?}"
    );
}
