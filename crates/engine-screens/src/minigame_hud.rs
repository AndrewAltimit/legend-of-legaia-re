//! The minigames' screen-space PSX primitives: the dance hall's HUD quads and
//! count-in banner and the Baka Fighter HUD strips, projected off the world's
//! minigame seat onto the shared `engine-ui` emitters. Both play hosts used to
//! carry a copy of each.

use legaia_engine_minigame_scenes::minigame_state::MinigameState;
use legaia_engine_ui::dance as ud;
use legaia_render_kernels::screen_prim::ScreenPrim;

/// The dance HUD's textured quads, off the seat's one predicate
/// (`MinigameState::dance_hud_quads`: HUD up and page resident). Empty
/// otherwise, when the hosts draw the text rows.
pub fn dance_hud_prims(mg: &MinigameState) -> Vec<ScreenPrim> {
    let views: Vec<ud::DanceHudQuadView> = mg
        .dance_hud_quads()
        .iter()
        .map(|q| ud::DanceHudQuadView {
            poly_code: q.poly_code,
            rect: (q.x0, q.y0, q.x1, q.y1),
            uv: q.uv,
            rgb_top: q.rgb_top,
            rgb_bottom: q.rgb_bottom,
            clut: q.clut,
            tpage: q.tpage_attr,
        })
        .collect();
    ud::dance_hud_prims(&views, ud::COUNTIN_OT)
}

/// The count-in banner (READY, then `GO!` - `FUN_801cf470` states 4 / 5).
/// Empty outside the count-in and while the hall's HUD page is not resident,
/// in which case the hosts draw the placeholder letterforms instead. The art
/// comes off the run's own widget table, so the cell, page and palette are
/// the disc's.
pub fn dance_countin_prims(mg: &MinigameState) -> Vec<ScreenPrim> {
    if !mg.dance_hud_art_staged {
        return Vec::new();
    }
    if let Some(go) = mg.dance_countin_go {
        return mg
            .dance
            .as_ref()
            .and_then(|g| g.widget(ud::COUNTIN_GO_WIDGET))
            .map(|(w, abr)| {
                ud::dance_go_prims(
                    go,
                    ud::DanceCountInArt::from_widget(&w, abr),
                    ud::COUNTIN_OT,
                )
            })
            .unwrap_or_default();
    }
    let Some(env) = mg.dance_countin_banner.as_ref() else {
        return Vec::new();
    };
    let art = mg
        .dance
        .as_ref()
        .and_then(|g| g.widget(0))
        .map(|(w, abr)| ud::DanceCountInArt::from_widget(&w, abr))
        .unwrap_or_default();
    ud::dance_countin_prims(
        ud::DanceCountInView {
            x_offset: env.x_offset,
            brightness: env.brightness,
            hold: env.hold,
        },
        art,
        ud::COUNTIN_OT,
    )
}

/// The Baka Fighter screen layer for one frame, both play hosts' one call:
/// the duel HUD renderer's packets (`FUN_801d2afc` - VITAL bars and frames,
/// combo counters, round pips, attack icons, the STAGE / PRESS SELECT strip
/// and the high score; `BakaDuelSurface::duel_hud_packets`), then the
/// cabinet's and the round chrome's widget quads (`BakaDuelSurface::hud_quads`,
/// `FUN_801d5ed0`). Empty while the duel VRAM does not carry the HUD art
/// (`BakaDuelSurface::hud_art_resident`), when the hosts draw text labels.
///
/// The packets are submitted in retail's link order; the ordering-table walk
/// (`screen_prim::order_primitives`) then draws the HUD renderer's own
/// bucket under the widget emitter's.
///
/// REF: FUN_801d2afc, FUN_801d5ed0
pub fn baka_screen_prims(
    surface: &legaia_engine_minigame_scenes::baka_duel_scene::BakaDuelSurface,
    fight: &legaia_engine_minigames::baka_fighter::BakaFight,
) -> Vec<ScreenPrim> {
    use legaia_engine_minigames::baka_cabinet::DuelHudPacket;
    let mut out = Vec::new();
    for p in surface.duel_hud_packets(fight) {
        match p {
            DuelHudPacket::Bar { slot, bar } => {
                let (left, right) = if slot == 0 {
                    (bar.rgb_far, bar.rgb_anchor)
                } else {
                    (bar.rgb_anchor, bar.rgb_far)
                };
                out.push(legaia_engine_ui::baka_strips::baka_bar_prim(
                    &legaia_engine_ui::baka_strips::BakaBarView {
                        rect: (bar.x0, bar.y0, bar.x1, bar.y1),
                        rgb_left: left,
                        rgb_right: right,
                    },
                    legaia_engine_minigames::baka_cabinet::DUEL_HUD_OT,
                ));
            }
            DuelHudPacket::Cell { quad, ot } => {
                out.extend(legaia_engine_ui::baka_strips::baka_hud_prims_at(
                    &[quad_view(&quad)],
                    ot,
                ));
            }
        }
    }
    let views: Vec<ud::DanceHudQuadView> = surface.hud_quads(fight).iter().map(quad_view).collect();
    out.extend(legaia_engine_ui::baka_strips::baka_hud_prims(&views));
    out
}

