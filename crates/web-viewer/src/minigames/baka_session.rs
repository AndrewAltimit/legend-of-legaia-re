//! The Baka Fighter rules-engine shell: roster, the start paths, the duel
//! tick, the cabinet run, and the names / ladder tables.

use super::*;

#[wasm_bindgen]
impl LegaiaMinigames {
    // ---------------------------------------------------------- baka fighter

    /// The parsed roster, for the opponent picker. The disc carries no names for
    /// these fighters - only their numbers - so each row is the record's own
    /// stat block:
    ///
    /// ```json
    /// [ { "id": 1, "gold": 30, "damage_mod": 20, "crit_chance": 10,
    ///     "atk_tiers": [..], "def_tiers": [..], "pattern": [2,1,3],
    ///     "power": [..] }, ... ]
    /// ```
    pub fn baka_roster_json(&self) -> String {
        let Some((opponents, actions)) = self.baka_tables.as_ref() else {
            return "[]".to_string();
        };
        let rows = opponents
            .iter()
            .map(|o| {
                let list = |v: &[i32]| {
                    v.iter()
                        .map(|x| x.to_string())
                        .collect::<Vec<_>>()
                        .join(",")
                };
                let power = actions
                    .get(o.index)
                    .map(|a| {
                        (1..=4u8)
                            .map(|t| a.attack_power(t).unwrap_or(0).to_string())
                            .collect::<Vec<_>>()
                            .join(",")
                    })
                    .unwrap_or_default();
                format!(
                    concat!(
                        r#"{{"id":{},"gold":{},"damage_mod":{},"crit_chance":{},"#,
                        r#""atk_tiers":[{}],"def_tiers":[{}],"pattern":[{}],"power":[{}]}}"#
                    ),
                    o.index,
                    o.gold_reward,
                    o.damage_mod,
                    o.crit_chance,
                    list(&o.atk_tiers),
                    list(&o.def_tiers),
                    o.ai_pattern
                        .iter()
                        .map(|b| b.to_string())
                        .collect::<Vec<_>>()
                        .join(","),
                    power,
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!("[{rows}]")
    }

    /// Start a best-of-3 duel as roster fighter 0 (Vahn) against `opponent`
    /// - [`Self::baka_start_as`] with the player-select cursor on its first
    /// column.
    pub fn baka_start(&mut self, opponent: usize, seed: u32) -> bool {
        self.baka_start_as(0, opponent, seed)
    }

    /// Start a best-of-3 duel: the visitor fights as party fighter `player`
    /// (`0..=2` - Vahn, Noa, Gala; the PLAYER SELECT column) against roster
    /// `opponent`. The pick is the player seat's **roster record**, not a skin:
    /// the round setup stores the select cursor `DAT_801DBF70` as slot 0's
    /// roster id (`sw a0,0x94(v1)` = `DAT_801DC050` at `0x801D0058`) and reads
    /// that record's `+0x44` stand-off off the same index
    /// (`0x801D0040..0x801D005C`), so the stats, action table, special camera
    /// and stand-off are the picked fighter's. Returns `false` when the tables
    /// didn't decode or a roster id is out of range.
    pub fn baka_start_as(&mut self, player: usize, opponent: usize, seed: u32) -> bool {
        if player >= legaia_engine_minigames::baka_cabinet::SELECT_OPTIONS as usize {
            return false;
        }
        match self.baka_fight_for(player, opponent, seed) {
            Some(f) => {
                self.baka = Some(f);
                true
            }
            None => false,
        }
    }

    /// Start the cabinet at its **boot** state: the attract card, then the
    /// player select, then the first rung - the front end the native window
    /// and the play page enter the cabinet on
    /// ([`BakaFight::with_attract`]). The pad reaches it through
    /// [`Self::baka_cabinet_pad`]; [`Self::baka_cabinet_json`] reports where
    /// it is and what it draws.
    ///
    /// Seeded the way the play hosts seed a cabinet
    /// ([`legaia_engine_minigames::baka_fighter::BAKA_RNG_BASE`] folded with the
    /// frame count), not with a page random.
    pub fn baka_start_cabinet(&mut self) -> bool {
        let seed = legaia_engine_minigames::baka_fighter::BAKA_RNG_BASE ^ self.sim_frames;
        self.baka_winnings = 0;
        let first = legaia_engine_minigames::baka_fighter::first_rung_roster();
        match self.baka_fight_for(0, first, seed) {
            Some(f) => {
                self.baka = Some(f.with_attract());
                true
            }
            None => false,
        }
    }
}

impl LegaiaMinigames {
    /// The fight every start path seats: the parsed tables plus every
    /// roster's clip headers, the special cameras and the impact templates.
    fn baka_fight_for(&self, player: usize, opponent: usize, seed: u32) -> Option<BakaFight> {
        let (opponents, actions) = self.baka_tables.as_ref()?;
        match BakaFight::from_tables(opponents, actions, player, opponent, seed) {
            Some(f) => {
                // Every roster fighter's clip headers, off the same banks this
                // page poses the fighters from, through the engine's one
                // loader (the play hosts stage the same) - so the strike clock
                // and the special's afterimage step and expire on the real
                // clips.
                let headers = legaia_engine_minigames::baka_fighter::roster_clip_headers(|i| {
                    self.baka_entry(i).map(<[u8]>::to_vec)
                });
                let img = overlay_image(
                    &self.prot,
                    &self.entries,
                    legaia_minigame_formats::baka_opponents::BAKA_OVERLAY_PROT_INDEX as u32,
                );
                let cameras = img
                    .as_deref()
                    .map(legaia_engine_minigames::baka_duel::parse_special_cameras)
                    .unwrap_or_default();
                // The impact pair's four effect templates, so this page's
                // duel spawns and steps the same parts the play hosts draw.
                let f = f
                    .with_roster_clip_headers(headers)
                    .with_special_cameras(cameras);
                Some(match img.as_deref() {
                    Some(i) => f.with_impact_overlay(i),
                    None => f,
                })
            }
            None => None,
        }
    }
}

#[wasm_bindgen]
impl LegaiaMinigames {
    /// Advance the duel one frame's worth of `frame_step` (the retail SM's
    /// per-frame delta; `1` is a normal frame).
    ///
    /// The rules kernel buffers a sound cue per exchange hit
    /// (`BakaFight::cues`), and **this host never drained it**: the other two
    /// take it every frame (`drain_baka_sfx_cues` natively, `tick_baka_ui` on
    /// the play page), so on this page the duel was both silent and growing a
    /// vector for the length of a ladder run. Drained here, audio up or not -
    /// a muted page must not accumulate a queue either.
    pub fn baka_tick(&mut self, frame_step: i32) {
        let cues: Vec<u8> = match self.baka.as_mut() {
            Some(f) => {
                f.tick(frame_step);
                f.take_cues()
            }
            None => return,
        };
        for id in cues {
            self.minigame_sfx_cue(u16::from(id));
        }
        // The round chrome's announcer line (`FUN_8003D53C`), the native
        // window's `tick_baka_chrome` / the play page's `tick_baka_ui` twin.
        if let Some(xa) = self.baka.as_ref().and_then(|f| f.chrome_frame().xa) {
            self.play_baka_xa(xa);
        }
    }

    /// The round chrome's draws this frame (`BakaChrome` - the intro title
    /// card, the ROUND banner and the READY / FIGHT countdown), the same
    /// runner the native window and the play page draw from:
    ///
    /// ```json
    /// [ { "w": 3, "x": 160, "y": 100, "b": 128, "s": 4096, "g": null }, ... ]
    /// ```
    ///
    /// `w` is the HUD widget id, `x`/`y` the quad centre, `b` the brightness
    /// (`0x80` = the descriptor's own RGB), `s` the 20.12 size scale, `g`
    /// the glyph-strip cell a digit draw pages widget 5 to (`null` for a
    /// plain widget) and `u` the texel column that paging stamps into widget
    /// 5's record - [`legaia_engine_minigames::baka_fighter_chrome::glyph_u`], the
    /// byte store retail's glyph wrapper performs (so it wraps at 256), and
    /// the same stamp the native window resolves its chrome draws with.
    /// `[]` outside a duel.
    pub fn baka_chrome_json(&self) -> String {
        let Some(f) = self.baka.as_ref() else {
            return "[]".to_string();
        };
        let rows: Vec<serde_json::Value> = f
            .chrome_frame()
            .draws
            .iter()
            .map(|d| {
                serde_json::json!({
                    "w": d.widget, "x": d.x, "y": d.y,
                    "b": d.brightness, "s": d.size, "g": d.glyph,
                    "u": d.glyph.map(legaia_engine_minigames::baka_fighter_chrome::glyph_u),
                })
            })
            .collect();
        serde_json::Value::Array(rows).to_string()
    }

    /// Commit the visitor's attack this exchange: `1`/`2`/`3` are the three
    /// rock-paper-scissors throws, `4` the special. Returns `false` when the
    /// fighter can't act yet (cooldown, or a choice is already pending).
    pub fn baka_choose(&mut self, attack: u8) -> bool {
        let Some(f) = self.baka.as_mut() else {
            return false;
        };
        let Some(a) = BakaAttack::from_type_id(attack) else {
            return false;
        };
        f.choose(0, a)
    }

    /// Live duel state.
    ///
    /// ```json
    /// { "live": true, "phase": "fighting"|"round_over"|"match_over",
    ///   "round": 0, "hp": [3200, 2900], "hp_start": 3200,
    ///   "wins": [0, 1], "combo": [0, 2], "chosen": [2, null],
    ///   "can_choose": true, "clock": [96, 0],
    ///   "ghosts": [ { "owner": 0, "passes": [ { "bit": 0, "frame": 3, "cue": 2048 } ] } ],
    ///   "gold": 30, "winner": null,
    ///   "last": { "winner": 0, "draw": false, "damage": 512,
    ///             "critical": false, "special": false } }
    /// ```
    ///
    /// `clock` is each fighter's strike-clock cursor (1/16 clip frame) over
    /// its chosen attack - the frame the retail clip is on, since the strike
    /// lands when it crosses the action's keyframe. `ghosts` are the special's
    /// afterimage passes that drew this tick.
    pub fn baka_state_json(&self) -> String {
        match self.baka.as_ref() {
            Some(f) => baka_presentation::baka_state_json_for(f),
            None => r#"{"live":false}"#.to_string(),
        }
    }

    // ------------------------------------------------- baka fighter: cabinet run

    /// One frame of the whole cabinet - attract card, player select, duel,
    /// result tally, NEXT GAME / PAY OUT sheet, the next rung and the
    /// game-over / all-clear sequence - on this frame's **packed** pad edge
    /// and held words: [`BakaFight::frame`], the per-frame step the play hosts'
    /// world tick runs. The page keeps the winnings accumulator the play hosts
    /// keep on the world.
    ///
    /// The page used to leave the cabinet after its player select and run a
    /// page-side ladder instead (`LadderRun`, with its own pot, its own HTML
    /// NEXT GAME / PAY OUT menu and a free choice of starting rung) - a second
    /// rule set beside the cabinet's own ladder. It is gone.
    ///
    /// ```json
    /// { "paid": 0, "forfeit": false, "exit": false, "winnings": 30 }
    /// ```
    pub fn baka_frame(&mut self, edge: u16, held: u16) -> String {
        let Some(f) = self.baka.as_mut() else {
            return r#"{"paid":0,"forfeit":false,"exit":false,"winnings":0}"#.to_string();
        };
        let out = f.frame(edge, held, self.baka_winnings);
        let cues = f.take_cues();
        let xa = f.chrome_frame().xa;
        if out.paid > 0 {
            self.baka_winnings = self.baka_winnings.saturating_add(out.paid);
        }
        if out.forfeit {
            self.baka_winnings = 0;
        }
        for id in cues {
            self.minigame_sfx_cue(u16::from(id));
        }
        if let Some(xa) = xa {
            self.play_baka_xa(xa);
        }
        serde_json::json!({
            "paid": out.paid,
            "forfeit": out.forfeit,
            "exit": out.exit,
            "winnings": self.baka_winnings,
        })
        .to_string()
    }

    // ------------------------------------------------------- baka: names + ladder

    /// The 17 fighter names, in roster order, read out of the roster records
    /// (`+0x00`, 32-byte ASCII). Empty when the overlay didn't decode.
    pub fn baka_names_json(&self) -> String {
        let Some(names) = self.baka_names.as_ref() else {
            return "[]".to_string();
        };
        format!(
            "[{}]",
            names.iter().map(|n| jstr(n)).collect::<Vec<_>>().join(",")
        )
    }

    /// The ladder the cabinet actually serves, as `[{stage, roster}]`.
    ///
    /// The stage counter starts at **2** and `roster = stage + 3`, so the first
    /// lap is roster ids `5..=16` - across which the prize gold is strictly
    /// monotonic. Roster `3` and `4` are only reachable after the all-clear
    /// wraps the counter, which is why the roster's gold column looks out of
    /// order if you read it straight down.
    pub fn baka_ladder_json(&self) -> String {
        let rows = minigame_art::baka_ladder()
            .into_iter()
            .map(|(stage, roster)| format!(r#"{{"stage":{stage},"roster":{roster}}}"#))
            .collect::<Vec<_>>()
            .join(",");
        format!("[{rows}]")
    }
}
