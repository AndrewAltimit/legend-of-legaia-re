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
    if let Some(level) = fight.cabinet_frame().clear_fade {
        out.push(legaia_engine_ui::baka_strips::baka_clear_fade(level));
    }
    // The round chrome's full-screen tint (the title card's red flash and
    // its fade-in): neither host drew it while the chrome produced it.
    if let Some(tint) = fight.chrome_frame().tint {
        out.push(legaia_engine_ui::baka_strips::baka_screen_tint(tint.rgb()));
    }
    if let Some((frame, _)) = fight.pause_panel() {
        out.push(legaia_engine_ui::baka_strips::baka_pause_shade(
            (frame.x, frame.y, frame.w, frame.h),
            legaia_engine_minigames::baka_cabinet::DUEL_HUD_OT,
        ));
    }
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

/// What a [`HubSprite`] samples.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HubSpriteSource {
    /// A hub page (`sheet` `0` = VRAM `(320, 0)`, `1` = `(320, 256)`) through
    /// 16-colour sub-palette `pal`; `tpage` / `clut` are the retail packet's
    /// words, which address the same texels in [`legaia_engine_ui::ringside_backdrop::hub_vram`].
    Page {
        sheet: u8,
        pal: u8,
        tpage: u16,
        clut: u16,
    },
    /// Ringside still `variant`, in still-sheet coordinates (VRAM `(384, 0)`
    /// is the sheet's origin).
    Still { variant: u32 },
    /// The first visit's backdrop shade: an untextured row, `color` the
    /// value it subtracts.
    Shade,
}

/// One Muscle Dome hub draw in the form both play hosts rasterise: a
/// rectangle of the retail 320x240 frame and the texel rectangle it samples,
/// mapped linearly onto it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HubSprite {
    pub source: HubSpriteSource,
    /// `(x, y, w, h)` in the retail frame, clipped to it.
    pub dst: (i32, i32, u32, u32),
    /// `(x, y, w, h)` in the source's own texels: page texels for
    /// [`HubSpriteSource::Page`], still-sheet pixels for
    /// [`HubSpriteSource::Still`]; unused for the shade.
    pub src: (u32, u32, u32, u32),
    /// Modulation colour, `0x80` neutral (`texel * c / 128`); the shade's
    /// subtracted value for [`HubSpriteSource::Shade`], already at the
    /// framebuffer's 5 bits.
    pub color: [u8; 3],
    /// The ABR equation the draw blends with, `None` for an opaque draw.
    pub abr: Option<u8>,
}

/// The hub's draws this frame, back to front, through the one decomposition
/// both play hosts draw: the ringside still under a re-entered hub (retail
/// links its two packets at the ordering table's far end), then
/// [`muscle_hub_quads`] with the first visit's shade between the wall tiles
/// and the screens drawn over them.
///
/// - A hub quad's extent is the sprite-table emitters' inclusive `+ 1`.
///   The retail display is the 320x240 frame and the GPU clips to it; the
///   first visit's wall tiles run past it (three 128-wide columns, two
///   128-high rows), so each draw is clipped here, its texel window shrunk
///   with the same ratio.
/// - A packet's vertical two-stop colour gradient (the course-title face's
///   record carries one) goes out as one draw per display row, each at the
///   Gouraud colour of its row and sampling the texel row the GPU steps to.
/// - A semi packet blends only through STP texels: `stp` classifies the hub
///   CLUTs as the arena uploads them (`HubPaletteStp::quad_abr`), so the
///   variant-2 knockout palettes blend subtractively (`B - F`) under the face
///   the variant-1 pass adds over them.
/// - The shade (`FUN_801D1610`, `B - F`, `0x64` at the top to `0` at the
///   bottom) goes out one row at a time at that row's Gouraud value, which
///   the GPU blends at the framebuffer's 5 bits: the packet's draw mode
///   `0x46` leaves dither off, so the value loses its low 3 bits. A row that
///   subtracts nothing is skipped.
pub fn muscle_hub_sprites(
    world: &legaia_engine_core::world::World,
    table: &[legaia_engine_ui::muscle_dome_hud::HudSprite],
    stp: &legaia_engine_ui::ringside_backdrop::HubPaletteStp,
) -> Vec<HubSprite> {
    use legaia_engine_ui::ringside_backdrop as rb;
    let MuscleHubQuads {
        quads,
        shade,
        in_dome,
    } = muscle_hub_quads(world, table);
    let mut out = Vec::new();
    if !in_dome && let Some(b) = world.minigames.muscle_hub.backdrop.filter(|b| b.visible()) {
        for q in rb::ringside_still_quads(b.level()) {
            let Some(d) = rb::StillDraw::from_quad(&q) else {
                continue;
            };
            out.push(HubSprite {
                source: HubSpriteSource::Still {
                    variant: b.variant(),
                },
                dst: d.dst,
                src: d.src,
                color: [d.level; 3],
                abr: None,
            });
        }
    }
    let shade_at = shade.map_or(quads.len(), |(at, _)| at);
    let shade = shade.map(|(_, sh)| sh);
    for (i, q) in quads.iter().enumerate() {
        if i == shade_at
            && let Some(sh) = shade
        {
            out.extend(shade_rows(&sh));
        }
        out.extend(hub_quad_sprites(q, stp.quad_abr(q)));
    }
    if shade_at >= quads.len()
        && let Some(sh) = shade
    {
        out.extend(shade_rows(&sh));
    }
    out
}

