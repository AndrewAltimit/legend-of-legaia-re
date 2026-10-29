//! Disc-gated: a two-wave expansion formation of real retail monsters runs
//! as one battle through the scene host. The second wave takes the freed
//! monster seat with its own record's numbers (the archive merge's real
//! stats under the pack's edits), and the fight ends in one results
//! sequence that pays both waves. Skips and passes without
//! `LEGAIA_DISC_BIN`.

use std::sync::Arc;

use legaia_engine_core::live_loop::LiveLoopOpts;
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_scene_host::scene_host::SceneHost;
use legaia_expansion_format::{ExpansionPack, PackFiles};

/// Caruban, the rikuroa boss.
const FIRST: u16 = 0x49;
/// A second, unrelated record.
const SECOND: u16 = 0x0A;

fn pack() -> Arc<ExpansionPack> {
    let mut files = PackFiles::new();
    files.insert(
        "pack.toml".into(),
        b"id = \"disctest\"\nname = \"Disc Test\"\nversion = \"0.1.0\"\n".to_vec(),
    );
    // Both waves are frail and toothless so the town01 party wins on auto.
    files.insert(
        "battle.toml".into(),
        format!(
            r#"
[monsters.husk]
base = {{ kind = "monster", id = {FIRST} }}
hp = 1
attack = {{ mul = 0.05 }}
exp = 300

[monsters.shade]
base = {{ kind = "monster", id = {SECOND} }}
name = "Shade"
hp = 3
attack = {{ mul = 0.05 }}
defence = {{ mul = 0.5 }}
exp = 700

[formations.two_waves]
monsters = ["husk"]

[[formations.two_waves.waves]]
monsters = ["shade"]
"#
        )
        .into_bytes(),
    );
    Arc::new(ExpansionPack::from_files(&files).expect("pack parses"))
}

#[test]
fn a_second_wave_seats_real_stats_and_one_results_screen_pays_both() {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut host = SceneHost::open_disc(&disc).expect("open the disc");
    let scene = "town01";
    host.enter_field_scene(scene, 0).expect("enter town01");
    host.world.arm_live_loop(scene, &LiveLoopOpts::playable());
    host.world.toggles.live_gameplay_loop = true;
    host.world.battle.player_driven = false;
    host.world
        .activate_expansion(pack())
        .expect("pack activates");
    host.merge_expansion_monsters();
    let second = host
        .world
        .tables
        .monster_catalog
        .get(SECOND)
        .cloned()
        .expect("the archive merge seats the second record");
    assert_ne!(second.raw_stats, [0; 6], "a real record");

    assert!(host.world.start_expansion_battle("two_waves"));
    for _ in 0..2_000 {
        if host.world.mode == SceneMode::Battle {
            break;
        }
        host.tick().expect("scene host tick");
    }
    assert_eq!(host.world.mode, SceneMode::Battle, "the fight starts");
    let seat = usize::from(host.world.party.party_count);
    assert_eq!(host.world.actors[seat].battle_monster_id, Some(FIRST));

    let mut second_seen = false;
    let mut victories = 0;
    let mut in_victory = false;
    for _ in 0..40_000 {
        host.tick().expect("scene host tick");
        if host.world.mode != SceneMode::Battle {
            break;
        }
        let a = &host.world.actors[seat];
        if !second_seen && a.battle_monster_id == Some(SECOND) && a.battle.liveness != 0 {
            second_seen = true;
            // The pack's numbers on the real record, installed as the
            // battle's entry installs them (a random-class fight).
            assert_eq!((a.battle.hp, a.battle.max_hp), (3, 3));
            let live = host.world.tables.monster_catalog.get(SECOND).unwrap();
            assert_eq!(live.name, "Shade");
            let half = |v: u16| {
                if v == 0 {
                    0
                } else {
                    ((u32::from(v) * 500 + 500) / 1000).max(1) as u16
                }
            };
            assert_eq!(live.raw_stats[2], half(second.raw_stats[2]));
            let bs = live.installed_stats(false);
            assert_eq!(host.world.battle.attack[seat], bs[1]);
            assert_eq!(host.world.battle.speed[seat], bs[5]);
            // Its mesh and clips come from the same archive read as any
            // seated monster's: the scene host stages its clip set.
        }
        if second_seen && host.world.actors[seat].battle_action_clips.is_some() {
            assert_eq!(host.world.actors[seat].battle_monster_id, Some(SECOND));
        }
        let v = host.world.battle.victory.is_some();
        if v && !in_victory {
            victories += 1;
        }
        in_victory = v;
    }
    assert!(second_seen, "the second wave took the seat");
    assert_ne!(host.world.mode, SceneMode::Battle, "the fight ended");
    assert_eq!(victories, 1, "one results sequence");
    let rewards = host
        .world
        .battle
        .last_rewards
        .clone()
        .expect("the party won");
    assert_eq!(rewards.xp, 300 + 700, "both waves' EXP, summed once");
    assert_eq!(
        host.world.tables.monster_catalog.get(SECOND).map(|d| d.hp),
        Some(second.hp),
        "the catalog's record is the retail one again"
    );
    assert!(host.world.expansion.battle.waves.is_none());
}
