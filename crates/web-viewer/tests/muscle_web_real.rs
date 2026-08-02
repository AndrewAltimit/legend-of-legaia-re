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

#[test]
fn muscle_hud_chrome_decodes_from_the_disc() {
    let Some((mg, _)) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let hud: serde_json::Value = serde_json::from_str(&mg.muscle_hud_json()).unwrap();
    assert_eq!(hud["ok"], true, "hud: {hud}");

    // Sheet dimensions: the boot-gap chrome TIMs + the etim banner page +
    // the dome-hub pages, exactly as the capture pinned them in VRAM.
    assert_eq!(hud["sheets"]["widget"][0], 256);
    assert_eq!(hud["sheets"]["widget"][1], 192);
    assert_eq!(hud["sheets"]["font"][0], 256);
    assert_eq!(hud["sheets"]["atlas"][1], 256);
    assert_eq!(hud["sheets"]["banner"][0], 256);
    assert_eq!(hud["sheets"]["hub0"][0], 256, "hub pages from 1220: {hud}");
    assert_eq!(hud["sheets"]["hub1"][0], 256);

    // The SCUS element table: capture-verified anchors. Element 8 is the
    // Item chip gliding to (204, 34); element 9 the Attack chip to
    // (160, 66); element 7 the 288-wide status plate at (16, 236->194).
    let elems = hud["elements"].as_array().unwrap();
    assert_eq!(elems.len(), 80);
    assert_eq!(elems[8]["b"][0], 204);
    assert_eq!(elems[8]["b"][1], 34);
    assert_eq!(elems[9]["b"][0], 160);
    assert_eq!(elems[9]["b"][1], 66);
    assert_eq!(elems[7]["w"], 288);
    assert_eq!(elems[7]["a"][1], 236);
    assert_eq!(elems[7]["b"][1], 194);

    // The PROT 0977 hub sprite table: record 3 is the 240x18 "Welcome to
    // the Muscle Dome!" strip on hub page 0; record 16 the 192x32 INTERVAL
    // heading; record 0 the 144x32 ROUND word.
    let hub = hud["hub"].as_array().unwrap();
    assert_eq!(hub.len(), 17);
    assert_eq!(hub[3]["wh"][0], 240);
    assert_eq!(hub[3]["wh"][1], 18);
    assert_eq!(hub[3]["sheet"], 4);
    assert_eq!(hub[16]["uv"][1], 192);
    assert_eq!(hub[16]["wh"][0], 192);
    assert_eq!(hub[0]["wh"][0], 144);

    // Font advances reproduce the captured chip-label pen positions:
    // "Begin" drew B->e at +7, e->g at +6, g->i at +6, i->n at +4.
    let adv = hud["advance"].as_array().unwrap();
    assert_eq!(adv.len(), 96);
    let a = |c: char| adv[c as usize - 0x20].as_u64().unwrap() as i32;
    assert_eq!(a('B'), 7);
    assert_eq!(a('e'), 6);
    assert_eq!(a('g'), 6);
    assert_eq!(a('i'), 4);

    // Every sheet the page fetches decodes to RGBA with real opaque
    // coverage (the chrome art is opaque-on-transparent).
    for (source, pal, name) in [
        (0u32, 4u32, "widget/blue"),
        (0, 12, "widget/gold"),
        (0, 7, "widget/dpad"),
        (0, 1, "widget/gauge"),
        (0, 5, "widget/slash"),
        (0, 6, "widget/chip+bar (arts input)"),
        (0, 2, "widget/list window"),
        (1, 13, "font"),
        (1, 15, "font/orange (arts list)"),
        (2, 13, "atlas"),
        (2, 15, "atlas/orange arrows"),
        (3, 3, "banner words"),
        (3, 4, "red X"),
        (4, 6, "hub0"),
        (5, 0, "hub1"),
        (6, 0, "button glyphs"),
    ] {
        let rgba = mg.muscle_hud_sheet_rgba(source, pal);
        assert!(!rgba.is_empty(), "{name} decodes");
        let opaque = rgba.chunks_exact(4).filter(|p| p[3] != 0).count();
        assert!(
            opaque * 50 > rgba.len() / 4,
            "{name} has real opaque coverage: {opaque}"
        );
    }

    // The arts-input piece block (recomp GP0 packet capture) rides in the
    // hud JSON, and the button-glyph gap TIM at PROT.DAT 0x7B00 decodes at
    // its captured shape (64x32 texels, own 16-entry CLUT).
    let ai = &hud["arts_input"];
    assert_eq!(ai["cmd_chip"]["body"][0], 215);
    assert_eq!(ai["cmd_label"]["v"]["high"], 104);
    assert_eq!(ai["arts_arrows"]["u"]["left"], 244);
    assert_eq!(ai["tri_button"]["r"][2], 16);
    assert_eq!(hud["sheets"]["button"][0], 64, "button TIM: {hud}");
    assert_eq!(hud["sheets"]["button"][1], 32);

    // The AP plate's meter is NOT a sheet tile - the widget sheet carries
    // none, because retail draws the fill as an untextured gouraud pair.
    // The only tile near it is the baked "100" numeral that fills the end
    // box at a full gauge. A piece named `gauge_fill` pointed the page at
    // that numeral, so the plate drew a stretched "100" where the orange
    // meter belongs and left the value box empty; keep the name honest so
    // the JSON can't invite that again.
    let pieces = &hud["pieces"];
    assert!(
        pieces.get("gauge_fill").is_none(),
        "no fill tile exists on the sheet: {pieces}"
    );
    let hundred = &pieces["gauge_100"];
    assert_eq!(
        [
            hundred["r"][0].as_u64().unwrap() as u32,
            hundred["r"][1].as_u64().unwrap() as u32,
            hundred["r"][2].as_u64().unwrap() as u32,
            hundred["r"][3].as_u64().unwrap() as u32,
        ],
        [
            legaia_asset::title_pak::OVERLAY_SYSTEM_UI_GAUGE_100.0,
            legaia_asset::title_pak::OVERLAY_SYSTEM_UI_GAUGE_100.1,
            legaia_asset::title_pak::OVERLAY_SYSTEM_UI_GAUGE_100.2,
            legaia_asset::title_pak::OVERLAY_SYSTEM_UI_GAUGE_100.3,
        ],
        "the end-box numeral is the shared GAUGE_100 rect"
    );
    // The meter's gouraud endpoints reach the page through the arts-input
    // block over the shared pinned span - the plate's only fill source.
    assert_eq!(
        ai["ap_input_fill"]["rect"][0],
        legaia_engine_ui::arts_input::AP_FILL_X
    );
    assert_eq!(
        ai["ap_input_fill"]["rect"][2],
        legaia_engine_ui::arts_input::AP_FILL_W
    );
    assert_eq!(
        ai["ap_input_fill"]["rgb"][0][0],
        legaia_asset::title_pak::OVERLAY_SYSTEM_UI_GAUGE_FILL_GOLD_RGB.0
    );
}

