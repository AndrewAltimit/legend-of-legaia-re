//! The three **3D-scene** minigames on the play page - the Muscle Dome, the
//! Baka Fighter duel and the Noa dance - as the page draws them: the live
//! session read-outs, the per-game text HUD, and delegates to the shared
//! presentation bundle for the meshes, animation banks, VRAM and hub art.
//!
//! The rules engines run in the engine's `World` off the routed pad word:
//!
//! - **Muscle Dome** (`World::tick_muscle_dome`): the four direction chips
//!   commit swings under the AP budget, Cross fights, Triangle opens the
//!   Ra-Seru list, Circle cancels; a decided leg is reported to the open
//!   contest on Cross and a finished ladder settles into the coin bank. The
//!   door warp opens **no contest** (the scene host stages stand-ins), so
//!   this host opens one on entry from the arena overlay's course ladder and
//!   the world's unlock flags - the same `DomeContest::from_overlay` the
//!   native launcher runs - which is what makes a leg pay, and what names the
//!   monster the page draws. The hub screens (intro card, ROUND banner,
//!   between-legs INTERVAL + score tally) run retail's own fade / hold
//!   envelopes ([`legaia_engine_core::muscle_dome::HubScreen`]) off the
//!   world's leg / contest edges, ported from the native window's
//!   `tick_muscle_hub`; the quads come out of the PROT 0977 sprite table
//!   through the shared `other_game_hud` emitters.
//! - **Baka Fighter** (`World::tick_baka_fighter`): Left / Right / Up commit
//!   attack types 1 / 2 / 3, Down the special; the result screen's tally
//!   banks into the mode-24 winnings the return warp pays out. The opponent
//!   the scene host rotated in is reproduced here (the same frame-keyed pick,
//!   cross-checked against the fight's prize) so the page draws the fighter
//!   the rules are running.
//! - **Dance** (`World::tick_dance`): Square / Circle / Triangle are the
//!   judged buttons; the song ends the run on its own.
//!
//! Start leaves any of the three (`World::poll_minigame_escape`).

use legaia_engine_core::baka_fighter::MatchPhase;
use legaia_engine_core::dance::{DanceGame, Judge};
use legaia_engine_core::muscle_dome::{
    self as md, DomeContest, HubScreen, MuscleDomeSession, MusclePhase,
};
use legaia_engine_core::other_game_overlay::ScoreTallyRamp;
use legaia_engine_ui::TextDraw;
use legaia_engine_ui::other_game_hud::{self as hud, HudQuad, HudSprite};
use wasm_bindgen::prelude::*;

use crate::play_minigames::{DIM, PEN_CONTEST, PEN_EXTRA, PEN_PROMPT, PEN_STATUS, WHITE, row};
use crate::runtime::LegaiaRuntime;

/// PROT entry of the arena roster / init overlay (course ladder, score
/// table, hub sprite table).
const ARENA_OVERLAY_PROT_INDEX: u32 = md::ARENA_OVERLAY_PROT_INDEX as u32;
/// PROT entry of the dome data container whose LZS section 0 carries the
/// two hub-page TIMs (`other6.lzs` slot 0).
const HUB_CONTAINER_PROT_INDEX: u32 = 1220;

// ------------------------------------------------------------- Muscle Dome

/// Muscle Dome presentation state: the staged opponent and the hub-screen
/// timers the native window's `tick_muscle_hub` keeps.
#[derive(Default)]
pub(crate) struct MuscleUi {
    /// The monster the contest's `(course, round)` stages, off the PROT 0977
    /// ladder. `None` when the ladder did not decode (the page then draws
    /// the text HUD alone).
    pub(crate) monster_id: Option<u16>,
    /// Player battle-file slot the fighter mesh assembles from (0 = Vahn).
    pub(crate) char_slot: u32,
    intro_card: Option<HubScreen>,
    round_banner: Option<(i32, HubScreen)>,
    interval: Option<HubScreen>,
    tally: Option<(ScoreTallyRamp, i32)>,
    prev_leg_open: bool,
    prev_contest_open: bool,
    /// Pristine parse of the PROT 0977 sprite table; the emitters write
    /// variants back, so every frame runs over a copy.
    sprite_table: Option<Vec<HudSprite>>,
    prev_phase: Option<MusclePhase>,
    /// Count of turns resolved this leg (the page's clip-trigger edge).
    pub(crate) turns_resolved: u32,
}

impl LegaiaRuntime {
    fn muscle_session(&self) -> Option<&MuscleDomeSession> {
        self.scene_host
            .as_ref()?
            .world
            .minigames
            .muscle_dome
            .as_ref()
    }

    /// Entry: open the contest the door warp did not, stage the ladder's
    /// monster, parse the hub sprite table.
    pub(crate) fn enter_muscle_ui(&mut self) {
        self.minigame_ui.muscle = MuscleUi::default();
        let Some(host) = self.scene_host.as_mut() else {
            return;
        };
        let raw = host
            .index
            .entry_bytes_extended(ARENA_OVERLAY_PROT_INDEX)
            .ok();
        if host.world.minigames.muscle_contest.is_none()
            && let Some(raw) = raw.as_deref()
        {
            let flags = host.world.muscle_contest_flags();
            host.world.minigames.muscle_contest = DomeContest::from_overlay(raw, &flags);
        }
        let (course, round) = host
            .world
            .minigames
            .muscle_contest
            .as_ref()
            .map_or((0usize, 0u32), |c| (c.course(), c.round()));
        let ui = &mut self.minigame_ui.muscle;
        ui.monster_id = raw
            .as_deref()
            .and_then(md::parse_course_ladder)
            .and_then(|ladder| {
                let rounds = &ladder.get(course)?.rounds;
                let n = (round as usize).min(rounds.len().checked_sub(1)?);
                Some(rounds.get(n)?.monster_id as u16)
            });
        ui.sprite_table = raw.as_deref().map(hud::parse_sprite_table);
        ui.char_slot = 0;
    }

