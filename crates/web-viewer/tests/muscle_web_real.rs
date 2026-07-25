//! Disc-gated: the Muscle Dome web surface resolves its battle tables from a
//! real disc and a contest round settles through the ported battle formulas
//! (move-power record -> arts predamage -> affinity scale -> damage finisher)
//! against fighter stats read off the disc's own records (SCUS new-game
//! template + growth curves for the player, PROT 867 monster record for the
//! opponent).
//!
//! No Sony bytes are asserted - only structural facts (tables decode, stats
//! are non-trivial, damage is deterministic per seed, the 3D accessors are
//! parallel). Skips + passes when `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_web_viewer::minigames::LegaiaMinigames;

fn loaded() -> Option<(LegaiaMinigames, serde_json::Value)> {
    let disc = std::env::var("LEGAIA_DISC_BIN").ok()?;
    let bytes = std::fs::read(&disc).ok()?;
    let mut mg = LegaiaMinigames::new();
    let status: serde_json::Value = serde_json::from_str(&mg.load_disc(bytes).ok()?).unwrap();
    Some((mg, status))
}

fn first_roster_id(mg: &LegaiaMinigames) -> u16 {
    let roster: serde_json::Value = serde_json::from_str(&mg.muscle_roster_json()).unwrap();
    roster[0]["id"].as_u64().expect("roster non-empty") as u16
}

#[test]
fn muscle_tables_and_disc_stats_resolve() {
    let Some((mg, status)) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    assert_eq!(status["muscle"]["ok"], true, "muscle: {status:?}");
    // A full disc image carries the executable, so the player record is disc
    // data (template + growth curves), not the fallback constants.
    assert_eq!(status["muscle"]["stats"], "disc");
    let cards = status["muscle"]["cards"].as_array().unwrap();
    assert_eq!(cards.len(), 4);
    for c in cards {
        let id = c.as_u64().unwrap();
        assert!(
            (0x0C..=0x0F).contains(&id),
            "hand ids are the four swing commands: {cards:?}"
        );
    }

    // The roster lists renderable monsters with the boosted battle profile.
    let roster: serde_json::Value = serde_json::from_str(&mg.muscle_roster_json()).unwrap();
    let rows = roster.as_array().unwrap();
    assert!(rows.len() > 20, "roster has real coverage: {}", rows.len());
    for r in rows.iter().take(5) {
        assert!(r["hp"].as_u64().unwrap() > 0);
        assert!(!r["name"].as_str().unwrap().is_empty());
        assert!(r["element"].as_u64().unwrap() <= 7);
    }
}

