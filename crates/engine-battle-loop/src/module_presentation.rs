//! The slot-B presentation hook's per-cast state (`engine-core`'s
//! `World::run_module_presentation` steps it).

use crate::cast_fx_state::CastFxState;
use legaia_overlay_images::cast_presentation::HandleSlot;

/// One fade a module arm spawned through the hook: the fade actor, the call
/// site that spawned it, and the slot the arm parked its pointer in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HookFade {
    pub site: u32,
    pub slot: Option<HandleSlot>,
    pub state: legaia_engine_system::fade::FadeState,
}

/// The presentation hook's per-cast state.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ModulePresentation {
    /// The `(body, arm)` whose once-per-arm calls have fired.
    pub entered: Option<(u32, u8)>,
    /// The colour-wash block `ctx[+0x27A..]`.
    pub wash: legaia_render_kernels::cast_colour_wash::CastColourWash,
    /// The module's last tick ran the dispatchers' tail with the wash armed.
    pub wash_live: bool,
    /// The armed wash (`FUN_8004695C`) the module's last tick armed.
    pub armed_wash: Option<u32>,
    /// The fades the hook spawned, live until they land or an arm kills them
    /// (`World::screen_fade_draws` composites them beside the band's).
    pub fades: Vec<HookFade>,
    /// Ticks the current arm has held (run without moving the phase).
    pub arm_held: u32,
    /// The band's flash-out no longer sits in `ctx[+0x102C]`: a module arm
    /// killed it there or parked a fade of its own over it.
    pub band_fade_unparked: bool,
    /// Arms passed on their first tick although they can hold, this cast -
    /// calls the hook skipped because the port does not pace them.
    pub unpaced_arms: u32,
    /// The handle slots an arm parked a seated part in, by the spawn record
    /// it was seated off (`PresentationCall::PartStore`): a later kill of
    /// the slot halts the parts of that record.
    pub part_slots: Vec<(HandleSlot, u32)>,
    /// The module word the engine carries, `(VA, value)`, as the tick that
    /// just ran opened on it: what a site's
    /// [`legaia_overlay_images::cast_presentation::WordGuard`] reads.
    pub word_in: Option<(u32, i32)>,
}

impl CastFxState {
    /// Park a part a module seated outside its tick body (a stager arm's
    /// seat) in handle slot `slot`, as a tick-body `PartStore` would, so the
    /// module's later kill of the slot halts it.
    /// Halt the part a module parked in handle slot `slot`, as a tick-body
    /// `ActorKill` of the slot does: a stager arm's halt (PROT 0923's arm 1).
    pub fn kill_parked_module_part(
        &mut self,
        slot: legaia_overlay_images::cast_presentation::HandleSlot,
    ) {
        let mut killed = Vec::new();
        self.module_pres.part_slots.retain(|&(s, rec)| {
            if s == slot {
                killed.push(rec);
            }
            s != slot
        });
        if let Some(scene) = self.active_summon.as_mut() {
            for part in &mut scene.parts {
                if part.tag.is_some_and(|t| killed.contains(&t)) {
                    part.finished = true;
                }
            }
        }
    }

    pub fn park_module_part(
        &mut self,
        slot: legaia_overlay_images::cast_presentation::HandleSlot,
        record: u32,
    ) {
        let ps = &mut self.module_pres.part_slots;
        ps.retain(|&(s, _)| s != slot);
        ps.push((slot, record));
    }

    /// Step the hook's fades one vsync. A fade held at its end colour (hold
    /// `-1`) lives until an arm kills it; one the module never killed is
    /// dropped once no cast module is resident, so a band that ends on a
    /// held tint does not leave it over the rest of the battle.
    pub fn step_cast_hook_fades(&mut self, vsyncs: u8) {
        let resident = self.summon_stager.is_some() || self.capture_spell.is_some();
        self.module_pres
            .fades
            .retain_mut(|f| f.state.step_vsyncs(vsyncs) || (resident && f.state.holds_at_end()));
    }

    /// The dispatchers' tail: an armed colour wash steps its scroll and
    /// draws on this tick.
    pub fn finish_cast_wash_tick(&mut self) {
        let w = &mut self.module_pres.wash;
        self.module_pres.wash_live = w.armed();
        if w.armed() {
            w.step_scroll(1);
        }
    }
}
