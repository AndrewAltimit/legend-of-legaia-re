//! The Muscle Dome's wasm-bindgen API: leg, contest, magic and arts exports.
//! Split out of `minigames_muscle.rs`.

use super::*;

#[wasm_bindgen]
impl LegaiaMinigames {
    /// Start a contest with defaults (Vahn at level 30 vs the archive's first
    /// decodable monster) - the compatibility entry the page's reset path and
    /// the older verification hooks call. Returns `false` when the tables
    /// didn't decode.
    pub fn muscle_start(&mut self) -> bool {
        let monster = self
            .monster_archive_entry()
            .and_then(|e| {
                let n = monster_archive::slot_count(e) as u16;
                (1..=n).find(|&id| {
                    monster_archive::record(e, id)
                        .ok()
                        .flatten()
                        .is_some_and(|r| r.hp > 0)
                })
            })
            .unwrap_or(1);
        self.muscle_start_vs(0, 30, monster, 0x2A)
    }

    /// Start a Muscle Dome contest: party character `char_slot` (0 = Vahn,
    /// 1 = Noa, 2 = Gala) at `level` versus monster `monster_id` (a PROT 867
    /// archive id), on PsyQ RNG seed `seed`.
    ///
    /// The player fighter's stats are the disc's own progression: the
    /// new-game template record leveled through the growth curves (the
    /// deterministic core gain per level - retail adds a `rand()` jitter of
    /// mean 0 on top, so the core is the expected retail stat line), then
    /// battle-load initialised (`FUN_80053CB8`). The opponent's stats are its
    /// monster record's boosted battle profile (`FUN_80054CB0`). Both round
    /// budgets seed from the fighters' AGL - the `+0x154` pool the dome's
    /// budget `ctx+0x6dc` reads. Returns `false` when the tables or the
    /// monster record don't resolve.
    pub fn muscle_start_vs(
        &mut self,
        char_slot: u32,
        level: u32,
        monster_id: u16,
        seed: u32,
    ) -> bool {
        self.muscle = None;
        let Some(tables) = self.muscle_tables.as_ref() else {
            return false;
        };
        let hand = tables.hand;
        let char_slot = (char_slot as usize).min(2);
        let Some((opponent, opp_name)) = self.muscle_monster_fighter(monster_id) else {
            return false;
        };
        let (player, player_name, stats_source) = match self.muscle_player_fighter(char_slot, level)
        {
            Some((f, n)) => (f, n, "disc"),
            None => (
                // No SCUS (raw PROT.DAT load): documented fallback
                // constants, surfaced as "fallback" in the state JSON.
                MuscleFighter {
                    hp_max: 500,
                    mp_max: 50,
                    budget_pool: 120,
                    int: 60,
                    udf: 40,
                    ldf: 40,
                    element: ELEMENT_NEUTRAL,
                },
                ["Vahn", "Noa", "Gala"][char_slot].to_string(),
                "fallback",
            ),
        };
        let player_costs = self
            .muscle_swing_costs(char_slot)
            .unwrap_or([FAVORED_COST; 4]);
        let player_hand = std::array::from_fn(|i| MuscleCard {
            command_id: hand[i],
            cost: player_costs[i],
        });
        // The opponent plays the same deck at the favored flat cost (a
        // monster has no player battle file to read swing costs from).
        let opp_hand = std::array::from_fn(|i| MuscleCard {
            command_id: hand[i],
            cost: FAVORED_COST,
        });
        let hp = [player.hp_max as i32, opponent.hp_max as i32];
        let mut session = MuscleDomeSession::new(
            player_hand,
            opp_hand,
            [player.budget_pool, opponent.budget_pool],
            hp,
            WEB_CAPTION_SERU,
        );
        // The special-battle word retail's arena entry seeds from the three
        // course-unlock story flags. This page has no save bank, so the word
        // is whatever `muscle_contest_start` latched from its `unlock` mask
        // (`0` - forbidding nothing - for a standalone leg opened without a
        // contest). Same kernel, same bits as the native host's
        // `World::dome_special_word`; only the flag *source* differs.
        let special = self.muscle_special;
        session.set_special_word(special);
        // NOT WIRED (this host only): no art catalog is installed, so this
        // panel's turn resolves the raw direction string rather than the
        // tokenizer's action queue. `MuscleDomeSession::install_art_catalog`
        // wants `(ActionConstant, commands)` rows and this page's only art
        // source is the SCUS arts-**name** table (`muscle_art_catalog`),
        // which carries a display index, not the action constant - the
        // constants live in the per-character art records (PROT `0x05C4`)
        // this page does not decode. The arena-door warp path
        // (`engine-core::scene::host::minigame_warp`) and the native window's
        // own dome entry both install one through
        // `muscle_dome::art_catalog_for`; this panel owes the art-record
        // decode, not another filter.
        //
        // Damage resolves through the shared retail kernel - the same
        // `DomeDamageModel` the native play-window host installs, so neither
        // host carries a damage rule of its own.
        // The Ra-Seru (magic) command class. The gates, the pricing and the
        // cast are the shared session's, so this panel and the native window
        // resolve a dome cast through one rule; what is this page's own model
        // is the *list*: a standalone contest has no save, so the fighter is
        // offered the disc's whole player Seru block (`0x81..=0x8b`) rather
        // than a record's learned ids, and the Ra-Seru gate is `true` because
        // a party character reaching Sol Tower carries one.
        if let Some(catalog) = self
            .scus
            .as_ref()
            .and_then(|s| legaia_engine_core::retail_magic::seru_magic_catalog_from_scus(s))
        {
            // The catalog carries the monster specials and the placeholder
            // block as well; the ring offers the **player Seru** ids only.
            let mut spells: Vec<_> = catalog
                .iter()
                .filter(|s| PLAYER_SERU_IDS.contains(&s.id))
                .cloned()
                .collect();
            spells.sort_by_key(|s| s.id);
            session.install_magic(
                0,
                legaia_engine_core::muscle_dome::DomeMagic {
                    ring: legaia_engine_core::muscle_dome::DomeRing {
                        special,
                        status: 0,
                        has_raseru: true,
                    },
                    mp: player.mp_max,
                    mp_max: player.mp_max,
                    ability_bits: 0,
                    magic_power: player.int,
                    spells,
                },
            );
        }
        session.install_damage_model(DomeDamageModel::new(
            tables.move_power.clone(),
            tables.move_map,
            tables.affinity.clone(),
            [player.combatant(), opponent.combatant()],
            hp,
            seed,
        ));
        self.muscle = Some(MuscleContest {
            session,
            fighters: [player, opponent],
            names: [player_name, opp_name],
            stats_source,
            monster_id,
            char_slot,
            level,
        });
        true
    }