/// One hub quad as [`HubSprite`]s: clipped to the frame, one draw per row
/// when its colour is a vertical gradient.
fn hub_quad_sprites(
    q: &legaia_engine_ui::muscle_dome_hud::HudQuad,
    abr: Option<u8>,
) -> Vec<HubSprite> {
    let source = HubSpriteSource::Page {
        sheet: u8::from(q.tpage & 0x10 != 0),
        pal: (q.clut & 0x3F) as u8,
        tpage: q.tpage,
        clut: q.clut,
    };
    let dw = (i32::from(q.xy[1].0) - i32::from(q.xy[0].0) + 1).max(0) as u32;
    let dh = (i32::from(q.xy[2].1) - i32::from(q.xy[0].1) + 1).max(0) as u32;
    let sw = (i32::from(q.uv[1].0) - i32::from(q.uv[0].0) + 1).max(0) as u32;
    let sh = (i32::from(q.uv[2].1) - i32::from(q.uv[0].1) + 1).max(0) as u32;
    if dw == 0 || dh == 0 || sw == 0 || sh == 0 {
        return Vec::new();
    }
    // PSX texture modulation is `texel * c / 128`, with `c` the Gouraud
    // colour of the row: the per-vertex colours are a vertical two-stop
    // gradient, top corners to bottom corners.
    let tint_at = |row: i64| -> [u8; 3] {
        let span = (dh as i64 - 1).max(1);
        std::array::from_fn(|k| {
            let (a, b) = (i64::from(q.rgb[0][k]), i64::from(q.rgb[2][k]));
            (a + (b - a) * row / span).clamp(0, 255) as u8
        })
    };
    let (dx, dy) = (i64::from(q.xy[0].0), i64::from(q.xy[0].1));
    let (x0, x1) = (dx.max(0), (dx + dw as i64).min(320));
    let (y0, y1) = (dy.max(0), (dy + dh as i64).min(240));
    if x1 <= x0 || y1 <= y0 {
        return Vec::new();
    }
    let sx = i64::from(q.uv[0].0) + (x0 - dx) * sw as i64 / dw as i64;
    let sy = i64::from(q.uv[0].1) + (y0 - dy) * sh as i64 / dh as i64;
    let csw = ((x1 - x0) * sw as i64 / dw as i64).max(1) as u32;
    let csh = ((y1 - y0) * sh as i64 / dh as i64).max(1) as u32;
    let draws: Vec<HubSprite> = if q.rgb[0] != q.rgb[2] {
        (y0..y1)
            .map(|y| {
                let row = y - dy;
                let v = i64::from(q.uv[0].1) + row * sh as i64 / dh as i64;
                HubSprite {
                    source,
                    dst: (x0 as i32, y as i32, (x1 - x0) as u32, 1),
                    src: (sx as u32, v as u32, csw, 1),
                    color: tint_at(row),
                    abr,
                }
            })
            .collect()
    } else {
        vec![HubSprite {
            source,
            dst: (x0 as i32, y0 as i32, (x1 - x0) as u32, (y1 - y0) as u32),
            src: (sx as u32, sy as u32, csw, csh),
            color: tint_at(0),
            abr,
        }]
    };
    draws.iter().flat_map(unit_runs).collect()
}