    /// Exit: a ladder that has run out settles into the coin bank (a no-op
    /// mid-ladder, and a no-op after the engine's own Won/Lost settle).
    pub(crate) fn exit_muscle_ui(&mut self) {
        if let Some(host) = self.scene_host.as_mut() {
            host.world.settle_muscle_contest();
        }
    }

    /// Per-tick inside the dome: the round time meter (retail's per-frame
    /// arena driver step, which the engine's pad tick does not run) and the
    /// turn-resolved edge the page keys its swing clips on.
    pub(crate) fn tick_muscle_ui(&mut self) {
        let phase = self.muscle_session().map(|s| s.phase());
        if let Some(host) = self.scene_host.as_mut()
            && let Some(s) = host.world.minigames.muscle_dome.as_mut()
        {
            s.tick_time_meter(1);
        }
        let ui = &mut self.minigame_ui.muscle;
        if ui.prev_phase == Some(MusclePhase::Resolve) && phase != Some(MusclePhase::Resolve) {
            ui.turns_resolved = ui.turns_resolved.wrapping_add(1);
        }
        ui.prev_phase = phase;
    }

    /// The hub-screen timers, off the world's leg / contest edges - the port
    /// of the native window's `tick_muscle_hub`. Runs every frame: the
    /// INTERVAL + tally screen plays after the leg has closed.
    pub(crate) fn tick_muscle_hub(&mut self) {
        let Some(host) = self.scene_host.as_ref() else {
            return;
        };
        let world = &host.world;
        let pad = world.input.retail_pad().pressed as u16;
        let volume_word = legaia_engine_core::new_game::GAME_STATE_COLD_RESET.voice_volume as u32;
        let leg_open = world.minigames.muscle_dome.is_some();
        let contest_open = world.minigames.muscle_contest.is_some();
        let round = world
            .minigames
            .muscle_contest
            .as_ref()
            .map_or(1, |c| c.round() as i32 + 1);
        let raises = md::leg_boundary_raises_interval(
            world.minigames.muscle_contest.as_ref().map(|c| c.state()),
        );
        let roll_seed = world
            .minigames
            .muscle_contest
            .as_ref()
            .map(|c| c.tally_roll());
        let ui = &mut self.minigame_ui.muscle;
        if leg_open && !ui.prev_leg_open {
            ui.round_banner = Some((round, HubScreen::round_banner()));
            if contest_open && !ui.prev_contest_open {
                ui.intro_card = Some(HubScreen::intro_card());
            }
            ui.interval = None;
        }
        if !leg_open && ui.prev_leg_open && ui.prev_contest_open {
            let roll = md::HUB_TALLY_ROLL_LEAD_TICKS
                + *md::HUB_TALLY_CUE_STAGGER.last().unwrap_or(&0) as i32;
            ui.interval = raises.then(|| HubScreen::interval(roll));
            ui.tally = if raises { roll_seed } else { None };
            ui.intro_card = None;
            ui.round_banner = None;
        }
        if let Some(card) = ui.intro_card.as_mut() {
            card.tick(1, pad);
            if card.done() {
                ui.intro_card = None;
            }
        } else if let Some((_, banner)) = ui.round_banner.as_mut() {
            banner.tick(1, pad);
            if banner.done() {
                ui.round_banner = None;
            }
        }
        // Each drained lane keys a voice directly, with no cue id in sight
        // (`FUN_801D1288` builds the whole attr set), so nothing in the
        // id-keyed scheduler could sound it. Collected here and keyed below,
        // once the `minigame_ui` borrow is done.
        let mut voice_cues = Vec::new();
        if let Some(interval) = ui.interval.as_mut() {
            interval.tick(1, pad);
            if let Some((ramp, tally)) = ui.tally.as_mut() {
                let step = ramp.tick(1, false, volume_word);
                *tally += step.tally_gain;
                voice_cues.extend(step.cues.iter().copied());
            }
            if interval.done() {
                ui.interval = None;
                ui.tally = None;
            }
        }
        ui.prev_leg_open = leg_open;
        ui.prev_contest_open = contest_open;
        for cue in voice_cues {
            self.key_on_voice_attr(legaia_engine_audio::VoiceAttr::from_cue_words(
                cue.voice,
                cue.vab_program_tone,
                cue.note_and_fine,
                cue.volume,
            ));
        }
    }

    /// This frame's hub-screen quads, the native `muscle_hub_sprite_draws`
    /// selection over the shared emitters.
    fn muscle_hub_quads(&self) -> Vec<HudQuad> {
        let ui = &self.minigame_ui.muscle;
        let Some(table) = ui.sprite_table.as_ref() else {
            return Vec::new();
        };
        let Some(host) = self.scene_host.as_ref() else {
            return Vec::new();
        };
        let world = &host.world;
        let in_dome = world.mode == legaia_engine_core::world::SceneMode::MuscleDome;
        let mut table = table.clone();
        let mut quads = Vec::new();
        if in_dome {
            if let Some(card) = ui.intro_card {
                quads.extend(hud::hub_screen_quads(
                    &mut table,
                    hud::HUB_INTRO_CARD,
                    card.brightness(),
                ));
            } else if let Some((round, banner)) = ui.round_banner {
                quads.extend(hud::hub_screen_quads(
                    &mut table,
                    &hud::round_banner_draws(round),
                    banner.brightness(),
                ));
            }
        } else if let Some(interval) = ui.interval {
            let bright = interval.brightness();
            quads.extend(hud::hub_screen_quads(
                &mut table,
                hud::HUB_INTERVAL_HEADING,
                bright,
            ));
            let (values, row_bright) = match ui.tally.as_ref() {
                Some((ramp, tally)) => (ramp.row_values(*tally), ramp.row_brightness(bright)),
                None => {
                    let (rows, tally) = world
                        .minigames
                        .muscle_contest
                        .as_ref()
                        .map_or((Default::default(), 0), |c| (c.rows(), c.tally()));
                    ([0, 0, 0, rows.hp_restore(), 0, tally], [bright; 6])
                }
            };
            quads.extend(hud::score_tally_quads(&mut table, values, row_bright));
        }
        quads
    }

