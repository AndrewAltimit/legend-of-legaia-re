//! The minigame keys: each toggles one minigame session, and `P` plus the
//! arrows drive the fishing prize exchange while a fishing session is up.

use super::super::super::*;

impl PlayWindowApp {
    /// `K` / `U` / `L` / `P` / `O` / `M` / `B`: the minigame entries and the fishing exchange. `true` when one took the key.
    pub(super) fn minigame_key(&mut self, code: KeyCode, state: ElementState) -> bool {
        self.key_dance(code, state)
            || self.key_dance_how_to(code, state)
            || self.key_fishing(code, state)
            || self.key_fishing_exchange(code, state)
            || self.key_slot_machine(code, state)
            || self.key_muscle_dome(code, state)
            || self.key_baka_fighter(code, state)
    }

    /// `K`: toggle the Noa dance (rhythm) minigame. Arms the mode-24 door
    /// warp to the hall (PROT 0980 loads on the drain, the floor read off the
    /// story flags), which runs the beat clock + hit judge; the three judged
    /// buttons are the retail ones - Square / Circle / Triangle.
    /// Pressing K again aborts and logs the final score. The song
    /// also ends itself after its length limit (see below).
    fn key_dance(&mut self, code: KeyCode, state: ElementState) -> bool {
        if matches!(code, KeyCode::KeyK)
            && state == ElementState::Pressed
            && !self.boot_ui.is_active()
        {
            if self.session.host.world.mode == SceneMode::Dance {
                // Covers the pre-song count-in too: the world holds it, so
                // leaving the mode drops it (and re-queues the hall's own
                // track through `World::restore_minigame_bgm`).
                if let Some(g) = self.session.host.world.exit_dance() {
                    log::info!(
                        "dance: aborted at score {} (pass={})",
                        g.score(),
                        g.passed()
                    );
                }
                // A door-entered hall closes its mode-24 round trip here as
                // the Start escape does (a no-op for a `K`-launched one).
                self.session.host.world.close_minigame_round_trip();
            } else {
                // The mode-24 door warp, as `O` / `M` / `B` and the browser
                // page's dance entry (`play_mg_debug_warp(6)`) take it: the
                // floor comes from the story flags the hall's script reads
                // (`dance_mode_from_flags`), with the warp's round trip and
                // door XA prestage. `K` used to build a Qualifier floor of its
                // own and skip the warp.
                self.session.host.world.request_minigame_warp(
                    legaia_engine_mode::minigame_entry::MinigameSubId::Dance.sub_id(),
                );
                log::info!(
                    "dance: door warp armed - Square/Circle are the arrows, Triangle spends a groovy move, K to quit"
                );
            }
            return true;
        }
        false
    }

    /// `U`: the how-to dance (retail's setumei mode) with the Disco King
    /// tutorial actor running beside the session - the opening prompt,
    /// the caption steps and the two practice gates, with placeholder
    /// caption text (the overlay's strings are Sony rodata).
    fn key_dance_how_to(&mut self, code: KeyCode, state: ElementState) -> bool {
        if matches!(code, KeyCode::KeyU)
            && state == ElementState::Pressed
            && !self.boot_ui.is_active()
            && self.session.host.world.mode != SceneMode::Dance
        {
            // The tutorial actor is installed by `World::enter_dance` when the
            // parsed run is a how-to one, so it reaches the browser play page
            // from the same call.
            if self
                .start_dance_minigame_mode(legaia_engine_minigames::dance::DanceMode::HowTo, false)
            {
                log::info!("dance: how-to started - face buttons advance the tutorial");
            }
            return true;
        }
        false
    }

    /// `L`: toggle the fishing minigame. Loads the fishing overlay
    /// (PROT 0972), suspends the current scene, and runs the cast /
    /// fight / score loop; Cross locks the cast and reels (reel A),
    /// Square is reel B. Pressing L again leaves and logs the points.
    fn key_fishing(&mut self, code: KeyCode, state: ElementState) -> bool {
        if matches!(code, KeyCode::KeyL)
            && state == ElementState::Pressed
            && !self.boot_ui.is_active()
        {
            if self.session.host.world.mode == SceneMode::Fishing {
                if let Some(s) = self.session.host.world.exit_fishing() {
                    log::info!(
                        "fishing: left with {} points (best {})",
                        s.record.points,
                        s.record.best_points
                    );
                }
                self.session.host.world.close_minigame_round_trip();
            } else if self.start_fishing_minigame() {
                log::info!(
                    "fishing: started - Circle casts/locks, Cross reels(A), Square reels(B), L to quit, P = prize exchange"
                );
            }
            return true;
        }
        false
    }

