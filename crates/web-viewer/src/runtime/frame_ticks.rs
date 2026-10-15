//! One display frame's sim ticks, owner by owner: the page's twin of the
//! native window's `run_sim_ticks` / `sim_tick` arms, in one export.
//!
//! Each tick goes to the first owner that takes it: the pause menu
//! (`play_menu_tick`), the naming prompt (`play_name_entry_tick`), the
//! opening chain's intro skip, the party-wipe hold, a menu-overlay screen
//! (`play_shop_tick`), and otherwise the field (`tick_frame`, then the
//! screen's after-tick step). The frame's just-pressed edges ride the first
//! tick only. The page's JavaScript used to spell this order itself, which
//! made it the one part of a frame a native test could check only through a
//! hand transcription; the JS now calls [`LegaiaRuntime::play_frame_ticks`]
//! and keeps only its own follow-up (blitters, geometry rebuilds, the quit
//! hand-back), which the report names.

use super::*;
use legaia_engine_session::{TickClaims, TickOwner};

/// What one frame's ticks did ([`LegaiaRuntime::frame_ticks`]), for the
/// page's follow-up.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FrameTicksReport {
    /// Sim ticks taken. `0` = the frame ran none and its edges are kept for
    /// the next one that does.
    pub ticks: u32,
    /// The pause menu took at least one tick (the page's `_afterMenuTicks`).
    pub menu_ticked: bool,
    /// A screen the page draws through its menu blitters came up (the pause
    /// menu opened).
    pub menu_opened: bool,
    /// The naming prompt committed a name.
    pub name_committed: bool,
    /// Every scene the frame entered, in order: the intro skip's hand-off
    /// and the doors the field walked through. The page rebuilds its scene
    /// geometry for the last.
    pub scenes: Vec<String>,
    /// The party-wipe hold returned to the title this frame (the title theme
    /// is already started).
    pub quit: bool,
}

impl LegaiaRuntime {
    /// Run `steps` sim ticks of one display frame (see the module docs).
    /// `edge` is the frame's just-pressed pad bits, `pad` the held word the
    /// field ticks on. Errors carry the failing step's message; the page
    /// treats one as an engine trap.
    pub fn frame_ticks(
        &mut self,
        steps: u32,
        edge: u16,
        pad: u16,
    ) -> Result<FrameTicksReport, String> {
        let mut r = FrameTicksReport::default();
        let mut edge = edge;
        for s in 0..steps {
            r.ticks = s + 1;
            // The pause menu once it owns the frame: one menu step per tick
            // (the native window's boot-UI arm). The SFX scheduler still
            // steps, as retail's mode-0x17 handler runs the cue drainer.
            if self.play_menu_owns_frame() {
                self.play_menu_input(edge);
                r.menu_ticked = true;
                edge = 0;
                self.play_tick_overlay_sfx();
                continue;
            }
            // The party-wipe hold: a host screen, like the native window's
            // boot UI (which holds it). It steps once per remaining tick;
            // the frame ends with it.
            if self.is_game_over() {
                for g in s..steps {
                    let picked = self.game_over_input(if g == s { edge } else { 0 });
                    if picked == "quit" {
                        // The battle score must not outlive the battle: the
                        // title card plays the title theme (the native
                        // window's post-wipe hand-off).
                        let _ = self.play_title_bgm();
                        r.quit = true;
                        break;
                    }
                }
                r.ticks = steps;
                break;
            }
            // The tick's owner, in the order both play hosts share
            // (`BootSession::claim_tick`): the naming prompt, stepped on the
            // tick's edge by the session; retail's prologue intro skip
            // (`FUN_801D1344`), whose hand-off the session has taken - the
            // frame's remaining ticks run on the new scene; the pause menu's
            // spawn hand-over and its press.
            let claims = TickClaims {
                overlay_open: self.menu.is_open(),
                overlay_suspends: self.menu.is_open() && self.menu.suspends_field(),
                pause_menu_ready: self.ensure_menu_assets(),
            };
            let owner = self
                .scene_host
                .session_mut()
                .map_or(TickOwner::Field(false), |s| s.claim_tick(claims, edge));
            match owner {
                TickOwner::NameEntry(committed) => {
                    r.name_committed |= committed;
                    edge = 0;
                    continue;
                }
                TickOwner::PrologueSkip(target) => {
                    self.enter_field_core(target, false)?;
                    r.scenes.push(target.to_string());
                    edge = 0;
                    continue;
                }
                TickOwner::PauseMenuSpawned => {
                    self.play_menu_take_spawn();
                    r.menu_ticked = true;
                    edge = 0;
                    self.play_tick_overlay_sfx();
                    continue;
                }
                TickOwner::PauseMenuOpened(scripted) => {
                    self.play_menu_attach_opened(scripted);
                    r.menu_ticked = true;
                    r.menu_opened = true;
                    edge = 0;
                    self.play_tick_overlay_sfx();
                    continue;
                }
                // A shop or the prize exchange suspends the field and takes
                // the tick: its session steps on the tick's edge, and the SFX
                // scheduler steps under it.
                TickOwner::OverlayScreen => {
                    self.play_shop_input(edge);
                    edge = 0;
                    self.play_tick_overlay_sfx();
                    continue;
                }
                TickOwner::Field(neutral) => {
                    self.set_pad(if neutral { 0 } else { pad });
                }
            }
            // The field's tick; an open inn prompt (which does not suspend
            // the field) then steps after it, on the tick's own edge.
            let tick_edge = edge;
            edge = 0;
            let entered = self.tick_frame().map_err(js_text)?;
            self.play_shop_after_tick(tick_edge);
            if !entered.is_empty() {
                r.scenes.push(entered);
            }
        }
        Ok(r)
    }
}

#[wasm_bindgen]
impl LegaiaRuntime {
    /// Run one display frame's `steps` sim ticks, each to the owner that
    /// takes it, and report what the page must follow up
    /// ([`FrameTicksReport`], as JSON). See [`Self::frame_ticks`].
    pub fn play_frame_ticks(&mut self, steps: u32, edge: u16, pad: u16) -> Result<String, JsValue> {
        let r = self
            .frame_ticks(steps, edge, pad)
            .map_err(|e| JsValue::from_str(&e))?;
        Ok(serde_json::json!({
            "ticks": r.ticks,
            "menu_ticked": r.menu_ticked,
            "menu_opened": r.menu_opened,
            "name_committed": r.name_committed,
            "scenes": r.scenes,
            "quit": r.quit,
        })
        .to_string())
    }
}

/// A `JsValue` error as text, without touching the JS runtime off wasm
/// (where every `JsValue` accessor panics).
fn js_text(e: JsValue) -> String {
    #[cfg(target_arch = "wasm32")]
    {
        e.as_string()
            .unwrap_or_else(|| "engine tick failed".to_string())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = e;
        "engine tick failed".to_string()
    }
}
