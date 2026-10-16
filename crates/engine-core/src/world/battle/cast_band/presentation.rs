//! The cast modules' **presentation calls**, fired from one generic hook.
//!
//! Every slot-B module arm reaches the same handful of shared helpers - the
//! screen-fade spawn `FUN_80024E80`, the armed wash `FUN_8004695C`, CLUT
//! copies through `MoveImage`, and the stores that arm PROT 0898's colour
//! wash `FUN_801F2410`. Their arguments are literals the arm computes, so
//! they are decoded off the module image as data
//! ([`legaia_overlay_images::cast_presentation`], carried on the cast-effect
//! pool) and fired here, after the module's tick, on the arm that made them.
//! No tick body is edited per site.
//!
//! ## When a site fires
//!
//! The decoder places each site on its phase arm and reads off the arm's
//! control flow whether it runs on the pass that moves the phase
//! ([`SiteTiming::Pass`]), on the passes that hold ([`SiteTiming::Hold`]), or
//! on every pass ([`SiteTiming::Every`]). The engine sees an arm's pass as
//! the module phase moving off `phase_in` (or the body finishing); the arm
//! ran on every tick the module was ticked on it.
//!
//! A call that leaves something behind - a fade actor, a VRAM copy, a
//! parameter store - fires **once**: on the passing tick for a `Pass` site,
//! on the arm's first tick otherwise (a `Hold` / `Every` fade spawned on
//! every frame would stack a fresh fade actor per frame, which the arm's
//! own guards prevent in retail). A per-frame call - the armed wash, which
//! is one-shot per arm (`gp+0x9D4` cleared by its drain), and the alpha
//! ramps - fires on every tick its timing covers.
//!
//! ## Only where the port paces the arm
//!
//! A call is placed in time by its arm, so it is only as right as the
//! port's pacing of that arm. Many tick ports walk their arms one per tick
//! where retail holds each on a countdown or a poll (PROT 0913's body runs
//! its 21 arms in 21 ticks; retail holds its arm 2 through the band's whole
//! flash-out), and firing their fades there would stack seconds of ramps
//! into a few frames. So on an arm the decoder marks as able to hold
//! ([`legaia_overlay_images::cast_presentation::PhaseDispatch::holding`]),
//! the hook fires only once the engine has held it at least one tick: its
//! `Every` / `Hold` one-shots on the first holding tick, its `Pass` sites on
//! the pass after a hold. An arm the engine passes on its first tick is
//! counted as unpaced and its calls are skipped. An arm that cannot hold
//! (no gate, or no phase write) fires as it runs.
//!
//! The colour wash itself is the dispatchers' tail: once the module's writes
//! for the tick are in, an armed block (`alpha != 0`) steps its scroll and
//! draws ([`legaia_render_kernels::cast_colour_wash`]).
//!
//! ## What is not fired here
//!
//! - Sites the decoder could not place (spawn-stager arms) or resolve.
//! - The streak ribbon `FUN_801E1D98`: its anchor is a stack trio the arm
//!   computes from seats and the camera, which the data decode does not fold.
//! - The party CLUT reload (`LoadImage` of a 240x1 row out of a RAM copy).
//! - Modules whose fades are already carried by a hand port: PROT 0905
//!   (Vera's director), PROT 0954 (ported whole), PROT 0966 (its one fade
//!   seat); and PROT 0903's CLUT copy (Gimard's director).
//!
//! REF: FUN_80024E80, FUN_8004695C, FUN_80058490, FUN_801F2410, FUN_801E1D98

use super::*;
use legaia_engine_mode::mode::SceneMode;
use legaia_overlay_images::cast_presentation::{
    HandleSlot, PresentationCall, PresentationSite, SiteTiming, WashStore,
};