    /// `P` (while fishing): toggle the point-exchange prize list
    /// decoded from the fishing overlay's per-venue tables. While
    /// open: Up/Down move, Left/Right switch venue, Enter buys one.
    fn key_fishing_exchange(&mut self, code: KeyCode, state: ElementState) -> bool {
        if self.session.host.world.mode == SceneMode::Fishing
            && state == ElementState::Pressed
            && !self.boot_ui.is_active()
        {
            use legaia_engine_core::fishing_exchange_input::{ExchangeInput, ExchangeOutcome};
            let world = &mut self.session.host.world;
            let open = world.minigames.fishing_exchange.is_some();
            // The key -> input map is this host's; what each input DOES
            // (the points bank on open, the cursor floor, the venue flip, the
            // buy at the cursor) is the shared engine kernel the browser
            // play page's exchange panel drives too.
            let input = match code {
                KeyCode::KeyP => Some(ExchangeInput::Toggle),
                KeyCode::ArrowUp if open => Some(ExchangeInput::Up),
                KeyCode::ArrowDown if open => Some(ExchangeInput::Down),
                KeyCode::ArrowLeft | KeyCode::ArrowRight if open => {
                    Some(ExchangeInput::SwitchVenue)
                }
                KeyCode::Enter if open => Some(ExchangeInput::Buy),
                _ => None,
            };
            if let Some(input) = input {
                // Decoded by the shared fishing entry, so a session walked
                // into through the venue door has them too.
                match world.minigames.fishing_prize_venues.clone() {
                    Some(venues) => match world.fishing_exchange_input(&venues, input) {
                        ExchangeOutcome::Bought(p) => log::info!(
                            "fishing exchange: bought item {:#04x} x{} for {} points ({} left)",
                            p.item_id,
                            p.qty,
                            p.cost,
                            world.minigames.fishing_points
                        ),
                        ExchangeOutcome::Refused if input == ExchangeInput::Buy => {
                            log::info!("fishing exchange: row unavailable (points/limit/one-time)")
                        }
                        _ => {}
                    },
                    None => log::info!("fishing: no exchange tables decoded (disc-free run?)"),
                }
                return true;
            }
        }
        false
    }

    /// `O`: toggle the casino slot-machine minigame. The launch is the
    /// mode-24 door warp itself (`World::request_minigame_warp` with
    /// sub-id 3, drained by the scene host next tick), as `B` / `M` and
    /// the browser page's `play_mg_debug_warp` are: the session is the
    /// one a cabinet installs, its balance assigned from the casino coin
    /// bank (`FUN_801CEC94`). A thin bank is the machine's own state-1
    /// gate (fewer than 3 coins cannot spin); the cabinet record's
    /// coin-bank compare that refuses an empty bank at the door is the
    /// walked door's script, which the field VM runs on both hosts.
    /// Cross spins / collects, Square / Cross / Circle stop reels 0 / 1 / 2;
    /// pressing O again cashes the
    /// balance out into the coin bank and leaves.
    fn key_slot_machine(&mut self, code: KeyCode, state: ElementState) -> bool {
        if matches!(code, KeyCode::KeyO)
            && state == ElementState::Pressed
            && !self.boot_ui.is_active()
        {
            if self.session.host.world.mode == SceneMode::SlotMachine {
                if let Some(m) = self.session.host.world.exit_slot_machine() {
                    log::info!(
                        "slots: cashed out {} coins into the bank (now {})",
                        m.balance(),
                        self.session.host.world.minigames.casino_coins
                    );
                }
                self.session.host.world.close_minigame_round_trip();
            } else {
                self.session.host.world.request_minigame_warp(
                    legaia_engine_mode::minigame_entry::MinigameSubId::SlotMachine.sub_id(),
                );
                log::info!(
                    "slots: door warp armed (bank {} coins) - Cross spins/collects, Square/Cross/Circle stop reels 0/1/2, O to cash out",
                    self.session.host.world.minigames.casino_coins
                );
            }
            return true;
        }
        false
    }

