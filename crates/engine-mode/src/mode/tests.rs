use super::*;

#[test]
fn the_boot_chain_is_ordered_and_every_step_names_its_store() {
    // Six stores, in the order a cold boot performs them. Each `mode` is
    // the literal the `sh` at `store_pc` writes.
    let modes: Vec<GameMode> = BOOT_MODE_CHAIN.iter().map(|s| s.mode).collect();
    assert_eq!(
        modes,
        vec![
            GameMode::ReadInit,
            GameMode::ReadMode,
            GameMode::CardInit,
            GameMode::CardMode,
            GameMode::MainInit,
            GameMode::MainMode,
        ]
    );
    // The title is not mode 0x10: that word belongs to the logo INIT, and
    // the title shares CARD MODE with the pause menu.
    assert_eq!(GameMode::ReadInit.as_index(), 0x10);
    assert_eq!(GameMode::CardMode.as_index(), 0x17);
    assert_eq!(GameMode::CardMode.scene_mode(), SceneMode::Menu);
    // Each INIT step in the chain is followed by the successor its own
    // handler stores.
    for pair in BOOT_MODE_CHAIN.windows(2) {
        if let Some(next) = init_successor(pair[0].mode) {
            assert_eq!(next, pair[1].mode, "{:?} hand-off", pair[0].mode);
            assert_eq!(
                pair[1].store_pc,
                INIT_HANDOFFS
                    .iter()
                    .find(|(m, _)| *m == pair[0].mode)
                    .unwrap()
                    .1
                    .store_pc,
                "the chain and the hand-off table must cite the same store"
            );
        }
    }
}

#[test]
fn mode_four_bounces_instead_of_advancing() {
    // The row that stops `init_successor` from being `index + 1`.
    assert_eq!(
        init_successor(GameMode::MonsterTest),
        Some(GameMode::ConfigInit)
    );
    assert_eq!(
        mode_init_bare(GameMode::MonsterTest),
        Some(ModeInitBare::SetsMode(GameMode::ConfigInit))
    );
    // Mode 18's hand-off lives in PROT 0902 (`li v0,0x13` at
    // `0x801CE8D4`, `sh` at `0x801CE8DC`), inside the staged entry
    // `0x801CE844` - so it is claimed here like the other eight.
    assert_eq!(
        init_successor(GameMode::GameOverInit),
        Some(GameMode::GameOverMode)
    );
    assert_eq!(
        INIT_HANDOFFS
            .iter()
            .find(|(m, _)| *m == GameMode::GameOverInit)
            .unwrap()
            .1
            .store_pc,
        0x801C_E8DC
    );
}

#[test]
fn adopting_a_scene_mode_round_trips_every_variant_the_word_can_name() {
    let mut seat = ModeSeat::new(GameMode::MainMode);
    for scene in [
        SceneMode::Field,
        SceneMode::WorldMap,
        SceneMode::Battle,
        SceneMode::Menu,
        SceneMode::Cutscene,
        SceneMode::Fishing,
        SceneMode::Dance,
        SceneMode::SlotMachine,
        SceneMode::BakaFighter,
        SceneMode::MuscleDome,
    ] {
        seat.adopt_scene_mode(scene);
        assert_eq!(
            seat.scene_mode(),
            scene,
            "the word plus the staged sub-id must name {scene:?} again"
        );
    }
    // Title is the port's "no scene loaded"; the retail word has no answer
    // for it, so the seat leaves the word where it was.
    let before = seat.game_mode();
    assert_eq!(seat.adopt_scene_mode(SceneMode::Title), None);
    assert_eq!(seat.game_mode(), before);
}

#[test]
fn an_init_frame_is_never_adopted_away() {
    let mut seat = ModeSeat::new(GameMode::MainInit);
    assert_eq!(seat.adopt_scene_mode(SceneMode::Battle), None);
    assert_eq!(seat.game_mode(), GameMode::MainInit);
}

