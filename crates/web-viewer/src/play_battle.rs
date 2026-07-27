//! Browser **live battle** host: encounter arming + battle overlay draws.
//!
//! The simulation half is entirely [`legaia_engine_core`]: with
//! `World::live_gameplay_loop` armed, `World::tick` rolls step-driven random
//! encounters off the scene MAN's own encounter table, flips
//! `Field -> Battle`, runs the battle-action state machine (player-driven
//! command menus included - `battle_player_driven`), and returns to the field
//! with loot on victory. Nothing here re-implements a rule; this module is
//! the browser twin of the native window's battle *presentation*:
//!
//! * [`LegaiaRuntime::arm_live_battles`] mirrors
//!   `BootSession::enter_field_live`'s live-loop arming (MAN encounter table
//!   first, synthetic vanilla registry as the fallback the native boot uses).
//! * [`LegaiaRuntime::tick_battle_presentation`] mirrors the native
//!   `sync_battle_render` mode-edge latch + `drain_and_log_battle_events`:
//!   arm the ENCOUNTER! banner on the `Field -> Battle` edge, fold battle
//!   events, feed strike FX into the damage-popup model, refresh the
//!   per-slot rows through the **shared**
//!   [`legaia_engine_core::battle_hud::sync_battle_hud_rows`] fold.
//! * [`LegaiaRuntime::battle_overlay_draws`] mirrors the native window's
//!   battle HUD block (`window/hud.rs`): the shared
//!   [`legaia_engine_ui::battle_hud_draws_for`] rows (carrying the retail
//!   HP / MP colour law, FUN_800349EC / FUN_80035EA8), the centred
//!   [`legaia_engine_ui::encounter_banner_draws_for`] transition banner, and
//!   the player-driven command / arts / magic / item submenus.
//!
//! Draws are emitted in **surface pixels** (not the 320x240 menu stage): the
//! native window draws its battle HUD in surface space, and the HUD's
//! measured column offsets (name / HP / MP / AP / K.O.) span wider than the
//! 320-px stage, so stage-scaling would push the status strip off screen.
//!
//! What this host still lacks vs the native window is the battle's 3D layer
//! (monster meshes, the assembled party battle forms, the stage dome): the
//! scene keeps rendering behind the overlay. That is a render gap, not a
//! rules gap - the fight itself is the same engine-core battle.

use crate::runtime::LegaiaRuntime;
use legaia_engine_core::battle_hud::{
    BattleHud, DamagePopup, encounter_banner_label, sync_battle_hud_rows,
};
use legaia_engine_core::world::SceneMode;
use legaia_engine_ui::{self as ui, HudPopupView, HudSlotMeta, HudSlotView, TextDraw};
use wasm_bindgen::prelude::*;

/// Top-left anchor of the battle HUD's slot-row block, in surface pixels
/// (the native window's `BATTLE_HUD_PEN`).
const BATTLE_HUD_PEN: (i32, i32) = (8, 60);
/// Frames the encounter-transition banner stays on screen after a
/// `Field -> Battle` mode change (~1.5 s at the 60 Hz sim tick; the native
/// window's `ENCOUNTER_BANNER_FRAMES`).
const ENCOUNTER_BANNER_FRAMES: u16 = 90;
/// Left margin of the battle command / arts / magic submenus.
const MENU_X: i32 = 8;
/// First row Y of the battle command / arts / magic submenus.
const MENU_Y: i32 = 210;

/// Project the HUD model's slot array into the shared builder's view type.
///
/// Every slot is emitted, **including inactive ones** (as empty-name rows the
/// builder skips): `battle_hud_draws_for` derives both a row's Y and a
/// popup's anchor from the slice index, so the index has to stay the absolute
/// actor-table slot. Same projection as the native window's
/// `battle_hud_slot_views`.
fn battle_hud_slot_views<'a>(hud: &'a BattleHud, letters: &'a [Vec<u8>]) -> Vec<HudSlotView<'a>> {
    hud.slots
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let meta = HudSlotMeta {
                is_party: s.is_party,
                alive: s.alive,
                hp: s.hp,
                hp_max: s.hp_max,
                mp: s.mp,
                mp_max: s.mp_max,
                ap_filled: s.ap_filled,
                ap_max: s.ap_max,
            };
            let name = if s.active { s.name.as_str() } else { "" };
            let strip: &'a [u8] = letters.get(i).map(|v| v.as_slice()).unwrap_or(&[]);
            HudSlotView::from_plain(meta, name, strip)
        })
        .collect()
}