    /// `M`: toggle the Muscle Dome contest (an ordinary battle, fought to
    /// a KO - nothing bounds it by turns). The selection is the battle's
    /// command flow: the ring (Left = Attack, Right = Ra-Seru, Down =
    /// Spirit), Auto | Command, the direction entry (Triangle pages the
    /// arts list), the review and Begin | Reselect. Pressing M again
    /// aborts (no reward on an abort).
    ///
    /// The launch is the mode-24 door warp itself
    /// (`World::request_minigame_warp`, drained by the scene host next
    /// tick), so the hotkey enters the session the arena door enters -
    /// the lead's own art catalog, the battle-theme swap through
    /// `swap_to_minigame_bgm`, the return warp on the way out. It used to
    /// build its own session, start the track through the director
    /// directly and install Vahn's arts whoever led.
    fn key_muscle_dome(&mut self, code: KeyCode, state: ElementState) -> bool {
        if matches!(code, KeyCode::KeyM)
            && state == ElementState::Pressed
            && !self.boot_ui.is_active()
        {
            if self.session.host.world.mode == SceneMode::MuscleDome {
                // `World::leave_muscle_dome` is the escape both hosts share:
                // it reports the leg (a leg left undecided is the arena's
                // run / give-up path, retail's `_DAT_80084448 = 4` arm) and
                // settles a contest that has run out.
                if let Some(s) = self.session.host.world.leave_muscle_dome() {
                    use legaia_engine_battle_menus::muscle_dome_session::MusclePhase;
                    let phase = s.phase();
                    match phase {
                        MusclePhase::Won => log::info!("muscle: leg won"),
                        MusclePhase::Lost => log::info!("muscle: leg lost"),
                        _ => log::info!(
                            "muscle: leg abandoned after {} turns - opponent left on {}% HP",
                            s.turn(),
                            s.hp_left()
                        ),
                    }
                }
                // The round trip restores the field's music with the scene.
                self.session.host.world.close_minigame_round_trip();
            } else {
                self.session.host.world.request_minigame_warp(
                    legaia_engine_mode::minigame_entry::MinigameSubId::MuscleDome.sub_id(),
                );
                log::info!(
                    "muscle: door warp armed - fight to a KO, arrows enter directions, Cross confirms, M to leave"
                );
            }
            return true;
        }
        false
    }

    /// `B`: toggle the Baka Fighter duel minigame - Square / Circle /
    /// Cross throw the three attacks (`World::tick_baka_fighter`; the
    /// special is retail's auto-finisher, no button); after a decided match the
    /// cabinet takes Left / Right (NEXT GAME / PAY OUT) and Cross.
    /// Pressing B again aborts (no coins on an abort). Launched
    /// through the mode-24 door warp like `M` above, so the overture swap
    /// and the return warp are the door's own.
    fn key_baka_fighter(&mut self, code: KeyCode, state: ElementState) -> bool {
        if matches!(code, KeyCode::KeyB)
            && state == ElementState::Pressed
            && !self.boot_ui.is_active()
        {
            if self.session.host.world.mode == SceneMode::BakaFighter {
                // `exit_baka_fighter` runs the round trip, and its music
                // restore, itself.
                if let Some(f) = self.session.host.world.exit_baka_fighter() {
                    match f.winner() {
                        Some(0) => log::info!(
                            "baka: match WON - {} coins banked (bank now {})",
                            f.gold_reward(),
                            self.session.host.world.minigames.casino_coins
                        ),
                        Some(_) => log::info!("baka: match lost"),
                        None => log::info!("baka: match aborted"),
                    }
                }
            } else {
                self.session.host.world.request_minigame_warp(
                    legaia_engine_mode::minigame_entry::MinigameSubId::BakaFighter.sub_id(),
                );
                log::info!("baka: door warp armed - Square/Circle/Cross attack, B to leave");
            }
            return true;
        }
        false
    }
}