#[test]
fn the_boot_handoff_branches_on_the_entry_word() {
    // Cold boot: `init.pak` raises the word itself, so the front end.
    let mut seat = ModeSeat::new(GameMode::ReadMode);
    assert_eq!(seat.entry_word(), 1);
    assert_eq!(seat.boot_handoff(), GameMode::CardInit);

    // The dev route: word clear, and the arm clears it again.
    let mut seat = ModeSeat::new(GameMode::ReadMode);
    seat.entry_word = 0;
    assert_eq!(seat.boot_handoff(), GameMode::ConfigInit);
    assert_eq!(seat.entry_word(), 0);

    // The request leaf writes both stores.
    let mut seat = ModeSeat::new(GameMode::MainMode);
    seat.entry_word = 0;
    seat.request_card_mode();
    assert_eq!(seat.game_mode(), GameMode::CardInit);
    assert_ne!(seat.entry_word(), 0);
}

#[test]
fn card_frame_body_replaces_the_master_driver_outright() {
    // Mode 23 is the only mode whose body is not FUN_80016444.
    assert_eq!(
        per_frame_stage(GameMode::CardMode).unwrap().body,
        FrameBody::CardDriver
    );
    assert_eq!(
        CARD_FRAME_BODY,
        CardFrameBody {
            runs_master_driver: false,
            ticks_card_actor: true,
            returns: 0,
        }
    );
    // Every other per-frame mode does run it.
    for m in TABLE.iter().map(|e| e.mode) {
        let Some(stage) = per_frame_stage(m) else {
            continue;
        };
        if m == GameMode::CardMode {
            continue;
        }
        assert!(
            matches!(stage.body, FrameBody::Master { .. }),
            "{m:?} unexpectedly not on the master driver"
        );
    }
}

#[test]
fn card_frame_body_never_aborts_the_frame_end_pass() {
    // FUN_80017978 ends `move v0,zero` with no other return path, so the
    // handler's abort test is structurally present but dead for CARD.
    assert_eq!(CARD_FRAME_BODY.returns, 0);
    assert!(per_frame_stage(GameMode::CardMode).unwrap().body_can_abort);
}

#[test]
fn debug_mode_advance_row_geometry_matches_the_ported_table() {
    // The chord reads mode_table[cur].next out of the same 24-byte rows
    // TABLE transcribes, so the two must agree on the geometry.
    assert_eq!(DEBUG_MODE_ADVANCE_TABLE_BASE, 0x8007_078C);
    assert_eq!(DEBUG_MODE_ADVANCE_ROW_STRIDE, 24);
    assert_eq!(DEBUG_MODE_ADVANCE_NEXT_OFFSET, 0x0A);
    // The i16 sentinel the chord tests with `bltz` is what `next: None`
    // stands for, so at least one row has to carry it.
    assert!(
        TABLE.iter().any(|e| e.next.is_none()),
        "no self-managed mode - the -1 sentinel would be unreachable"
    );
}

#[test]
fn core_state_reset_matches_retail_stores() {
    // FUN_80025CB4's literal stores, read off the disassembly
    // (li/sh + li/sw pairs at 0x80025CCC..0x80025D94).
    let r = CORE_STATE_RESET;
    assert_eq!(r.brightness, 0x80);
    assert_eq!(r.gte_h_zoom, 0xA0);
    assert_eq!(r.field_warm_entry, 0);
    assert_eq!(r.game_mode, 1, "CONFIG INIT hands off to CONFIG MODE");
    assert_eq!(r.data_field_index, 0xFFFF);
    assert_eq!(r.retail_ba36_b71c, 1);
    assert_eq!(r.b900, 0xFFFF_FFFF);
}

