use super::*;

/// The battle-intro enemy-name labels reach the page's overlay draw list
/// on retail's class-0 frame: the marbled fill columns (`FUN_8002BDC4`)
/// from the frame origin `(x - 8, 40)`, each the full 28-row height.
#[test]
fn live_battle_intro_labels_are_filled_class0_frames() {
    let Ok(disc) = std::env::var("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let Ok(bytes) = std::fs::read(&disc) else {
        eprintln!("[skip] disc unreadable");
        return;
    };
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(bytes, String::new()).expect("load disc");
    rt.enter_field("town01").expect("enter town01");
    for _ in 0..5 {
        rt.tick_frame().expect("tick");
    }
    if !rt.debug_start_test_battle() {
        eprintln!("[skip] no scripted formation row resolved");
        return;
    }
    let mut seen = false;
    for _ in 0..400 {
        rt.tick_frame().expect("tick");
        let labels = rt.scene_host.host().is_some_and(|h| {
            h.world.mode == SceneMode::Battle
                && h.world.battle.intro_names_frames > 0
                && !h.world.battle_entry_sweeping()
        });
        if !labels {
            continue;
        }
        let json = rt.play_overlay_draws_json(960, 720);
        let v: serde_json::Value = serde_json::from_str(&json).expect("overlay json");
        let fill: Vec<&serde_json::Value> = v["sprites"]
            .as_array()
            .expect("sprites array")
            .iter()
            .filter(|q| q["src"][0] == 128 && q["src"][1] == 0 && q["dst"][1] == 40 * 3)
            .collect();
        if fill.is_empty() {
            continue;
        }
        assert!(
            fill.iter().all(|q| q["dst"][3] == 28 * 3),
            "every fill column spans the 28-row frame: {fill:?}"
        );
        seen = true;
        break;
    }
    assert!(seen, "no filled intro label reached the page's draw list");
    eprintln!("[ran] intro labels drew on filled class-0 frames");
}

