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
//! The battle's 3D layer under this overlay is no longer missing: the stage
//! dome, ground grid, monster meshes and assembled party battle forms build in
//! [`crate::play_battle_render`], and the effect layer on top of them - the
//! effect-script spawn drain, the effect-pool billboards, the `etmd` / move-VM
//! FX models, the summon creature and the target-select cursor tint - in
//! [`crate::play_battle_fx`].

use crate::runtime::LegaiaRuntime;
use legaia_engine_core::battle_hud::{
    BattleHud, DamagePopup, battle_active_actor, battle_plaque_element_badge,
    encounter_banner_enabled, encounter_banner_label, sync_battle_hud_rows,
};
use legaia_engine_core::world::SceneMode;
use legaia_engine_ui::screen_prim::{ScreenPrim, fade_prim};
use legaia_engine_ui::{self as ui, HudPopupView, HudSlotMeta, HudSlotView, SpriteDraw, TextDraw};
use wasm_bindgen::prelude::*;

/// Top-left anchor of the battle HUD's slot-row block, in surface pixels
/// (the native window's `BATTLE_HUD_PEN`).
const BATTLE_HUD_PEN: (i32, i32) = (8, 60);
/// Frames the encounter-transition banner stays on screen after a
/// `Field -> Battle` mode change (~1.5 s at the 60 Hz sim tick; the native
/// window's `ENCOUNTER_BANNER_FRAMES`).
const ENCOUNTER_BANNER_FRAMES: u16 = 90;
/// The retail 4x battle world scale (`play_battle_render::BATTLE_WORLD_SCALE`,
/// private to that module) - the actor stage the battle view-projection is
/// built for, so an actor origin has to be scaled before it is projected.
const BATTLE_WORLD_SCALE_LOCAL: f32 = 4.0;
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
fn battle_hud_slot_views(hud: &BattleHud) -> Vec<HudSlotView<'_>> {
    hud.slots
        .iter()
        .map(|s| {
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
                // The single retail-selected status element
                // (`FUN_8002C2E4`'s ladder over the packed `+0x16E` word)
                // plus the level its no-ailment arm draws.
                status_sprite: s.status_sprite(),
                level: s.level,
            };
            let name = if s.active { s.name.as_str() } else { "" };
            HudSlotView::from_plain(meta, name)
        })
        .collect()
}

