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
            let (hp_fill, mp_fill) = s.gauge_fill_indices();
            let meta = HudSlotMeta {
                is_party: s.is_party,
                alive: s.alive,
                hp: s.hp,
                hp_max: s.hp_max,
                mp: s.mp,
                mp_max: s.mp_max,
                ap_filled: s.ap_filled,
                ap_max: s.ap_max,
                hp_fill,
                mp_fill,
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
    /// The retail enemy target-name strip for a picker parked on the enemy
    /// row - the browser twin of the native window's
    /// `enemy_target_strip_draws`: rows deduplicated + labelled by the
    /// ported `FUN_801D9D3C` (`battle_enemy_target_rows`), placed by its
    /// centre/relax/clamp layout with the page font as the measurer, cursor
    /// row highlighted. `None` when the cursor is not on the enemy row or no
    /// monster is up.
    fn enemy_target_strip_draws(
        &self,
        font: &legaia_font::Font,
        picker: &legaia_engine_core::target_picker::TargetPickerSession,
        surface_w: u32,
        surface_h: u32,
    ) -> Option<Vec<TextDraw>> {
        use legaia_engine_core::target_picker::{CursorRow, PickerState, layout_enemy_menu_rows};
        let PickerState::Cursor {
            row: CursorRow::Enemy,
            slot,
        } = picker.state()
        else {
            return None;
        };
        let world = &self.scene_host.as_ref()?.world;
        let mut rows = legaia_engine_core::battle_hud::battle_enemy_target_rows(world);
        if rows.is_empty() {
            return None;
        }
        layout_enemy_menu_rows(&mut rows, |s| font.layout_ascii(s).advance_x as i16);
        let views: Vec<ui::EnemyTargetRowView<'_>> = rows
            .iter()
            .map(|r| ui::EnemyTargetRowView {
                label: &r.label,
                x: r.x,
                selected: slot >= r.first_slot && slot < r.first_slot + r.members,
            })
            .collect();
        Some(ui::enemy_target_menu_draws_for(
            font,
            &views,
            (surface_w, surface_h),
        ))
    }

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
        // One shared kernel with the native host (`BootSession::enter_field_live`
        // calls the same `World::arm_live_loop`). This used to be a
        // hand-maintained copy of that block and had already lost the scene
        // label and the Battle<->Field BGM swap - which is why battle music
        // was silent in the browser while it played natively.
        let mut opts = legaia_engine_core::live_loop::LiveLoopOpts::playable();
        opts.battle_bgm = self.battle_bgm;
        host.world.arm_live_loop(scene, &opts);
    }

    /// Per-tick battle presentation, called from [`LegaiaRuntime::tick_frame`]:
    /// the browser twin of the native `sync_battle_render` mode-edge latch +
    /// `drain_and_log_battle_events`. Cheap no-op while no scene is up.
    pub(crate) fn tick_battle_presentation(&mut self) {
        let Some(host) = self.scene_host.as_mut() else {
            return;
        };
        // Mode-edge latch: arm the ENCOUNTER! banner + build the battle 3D
        // render entering battle; drop both (and any stale popups) leaving -
        // the browser twin of the native `sync_battle_render` edge.
        let mode = host.world.mode;
        let prev = self.prev_scene_mode.replace(mode);
        if prev != Some(mode) {
            match (prev, mode) {
                (_, SceneMode::Battle) => {
                    self.encounter_banner =
                        Some((ENCOUNTER_BANNER_FRAMES, encounter_banner_label(&host.world)));
                    // 3D layer: battle VRAM + backdrop / grid / monster /
                    // party meshes ([`crate::play_battle_render`]). Ends the
                    // `host` borrow first - the build re-borrows the host.
                    self.enter_battle_render();
                }
                (Some(SceneMode::Battle), _) => {
                    self.encounter_banner = None;
                    self.battle_hud.clear_popups();
                    self.exit_battle_render();
                }
                _ => {}
            }
        }
        // Step the battle camera's idle orbit one sim tick (no-op outside
        // battle - the render state only exists while one is up).
        self.tick_battle_camera_web();
        let Some(host) = self.scene_host.as_mut() else {
            return;
        };
        // Drain world battle events. **Observation only** - the live battle
        // loop owns the gameplay fold and re-publishes the stream, so folding
        // again here would apply an art strike's HP twice.
        let _events = host.world.drain_battle_events();
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

    /// Out-of-battle battle presentation, in **surface pixels**: the
    /// post-battle spoils panel and the game-over panel. Both sit outside
    /// [`SceneMode::Battle`], which is why they are not part of
    /// [`Self::battle_overlay_draws`], and both use the same shared
    /// `engine-ui` builder + world model as the native window
    /// (`window/battle.rs`, `window/boot_cutscene.rs`).
    ///
    /// The "this scene rolls no encounters" hint does **not** belong here.
    /// The page treats a non-empty overlay as owning the frame - it clears
    /// the canvas, blits, and returns before the dialog-box layer - so a
    /// passive hint routed through this list would suppress every NPC
    /// dialogue for the first seconds of a town. The page reads
    /// [`Self::scene_rolls_encounters`] and prints its own notice instead.
    pub(crate) fn post_battle_overlay_draws(
        &self,
        assets: &crate::play_menu::PlayMenuAssets,
        surface_w: u32,
        surface_h: u32,
    ) -> Vec<TextDraw> {
        let Some(w) = self.scene_host.as_ref().map(|h| &h.world) else {
            return Vec::new();
        };
        let font = assets.font_ref();
        let mut out: Vec<TextDraw> = Vec::new();

        // Party wipe owns the frame. The panel is the live session's - see
        // `game_over_draws`.
        if let Some(s) = self.game_over.as_ref() {
            out.extend(self.game_over_draws(font, s));
            return out;
        }

        if let Some(banner) = w.battle_spoils_banner() {
            let view = ui::BattleSpoilsView {
                xp: banner.xp,
                gold: banner.gold,
                level_ups: &banner.level_ups,
                drops: &banner.drops,
            };
            let pen = (surface_w as i32 / 2 - 60, surface_h as i32 / 3);
            out.extend(ui::battle_spoils_draws_for(font, &view, pen));
        }

        out
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

        // Per-slot panels (party, retail-shaped with filled bars) + monster
        // rows, status strips and floating popups all come from the shared
        // builder, which carries the ported retail HP / MP readout-tint law
        // (`hp_bar_color_index` / `mp_bar_color_index`, FUN_800349EC /
        // FUN_80035EA8) and the gauge-fill law (`battle_gauge::gauge_colors`,
        // FUN_80046A20). Rows are fed from the `BattleHud` model, refreshed
        // each tick by the shared `sync_battle_hud_rows` fold. The filled
        // rects sample a solid-white font-atlas texel, which the page's
        // canvas blitter stretches + tints like any other glyph quad.
        let letters: Vec<Vec<u8>> = self
            .battle_hud
            .slots
            .iter()
            .map(|s| s.status_letters())
            .collect();
        out.extend(ui::battle_hud_draws_for(
            font,
            &ui::BattleHudFrame {
                slots: &battle_hud_slot_views(&self.battle_hud, &letters),
                popups: &battle_hud_popup_views(&self.battle_hud),
                log: &[],
                solid_src: ui::font_solid_src(font),
                surface: (surface_w, surface_h),
            },
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
                    // Enemy cursor: the retail dedup name strip
                    // (FUN_801D9D3C rows + layout). Ally / sweep states keep
                    // the text line.
                    if let Some(strip) =
                        self.enemy_target_strip_draws(font, picker, surface_w, surface_h)
                    {
                        out.extend(strip);
                    } else {
                        let line = match picker.state() {
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
                    }
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
                    if let Some(strip) =
                        self.enemy_target_strip_draws(font, picker, surface_w, surface_h)
                    {
                        out.extend(strip);
                    } else {
                        let line = match picker.state() {
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
                    }
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
                    if let Some(strip) =
                        self.enemy_target_strip_draws(font, picker, surface_w, surface_h)
                    {
                        out.extend(strip);
                    } else {
                        let line = match picker.state() {
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
                    }
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

    /// Set the Battle<->Field BGM swap track (`0` clears it), the browser twin
    /// of the native window's `--battle-bgm <id>`. The id is routed through
    /// the same director as field op-`0x35` starts, so it must resolve in the
    /// current scene's asset table.
    pub fn set_battle_bgm(&mut self, bgm_id: u16) {
        self.battle_bgm = (bgm_id != 0).then_some(bgm_id);
        if let Some(h) = self.scene_host.as_mut() {
            h.world.set_battle_bgm(self.battle_bgm);
        }
    }

    /// `true` when the current scene can produce a random encounter at all.
    /// The page shows a "no random encounters here" hint when it is `false`,
    /// so a town's designed silence doesn't read as a broken engine.
    pub fn scene_rolls_encounters(&self) -> bool {
        self.scene_host
            .as_ref()
            .is_some_and(|h| h.world.scene_encounters_rollable)
    }

    /// `true` while the game-over panel owns the frame. The page draws the
    /// panel and routes pad edges into it through
    /// [`Self::game_over_input`].
    pub fn is_game_over(&self) -> bool {
        self.game_over.is_some()
    }

    /// Whether the panel's **Continue** row is offered - `false` greys it.
    /// The browser's save data is the memory-card rack, so the scan is "does
    /// any inserted card hold a readable block", the twin of the native
    /// window's `scan_save_dir` probe.
    pub fn game_over_continue_enabled(&self) -> bool {
        self.game_over
            .as_ref()
            .map(|s| s.continue_enabled)
            .unwrap_or(false)
    }

    /// Drive the game-over panel one frame from an edge-triggered PSX pad
    /// word. Returns `""` while it runs, or the picked row once the player
    /// confirms: `"continue"`, `"retry"` or `"quit"`.
    ///
    /// Routing matches the native window's `BootUiState::GameOver` arm:
    /// **Continue** opens the retail save-select on the card rack (through
    /// the shared pause-menu Load row), **Retry** stands the party back up
    /// and returns to the scene, **Quit** hands back to the page, which
    /// re-runs the boot title.
    pub fn game_over_input(&mut self, edge: u16) -> String {
        use legaia_engine_core::game_over::{GameOverInput, GameOverOutcome};
        let Some(session) = self.game_over.as_mut() else {
            return String::new();
        };
        let _ = session.tick(GameOverInput::from_pad_edge(edge));
        let Some(outcome) = session.outcome() else {
            return String::new();
        };
        self.game_over = None;
        if let Some(h) = self.scene_host.as_mut() {
            h.world.game_over = false;
        }
        match outcome {
            GameOverOutcome::Continue => {
                self.play_menu_open_row("Load");
                "continue".to_string()
            }
            GameOverOutcome::Retry => {
                // Post-battle HP survives the fight, so a wiped party dropped
                // straight back into the field would simply re-wipe.
                if let Some(h) = self.scene_host.as_mut() {
                    h.world.revive_party_full();
                }
                "retry".to_string()
            }
            GameOverOutcome::Quit => "quit".to_string(),
        }
    }
}

impl LegaiaRuntime {
    /// Raise the game-over panel on the `World::game_over` edge - the browser
    /// twin of the native window's redraw-loop probe. Seeds
    /// `continue_enabled` off the card rack, exactly as the native side seeds
    /// it off `scan_save_dir`.
    pub(crate) fn poll_game_over(&mut self) {
        let wiped = self.scene_host.as_ref().is_some_and(|h| h.world.game_over);
        if !wiped || self.game_over.is_some() {
            return;
        }
        use legaia_engine_core::game_over::GameOverSession;
        let has_saves = (0..crate::cards::CARD_SLOTS)
            .any(|port| self.card_block_snapshots(port).iter().any(|b| b.present));
        self.game_over = Some(if has_saves {
            GameOverSession::new()
        } else {
            GameOverSession::with_no_save()
        });
    }

    /// The game-over panel's draws, off the live session.
    ///
    /// A named site rather than an inline branch so it can be paired against
    /// the native window's `game_over_draws`: both hosts must project the
    /// *session* here (cursor and the save-scan `continue_enabled`), not a
    /// pinned pair of literals, and the pen is the shared `engine-ui`
    /// constant so the panel cannot land in two places.
    fn game_over_draws(
        &self,
        font: &legaia_font::Font,
        s: &legaia_engine_core::game_over::GameOverSession,
    ) -> Vec<TextDraw> {
        ui::game_over_draws_for(font, s.cursor(), s.continue_enabled, ui::GAME_OVER_PEN)
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
