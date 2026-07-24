//! `KeyboardInput` window-event handler, extracted from `event_handler.rs`
//! (mechanical split; behavior-preserving).

use super::super::*;

impl PlayWindowApp {
    pub(super) fn handle_keyboard(
        &mut self,
        evl: &ActiveEventLoop,
        code: KeyCode,
        state: ElementState,
    ) {
        if matches!(code, KeyCode::Escape) && state == ElementState::Pressed {
            if let Some(log) = self.record_log.as_mut()
                && let Err(e) = log.flush()
            {
                log::error!("record: flush on Escape failed: {e:#}");
            }
            evl.exit();
            return;
        }
        // Dev affordance: spawn a debug effect marker at the player so
        // the effect-pool render bridge can be exercised by hand
        // before the runtime effect catalog is wired into battle-enter.
        if matches!(code, KeyCode::KeyE)
            && state == ElementState::Pressed
            && !self.boot_ui.is_active()
        {
            let pos = self
                .session
                .host
                .world
                .actors
                .iter()
                .find(|a| a.active)
                .map(|a| {
                    [
                        a.move_state.world_x as f32,
                        a.move_state.world_y as f32,
                        a.move_state.world_z as f32,
                    ]
                })
                .unwrap_or([0.0, 0.0, 0.0]);
            self.session.host.world.spawn_debug_effect(pos);
            return;
        }
        // `F`: seat a synthetic *Tail Fire* effect carrying its `etmd`
        // 3D model (global TMD pool index 4, textured by the resident
        // `etim` texels) at the first active actor, exercising the
        // model render path with a fixed model index. The data-driven
        // effect-id -> etmd-model selection is decoded (move-power
        // record `+0x12`/`+0x16` effect-id lists -> the `0x801F6324`
        // prototype table -> record `model_sel` -> `global_tmd_pool
        // [model_sel + 3]`; `legaia_asset::move_power::EffectListEntry`
        // + `World::spawn_move_fx`, exercised by the `H` key); `F` is
        // just the simpler fixed-model dev hand-spawn.
        if matches!(code, KeyCode::KeyF)
            && state == ElementState::Pressed
            && !self.boot_ui.is_active()
        {
            let pos = self
                .session
                .host
                .world
                .actors
                .iter()
                .find(|a| a.active)
                .map(|a| {
                    [
                        a.move_state.world_x as f32,
                        a.move_state.world_y as f32,
                        a.move_state.world_z as f32,
                    ]
                })
                .unwrap_or([0.0, 0.0, 0.0]);
            // Prefer the real Gimard *Tail Fire* mesh from the PROT
            // 0871 effect-model library when it's resident; fall back
            // to the PROT 0874 §0 preview stand-in otherwise.
            let model_index = if self
                .session
                .host
                .world
                .global_tmd(legaia_engine_core::scene::GIMARD_TAIL_FIRE_MODEL_INDEX as i16)
                .is_some()
            {
                legaia_engine_core::scene::GIMARD_TAIL_FIRE_MODEL_INDEX
            } else {
                legaia_engine_core::scene::ETMD_TAIL_FIRE_MODEL_INDEX
            };
            self.session
                .host
                .world
                .spawn_debug_effect_model(pos, model_index);
            return;
        }
        // `G`: debug-spawn the Gimard *Burning Attack* summon scene-
        // graph (extraction PROT 0903). Loads the stager overlay, parses
        // records, and seats a `SummonScene` at the first active actor;
        // it then animates each frame via `tick_summon` (the move VM)
        // and renders through the summon-part draw block below. Not the
        // production cast-band trigger (state 0x29) -- a hand-spawn to
        // exercise the driver, like the `F`-key static effect spawn.
        if matches!(code, KeyCode::KeyG)
            && state == ElementState::Pressed
            && !self.boot_ui.is_active()
        {
            // In battle, debug-spawn the faithful summon creature
            // (Gimard, 0x81) through the battle-creature render path.
            if self.session.host.world.mode == SceneMode::Battle {
                self.spawn_summon_creature(0x81);
                return;
            }
            const PROT_GIMARD_SUMMON_STAGER: u32 = 903;
            let origin = self
                .session
                .host
                .world
                .actors
                .iter()
                .find(|a| a.active)
                .map(|a| {
                    [
                        a.move_state.world_x,
                        a.move_state.world_y,
                        a.move_state.world_z,
                    ]
                })
                .unwrap_or([0, 0, 0]);
            // Read the stager's TOC-gap LBA footprint, not the
            // indexed sub-region: the stager `.BIN`s over-read into
            // the next entry, so the spawn-site pointer table must be
            // parsed against the trimmed window (mirrors the disc-gated
            // `summon_overlay_real` test's `unique_content_len` trim).
            match self
                .session
                .host
                .index
                .entry_bytes_lba_footprint(PROT_GIMARD_SUMMON_STAGER)
            {
                Ok(bytes) => {
                    let overlay = legaia_asset::summon_overlay::parse(
                        &bytes,
                        legaia_asset::summon_overlay::SUMMON_OVERLAY_LINK_BASE,
                    );
                    self.session.host.world.spawn_summon(
                        &overlay,
                        &bytes,
                        legaia_engine_core::scene::GIMARD_TAIL_FIRE_MODEL_INDEX,
                        origin,
                    );
                    log::info!(
                        "spawned Gimard summon: {} parts at {origin:?}",
                        overlay.parts.len()
                    );
                }
                Err(e) => log::warn!("summon spawn: read summon stager PROT entry: {e:#}"),
            }
            return;
        }
        // `H`: debug-spawn a battle move's effect-FX scene-graph (the
        // move-power `0x801f6324` prototype records, summon-format
        // move-VM parts spawned through `FUN_80021B04`). Battle only -
        // the move-power table + overlay are battle-context. Move id
        // 0x06 is the worked example with library-mesh Spawn entries
        // (0x27 / 0x28); its parts resolve into the effect-model library
        // `global_tmd_pool[model_sel + 3]` and animate via `tick_move_fx`,
        // rendered by the move-FX draw block below.
        if matches!(code, KeyCode::KeyH)
            && state == ElementState::Pressed
            && !self.boot_ui.is_active()
        {
            if self.session.host.world.mode != SceneMode::Battle {
                log::info!("move-FX spawn (H) is battle-only");
                return;
            }
            // Enumerate every move the parsed move-power table can render
            // as a 3D scene-graph and cycle through them across presses
            // (data-driven, so the preview reflects the real overlay
            // rather than one hard-coded id). The 0x06 worked example -
            // library-mesh Spawn entries 0x27/0x28 resolving into
            // `global_tmd_pool[model_sel + 3]` - is the starting point
            // when present.
            let spawnable = self
                .session
                .host
                .world
                .move_power
                .as_ref()
                .map(|c| c.spawnable_move_ids())
                .unwrap_or_default();
            if spawnable.is_empty() {
                log::info!(
                    "move-FX spawn (H): move-power table not installed / no spawnable moves"
                );
                return;
            }
            use std::sync::atomic::{AtomicUsize, Ordering};
            static MOVE_FX_CYCLE: AtomicUsize = AtomicUsize::new(0);
            let start = spawnable.iter().position(|&id| id == 0x06).unwrap_or(0);
            let n = MOVE_FX_CYCLE.fetch_add(1, Ordering::Relaxed);
            let slot = (start + n) % spawnable.len();
            let move_fx_id = spawnable[slot];
            log::info!(
                "move-FX preview {}/{}: move {move_fx_id:#04x} (spawnable {:#04x?})",
                slot + 1,
                spawnable.len(),
                spawnable
            );
            let origin = self
                .session
                .host
                .world
                .actors
                .iter()
                .find(|a| a.active)
                .map(|a| {
                    [
                        a.move_state.world_x,
                        a.move_state.world_y,
                        a.move_state.world_z,
                    ]
                })
                .unwrap_or([0, 0, 0]);
            if self.session.host.world.spawn_move_fx(move_fx_id, origin) {
                log::info!(
                    "spawned move-FX for move {move_fx_id:#04x}: {} mesh parts at {origin:?}",
                    self.session.host.world.active_move_fx_part_draws().len()
                );
                // Consume the surfaced presentation fields: the trail
                // texpage (render layer's streak pass) and the sound cue
                // (routed through the FUN_8004fcc8 dispatch decode). The
                // host has no battle SFX bank wired yet, so the cue is
                // resolved + logged rather than fired through the SPU.
                if let Some(trail) = self.session.host.world.active_move_fx_trail_texpage() {
                    log::info!("  move-FX trail texpage = {trail:#06x}");
                }
                if let Some(cue) = self.session.host.world.take_pending_move_fx_cue() {
                    let dispatch = legaia_engine_audio::classify_cue(cue as u32);
                    log::info!("  move-FX sound cue {cue:#04x} -> {dispatch:?}");
                }
            } else {
                log::info!(
                    "move-FX spawn for move {move_fx_id:#04x} produced no parts \
                     (table not installed / no spawnable entries)"
                );
            }
            return;
        }
        // `J`: debug-spawn one field move-VM scene-graph effect from the
        // current scene's prescript stager table (the op-0x34-sub-3 /
        // `FUN_800252EC` path), cycling through the table across presses.
        // Exercises the field-FX tick/draw wiring without waiting for the
        // field VM to execute an op-0x34-sub-3; the production trigger is
        // `FieldHostImpl::effect_anim_trigger`.
        if matches!(code, KeyCode::KeyJ)
            && state == ElementState::Pressed
            && !self.boot_ui.is_active()
        {
            let count = self.session.host.world.field_stagers.len();
            if count == 0 {
                log::info!("field-FX spawn (J): no prescript stager table for this scene");
                return;
            }
            let origin = self
                .session
                .host
                .world
                .actors
                .iter()
                .find(|a| a.active)
                .map(|a| {
                    [
                        a.move_state.world_x,
                        a.move_state.world_y,
                        a.move_state.world_z,
                    ]
                })
                .unwrap_or([0, 0, 0]);
            use std::sync::atomic::{AtomicUsize, Ordering};
            static FIELD_FX_CYCLE: AtomicUsize = AtomicUsize::new(0);
            let id = FIELD_FX_CYCLE.fetch_add(1, Ordering::Relaxed) % count;
            // Debug ergonomics: isolate one record per press (the
            // production op-0x34-sub-3 path lets them stack).
            self.session.host.world.active_field_fx.clear();
            if self.session.host.world.spawn_field_stager(id, origin) {
                let mesh: Vec<usize> = self
                    .session
                    .host
                    .world
                    .active_field_fx_part_draws()
                    .iter()
                    .map(|d| d.model_index)
                    .collect();
                let nodes: Vec<_> = self
                    .session
                    .host
                    .world
                    .active_field_fx_render_nodes()
                    .iter()
                    .map(|n| n.mode)
                    .collect();
                log::info!(
                    "field-FX spawn {}/{count} at {origin:?}: {} mesh parts (model_index {mesh:?}), {} render-mode nodes {nodes:?}",
                    id + 1,
                    mesh.len(),
                    nodes.len(),
                );
            }
            return;
        }
        // `K`: toggle the Noa dance (rhythm) minigame. Loads the dance
        // overlay (PROT 0980), suspends the current scene, and runs the
        // beat clock + hit judge; the three judged buttons are the retail
        // ones - Square / Circle / Triangle.
        // Pressing K again aborts and logs the final score. The song
        // also ends itself after its length limit (see below).
        if matches!(code, KeyCode::KeyK)
            && state == ElementState::Pressed
            && !self.boot_ui.is_active()
        {
            if self.session.host.world.mode == SceneMode::Dance {
                if let Some(g) = self.session.host.world.exit_dance() {
                    log::info!(
                        "dance: aborted at score {} (pass={})",
                        g.score(),
                        g.passed()
                    );
                    self.session.restore_field_bgm();
                }
            } else if self.start_dance_minigame(false) {
                log::info!("dance: started - Square/Circle/Triangle are the arrows, K to quit");
            }
            return;
        }
        // `L`: toggle the fishing minigame. Loads the fishing overlay
        // (PROT 0972), suspends the current scene, and runs the cast /
        // fight / score loop; Cross locks the cast and reels (reel A),
        // Square is reel B. Pressing L again leaves and logs the points.
        if matches!(code, KeyCode::KeyL)
            && state == ElementState::Pressed
            && !self.boot_ui.is_active()
        {
            if self.session.host.world.mode == SceneMode::Fishing {
                if let Some(s) = self.session.host.world.exit_fishing() {
                    log::info!(
                        "fishing: left with {} points (best {})",
                        s.record().points,
                        s.record().best_points
                    );
                }
            } else if self.start_fishing_minigame() {
                log::info!(
                    "fishing: started - Cross casts/reels(A), Circle reels(B), L to quit, P = prize exchange"
                );
            }
            return;
        }
        // `P` (while fishing): toggle the point-exchange prize list
        // decoded from the fishing overlay's per-venue tables. While
        // open: Up/Down move, Left/Right switch venue, Enter buys one.
        if self.session.host.world.mode == SceneMode::Fishing
            && state == ElementState::Pressed
            && !self.boot_ui.is_active()
        {
            let world = &mut self.session.host.world;
            match code {
                KeyCode::KeyP => {
                    if world.fishing_exchange.is_some() {
                        world.close_fishing_exchange();
                    } else if let Some(venues) = &self.fishing_prize_venues {
                        world.open_fishing_exchange(venues[0].clone());
                    } else {
                        log::info!("fishing: no exchange tables decoded (disc-free run?)");
                    }
                    return;
                }
                KeyCode::ArrowUp | KeyCode::ArrowDown if world.fishing_exchange.is_some() => {
                    let points = world.fishing_points;
                    if let Some(ex) = &mut world.fishing_exchange {
                        let floor = ex.first_visible(points);
                        let last = ex.rows.len().saturating_sub(1);
                        ex.cursor = if matches!(code, KeyCode::ArrowUp) {
                            ex.cursor.saturating_sub(1).max(floor)
                        } else {
                            (ex.cursor + 1).min(last)
                        };
                    }
                    return;
                }
                KeyCode::ArrowLeft | KeyCode::ArrowRight if world.fishing_exchange.is_some() => {
                    if let (Some(open), Some(venues)) =
                        (&world.fishing_exchange, &self.fishing_prize_venues)
                    {
                        let other = venues[1 - open.venue.min(1)].clone();
                        world.open_fishing_exchange(other);
                    }
                    return;
                }
                KeyCode::Enter if world.fishing_exchange.is_some() => {
                    let row = world.fishing_exchange.as_ref().map(|e| e.cursor);
                    if let Some(row) = row {
                        match world.fishing_exchange_buy(row, 1) {
                            Some(p) => log::info!(
                                "fishing exchange: bought item {:#04x} x{} for {} points ({} left)",
                                p.item_id,
                                p.qty,
                                p.cost,
                                world.fishing_points
                            ),
                            None => log::info!(
                                "fishing exchange: row {row} unavailable (points/limit/one-time)"
                            ),
                        }
                    }
                    return;
                }
                _ => {}
            }
        }
        // `O`: toggle the casino slot-machine minigame. Loads the slot
        // overlay (PROT 0975), suspends the current scene, and runs
        // the reel state machine; Cross spins / stops / collects (a
        // spin is a flat 3-coin bet across all three paylines).
        // Pressing O again cashes the balance out into the casino
        // coin bank and leaves.
        if matches!(code, KeyCode::KeyO)
            && state == ElementState::Pressed
            && !self.boot_ui.is_active()
        {
            if self.session.host.world.mode == SceneMode::SlotMachine {
                if let Some(m) = self.session.host.world.exit_slot_machine() {
                    log::info!(
                        "slots: cashed out {} coins into the bank (now {})",
                        m.balance(),
                        self.session.host.world.casino_coins
                    );
                }
            } else if self.start_slot_minigame() {
                log::info!(
                    "slots: started - Cross spins/stops/collects (3 coins a spin), O to cash out"
                );
            }
            return;
        }
        // `M`: toggle the Muscle Dome card-battle contest. Loads the
        // hand tables from the battle overlay (PROT 0898), suspends
        // the current scene, and runs the select/commit/resolve loop;
        // Left/Right/Up/Down commit the four cards, Cross confirms /
        // continues. Pressing M again aborts (no reward on an abort).
        if matches!(code, KeyCode::KeyM)
            && state == ElementState::Pressed
            && !self.boot_ui.is_active()
        {
            if self.session.host.world.mode == SceneMode::MuscleDome {
                if let Some(s) = self.session.host.world.exit_muscle_dome() {
                    use legaia_engine_core::muscle_dome::MusclePhase;
                    match s.phase() {
                        MusclePhase::Won => log::info!(
                            "muscle: contest WON - reward Seru spell id {:#x} credited",
                            s.reward_spell_id()
                        ),
                        MusclePhase::Lost => log::info!("muscle: contest lost"),
                        _ => log::info!("muscle: contest aborted"),
                    }
                    self.session.restore_field_bgm();
                }
            } else if self.start_muscle_minigame() {
                log::info!(
                    "muscle: started - Left/Right/Up/Down commit cards, Cross confirms, M to leave"
                );
            }
            return;
        }
        // `B`: toggle the Baka Fighter duel minigame. Loads the Baka
        // Fighter overlay (PROT 0976), suspends the current scene,
        // and runs the best-of-3 duel; Left/Right/Up throw the three
        // attacks, Down charges the special, Cross leaves a decided
        // match. Pressing B again aborts (no gold on an abort).
        if matches!(code, KeyCode::KeyB)
            && state == ElementState::Pressed
            && !self.boot_ui.is_active()
        {
            if self.session.host.world.mode == SceneMode::BakaFighter {
                if let Some(f) = self.session.host.world.exit_baka_fighter() {
                    match f.winner() {
                        Some(0) => log::info!(
                            "baka: match WON - {} gold banked (money now {})",
                            f.gold_reward(),
                            self.session.host.world.money
                        ),
                        Some(_) => log::info!("baka: match lost"),
                        None => log::info!("baka: match aborted"),
                    }
                    self.session.restore_field_bgm();
                }
            } else if self.start_baka_minigame() {
                log::info!("baka: started - Left/Right/Up attack, Down special, B to leave");
            }
            return;
        }
        // `V`: master audio mute toggle. Flips the engine-only `muted`
        // options knob, pushes it into the mixer's master gate (output
        // silenced; sequencer + SPU keep ticking so unmute stays in sync),
        // and persists it to the options config file. The HUD status line
        // reflects the state ("audio muted").
        if matches!(code, KeyCode::KeyV) && state == ElementState::Pressed {
            self.options_state.muted = !self.options_state.muted;
            self.persist_and_apply_options();
            log::info!(
                "audio: {}",
                if self.options_state.muted {
                    "muted (V to unmute)"
                } else {
                    "unmuted"
                }
            );
            return;
        }
        // `I`: toggle the opt-in dynamic-lighting enhancement (the
        // `--dynamic-lighting` flag's runtime twin). NON-RETAIL: the field
        // path has no light source, so OFF (the default) is the faithful
        // pixel-identical render; ON layers a soft warm directional light +
        // screen-centred light pool over the baked shading (capped ~1.3x).
        // Pure renderer state - no world/sim effect, replays unaffected.
        if matches!(code, KeyCode::KeyI) && state == ElementState::Pressed {
            self.dynamic_lighting = !self.dynamic_lighting;
            if let Some(r) = self.win.renderer.as_ref() {
                r.set_dynamic_lighting(self.dynamic_lighting);
            }
            log::info!(
                "render: dynamic lighting {}",
                if self.dynamic_lighting {
                    "ON (enhancement - not retail)"
                } else {
                    "off (faithful baked shading)"
                }
            );
            return;
        }
        // `C`: toggle the field camera between the retail follow view
        // (savestate-pinned pitch/yaw/H, player-anchored - the
        // faithful framing) and the wide debug orbit vantage (better
        // for eyeballing scene completeness).
        if matches!(code, KeyCode::KeyC)
            && state == ElementState::Pressed
            && !self.boot_ui.is_active()
        {
            self.field_debug_camera = !self.field_debug_camera;
            log::info!(
                "camera: field vantage = {}",
                if self.field_debug_camera {
                    "debug orbit"
                } else {
                    "retail follow"
                }
            );
            return;
        }
        // `T`: cycle the field camera-distance preset (retail -> far ->
        // farther). A pure framing knob on the follow / debug-orbit
        // cameras - it never feeds the world simulation. Persisted to the
        // options config file; the interactive default is `far`.
        if matches!(code, KeyCode::KeyT)
            && state == ElementState::Pressed
            && !self.boot_ui.is_active()
        {
            let next = self.session.camera.distance.cycle();
            self.session.camera.distance = next;
            self.options_state.camera_distance = next;
            self.persist_and_apply_options();
            log::info!("camera: distance = {} (T cycles)", next.label());
            return;
        }
        // `R`: toggle precise movement (opt-in, NON-RETAIL): free-angle
        // camera-relative locomotion instead of retail's 4/8-way
        // quantisation - key diagonals walk true 45-degree vectors and an
        // analog stick's angle passes through continuously. Persisted to
        // the options config file; default off (the faithful remap).
        if matches!(code, KeyCode::KeyR)
            && state == ElementState::Pressed
            && !self.boot_ui.is_active()
        {
            let on = !self.options_state.precise_movement;
            self.options_state.precise_movement = on;
            self.session.host.world.precise_movement = on;
            self.persist_and_apply_options();
            log::info!(
                "movement: precise {}",
                if on {
                    "ON (free-angle - not retail)"
                } else {
                    "off (retail 8-way remap)"
                }
            );
            return;
        }
        // `N`: open the name-entry overlay for the lead character. The
        // NEW GAME flow opens it automatically when the `town01` opening
        // timeline reaches its op-`0x49` STATE_RESUME (P2[3] body
        // `0x02c6`); this key is a dev hand-trigger to exercise the
        // ported overlay outside that flow.
        if matches!(code, KeyCode::KeyN)
            && state == ElementState::Pressed
            && !self.boot_ui.is_active()
            && !self.session.host.world.name_entry_active()
        {
            self.session.host.world.open_name_entry(0);
            return;
        }
        // Menu input: while the active dialog box is a multiple-choice
        // menu (a picker decoded from the inline interaction script),
        // Up/Down move the option cursor and a confirm button applies
        // the chosen option's relative jump (`FUN_80038050`) instead of
        // driving movement / dismissing the box. Resolve the key->button
        // first so the immutable `mapping` borrow ends before the
        // mutable `active_dialog` borrow.
        if state == ElementState::Pressed {
            let is_confirm = matches!(
                self.mapping.pad_button_for_key(keycode_to_name(code)),
                Some(
                    legaia_engine_core::input::PadButton::Cross
                        | legaia_engine_core::input::PadButton::Circle
                )
            );
            if let Some(panel) = self.active_dialog.as_mut()
                && panel.menu_active()
            {
                if matches!(code, KeyCode::ArrowUp) {
                    panel.move_picker_cursor(-1);
                    return;
                }
                if matches!(code, KeyCode::ArrowDown) {
                    panel.move_picker_cursor(1);
                    return;
                }
                if is_confirm {
                    panel.confirm_menu();
                    return;
                }
            }
        }
        let key_name = keycode_to_name(code);
        if let Some(button) = self.mapping.pad_button_for_key(key_name) {
            let prev = self.pad;
            if state == ElementState::Pressed {
                self.pad |= button.mask();
            } else {
                self.pad &= !button.mask();
            }
            // Record the transition iff the pad actually changed
            // (auto-repeat sends a stream of Pressed events with
            // identical mask; dedup in RecordLog::record_transition).
            if self.pad != prev
                && let Some(log) = self.record_log.as_mut()
            {
                log.record_transition(self.session.frames, self.pad);
            }
        }
    }
}