/// Project the HUD model's popup queue into the shared builder's view type.
fn battle_hud_popup_views(hud: &BattleHud) -> Vec<HudPopupView> {
    hud.popup_views()
        .into_iter()
        .map(|p| HudPopupView {
            slot: p.slot,
            amount: p.amount,
            is_heal: p.is_heal,
            is_crit: p.is_crit,
            status_letter: p.status_letter,
            alpha: p.alpha,
        })
        .collect()
}

impl LegaiaRuntime {
    /// Arm the live gameplay loop on the freshly-entered scene, the browser
    /// twin of `BootSession::enter_field_live`'s live-loop block:
    /// `enter_field_scene` already installed the scene MAN's own encounter
    /// table (with its formations' real archive stats), so the synthetic
    /// vanilla registry is only the fallback for scenes whose MAN carries no
    /// rollable encounter (towns resolve to a 0% trigger rate there - no
    /// invented town fights). Battles are player-driven, as the native
    /// `--player-battle` flag makes them.
    pub(crate) fn arm_live_battles(&mut self, scene: &str) {
        let Some(host) = self.scene_host.as_mut() else {
            return;
        };
        let world = &mut host.world;
        if world.encounter.is_none() && matches!(world.mode, SceneMode::Field) {
            world.set_formation_table(
                legaia_engine_core::monster_catalog::vanilla_formation_table(),
                legaia_engine_core::monster_catalog::vanilla_monster_catalog(),
            );
            let registry = legaia_engine_core::encounter_registry::vanilla_encounter_registry();
            world.install_encounter_for_scene(&registry, scene);
        }
        world.live_gameplay_loop = true;
        world.battle_player_driven = true;
        world.set_seru_registry(legaia_engine_core::seru_learning::SeruRegistry::retail());
    }

    /// Per-tick battle presentation, called from [`LegaiaRuntime::tick_frame`]:
    /// the browser twin of the native `sync_battle_render` mode-edge latch +
    /// `drain_and_log_battle_events`. Cheap no-op while no scene is up.
    pub(crate) fn tick_battle_presentation(&mut self) {
        let Some(host) = self.scene_host.as_mut() else {
            return;
        };
        // Mode-edge latch: arm the ENCOUNTER! banner entering battle, drop
        // it (and any stale popups) leaving.
        let mode = host.world.mode;
        let prev = self.prev_scene_mode.replace(mode);
        if prev != Some(mode) {
            match (prev, mode) {
                (_, SceneMode::Battle) => {
                    self.encounter_banner =
                        Some((ENCOUNTER_BANNER_FRAMES, encounter_banner_label(&host.world)));
                }
                (Some(SceneMode::Battle), _) => {
                    self.encounter_banner = None;
                    self.battle_hud.clear_popups();
                }
                _ => {}
            }
        }
        // Drain world battle events and fold each into gameplay state
        // (`ApplyArtStrike` mutates HP / status; the rest are visual-only).
        let events = host.world.drain_battle_events();
        for ev in events {
            host.world.fold_battle_event(&ev);
        }
        // Floating damage / heal numbers: the live loop resolves HP itself
        // and queues a presentation-only FX per strike.
        let fx = host.world.drain_battle_hit_fx();
        for f in fx {
            if f.is_heal {
                self.battle_hud.push_heal(f.target_slot, f.amount);
            } else if f.is_crit {
                self.battle_hud
                    .push_popup(DamagePopup::damage(f.target_slot, f.amount).crit());
            } else {
                self.battle_hud.push_damage(f.target_slot, f.amount);
            }
        }
        // Battle strike SFX cues route into the page's existing delay
        // scheduler (`crate::play_sfx`); the arts-voice shouts are CD-XA
        // clips this host has no demuxed channel bank for yet, so they are
        // drained (the world must not accumulate them) and dropped.
        let cues = host.world.drain_battle_sfx_cues();
        let _ = host.world.drain_battle_shout_cues();
        // Refresh per-slot rows + status icons through the shared fold, then
        // age the popups one frame.
        if host.world.mode == SceneMode::Battle {
            sync_battle_hud_rows(&mut self.battle_hud, &host.world);
            for slot in 0..self.battle_hud.slots.len() as u8 {
                self.battle_hud
                    .sync_status(slot, &host.world.status_effects);
            }
        }
        self.battle_hud.tick();
        // Age the encounter-transition banner one frame; drop it at zero.
        if let Some((frames, _)) = &mut self.encounter_banner {
            *frames = frames.saturating_sub(1);
            if *frames == 0 {
                self.encounter_banner = None;
            }
        }
        // `enqueue_sfx` needs `&mut self`, so fire after the host borrow ends.
        for cue in cues {
            if let Ok(id) = u8::try_from(cue.kind) {
                self.enqueue_sfx(id, cue.timing_frames);
            }
        }
    }

