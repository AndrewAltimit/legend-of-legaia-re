//! The fishing HUD's one-shot **banners**, stepped once per sim tick.

use legaia_engine_core::world::World;
use legaia_engine_fishing::fishing::PondEvent;
use legaia_engine_ui::{FishingBanners, HudDraw};

/// One sim tick of the fishing banner one-shots (hook, reel-in, miss, the
/// auxiliary converging pair, the strike splash): seed a timer on each event
/// the pond session raised this tick (`World::minigames.fishing_events`),
/// then service every timer through the retail driver-tail loop
/// ([`FishingBanners::service_frame`]) and hand back this tick's draws.
///
/// With no fishing session the timers are dropped with it and the list is
/// empty, so a half-run banner cannot reappear on the next visit.
///
/// The frame step is the engine's fixed one per sim tick; retail reads
/// `DAT_1F800393`, its frame-rate compensation word. Both play hosts call
/// this from their frame path and cache the result for a draw pass that
/// cannot advance the timers itself.
pub fn fishing_banner_frame(world: &World, banners: &mut FishingBanners) -> Vec<HudDraw> {
    if world.minigames.fishing.is_none() {
        *banners = FishingBanners::default();
        return Vec::new();
    }
    for event in &world.minigames.fishing_events {
        match event {
            PondEvent::Splash => banners.splash.start(),
            PondEvent::Hooked(_) => banners.on_hook(),
            PondEvent::Landed(_) => banners.on_landed(),
            PondEvent::Snapped => banners.on_snapped(),
            PondEvent::Recast => banners.on_recast(),
            PondEvent::LureRing(..) => {}
        }
    }
    banners.service_frame(1)
}

/// This frame's fishing HUD draw list, the one both play hosts' text and
/// sprite passes read: the retail persistent rows (`FUN_801d13f0`: best
/// catch, capped point total, lure label, lures remaining) at their traced
/// stage-pixel pens, the catch HUD while a cast is out (`FUN_801d1580`, its
/// gauge block only while the fish is on), a landed catch's result plate
/// (`FUN_801D5298`: rank, points in the large digits, label and the rising
/// species name), then `banners`, this frame's live one-shots from
/// [`fishing_banner_frame`]. Empty with no fishing session.
pub fn fishing_hud_items(world: &World, banners: &[HudDraw]) -> Vec<HudDraw> {
    let Some(s) = world.minigames.fishing.as_ref() else {
        return Vec::new();
    };
    // The lure index is the session's - the entry's ownership gate already
    // re-pointed it at an owned lure.
    let lure = s.lure;
    let lures_left = *world
        .party
        .inventory
        .get(&(legaia_engine_fishing::fishing::lure_item_id(lure) as u8))
        .unwrap_or(&0) as i32;
    let mut items = legaia_engine_ui::persistent_hud_draws(
        s.record.points,
        s.record.best_points,
        lure,
        lures_left,
    );
    // One derivation for the catch HUD (`PondSession::catch_hud`).
    let c = s.catch_hud();
    if c.visible {
        items.extend(legaia_engine_ui::catch_hud_draws(
            &legaia_engine_ui::CatchHudState {
                record: c.record,
                line_extent: c.line_extent,
                cast_power: c.cast_power,
                depth: c.depth,
                tension: c.tension,
                gauges_visible: c.gauges_visible,
            },
        ));
    }
    if let Some(r) = s.catch_result() {
        let name_len = world
            .minigames
            .fishing_captions
            .as_ref()
            .map_or(0, |t| t.species_name(r.species).len());
        items.extend(legaia_engine_ui::catch_result_draws(
            r.ramp,
            r.strength,
            r.points,
            r.species as u32,
            name_len,
        ));
    }
    items.extend(banners.iter().copied());
    // The HUD runs in the driver's tail, after the state's screens, so the
    // ordering table draws it under their panels (`FUN_801D13F0` after the
    // switch, `0x801D01A4..`): an item a panel covers does not show.
    let covered = venue_panel_footprints(world);
    items.retain(|d| {
        let (x, y) = hud_anchor(d);
        !covered
            .iter()
            .any(|&(fx, fy, fw, fh)| x >= fx && x < fx + fw && y >= fy && y < fy + fh)
    });
    items
}