    /// The native window's Muscle Dome HUD lines, with the page's bindings.
    pub(crate) fn muscle_status_draws(&self, font: &legaia_font::Font) -> Vec<TextDraw> {
        let Some(s) = self.muscle_session() else {
            return Vec::new();
        };
        let Some(host) = self.scene_host.as_ref() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        if let Some(c) = &host.world.minigames.muscle_contest {
            let flags = host.world.muscle_contest_flags();
            let l0 = format!(
                "Course {}  Round {}/{}   Coins banked: {}",
                c.course() + 1,
                c.round() + 1,
                c.staged_course_length(&flags),
                c.tally(),
            );
            out.extend(row(font, &l0, PEN_CONTEST, WHITE));
        }
        let l1 = format!("      Turn: {}         HP Left: {}", s.turn(), s.hp_left());
        out.extend(row(font, &l1, PEN_STATUS, WHITE));
        let status = match s.phase() {
            MusclePhase::Select if s.magic_open() => {
                let rows = s.spell_rows(0);
                let cursor = s.magic_cursor() as usize;
                let line = rows
                    .get(cursor)
                    .map(|r| {
                        format!(
                            "{} ({} MP){}",
                            r.name,
                            r.mp_cost,
                            if r.affordable {
                                ""
                            } else {
                                "  - not enough MP"
                            }
                        )
                    })
                    .unwrap_or_else(|| "(no Seru learned)".to_string());
                format!(
                    "Ra-Seru {}/{}: {}   MP {}   (Up/Down, Cross = cast, Circle = back)",
                    cursor + 1,
                    rows.len().max(1),
                    line,
                    s.mp(0),
                )
            }
            MusclePhase::Select => {
                let h = s.hand(0);
                let chip = if s.chip_enabled(0, md::DomeRingChip::RaSeru) {
                    "  Triangle = Ra-Seru"
                } else {
                    ""
                };
                format!(
                    "AP L:{} R:{} U:{} D:{}  budget {}  entered {}  (Cross = fight){chip}",
                    h[0].cost,
                    h[1].cost,
                    h[2].cost,
                    h[3].cost,
                    s.budget(0),
                    s.queue(0).len()
                )
            }
            MusclePhase::Resolve => "resolving...".to_string(),
            MusclePhase::TurnOver => {
                let [taken, dealt] = s.last_turn_damage();
                format!("turn: dealt {dealt}, took {taken}")
            }
            // The caption names a spell; it awards nothing (the contest's
            // payout lands when the ladder settles). This host dropped the id
            // and printed the bare banner, so the one piece of information
            // the Won caption carries was visible on the native window only.
            MusclePhase::Won => format!(
                "LEG WON! caption spell {:#x}  (Cross = next leg)",
                s.reward_spell_id()
            ),
            MusclePhase::Lost => "you lose the leg  (Cross = leave)".to_string(),
        };
        let l2 = format!(
            "{status}   you {}hp  foe {}hp  time {}/{}   (Start = quit)",
            s.hp(0),
            s.hp(1),
            s.time_meter(),
            md::TIME_METER_MAX,
        );
        out.extend(row(font, &l2, PEN_PROMPT, DIM));
        out
    }

    /// The two hub-page TIMs out of the dome data container (extraction
    /// 1220, LZS section 0 = `[12-byte header][TIM][TIM]`).
    fn muscle_hub_tims(&self) -> Option<(legaia_tim::Tim, legaia_tim::Tim)> {
        let host = self.scene_host.as_ref()?;
        let entry = host
            .index
            .entry_bytes_extended(HUB_CONTAINER_PROT_INDEX)
            .ok()?;
        let sections = legaia_lzs::decompress_container(&entry).ok()?;
        let blob = sections.first()?;
        let t0 = legaia_tim::parse(blob.get(0xC..)?).ok()?;
        let t1 = legaia_tim::parse(blob.get(0xC + t0.byte_extent()..)?).ok()?;
        Some((t0, t1))
    }
}

// ------------------------------------------------------------ Baka Fighter

/// Baka Fighter presentation state: which roster fighter the rules are
/// running against.
#[derive(Default)]
pub(crate) struct BakaUi {
    /// Roster id of the opponent (`1..=16`); `0` until an entry resolves it.
    pub(crate) opponent: usize,
    /// PROT 1204 slot the player-side mesh comes from (0 = Vahn).
    pub(crate) player_char: u32,
}

impl LegaiaRuntime {
    fn baka_session(&self) -> Option<&legaia_engine_core::baka_fighter::BakaFight> {
        self.scene_host
            .as_ref()?
            .world
            .minigames
            .baka_fighter
            .as_ref()
    }

    /// Entry: reproduce the scene host's opponent pick (`1 + frame % (n-1)`
    /// at the drain tick, which is this tick), then cross-check it against
    /// the fight's prize - the roster row the rules engine actually holds -
    /// and fall back to the row whose prize matches when the frame-keyed
    /// guess disagrees.
    pub(crate) fn enter_baka_ui(&mut self) {
        use legaia_asset::static_overlay;
        self.minigame_ui.baka = BakaUi::default();
        let Some(host) = self.scene_host.as_ref() else {
            return;
        };
        let frame = host.world.frame as u32;
        let prize = self.baka_session().map(|f| f.gold_reward());
        let roster = static_overlay::overlay_map()
            .by_prot_index(legaia_asset::baka_opponents::BAKA_OVERLAY_PROT_INDEX as u32)
            .and_then(|rec| {
                let raw = host.index.entry_bytes_extended(rec.prot_index).ok()?;
                let loaded = static_overlay::as_loaded(&raw, rec).ok()?;
                legaia_asset::baka_opponents::parse(&loaded)
            });
        let Some(roster) = roster else {
            return;
        };
        let guess = 1 + (frame as usize % roster.len().saturating_sub(1).max(1));
        let opponent = match prize {
            Some(p) if roster.get(guess).is_some_and(|o| o.gold_reward == p) => guess,
            Some(p) => roster
                .iter()
                .position(|o| o.index != 0 && o.gold_reward == p)
                .unwrap_or(guess),
            None => guess,
        };
        self.minigame_ui.baka.opponent = opponent;
    }