#[test]
fn mode_init_stage_plans_match_retail_wrappers() {
    // The three thin INIT wrappers' overlay params + jal targets.
    let cfg = mode_init_stage(GameMode::ConfigInit).unwrap();
    assert_eq!(cfg.overlay_a_param, 0x4C);
    assert_eq!(cfg.overlay_entry, 0x801C_E8EC);
    assert!(cfg.runs_core_reset);
    let main = mode_init_stage(GameMode::MainInit).unwrap();
    assert_eq!(main.overlay_a_param, 2);
    assert_eq!(main.overlay_entry, 0x801D_6704);
    assert!(!main.runs_core_reset);
    let go = mode_init_stage(GameMode::GameOverInit).unwrap();
    assert_eq!(go.overlay_a_param, 7);
    assert_eq!(go.overlay_entry, 0x801C_E844);
    // Non-wrapper modes have no plan.
    assert!(mode_init_stage(GameMode::MainMode).is_none());
    assert!(mode_init_stage(GameMode::BattleInit).is_none());
}

#[test]
fn bare_init_handlers_are_disjoint_from_the_staging_ones() {
    // Mode 4 writes the mode word and returns: it never reaches mode 5.
    assert_eq!(
        mode_init_bare(GameMode::MonsterTest),
        Some(ModeInitBare::SetsMode(GameMode::ConfigInit))
    );
    assert_eq!(
        mode_init_bare(GameMode::ReadInit),
        Some(ModeInitBare::Calls(0x801C_E9C0))
    );
    assert_eq!(READ_INIT_TARGET, 0x801C_E9C0);
    assert_eq!(
        mode_init_bare(GameMode::BattleInit),
        Some(ModeInitBare::Calls(0x8005_5B6C))
    );

    // The two shapes never overlap: a mode is staged or bare, not both.
    for m in [
        GameMode::ConfigInit,
        GameMode::MainInit,
        GameMode::GameOverInit,
        GameMode::MonsterTest,
        GameMode::ReadInit,
        GameMode::BattleInit,
        GameMode::MainMode,
    ] {
        assert!(
            mode_init_stage(m).is_none() || mode_init_bare(m).is_none(),
            "{m:?} claims both an overlay stage and a bare body"
        );
    }
    assert!(mode_init_bare(GameMode::MainMode).is_none());
}

/// The default handler covers 12 of the 14 per-frame modes; MAPDISP and
/// CARD are the two exceptions, and each differs in exactly one field.
#[test]
fn per_frame_stage_separates_the_two_exception_handlers() {
    // FUN_80025EEC: the master driver with a0 = 1, no overlay hook.
    for m in [
        GameMode::ConfigMode,
        GameMode::MainMode,
        GameMode::BattleMode,
        GameMode::StrMode,
    ] {
        let s = per_frame_stage(m).unwrap();
        assert_eq!(s.body, FrameBody::Master { param: 1 }, "mode {m:?}");
        assert!(s.overlay_hook.is_none(), "mode {m:?}");
    }
    // FUN_80025F2C: `jal 0x801CE850` between the passes, and the master
    // driver takes 0 (`jal 0x80016444; _clear a0`).
    let map = per_frame_stage(GameMode::MapdispMode).unwrap();
    assert_eq!(map.overlay_hook, Some(0x801C_E850));
    assert_eq!(map.body, FrameBody::Master { param: 0 });
    // FUN_80025F74: a different driver entirely.
    let card = per_frame_stage(GameMode::CardMode).unwrap();
    assert_eq!(card.body, FrameBody::CardDriver);
    assert!(card.overlay_hook.is_none());
    // INIT (even-indexed) modes have no per-frame plan.
    assert!(per_frame_stage(GameMode::MainInit).is_none());
    assert!(per_frame_stage(GameMode::CardInit).is_none());
}

#[test]
fn every_odd_mode_has_a_per_frame_stage_and_every_even_mode_has_none() {
    for i in 0..28usize {
        let m = GameMode::from_index(i).unwrap();
        assert_eq!(
            per_frame_stage(m).is_some(),
            i % 2 == 1,
            "mode {i} ({m:?}) parity"
        );
    }
}

