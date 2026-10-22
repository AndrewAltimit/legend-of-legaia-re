//! Page-facing battle session exports: live / player-driven toggles, the
//! active party, battle BGM, the debug battle seeds and the game-over
//! hand-off input.

use super::*;

#[wasm_bindgen]
impl LegaiaRuntime {
    /// Enable / disable the live battle loop for subsequently-entered scenes
    /// (and the currently-running one). On by default - the play page rolls
    /// the disc's own step-driven encounters like retail. Off = the old
    /// walk-only behaviour (explore but never fight); the browser twin of
    /// running the native window without `--live-loop`.
    pub fn set_live_battles(&mut self, on: bool) {
        self.live_battles = on;
        if let Some(h) = self.scene_host.host_mut() {
            h.world.toggles.live_gameplay_loop = on;
            h.world.battle.player_driven = on;
        }
    }

    /// Player-driven vs auto-resolved party turns, independently of the
    /// encounter roll - the browser twin of the native `--no-player-battle`
    /// (the two are separate knobs there; `set_live_battles` sets both).
    /// Off = each party turn auto-attacks the first living monster.
    pub fn set_player_driven_battles(&mut self, on: bool) {
        if let Some(h) = self.scene_host.host_mut() {
            h.world.battle.player_driven = on;
        }
    }

    /// Install a present-party composition - the browser twin of the native
    /// `--party` flag. `spec` is comma-separated character names
    /// (`vahn`/`noa`/`gala`/`terra`) or 0-based roster indices in battle
    /// order, e.g. `noa,terra`. Roster slots the spec names that carry no
    /// stats yet are seeded from the SCUS new-game template (the rows retail
    /// seeds those characters from when they join); records already carrying
    /// stats are left alone. Returns an empty string on success, else the
    /// reason. Caps at the three on-screen positions.
    pub fn set_active_party_spec(&mut self, spec: &str) -> String {
        let slots = match legaia_engine_session::party_spec::parse_party_spec(spec) {
            Ok(s) if !s.is_empty() => s,
            Ok(_) => return "empty party spec".to_string(),
            Err(e) => return e,
        };
        let Some(host) = self.scene_host.host_mut() else {
            return "no disc loaded".to_string();
        };
        if let Some(defaults) = host.new_game_defaults.as_ref() {
            let starting = defaults.party.clone();
            host.world.seed_party_members(&starting, &slots);
        }
        // The composition, the battle stat fold and the MP ceiling the HUD
        // draws: the install the native `--party` flag shares.
        legaia_engine_session::party_spec::install_party_composition(&mut host.world, slots);
        String::new()
    }

    /// The present party as a JSON array of roster slots in battle order.
    pub fn active_party_json(&self) -> String {
        let slots = self
            .scene_host
            .host()
            .map(|h| h.world.party.active_party.clone())
            .unwrap_or_default();
        let body: Vec<String> = slots.iter().map(|i| i.to_string()).collect();
        format!("[{}]", body.join(","))
    }

    /// Teach `spell_id` to the lead's roster record, prepended to the spell
    /// list at level 1 - the browser twin of the native `--learn-spell`,
    /// which is how the Magic arm is reachable in a test fight without
    /// playing to the Seru. Returns `false` with no roster record.
    pub fn learn_spell(&mut self, roster_slot: u8, spell_id: u8) -> bool {
        let Some(host) = self.scene_host.host_mut() else {
            return false;
        };
        let Some(record) = host
            .world
            .party
            .roster
            .members
            .get_mut(roster_slot as usize)
        else {
            return false;
        };
        legaia_engine_battle::magic_xp::learn_spell_prepend(record, spell_id);
        true
    }

    /// Override the Battle<->Field BGM swap track (`0` disables the swap;
    /// with no override the shipped default battle theme plays), the browser
    /// twin of the native window's `--battle-bgm <id>`. The id is routed
    /// through the same director as field op-`0x35` starts: scene-local ids
    /// resolve through the scene's asset table, `>= 2000` through the global
    /// `music_01` pool.
    pub fn set_battle_bgm(&mut self, bgm_id: u16) {
        self.battle_bgm = Some(bgm_id);
        if let Some(h) = self.scene_host.host_mut() {
            h.world.set_battle_bgm((bgm_id != 0).then_some(bgm_id));
        }
    }

    /// `true` when the current scene can produce a random encounter at all.
    /// The page shows a "no random encounters here" hint when it is `false`,
    /// so a town's designed silence doesn't read as a broken engine.
    pub fn scene_rolls_encounters(&self) -> bool {
        self.scene_host
            .host()
            .is_some_and(|h| h.world.encounters.scene_rollable)
    }