    /// Commit one of the player's four hand cards (0..4) into the action queue,
    /// debiting the budget. Returns `false` when it can't be committed
    /// (overspend, queue full, or outside the selection phase).
    pub fn muscle_commit(&mut self, card_slot: usize) -> bool {
        self.muscle
            .as_mut()
            .is_some_and(|c| c.session.commit_card(0, card_slot))
    }

    /// Run the opponent's greedy in-order commit (the host AI model), then
    /// close the selection phase so the round is ready to resolve.
    pub fn muscle_end_selection(&mut self) {
        if let Some(c) = self.muscle.as_mut() {
            c.session.ai_commit_all(1);
            c.session.end_selection();
        }
    }

    /// Play the turn out through the shared retail damage kernel
    /// ([`legaia_engine_core::muscle_dome::DomeDamageModel`], installed at
    /// [`Self::muscle_start_vs`]) - the player's whole queued command string,
    /// then the opponent's, not interleaved. The native play-window host
    /// resolves through the same kernel; this method holds no damage rule of
    /// its own. No-op unless the turn is in the resolve phase.
    ///
    /// The kernel-absent arm is shared too
    /// ([`MuscleDomeSession::resolve_turn_or_zero`]): with no disc tables
    /// installed the turn still closes, at zero damage. Dropping that arm on
    /// one host only is what left the browser contest parked in
    /// `MusclePhase::Resolve` forever while the window's advanced.
    pub fn muscle_resolve(&mut self) {
        if let Some(c) = self.muscle.as_mut() {
            c.session.resolve_turn_or_zero();
        }
    }