/// Most stretched runs an axis is split into by [`unit_runs`]; an axis that
/// needs more (the title's zoom) stays one scaled draw.
const UNIT_RUNS_MAX: usize = 3;

/// One axis of a draw, `d` pixels over `s` texels, as the runs over which the
/// centre-sampling rule `t(p) = floor((p + 1/2) * s / d)` steps one texel per
/// pixel: `(pixel offset, texel offset, length)`. `None` when that takes
/// more than [`UNIT_RUNS_MAX`] runs.
fn axis_runs(d: u32, s: u32) -> Option<Vec<(u32, u32, u32)>> {
    let t = |p: u32| ((2 * u64::from(p) + 1) * u64::from(s) / (2 * u64::from(d))) as u32;
    let mut runs: Vec<(u32, u32, u32)> = Vec::new();
    for p in 0..d {
        let tp = t(p);
        match runs.last_mut() {
            Some((p0, t0, n)) if tp == *t0 + (p - *p0) => *n += 1,
            _ => {
                if runs.len() == UNIT_RUNS_MAX {
                    return None;
                }
                runs.push((p, tp, 1));
            }
        }
    }
    Some(runs)
}

/// A draw whose texel window is one or two texels off its pixel extent
/// (the emitters' inclusive `+ 1` stretches a 128-texel wall tile over 129
/// pixels, the frame clip leaves 63 over 64) as one-texel-per-pixel draws,
/// with the repeated texel the one the centre-sampling rule picks.
///
/// A stretch of that kind puts one pixel's sample exactly on a texel edge,
/// and which side a rasteriser takes there is its own half-texel convention:
/// the native window's sprite pass and the play page's screen-primitive pass
/// differ, so each drew the wall's repeated column one texel apart. Deciding
/// the repeat here leaves neither a tie. A draw that needs more runs (the
/// title's zoom) stays scaled.
fn unit_runs(d: &HubSprite) -> Vec<HubSprite> {
    let (x, y, dw, dh) = d.dst;
    let (u, v, sw, sh) = d.src;
    if (dw, dh) == (sw, sh) {
        return vec![*d];
    }
    let (Some(xr), Some(yr)) = (axis_runs(dw, sw), axis_runs(dh, sh)) else {
        return vec![*d];
    };
    let mut out = Vec::with_capacity(xr.len() * yr.len());
    for &(py, ty, ny) in &yr {
        for &(px, tx, nx) in &xr {
            out.push(HubSprite {
                dst: (x + px as i32, y + py as i32, nx, ny),
                src: (u + tx, v + ty, nx, ny),
                ..*d
            });
        }
    }
    out
}

/// The first visit's shade as one subtractive row per display row.
fn shade_rows(sh: &legaia_engine_ui::ringside_backdrop::BackdropShade) -> Vec<HubSprite> {
    let (x0, y0) = (i32::from(sh.xy[0].0), i32::from(sh.xy[0].1));
    let w = (i32::from(sh.xy[1].0) - x0).max(0) as u32;
    let h = (i32::from(sh.xy[2].1) - y0).max(0);
    let top = i32::from(sh.rgb[0][0]);
    let bottom = i32::from(sh.rgb[2][0]);
    (0..h)
        .filter_map(|row| {
            let f = (top + (bottom - top) * row / h) & !7;
            (f > 0).then(|| HubSprite {
                source: HubSpriteSource::Shade,
                dst: (x0, y0 + row, w, 1),
                src: (0, 0, 1, 1),
                color: [f.clamp(0, 255) as u8; 3],
                abr: Some(sh.abr),
            })
        })
        .collect()
}

