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
    // One derivation for the catch HUD (`PondSession::catch_hud`). The cast
    // line-projection term `DAT_801d9178` has no engine analogue and stays
    // zero.
    let c = s.catch_hud();
    if c.visible {
        items.extend(legaia_engine_ui::catch_hud_draws(
            &legaia_engine_ui::CatchHudState {
                record: c.record,
                line_extent: 0,
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
    items
}

/// The fishing rod and line as screen primitives for this frame: the rod
/// model the rod actor posed (`PondSession::rod_faces`, wrapped by the shared
/// `fishing::rod` builder), then the session's line
/// (`legaia_engine_minigame_scenes::fishing_venue::fishing_line_frame`, the
/// fish end projected through the camera the pond draws with: the venue
/// camera, else the host's follow camera framed on `scene_center_xz`),
/// wrapped by the shared `fishing::line` builder. Empty outside the fishing
/// mode, and with no rod and no line out.
pub fn fishing_line_screen_prims(
    world: &mut World,
    camera: &legaia_engine_field::camera::Camera,
    scene_center_xz: [f32; 2],
) -> Vec<legaia_render_kernels::screen_prim::ScreenPrim> {
    if world.mode != legaia_engine_mode::mode::SceneMode::Fishing {
        return Vec::new();
    }
    let view =
        legaia_engine_minigame_scenes::fishing_venue::venue_view(&world.minigames).or_else(|| {
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
        )
        .and_then(|l| {
            legaia_engine_ui::fishing::line::fishing_line_prim(
                l.fish, l.rod, l.fish_rgb, l.rod_rgb, l.ot,
            )
        }),
    );
    prims
}

/// The open point-exchange sub-screen's draws in 320x240 **stage** pixels,
/// through `legaia_engine_ui::fishing::exchange` - the row layout, the ink
/// rule and the one-time tag decided once for both play hosts. Empty with no
/// exchange open.
///
/// The pen is the venue sub-screen's panel frame (`FUN_801d74b0`): the retail
/// menu-picker rect, centre-x converted to a left edge with the two-left /
/// six-down skin bias, swaying on the overlay's idle sway triple
/// (`FUN_801d03b0`), which the shared venue step advances on the world. Each
/// row carries the live bag count of its prize. `legend` is the host's own
/// key line; `ink` / `dim` are its pens for live and unavailable rows.
pub fn fishing_exchange_draws(
    world: &World,
    font: &legaia_font::Font,
    legend: &str,
    ink: [f32; 4],
    dim: [f32; 4],
) -> Vec<legaia_engine_ui::TextDraw> {
    use legaia_engine_ui::fishing::exchange as fx;
    let Some(ex) = world.minigames.fishing_exchange.as_ref() else {
        return Vec::new();
    };
    let sway = world.minigames.fishing_venue.sway_offset;
    let pen = legaia_engine_fishing::fishing_chrome::centred_panel(0xA0, 0x50, 0x68, 0x50)
        .map(|p| (p.x as i32 + sway.0 as i32, p.y as i32 + sway.1 as i32))
        .unwrap_or((8, 98));
    let names: Vec<String> = ex
        .rows
        .iter()
        .map(|r| {
            r.name
                .clone()
                .unwrap_or_else(|| format!("item {:#04x}", r.item_id))
        })
        .collect();
    let rows: Vec<fx::ExchangeRowView<'_>> = ex
        .rows
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let owned = *world.party.inventory.get(&r.item_id).unwrap_or(&0) as u32;
            fx::ExchangeRowView {
                name: names[i].as_str(),
                price: r.price,
                owned,
                available: ex.is_available(
                    i,
                    world.minigames.fishing_points,
                    owned,
                    world.minigames.fishing_prizes_purchased,
                ),
                one_time: r.is_one_time(),
                latched: ex.is_latched(i, world.minigames.fishing_prizes_purchased),
            }
        })
        .collect();
    let view = fx::ExchangeView {
        venue: ex.venue as u8,
        points: world.minigames.fishing_points,
        cursor: ex.cursor,
        first_visible: ex.first_visible(world.minigames.fishing_points),
        rows: &rows,
    };
    fx::exchange_screen_draws_for(font, &view, legend, pen, ink, dim)
}
