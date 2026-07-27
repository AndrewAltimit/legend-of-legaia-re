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
    assert!(mg.muscle_scene_ready(monster, 0), "3D scene decodes");

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
    let vram = mg.muscle_vram(monster, 0);
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

#[test]
fn muscle_fighter_battle_form_accessors_are_parallel() {
    let Some((mg, _)) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    // Every dome fighter slot (Vahn / Noa / Gala) assembles its battle form
    // from its player battle file, with parallel accessor buffers.
    for ch in 0..3u32 {
        let pos = mg.muscle_fighter_positions(ch);
        let n = pos.len() / 3;
        assert!(n > 0, "char {ch} battle form assembles");
        assert_eq!(mg.muscle_fighter_uvs(ch).len(), n * 2);
        assert_eq!(mg.muscle_fighter_cba_tsb(ch).len(), n * 2);
        assert_eq!(mg.muscle_fighter_flat_rgba(ch).len(), n * 4);
        assert_eq!(mg.muscle_fighter_object_ids(ch).len(), n);
        let idx = mg.muscle_fighter_indices(ch);
        assert!(!idx.is_empty() && idx.len() % 3 == 0);
        assert!(idx.iter().all(|&i| (i as usize) < n), "indices in range");

        let parts = mg.muscle_fighter_part_count(ch);
        assert!(parts > 0);
        let anims: serde_json::Value =
            serde_json::from_str(&mg.muscle_fighter_anims_json(ch)).unwrap();
        let anims = anims.as_array().unwrap();
        // Idle (slot 0) and all four per-command swings (0xC..=0xF - the
        // card ids themselves) must be present; the flinch (slot 2) too.
        for want in [0u64, 2, 0xC, 0xD, 0xE, 0xF] {
            let row = anims
                .iter()
                .find(|a| a["slot"].as_u64() == Some(want))
                .unwrap_or_else(|| panic!("char {ch} slot {want:#x} clip decodes"));
            let frames = row["frame_count"].as_u64().unwrap() as usize;
            assert!(frames > 0);
            let stream = mg.muscle_fighter_pose_frames(ch, want as u32, parts);
            assert_eq!(
                stream.len(),
                frames * parts as usize * 6,
                "char {ch} slot {want:#x} pose stream padded to the rig"
            );
        }
    }
}

#[test]
fn muscle_queue_resolves_arts_through_the_real_tables() {
    let Some((mut mg, _)) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let monster = first_roster_id(&mg);
    // Level 50 Vahn: enough AGL budget for a three-swing sequence.
    assert!(mg.muscle_start_vs(0, 50, monster, 0x2A));
    let st: serde_json::Value = serde_json::from_str(&mg.muscle_state_json()).unwrap();
    let hand = st["hand"].as_array().unwrap();
    let slot_for = |cmd: u64| {
        hand.iter()
            .position(|c| c["cmd"].as_u64() == Some(cmd))
            .expect("hand carries all four directions")
    };
    // Vahn's Tornado Flame (Hyper): Ra-Seru Ra-Seru Arms = R R L =
    // command ids 0xD 0xD 0xC (curated arts table, cross-checked against
    // the SCUS arts-name table's own combo string).
    for cmd in [0xDu64, 0xD, 0xC] {
        assert!(mg.muscle_commit(slot_for(cmd)), "commit {cmd:#x}");
    }
    let arts: serde_json::Value = serde_json::from_str(&mg.muscle_round_arts_json()).unwrap();
    let arts = arts.as_array().unwrap();
    assert!(!arts.is_empty(), "queue R R L performs an art");
    assert_eq!(arts[0]["start"], 0);
    assert_eq!(arts[0]["len"], 3);
    assert_eq!(arts[0]["kind"], "hyper", "Tornado Flame is a Hyper Art");
    assert!(
        arts[0]["name"]
            .as_str()
            .unwrap()
            .to_lowercase()
            .contains("tornado"),
        "named off the disc's arts table: {}",
        arts[0]["name"]
    );
}