    /// Take the next turn after a non-terminal resolution: reseed budgets,
    /// clear queues. No-op unless the contest is at a turn break - only a KO
    /// closes the leg for good.
    pub fn muscle_next_turn(&mut self) {
        if let Some(c) = self.muscle.as_mut() {
            c.session.next_turn();
        }
    }

    /// Open a **contest** on the arena's course ladder - the ladder run a leg
    /// belongs to. `unlock` is the three course-unlock story flags
    /// (`0x536` / `0x537` / `0x538`) as a bitmask, `gates` the three Master
    /// course-length flags (`0x378` / `0x382` / `0x471`); a page with no save
    /// to read them from passes `0b111` for both to mean "everything open".
    ///
    /// Returns `false` when PROT 0977 does not decode.
    pub fn muscle_contest_start(&mut self, unlock: u32, gates: u32) -> bool {
        let Some(raw) = entry_bytes(&self.prot, &self.entries, ARENA_OVERLAY_PROT_INDEX) else {
            return false;
        };
        let flags = web_contest_flags(unlock, gates, false);
        // Latch the special-battle word this visit's unlocks seed, so every
        // leg the contest stages gates its ring chips the way the arena's own
        // entry would (`FUN_801CEA6C`). The native host reads the same three
        // flags out of `World` instead.
        self.muscle_special = legaia_engine_core::muscle_dome::contest_entry_word(&flags);
        match legaia_engine_core::muscle_dome::DomeContest::from_overlay(raw, &flags) {
            Some(c) => {
                self.muscle_run = Some(c);
                self.muscle_settlement = None;
                true
            }
            None => false,
        }
    }

    /// The special-battle word (`0x8007BAC0`) the next leg opens on - `0`
    /// until a contest latches one. Surfaced so the page can draw the ring's
    /// crossed-out chips before a leg starts.
    pub fn muscle_special_word(&self) -> u32 {
        self.muscle_special
    }

    /// The `(course, round)` the open contest stages next, as
    /// `[course, round]`. `[0, 0]` with no contest open, which is also the
    /// first leg of a fresh one.
    pub fn muscle_contest_cursor(&self) -> Vec<u32> {
        self.muscle_run
            .as_ref()
            .map_or(vec![0, 0], |c| vec![c.course() as u32, c.round()])
    }

    /// Report the finished leg to the open contest: the ladder advances, the
    /// leg's four rows are computed and drained, and the between-leg HP
    /// recovery is returned.
    ///
    /// `outcome` is the battle's own outcome code -
    /// [`legaia_engine_core::muscle_dome::LEG_OUTCOME_RAN`] gives the contest
    /// up. `hp_max` scales every recovery lane. Returns the HP the fighter
    /// gets back (capped at `hp_max`), or `-1` when no contest is open.
    ///
    /// This is the same kernel the native host reaches through
    /// `World::report_muscle_leg`; the browser holds no ladder rule of its
    /// own. The page has no persistent party record to carry HP across legs -
    /// each leg re-seeds from the fighter's full stats - so the returned
    /// figure is what the ladder *would* hand back, reported rather than
    /// applied.
    pub fn muscle_report_leg(
        &mut self,
        survived: bool,
        outcome: u32,
        turns_taken: u32,
        hp_max: u16,
        unlock: u32,
        gates: u32,
    ) -> i32 {
        use legaia_engine_core::muscle_dome::{ContestState, LegReport};
        let flags = web_contest_flags(unlock, gates, false);
        let Some(run) = self.muscle_run.as_mut() else {
            return -1;
        };
        run.finish_leg(
            LegReport {
                survived,
                outcome,
                turns_taken,
            },
            hp_max,
            &flags,
        );
        while matches!(
            run.state(),
            ContestState::LegScore | ContestState::Tally | ContestState::Restore
        ) {
            run.advance();
        }
        run.take_hp_restore(0, hp_max) as i32
    }