    /// Per-tick inside the duel: drain the rules kernel's SFX cues (the
    /// exchange hit, `BAKA_CUE_HIT`) into the page's scheduler - the native
    /// window's `drain_baka_sfx_cues`.
    pub(crate) fn tick_baka_ui(&mut self) {
        let cues: Vec<u8> = self
            .scene_host
            .as_mut()
            .and_then(|h| h.world.minigames.baka_fighter.as_mut())
            .map(|f| f.take_cues())
            .unwrap_or_default();
        for id in cues {
            self.minigame_sfx(id as u16);
        }
    }

    /// The native window's Baka Fighter HUD lines.
    pub(crate) fn baka_status_draws(&self, font: &legaia_font::Font) -> Vec<TextDraw> {
        let Some(f) = self.baka_session() else {
            return Vec::new();
        };
        let l1 = format!(
            "BAKA  you {}hp (wins {})  vs  foe {}hp (wins {})  round {}",
            f.hp(0),
            f.round_wins(0),
            f.hp(1),
            f.round_wins(1),
            f.round() + 1
        );
        let status = match f.phase() {
            MatchPhase::MatchOver(0) => format!(
                "YOU WIN the match! +{} coins  (Cross = leave)",
                f.gold_reward()
            ),
            MatchPhase::MatchOver(_) => "you lose the match  (Cross = leave)".to_string(),
            MatchPhase::RoundOver(0) => "round won!".to_string(),
            MatchPhase::RoundOver(_) => "round lost".to_string(),
            MatchPhase::Fighting => match f.last_exchange() {
                Some(r) => {
                    let who = if r.draw {
                        "trade"
                    } else if r.winner == 0 {
                        "you hit"
                    } else {
                        "foe hits"
                    };
                    let crit = if r.critical { " CRIT" } else { "" };
                    let sp = if r.special_round_win { " SPECIAL" } else { "" };
                    format!("{who} {}{crit}{sp}", r.damage)
                }
                None => "choose your attack".to_string(),
            },
        };
        let l2 = format!("{status}   Left/Right/Up attack, Down special (Start = quit)");
        let mut out = row(font, &l1, PEN_STATUS, WHITE);
        out.extend(row(font, &l2, PEN_PROMPT, DIM));
        // The duel's three retail number drawers - the round digit, the
        // right-aligned score field and the `0x10` px "GET COIN" strip - at
        // the ported cell layout the native window draws. This page printed
        // `tally N   coins N` as one prose line instead, so the two hosts
        // showed the same numbers in different places and at different
        // strides. Layout from `baka_fighter_chrome::hud_digit_placements`,
        // quads from `ui_baka_strips`, both shared.
        let placed = legaia_engine_core::baka_fighter_chrome::hud_digit_placements(
            f.round() as i32,
            f.tally().map(|t| (t.total(), t.gold_remaining())),
        );
        out.extend(
            legaia_engine_ui::ui_baka_strips::baka_digit_strip_draws_for(font, &placed, DIM),
        );
        out
    }
}

// ------------------------------------------------------------------- Dance

impl LegaiaRuntime {
    fn dance_session(&self) -> Option<&DanceGame> {
        self.scene_host.as_ref()?.world.minigames.dance.as_ref()
    }

    /// The native window's dance HUD lines: score / gauge / lane, the arrow
    /// the beat calls for, the last judgement, and the scrolling beat track
    /// at the ported note positions.
    pub(crate) fn dance_status_draws(&self, font: &legaia_font::Font) -> Vec<TextDraw> {
        use legaia_engine_core::dance::{
            GAUGE_STEP, dance_beat_track_note_x, dance_combo_window_bright, dance_number_digits,
        };
        let Some(g) = self.dance_session() else {
            return Vec::new();
        };
        let Some(host) = self.scene_host.as_ref() else {
            return Vec::new();
        };
        let arrow = match g.required_symbol() {
            Some(1) => "< (Square)",
            Some(2) => "> (Circle)",
            Some(3) => "^ (Triangle)",
            _ => "- (rest)",
        };
        let judge = match host.world.minigames.dance_last_judge {
            Some(Judge::Sequence { .. }) => "SEQUENCE!",
            Some(Judge::Hit { .. }) => "HIT",
            Some(Judge::Miss) => "miss",
            None => "",
        };
        let score_digits: String = dance_number_digits(g.score())
            .iter()
            .map(|d| match d {
                Some(v) => char::from(b'0' + v),
                None => ' ',
            })
            .collect();
        let l1 = format!(
            "DANCE  score {}  gauge {}  lane {}",
            score_digits.trim_start(),
            g.gauge(),
            g.lane()
        );
        let l2 = format!("press {arrow}   {judge}   (Start = quit)");
        let mut out = row(font, &l1, PEN_STATUS, WHITE);
        out.extend(row(font, &l2, PEN_PROMPT, DIM));
        let beat = g.beat_index();
        let frac = g.intra_beat_phase();
        let level = g.gauge() / GAUGE_STEP;
        let bright = dance_combo_window_bright(beat, level, frac);
        out.extend(row(
            font,
            if bright { "COMBO" } else { "beat " },
            PEN_EXTRA,
            if bright { WHITE } else { DIM },
        ));
        const TRACK_BASE_X: i32 = 60;
        if let Some(chart_row) = g.chart_row(g.lane()) {
            for i in 0..8u32 {
                let cell = chart_row[((beat + i) % chart_row.len() as u32) as usize];
                let glyph = match cell {
                    1 => "<",
                    2 => ">",
                    3 => "^",
                    _ => ".",
                };
                let x = dance_beat_track_note_x(TRACK_BASE_X, i, frac);
                out.extend(row(
                    font,
                    glyph,
                    (x, PEN_EXTRA.1),
                    if i == 0 && !g.in_dead_zone() {
                        WHITE
                    } else {
                        DIM
                    },
                ));
            }
        }
        // The retail-coordinate HUD frame - the three dancers' score
        // readouts, their box brackets, the Lv gauges and the rivals' beat
        // tracks - laid out by the engine
        // (`DanceGame::hud_frame_rows`, the presentation half of
        // `FUN_801d231c`). This host drew none of it: the frame's whole
        // resolution lived inside the native window's dance block, so the
        // browser showed the two plain status lines above and nothing else,
        // for a run driven by the same `DanceGame`.
        //
        // These rows are in retail's 320x240 stage coordinates and the two
        // lines above are in this page's own pen space; both go through the
        // caller's single `scale_stage_text_draws`, which is the transform
        // the native window also applies to this block.
        for r in g.hud_frame_rows(g.rival_hud_visible()) {
            out.extend(row(
                font,
                &r.text,
                (r.x, r.y),
                if r.dim { DIM } else { WHITE },
            ));
        }
        out
    }
}