/// The Baka Fighter frame's font-digit placements, `(stage x, stage y,
/// digit)`, both play hosts' one call: nothing across the cabinet's front end
/// (attract card, player select), else the number drawers' layout
/// (`baka_fighter_chrome::hud_digit_placements`) - minus its one-glyph round
/// digit while [`baka_screen_prims`] draws the duel HUD, whose STAGE strip
/// carries the retail number in its own cells.
pub fn baka_digit_placements(
    fight: &legaia_engine_minigames::baka_fighter::BakaFight,
    hud_art_drawn: bool,
) -> Vec<(i32, i32, u8)> {
    if fight.cabinet().front_end() {
        return Vec::new();
    }
    let mut placed = legaia_engine_minigames::baka_fighter_chrome::hud_digit_placements(
        fight.round() as i32,
        fight.tally().map(|t| (t.total(), t.gold_remaining())),
    );
    if hud_art_drawn {
        // The round digit is always the layout's first placement.
        placed.remove(0);
    }
    placed
}

fn quad_view(q: &legaia_engine_minigames::baka_fighter::HudWidgetQuad) -> ud::DanceHudQuadView {
    ud::DanceHudQuadView {
        poly_code: q.poly_code,
        rect: (q.x0, q.y0, q.x1, q.y1),
        uv: q.uv,
        rgb_top: q.rgb_top,
        rgb_bottom: q.rgb_bottom,
        clut: q.clut,
        tpage: q.tpage_attr,
    }
}

/// One frame of the Muscle Dome arena hub's screen quads, selected off the
/// world's hub timers ([`muscle_hub_quads`]).
#[derive(Debug, Clone, Default)]
pub struct MuscleHubQuads {
    /// The hub-screen quads in draw order.
    pub quads: Vec<legaia_engine_ui::muscle_dome_hud::HudQuad>,
    /// The first visit's backdrop shade and the index in [`Self::quads`] it
    /// draws before: the wall tiles sit below it, the screens composed over
    /// the wall above it.
    pub shade: Option<(usize, legaia_engine_ui::ringside_backdrop::BackdropShade)>,
    /// A leg is open (`SceneMode::MuscleDome` with a dome session). Outside
    /// one the arena hub is between legs, or the contest has handed the
    /// field back, and the INTERVAL and still screens are the hub's own.
    pub in_dome: bool,
}

/// The Muscle Dome hub screens this frame owes, off the world's hub timers
/// (`world.minigames.muscle_hub`) and contest, through the shared emitters
/// over a per-frame copy of the pristine PROT 0977 sprite `table` (the
/// retail emitters write their variant back into the records).
///
/// Inside a leg: the first visit's composed frame
/// (`ringside_backdrop::first_visit_hub_draw` - wall, shade, intro strip,
/// title zoom, course card, ROUND card), else the leg-open ROUND banner.
/// Between legs: the INTERVAL heading and the six-row score tally - the
/// roll's own rows while a roll is armed, the settled values otherwise -
/// then the re-entered hub's ROUND card over the still. Both play hosts
/// draw from this one selection; each keeps only its output form (sprite
/// draws on the native window, blit rows on the browser page) and the
/// ringside still under it.
pub fn muscle_hub_quads(
    world: &legaia_engine_core::world::World,
    table: &[legaia_engine_ui::muscle_dome_hud::HudSprite],
) -> MuscleHubQuads {
    use legaia_engine_ui::muscle_dome_hud as hud;
    use legaia_engine_ui::ringside_backdrop as rb;
    let timers = &world.minigames.muscle_hub;
    let in_dome = world.mode == legaia_engine_mode::mode::SceneMode::MuscleDome
        && world.minigames.muscle_dome.is_some();
    // The brightness argument is the screen's own fade counter, which clamps
    // at `HUB_FADE_FULL` (0x80) - the emitter's neutral, since it scales each
    // stored channel by `c * brightness / 256`.
    let mut table = table.to_vec();
    let mut quads: Vec<hud::HudQuad> = Vec::new();
    let mut shade = None;
    if in_dome {
        if let Some(hub) = timers.first_visit {
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
            shade = d.shade.map(|sh| (quads.len(), sh));
            quads.extend(d.hud);
        } else if let Some((round, banner)) = timers.round_banner {
            quads.extend(hud::hub_screen_quads(
                &mut table,
                &hud::round_banner_draws(round),
                banner.brightness(),
            ));
        }
    } else if let Some(interval) = timers.interval {
        let bright = interval.brightness();
        quads.extend(hud::hub_screen_quads(
            &mut table,
            hud::HUB_INTERVAL_HEADING,
            bright,
        ));
        // The six rows are the roll's own, not the settled totals: three
        // recovery lanes counting down, the HP they count into, the score
        // lane counting down and the coin tally counting up. With no roll
        // armed the screen draws the settled values, which is where the roll
        // ends anyway.
        let (values, row_bright) = match timers.tally.as_ref() {
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
        && timers.interval.is_none()
        && let Some(card) = timers.backdrop.and_then(|b| b.card_brightness())
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
    MuscleHubQuads {
        quads,
        shade,
        in_dome,
    }
}