/// Where a HUD item is pinned, for the cover test.
fn hud_anchor(d: &HudDraw) -> (i32, i32) {
    match *d {
        HudDraw::Number { x, y, .. }
        | HudDraw::LargeNumber { x, y, .. }
        | HudDraw::Glyph { x, y, .. }
        | HudDraw::Count { x, y, .. }
        | HudDraw::Caption { x, y, .. }
        | HudDraw::Bar { x, y, .. }
        | HudDraw::PowerBar { x, y, .. } => (x, y),
    }
}

/// The footprints (stage pixels, the emitter's rect grown by 8) of the
/// venue screens' panels this frame: the hub's, and the exchange's list and
/// dialogs ([`fishing_venue_screen`] draws them).
pub fn venue_panel_footprints(world: &World) -> Vec<(i32, i32, i32, i32)> {
    use legaia_engine_fishing::fishing_chrome::centred_panel;
    let mut rects: Vec<(i16, i16, i16, i16)> = Vec::new();
    let centred = |cx: i16, y: i16, w: i16, h: i16, out: &mut Vec<_>| {
        if let Some(p) = centred_panel(cx, y, w, h) {
            out.push((p.x, p.y, p.w, p.h));
        }
    };
    for l in world.fishing_hub_layers() {
        if let Some((cx, y, w, h)) = l.panel {
            centred(cx, y, w, h, &mut rects);
        }
        rects.extend(l.frame);
    }
    if let Some(ex) = world.minigames.fishing_exchange.as_ref() {
        let m = &world.minigames;
        let (cx, y, w, h) = ex.list_draw(false, m.fishing_points, |_| true).panel;
        centred(cx, y, w, h, &mut rects);
        let d = &m.fishing_exchange_dialog;
        for l in ex.dialog_layers(d, m.fishing_points, 0, m.fishing_prizes_purchased) {
            for &(cx, y, w, h) in &l.panels {
                centred(cx, y, w, h, &mut rects);
            }
        }
    }
    rects
        .into_iter()
        .map(|(x, y, w, h)| {
            legaia_engine_ui::dialog_window_footprint((
                i32::from(x),
                i32::from(y),
                i32::from(w),
                i32::from(h),
            ))
        })
        .collect()
}

/// The fishing rod and line as screen primitives for this frame: the rod
/// model the rod actor posed (`PondSession::rod_faces`, wrapped by the shared
/// `fishing::rod` builder), then the session's line
/// (`legaia_engine_minigame_scenes::fishing_venue::fishing_line_frame`, the
/// fish end projected through the camera the pond draws with: the venue
/// camera, else the host's follow camera framed on `scene_center_xz`),
/// wrapped by the shared `fishing::line` builder. Empty outside the fishing
/// mode, and with no rod and no line out.
///
/// `catch_line` is the result plate's line the pond surface projected
/// (`FishingScene::catch_line`: from the top of the frame to the landed
/// fish's tie vertex, retail's `0x50` packet in `FUN_801D5298`), drawn
/// through the same builder with its packet's two colours.
pub fn fishing_line_screen_prims(
    world: &mut World,
    camera: &legaia_engine_field::camera::Camera,
    scene_center_xz: [f32; 2],
    catch_line: Option<((i16, i16), (i16, i16))>,
) -> Vec<legaia_render_kernels::screen_prim::ScreenPrim> {
    if world.mode != legaia_engine_mode::mode::SceneMode::Fishing {
        return Vec::new();
    }
    let venue = legaia_engine_minigame_scenes::fishing_venue::venue_view(&world.minigames);
    let venue_camera = venue.is_some();
    let view = venue.or_else(|| {
        legaia_engine_world_views::camera_view::resolve_field_camera(
            world,
            camera,
            None,
            scene_center_xz,
        )
        .field_view()
    });
    // The rod model first: its actor runs ahead of the lure tick, so in a
    // shared bucket its packets are the earlier `AddPrim`s.
    let mut prims: Vec<_> = world
        .minigames
        .fishing
        .as_ref()
        .map(|p| p.rod_faces())
        .unwrap_or_default()
        .into_iter()
        .map(|f| legaia_engine_ui::fishing::rod::fishing_rod_prim(f.xy, f.rgb, f.ot))
        .collect();
    prims.extend(
        legaia_engine_minigame_scenes::fishing_venue::fishing_line_frame(
            &mut world.minigames,
            view.as_ref(),
            venue_camera,
        )
        .and_then(|l| {
            legaia_engine_ui::fishing::line::fishing_line_prim(
                l.fish, l.rod, l.fish_rgb, l.rod_rgb, l.ot,
            )
        }),
    );
    use legaia_engine_fishing::fishing_actors::{PLATE_LINE_TIE_RGB, PLATE_LINE_TOP_RGB};
    prims.extend(catch_line.and_then(|(top, tie)| {
        legaia_engine_ui::fishing::line::fishing_line_prim(
            tie,
            top,
            PLATE_LINE_TIE_RGB,
            PLATE_LINE_TOP_RGB,
            0,
        )
    }));
    prims
}