#[test]
fn other_warp_stage_maps_sub_ids_to_overlays() {
    // sub 0..5 -> overlay params 0x4D..0x52 (PROT 972..977); sub 6
    // skips by 2 -> 0x55 (PROT 980, the dance overlay).
    let params: Vec<i32> = (0..7)
        .map(|s| other_warp_init_stage(s).unwrap().overlay_a_param)
        .collect();
    assert_eq!(params, vec![0x4D, 0x4E, 0x4F, 0x50, 0x51, 0x52, 0x55]);
    // Pinned attributions: fishing / slot / dance.
    assert_eq!(
        other_warp_init_stage(0).unwrap().overlay_a_param + 0x37F,
        972
    );
    assert_eq!(
        other_warp_init_stage(3).unwrap().overlay_a_param + 0x37F,
        975
    );
    assert_eq!(
        other_warp_init_stage(6).unwrap().overlay_a_param + 0x37F,
        980
    );
    // Entry table matches the 0x80010AE4 jump table.
    assert_eq!(other_warp_init_stage(2).unwrap().overlay_entry, 0x801C_EE80);
    assert_eq!(other_warp_init_stage(4).unwrap().overlay_entry, 0x801C_F00C);
    // Out-of-range sub-ids miss the retail `sltiu 7` bound.
    assert!(other_warp_init_stage(7).is_none());
    assert!(other_warp_init_stage(-1).is_none());
}

#[test]
fn table_has_28_entries_in_order() {
    assert_eq!(TABLE.len(), 28);
    for (i, entry) in TABLE.iter().enumerate() {
        assert_eq!(entry.mode.as_index(), i, "entry {i} index mismatch");
    }
}

#[test]
fn from_index_round_trips() {
    for i in 0..28 {
        let m = GameMode::from_index(i).unwrap();
        assert_eq!(m.as_index(), i);
    }
    assert!(GameMode::from_index(28).is_none());
}

#[test]
fn scene_mode_field_is_main_mode_not_mapdisp() {
    // Field/town gameplay is MainInit/MainMode (modes 2/3, game_mode
    // 0x03); MAPDISP (12/13) is the world-map display mode. The init
    // mode holds its successor's scene mode, same as Mapdisp/Battle/Str.
    assert_eq!(GameMode::MainInit.scene_mode(), SceneMode::Field);
    assert_eq!(GameMode::MainMode.scene_mode(), SceneMode::Field);
    assert_eq!(GameMode::MapdispInit.scene_mode(), SceneMode::WorldMap);
    assert_eq!(GameMode::MapdispMode.scene_mode(), SceneMode::WorldMap);
}

#[test]
fn scene_mode_for_battle_modes_is_battle() {
    assert_eq!(GameMode::BattleInit.scene_mode(), SceneMode::Battle);
    assert_eq!(GameMode::BattleMode.scene_mode(), SceneMode::Battle);
}

#[test]
fn scene_mode_for_str_modes_is_cutscene() {
    assert_eq!(GameMode::StrInit.scene_mode(), SceneMode::Cutscene);
    assert_eq!(GameMode::StrMode.scene_mode(), SceneMode::Cutscene);
}

#[test]
fn scene_mode_for_card_modes_is_menu() {
    // The in-field pause menu runs under the CARD pair (game_mode 0x17 =
    // 23, CARD MODE): all six menu-open library captures hold
    // `_DAT_8007B83C = 0x17`. The init mode holds its successor's scene
    // mode like the other pairs.
    assert_eq!(GameMode::CardInit.scene_mode(), SceneMode::Menu);
    assert_eq!(GameMode::CardMode.scene_mode(), SceneMode::Menu);
    assert_eq!(GameMode::CardMode.as_index(), 0x17);
}