/// The 15-bit texture page holding still-sheet column `sheet_x` (the sheet's
/// origin is VRAM `(384, 0)`) and the column's `u` inside it - the page
/// retail's still packets name (`0x106` for column `0`, `0x109` for `192`).
pub fn still_page(sheet_x: u32) -> (u16, u32) {
    let x =
        u32::from(legaia_engine_battle_vm::panel_backread_loader::BACKREAD_RECT_X as u16) + sheet_x;
    let page = x / 64;
    (0x100 | page as u16, x - page * 64)
}

/// [`muscle_hub_sprites`] as screen primitives over
/// [`legaia_engine_ui::ringside_backdrop::hub_vram`]: each draw a quad over
/// its rectangle sampling its texel rectangle by the retail texture page
/// and CLUT, the still through a 15-bit page at its VRAM column, the shade a
/// flat subtractive row. All share one ordering-table slot and are
/// submitted in reverse paint order, so the slot's LIFO walk
/// (`screen_prim::order_primitives`) draws them in `sprites` order. A draw
/// whose texel window would leave its texture page is dropped (none of the
/// hub's does).
pub fn hub_sprite_prims(sprites: &[HubSprite]) -> Vec<ScreenPrim> {
    use legaia_render_kernels::screen_prim::{FlatQuad, ScreenQuad};
    let rgb = |c: [u8; 3]| (u32::from(c[0]) << 16) | (u32::from(c[1]) << 8) | u32::from(c[2]);
    let corners = |(x, y, w, h): (i32, i32, u32, u32)| {
        let (x1, y1) = (x + w as i32, y + h as i32);
        let p = |a: i32, b: i32| (a as i16, b as i16);
        [p(x, y), p(x1, y), p(x, y1), p(x1, y1)]
    };
    let uvs = |u: u32, v: u32, w: u32, h: u32| -> Option<[(u8, u8); 4]> {
        let (u1, v1) = (u + w, v + h);
        (u1 <= 255 && v1 <= 255).then(|| {
            let (u, v, u1, v1) = (u as u8, v as u8, u1 as u8, v1 as u8);
            [(u, v), (u1, v), (u, v1), (u1, v1)]
        })
    };
    sprites
        .iter()
        .filter_map(|s| {
            let (u, v, w, h) = s.src;
            let textured = |tpage: u16, clut: u16, uv: [(u8, u8); 4]| {
                ScreenPrim::Textured(ScreenQuad {
                    xy: corners(s.dst),
                    uv,
                    clut,
                    tpage: match s.abr {
                        Some(abr) => (tpage & !0x60) | (u16::from(abr & 3) << 5),
                        None => tpage,
                    },
                    color: rgb(s.color),
                    gouraud: None,
                    semi_transparent: s.abr.is_some(),
                    ot_index: 0,
                    depth: None,
                })
            };
            match s.source {
                HubSpriteSource::Page { tpage, clut, .. } => {
                    Some(textured(tpage, clut, uvs(u, v, w, h)?))
                }
                HubSpriteSource::Still { .. } => {
                    let (tpage, u) = still_page(u);
                    Some(textured(tpage, 0, uvs(u, v, w, h)?))
                }
                HubSpriteSource::Shade => Some(ScreenPrim::Flat(FlatQuad {
                    xy: corners(s.dst),
                    color: [s.color[0], s.color[1], s.color[2], 0xFF],
                    gouraud: None,
                    semi_transparent: true,
                    abr_mode: s.abr.unwrap_or(2),
                    ot_index: 0,
                    depth: None,
                })),
            }
        })
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

/// The shared minigame effect pool's live parts (`MinigameState::fx` - the
/// fishing splash, wander ripples, catch bursts) as the view the
/// `engine-ui::minigame_fx::fx_part_draws` builder takes.
pub fn fx_part_views(
    pool: &legaia_engine_minigames::minigame_fx::MinigameFxPool,
) -> Vec<legaia_engine_ui::minigame_fx::FxPartView> {
    pool.frames()
        .into_iter()
        .map(|p| legaia_engine_ui::minigame_fx::FxPartView {
            x: p.x as i32,
            y: p.y as i32,
            sprite: p.sprite,
            fade: p.fade,
        })
        .collect()
}

/// The dance run's own sprite-part emits as the view the
/// `engine-ui::minigame_fx::dance_sprite_part_draws` builder takes.
///
/// The geometry is the port's: `dance::sprite_part_emit` (`FUN_801d387c`)
/// resolves each live part's arm off its own actor record, and
/// `dance::sprite_part_fade_weight` its alpha. Only the `Shadowed` arm draws
/// the cell twice; the template / marker-Z arms carry no screen pair to draw.
pub fn dance_sprite_part_views(
    frames: &[legaia_engine_minigames::dance::SpritePartFrame],
) -> Vec<legaia_engine_ui::minigame_fx::DanceSpritePartView> {
    use legaia_engine_minigames::dance::SpritePartEmit;
    frames
        .iter()
        .filter_map(|f| {
            let (x, y, shadow) = match f.emit {
                SpritePartEmit::Shadowed { x, y, .. } => (x, y, true),
                SpritePartEmit::Plain { x, y, .. } | SpritePartEmit::Marker { x, y, .. } => {
                    (x, y, false)
                }
                SpritePartEmit::CopyTemplate
                | SpritePartEmit::SetTemplateZ
                | SpritePartEmit::None => return None,
            };
            Some(legaia_engine_ui::minigame_fx::DanceSpritePartView {
                x: x as i32,
                y: y as i32,
                sprite: f.sprite,
                fade: f.fade,
                shadow,
            })
        })
        .collect()
}

/// The slot machine's five paylines as screen primitives: the ported payline
/// pass (`FUN_801D3380`) with its projection (`SlotMachine::payline_segments`)
/// through the shared `slot_paylines` builder. Empty on a rules page, which
/// replaces the machine, paylines included.
///
/// Its own list, not part of [`slot_cabinet_prims`]: the composer appends it
/// after the cabinet (`screen_layers::compose_screen_prims`), and the
/// paylines and the cash-out box share OT bucket `2`, where the later append
/// draws first - so the box covers the lines on every host.
pub fn slot_payline_prims(
    m: &legaia_engine_minigames::slot_machine::SlotMachine,
) -> Vec<ScreenPrim> {
    use legaia_engine_ui::slot_paylines as usp;
    if matches!(
        m.screen(),
        legaia_minigame_formats::minigame_slot_scene::SlotScreen::Instructions { .. }
    ) {
        return Vec::new();
    }
    let segments: Vec<usp::PaylineSegment> = m
        .payline_segments()
        .iter()
        .map(|l| usp::PaylineSegment {
            a: [l.a.0, l.a.1],
            b: [l.b.0, l.b.1],
            rgb: [l.prim.color.0, l.prim.color.1, l.prim.color.2],
            semi: l.prim.code & 0x02 != 0,
        })
        .collect();
    usp::payline_screen_prims(&segments)
}

/// The whole machine as screen primitives - cabinet, reels, furniture, dot
/// matrix and coin HUD through `slot_cabinet::slot_cabinet_prims`, then the
/// cash-out flow over it (the picker, the prompt, the fade; on a rules page,
/// the page alone). `dots` is the marquee's composed buffer for this frame,
/// empty for the cleared matrix; `blink` its blink counter.
pub fn slot_cabinet_prims(
    m: &legaia_engine_minigames::slot_machine::SlotMachine,
    assets: &legaia_engine_ui::slot_cabinet::SlotCabinetAssets,
    dots: &[u8],
    blink: u32,
) -> Vec<ScreenPrim> {
    use legaia_engine_ui::slot_cabinet as usc;
    let menu = usc::slot_menu_prims(assets, m.screen(), m.fade_level());
    if matches!(
        m.screen(),
        legaia_minigame_formats::minigame_slot_scene::SlotScreen::Instructions { .. }
    ) {
        return menu;
    }
    let strips = m.strips();
    let clear;
    let dots: &[u8] = if dots.is_empty() {
        clear = legaia_minigame_formats::minigame_slot_scene::clear_dots();
        &clear
    } else {
        dots
    };
    let mut prims = usc::slot_cabinet_prims(&usc::SlotCabinetInput {
        scene: &assets.scene,
        cabinet: assets.cabinet.as_ref(),
        hud: &assets.hud,
        reel_pos: core::array::from_fn(|r| m.reel_pos(r)),
        strips: [&strips[0], &strips[1], &strips[2]],
        stop_open: core::array::from_fn(|r| m.reel_stop_open(r)),
        winning_line: m.winning_line_word(),
        dots,
        blink,
        balance: m.balance(),
    });
    prims.extend(menu);
    prims
}

#[cfg(test)]
mod hub_sprite_tests {
    use super::*;
    use legaia_render_kernels::screen_prim::ScreenPrim;

    fn page_sprite(dst: (i32, i32, u32, u32), src: (u32, u32, u32, u32)) -> HubSprite {
        HubSprite {
            source: HubSpriteSource::Page {
                sheet: 0,
                pal: 4,
                tpage: 0x05,
                clut: (502 << 6) | 4,
            },
            dst,
            src,
            color: [0x80; 3],
            abr: None,
        }
    }

    #[test]
    fn a_wall_tile_stretched_by_one_repeats_the_centre_samples_texel() {
        // 128 texels over 129 pixels: pixel 64's centre sits on texel edge
        // 64 exactly; the rule takes texel 64 there and repeats it at 65.
        assert_eq!(axis_runs(129, 128), Some(vec![(0, 0, 65), (65, 64, 64)]));
        // The frame clip's 63 texels over 64 pixels: one repeat.
        let r = axis_runs(64, 63).unwrap();
        assert_eq!(r.len(), 2);
        assert_eq!(r.iter().map(|&(_, _, n)| n).sum::<u32>(), 64);
        assert_eq!(r.last().map(|&(p, t, n)| (p + n, t + n)), Some((64, 63)));
        // The title's zoom stays one scaled draw.
        assert_eq!(axis_runs(296, 218), None);
    }

    #[test]
    fn unit_runs_tile_the_draw_one_texel_per_pixel() {
        let d = page_sprite((0, 0, 129, 129), (0, 64, 128, 128));
        let runs = unit_runs(&d);
        assert_eq!(runs.len(), 4);
        let mut area = 0;
        for r in &runs {
            assert_eq!((r.dst.2, r.dst.3), (r.src.2, r.src.3), "1:1: {r:?}");
            area += r.dst.2 * r.dst.3;
        }
        assert_eq!(area, 129 * 129);
        // An unscaled draw passes through untouched.
        let flat = page_sprite((40, 111, 240, 18), (0, 224, 240, 18));
        assert_eq!(unit_runs(&flat), vec![flat]);
    }

    #[test]
    fn hub_prims_paint_in_list_order_through_the_retail_pages() {
        let a = page_sprite((0, 0, 8, 8), (0, 0, 8, 8));
        let still = HubSprite {
            source: HubSpriteSource::Still { variant: 0 },
            dst: (192, -20, 128, 240),
            src: (192, 0, 128, 240),
            color: [0x80; 3],
            abr: None,
        };
        let shade = HubSprite {
            source: HubSpriteSource::Shade,
            dst: (0, 3, 320, 1),
            src: (0, 0, 1, 1),
            color: [0x60; 3],
            abr: Some(2),
        };
        let prims = hub_sprite_prims(&[still, a, shade]);
        // One ordering-table slot, LIFO: submitted reversed, drawn in order.
        let order = legaia_render_kernels::screen_prim::order_primitives(&prims);
        let drawn: Vec<_> = order.iter().map(|&i| prims[i]).collect();
        let ScreenPrim::Textured(s) = drawn[0] else {
            panic!("the still first");
        };
        assert_eq!((s.tpage, s.uv[0], s.uv[3]), (0x109, (0, 0), (128, 240)));
        let ScreenPrim::Textured(p) = drawn[1] else {
            panic!("the page sprite second");
        };
        assert_eq!(
            (p.tpage, p.clut, p.color),
            (0x05, (502 << 6) | 4, 0x80_8080)
        );
        let ScreenPrim::Flat(f) = drawn[2] else {
            panic!("the shade last");
        };
        assert!(f.semi_transparent && f.abr_mode == 2);
        assert_eq!(still_page(0), (0x106, 0));
        assert_eq!(still_page(192), (0x109, 0));
    }
}