/// The fishing venue's screens over the pond, composed once for both play
/// hosts: the hub (its menu, tackle list, help pages and the no-lure line)
/// and the point exchange (the prize list, the quantity picker, the
/// confirm), each screen with the panel it sits on.
#[derive(Debug, Clone, Default)]
pub struct FishingVenueScreen {
    /// The screens' chrome, back to front: each screen's panel, then its
    /// cursor icon. Hosts draw it in their chrome pass under every text
    /// ([`fishing_venue_chrome_sprites`]).
    pub chrome: Vec<VenueChrome>,
    /// Stage-pixel texts, back to front, each screen's cropped against the
    /// panels of the screens drawn over it
    /// (`legaia_engine_ui::fishing::exchange::occlude_text_draws`).
    pub texts: Vec<legaia_engine_ui::TextDraw>,
}

/// One piece of the venue screens' chrome, in stage pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VenueChrome {
    /// A panel as the box emitter `FUN_8002C69C` takes it (`FUN_801D74B0`'s
    /// centred rect resolved), in style `0x44`
    /// (`legaia_engine_ui::fishing::exchange::venue_panel_draws_for`).
    Panel((i32, i32, i32, i32)),
    /// The cursor icon `FUN_8002C488(x, y, 0x4E)`
    /// (`legaia_engine_ui::fishing::exchange::venue_hand_draw_for`).
    Hand((i32, i32)),
}

/// A `FUN_801D74B0` panel's `(cx, y, w, h)`.
type CentredRect = (i16, i16, i16, i16);

/// One screen of the venue: its panel, its cursor icons and its text.
struct VenueLayer {
    panel: Option<CentredRect>,
    /// A box the screen emits itself (`FUN_8002C69C`'s own rect).
    frame: Option<CentredRect>,
    hands: Vec<(i32, i32)>,
    texts: Vec<legaia_engine_ui::TextDraw>,
}