/// Modules whose fades a hand port already spawns.
const FADES_PORTED_BY_HAND: [u32; 3] = [905, 954, 966];
/// Modules whose CLUT copy a hand port already issues.
const VRAM_MOVES_PORTED_BY_HAND: [u32; 1] = [903];

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
    pub(crate) entered: Option<(u32, u8)>,
    /// The colour-wash block `ctx[+0x27A..]`.
    pub wash: legaia_render_kernels::cast_colour_wash::CastColourWash,
    /// The module's last tick ran the dispatchers' tail with the wash armed.
    pub(crate) wash_live: bool,
    /// The armed wash (`FUN_8004695C`) the module's last tick armed.
    pub(crate) armed_wash: Option<u32>,
    /// The fades the hook spawned, live until they land or an arm kills them
    /// (`World::screen_fade_draws` composites them beside the band's).
    pub fades: Vec<HookFade>,
    /// Ticks the current arm has held (run without moving the phase).
    pub(crate) arm_held: u32,
    /// Arms passed on their first tick although they can hold, this cast -
    /// calls the hook skipped because the port does not pace them.
    pub unpaced_arms: u32,
}

impl World {
    /// Fire the presentation calls of module `entry`'s arm `phase_in` for
    /// the tick that just ran. `body` is the trampoline-selected tick body
    /// (`None` for a module with one phase switch), `phase_out` the phase the
    /// tick left, `finished` whether the body returned done. `stepped` is
    /// the arm a state-touching stager passed ahead of the body on the same
    /// tick (into `phase_in`), whose calls fire first.
    pub(crate) fn fire_cast_presentation(
        &mut self,
        entry: u32,
        body: Option<u32>,
        stepped: Option<u8>,
        phase_in: u8,
        phase_out: u8,
        finished: bool,
    ) {
        self.casting.module_pres.armed_wash = None;
        if let Some(pres) = self
            .casting
            .effect_pool
            .as_ref()
            .and_then(|p| p.module(entry))
            .map(|m| Arc::clone(&m.presentation))
        {
            if let Some(arm) = stepped {
                self.fire_arm_calls(&pres, entry, body, arm, phase_in, false);
            }
            self.fire_arm_calls(&pres, entry, body, phase_in, phase_out, finished);
        }
        self.finish_cast_wash_tick();
    }