/// Quad -> the page's blit record (the standalone page's
/// `muscle_hub_quads_json` row shape, so one JS blitter serves both).
fn hub_quad_json(q: &HudQuad) -> serde_json::Value {
    serde_json::json!({
        "sheet": if q.tpage & 0x10 != 0 { 5 } else { 4 },
        "pal": q.clut & 0x3F,
        "u": q.uv[0].0, "v": q.uv[0].1,
        "w": q.uv[1].0 as i32 - q.uv[0].0 as i32 + 1,
        "h": q.uv[2].1 as i32 - q.uv[0].1 as i32 + 1,
        "x": q.xy[0].0, "y": q.xy[0].1,
        "dw": q.xy[1].0 as i32 - q.xy[0].0 as i32 + 1,
        "dh": q.xy[2].1 as i32 - q.xy[0].1 as i32 + 1,
        "semi": q.semi_transparent,
    })
}

#[wasm_bindgen]
impl LegaiaRuntime {
    // ------------------------------------------------------ Muscle Dome

    /// The live dome leg's state for the page's clip triggers:
    ///
    /// ```json
    /// { "live": true, "phase": "select", "turn": 2, "hp": [400, 310],
    ///   "budget": 120, "spent": 60, "queued": 2, "last_damage": [0, 90],
    ///   "plays": [ { "attacker": 0, "cmd": 12, "damage": 90 } ],
    ///   "turns_resolved": 1, "time_meter": 3, "magic_open": false }
    /// ```
    pub fn play_mg_muscle_state_json(&self) -> String {
        let Some(s) = self.muscle_session() else {
            return r#"{"live":false}"#.to_string();
        };
        let phase = match s.phase() {
            MusclePhase::Select => "select",
            MusclePhase::Resolve => "resolve",
            MusclePhase::TurnOver => "turn_over",
            MusclePhase::Won => "won",
            MusclePhase::Lost => "lost",
        };
        let plays: Vec<serde_json::Value> = s
            .last_turn_plays()
            .iter()
            .map(|p| {
                serde_json::json!({
                    "attacker": p.attacker,
                    "cmd": p.cmd,
                    "damage": p.damage,
                })
            })
            .collect();
        serde_json::json!({
            "live": true,
            "phase": phase,
            "turn": s.turn(),
            "hp": [s.hp(0), s.hp(1)],
            "budget": s.budget(0),
            "spent": s.spent(0),
            "queued": s.queue(0).len(),
            "last_damage": s.last_turn_damage(),
            "plays": plays,
            "turns_resolved": self.minigame_ui.muscle.turns_resolved,
            "time_meter": s.time_meter(),
            "magic_open": s.magic_open(),
        })
        .to_string()
    }

    /// This frame's hub-screen quads (intro card / ROUND banner inside the
    /// dome, INTERVAL + score tally after a leg), `{ ok, quads: [...] }` in
    /// the standalone page's row shape. `quads` is empty when no screen is
    /// up.
    pub fn play_mg_muscle_hub_quads_json(&self) -> String {
        let quads = self.muscle_hub_quads();
        serde_json::json!({
            "ok": self.minigame_ui.muscle.sprite_table.is_some(),
            "quads": quads.iter().map(hub_quad_json).collect::<Vec<_>>(),
        })
        .to_string()
    }

    /// One dome hub page (`4` = VRAM (320,0), `5` = (320,256)) through
    /// 16-colour sub-palette `palette`, RGBA8. Empty when absent.
    pub fn play_mg_muscle_hub_sheet_rgba(&self, sheet: u32, palette: u32) -> Vec<u8> {
        self.minigame_art()
            .map(|a| a.muscle_hud_sheet_rgba(sheet, palette))
            .unwrap_or_default()
    }

    /// `[width, height]` of hub page `4` / `5`; empty when absent.
    pub fn play_mg_muscle_hub_sheet_dims(&self, sheet: u32) -> Vec<u32> {
        let Some((t0, t1)) = self.muscle_hub_tims() else {
            return Vec::new();
        };
        let t = if sheet == 5 { t1 } else { t0 };
        vec![t.pixel_width() as u32, t.pixel_height() as u32]
    }

    /// Whether the dome scene decodes for `(monster_id, char_slot)`.
    pub fn play_mg_muscle_scene_ready(&self, monster_id: u16, char_slot: u32) -> bool {
        self.minigame_art()
            .is_some_and(|a| a.muscle_scene_ready(monster_id, char_slot))
    }

    pub fn play_mg_muscle_fighter_positions(&self, char_slot: u32) -> Vec<f32> {
        self.minigame_art()
            .map(|a| a.muscle_fighter_positions(char_slot))
            .unwrap_or_default()
    }

    pub fn play_mg_muscle_fighter_uvs(&self, char_slot: u32) -> Vec<i32> {
        self.minigame_art()
            .map(|a| a.muscle_fighter_uvs(char_slot))
            .unwrap_or_default()
    }

    pub fn play_mg_muscle_fighter_cba_tsb(&self, char_slot: u32) -> Vec<u32> {
        self.minigame_art()
            .map(|a| a.muscle_fighter_cba_tsb(char_slot))
            .unwrap_or_default()
    }

    pub fn play_mg_muscle_fighter_indices(&self, char_slot: u32) -> Vec<u32> {
        self.minigame_art()
            .map(|a| a.muscle_fighter_indices(char_slot))
            .unwrap_or_default()
    }