/// The fishing venue's screens this frame ([`FishingVenueScreen`]). Empty
/// with nothing up.
///
/// The layers stack as the session machine calls their routines, the live
/// one first, so its primitives draw last: the hub menu
/// (`FUN_801D0474`) under the tackle list or the prize list, the list under
/// the quantity picker, the picker under the confirm
/// (`0x801D0028..0x801D0080`). The list opens on the pond's own page and
/// draws at retail's pens (`PrizeExchange::list_draw`); the dialogs at
/// theirs (`PrizeExchange::dialog_layers`), their lines off the overlay
/// (`FishingHubText::prize`) and the item names off the item table.
///
/// `hand_sprites`: the host draws the cursor icon from the system-UI atlas
/// ([`VenueChrome::Hand`]); without the atlas the cursors stay the `>` text
/// every host draws for that icon.
pub fn fishing_venue_screen(
    world: &World,
    font: &legaia_font::Font,
    hand_sprites: bool,
) -> FishingVenueScreen {
    use legaia_engine_fishing::fishing_chrome::centred_panel;
    use legaia_engine_ui::fishing::exchange as fx;
    let ink = legaia_engine_ui::MENU_TEXT_WHITE;
    let cursor_text = |hands: &[(i32, i32)]| {
        legaia_engine_ui::text_lines::text_line_draws_for(
            font,
            hands.iter().map(|&(x, y)| (&b">"[..], x, y, false)),
            ink,
            ink,
        )
    };
    let mut layers: Vec<VenueLayer> = Vec::new();
    for l in world.fishing_hub_layers() {
        // The tackle list's rows stage ink 7 on the equipped lure (if held)
        // and rod and ink 0 on the rest (`0x801D1120..0x801D1334`); every
        // other hub line is ink 7.
        let tackle = l.panel == Some(legaia_engine_fishing::fishing_hub::TACKLE_PANEL);
        let unmarked = if tackle {
            legaia_engine_ui::fishing::exchange::EXCHANGE_DIM_INK
        } else {
            ink
        };
        let hands: Vec<(i32, i32)> = l
            .lines
            .iter()
            .filter(|h| hand_sprites && h.is_cursor())
            .map(|h| (i32::from(h.x), i32::from(h.y)))
            .collect();
        let mut texts = legaia_engine_ui::text_lines::text_line_draws_for(
            font,
            l.lines
                .iter()
                .filter(|h| !(hand_sprites && h.is_cursor()) && h.number.is_none())
                .map(|h| (&h.text[..], i32::from(h.x), i32::from(h.y), h.marked)),
            unmarked,
            ink,
        );
        for h in &l.lines {
            if let Some((value, places)) = h.number {
                texts.extend(legaia_engine_ui::menu::decimal_field::decimal_field_draws(
                    font,
                    value,
                    (i32::from(h.x), i32::from(h.y)),
                    places,
                    false,
                    if h.marked { ink } else { unmarked },
                ));
            }
        }
        layers.push(VenueLayer {
            panel: l.panel,
            frame: l.frame,
            hands,
            texts,
        });
    }
    if let Some(ex) = world.minigames.fishing_exchange.as_ref() {
        let m = &world.minigames;
        let points = m.fishing_points;
        let owned = |row: usize| {
            ex.rows.get(row).map_or(0, |r| {
                u32::from(world.party.inventory.get(&r.item_id).copied().unwrap_or(0))
            })
        };
        let name = |row: usize| -> Vec<u8> {
            ex.rows
                .get(row)
                .map(|r| {
                    r.name
                        .clone()
                        .unwrap_or_else(|| format!("item {:#04x}", r.item_id))
                        .into_bytes()
                })
                .unwrap_or_default()
        };
        let text = m.fishing_hub_text.as_ref();
        let line = |i: usize| -> Vec<u8> { text.map(|h| h.prize[i].clone()).unwrap_or_default() };
        let dlg = &m.fishing_exchange_dialog;
        use legaia_engine_fishing::fishing::{PrizeScreen, PrizeText};
        let list = ex.list_draw(dlg.screen == PrizeScreen::List, points, |row| {
            ex.is_available(row, points, owned(row), m.fishing_prizes_purchased)
        });
        let names: Vec<Vec<u8>> = list.rows.iter().map(|r| name(r.row)).collect();
        let rows: Vec<fx::ExchangeListRow<'_>> = list
            .rows
            .iter()
            .zip(&names)
            .map(|(r, n)| fx::ExchangeListRow {
                name: n,
                name_at: (i32::from(r.name_at.0), i32::from(r.name_at.1)),
                price: (
                    r.price.0,
                    r.price.1,
                    i32::from(r.price.2),
                    i32::from(r.price.3),
                ),
                available: r.available,
            })
            .collect();
        let label = line(4);
        let (lx, ly) = list.points_label_at;
        let (pv, pn, px, py) = list.points;
        let hands: Vec<(i32, i32)> = list
            .cursor
            .map(|(x, y)| (i32::from(x), i32::from(y)))
            .into_iter()
            .collect();
        let mut texts = fx::exchange_list_draws_for(
            font,
            &rows,
            (&label, i32::from(lx), i32::from(ly)),
            (pv, pn, i32::from(px), i32::from(py)),
        );
        if !hand_sprites {
            texts.extend(cursor_text(&hands));
        }
        layers.push(VenueLayer {
            panel: Some(list.panel),
            frame: None,
            hands: if hand_sprites { hands } else { Vec::new() },
            texts,
        });
        let item = name(dlg.row);
        let pick = |t: PrizeText| -> Vec<u8> {
            match t {
                PrizeText::ItemName => item.clone(),
                PrizeText::QuantityLabel => line(0),
                PrizeText::Question => line(1),
                PrizeText::Yes => line(2),
                PrizeText::No => line(3),
            }
        };
        for d in ex.dialog_layers(dlg, points, owned(dlg.row), m.fishing_prizes_purchased) {
            let strings: Vec<(Vec<u8>, i32, i32)> = d
                .texts
                .iter()
                .map(|&(t, x, y)| (pick(t), i32::from(x), i32::from(y)))
                .collect();
            let lines: Vec<(&[u8], i32, i32)> =
                strings.iter().map(|(t, x, y)| (&t[..], *x, *y)).collect();
            let numbers: Vec<(i64, i32, i32, i32)> = d
                .numbers
                .iter()
                .map(|&(v, n, x, y)| (v, n, i32::from(x), i32::from(y)))
                .collect();
            let hands: Vec<(i32, i32)> = d
                .cursors
                .iter()
                .map(|&(x, y)| (i32::from(x), i32::from(y)))
                .collect();
            let mut texts = fx::exchange_dialog_draws_for(
                font,
                &lines,
                &numbers,
                d.slash.map(|(x, y)| (i32::from(x), i32::from(y))),
                ink,
            );
            if !hand_sprites {
                texts.extend(cursor_text(&hands));
            }
            layers.push(VenueLayer {
                panel: d.panels.first().copied(),
                frame: None,
                hands: if hand_sprites { hands } else { Vec::new() },
                texts,
            });
        }
    }
    let rects: Vec<Option<(i32, i32, i32, i32)>> = layers
        .iter()
        .map(|l| {
            l.panel
                .and_then(|(cx, y, w, h)| centred_panel(cx, y, w, h))
                .map(|p| (p.x, p.y, p.w, p.h))
                .or(l.frame)
                .map(|(x, y, w, h)| (i32::from(x), i32::from(y), i32::from(w), i32::from(h)))
        })
        .collect();
    let mut out = FishingVenueScreen::default();
    for (i, mut l) in layers.into_iter().enumerate() {
        out.chrome.extend(rects[i].map(VenueChrome::Panel));
        out.chrome
            .extend(l.hands.iter().map(|&p| VenueChrome::Hand(p)));
        for above in rects[i + 1..].iter().flatten() {
            fx::occlude_text_draws(
                &mut l.texts,
                legaia_engine_ui::dialog_window_footprint(*above),
            );
        }
        out.texts.extend(l.texts);
    }
    out
}

