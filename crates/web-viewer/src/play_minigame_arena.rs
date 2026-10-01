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
//! - **Baka Fighter** (`World::tick_baka_fighter`): Square / Circle / Cross
//!   commit attack types 1 / 2 / 3 (retail's face-button read), Triangle the
//!   port's chargeable special; the result screen's tally
//!   banks into the mode-24 winnings the return warp pays out. The opponent
//!   the scene host rotated in is reproduced here (the same frame-keyed pick,
//!   cross-checked against the fight's prize) so the page draws the fighter
//!   the rules are running.
//! - **Dance** (`World::tick_dance`): Square / Circle / Triangle are the
//!   judged buttons; the song ends the run on its own.
//!
//! Start leaves any of the three (`World::poll_minigame_escape`).

use legaia_engine_core::dance::DanceGame;
use legaia_engine_core::muscle_dome::{self as md, DomeContest, MuscleDomeSession, MusclePhase};
use legaia_engine_ui::TextDraw;
use legaia_engine_ui::other_game_hud::{self as hud, HudQuad, HudSprite};
use wasm_bindgen::prelude::*;

use crate::minigames::duel_surface;
use crate::play_minigames::{DIM, WHITE, row};
use crate::runtime::LegaiaRuntime;

/// PROT entry of the arena roster / init overlay (course ladder, score
/// table, hub sprite table).
const ARENA_OVERLAY_PROT_INDEX: u32 = md::ARENA_OVERLAY_PROT_INDEX as u32;
/// PROT entry of the dome data container whose LZS section 0 carries the
/// two hub-page TIMs (`other6.lzs` slot 0).
const HUB_CONTAINER_PROT_INDEX: u32 = 1220;
/// The page's sheet id for the ringside still (the hub pages are `4` / `5`);
/// its `pal` is the still variant, `0` = extraction 1221, `1` = 1222.
const STILL_SHEET: u32 = 8;

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
    /// The hub's screen timers - the engine kernel the native window drives
    /// too (`legaia_engine_core::muscle_ringside::HubTimers`).
    timers: legaia_engine_core::muscle_ringside::HubTimers,
    /// Pristine parse of the PROT 0977 sprite table; the emitters write
    /// variants back, so every frame runs over a copy.
    sprite_table: Option<Vec<HudSprite>>,
    /// The hub pages' palette STP classes (whether a semi packet blends).
    palette_stp: legaia_engine_ui::ringside_backdrop::HubPaletteStp,
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
        if let Some((t0, t1)) = self.muscle_hub_tims() {
            self.minigame_ui.muscle.palette_stp =
                legaia_engine_ui::ringside_backdrop::HubPaletteStp::from_tims(&t0, &t1);
        }
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

    /// The hub-screen timers, off the world's leg / contest edges, through
    /// the one engine kernel the native window's `tick_muscle_hub` runs too
    /// (`muscle_ringside::HubTimers`). Runs every frame: the INTERVAL +
    /// tally screen plays after the leg has closed.
    pub(crate) fn tick_muscle_hub(&mut self) {
        let Some(host) = self.scene_host.as_ref() else {
            return;
        };
        let pad = host.world.input.retail_pad().pressed as u16;
        // `_DAT_80084580` off the world - a loaded save's word or the cold
        // reset - as the native window reads it.
        let volume_word = host.world.audio.levels.voice_volume as u32;
        let frame = self
            .minigame_ui
            .muscle
            .timers
            .tick(&host.world, pad, volume_word);
        let hub_xa = frame.xa;
        let voice_cues = frame.voice_cues;
        if let Some(c) = hub_xa {
            self.play_xa_clip(
                u32::from(c.clip),
                u32::from(c.channel),
                u32::from(c.duration_sectors),
            );
        }
        for cue in voice_cues {
            self.key_on_voice_attr(legaia_engine_audio::VoiceAttr::from_cue_words(
                cue.voice,
                cue.vab_program_tone,
                cue.note_and_fine,
                cue.volume,
            ));
        }
    }

    /// This frame's hub-screen rows, the native `muscle_hub_sprite_draws`
    /// selection over the shared emitters: blit rows for the quads, plus a
    /// `shade` row for the first visit's backdrop shade between the wall
    /// tiles and the screens drawn over them.
    fn muscle_hub_rows(&self) -> Vec<serde_json::Value> {
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
        let mut shade_row: Option<(usize, serde_json::Value)> = None;
        if in_dome {
            // A first visit's frame: wall + shade behind the arm's screens,
            // through the shared kernel the native window draws with.
            if let Some(hub) = ui.timers.first_visit {
                use legaia_engine_ui::ringside_backdrop as rb;
                let f = hub.frame();
                let levels = rb::FirstVisitLevels {
                    backdrop: f.backdrop,
                    intro: f.intro,
                    title_scale: f.title_scale,
                    course_card: f.course_card,
                    round_card: f.round_card,
                };
                let (course, round) = world
                    .minigames
                    .muscle_contest
                    .as_ref()
                    .map_or((0, 1), |c| (c.course() as i32, c.round() as i32 + 1));
                let d = rb::first_visit_hub_draw(&mut table, &levels, course, round);
                quads.extend(d.tiles);
                if let Some(sh) = d.shade {
                    shade_row = Some((quads.len(), shade_json(&sh)));
                }
                quads.extend(d.hud);
            } else if let Some((round, banner)) = ui.timers.round_banner {
                quads.extend(hud::hub_screen_quads(
                    &mut table,
                    &hud::round_banner_draws(round),
                    banner.brightness(),
                ));
            }
        } else if let Some(interval) = ui.timers.interval {
            let bright = interval.brightness();
            quads.extend(hud::hub_screen_quads(
                &mut table,
                hud::HUB_INTERVAL_HEADING,
                bright,
            ));
            let (values, row_bright) = match ui.timers.tally.as_ref() {
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
        // The re-entered hub's ROUND card (arms 0x15 / 0x16) over the still.
        if !in_dome
            && ui.timers.interval.is_none()
            && let Some(card) = ui.timers.backdrop.and_then(|b| b.card_brightness())
        {
            let round = world
                .minigames
                .muscle_contest
                .as_ref()
                .map_or(1, |c| c.round() as i32 + 1);
            quads.extend(hud::hub_screen_quads(
                &mut table,
                &hud::round_banner_draws(round),
                card,
            ));
        }
        let mut rows: Vec<serde_json::Value> = quads
            .iter()
            .map(|q| hub_quad_json(q, &ui.palette_stp))
            .collect();
        if let Some((at, row)) = shade_row {
            rows.insert(at.min(rows.len()), row);
        }
        rows
    }

    /// The re-entered hub's backdrop as blit rows, drawn under
    /// [`Self::muscle_hub_rows`]: the two still quads, resolved through
    /// their texture pages onto the still sheet (`sheet` [`STILL_SHEET`],
    /// `pal` = the still variant). Empty unless a still is up.
    fn muscle_still_rows(&self) -> Vec<serde_json::Value> {
        use legaia_engine_ui::ringside_backdrop as rb;
        let Some(host) = self.scene_host.as_ref() else {
            return Vec::new();
        };
        if host.world.mode == legaia_engine_core::world::SceneMode::MuscleDome {
            return Vec::new();
        }
        let Some(b) = self
            .minigame_ui
            .muscle
            .timers
            .backdrop
            .filter(|b| b.visible())
        else {
            return Vec::new();
        };
        rb::ringside_still_quads(b.level())
            .iter()
            .filter_map(|q| {
                let d = rb::StillDraw::from_quad(q)?;
                Some(serde_json::json!({
                    "sheet": STILL_SHEET,
                    "pal": b.variant(),
                    "tpage": q.tpage,
                    "u": d.src.0, "v": d.src.1, "w": d.src.2, "h": d.src.3,
                    "x": d.dst.0, "y": d.dst.1, "dw": d.dst.2, "dh": d.dst.3,
                    "bright": d.level,
                    "semi": false,
                }))
            })
            .collect()
    }

    /// The Muscle Dome HUD rows, the engine's
    /// (`minigame_status::muscle_status_rows`) through the shared draw kernel
    /// the native window calls.
    pub(crate) fn muscle_status_draws(&self, font: &legaia_font::Font) -> Vec<TextDraw> {
        let Some(host) = self.scene_host.as_ref() else {
            return Vec::new();
        };
        let rows = legaia_engine_core::minigame_status::muscle_status_rows(&host.world);
        legaia_engine_ui::ui_text_lines::status_row_draws_for(
            font,
            rows.iter().map(|r| (r.text.as_str(), r.pen, r.bright)),
        )
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

    /// Entry: the opponent is whoever the fight seated - the cabinet's
    /// first rung, read straight off the rules engine rather than
    /// reconstructed from the entry frame.
    pub(crate) fn enter_baka_ui(&mut self) {
        self.minigame_ui.baka = BakaUi::default();
        if let Some(f) = self.baka_session() {
            self.minigame_ui.baka.opponent = f.opponent_roster();
        }
    }

    /// Per-tick inside the duel: drain the rules kernel's SFX cues (the
    /// exchange hit, `BAKA_CUE_HIT`) into the page's scheduler - the native
    /// window's `drain_baka_sfx_cues`.
    pub(crate) fn tick_baka_ui(&mut self) {
        // A rung the cabinet seats inside the visit needs nothing here: the
        // duel surface (`play_mg_baka_scene_frame`) rebuilds its buffers and
        // VRAM when the seated pair changes, on both play hosts.
        let (cues, xa): (Vec<u8>, _) = self
            .scene_host
            .as_mut()
            .and_then(|h| h.world.minigames.baka_fighter.as_mut())
            .map(|f| (f.take_cues(), f.chrome_frame().xa))
            .unwrap_or_default();
        for id in cues {
            self.minigame_sfx(id as u16);
        }
        // The round chrome's announcer line (`FUN_8003D53C`), the native
        // window's `tick_baka_chrome` twin.
        if let Some(xa) = xa {
            self.play_xa_clip(u32::from(xa.clip), u32::from(xa.chan), u32::from(xa.dur));
        }
    }

    /// The native window's Baka Fighter HUD lines.
    pub(crate) fn baka_status_draws(&self, font: &legaia_font::Font) -> Vec<TextDraw> {
        let Some(f) = self.baka_session() else {
            return Vec::new();
        };
        let rows = legaia_engine_core::minigame_status::baka_status_rows(f);
        let mut out = legaia_engine_ui::ui_text_lines::status_row_draws_for(
            font,
            rows.iter().map(|r| (r.text.as_str(), r.pen, r.bright)),
        );
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
            legaia_engine_ui::ui_baka_strips::baka_digit_strip_draws_for(
                font,
                &placed,
                legaia_engine_ui::ui_text_lines::STATUS_ROW_DIM_INK,
            ),
        );
        // The round chrome (`BakaChrome`: intro card, ROUND banner,
        // countdown) and the "NEXT GAME / PAY OUT" sheet, through the label
        // kernels the native window draws with.
        use legaia_engine_core::{baka_cabinet as bcab, baka_fighter_chrome as bc};
        out.extend(
            legaia_engine_ui::ui_baka_strips::baka_widget_label_draws_for(
                font,
                &bc::chrome_labels(&f.chrome_frame().draws),
                WHITE,
            ),
        );
        if let Some(cells) = f.cabinet().choice_sheet() {
            out.extend(
                legaia_engine_ui::ui_baka_strips::baka_widget_label_draws_for(
                    font,
                    &bcab::choice_sheet_labels(&cells),
                    WHITE,
                ),
            );
            let pot = self
                .scene_host
                .as_ref()
                .map(|h| h.world.minigames.winnings)
                .unwrap_or(0);
            out.extend(
                legaia_engine_ui::ui_baka_strips::baka_digit_strip_draws_for(
                    font,
                    &bcab::choice_pot_placements(pot),
                    WHITE,
                ),
            );
        }
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
        let Some(g) = self.dance_session() else {
            return Vec::new();
        };
        let Some(host) = self.scene_host.as_ref() else {
            return Vec::new();
        };
        // Score / gauge / lane, the called arrow with the last judgement and
        // the beat track: the engine's rows
        // (`minigame_status::dance_status_rows`), the ones the native window
        // draws.
        let rows = legaia_engine_core::minigame_status::dance_status_rows(
            g,
            host.world.minigames.dance_last_judge.as_ref(),
        );
        let mut out = legaia_engine_ui::ui_text_lines::status_row_draws_for(
            font,
            rows.iter().map(|r| (r.text.as_str(), r.pen, r.bright)),
        );
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
        // With the hall's HUD page resident the frame draws as retail's own
        // quads in the prim pass (`dance_hud_prims`); these rows are the
        // fallback without it - the same either/or the native window takes.
        let frame_rows = if host.world.minigames.dance_hud_art_staged {
            Vec::new()
        } else {
            g.hud_frame_rows(g.rival_hud_visible())
        };
        for r in frame_rows {
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
///
/// `abr` is the equation the quad's texels blend with, `null` for a quad
/// that draws opaque (`HubPaletteStp::quad_abr`).
fn hub_quad_json(
    q: &HudQuad,
    stp: &legaia_engine_ui::ringside_backdrop::HubPaletteStp,
) -> serde_json::Value {
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
        "abr": stp.quad_abr(q),
    })
}

/// The first visit's backdrop shade (`FUN_801D1610`) as a page row:
/// `{ shade: true, x, y, dw, dh, top, bottom }` - a vertical ramp the page
/// applies subtractively (`B - F`, clamped at 0) to the pixels already on its
/// 2D layer (`subtractShade` in `play-minigames.js`), which is retail's
/// ABR 2 - the equation the native window's sprite pass runs in hardware.
fn shade_json(sh: &legaia_engine_ui::ringside_backdrop::BackdropShade) -> serde_json::Value {
    serde_json::json!({
        "shade": true,
        "x": sh.xy[0].0, "y": sh.xy[0].1,
        "dw": sh.xy[1].0 as i32 - sh.xy[0].0 as i32,
        "dh": sh.xy[2].1 as i32 - sh.xy[0].1 as i32,
        "top": sh.rgb[0][0],
        "bottom": sh.rgb[2][0],
    })
}

#[wasm_bindgen]
impl LegaiaRuntime {
    // ------------------------------------------------------ Muscle Dome

    /// The live dome leg's state for the page's clip triggers:
    ///
    /// ```json
    /// { "live": true, "phase": "select", "turn": 2, "hp": [400, 310],
    ///   "budget": 120, "costs": [30, 30, 30, 30], "spent": 60, "queued": 2,
    ///   "last_damage": [0, 90],
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
            "costs": s.hand(0).iter().map(|c| c.cost).collect::<Vec<_>>(),
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
    ///
    /// A re-entered hub's ringside still leads the list (retail links it at
    /// the ordering table's far end): two rows on sheet `8` whose `tpage` is
    /// the packet's own `0x106` / `0x109`, `pal` the still variant and
    /// `bright` the packet colour (`0x80` = neutral modulation).
    pub fn play_mg_muscle_hub_quads_json(&self) -> String {
        let mut rows = self.muscle_still_rows();
        rows.extend(self.muscle_hub_rows());
        serde_json::json!({
            "ok": self.minigame_ui.muscle.sprite_table.is_some(),
            "quads": rows,
        })
        .to_string()
    }

    /// One dome hub page (`4` = VRAM (320,0), `5` = (320,256)) through
    /// 16-colour sub-palette `palette`, RGBA8. Empty when absent.
    ///
    /// Sheet `8` is the ringside still `palette` (`0` = extraction 1221,
    /// `1` = 1222), 320x256, as the battle end's loader lays it into VRAM
    /// at `(384, 0)`.
    pub fn play_mg_muscle_hub_sheet_rgba(&self, sheet: u32, palette: u32) -> Vec<u8> {
        if sheet == STILL_SHEET {
            return self
                .scene_host
                .as_ref()
                .and_then(|h| {
                    h.index
                        .entry_bytes_extended(
                            legaia_asset::ringside_still::PROT_INDEX_DEFAULT + palette.min(1),
                        )
                        .ok()
                })
                .and_then(|b| legaia_engine_ui::ringside_backdrop::still_sheet_rgba(&b))
                .unwrap_or_default();
        }
        self.minigame_art()
            .map(|a| a.muscle_hud_sheet_rgba(sheet, palette))
            .unwrap_or_default()
    }

    /// `[width, height]` of hub page `4` / `5`; empty when absent.
    pub fn play_mg_muscle_hub_sheet_dims(&self, sheet: u32) -> Vec<u32> {
        if sheet == STILL_SHEET {
            return vec![
                legaia_asset::ringside_still::WIDTH as u32,
                legaia_asset::ringside_still::HEIGHT as u32,
            ];
        }
        let Some((t0, t1)) = self.muscle_hub_tims() else {
            return Vec::new();
        };
        let t = if sheet == 5 { t1 } else { t0 };
        vec![t.pixel_width() as u32, t.pixel_height() as u32]
    }

    /// Pose the dome's 3D arena surface for this frame
    /// (`legaia_engine_core::muscle_dome_scene::MuscleDomeSurface::frame`,
    /// the call the native window makes too) and return its generation - or
    /// `-1` when no dome session is live or its scene does not decode. A
    /// generation the page has not seen means the static buffers and the
    /// VRAM changed (a new rung seated a new monster): re-read them before
    /// the positions.
    pub fn play_mg_muscle_scene_frame(&mut self) -> i32 {
        let host = self.scene_host.as_ref();
        let world = host.map(|h| &h.world);
        let live =
            world.is_some_and(|w| w.mode == legaia_engine_core::world::SceneMode::MuscleDome);
        let session = world
            .filter(|_| live)
            .and_then(|w| w.minigames.muscle_dome.as_ref());
        let contest = world.and_then(|w| w.minigames.muscle_contest.as_ref());
        let read = |i: usize| host.and_then(|h| h.index.entry_bytes(i as u32).ok());
        let char_slot = self.minigame_ui.muscle.char_slot;
        let surface = &mut self.minigame_ui.muscle_surface;
        match surface.frame(read, session, contest, char_slot) {
            Some(_) => surface.generation() as i32,
            None => -1,
        }
    }

    /// This frame's posed positions, `[x, y, z]` per vertex, raw retail world
    /// coordinates (Y down): the fighter, the monster, then the arena.
    pub fn play_mg_muscle_scene_positions(&self) -> Vec<f32> {
        self.minigame_ui
            .muscle_surface
            .scene()
            .map(|s| s.positions.iter().flatten().copied().collect())
            .unwrap_or_default()
    }

    /// Per-vertex `[u, v]`.
    pub fn play_mg_muscle_scene_uvs(&self) -> Vec<u8> {
        self.minigame_ui
            .muscle_surface
            .scene()
            .map(|s| s.uvs.iter().flatten().copied().collect())
            .unwrap_or_default()
    }

    /// Per-vertex `[cba, tsb]`.
    pub fn play_mg_muscle_scene_cba_tsb(&self) -> Vec<u16> {
        self.minigame_ui
            .muscle_surface
            .scene()
            .map(|s| s.cba_tsb.iter().flatten().copied().collect())
            .unwrap_or_default()
    }

    /// Per-vertex `[r, g, b, textured]`.
    pub fn play_mg_muscle_scene_flat_rgba(&self) -> Vec<u8> {
        self.minigame_ui
            .muscle_surface
            .scene()
            .map(|s| s.flat_rgba.clone())
            .unwrap_or_default()
    }

    /// Triangle indices.
    pub fn play_mg_muscle_scene_indices(&self) -> Vec<u32> {
        self.minigame_ui
            .muscle_surface
            .scene()
            .map(|s| s.indices.clone())
            .unwrap_or_default()
    }

    /// The seated dome's VRAM (character pool + palette, monster pool, arena
    /// pages); empty with no scene.
    pub fn play_mg_muscle_scene_vram(&self) -> Vec<u8> {
        self.minigame_ui
            .muscle_surface
            .vram()
            .map(|v| v.as_bytes().to_vec())
            .unwrap_or_default()
    }

    /// The dome camera's view-projection for a raw (Y-down) world vertex,
    /// column-major (`DomeCamera::vp_raw`, the matrix the native window
    /// draws the dome with). Empty with no scene.
    pub fn play_mg_muscle_scene_vp(&self, aspect: f32) -> Vec<f32> {
        self.minigame_ui
            .muscle_surface
            .scene()
            .map(|s| s.camera.vp_raw(aspect).to_vec())
            .unwrap_or_default()
    }

    // ----------------------------------------------------- Baka Fighter

    /// The live duel's state, through the one builder the standalone page's
    /// `baka_state_json` uses (`crate::minigames::baka_state_json_for`) - so
    /// the play page reads the strike clock, the display clips and the
    /// afterimage passes too.
    pub fn play_mg_baka_state_json(&self) -> String {
        match self.baka_session() {
            Some(f) => crate::minigames::baka_state_json_for(f),
            None => r#"{"live":false}"#.to_string(),
        }
    }

    /// Pose the duel's 3D surface for this frame
    /// (`legaia_engine_core::baka_duel_scene::BakaDuelSurface::frame`, the
    /// call the native window makes too) and return its generation - or `-1`
    /// when no duel is live. A generation the page has not seen means the
    /// static buffers and the VRAM changed (a rung seated a new opponent):
    /// re-read them before the positions.
    pub fn play_mg_baka_scene_frame(&mut self) -> i32 {
        let host = self.scene_host.as_ref();
        let fight = host.and_then(|h| h.world.minigames.baka_fighter.as_ref());
        let read =
            |i: usize| host.and_then(|h| h.index.entry_bytes(i as u32).ok().map(|b| b.to_vec()));
        let surface = &mut self.minigame_ui.baka_surface;
        match surface.frame(read, fight) {
            Some(_) => surface.generation() as i32,
            None => -1,
        }
    }

    /// The scene's attribute generation
    /// (`BakaDuelScene::attr_generation`): it moves when a pose rewrote the
    /// UVs, CBA/TSB words or colours - the impact effect's flip-book cells
    /// and fades - so the page re-reads those without re-uploading the VRAM.
    /// `-1` with no scene.
    pub fn play_mg_baka_scene_attr_generation(&self) -> i32 {
        duel_surface::attr_generation(&self.minigame_ui.baka_surface)
    }

    /// This frame's posed positions, `[x, y, z]` per vertex, raw retail world
    /// coordinates (Y down).
    pub fn play_mg_baka_scene_positions(&self) -> Vec<f32> {
        duel_surface::positions(&self.minigame_ui.baka_surface)
    }

    /// Per-vertex `[u, v]`.
    pub fn play_mg_baka_scene_uvs(&self) -> Vec<u8> {
        duel_surface::uvs(&self.minigame_ui.baka_surface)
    }

    /// Per-vertex `[cba, tsb]`.
    pub fn play_mg_baka_scene_cba_tsb(&self) -> Vec<u16> {
        duel_surface::cba_tsb(&self.minigame_ui.baka_surface)
    }

    /// Per-vertex `[r, g, b, flag]` (the hybrid textured / fill layout).
    pub fn play_mg_baka_scene_flat_rgba(&self) -> Vec<u8> {
        duel_surface::flat_rgba(&self.minigame_ui.baka_surface)
    }

    /// Triangle indices.
    pub fn play_mg_baka_scene_indices(&self) -> Vec<u32> {
        duel_surface::indices(&self.minigame_ui.baka_surface)
    }

    /// The duel VRAM for the seated opponent.
    pub fn play_mg_baka_scene_vram(&self) -> Vec<u8> {
        duel_surface::vram(&self.minigame_ui.baka_surface)
    }

    /// The arena camera's view-projection for a raw (Y-down) world vertex,
    /// column-major (`DuelCamera::vp_raw`, the matrix the native window
    /// draws the duel with).
    pub fn play_mg_baka_scene_vp(&self, aspect: f32) -> Vec<f32> {
        self.baka_session()
            .map(|f| f.duel_camera().vp_raw(aspect).to_vec())
            .unwrap_or_default()
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

    /// Pose the dance floor's bodies for this frame through the engine's
    /// cast surface (`legaia_engine_core::dance_cast_scene::DanceCastSurface::frame`
    /// over the world's run - the call the native window makes too) and
    /// return its generation, or `-1` when no run is live. A generation the
    /// page has not seen means the static buffers changed (a run on another
    /// mode seated another cast): re-read them before the positions.
    pub fn play_mg_dance_scene_frame(&mut self) -> i32 {
        if !self.minigame_ui.dance_surface.has_assets()
            && let Some((assets, origin)) = self.minigame_art().and_then(|a| a.dance_cast_assets())
        {
            self.minigame_ui.dance_surface.set_assets(Some(assets));
            self.minigame_ui.dance_origin = origin;
        }
        let game = self
            .scene_host
            .as_ref()
            .and_then(|h| h.world.minigames.dance.as_ref());
        let surface = &mut self.minigame_ui.dance_surface;
        match surface.frame(game) {
            Some(_) => surface.generation() as i32,
            None => -1,
        }
    }

    /// This frame's posed positions, `[x, y, z]` per vertex, in the frame the
    /// page's baked hall is drawn in (raw retail world coordinates re-based
    /// on the hall origin, `LegaiaMinigames::dance_venue_vp`'s frame).
    pub fn play_mg_dance_scene_positions(&self) -> Vec<f32> {
        let (ox, oy, oz) = self.minigame_ui.dance_origin;
        self.minigame_ui
            .dance_surface
            .scene()
            .map(|s| {
                s.positions
                    .iter()
                    .flat_map(|p| [p[0] - ox, p[1] - oy, p[2] - oz])
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Per-vertex `[u, v]`.
    pub fn play_mg_dance_scene_uvs(&self) -> Vec<u8> {
        self.minigame_ui
            .dance_surface
            .scene()
            .map(|s| s.uvs.iter().flatten().copied().collect())
            .unwrap_or_default()
    }

    /// Per-vertex `[cba, tsb]`.
    pub fn play_mg_dance_scene_cba_tsb(&self) -> Vec<u16> {
        self.minigame_ui
            .dance_surface
            .scene()
            .map(|s| s.cba_tsb.iter().flatten().copied().collect())
            .unwrap_or_default()
    }

    /// Per-vertex `[r, g, b, flag]` (the hybrid textured / fill layout).
    pub fn play_mg_dance_scene_flat_rgba(&self) -> Vec<u8> {
        self.minigame_ui
            .dance_surface
            .scene()
            .map(|s| s.flat_rgba.clone())
            .unwrap_or_default()
    }

    /// Triangle indices.
    pub fn play_mg_dance_scene_indices(&self) -> Vec<u32> {
        self.minigame_ui
            .dance_surface
            .scene()
            .map(|s| s.indices.clone())
            .unwrap_or_default()
    }

    /// The dance camera over the baked hall's frame: the world's staged
    /// venue camera (`World::minigames.dance_venue`, which
    /// `dance_venue::sync_dance_venue` re-frames every staged frame on the
    /// run's keyframe track - the camera the native window draws the hall
    /// with), else the entry's pose (`LegaiaMinigames::dance_venue_vp`).
    /// Empty when the cast did not decode.
    pub fn play_mg_dance_venue_vp(&self, aspect: f32) -> Vec<f32> {
        let Some(art) = self.minigame_art() else {
            return Vec::new();
        };
        let staged = self
            .scene_host
            .as_ref()
            .and_then(|h| h.world.minigames.dance_venue.as_ref())
            .map(|s| s.camera);
        match staged {
            Some(camera) => art.dance_venue_vp_with(&camera, aspect),
            None => art.dance_venue_vp(aspect),
        }
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

    /// This frame's drawable subset of [`Self::play_mg_dance_env_indices`]:
    /// the triangles the PSX GPU draws under the staged venue camera (the
    /// camera [`Self::play_mg_dance_venue_vp`] frames with), through the
    /// kernel the native window cuts its hall with
    /// (`dance_venue::psx_gpu_visible_indices`).
    pub fn play_mg_dance_env_visible_indices(&self) -> Vec<u32> {
        let Some(art) = self.minigame_art() else {
            return Vec::new();
        };
        let staged = self
            .scene_host
            .as_ref()
            .and_then(|h| h.world.minigames.dance_venue.as_ref())
            .map(|s| s.camera);
        match staged {
            Some(camera) => art.dance_env_visible_indices_with(&camera),
            None => art.dance_env_visible_indices(),
        }
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