#[test]
fn live_battle_overlay_carries_bars_and_enemy_target_strip() {
    let Ok(disc) = std::env::var("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let Ok(bytes) = std::fs::read(&disc) else {
        eprintln!("[skip] disc unreadable");
        return;
    };
    let mut rt = LegaiaRuntime::new();
    rt.load_disc(bytes, String::new()).expect("load disc");
    rt.enter_field("town01").expect("enter town01");
    for _ in 0..5 {
        rt.tick_frame().expect("tick");
    }
    if !rt.debug_start_test_battle() {
        eprintln!("[skip] no scripted formation row resolved");
        return;
    }
    // Let the player-driven command menu open for party slot 0.
    for _ in 0..300 {
        let open = rt
            .scene_host
            .host()
            .is_some_and(|h| h.world.battle.command.is_some());
        if open {
            break;
        }
        rt.tick_frame().expect("tick");
    }
    assert!(
        rt.scene_host
            .host()
            .is_some_and(|h| h.world.battle.command.is_some()),
        "player-driven battle opens the command menu"
    );
    // The round prompt opens on a sixteen-vsync glide (sub-draw step 0
    // spawns the panels under the screen); the packet-pinned bands are
    // where it lands.
    for _ in 0..16 {
        rt.tick_frame().expect("tick");
    }

    // The prompt opens on a glide - the panels rise from under the screen
    // and the chips unfold - and the menu SM reads no pad until it lands
    // (`World::battle_command_pad_held`). Sample the surface at rest.
    let settle = |rt: &mut LegaiaRuntime| {
        for _ in 0..64 {
            let held = rt
                .scene_host
                .host()
                .is_some_and(|h| h.world.battle_command_pad_held());
            if !held {
                break;
            }
            rt.set_pad(0);
            rt.tick_frame().expect("tick");
        }
    };
    settle(&mut rt);

    // (1) The retail party surface reaches the page's overlay draw list.
    // 960x720 -> stage scale 3, origin (0,0). At rest that surface is the
    // roster panels (102x48 at the packet-pinned seats, `y 164`); the
    // acting member's readout moves to the full-width bar at `y 188`.
    // Whichever is up, retail draws NO gauge bar inside either - all of
    // that is asserted, so neither a lost surface nor a resurrected bar
    // can pass.
    const PANEL_ROW: i64 = 164 * 3;
    const PANEL_BOT: i64 = (164 + 48) * 3;
    const BAR_ROW: i64 = 188 * 3;
    let json = rt.play_overlay_draws_json(960, 720);
    let v: serde_json::Value = serde_json::from_str(&json).expect("overlay json");
    let texts = v["texts"].as_array().expect("texts array");
    let sprites = v["sprites"].as_array().expect("sprites array");
    let in_band = |a: &[serde_json::Value], y0: i64, y1: i64| {
        a.iter()
            .filter(|t| t["dst"][1].as_i64().is_some_and(|y| (y0..y1).contains(&y)))
            .count()
    };
    assert!(
        in_band(sprites, PANEL_ROW, PANEL_BOT) > 0 || in_band(sprites, BAR_ROW, BAR_ROW + 60) > 0,
        "no party chrome sprite on either packet-pinned band"
    );
    assert!(
        in_band(texts, PANEL_ROW, PANEL_BOT) > 0 || in_band(texts, BAR_ROW, BAR_ROW + 60) > 0,
        "no party glyph on either packet-pinned band"
    );
    for t in texts {
        let (Some(y), Some(w), Some(h)) = (
            t["dst"][1].as_i64(),
            t["dst"][2].as_i64(),
            t["dst"][3].as_i64(),
        ) else {
            continue;
        };
        if t["src"][2] != 1 || t["src"][3] != 1 {
            continue;
        }
        if !(PANEL_ROW..BAR_ROW + 60).contains(&y) {
            continue;
        }
        // Every solid rect on the retail surface is a plate body or one
        // of the 1-px rims the chrome-less fallback draws round it.
        let rim = w == 3 || h == 3;
        let plate = h == 20 * 3 || (w, h) == (102 * 3, 48 * 3);
        assert!(
            rim || plate,
            "a gauge bar survives on the retail party surface: {:?}",
            t["dst"]
        );
    }

    // (2) Reaching Attack opens targeting; the retail dedup name strip
    // resolves rows off the live formation and lands in the draw list at
    // the strip's stage band.
    //
    // The walk has to be PHASE-DRIVEN. Retail's opening flow is three
    // selection surfaces, not one flat list - the `Begin | Run` round
    // prompt, the four-arm ring in seat order (Item / Attack / magic /
    // Spirit), then `Auto | Command` - and the ring opens on **Item**.
    // This test used to mash Cross, which worked only while `Attack` was
    // entry 0 of a flat menu; against the retail flow the first confirm
    // opens the item submenu and hands the command session away, which is
    // why the failure read `cmd_phase=None`. Step to the arm, then
    // confirm.
    {
        use legaia_engine_battle_menus::battle_input::{AttackMode, BattleCommand, CommandPhase};
        let tap = |rt: &mut LegaiaRuntime, b: legaia_engine_system::input::PadButton| {
            settle(rt);
            rt.set_pad(b.mask());
            rt.tick_frame().expect("tick");
            rt.set_pad(0);
            rt.tick_frame().expect("tick");
        };
        use legaia_engine_system::input::PadButton;
        for _ in 0..40 {
            let phase = rt
                .scene_host
                .host()
                .and_then(|h| h.world.battle.command.as_ref())
                .map(|c| {
                    (
                        std::mem::discriminant(&c.phase),
                        matches!(c.phase, CommandPhase::RoundPrompt { .. }),
                        matches!(c.phase, CommandPhase::Menu { .. }),
                        matches!(c.phase, CommandPhase::AttackMode { .. }),
                        matches!(c.phase, CommandPhase::Targeting { .. }),
                        c.menu_command(),
                        c.attack_mode(),
                    )
                });
            let Some((_, round, menu, atk_mode, targeting, cmd, mode)) = phase else {
                break;
            };
            if targeting {
                break;
            }
            if round {
                tap(&mut rt, PadButton::Cross); // opens on Begin
            } else if menu {
                if cmd == Some(BattleCommand::Attack) {
                    tap(&mut rt, PadButton::Cross);
                } else {
                    // Spatial seating: Attack sits on the ring's left arm.
                    tap(&mut rt, PadButton::Left);
                }
            } else if atk_mode {
                if mode == Some(AttackMode::Auto) {
                    tap(&mut rt, PadButton::Cross);
                } else {
                    // Spatial seating: Auto is the left chip of Auto|Command.
                    tap(&mut rt, PadButton::Left);
                }
            } else {
                break;
            }
        }
    }
    {
        let w = &rt.scene_host.host().expect("host").world;
        let targeting = matches!(
            w.battle.command.as_ref().map(|c| &c.phase),
            Some(legaia_engine_battle_menus::battle_input::CommandPhase::Targeting { .. })
        );
        if !targeting {
            let pc = w.party.party_count.clamp(1, 3) as usize;
            let monsters: Vec<(usize, u16, u16, u16)> = w
                .actors
                .iter()
                .enumerate()
                .skip(pc)
                .take(5)
                .map(|(i, a)| (i, a.battle.hp, a.battle.max_hp, a.battle.liveness))
                .collect();
            eprintln!(
                "[dbg] mode={:?} cmd_phase={:?} dialog={} inline={} monsters={monsters:?}",
                w.mode,
                w.battle
                    .command
                    .as_ref()
                    .map(|c| std::mem::discriminant(&c.phase)),
                w.dialog.current.is_some(),
                w.dialog.inline.is_some(),
            );
        }
        assert!(targeting, "Cross on Attack opens the target picker");
        let rows = legaia_engine_battle_hud::battle_enemy_target_rows(w);
        assert!(
            !rows.is_empty(),
            "enemy target rows resolve from the live formation"
        );
        assert!(
            rows.iter().all(|r| !r.label.is_empty()),
            "every row carries a monster name"
        );
    }
    let json = rt.play_overlay_draws_json(960, 720);
    let v: serde_json::Value = serde_json::from_str(&json).expect("overlay json");
    // 960x720 -> stage scale 3, origin (0,0); the target-select plaque
    // (placement record 0x29) pens its name at stage Y 162 - 2 = 160 ->
    // surface y 480, content box centred on stage x 0xE8.
    let pen_y = 3 * (i64::from(legaia_engine_ui::battle_chrome::TARGET_SELECT_Y) - 2);
    let strip_glyphs = v["texts"]
        .as_array()
        .expect("texts array")
        .iter()
        .filter(|t| t["dst"][1] == pen_y)
        .count();
    assert!(
        strip_glyphs > 0,
        "target-select plaque glyphs land on record 0x29's pen row"
    );
    if let Ok(path) = std::env::var("LEGAIA_HUD_DUMP") {
        std::fs::write(path, &json).expect("write hud dump");
    }
}