/// A venue screen's chrome as sprites over a `surface_w x surface_h`
/// surface, `rects` the system-UI chrome atlas: the panels in style `0x44`
/// and the hand cursors, back to front.
pub fn venue_chrome_sprites(
    chrome: &[VenueChrome],
    rects: &legaia_engine_ui::SaveMenuAtlasRects,
    surface_w: u32,
    surface_h: u32,
) -> Vec<legaia_engine_ui::SpriteDraw> {
    use legaia_engine_ui::fishing::exchange as fx;
    let (origin, scale) = legaia_engine_ui::pause_menu::stage_transform(surface_w, surface_h);
    let mut out = Vec::new();
    for c in chrome {
        match *c {
            VenueChrome::Panel(p) => out.extend(fx::venue_panel_draws_for(rects, p, origin, scale)),
            VenueChrome::Hand(p) => out.push(fx::venue_hand_draw_for(rects, p, origin, scale)),
        }
    }
    out
}

/// The venue screens' chrome this frame ([`fishing_venue_screen`] with the
/// atlas's hand), as sprites. Empty outside fishing.
pub fn fishing_venue_chrome_sprites(
    world: &World,
    font: &legaia_font::Font,
    rects: &legaia_engine_ui::SaveMenuAtlasRects,
    surface_w: u32,
    surface_h: u32,
) -> Vec<legaia_engine_ui::SpriteDraw> {
    if world.mode != legaia_engine_mode::mode::SceneMode::Fishing {
        return Vec::new();
    }
    venue_chrome_sprites(
        &fishing_venue_screen(world, font, true).chrome,
        rects,
        surface_w,
        surface_h,
    )
}