/// The browser's shading law: a textured vert's `a_flat_rgba` carries the
/// prim's **packet colour**, and the shader draws `texel * rgb / 128`. White
/// there is not "unlit" - it is `texel * 255/128`, which reads as blown-out
/// lighting rather than as a missing stream, so nothing about the frame says
/// the colour word was dropped.
///
/// The parallel-accessor tests above assert stream *lengths*, and a
/// `vec![255; n * 4]` satisfies every one of them. This asserts the
/// *content*: the dome's three bodies must carry the disc's own modulation.
#[test]
fn muscle_vertex_colours_are_the_packet_colours_not_white() {
    let Some((mg, _)) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let monster = first_roster_id(&mg);

    // Textured verts (flag 255) whose rgb is not the neutral 0x80 are the
    // proof the colour word survived; a white stream has none below 255.
    let modulated = |flat: &[u8]| -> (usize, usize) {
        let mut textured = 0usize;
        let mut white = 0usize;
        for p in flat.chunks_exact(4) {
            if p[3] == 0 {
                continue; // untextured fill - its colour is the fill itself
            }
            textured += 1;
            if p[0] == 255 && p[1] == 255 && p[2] == 255 {
                white += 1;
            }
        }
        (textured, white)
    };

    for (name, flat) in [
        ("arena shell", mg.muscle_arena_flat_rgba()),
        ("fighter body", mg.muscle_fighter_flat_rgba(0)),
        ("monster body", mg.muscle_monster_flat_rgba(monster)),
    ] {
        assert!(!flat.is_empty(), "{name} uploads a colour stream");
        let (textured, white) = modulated(&flat);
        assert!(textured > 0, "{name} has textured verts");
        assert_eq!(
            white, 0,
            "{name}: {white}/{textured} textured verts upload white, i.e. \
             draw at texel * 255/128"
        );
    }
}

#[test]
fn muscle_arts_list_rows_come_from_the_scus_table() {
    let Some((mut mg, _)) = loaded() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let monster = first_roster_id(&mg);
    assert!(mg.muscle_start_vs(0, 30, monster, 7));
    let rows: serde_json::Value = serde_json::from_str(&mg.muscle_arts_list_json()).unwrap();
    let rows = rows.as_array().unwrap();
    assert!(!rows.is_empty(), "Vahn's arts resolve from the SCUS table");
    for row in rows {
        assert!(!row["name"].as_str().unwrap().is_empty());
        let dirs = row["dirs"].as_array().unwrap();
        assert!(!dirs.is_empty());
        assert!(dirs.iter().all(|d| (1..=4).contains(&d.as_u64().unwrap())));
        // Every SCUS-backed row carries the retail AP byte (the menu
        // minimum is 18).
        assert!(row["ap"].as_u64().unwrap() >= 18, "row: {row}");
    }

    // The retail input flow: committing until exhaustion trips the
    // auto-end, and reselect refunds the budget.
    let state: serde_json::Value = serde_json::from_str(&mg.muscle_state_json()).unwrap();
    let pool = state["budget"][0].as_u64().unwrap();
    let mut committed = 0u64;
    while !mg.muscle_selection_exhausted() {
        assert!(
            (0..4).any(|slot| mg.muscle_commit(slot)),
            "some card commits until exhausted"
        );
        committed += 1;
        assert!(committed < 32);
    }
    let state: serde_json::Value = serde_json::from_str(&mg.muscle_state_json()).unwrap();
    assert!(state["budget"][0].as_u64().unwrap() < pool);
    mg.muscle_reset_selection();
    let state: serde_json::Value = serde_json::from_str(&mg.muscle_state_json()).unwrap();
    assert_eq!(
        state["budget"][0].as_u64().unwrap(),
        pool,
        "reselect refunds"
    );
    assert_eq!(state["queue"][0].as_array().unwrap().len(), 0);
}