/// The OTHER pair is the one place the retail mode word under-determines
/// the engine's `SceneMode`, and the sub-id is what closes the gap.
#[test]
fn the_other_pair_needs_the_warp_sub_id_to_resolve() {
    use crate::minigame_entry::MinigameSubId;
    // The live minigame mode is 0x19, not 0x18: 0x18 is the init half that
    // runs for one frame and hands the word on.
    assert_eq!(GameMode::OtherInit.as_index(), 0x18);
    assert_eq!(GameMode::OtherMode.as_index(), 0x19);
    // Mode word alone: no answer. This is what a host keying on
    // `scene_mode()` would have installed for a live minigame frame.
    assert_eq!(GameMode::OtherMode.scene_mode(), SceneMode::Title);
    // Mode word + sub-id: every playable slot resolves, on BOTH halves of
    // the pair (init modes hold their successor's scene mode).
    for slot in MinigameSubId::ALL {
        let sub = Some(i16::from(slot.sub_id()));
        let expect = slot.scene_mode().unwrap_or(SceneMode::Title);
        assert_eq!(
            GameMode::OtherMode.scene_mode_with_warp(sub),
            expect,
            "sub_id {} ({})",
            slot.sub_id(),
            slot.label()
        );
        assert_eq!(GameMode::OtherInit.scene_mode_with_warp(sub), expect);
    }
    // All five playable slots land on five DISTINCT scene modes - the
    // partition the retail mode word cannot express.
    let modes: Vec<_> = MinigameSubId::ALL
        .into_iter()
        .filter_map(|s| s.scene_mode())
        .collect();
    assert_eq!(modes.len(), 5);
    for (i, a) in modes.iter().enumerate() {
        assert!(
            !modes[i + 1..].contains(a),
            "two warp slots share {a:?} - the sub-id would not separate them"
        );
    }
    // Out of range and the two dev slots fall back to the mode word's own
    // answer rather than to a neighbouring minigame.
    assert_eq!(
        GameMode::OtherMode.scene_mode_with_warp(Some(7)),
        SceneMode::Title
    );
    assert_eq!(
        GameMode::OtherMode.scene_mode_with_warp(Some(-1)),
        SceneMode::Title
    );
    assert_eq!(
        GameMode::OtherMode.scene_mode_with_warp(Some(1)),
        SceneMode::Title
    );
}

/// Every mode outside the OTHER pair ignores the sub-id: the pair is one
/// exception, not a second axis.
#[test]
fn the_sub_id_only_moves_the_other_pair() {
    for i in 0..28usize {
        let m = GameMode::from_index(i).unwrap();
        if matches!(m, GameMode::OtherInit | GameMode::OtherMode) {
            continue;
        }
        for sub in [None, Some(-1), Some(0), Some(3), Some(6), Some(9)] {
            assert_eq!(
                m.scene_mode_with_warp(sub),
                m.scene_mode(),
                "mode {i} ({m:?}) moved on sub-id {sub:?}"
            );
        }
    }
}

/// The register is a signed halfword at `0x8007BA34`; the reader has to
/// agree with the `lh` the mode-24 init issues.
#[test]
fn the_warp_sub_id_reader_matches_the_retail_register() {
    assert_eq!(WARP_SUB_ID_ADDR, 0x8007_BA34);
    let mut ram = vec![0u8; 0x20_0000];
    let off = (WARP_SUB_ID_ADDR - 0x8000_0000) as usize;
    ram[off..off + 2].copy_from_slice(&6i16.to_le_bytes());
    assert_eq!(read_warp_sub_id(&ram), Some(6));
    // `lh`, so the top bit sign-extends rather than reading as 0xFFFF.
    ram[off..off + 2].copy_from_slice(&(-1i16).to_le_bytes());
    assert_eq!(read_warp_sub_id(&ram), Some(-1));
    assert_eq!(read_warp_sub_id(&[]), None);
}

#[test]
fn driver_starts_in_main_init() {
    let d = ModeDriver::new_at_boot();
    assert_eq!(d.current(), GameMode::MainInit);
    assert_eq!(d.frames_in_mode, 0);
}

#[test]
fn jump_to_resets_frame_counter() {
    let mut d = ModeDriver::new(GameMode::MapdispMode);
    d.frames_in_mode = 100;
    d.jump_to(GameMode::BattleInit);
    assert_eq!(d.frames_in_mode, 0);
}

#[test]
fn jump_to_same_mode_is_idempotent() {
    let mut d = ModeDriver::new(GameMode::MapdispMode);
    d.frames_in_mode = 100;
    d.jump_to(GameMode::MapdispMode);
    // Same mode -> frame counter NOT reset (a self-jump should be a no-op).
    assert_eq!(d.frames_in_mode, 100);
}