    pub fn play_mg_muscle_fighter_object_ids(&self, char_slot: u32) -> Vec<u32> {
        self.minigame_art()
            .map(|a| a.muscle_fighter_object_ids(char_slot))
            .unwrap_or_default()
    }

    pub fn play_mg_muscle_fighter_flat_rgba(&self, char_slot: u32) -> Vec<u8> {
        self.minigame_art()
            .map(|a| a.muscle_fighter_flat_rgba(char_slot))
            .unwrap_or_default()
    }

    pub fn play_mg_muscle_fighter_part_count(&self, char_slot: u32) -> u32 {
        self.minigame_art()
            .map(|a| a.muscle_fighter_part_count(char_slot))
            .unwrap_or(0)
    }

    pub fn play_mg_muscle_fighter_anims_json(&self, char_slot: u32) -> String {
        self.minigame_art()
            .map(|a| a.muscle_fighter_anims_json(char_slot))
            .unwrap_or_else(|| "[]".to_string())
    }

    pub fn play_mg_muscle_fighter_pose_frames(
        &self,
        char_slot: u32,
        slot: u32,
        target_part_count: u32,
    ) -> Vec<i32> {
        self.minigame_art()
            .map(|a| a.muscle_fighter_pose_frames(char_slot, slot, target_part_count))
            .unwrap_or_default()
    }

    pub fn play_mg_muscle_monster_positions(&self, monster_id: u16) -> Vec<f32> {
        self.minigame_art()
            .map(|a| a.muscle_monster_positions(monster_id))
            .unwrap_or_default()
    }

    pub fn play_mg_muscle_monster_uvs(&self, monster_id: u16) -> Vec<i32> {
        self.minigame_art()
            .map(|a| a.muscle_monster_uvs(monster_id))
            .unwrap_or_default()
    }

    pub fn play_mg_muscle_monster_cba_tsb(&self, monster_id: u16) -> Vec<u32> {
        self.minigame_art()
            .map(|a| a.muscle_monster_cba_tsb(monster_id))
            .unwrap_or_default()
    }

    pub fn play_mg_muscle_monster_indices(&self, monster_id: u16) -> Vec<u32> {
        self.minigame_art()
            .map(|a| a.muscle_monster_indices(monster_id))
            .unwrap_or_default()
    }

    pub fn play_mg_muscle_monster_object_ids(&self, monster_id: u16) -> Vec<u32> {
        self.minigame_art()
            .map(|a| a.muscle_monster_object_ids(monster_id))
            .unwrap_or_default()
    }

    pub fn play_mg_muscle_monster_flat_rgba(&self, monster_id: u16) -> Vec<u8> {
        self.minigame_art()
            .map(|a| a.muscle_monster_flat_rgba(monster_id))
            .unwrap_or_default()
    }

    pub fn play_mg_muscle_monster_part_count(&self, monster_id: u16) -> u32 {
        self.minigame_art()
            .map(|a| a.muscle_monster_part_count(monster_id))
            .unwrap_or(0)
    }

    pub fn play_mg_muscle_monster_anims_json(&self, monster_id: u16) -> String {
        self.minigame_art()
            .map(|a| a.muscle_monster_anims_json(monster_id))
            .unwrap_or_else(|| "[]".to_string())
    }

    pub fn play_mg_muscle_monster_pose_frames(
        &self,
        monster_id: u16,
        index: u32,
        target_part_count: u32,
    ) -> Vec<i32> {
        self.minigame_art()
            .map(|a| a.muscle_monster_pose_frames(monster_id, index, target_part_count))
            .unwrap_or_default()
    }

    /// The dome's merged VRAM (character band-0 pool + palette, the
    /// monster's pool, the arena pages).
    pub fn play_mg_muscle_vram(&self, monster_id: u16, char_slot: u32) -> Vec<u8> {
        self.minigame_art()
            .map(|a| a.muscle_vram(monster_id, char_slot))
            .unwrap_or_default()
    }

    pub fn play_mg_muscle_arena_positions(&self) -> Vec<f32> {
        self.minigame_art()
            .map(|a| a.muscle_arena_positions())
            .unwrap_or_default()
    }

    pub fn play_mg_muscle_arena_uvs(&self) -> Vec<i32> {
        self.minigame_art()
            .map(|a| a.muscle_arena_uvs())
            .unwrap_or_default()
    }

    pub fn play_mg_muscle_arena_cba_tsb(&self) -> Vec<u32> {
        self.minigame_art()
            .map(|a| a.muscle_arena_cba_tsb())
            .unwrap_or_default()
    }

    pub fn play_mg_muscle_arena_indices(&self) -> Vec<u32> {
        self.minigame_art()
            .map(|a| a.muscle_arena_indices())
            .unwrap_or_default()
    }

    pub fn play_mg_muscle_arena_flat_rgba(&self) -> Vec<u8> {
        self.minigame_art()
            .map(|a| a.muscle_arena_flat_rgba())
            .unwrap_or_default()
    }

    // ----------------------------------------------------- Baka Fighter

    /// The live duel's state in the standalone page's `baka_state_json`
    /// shape, read off the world's session.
    pub fn play_mg_baka_state_json(&self) -> String {
        let Some(f) = self.baka_session() else {
            return r#"{"live":false}"#.to_string();
        };
        let phase = match f.phase() {
            MatchPhase::Fighting => "fighting",
            MatchPhase::RoundOver(_) => "round_over",
            MatchPhase::MatchOver(_) => "match_over",
        };
        let chosen = |s: usize| f.chosen(s).map(|a| a.type_id());
        let last = f.last_exchange().map(|e| {
            serde_json::json!({
                "winner": e.winner,
                "draw": e.draw,
                "damage": e.damage,
                "critical": e.critical,
                "special": e.special_round_win,
            })
        });
        serde_json::json!({
            "live": true,
            "phase": phase,
            "round": f.round(),
            "hp": [f.hp(0), f.hp(1)],
            "hp_start": legaia_engine_core::baka_fighter::HP_START,
            "wins": [f.round_wins(0), f.round_wins(1)],
            "combo": [f.combo(0), f.combo(1)],
            "chosen": [chosen(0), chosen(1)],
            "can_choose": f.can_choose(0),
            "gold": f.gold_reward(),
            "winner": f.winner(),
            "last": last,
        })
        .to_string()
    }