/// Project the HUD model's popup queue into the shared builder's view type.
/// Borrow an engine-core battle-item-window model as the shared builder's
/// frame, handing it to `f` - the same CPS borrow glue the native window
/// carries (`window/hud.rs`); the projection itself is
/// `World::battle_item_menu_model`, shared by both hosts.
fn with_battle_item_frame<R>(
    model: &legaia_engine_core::inventory_use::BattleItemMenuModel,
    f: impl FnOnce(&legaia_engine_ui::battle_item_ui::BattleItemMenuFrame<'_>) -> R,
) -> R {
    use legaia_engine_ui::battle_item_ui as bii;
    let rows: Vec<bii::BattleItemRowView<'_>> = model
        .view
        .rows
        .iter()
        .map(|r| bii::BattleItemRowView {
            name: &r.name,
            count: r.count,
            admissible: r.admissible,
        })
        .collect();
    let target_rows: Vec<bii::BattleItemTargetView<'_>> = model
        .targets
        .as_ref()
        .map(|(rows, _)| {
            rows.iter()
                .map(|t| bii::BattleItemTargetView {
                    name: &t.name,
                    hp: t.hp,
                    hp_max: t.hp_max,
                    alive: t.alive,
                })
                .collect()
        })
        .unwrap_or_default();
    let frame = bii::BattleItemMenuFrame {
        rows: &rows,
        cursor: model.view.cursor_row,
        description: model.description.as_deref(),
        actor_name: &model.actor_name,
        targets: model
            .targets
            .as_ref()
            .map(|(_, cursor)| (target_rows.as_slice(), *cursor)),
    };
    f(&frame)
}

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
        // The strip shares a row band with a host-drawn prompt box, so it
        // steps up in whole 14 px rows off that box's rect rather than
        // overprinting it - the native window resolves the same collision.
        Some(ui::enemy_target_menu_draws_at(
            font,
            &views,
            (surface_w, surface_h),
            ui::enemy_target_menu_rows_y(self.battle_tutorial_stage_rect(font)),
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
        // Battle effect-script spawn requests (one per effect record the
        // per-actor effect-script walk consumed this tick). Routed into the
        // world's own spawn paths so the FX render layers
        // ([`crate::play_battle_fx`]) have something to draw - the browser
        // used to leave this queue undrained, which is why every cast, art
        // impact and enemy special was visually a no-op here.
        self.drain_battle_effect_spawns_web();
        let Some(host) = self.scene_host.as_mut() else {
            return;
        };
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
    /// A non-empty overlay clears the canvas before it blits, so a passive
    /// hint routed through this list would wipe whatever the frame had
    /// already painted for the first seconds of a town. The page reads
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

        // Party wipe owns the frame, and draws nothing on it: retail's next
        // frame after the wipe store is the title overlay fading in, so the
        // hand-off holds the frozen battle frame and adds no chrome. The
        // native host's `boot_ui_draws` arm is silent for the same reason.
        if self.game_over.is_some() {
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

    /// The **arts command-input** chrome for the browser play page:
    /// `(sprites, texts)`, both already in surface pixels. Empty unless a
    /// party member owns the pad in the retail-model entry session.
    ///
    /// Sibling of the native window's `arts_input_chrome_sprite_draws` -
    /// same shared builders, same baked atlas, same stage transform, so
    /// the two hosts cannot drift.
    pub(crate) fn arts_input_stage_draws(
        &self,
        font: &legaia_font::Font,
        chrome: Option<&ui::SaveMenuAtlasRects>,
        origin: (i32, i32),
        scale: u32,
    ) -> (Vec<ui::SpriteDraw>, Vec<TextDraw>) {
        use legaia_engine_core::arts_command_input::ArtsInputScreen as Sim;
        use legaia_engine_ui::arts_input as ai;
        let empty = (Vec::new(), Vec::new());
        let Some(bw) = self.scene_host.as_ref().map(|h| &h.world) else {
            return empty;
        };
        let Some(view) = bw.arts_input_view() else {
            return empty;
        };
        let frame = ai::ArtsInputFrame {
            buffer: view.buffer,
            spent: view.spent,
            pool: view.pool,
            pool_max: view.pool_max,
            plate_value: view.plate_value,
            list_page: view.list_page,
            // The two enums are separate types because `engine-ui` is a
            // leaf that does not link `engine-core`; the native window
            // carries the same three-line map.
            phase: match view.phase {
                Sim::Entering => ai::ArtsInputScreen::Entering,
                Sim::Review => ai::ArtsInputScreen::Review,
                Sim::BeginMenu { cursor } => ai::ArtsInputScreen::BeginMenu { cursor },
                Sim::Targeting => ai::ArtsInputScreen::Targeting,
            },
        };
        let mut sprites =
            ai::arts_input_chrome_draws(&ai::ArtsInputAtlasRects::BAKED, &frame, origin, scale);
        // The AP plate reuses the status screen's own AP-gauge pieces, so
        // it only draws when the page has the system-UI chrome loaded.
        if let Some(r) = chrome {
            sprites.extend(ai::arts_input_ap_plate_draws(
                &ai::ApPlateRects {
                    cap: r.gauge_cap,
                    trough: r.gauge_trough,
                    fill: r.gauge_fill,
                    box_: r.gauge_box,
                    digits: r.gauge_digits,
                },
                &frame,
                origin,
                scale,
            ));
        }
        (
            sprites,
            ai::arts_input_text_draws(font, &frame, origin, scale),
        )
    }

    /// Battle overlay text draws in **surface pixels**: HUD rows, the
    /// encounter banner, and the player-driven submenus. Empty outside
    /// [`SceneMode::Battle`]. Mirrors the native window's battle HUD block
    /// leg for leg (`window/hud.rs`).
    /// One battle-HUD frame from the shared builder: the party strip, the
    /// top-left plaque and the popups.
    ///
    /// Both halves come from one call so the page's two draw arrays cannot
    /// drift - the text half joins the surface-space battle text, the sprite
    /// half the system-UI atlas `sprites` array.
    pub(crate) fn battle_hud_frame_draws(
        &self,
        assets: &crate::play_menu::PlayMenuAssets,
        surface_w: u32,
        surface_h: u32,
    ) -> ui::BattleHudDraws {
        let font = assets.font_ref();
        // The badge cells the HUD blits, projected out of the baked atlas. A
        // `None` CELL inside it means that badge's palette source was outside
        // the slice the atlas was built from, and the HUD keeps its tag.
        let badges = assets.battle_badges();
        let banner = self.battle_banner_message(assets);
        let active = self
            .scene_host
            .as_ref()
            .and_then(|h| battle_active_actor(&h.world));
        // The arts-input session owns both halves of the park: it names the
        // actor whose full-width bar shows, and its being open is what sends
        // the roster panels off-screen.
        let parked = self
            .scene_host
            .as_ref()
            .is_some_and(|h| h.world.arts_input_active());
        let active = self
            .scene_host
            .as_ref()
            .and_then(|h| h.world.arts_input_actor())
            .and_then(|slot| active.map(|(_, name)| (slot, name)))
            .or_else(|| {
                self.scene_host
                    .as_ref()
                    .and_then(|h| battle_active_actor(&h.world))
            });
        ui::battle_hud_draws_for(
            font,
            &ui::BattleHudFrame {
                slots: &battle_hud_slot_views(&self.battle_hud),
                popups: &battle_hud_popup_views(&self.battle_hud),
                log: &[],
                solid_src: ui::font_solid_src(font),
                surface: (surface_w, surface_h),
                chrome: assets.chrome_rects(),
                // The plaque shares its top-left seat with the item window's
                // breadcrumb trail; retail parks it while that window is up
                // (battle_item_window capture), so one or the other draws.
                plaque: active
                    .as_ref()
                    .filter(|_| {
                        self.scene_host
                            .as_ref()
                            .is_none_or(|h| h.world.battle_item_menu.is_none())
                    })
                    .map(|(_, n)| n.as_str()),
                // The element badge the plaque wears in front of the name;
                // `None` draws the bare name.
                plaque_badge: self
                    .scene_host
                    .as_ref()
                    .and_then(|h| battle_plaque_element_badge(&h.world)),
                banner: banner.as_deref(),
                // The sparring-tutorial prompt is a box this page draws
                // itself, and its rect starts on the plaque's own content
                // pen - so while it is up the plaque must not draw, or two
                // text runs land on the same pixels. Same three conditions
                // the native window suppresses on.
                plaque_seat_taken: self.battle_tutorial_stage_rect(font).is_some()
                    || self.scene_host.as_ref().is_some_and(|h| {
                        h.world.current_dialog.is_some() || h.world.inline_dialogue.is_some()
                    }),
                badges: badges.as_ref(),
                // The same tutorial box that takes the plaque's seat also
                // sits on a party surface's row; naming its rect is what
                // parks the covered surface instead of letting two text
                // runs share the pixels.
                host_box: self.battle_tutorial_stage_rect(font),
                active_slot: active.as_ref().map(|(s, _)| *s),
                // Retail parks the status plate off-screen while a command
                // entry session owns the frame; the port emits no strip.
                input_session_parked: parked,
                diag: ui::diag_hud_enabled(),
            },
            BATTLE_HUD_PEN,
        )
    }

    /// The message holding retail's top-of-screen banner this frame, if any -
    /// the browser twin of the native window's `battle_banner_message`.
    ///
    /// The port's two battle messages are the level-up and Seru-capture
    /// lines, and retail draws exactly those in this widget. Deliberately NOT
    /// gated on `SceneMode::Battle`: the port grants XP after the mode has
    /// flipped back to Field, so a battle-mode gate would leave the widget
    /// wired and never drawn.
    ///
    /// `None` without the system-UI atlas - there is no frame to put a
    /// message in, so a chrome-less host keeps the loose pens instead.
    fn battle_banner_message(&self, assets: &crate::play_menu::PlayMenuAssets) -> Option<String> {
        assets.chrome_rects()?;
        let w = &self.scene_host.as_ref()?.world;
        if let Some(b) = &w.current_level_up_banner {
            // Name the character, not their roster ordinal - `P3` is an index
            // only this codebase knows. `char_id` is the ROSTER slot the
            // level-up applier wrote, so it indexes `roster.members`
            // directly (not the battle order). Twin of the native window's
            // `battle_banner_message`.
            let who = w
                .roster
                .members
                .get(b.char_id as usize)
                .map(|r| r.name())
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| format!("P{}", b.char_id + 1));
            return Some(format!(
                "LEVEL UP!  {who} -> LV {}\nHP +{}  MP +{}",
                b.new_level, b.hp_gained, b.mp_gained
            ));
        }
        w.current_capture_banner
            .as_ref()
            .and_then(|b| b.current_banner())
    }

    /// The live battle command surface projected into the shared chip-cluster
    /// view: one [`legaia_engine_ui::battle_command_ui::CommandChipView`] per
    /// chip of whichever phase is up, the cursor index, and the phase (which
    /// names the seats). `None` when no command surface owns the frame.
    ///
    /// The three phases are retail's three selection states - the round-open
    /// `Begin | Run` prompt, the four-arm command ring, and the
    /// `Auto | Command` attack-mode prompt.
    ///
    /// One projector feeds both halves of the cluster - the plate sprites
    /// and the labels - so the page's two draw arrays cannot disagree about
    /// whether the menu is up. Twin of the native window's method of the
    /// same name, with the same suppression rules.
    pub(crate) fn battle_command_menu_chips(
        &self,
    ) -> Option<(
        Vec<legaia_engine_ui::battle_command_ui::CommandChipView<'static>>,
        usize,
        legaia_engine_ui::battle_command_ui::ChipPhase,
    )> {
        use legaia_engine_core::battle_input::{
            AttackMode, BattleCommand, CommandPhase, RoundChoice,
        };
        use legaia_engine_ui::battle_command_ui::{ChipPhase, CommandChipView};
        let bw = self.scene_host.as_ref().map(|h| &h.world)?;
        if bw.mode != SceneMode::Battle {
            return None;
        }
        if bw.current_dialog.is_some() || bw.inline_dialogue.is_some() {
            return None;
        }
        if bw.arts_input_view().is_some()
            || bw.battle_arts_menu.is_some()
            || bw.battle_spell_menu.is_some()
            || bw.battle_item_menu.is_some()
        {
            return None;
        }
        let cmd = bw.battle_command.as_ref()?;
        let no_escape = bw.battle_no_escape;
        let chip = |label: &'static str, enabled: bool| CommandChipView { label, enabled };
        match cmd.phase {
            CommandPhase::RoundPrompt { cursor } => Some((
                RoundChoice::PROMPT
                    .iter()
                    .map(|c| chip(c.label(), !matches!(c, RoundChoice::Run) || !no_escape))
                    .collect(),
                cursor as usize,
                ChipPhase::RoundPrompt,
            )),
            CommandPhase::Menu { cursor } => Some((
                BattleCommand::MENU
                    .iter()
                    .map(|c| chip(c.label(), c.available(no_escape)))
                    .collect(),
                cursor as usize,
                ChipPhase::CommandRing,
            )),
            CommandPhase::AttackMode { cursor } => Some((
                AttackMode::PROMPT
                    .iter()
                    .map(|m| chip(m.label(), true))
                    .collect(),
                cursor as usize,
                ChipPhase::AttackMode,
            )),
            _ => None,
        }
    }

    /// The battle HUD's chrome sprites (strip + plaque lozenges, gold `HP` /
    /// green `MP` label cells) for the page's system-UI atlas array, plus
    /// the command menu's chip plates + D-pad glyph when a menu is up.
    /// Empty outside battle.
    pub(crate) fn battle_chrome_sprite_draws(
        &self,
        assets: &crate::play_menu::PlayMenuAssets,
        surface_w: u32,
        surface_h: u32,
    ) -> Vec<ui::SpriteDraw> {
        let in_battle = self
            .scene_host
            .as_ref()
            .is_some_and(|h| h.world.mode == SceneMode::Battle);
        if !in_battle {
            return Vec::new();
        }
        let mut out = self
            .battle_hud_frame_draws(assets, surface_w, surface_h)
            .sprites;
        // The battle item window's chrome (both packet-pinned 9-slice
        // windows, the breadcrumb tabs and the hand cursor) rides the same
        // atlas array as the rest of the menu chrome.
        if let (Some(rects), Some(model)) = (
            assets.chrome_rects(),
            self.scene_host
                .as_ref()
                .and_then(|h| h.world.battle_item_menu_model()),
        ) {
            let (origin, scale) =
                crate::play_menu::stage_transform(surface_w.max(1), surface_h.max(1));
            out.extend(with_battle_item_frame(&model, |frame| {
                legaia_engine_ui::battle_item_ui::battle_item_window_sprites(
                    assets.font_ref(),
                    rects,
                    frame,
                    origin,
                    scale,
                )
            }));
        }
        // The command chips sample the same blue plate 3-slice the party
        // bar does, so they ride this array rather than a second one.
        if let (Some(rects), Some((chips, cursor, phase))) = (
            assets.chrome_rects().and_then(|r| r.battle),
            self.battle_command_menu_chips(),
        ) {
            use legaia_engine_ui::battle_command_ui as bcu;
            let (origin, scale) =
                crate::play_menu::stage_transform(surface_w.max(1), surface_h.max(1));
            out.extend(bcu::battle_command_chip_sprites(
                &bcu::CommandChipAtlas::from_battle_chrome(&rects),
                &bcu::BattleCommandMenuFrame {
                    chips: &chips,
                    cursor: Some(cursor),
                    phase,
                },
                origin,
                scale,
            ));
        }
        out
    }

    pub(crate) fn battle_overlay_draws(
        &self,
        assets: &crate::play_menu::PlayMenuAssets,
        surface_w: u32,
        surface_h: u32,
    ) -> Vec<TextDraw> {
        use legaia_engine_core::battle_input::CommandPhase;
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

        // The retail party strip (one full-width lozenge per live member
        // across the stage bottom), the top-left plaque and the floating
        // popups all come from the shared builder. Its text half lands here;
        // its chrome sprites go out of `battle_chrome_sprite_draws` into the
        // page's sprite array. Numerals carry the ported retail readout-tint
        // law (`hp_bar_color_index` / `mp_bar_color_index`, FUN_800349EC /
        // FUN_80035EA8). Rows are fed from the `BattleHud` model, refreshed
        // each tick by the shared `sync_battle_hud_rows` fold.
        out.extend(
            self.battle_hud_frame_draws(assets, surface_w, surface_h)
                .text,
        );

        // Floating value readout: the numeral a landed hit throws, over the
        // struck actor. Layout is the packet-pinned
        // `engine-vm::battle_value_readout`; this page draws it through the
        // font fallback rather than the retail 24x24 cells, because its
        // overlay list is font-atlas quads (the native window has a
        // screen-space VRAM sink and draws the real art).
        out.extend(self.battle_value_readout_draws(font, surface_w, surface_h));

        // Encounter-transition banner: centred "ENCOUNTER!" over the
        // formation label, shown for the opening frames of the battle. A port
        // invention with no retail counterpart - retail's Field -> Battle edge
        // draws no banner at all - so it is gated off by default and only
        // appears under `LEGAIA_DIAG_HUD` (`encounter_banner_enabled`).
        if let Some((_, label)) = self
            .encounter_banner
            .as_ref()
            .filter(|_| encounter_banner_enabled())
        {
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
        } else if bw.battle_item_menu.is_some() {
            // Retail's item window (state 0x3C): the packet-pinned list +
            // description windows with breadcrumbs and the hand cursor.
            // Same engine-core projection + engine-ui builder the native
            // window uses; the chrome sprites ride
            // `battle_chrome_sprite_draws`.
            if let Some(model) = bw.battle_item_menu_model() {
                let (origin, scale) =
                    crate::play_menu::stage_transform(surface_w.max(1), surface_h.max(1));
                out.extend(with_battle_item_frame(&model, |frame| {
                    legaia_engine_ui::battle_item_ui::battle_item_window_text(
                        font, frame, origin, scale,
                    )
                }));
            }
        } else if let Some(cmd) = &bw.battle_command {
            let mut my = MENU_Y;
            match &cmd.phase {
                CommandPhase::RoundPrompt { .. }
                | CommandPhase::Menu { .. }
                | CommandPhase::AttackMode { .. } => {
                    // Retail's command surfaces are chip clusters, not
                    // lists: the round-open `Begin | Run` pair, the
                    // packet-pinned four-arm diamond, and the
                    // `Auto | Command` pair on the diamond's own arms.
                    // Same shared builder + same projector the native
                    // window uses, so the two hosts cannot drift; the
                    // plates go out in `battle_chrome_sprite_draws`.
                    if let Some((chips, cursor, phase)) = self.battle_command_menu_chips() {
                        use legaia_engine_ui::battle_command_ui as bcu;
                        let (origin, scale) =
                            crate::play_menu::stage_transform(surface_w.max(1), surface_h.max(1));
                        out.extend(bcu::battle_command_chip_text(
                            font,
                            &bcu::BattleCommandMenuFrame {
                                chips: &chips,
                                cursor: Some(cursor),
                                phase,
                            },
                            origin,
                            scale,
                        ));
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

        // The sparring-tutorial prompt box is NOT part of this list: its rect
        // is in retail's 320x240 stage space while everything above is in
        // surface pixels, and it is a framed window rather than loose text.
        // It is built by `battle_tutorial_stage_draws` /
        // `battle_tutorial_chrome_draws` and folded into the stage-scaled
        // group in `play_overlay_draws_json`.
        out
    }

    /// The live sparring-tutorial prompt's box rect in 320x240 **stage**
    /// pixels, or `None` when no box is up.
    ///
    /// The width is measured in this host's font (retail measures it with
    /// `FUN_80035F04`) and `engine-core` applies the emitter's placement +
    /// sizing arithmetic (`FUN_801F747C`). Shared by the text and chrome
    /// layers so the frame and the rows cannot disagree - the native window's
    /// `battle_tutorial_stage_rect` twin.
    /// The frame's floating value readout, as font-fallback draws in surface
    /// pixels.
    ///
    /// One run of digit cells per live damage popup, seated over the struck
    /// actor's projected screen position and laid out by
    /// `legaia_engine_vm::battle_value_readout::value_cells` - the same model
    /// the native window draws with retail's own 24x24 cells. Only the
    /// newest popup per actor draws: retail's readout is a per-slot value
    /// window, and two runs centred on one point interleave unreadably.
    pub(crate) fn battle_value_readout_draws(
        &self,
        font: &legaia_font::Font,
        surface_w: u32,
        surface_h: u32,
    ) -> Vec<TextDraw> {
        use legaia_engine_vm::battle_value_readout as vr;
        if surface_w == 0 || surface_h == 0 || self.battle_hud.popups.is_empty() {
            return Vec::new();
        }
        let Some(world) = self.scene_host.as_ref().map(|h| &h.world) else {
            return Vec::new();
        };
        // The FX camera: the page's battle view-projection with the retail 4x
        // world scale composed on, i.e. the native `fx_cam`.
        let vp = self.play_battle_camera_vp(surface_w as f32 / surface_h as f32);
        if vp.len() != 16 {
            return Vec::new();
        }
        // The stage transform the rest of the battle chrome uses.
        let scale = (surface_w / 320).min(surface_h / 240).clamp(1, 4);
        let origin = (
            (surface_w as i32 - 320 * scale as i32) / 2,
            (surface_h as i32 - 240 * scale as i32) / 2,
        );
        let mut newest: Vec<&legaia_engine_core::battle_hud::DamagePopup> = Vec::new();
        for p in &self.battle_hud.popups {
            if p.status.is_some() {
                continue;
            }
            match newest.iter_mut().find(|q| q.slot == p.slot) {
                Some(q) if q.frames_remaining >= p.frames_remaining => {}
                Some(q) => *q = p,
                None => newest.push(p),
            }
        }
        let mut out = Vec::new();
        for p in newest {
            let Some(a) = world.actors.get(usize::from(p.slot)) else {
                continue;
            };
            // Column-major mat4 times the scaled actor origin.
            let w = [
                a.move_state.world_x as f32 * BATTLE_WORLD_SCALE_LOCAL,
                a.move_state.world_y as f32 * BATTLE_WORLD_SCALE_LOCAL,
                a.move_state.world_z as f32 * BATTLE_WORLD_SCALE_LOCAL,
                1.0,
            ];
            let mut clip = [0.0f32; 4];
            for (i, c) in clip.iter_mut().enumerate() {
                *c = (0..4).map(|j| vp[j * 4 + i] * w[j]).sum();
            }
            if clip[3] <= 0.01 {
                continue;
            }
            let ax = ((clip[0] / clip[3] * 0.5 + 0.5) * 320.0) as i32;
            let ay = ((0.5 - clip[1] / clip[3] * 0.5) * 240.0) as i32;
            let age = p.frames_total.saturating_sub(p.frames_remaining);
            let cells: Vec<ui::ValueCellView> = vr::value_cells(p.amount, ax, ay - 26, age)
                .into_iter()
                .map(|c| ui::ValueCellView {
                    digit: c.digit,
                    x: c.x,
                    y: c.y,
                    w: c.w,
                    h: c.h,
                })
                .collect();
            out.extend(ui::battle_value_readout_draws_for(
                font,
                &cells,
                ui::VALUE_READOUT_FALLBACK_COLOR,
                origin,
                scale,
            ));
        }
        out
    }

    pub(crate) fn battle_tutorial_stage_rect(
        &self,
        font: &legaia_font::Font,
    ) -> Option<(i32, i32, i32, i32)> {
        let tbox = self.scene_host.as_ref()?.world.battle_tutorial_box()?;
        let width = ui::battle_tutorial_text_width(font, &tbox.text);
        let (x, y, w, h) = tbox.rect(width)?;
        Some((x as i32, y as i32, w as i32, h as i32))
    }

    /// Tutorial prompt rows in stage pixels, at the rect origin on the retail
    /// 14-px pitch.
    pub(crate) fn battle_tutorial_stage_draws(
        &self,
        font: &legaia_font::Font,
        has_chrome: bool,
    ) -> Vec<TextDraw> {
        let Some(rect) = self.battle_tutorial_stage_rect(font) else {
            return Vec::new();
        };
        let Some(tbox) = self
            .scene_host
            .as_ref()
            .and_then(|h| h.world.battle_tutorial_box())
        else {
            return Vec::new();
        };
        let mut out = ui::battle_tutorial_text_draws_for(font, &tbox.text, rect);
        // Without the system-UI atlas there is no frame and no advance hand,
        // so keep a plain confirm hint as the only affordance a waiting box
        // would otherwise have.
        if tbox.waits_for_input && !has_chrome {
            let lines = tbox.text.lines().count() as i32;
            out.extend(ui::text_draws_for(
                &font.layout_ascii("Cross=continue"),
                (rect.0, rect.1 + lines * 14),
                [0.75, 0.75, 0.8, 1.0],
            ));
        }
        out
    }

    /// Tutorial prompt-box chrome: the same gradient fill + gold 9-slice
    /// frame the dialog reading box wears, at the emitter's rect.
    pub(crate) fn battle_tutorial_chrome_draws(
        &self,
        font: &legaia_font::Font,
        rects: &ui::SaveMenuAtlasRects,
        origin: (i32, i32),
        scale: u32,
    ) -> Vec<SpriteDraw> {
        let Some(rect) = self.battle_tutorial_stage_rect(font) else {
            return Vec::new();
        };
        let waits = self
            .scene_host
            .as_ref()
            .and_then(|h| h.world.battle_tutorial_box())
            .is_some_and(|b| b.waits_for_input);
        ui::battle_tutorial_chrome_draws_for(rects, rect, waits, origin, scale)
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

    /// The formation rows the current scene registered, as a JSON array of
    /// ids (`[]` with no scene up). The list the native window prints when
    /// `--battle <ROW>` names a row the scene never registered, exposed so a
    /// page - or a headless driver - can pick a row that exists instead of
    /// guessing.
    pub fn debug_formation_rows(&self) -> String {
        let ids = self
            .scene_host
            .as_ref()
            .map(|h| h.world.registered_formation_ids())
            .unwrap_or_default();
        let body: Vec<String> = ids.iter().map(|i| i.to_string()).collect();
        format!("[{}]", body.join(","))
    }

    /// Arm a deterministic battle instead of waiting for a step roll - the
    /// browser twin of the native window's `--battle <ROW|first>`.
    ///
    /// `row >= 0` names one of the scene's MAN encounter formation rows (the
    /// id space the region roll itself produces); `row < 0` takes the lowest
    /// row that carries monsters, i.e. `--battle first`. The fight is armed
    /// through [`legaia_engine_core::world::World::force_encounter`], which
    /// hands the row to the encounter session's transition state machine
    /// exactly as a region roll does, so the intro, the BGM swap and the
    /// battle-load path are the ordinary ones and what the page shows is what
    /// an organic encounter shows.
    ///
    /// Like the native flag it turns the live loop on: the transition is
    /// drained by the live field tick, so an armed fight cannot open without
    /// it. Returns `true` when the row resolved and the transition armed - the
    /// page still has to tick for the `Field -> Battle` edge to land.
    ///
    /// This is the page's only way into a fight in a scene that rolls none
    /// (every town), which is what previously made the browser battle screen
    /// unreachable to a headless driver: `debug_start_test_battle` is
    /// `#[cfg(not(target_arch = "wasm32"))]` and never crossed into the
    /// bundle.
    pub fn debug_force_battle(&mut self, row: i32) -> bool {
        let Some(host) = self.scene_host.as_mut() else {
            return false;
        };
        let resolved = if row < 0 {
            host.world.first_rollable_formation_id()
        } else {
            u16::try_from(row).ok()
        };
        let Some(id) = resolved else {
            crate::console_log(&format!(
                "debug_force_battle: no formation resolved in '{}' (registered rows: {:?})",
                host.world.active_scene_label,
                host.world.registered_formation_ids()
            ));
            return false;
        };
        if !host.world.live_gameplay_loop {
            host.world.live_gameplay_loop = true;
            host.world.battle_player_driven = true;
        }
        host.world.force_encounter(id)
    }

    /// `true` while the party-wipe hand-off owns the frame. The page stops
    /// feeding the pad into the world and drives
    /// [`Self::game_over_input`] instead.
    pub fn is_game_over(&self) -> bool {
        self.game_over.is_some()
    }

    /// Drive the party-wipe hand-off one frame. Returns `""` while it holds,
    /// then `"quit"` once - the page's cue to stop the view and re-run the
    /// boot title.
    ///
    /// The `_edge` pad word is accepted and **ignored**, and the name is kept
    /// because it is the page's ABI. Retail asks the player nothing here: the
    /// wipe arm of `FUN_8003AEB0` stores `game_mode = 0x16` (22, CARD INIT)
    /// and `_DAT_8007BB00 = 1` at `0x8003B5D0` / `0x8003B5E0` and the title
    /// overlay takes the screen. Reading a button here would be the deleted
    /// Continue / Retry / Quit panel growing back. Routing matches the native
    /// window's `BootUiState::GameOver` arm exactly - one destination.
    pub fn game_over_input(&mut self, _edge: u16) -> String {
        use legaia_engine_core::game_over::GameOverOutcome;
        let Some(session) = self.game_over.as_mut() else {
            return String::new();
        };
        session.tick();
        let Some(GameOverOutcome::ReturnToTitle) = session.outcome() else {
            return String::new();
        };
        self.game_over = None;
        if let Some(h) = self.scene_host.as_mut() {
            h.world.game_over = false;
        }
        "quit".to_string()
    }
}

impl LegaiaRuntime {
    /// Raise the return-to-title hand-off on the `World::game_over` edge -
    /// the browser twin of the native window's redraw-loop probe. Nothing is
    /// scanned off the card rack any more: the hand-off has one destination
    /// whether or not a save exists, because retail's wipe arm has one exit
    /// store.
    pub(crate) fn poll_game_over(&mut self) {
        let wiped = self.scene_host.as_ref().is_some_and(|h| h.world.game_over);
        if !wiped || self.game_over.is_some() {
            return;
        }
        self.game_over = Some(legaia_engine_core::game_over::GameOverSession::new());
    }
}

/// Disc-gated runtime oracle for the drawn battle-HUD surface + the enemy
/// target strip, driven through a REAL scripted battle off the scene MAN.
/// Skips + passes without `LEGAIA_DISC_BIN`. Set `LEGAIA_HUD_DUMP=<path>` to
/// also write the raw overlay JSON for external inspection.
#[cfg(all(test, not(target_arch = "wasm32")))]
mod live_hud_tests {
    use super::*;

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
                .as_ref()
                .is_some_and(|h| h.world.battle_command.is_some());
            if open {
                break;
            }
            rt.tick_frame().expect("tick");
        }
        assert!(
            rt.scene_host
                .as_ref()
                .is_some_and(|h| h.world.battle_command.is_some()),
            "player-driven battle opens the command menu"
        );

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
            in_band(sprites, PANEL_ROW, PANEL_BOT) > 0
                || in_band(sprites, BAR_ROW, BAR_ROW + 60) > 0,
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
            use legaia_engine_core::battle_input::{AttackMode, BattleCommand, CommandPhase};
            let tap = |rt: &mut LegaiaRuntime, b: legaia_engine_core::input::PadButton| {
                rt.set_pad(b.mask());
                rt.tick_frame().expect("tick");
                rt.set_pad(0);
                rt.tick_frame().expect("tick");
            };
            use legaia_engine_core::input::PadButton;
            for _ in 0..40 {
                let phase = rt
                    .scene_host
                    .as_ref()
                    .and_then(|h| h.world.battle_command.as_ref())
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
            let w = &rt.scene_host.as_ref().expect("host").world;
            let targeting = matches!(
                w.battle_command.as_ref().map(|c| &c.phase),
                Some(legaia_engine_core::battle_input::CommandPhase::Targeting { .. })
            );
            if !targeting {
                let pc = w.party_count.clamp(1, 3) as usize;
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
                    w.battle_command
                        .as_ref()
                        .map(|c| std::mem::discriminant(&c.phase)),
                    w.current_dialog.is_some(),
                    w.inline_dialogue.is_some(),
                );
            }
            assert!(targeting, "Cross on Attack opens the target picker");
            let rows = legaia_engine_core::battle_hud::battle_enemy_target_rows(w);
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
        // 960x720 -> stage scale 3, origin (0,0); the strip draws at stage
        // Y 166 (engine-ui ENEMY_MENU_STAGE_Y) -> surface y 498.
        let strip_glyphs = v["texts"]
            .as_array()
            .expect("texts array")
            .iter()
            .filter(|t| t["dst"][1] == 498)
            .count();
        assert!(
            strip_glyphs > 0,
            "enemy target strip glyphs land at the strip band"
        );
        if let Ok(path) = std::env::var("LEGAIA_HUD_DUMP") {
            std::fs::write(path, &json).expect("write hud dump");
        }
    }
}

// ---------------------------------------------------------------------------
// Screen-space PSX primitives (the field-to-battle transition)
// ---------------------------------------------------------------------------

impl LegaiaRuntime {
    /// This frame's screen-space PSX primitives, in the OT buckets retail
    /// links them at. Empty whenever no field-to-battle transition is running,
    /// which is what keeps the page's pass a two-line early-out.
    ///
    /// **What is here, and what is not.** The simulation half of the
    /// transition is shared and already live on this host:
    /// `World::tick_encounter` runs `tick_transition` every frame the
    /// encounter session sits in its `Transition` phase, so the clock, the
    /// BGM swap and the battle that opens are the native window's. The *style
    /// body* - the confetti, the shattering tiles, the curtain strips, the
    /// swirl fan - is emitted by `legaia_engine_render::battle_intro`, which
    /// this crate cannot link (wgpu), and every one of those styles textures
    /// its geometry with a **captured field frame** that nothing on this page
    /// reads back. So what the browser draws is the layer that needs neither:
    /// the full-screen fade, resolved by the same shared
    /// [`legaia_engine_vm::battle_intro_styles::intro_fade`] ramp and built by
    /// the same shared [`legaia_engine_ui::screen_prim::fade_prim`] packet the
    /// native window emits.
    ///
    /// The native emitter also pushes a `backdrop_prim` - an opaque black
    /// display-rect quad standing in for "retail's field renderer is not in
    /// the ordering table". That one is deliberately **not** emitted here: it
    /// is only correct underneath a style body that reconstructs the frame
    /// from the capture, and on its own it would black the field out for the
    /// whole 132-frame window. Drawing the fade over the still-rendering field
    /// is the honest subset.
    ///
    /// See `docs/tooling/host-drift.md` for the capability ledger.
    pub(crate) fn battle_intro_screen_prims(&self) -> Vec<ScreenPrim> {
        use legaia_engine_core::encounter::EncounterPhase;
        use legaia_engine_vm::battle_intro_styles::{
            IntroStyleInputs, intro_fade, select_intro_style,
        };

        let Some(host) = self.scene_host.as_ref() else {
            return Vec::new();
        };
        let phase = host.world.encounter.as_ref().map(|s| s.phase());
        let Some(EncounterPhase::Transition { roll, .. }) = phase else {
            return Vec::new();
        };
        let Some(entity) = host.world.battle_intro else {
            return Vec::new();
        };
        let total = host
            .world
            .encounter
            .as_ref()
            .map(|s| i32::from(s.transition_frames))
            .unwrap_or(0);
        // The three selector inputs, resolved exactly as the native window's
        // `arm_battle_intro` resolves them: the formation's **first monster
        // id** (not the row index - reading the row is what made every
        // id-keyed override unreachable), the row's own per-battle flags byte
        // (`DAT_8007BD60` bit `0x80`, the only bit the selector reads), and
        // the scene's PROT base (`DAT_80084540`).
        let def = host.world.formation_table.formation(roll.formation_id);
        let slot0 = def
            .and_then(|d| d.slots.first())
            .map(|s| s.monster_id as u8)
            .unwrap_or(roll.formation_id as u8);
        let battle_flags = def.map(|d| d.per_battle_flags()).unwrap_or(0);
        let scene_index = host.scene.as_ref().map(|s| s.start).unwrap_or(0);
        let choice = select_intro_style(&IntroStyleInputs {
            battle_flags,
            formation_slot0: slot0,
            scene_index,
        });
        // `None` until the ramp starts - retail branches straight to the
        // epilogue rather than emitting a level-zero quad.
        let Some(f) = intro_fade(
            choice.style,
            choice.sub_style,
            i32::from(entity.elapsed),
            total,
        ) else {
            return Vec::new();
        };
        vec![fade_prim(f.rgb, f.abr, u32::from(f.layer))]
    }

    /// This frame's primitives already ordered and turned into a drawable
    /// vertex/index/run triple by the **shared** builder.
    ///
    /// The page never sees the primitive list, only its output: the
    /// ordering-table walk (farthest bucket first, LIFO within a bucket) is
    /// baked into the index buffer before the data crosses the WASM boundary,
    /// so there is no second place for a host to order these and no way for
    /// the two hosts to disagree about it.
    fn screen_prim_geometry(&self) -> legaia_engine_ui::screen_prim::OverlayGeometry {
        let prims = self.battle_intro_screen_prims();
        if prims.is_empty() {
            return Default::default();
        }
        legaia_engine_ui::screen_prim::build_geometry(
            &prims,
            legaia_engine_ui::screen_prim::PSX_DISPLAY_W as u32,
            legaia_engine_ui::screen_prim::PSX_DISPLAY_H as u32,
        )
    }
}

#[wasm_bindgen]
impl LegaiaRuntime {
    /// How many screen-space PSX primitives this frame carries. `0` is the
    /// page's early-out: the three accessors below each rebuild the geometry,
    /// which is cheap for a handful of quads and pointless for none.
    pub fn play_screen_prim_count(&self) -> u32 {
        self.battle_intro_screen_prims().len() as u32
    }

    /// The screen-prim vertex stream as raw bytes, in the shared
    /// `ScreenVertex` layout: stride 44, `pos: vec2<f32>` (already NDC) at 0,
    /// `uv: vec2<f32>` at 8, `cba_tsb: vec2<u32>` at 16, `color: vec4<f32>` at
    /// 24, `flags: u32` at 40 (bit 0 = textured). The same bytes the native
    /// renderer maps into its wgpu vertex buffer.
    pub fn play_screen_prim_vertex_bytes(&self) -> Vec<u8> {
        self.screen_prim_geometry().vertex_bytes()
    }

    /// The screen-prim triangle index buffer, already in ordering-table draw
    /// order.
    pub fn play_screen_prim_indices(&self) -> Vec<u32> {
        self.screen_prim_geometry().indices
    }

    /// The screen-prim run table, flattened to
    /// `[class_code, index_start, index_count]` triples. `class_code` is
    /// `legaia_engine_ui::screen_prim::BlendClass::code`: `0` = opaque,
    /// `1 + abr_mode` = semi-transparent - so `0` stays reserved for "no
    /// blending" and an ABR-0 (`0.5B + 0.5F`) run cannot be read as one.
    pub fn play_screen_prim_runs(&self) -> Vec<u32> {
        self.screen_prim_geometry().run_words()
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
        // The pending carrier battle is drained by the field tick, and the
        // field-to-battle intro transition holds the mode in Field for its
        // 132 display frames before the flip.
        for _ in 0..220 {
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

#[cfg(test)]
mod tests {
    use legaia_engine_core::battle_tutorial::BoxStyle;

    /// The tutorial box rect is in 320x240 stage space, so the page's own
    /// stage transform must carry it - the same law the native window applies
    /// via `save_select_stage`. Pinned at the browser play canvas's real
    /// shape (4:3, not an integer multiple of the stage) because that is
    /// exactly where reading the rect as surface pixels went wrong.
    #[test]
    fn tutorial_rect_rides_the_page_stage_transform() {
        let (x, y, w, h) = BoxStyle::from_raw(0).unwrap().box_rect(177, 2);
        assert_eq!((x, y, w, h), (0x10, 0x0E, 177, 24));

        let ((ox, oy), scale) = crate::play_menu::stage_transform(862, 646);
        assert_eq!((ox, oy, scale), (111, 83, 2));
        // Placed on the stage the box is inset by the letterbox plus the
        // scaled margin; read as surface pixels it would sit at (16, 14) -
        // hard against the corner.
        assert_eq!(ox + x as i32 * scale as i32, 143);
        assert_eq!(oy + y as i32 * scale as i32, 111);

        // And it stays inside the stage at every supported scale.
        for (sw, sh) in [(320u32, 240u32), (960, 720), (1920, 1080)] {
            let ((ox, oy), scale) = crate::play_menu::stage_transform(sw, sh);
            let right = ox + (x as i32 + w as i32) * scale as i32;
            let bottom = oy + (y as i32 + h as i32) * scale as i32;
            assert!(right <= sw as i32, "{sw}x{sh}: right {right}");
            assert!(bottom <= sh as i32, "{sw}x{sh}: bottom {bottom}");
        }
    }
}