    /// Battle overlay text draws in **surface pixels**: HUD rows, the
    /// encounter banner, and the player-driven submenus. Empty outside
    /// [`SceneMode::Battle`]. Mirrors the native window's battle HUD block
    /// leg for leg (`window/hud.rs`).
    pub(crate) fn battle_overlay_draws(
        &self,
        assets: &crate::play_menu::PlayMenuAssets,
        surface_w: u32,
        surface_h: u32,
    ) -> Vec<TextDraw> {
        use legaia_engine_core::battle_input::{BattleCommand, CommandPhase};
        use legaia_engine_core::target_picker::{CursorRow, PickerState};

        let Some(bw) = self.scene_host.as_ref().map(|h| &h.world) else {
            return Vec::new();
        };
        if bw.mode != SceneMode::Battle {
            return Vec::new();
        }
        let font = assets.font_ref();
        let white = [1.0f32, 1.0, 1.0, 1.0];
        let dim = [0.7f32, 0.85, 1.0, 1.0];
        // Greyed-out row tint (K.O.'d targets, unaffordable spells).
        let down_color = [0.6f32, 0.6, 0.6, 1.0];
        let mut out: Vec<TextDraw> = Vec::new();

        // Per-slot rows, status strip and floating popups all come from the
        // shared builder, which carries the ported retail HP / MP colour law
        // (`hp_bar_color_index` / `mp_bar_color_index`, FUN_800349EC /
        // FUN_80035EA8). Rows are fed from the `BattleHud` model, refreshed
        // each tick by the shared `sync_battle_hud_rows` fold.
        let letters: Vec<Vec<u8>> = self
            .battle_hud
            .slots
            .iter()
            .map(|s| s.status_letters())
            .collect();
        out.extend(ui::battle_hud_draws_for(
            font,
            &battle_hud_slot_views(&self.battle_hud, &letters),
            &battle_hud_popup_views(&self.battle_hud),
            &[],
            BATTLE_HUD_PEN,
        ));

        // Encounter-transition banner: centred "ENCOUNTER!" over the
        // formation label, shown for the opening frames of the battle.
        if let Some((_, label)) = &self.encounter_banner {
            let head_w = font.layout_ascii("ENCOUNTER!").advance_x as i32;
            let pen = ((surface_w as i32 - head_w) / 2, surface_h as i32 / 4);
            out.extend(ui::encounter_banner_draws_for(font, label, pen));
        }

        // Player-driven submenus (opened from the Arts / Magic / Item
        // commands). Each parks both the SM and the command session while
        // open, so it takes priority over the command menu. While an
        // in-battle dialogue box owns the frame (the tutorial text), the
        // menus are hidden - retail shows no command chrome under it.
        let dialogue_up = bw.current_dialog.is_some() || bw.inline_dialogue.is_some();
        if dialogue_up {
            // Dialogue box up: no menu chrome.
        } else if let Some(arts) = &bw.battle_arts_menu {
            use legaia_engine_core::battle_arts::ArtsPhase;
            let mut my = MENU_Y;
            match &arts.phase {
                ArtsPhase::Select { cursor } => {
                    let header = format!("P{} - arts:", arts.actor + 1);
                    out.extend(ui::text_draws_for(
                        &font.layout_ascii(&header),
                        (MENU_X, my),
                        white,
                    ));
                    my += 16;
                    if arts.arts.is_empty() {
                        out.extend(ui::text_draws_for(
                            &font.layout_ascii("  (no saved arts)"),
                            (MENU_X + 8, my),
                            down_color,
                        ));
                    }
                    for (i, row) in arts.arts.iter().enumerate() {
                        let sel = i as u8 == *cursor;
                        let marker = if sel { ">" } else { " " };
                        let line = match (row.miracle, row.super_art) {
                            (Some(name), _) => {
                                format!("{} {} x{} *{}*", marker, row.name, row.hits(), name)
                            }
                            (None, Some(name)) => {
                                format!("{} {} x{} <{}>", marker, row.name, row.hits(), name)
                            }
                            (None, None) => format!("{} {} x{}", marker, row.name, row.hits()),
                        };
                        let color = if sel { white } else { dim };
                        out.extend(ui::text_draws_for(
                            &font.layout_ascii(&line),
                            (MENU_X + 8, my),
                            color,
                        ));
                        my += 14;
                    }
                }
                ArtsPhase::Targeting { picker, .. } => {
                    let line = match picker.state() {
                        PickerState::Cursor {
                            row: CursorRow::Enemy,
                            slot,
                        } => format!("art -> target M{}", slot + 1),
                        PickerState::Cursor {
                            row: CursorRow::Ally,
                            slot,
                        } => format!("art -> target P{}", slot + 1),
                        _ => "art -> select target".to_string(),
                    };
                    out.extend(ui::text_draws_for(
                        &font.layout_ascii(&line),
                        (MENU_X, my),
                        white,
                    ));
                    my += 14;
                    out.extend(ui::text_draws_for(
                        &font.layout_ascii("Left/Right=move  Cross=confirm  Circle=back"),
                        (MENU_X, my),
                        dim,
                    ));
                }
                _ => {}
            }
        } else if let Some(spell) = &bw.battle_spell_menu {
            use legaia_engine_core::battle_magic::SpellPhase;
            let mut my = MENU_Y;
            match &spell.phase {
                SpellPhase::Select { cursor } => {
                    let header = format!("P{} - magic:", spell.actor + 1);
                    out.extend(ui::text_draws_for(
                        &font.layout_ascii(&header),
                        (MENU_X, my),
                        white,
                    ));
                    my += 16;
                    if spell.spells.is_empty() {
                        out.extend(ui::text_draws_for(
                            &font.layout_ascii("  (no spells)"),
                            (MENU_X + 8, my),
                            down_color,
                        ));
                    }
                    for (i, row) in spell.spells.iter().enumerate() {
                        let sel = i as u8 == *cursor;
                        let marker = if sel { ">" } else { " " };
                        let line = format!("{} {} {:>2}MP", marker, row.name, row.mp_cost);
                        let color = if !row.affordable {
                            down_color
                        } else if sel {
                            white
                        } else {
                            dim
                        };
                        out.extend(ui::text_draws_for(
                            &font.layout_ascii(&line),
                            (MENU_X + 8, my),
                            color,
                        ));
                        my += 14;
                    }
                }
                SpellPhase::Targeting { picker, .. } => {
                    let line = match picker.state() {
                        PickerState::Cursor {
                            row: CursorRow::Enemy,
                            slot,
                        } => format!("cast -> target M{}", slot + 1),
                        PickerState::Cursor {
                            row: CursorRow::Ally,
                            slot,
                        } => format!("cast -> target P{}", slot + 1),
                        _ => "cast -> select target".to_string(),
                    };
                    out.extend(ui::text_draws_for(
                        &font.layout_ascii(&line),
                        (MENU_X, my),
                        white,
                    ));
                    my += 14;
                    out.extend(ui::text_draws_for(
                        &font.layout_ascii("Left/Right=move  Cross=confirm  Circle=back"),
                        (MENU_X, my),
                        dim,
                    ));
                }
                _ => {}
            }
        } else if let Some(menu) = &bw.battle_item_menu {
            out.extend(self.items_session_draws(assets, menu));
        } else if let Some(cmd) = &bw.battle_command {
            let mut my = MENU_Y;
            match &cmd.phase {
                CommandPhase::Menu { .. } => {
                    let header = format!("P{} - command:", cmd.actor + 1);
                    out.extend(ui::text_draws_for(
                        &font.layout_ascii(&header),
                        (MENU_X, my),
                        white,
                    ));
                    my += 16;
                    let cur = cmd.menu_command();
                    for c in BattleCommand::MENU {
                        let marker = if Some(c) == cur { ">" } else { " " };
                        let line = if c.enabled() {
                            format!("{} {}", marker, c.label())
                        } else {
                            format!("{} {} --", marker, c.label())
                        };
                        let color = if Some(c) == cur {
                            white
                        } else if c.enabled() {
                            dim
                        } else {
                            down_color
                        };
                        out.extend(ui::text_draws_for(
                            &font.layout_ascii(&line),
                            (MENU_X + 8, my),
                            color,
                        ));
                        my += 14;
                    }
                }
                CommandPhase::Targeting { command, picker } => {
                    let line = match picker.state() {
                        PickerState::Cursor {
                            row: CursorRow::Enemy,
                            slot,
                        } => format!("{} -> target M{}", command.label(), slot + 1),
                        PickerState::Cursor {
                            row: CursorRow::Ally,
                            slot,
                        } => format!("{} -> target P{}", command.label(), slot + 1),
                        _ => format!("{} -> select target", command.label()),
                    };
                    out.extend(ui::text_draws_for(
                        &font.layout_ascii(&line),
                        (MENU_X, my),
                        white,
                    ));
                    my += 14;
                    let hint = "Left/Right=move  Cross=confirm  Circle=back";
                    out.extend(ui::text_draws_for(
                        &font.layout_ascii(hint),
                        (MENU_X, my),
                        dim,
                    ));
                }
                _ => {}
            }
        }

        // Sparring-tutorial prompt box, placed by the retail style table
        // (`FUN_801F747C`): the engine-core box carries the style index, the
        // host supplies the measured text width and it returns the corner.
        // Drawn last so it sits over the menus, where retail's box lands too.
        if let Some(tbox) = bw.battle_tutorial_box() {
            let layouts: Vec<_> = tbox.text.lines().map(|l| font.layout_ascii(l)).collect();
            let width = layouts
                .iter()
                .map(|l| l.advance_x as i16)
                .max()
                .unwrap_or(0);
            let (bx, by) = tbox.position(width).unwrap_or((0x10, 0x0E));
            for (i, l) in layouts.iter().enumerate() {
                out.extend(ui::text_draws_for(
                    l,
                    (bx as i32, by as i32 + (i as i32) * 14),
                    white,
                ));
            }
            if tbox.waits_for_input {
                out.extend(ui::text_draws_for(
                    &font.layout_ascii("Cross=continue"),
                    (bx as i32, by as i32 + (layouts.len() as i32) * 14),
                    dim,
                ));
            }
        }
        out
    }
}