    /// One arm's calls for this tick ([`Self::fire_cast_presentation`]).
    fn fire_arm_calls(
        &mut self,
        pres: &legaia_overlay_images::cast_presentation::CastPresentation,
        entry: u32,
        body: Option<u32>,
        phase_in: u8,
        phase_out: u8,
        finished: bool,
    ) {
        let key = (body.unwrap_or(0), phase_in);
        let first = self.casting.module_pres.entered != Some(key);
        self.casting.module_pres.entered = Some(key);
        if first {
            self.casting.module_pres.arm_held = 0;
        }
        let passed = phase_out != phase_in || finished;
        // Pacing: a holdable arm fires only once the engine has held it.
        let holds = pres.arm_holds(body, phase_in);
        let held = self.casting.module_pres.arm_held;
        // A module whose countdown gates the engine ports
        // (`cast_module_camera::summon_holds`) holds exactly as long as
        // retail, so an arm it passes on its first tick is one retail runs
        // in a single pass too (PROT 0918's ungated arm 17, whose kill of
        // arm 15's held white fade would otherwise never fire).
        let engine_paced =
            legaia_engine_battle_vm::cast_module_camera::summon_holds::summon_holds(entry)
                .is_some();
        let paced = !holds || held > 0 || engine_paced;
        let first_hold = holds && !passed && held == 0;
        let first_pass = engine_paced && first && passed;
        if holds && passed && held == 0 {
            self.casting.module_pres.unpaced_arms += 1;
        }
        if !passed {
            self.casting.module_pres.arm_held += 1;
        }
        let sites: Vec<PresentationSite> = pres.arm_sites(body, phase_in).copied().collect();
        // `LEGAIA_DIAG_CAST_PRES=1`: one line per module tick that has
        // sites, for lining an engine frame up against a retail state.
        static DIAG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        if *DIAG.get_or_init(|| std::env::var_os("LEGAIA_DIAG_CAST_PRES").is_some())
            && (first || passed)
        {
            eprintln!(
                "cast-pres: PROT {entry} body {:08X} arm {phase_in} -> {phase_out} first={first} \
                 holds={holds} held={held} sites={}",
                body.unwrap_or(0),
                sites.len()
            );
        }
        // The fade sites that spawned on this tick: a `FadeStore` parks the
        // fade its own call just made, whatever timing the decoder gave the
        // store. A store in the delay slot of the arm's exit jump (PROT 0922
        // arm 11, `0x801F746C`) reads as `Every`, which on a held arm fires on
        // the first hold - before the pass that spawns its fade - and left
        // that held fade unkillable.
        let mut spawned_now: Vec<u32> = Vec::new();
        for s in &sites {
            // Past a ported countdown gate a site runs on the pass alone
            // (`summon_holds::gate_va`), whatever timing the decoder read.
            let timing = match legaia_engine_battle_vm::cast_module_camera::summon_holds::gate_va(
                entry, phase_in,
            ) {
                Some(gate) if s.va > gate => SiteTiming::Pass,
                _ => s.timing,
            };
            let once = match timing {
                SiteTiming::Pass => passed && paced,
                SiteTiming::Every if holds => first_hold || first_pass,
                SiteTiming::Hold if holds => first_hold,
                SiteTiming::Hold | SiteTiming::Every => first,
            };
            let each = match timing {
                SiteTiming::Pass => passed && paced,
                SiteTiming::Hold => !passed,
                SiteTiming::Every => paced || !passed,
            };
            match s.call {
                PresentationCall::Fade(f) if once && !FADES_PORTED_BY_HAND.contains(&entry) => {
                    if let Some(r) = f.resolved() {
                        let t = legaia_engine_system::fade::FadeTemplate {
                            kind: r.kind,
                            duration: r.duration,
                            start_rgb: r.start_rgb,
                            end_rgb: r.end_rgb,
                            mode: [r.delay, r.hold, 0],
                        };
                        let mut seat = None;
                        legaia_engine_system::fade::spawn_fade(&mut seat, &t, r.id);
                        if let Some(state) = seat {
                            spawned_now.push(s.va);
                            self.casting.module_pres.fades.push(HookFade {
                                site: s.va,
                                slot: None,
                                state,
                            });
                        }
                    }
                }
                // The arm parks the fade it just spawned (`sw v0, slot`).
                PresentationCall::FadeStore { fade, slot }
                    if once || spawned_now.contains(&fade) =>
                {
                    let fades = &mut self.casting.module_pres.fades;
                    for f in fades.iter_mut().filter(|f| f.slot == Some(slot)) {
                        f.slot = None;
                    }
                    if let Some(f) = fades.iter_mut().rev().find(|f| f.site == fade) {
                        f.slot = Some(slot);
                    }
                }
                // `ori 0x8` into the parked actor's flag word: the actor list
                // drops it.
                PresentationCall::ActorKill { slot } if once => {
                    self.casting
                        .module_pres
                        .fades
                        .retain(|f| f.slot != Some(slot));
                }
                PresentationCall::VramMove { rect, dst }
                    if once && !VRAM_MOVES_PORTED_BY_HAND.contains(&entry) =>
                {
                    if let ([Some(x), Some(y), Some(w), Some(h)], [Some(dx), Some(dy)]) =
                        (rect, dst)
                    {
                        self.battle.vram_moves.push(crate::world::ScriptVramMove {
                            src: (x, y),
                            size: (w, h),
                            dst: (dx, dy),
                        });
                    }
                }
                PresentationCall::ArmedWash { rgb: Some(rgb) } if each => {
                    self.casting.module_pres.armed_wash = Some(rgb);
                }
                PresentationCall::WashField {
                    field,
                    store: Some(WashStore::Set(v)),
                    ..
                } if once => {
                    self.casting.module_pres.wash.store(field, v);
                }
                PresentationCall::WashField {
                    field,
                    store: Some(WashStore::Add(d)),
                    ..
                } if each => {
                    let bound = sites.iter().find_map(|c| match c.call {
                        PresentationCall::WashField {
                            field: f,
                            store: Some(WashStore::Clamp(b)),
                            ..
                        } if f == field => Some(b),
                        _ => None,
                    });
                    let w = &mut self.casting.module_pres.wash;
                    let v = w.load(field);
                    w.store(field, wash_add(field, v, d, bound));
                }
                _ => {}
            }
        }
    }