    /// Whether the leg boundary just reported by [`Self::muscle_report_leg`]
    /// raises the arena's between-legs INTERVAL + score-tally screen.
    ///
    /// The answer is the shared rule's
    /// ([`legaia_engine_core::muscle_dome::leg_boundary_raises_interval`]) -
    /// the same call the native window's `tick_muscle_hub` makes - so the page
    /// cannot grow a cadence of its own. It says `true` only for a survived leg
    /// with the course not yet exhausted; a lost, run-from or final leg settles
    /// and shows nothing.
    ///
    /// It is not a *turn* question. A turn boundary keeps the leg open and the
    /// arena hub unreached, so the page must go straight back to the command
    /// cluster there ([`legaia_engine_core::muscle_dome::MusclePhase::ends_turn`]).
    pub fn muscle_leg_shows_interval(&self) -> bool {
        legaia_engine_core::muscle_dome::leg_boundary_raises_interval(
            self.muscle_run.as_ref().map(|r| r.state()),
        )
    }

    /// One hub screen's retail fade / hold envelope, sampled at `tick`
    /// ticks in with `pad` as the arm's edge word - the browser twin of the
    /// native window's `tick_muscle_hub`, over the same
    /// [`legaia_engine_core::muscle_dome::HubScreen`] kernel, so neither host
    /// can pick a frame count of its own.
    ///
    /// `screen`: 0 = intro card, 1 = ROUND banner, 2 = opponent / ROUND-n
    /// card, 3 = the between-legs INTERVAL + tally. Returns
    /// `{brightness, stage, done, total}` - `brightness` is exactly the
    /// argument [`Self::muscle_hub_quads_json`] wants (`0 ..= 0x80`; `0x80`
    /// is the emitter's neutral, **not** `0x100`), `stage` is
    /// `0` fade-in / `1` hold / `2` fade-out / `3` done, and `total` is the
    /// screen's unskipped length in ticks.
    pub fn muscle_hub_screen_json(&self, screen: u32, tick: i32, pad: u32) -> String {
        use legaia_engine_core::muscle_dome as md;
        let mut env = match screen {
            0 => md::HubScreen::intro_card(),
            1 => md::HubScreen::round_banner(),
            2 => md::HubScreen::opponent_card(),
            _ => md::HubScreen::interval(
                md::HUB_TALLY_ROLL_LEAD_TICKS
                    + *md::HUB_TALLY_CUE_STAGGER.last().unwrap_or(&0) as i32,
            ),
        };
        let total = env.total_ticks();
        // The envelope is monotone and terminates, so a tick past its total
        // answers the same as the total - clamp rather than replay a page's
        // unbounded counter.
        for _ in 0..tick.clamp(0, total) {
            env.tick(1, pad as u16);
        }
        let stage = match env.stage() {
            md::HubScreenStage::FadeIn => 0,
            md::HubScreenStage::Hold => 1,
            md::HubScreenStage::FadeOut => 2,
            md::HubScreenStage::Done => 3,
        };
        serde_json::json!({
            "brightness": env.brightness(),
            "stage": stage,
            "done": env.done(),
            "total": total,
        })
        .to_string()
    }

    /// Settle the open contest if it has run out: pay the tally into the
    /// page's coin bank and latch the one-shot Master-course prize.
    ///
    /// Returns `true` when a settlement happened. `prize_awarded` is the
    /// `0x6CB` one-shot flag as the page knows it.
    pub fn muscle_contest_settle(&mut self, unlock: u32, gates: u32, prize_awarded: bool) -> bool {
        use legaia_engine_core::muscle_dome as md;
        let flags = web_contest_flags(unlock, gates, prize_awarded);
        let Some(run) = self.muscle_run.as_mut() else {
            return false;
        };
        if !run.over() {
            return false;
        }
        let out = run.settle(&flags);
        self.muscle_run = None;
        self.muscle_settlement = Some(out);
        self.muscle_coins = md::credit_casino_coins(self.muscle_coins, out.score);
        true
    }