#[test]
fn muscle_round_resolves_through_battle_formulas_deterministically() {
    let Some((mut mg, _)) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let monster = first_roster_id(&mg);

    let run = |mg: &mut LegaiaMinigames, seed: u32| -> (serde_json::Value, serde_json::Value) {
        assert!(mg.muscle_start_vs(0, 30, monster, seed));
        let st: serde_json::Value = serde_json::from_str(&mg.muscle_state_json()).unwrap();
        // Commit every affordable card, then close and resolve.
        for slot in 0..4 {
            mg.muscle_commit(slot);
        }
        mg.muscle_end_selection();
        mg.muscle_resolve();
        let log: serde_json::Value = serde_json::from_str(&mg.muscle_round_log_json()).unwrap();
        (st, log)
    };

    let (st, log) = run(&mut mg, 0x1234);

    // Player record is the SCUS template leveled through the growth curves.
    assert_eq!(st["source"], "disc");
    assert_eq!(st["names"].as_array().unwrap().len(), 2);
    assert!(!st["names"][0].as_str().unwrap().is_empty(), "player name");
    assert!(!st["names"][1].as_str().unwrap().is_empty(), "monster name");
    // Round budgets seed from the fighters' AGL pools (the +0x154 read).
    let budget0 = st["budget"][0].as_u64().unwrap();
    assert!(budget0 > 0, "player budget = leveled AGL: {st}");
    assert_eq!(
        budget0,
        st["stats"][0]["budget_pool"].as_u64().unwrap(),
        "budget seeds from the AGL pool"
    );
    // Swing-card costs come from the player battle file's +0x74 records: the
    // retail per-command value set (favored / off-class / far).
    for c in st["hand"].as_array().unwrap() {
        let cost = c["cost"].as_u64().unwrap();
        assert!(
            [0x1E, 0x2A, 0x36].contains(&cost),
            "swing cost off the disc's value set: {cost:#x}"
        );
    }

    // The round resolved through the formulas: plays logged, damage capped,
    // HP monotonically consistent with the log.
    let plays = log.as_array().unwrap();
    assert!(!plays.is_empty(), "round log has plays");
    for p in plays {
        let dmg = p["damage"].as_i64().unwrap();
        assert!((0..=9999).contains(&dmg), "finisher cap: {p}");
        let cmd = p["cmd"].as_u64().unwrap();
        assert!((0x0C..=0x0F).contains(&cmd));
    }
    let st_after: serde_json::Value = serde_json::from_str(&mg.muscle_state_json()).unwrap();
    let last = plays.last().unwrap();
    assert_eq!(
        st_after["hp"][0].as_i64().unwrap(),
        last["hp"][0].as_i64().unwrap(),
        "session HP matches the log's shadow mirror"
    );
    assert_eq!(
        st_after["hp"][1].as_i64().unwrap(),
        last["hp"][1].as_i64().unwrap()
    );
    // The defender of any hit accrued spirit (the +0x170 gauge).
    let spirit: Vec<u64> = st_after["spirit"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap())
        .collect();
    assert!(spirit[0] > 0 || spirit[1] > 0, "spirit accrued: {spirit:?}");

    // Determinism: the same seed replays byte-identically; a different seed
    // (fresh PsyQ rand stream) diverges somewhere in the rolls.
    let (_, log_same) = run(&mut mg, 0x1234);
    assert_eq!(log.to_string(), log_same.to_string(), "seeded determinism");
    let (_, log_other) = run(&mut mg, 0xBEEF);
    assert_ne!(
        log.to_string(),
        log_other.to_string(),
        "a different seed draws a different roll stream"
    );
}

#[test]
fn muscle_scene_accessors_are_parallel() {
    let Some((mg, _)) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let monster = first_roster_id(&mg);
    assert!(mg.muscle_scene_ready(monster), "3D scene decodes");

    let pos = mg.muscle_monster_positions(monster);
    let n = pos.len() / 3;
    assert!(n > 0, "monster mesh has vertices");
    assert_eq!(mg.muscle_monster_uvs(monster).len(), n * 2);
    assert_eq!(mg.muscle_monster_cba_tsb(monster).len(), n * 2);
    assert_eq!(mg.muscle_monster_flat_rgba(monster).len(), n * 4);
    assert_eq!(mg.muscle_monster_object_ids(monster).len(), n);
    let idx = mg.muscle_monster_indices(monster);
    assert!(!idx.is_empty() && idx.len() % 3 == 0);
    assert!(idx.iter().all(|&i| (i as usize) < n), "indices in range");

    let parts = mg.muscle_monster_part_count(monster);
    assert!(parts > 0);
    let anims: serde_json::Value =
        serde_json::from_str(&mg.muscle_monster_anims_json(monster)).unwrap();
    let anims = anims.as_array().unwrap();
    assert!(!anims.is_empty(), "monster has action animations");
    assert_eq!(anims[0]["action_id"], 0, "action 0 is the idle loop");
    let frames0 = anims[0]["frame_count"].as_u64().unwrap() as usize;
    let stream = mg.muscle_monster_pose_frames(monster, 0, parts);
    assert_eq!(
        stream.len(),
        frames0 * parts as usize * 6,
        "pose stream padded to the mesh's part count"
    );

    // One VRAM serves both bodies: 1 MB, with the monster page injected at
    // battle slot 0 (some non-zero bytes in the (320,256) page region).
    let vram = mg.muscle_vram(monster);
    assert_eq!(vram.len(), 1024 * 512 * 2);
    let row = 300usize; // inside the 256..512 page rows
    let off = (row * 1024 + 320) * 2;
    assert!(
        vram[off..off + 128].iter().any(|&b| b != 0),
        "monster texture page injected at slot 0"
    );

    // The reward names through the SCUS spell table (player Seru block).
    let name = mg.muscle_spell_name(0x81);
    assert!(!name.is_empty(), "reward spell names from the disc");
}