    /// The formation rows the current scene registered, as a JSON array of
    /// ids (`[]` with no scene up). The list the native window prints when
    /// `--battle <ROW>` names a row the scene never registered, exposed so a
    /// page - or a headless driver - can pick a row that exists instead of
    /// guessing.
    pub fn debug_formation_rows(&self) -> String {
        let ids = self
            .scene_host
            .host()
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
    /// DEBUG affordance, not a player surface - the browser twin of the
    /// native window's `LEGAIA_DEBUG_SHATTER` / `LEGAIA_DEBUG_SHATTER_FLAGS`:
    /// stamp `value` onto every seated monster's `+0x1BA` shatter word and,
    /// in the field, onto every catalogued NPC's `+0x7A` over a debug
    /// deformation row carrying `flags`, so the shatter renderer can be
    /// looked at headless. Nothing in a fight clears the stamp.
    pub fn debug_shatter(&mut self, value: u16, flags: u32) {
        let catalog: Vec<u8> = self.actors.npcs.as_ref().map_or_else(Vec::new, |n| {
            n.pack
                .entries
                .iter()
                .map(|e| e.placement.index as u8)
                .collect()
        });
        let Some(host) = self.scene_host.host_mut() else {
            return;
        };
        let w = &mut host.world;
        for (slot, _, _) in w.battle_monster_slots() {
            if let Some(a) = w.actors.get_mut(slot) {
                a.battle.shatter = value;
            }
        }
        if w.mode == legaia_engine_mode::mode::SceneMode::Field {
            const DEBUG_ROW: u8 = 0xFE;
            w.npcs.face_rows.insert(
                DEBUG_ROW,
                legaia_engine_core::world::FieldFaceRow {
                    offset: [0; 4],
                    flags,
                },
            );
            for slot in catalog {
                w.npcs.faces.insert(slot, (DEBUG_ROW, value as i16));
            }
        }
    }

    pub fn debug_force_battle(&mut self, row: i32) -> bool {
        let Some(host) = self.scene_host.host_mut() else {
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
        if !host.world.toggles.live_gameplay_loop {
            host.world.toggles.live_gameplay_loop = true;
            host.world.battle.player_driven = true;
        }
        host.world.force_encounter(id)
    }

    /// Talk to the live scene's placement `slot` through
    /// `World::trigger_field_interact` - the call the walk-up interaction
    /// probe makes - so a headless driver can open a merchant's or an
    /// innkeeper's conversation without walking to it. The native twin is the
    /// screenshot harness's `LEGAIA_CAPTURE_TALK=<slot>@<tick>`.
    pub fn debug_talk_to_placement(&mut self, slot: u8) -> bool {
        let Some(host) = self.scene_host.host_mut() else {
            return false;
        };
        host.world.trigger_field_interact(0xFF, slot);
        true
    }

    /// Dispatch a cast the moment the next battle's first command prompt
    /// opens - the browser twin of the native window's
    /// `LEGAIA_BATTLE_INFLIGHT=caster,spell,target` debug seam (engine actor
    /// indices; every seat keeps its ground). Pair with
    /// [`Self::debug_force_battle`] to watch one cast from a cold page.
    pub fn debug_seed_inflight_cast(&mut self, caster: u8, spell_id: u8, target: u8) -> bool {
        let Some(host) = self.scene_host.host_mut() else {
            return false;
        };
        host.world.battle.inflight_seed = Some(legaia_engine_core::world::InflightCastSeed {
            caster,
            spell_id,
            target,
            ground: [None; legaia_engine_core::world::INFLIGHT_GROUND_SLOTS],
            idle: [None; legaia_engine_core::world::INFLIGHT_GROUND_SLOTS],
        });
        true
    }

    /// Make the monster in engine battle `seat` cast `spell_id` on its next
    /// turn instead of the AI's pick - the browser twin of the native
    /// window's `LEGAIA_BATTLE_MONSTER_CAST=seat,spell` debug seam
    /// (`BattleState::forced_monster_cast`). Pair with
    /// [`Self::debug_force_battle`] to watch one enemy special from a cold
    /// page.
    pub fn debug_seed_monster_cast(&mut self, seat: u8, spell_id: u8) -> bool {
        let Some(host) = self.scene_host.host_mut() else {
            return false;
        };
        host.world.battle.forced_monster_cast = Some((seat, spell_id));
        true
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
        let Some(hold) = self.game_over.as_mut() else {
            return String::new();
        };
        // The session's step, the native window's too: the deferred field
        // restore (the hold froze the battle frame) and the battle track
        // stopped on the resolving frame. Without a session the hold has no
        // world to resolve and simply ends.
        let resolved = match self.scene_host.session_mut() {
            Some(s) => s.step_party_wipe_hold(hold),
            None => {
                hold.tick();
                hold.outcome().is_some()
            }
        };
        if !resolved {
            return String::new();
        }
        self.game_over = None;
        "quit".to_string()
    }
}
