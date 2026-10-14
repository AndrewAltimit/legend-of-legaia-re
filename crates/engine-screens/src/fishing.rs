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