    /// Whether the Baka roster / art / stage packs decoded.
    pub fn play_mg_baka_presentation_ready(&self) -> bool {
        self.minigame_art()
            .is_some_and(|a| a.baka_presentation_ready())
    }

    pub fn play_mg_baka_fighter_positions(&self, side: u32, id: u32) -> Vec<f32> {
        self.minigame_art()
            .map(|a| a.baka_fighter_positions(side, id))
            .unwrap_or_default()
    }

    pub fn play_mg_baka_fighter_uvs(&self, side: u32, id: u32) -> Vec<i32> {
        self.minigame_art()
            .map(|a| a.baka_fighter_uvs(side, id))
            .unwrap_or_default()
    }

    pub fn play_mg_baka_fighter_cba_tsb(&self, side: u32, id: u32) -> Vec<u32> {
        self.minigame_art()
            .map(|a| a.baka_fighter_cba_tsb(side, id))
            .unwrap_or_default()
    }

    pub fn play_mg_baka_fighter_indices(&self, side: u32, id: u32) -> Vec<u32> {
        self.minigame_art()
            .map(|a| a.baka_fighter_indices(side, id))
            .unwrap_or_default()
    }

    pub fn play_mg_baka_fighter_object_ids(&self, side: u32, id: u32) -> Vec<u32> {
        self.minigame_art()
            .map(|a| a.baka_fighter_object_ids(side, id))
            .unwrap_or_default()
    }

    pub fn play_mg_baka_fighter_flat_rgba(&self, side: u32, id: u32) -> Vec<u8> {
        self.minigame_art()
            .map(|a| a.baka_fighter_flat_rgba(side, id))
            .unwrap_or_default()
    }

    pub fn play_mg_baka_fighter_part_count(&self, side: u32, id: u32) -> u32 {
        self.minigame_art()
            .map(|a| a.baka_fighter_part_count(side, id))
            .unwrap_or(0)
    }

    pub fn play_mg_baka_anim_dims(&self, side: u32, id: u32, action: u32) -> Vec<u32> {
        self.minigame_art()
            .map(|a| a.baka_anim_dims(side, id, action))
            .unwrap_or_default()
    }

    pub fn play_mg_baka_anim_pose_frames(
        &self,
        side: u32,
        id: u32,
        action: u32,
        target_part_count: u32,
    ) -> Vec<i32> {
        self.minigame_art()
            .map(|a| a.baka_anim_pose_frames(side, id, action, target_part_count))
            .unwrap_or_default()
    }

    pub fn play_mg_baka_stage_positions(&self, index: usize) -> Vec<f32> {
        self.minigame_art()
            .map(|a| a.baka_stage_positions(index))
            .unwrap_or_default()
    }

    pub fn play_mg_baka_stage_uvs(&self, index: usize) -> Vec<i32> {
        self.minigame_art()
            .map(|a| a.baka_stage_uvs(index))
            .unwrap_or_default()
    }

    pub fn play_mg_baka_stage_cba_tsb(&self, index: usize) -> Vec<u32> {
        self.minigame_art()
            .map(|a| a.baka_stage_cba_tsb(index))
            .unwrap_or_default()
    }

    pub fn play_mg_baka_stage_indices(&self, index: usize) -> Vec<u32> {
        self.minigame_art()
            .map(|a| a.baka_stage_indices(index))
            .unwrap_or_default()
    }

    pub fn play_mg_baka_stage_flat_rgba(&self, index: usize) -> Vec<u8> {
        self.minigame_art()
            .map(|a| a.baka_stage_flat_rgba(index))
            .unwrap_or_default()
    }

    /// The duel VRAM for roster `opponent`.
    pub fn play_mg_baka_duel_vram(&self, opponent: u32) -> Vec<u8> {
        self.minigame_art()
            .map(|a| a.baka_duel_vram(opponent))
            .unwrap_or_default()
    }

    /// Which side each fighter stands on and faces
    /// (`LegaiaMinigames::baka_duel_facing_json`).
    pub fn play_mg_baka_duel_facing_json(&self) -> String {
        self.minigame_art()
            .map(|a| a.baka_duel_facing_json())
            .unwrap_or_else(|| {
                r#"{"player":{"side":-1,"facing":1},"opponent":{"side":1,"facing":-1}}"#.to_string()
            })
    }

    // ------------------------------------------------------------ Dance

    /// The live dance run in the standalone page's `dance_state_json` shape
    /// (the fields its body renderer reads), off the world's session.
    pub fn play_mg_dance_state_json(&self) -> String {
        let Some(g) = self.dance_session() else {
            return r#"{"live":false}"#.to_string();
        };
        let rivals: Vec<serde_json::Value> = (1..g.dancer_count())
            .map(|i| {
                serde_json::json!({
                    "score": g.dancer_score(i),
                    "gauge": g.dancer_gauge(i),
                    "lane": g.dancer_lane(i),
                    "kind": g.dancer_kind(i),
                    "triangles": g.dancer_triangles(i),
                })
            })
            .collect();
        serde_json::json!({
            "live": true,
            "score": g.score(),
            "gauge": g.gauge(),
            "lane": g.lane(),
            "beat": g.beat_index(),
            "phase": g.intra_beat_phase(),
            "judged": g.judged_symbol(),
            "displayed": g.required_symbol(),
            "triangles": g.triangles(),
            "feedback": g.triangle_feedback(),
            "rivals": rivals,
            "song_timer": g.song_timer(),
            "song_len": g.song_len(),
            "over": g.song_over(),
            "passed": g.passed(),
        })
        .to_string()
    }