    /// Step the hook's fades one vsync. A fade held at its end colour (hold
    /// `-1`) lives until an arm kills it; one the module never killed is
    /// dropped once no cast module is resident, so a band that ends on a
    /// held tint does not leave it over the rest of the battle.
    pub(crate) fn step_cast_hook_fades(&mut self) {
        let resident = self.casting.summon_stager.is_some() || self.casting.capture_spell.is_some();
        self.casting
            .module_pres
            .fades
            .retain_mut(|f| f.state.step() || (resident && f.state.holds_at_end()));
    }

    /// The dispatchers' tail: an armed colour wash steps its scroll and
    /// draws on this tick.
    fn finish_cast_wash_tick(&mut self) {
        let w = &mut self.casting.module_pres.wash;
        self.casting.module_pres.wash_live = w.armed();
        if w.armed() {
            w.step_scroll(1);
        }
    }

    /// The cast modules' full-screen washes for this frame, as screen
    /// primitives both hosts composite under the text layer: PROT 0898's
    /// colour wash (behind the scene, farthest bucket) and the armed wash
    /// (`FUN_8004695C`, subtractive, farthest bucket). Empty outside a
    /// resident cast module.
    pub fn cast_wash_prims(&self) -> Vec<legaia_render_kernels::screen_prim::ScreenPrim> {
        let resident = self.casting.summon_stager.is_some() || self.casting.capture_spell.is_some();
        if self.mode != SceneMode::Battle || !resident {
            return Vec::new();
        }
        let p = &self.casting.module_pres;
        let mut out = Vec::new();
        if let Some(rgb) = p.armed_wash {
            out.push(legaia_render_kernels::battle_intro::wash_prim(rgb));
        }
        if p.wash_live {
            out.extend(legaia_render_kernels::cast_colour_wash::band_prims(&p.wash));
        }
        out
    }
}

/// `ctx[field] += d` as the modules' ramps store it: a byte that wraps,
/// bounded by the literal the arm's bound test stores (`if (a > 0x80) a =
/// 0x80` on the way up, `if (a < step) a = 0` on the way down).
fn wash_add(field: u16, v: i32, d: i32, bound: Option<i32>) -> i32 {
    if field == 0x32A || field == 0x32C {
        return v + d;
    }
    if d < 0 {
        match bound {
            Some(b) if v < -d => b,
            _ => (v + d) & 0xFF,
        }
    } else {
        let n = (v + d) & 0xFF;
        match bound {
            Some(b) if n > b => b,
            _ => n,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::wash_add;

    #[test]
    fn ramps_hold_at_their_bound() {
        assert_eq!(wash_add(0x27A, 0x7F, 1, Some(0x80)), 0x80);
        assert_eq!(wash_add(0x27A, 0x80, 1, Some(0x80)), 0x80);
        assert_eq!(wash_add(0x27A, 1, -2, Some(0)), 0);
        assert_eq!(wash_add(0x27A, 5, -1, Some(0)), 4);
        assert_eq!(wash_add(0x286, 0, -1, None), 0xFF);
    }
}
