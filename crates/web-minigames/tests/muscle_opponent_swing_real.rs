//! Disc-gated: on the minigames page the dome opponent **attacks back** with
//! its own attack clip.
//!
//! The opponent's string is the battle's monster AI (`FUN_801DABA4` calls the
//! picker `FUN_801E9FD4` for a monster slot): its physical branch queues the
//! monster's own PROT 867 entries, not the player's direction bytes
//! `0x0C..=0x0F`. A selection the page closes itself (`muscle_end_selection`)
//! used to fill the opponent with the player's directions, so its swing
//! looked up archive entry `0x0C` - past the end of a small monster's entry
//! table - and the play-out acted out a stand-in. Here a fighter weak enough
//! to leave the round-1 monster standing lets it swing, and the swing is an
//! attack-tagged entry that moves the monster's body.
//!
//! Skips and passes when `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_web_minigames::LegaiaMinigames;

#[test]
fn the_opponent_swings_its_own_attack_entry() {
    let Some(disc) = std::env::var("LEGAIA_DISC_BIN").ok() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let bytes = std::fs::read(&disc).expect("read disc");
    let mut mg = LegaiaMinigames::new();
    mg.load_disc(bytes).expect("load disc");
    let ladder: serde_json::Value =
        serde_json::from_str(&mg.muscle_course_ladder_json()).expect("ladder json");
    let id = ladder[0]["rounds"][0]["id"].as_u64().expect("round-1 rung") as u16;

    // The monster's attack entries: the picker's candidates, tags
    // `0x0C..=0x1F` with a real AGL cost.
    let host = legaia_engine_scene_host::scene_host::SceneHost::open_disc(&disc).expect("disc");
    let archive = host.index.entry_bytes_extended(867).expect("archive");
    let record = legaia_actor_formats::monster_archive::record(&archive, id)
        .expect("archive parse")
        .expect("rung record");
    let attacks: Vec<u64> = record
        .spells
        .iter()
        .filter(|s| (0x0C..=0x1F).contains(&s.id) && s.agl_cost != 0xFF)
        .map(|s| u64::from(s.entry_index))
        .collect();
    assert!(!attacks.is_empty(), "the rung has attack entries");

    // A level-1 fighter cannot finish the monster in one string.
    assert!(mg.muscle_start_vs(0, 1, id, 7));
    for slot in 0..4 {
        mg.muscle_commit(slot);
    }
    mg.muscle_end_selection();
    mg.muscle_resolve();
    let log: serde_json::Value =
        serde_json::from_str(&mg.muscle_round_log_json()).expect("log json");
    let swings: Vec<u64> = log
        .as_array()
        .expect("plays")
        .iter()
        .filter(|p| p["attacker"] == 1)
        .map(|p| p["cmd"].as_u64().unwrap())
        .collect();
    assert!(!swings.is_empty(), "the opponent acted: {log}");
    for cmd in &swings {
        assert!(
            attacks.contains(cmd),
            "opponent swing {cmd} is not an attack entry {attacks:?}"
        );
    }

    // The play-out moves the monster's body through its swing.
    let nf = mg.muscle_fighter_positions(0).len();
    let nm = mg.muscle_monster_positions(id).len();
    let mut last: Option<Vec<f32>> = None;
    let mut moved_in_swing = 0usize;
    for _ in 0..800 {
        mg.muscle_surface_frame(0, 0);
        let beat: serde_json::Value =
            serde_json::from_str(&mg.muscle_surface_beat_json()).unwrap_or_default();
        let p = mg.muscle_surface_positions();
        if beat["attacker"] == 1
            && beat["kind"] == "swing"
            && let Some(l) = last.as_ref()
        {
            moved_in_swing = moved_in_swing.max(
                l[nf..nf + nm]
                    .iter()
                    .zip(&p[nf..nf + nm])
                    .filter(|(a, b)| (**a - **b).abs() > 0.5)
                    .count(),
            );
        }
        last = Some(p);
    }
    assert!(
        moved_in_swing > 0,
        "the monster's body moves through its swing"
    );
    eprintln!(
        "[ok] opponent swings {swings:?} (attacks {attacks:?}), {moved_in_swing} floats moved"
    );
}