    /// The contest layer's state for the page's chrome:
    ///
    /// ```json
    /// { "live": true, "course": 0, "round": 2, "length": 8, "tally": 8,
    ///   "over": false, "gave_up": false, "state": 20, "coins": 0,
    ///   "rows": { "round": 30, "turns": 25, "outcome": 20, "score": 5 },
    ///   "settlement": { "score": 818, "prize": false } }
    /// ```
    ///
    /// `rows` is the between-leg tally screen's four count-up lanes: the
    /// first three are HP recovery scaled by max HP, and only `score` is
    /// money.
    pub fn muscle_contest_json(&self, unlock: u32, gates: u32) -> String {
        let settlement = self.muscle_settlement.as_ref().map(|s| {
            serde_json::json!({ "score": s.score, "prize": s.award_prize,
                                "continuing": s.continuing })
        });
        let Some(run) = self.muscle_run.as_ref() else {
            return serde_json::json!({
                "live": false, "coins": self.muscle_coins, "settlement": settlement,
            })
            .to_string();
        };
        let flags = web_contest_flags(unlock, gates, false);
        let rows = run.rows();
        serde_json::json!({
            "live": true,
            "course": run.course(),
            "round": run.round(),
            "length": run.staged_course_length(&flags),
            "tally": run.tally(),
            "over": run.over(),
            "gave_up": run.gave_up(),
            "state": run.state() as u32,
            "coins": self.muscle_coins,
            "rows": {
                "round": rows.round_lane,
                "turns": rows.turns_lane,
                "outcome": rows.outcome_lane,
                "score": rows.score_cell,
            },
            "settlement": settlement,
        })
        .to_string()
    }

    /// The last resolved turn's play-by-play, for the page's 3D playback:
    ///
    /// ```json
    /// [ { "attacker": 0, "cmd": 12, "power": 10, "damage": 55,
    ///     "hp": [500, 345] }, ... ]
    /// ```
    pub fn muscle_round_log_json(&self) -> String {
        let Some(c) = self.muscle.as_ref() else {
            return "[]".to_string();
        };
        let plays: Vec<serde_json::Value> = c
            .session
            .last_turn_plays()
            .iter()
            .map(|p| {
                serde_json::json!({
                    "attacker": p.attacker,
                    "cmd": p.cmd,
                    "power": p.power,
                    "damage": p.damage,
                    "hp": p.hp_after,
                })
            })
            .collect();
        serde_json::Value::Array(plays).to_string()
    }

    /// Live contest state: `live`, `phase` (`select` / `resolve` /
    /// `turn_over` / `won` / `lost`), `hp`, `hp_max`, `mp_max`,
    /// `budget`, `spent`, `queue`, `last_damage`, `hand`, `reward_spell`,
    /// `names`, `spirit` (the `+0x170` gauges the dome HUD bars display),
    /// `stats` (per-fighter INT/UDF/LDF/element the formulas used), `source`
    /// (`"disc"` / `"fallback"` player record), `char`, `level`, `monster`.
    ///
    /// `turn` is the battle turn counter and is **not** accompanied by a
    /// remaining-turns field: a dome leg is an ordinary battle and ends on a
    /// KO. `hp_left` is the opponent's HP percentage (the number retail
    /// stamps at x=`0xd2`), `hp_left_pct` carries both fighters' percentages
    /// for the page's bars, and `time_meter` / `time_meter_max` mirror the
    /// `FUN_801d3444` ramp.
    pub fn muscle_state_json(&self) -> String {
        let Some(c) = self.muscle.as_ref() else {
            return r#"{"live":false}"#.to_string();
        };
        let s = &c.session;
        let phase = match s.phase() {
            MusclePhase::Select => "select",
            MusclePhase::Resolve => "resolve",
            MusclePhase::TurnOver => "turn_over",
            MusclePhase::Won => "won",
            MusclePhase::Lost => "lost",
        };
        let hand: Vec<serde_json::Value> = s
            .hand(0)
            .iter()
            .map(|card| serde_json::json!({ "cmd": card.command_id, "cost": card.cost }))
            .collect();
        let stats: Vec<serde_json::Value> = c
            .fighters
            .iter()
            .map(|f| {
                serde_json::json!({
                    "int": f.int, "udf": f.udf, "ldf": f.ldf,
                    "budget_pool": f.budget_pool, "element": f.element,
                })
            })
            .collect();
        serde_json::json!({
            "live": true,
            "phase": phase,
            "turn": s.turn(),
            "hp": [s.hp(0), s.hp(1)],
            "hp_max": [c.fighters[0].hp_max, c.fighters[1].hp_max],
            "mp_max": [c.fighters[0].mp_max, c.fighters[1].mp_max],
            "budget": [s.budget(0), s.budget(1)],
            "spent": [s.spent(0), s.spent(1)],
            "hp_left": s.hp_left(),
            "hp_left_pct": [s.hp_left_percent(0), s.hp_left_percent(1)],
            "time_meter": s.time_meter(),
            "time_meter_max": legaia_engine_core::muscle_dome::TIME_METER_MAX,
            "queue": [s.queue(0), s.queue(1)],
            "last_damage": s.last_turn_damage(),
            "hand": hand,
            "reward_spell": s.reward_spell_id(),
            "names": c.names,
            "spirit": [s.spirit(0), s.spirit(1)],
            "stats": stats,
            "source": c.stats_source,
            "char": c.char_slot,
            "level": c.level,
            "monster": c.monster_id,
            "mp": [s.mp(0), s.mp(1)],
            "magic_open": s.magic_open(),
            "magic_cursor": s.magic_cursor(),
            "cast": s.queued_cast(0),
            "chips": Self::muscle_chip_json(s),
        })
        .to_string()
    }