    /// The step chart the run plays (`{ "rows": [[u8; 32], ...] }`, one row
    /// per difficulty lane), off the world's session.
    pub fn play_mg_dance_chart_json(&self) -> String {
        let Some(g) = self.dance_session() else {
            return r#"{"rows":[]}"#.to_string();
        };
        let rows: Vec<Vec<u8>> = (0..8usize)
            .map_while(|lane| g.chart_row(lane).map(|r| r.to_vec()))
            .collect();
        serde_json::json!({ "rows": rows }).to_string()
    }

    pub fn play_mg_dance_body_ready(&self) -> bool {
        self.minigame_art().is_some_and(|a| a.dance_body_ready())
    }

    pub fn play_mg_dance_body_count(&self) -> u32 {
        self.minigame_art()
            .map(|a| a.dance_body_count())
            .unwrap_or(0)
    }

    pub fn play_mg_dance_body_human_index(&self) -> u32 {
        self.minigame_art()
            .map(|a| a.dance_body_human_index())
            .unwrap_or(0)
    }

    pub fn play_mg_dance_cast_json(&self) -> String {
        self.minigame_art()
            .map(|a| a.dance_cast_json())
            .unwrap_or_else(|| "null".to_string())
    }

    pub fn play_mg_dance_body_positions(&self, dancer: u32) -> Vec<f32> {
        self.minigame_art()
            .map(|a| a.dance_body_positions(dancer))
            .unwrap_or_default()
    }

    pub fn play_mg_dance_body_uvs(&self, dancer: u32) -> Vec<i32> {
        self.minigame_art()
            .map(|a| a.dance_body_uvs(dancer))
            .unwrap_or_default()
    }

    pub fn play_mg_dance_body_cba_tsb(&self, dancer: u32) -> Vec<u32> {
        self.minigame_art()
            .map(|a| a.dance_body_cba_tsb(dancer))
            .unwrap_or_default()
    }

    pub fn play_mg_dance_body_indices(&self, dancer: u32) -> Vec<u32> {
        self.minigame_art()
            .map(|a| a.dance_body_indices(dancer))
            .unwrap_or_default()
    }

    pub fn play_mg_dance_body_object_ids(&self, dancer: u32) -> Vec<u32> {
        self.minigame_art()
            .map(|a| a.dance_body_object_ids(dancer))
            .unwrap_or_default()
    }

    pub fn play_mg_dance_body_flat_rgba(&self, dancer: u32) -> Vec<u8> {
        self.minigame_art()
            .map(|a| a.dance_body_flat_rgba(dancer))
            .unwrap_or_default()
    }

    pub fn play_mg_dance_body_part_count(&self, dancer: u32) -> u32 {
        self.minigame_art()
            .map(|a| a.dance_body_part_count(dancer))
            .unwrap_or(0)
    }

    pub fn play_mg_dance_body_anim_dims(&self, dancer: u32, clip: u32) -> Vec<u32> {
        self.minigame_art()
            .map(|a| a.dance_body_anim_dims(dancer, clip))
            .unwrap_or_default()
    }

    pub fn play_mg_dance_body_pose_frames(
        &self,
        dancer: u32,
        clip: u32,
        target_part_count: u32,
    ) -> Vec<i32> {
        self.minigame_art()
            .map(|a| a.dance_body_pose_frames(dancer, clip, target_part_count))
            .unwrap_or_default()
    }

    pub fn play_mg_dance_body_vram(&self) -> Vec<u8> {
        self.minigame_art()
            .map(|a| a.dance_body_vram())
            .unwrap_or_default()
    }

    pub fn play_mg_dance_env_positions(&self) -> Vec<f32> {
        self.minigame_art()
            .map(|a| a.dance_env_positions())
            .unwrap_or_default()
    }

    pub fn play_mg_dance_env_uvs(&self) -> Vec<i32> {
        self.minigame_art()
            .map(|a| a.dance_env_uvs())
            .unwrap_or_default()
    }

    pub fn play_mg_dance_env_cba_tsb(&self) -> Vec<u32> {
        self.minigame_art()
            .map(|a| a.dance_env_cba_tsb())
            .unwrap_or_default()
    }

    pub fn play_mg_dance_env_indices(&self) -> Vec<u32> {
        self.minigame_art()
            .map(|a| a.dance_env_indices())
            .unwrap_or_default()
    }

    pub fn play_mg_dance_env_flat_rgba(&self) -> Vec<u8> {
        self.minigame_art()
            .map(|a| a.dance_env_flat_rgba())
            .unwrap_or_default()
    }

    pub fn play_mg_dance_marker_tiles(&self) -> u32 {
        self.minigame_art()
            .map(|a| a.dance_marker_tiles())
            .unwrap_or(0)
    }

    pub fn play_mg_dance_marker_uvs(&self) -> Vec<i32> {
        self.minigame_art()
            .map(|a| a.dance_marker_uvs())
            .unwrap_or_default()
    }

    pub fn play_mg_dance_marker_cba_tsb(&self) -> Vec<u32> {
        self.minigame_art()
            .map(|a| a.dance_marker_cba_tsb())
            .unwrap_or_default()
    }

    pub fn play_mg_dance_marker_indices(&self) -> Vec<u32> {
        self.minigame_art()
            .map(|a| a.dance_marker_indices())
            .unwrap_or_default()
    }

    pub fn play_mg_dance_marker_flat_rgba(&self) -> Vec<u8> {
        self.minigame_art()
            .map(|a| a.dance_marker_flat_rgba())
            .unwrap_or_default()
    }

    /// The marker flipbook's positions without advancing it (the initial
    /// upload).
    pub fn play_mg_dance_marker_positions(&mut self) -> Vec<f32> {
        self.minigame_ui
            .art
            .as_mut()
            .map(|a| a.dance_marker_positions())
            .unwrap_or_default()
    }

    /// Advance the marker flipbook `frame_delta` retail frames and return
    /// the block's positions.
    pub fn play_mg_dance_marker_step(&mut self, frame_delta: u8) -> Vec<f32> {
        self.minigame_ui
            .art
            .as_mut()
            .map(|a| a.dance_marker_step(frame_delta))
            .unwrap_or_default()
    }
}