#[test]
fn muscle_arena_backdrop_decodes() {
    let Some((mg, _)) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    // The Sol arena backdrop (PROT 1225, the other6.lzs tail slot) decodes as
    // a scene_tmd_stream: shell TMD + two TIM pages.
    let meta: serde_json::Value = serde_json::from_str(&mg.muscle_arena_json()).unwrap();
    assert_eq!(meta["ok"], true, "arena backdrop: {meta}");
    assert_eq!(meta["prot"], 1225);
    assert_eq!(meta["tims"], 2, "two type-0x01 texture pages");

    let pos = mg.muscle_arena_positions();
    let n = pos.len() / 3;
    assert!(n > 300, "arena shell has real geometry: {n} verts");
    assert_eq!(mg.muscle_arena_uvs().len(), n * 2);
    assert_eq!(mg.muscle_arena_cba_tsb().len(), n * 2);
    assert_eq!(mg.muscle_arena_flat_rgba().len(), n * 4);
    let idx = mg.muscle_arena_indices();
    assert!(!idx.is_empty() && idx.len() % 3 == 0);
    assert!(idx.iter().all(|&i| (i as usize) < n), "indices in range");

    // Half-stage authoring rule: the shell sits at X >= 0 (open side -X),
    // spanning thousands of world units on a Y <= ~0 (Y-down) profile.
    let (mut min_x, mut max_x) = (f32::MAX, f32::MIN);
    for v in pos.chunks_exact(3) {
        min_x = min_x.min(v[0]);
        max_x = max_x.max(v[0]);
    }
    assert!(min_x >= -1.0, "authored at X >= 0: min_x = {min_x}");
    assert!(max_x > 2000.0, "arena-scale extent: max_x = {max_x}");

    // The backdrop's texture pages ride in the dome VRAM: the (832, 0) page
    // band (the ground-grid sampling address) is non-zero after the merge.
    let monster = first_roster_id(&mg);
    let vram = mg.muscle_vram(monster, 0);
    let row = 64usize; // inside the 0..256 page rows
    let off = (row * 1024 + 832) * 2;
    assert!(
        vram[off..off + 128].iter().any(|&b| b != 0),
        "arena texture page resident at (832, 0)"
    );
}

#[test]
fn muscle_sfx_cues_decode() {
    let Some((mg, _)) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let meta: serde_json::Value = serde_json::from_str(&mg.muscle_sfx_json()).unwrap();
    assert_eq!(meta["ok"], true, "sfx: {meta}");
    // The match SM's blip ids 0x21/0x22/0x23 through FUN_8004fcc8's id-1 leg.
    let ui: Vec<u64> = meta["ui"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap())
        .collect();
    assert_eq!(ui, vec![0x20, 0x21, 0x22]);
    assert_eq!(meta["hit"], 0x09);
    assert_eq!(meta["hit_voices"], 2, "row 9 keys two voice layers");

    // Every pinned row decodes to real PCM at a sane rate.
    for &row in &[0x20u8, 0x21, 0x22, 0x09] {
        let pcm = mg.muscle_sfx_pcm(row, 0);
        let rate = mg.muscle_sfx_rate(row, 0);
        assert!(pcm.len() > 100, "row {row:#x} decodes PCM: {}", pcm.len());
        assert!((4000..=96_000).contains(&rate), "row {row:#x} rate: {rate}");
    }
    // A second voice layer either resolves to real PCM or is cleanly absent
    // (a consecutive tone region with no VAG is silent, never garbage) - and
    // out-of-range voice indexes are refused rather than aliased.
    let hit_l1 = mg.muscle_sfx_pcm(0x09, 1);
    assert!(hit_l1.is_empty() || hit_l1.len() > 100);
    assert!(mg.muscle_sfx_pcm(0x20, 4).is_empty());
}