    /// The command ring's four chips as the page draws them:
    ///
    /// ```json
    /// [ { "chip": "item", "x": 204, "y": 34, "enabled": false,
    ///     "mark": "forbidden" }, ... ]
    /// ```
    ///
    /// `enabled` is the session's gate - the same one the native window
    /// reads - and `mark` names which of retail's three mark emitters lays
    /// over the chip (`forbidden` = the red cross-out X `FUN_801DBC30`,
    /// `blocked` = `FUN_801DBD04`, `sealed` = `FUN_801DBEC4`), or `null` for
    /// a chip that draws none. A chip can be disabled with **no** mark: a
    /// fighter carrying no Ra-Seru gets the `-` label, not an X.
    ///
    /// A `forbidden` chip also carries `mark_quad`, the red cross-out X as
    /// `FUN_801DBC30` itself places it (the phase-`0x28` arm calls it with the
    /// chip's anchor at `0x801D12D4` / `0x801D12F0`): `x`/`y`/`dw`/`dh` the
    /// screen rect, `u`/`v`/`w`/`h` the texels on the `etim` page, `pal` the
    /// sub-palette its CLUT word names.
    pub(super) fn muscle_chip_json(s: &MuscleDomeSession) -> Vec<serde_json::Value> {
        use legaia_engine_core::muscle_dome::{ChipMark, DomeRingChip};
        DomeRingChip::RING
            .iter()
            .map(|chip| {
                let (x, y) = chip.anchor();
                let mark_quad = (s.chip_mark(0, *chip) == Some(ChipMark::Forbidden)).then(|| {
                    let q = legaia_engine_vm::battle_party_panel::cross_out_mark(x, y);
                    serde_json::json!({
                        "x": q.xy[0].0, "y": q.xy[0].1,
                        "dw": i32::from(q.xy[1].0) - i32::from(q.xy[0].0) + 1,
                        "dh": i32::from(q.xy[2].1) - i32::from(q.xy[0].1) + 1,
                        "u": q.uv[0].0, "v": q.uv[0].1,
                        "w": i32::from(q.uv[1].0) - i32::from(q.uv[0].0) + 1,
                        "h": i32::from(q.uv[2].1) - i32::from(q.uv[0].1) + 1,
                        "pal": q.clut & 0x3F,
                        "tpage": q.tpage,
                    })
                });
                let mark = s.chip_mark(0, *chip).map(|m| match m {
                    ChipMark::Forbidden => "forbidden",
                    ChipMark::Blocked => "blocked",
                    ChipMark::Sealed => "sealed",
                });
                serde_json::json!({
                    "chip": match chip {
                        DomeRingChip::Item => "item",
                        DomeRingChip::Attack => "attack",
                        DomeRingChip::RaSeru => "raseru",
                        DomeRingChip::Spirit => "spirit",
                    },
                    "x": x,
                    "y": y,
                    "enabled": s.chip_enabled(0, *chip),
                    "mark": mark,
                    "mark_quad": mark_quad,
                })
            })
            .collect()
    }