#[wasm_bindgen]
impl LegaiaRuntime {
    /// Enable / disable the live battle loop for subsequently-entered scenes
    /// (and the currently-running one). On by default - the play page rolls
    /// the disc's own step-driven encounters like retail. Off = the old
    /// walk-only behaviour (explore but never fight); the browser twin of
    /// running the native window without `--live-loop`.
    pub fn set_live_battles(&mut self, on: bool) {
        self.live_battles = on;
        if let Some(h) = self.scene_host.as_mut() {
            h.world.live_gameplay_loop = on;
            h.world.battle_player_driven = on;
        }
    }
}

/// Test-only probes for the disc-gated battle-overlay oracle
/// (`tests/battle_overlay_parity.rs`). Native-only so the wasm export surface
/// the page consumes stays exactly the player-facing API.
#[cfg(not(target_arch = "wasm32"))]
impl LegaiaRuntime {
    /// Force entry into a real scripted battle off the current scene's own
    /// MAN formation table (the op-`0x3E` boss-entry path), so a test reaches
    /// `SceneMode::Battle` without walking the RNG. Tries every plausible
    /// formation row and returns `true` once the world is in battle.
    pub fn debug_start_test_battle(&mut self) -> bool {
        let Some(host) = self.scene_host.as_mut() else {
            return false;
        };
        let armed = (0u8..32).any(|row| host.world.trigger_scripted_battle(row));
        if !armed {
            return false;
        }
        // The pending carrier battle is drained by the field tick.
        for _ in 0..4 {
            if self
                .scene_host
                .as_ref()
                .is_some_and(|h| h.world.mode == SceneMode::Battle)
            {
                return true;
            }
            let _ = self.tick_frame();
        }
        self.scene_host
            .as_ref()
            .is_some_and(|h| h.world.mode == SceneMode::Battle)
    }
}