    /// The player fighter's Ra-Seru list, priced through the session's own
    /// [`MuscleDomeSession::spell_mp_cost`] so the displayed cost is the one
    /// the cast charges:
    ///
    /// ```json
    /// [ { "id": 129, "name": "Gimard", "mp": 8, "affordable": true }, ... ]
    /// ```
    pub fn muscle_magic_json(&self) -> String {
        let Some(c) = self.muscle.as_ref() else {
            return "[]".to_string();
        };
        let rows: Vec<serde_json::Value> = c
            .session
            .spell_rows(0)
            .into_iter()
            .map(|r| {
                serde_json::json!({
                    "id": r.id, "name": r.name,
                    "mp": r.mp_cost, "affordable": r.affordable,
                })
            })
            .collect();
        serde_json::to_string(&rows).unwrap_or_else(|_| "[]".to_string())
    }

    /// Take the ring's Ra-Seru chip. Returns `""` on success, else the
    /// refusal name (`no_loadout` / `no_raseru` / `sealed` / `forbidden` /
    /// `wrong_phase`) so the page can play the refused blip and say why.
    pub fn muscle_open_magic(&mut self) -> String {
        let Some(c) = self.muscle.as_mut() else {
            return "no_loadout".to_string();
        };
        match c.session.open_magic(0) {
            Ok(()) => String::new(),
            Err(e) => muscle_refusal_name(e).to_string(),
        }
    }

    /// Back out of the open Ra-Seru list.
    pub fn muscle_close_magic(&mut self) {
        if let Some(c) = self.muscle.as_mut() {
            c.session.close_magic();
        }
    }

    /// Walk the open list's cursor.
    pub fn muscle_magic_move(&mut self, delta: i32) {
        if let Some(c) = self.muscle.as_mut() {
            c.session.move_magic_cursor(0, delta);
        }
    }

    /// Confirm the row under the cursor. Returns `""` on success, else the
    /// refusal name (`not_enough_mp` is the one the page shows most).
    pub fn muscle_magic_confirm(&mut self) -> String {
        let Some(c) = self.muscle.as_mut() else {
            return "no_loadout".to_string();
        };
        match c.session.confirm_magic(0) {
            Ok(_) => String::new(),
            Err(e) => muscle_refusal_name(e).to_string(),
        }
    }

    /// Name of spell id `id` from the SCUS spell-name table (the table the
    /// dome's victory banner reads at `DAT_800754d0`). Empty when no
    /// executable was loaded (raw `PROT.DAT` input).
    pub fn muscle_spell_name(&self, id: u8) -> String {
        self.scus
            .as_ref()
            .and_then(|s| legaia_asset::spell_names::SpellNameTable::from_scus(s))
            .and_then(|t| t.name(id).map(str::to_owned))
            .unwrap_or_default()
    }

    /// The monster archive roster, for the page's opponent picker:
    ///
    /// ```json
    /// [ { "id": 1, "name": "Gimard", "hp": 43, "agl": 60, "atk": 15,
    ///     "udf": 14, "ldf": 14, "int": 8, "spd": 12, "element": 2 }, ... ]
    /// ```
    ///
    /// Stats are the boosted battle profile (`battle_stats()`), i.e. the
    /// numbers the contest actually fights with. Only records with a
    /// decodable mesh + idle animation are listed (the dome renders its
    /// opponent in 3D). Names are the archive's own.
    pub fn muscle_roster_json(&self) -> String {
        let Some(entry) = self.monster_archive_entry() else {
            return "[]".to_string();
        };
        let Ok(records) = monster_archive::records(entry) else {
            return "[]".to_string();
        };
        let rows: Vec<serde_json::Value> = records
            .iter()
            .filter(|r| {
                r.hp > 0
                    && matches!(monster_archive::mesh(entry, r.id), Ok(Some(_)))
                    && matches!(monster_archive::idle_animation(entry, r.id), Ok(Some(_)))
            })
            .map(|r| {
                let bs = r.battle_stats();
                serde_json::json!({
                    "id": r.id, "name": r.name, "hp": r.hp,
                    "agl": bs[0], "atk": bs[1], "udf": bs[2], "ldf": bs[3],
                    "int": bs[4], "spd": bs[5], "element": r.element,
                })
            })
            .collect();
        serde_json::Value::Array(rows).to_string()
    }
}
