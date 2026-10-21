//! One display frame's sim ticks, owner by owner: the page's twin of the
//! native window's `run_sim_ticks` / `sim_tick`, in one export.
//!
//! Each tick goes to the owner `BootSession::claim_tick` names - the one
//! order both play hosts share (a host screen, the naming prompt, the intro
//! skip, the pause menu's spawn and press, a menu-overlay screen, the
//! field) - and the page acts only on that answer. The frame's just-pressed
//! edges ride the first tick only. The page's JavaScript calls
//! [`LegaiaRuntime::play_frame_ticks`] and keeps only its own follow-up
//! (blitters, geometry rebuilds, the quit hand-back), which the report names.

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
    /// The lead's rig was rebuilt during the frame
    /// (`World::take_player_rig_change`: `cave01`'s entry script swapping
    /// the party to Noa, a `CC F8 50` re-stage). The page uploads the player's geometry once at scene
    /// build and only re-poses it per frame, so a mid-scene rig swap must
    /// re-upload indices, UVs and texture words too: re-posing the old
    /// mesh's index buffer with the new mesh's vertices drew Noa's naming
    /// prompt over a scrambled Vahn. The native window re-uploads in
    /// `rebind_live_npc_models` on the same signal. A frame that also
    /// entered a scene is covered by the scene rebuild.
    pub player_rig_rebuilt: bool,
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
        let rig_gen = self.player_rig_gen;
        for s in 0..steps {
            r.ticks = s + 1;
            // The tick's owner, in the order both play hosts share
            // (`BootSession::claim_tick`): a host-held screen; the naming
            // prompt, stepped on the tick's edge by the session; retail's
            // prologue intro skip (`FUN_801D1344`), whose hand-off the
            // session has taken - the frame's remaining ticks run on the new
            // scene; the pause menu's spawn hand-over and its press; a
            // menu-overlay screen that suspends the field; the field.
            let claims = TickClaims {
                host_screen: self.play_menu_owns_frame() || self.is_game_over(),
                overlay_open: self.menu.is_open(),
                overlay_suspends: self.menu.is_open() && self.menu.suspends_field(),
                pause_menu_ready: self.ensure_menu_assets(),
            };
            let owner = self
                .scene_host
                .session_mut()
                .map_or(TickOwner::Field(false), |s| s.claim_tick(claims, edge));
            match owner {
                // A host-held screen: the pause menu once it owns the frame
                // (one menu step per tick, with the SFX scheduler stepping
                // under it, as retail's mode-0x17 handler runs the cue
                // drainer), or the party-wipe hold, which steps once per
                // remaining tick and ends the frame.
                TickOwner::HostScreen => {
                    if self.play_menu_owns_frame() {
                        self.play_menu_input(edge);
                        r.menu_ticked = true;
                        edge = 0;
                        self.play_tick_overlay_sfx();
                        self.tick_field_party_hud();
                        continue;
                    }
                    for g in s..steps {
                        self.tick_field_party_hud();
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
                TickOwner::NameEntry(committed) => {
                    r.name_committed |= committed;
                    edge = 0;
                    self.tick_field_party_hud();
                    continue;
                }
                TickOwner::PrologueSkip(target) => {
                    self.enter_field_core(target, false)?;
                    r.scenes.push(target.to_string());
                    edge = 0;
                    self.tick_field_party_hud();
                    continue;
                }
                TickOwner::PauseMenuSpawned => {
                    self.play_menu_take_spawn();
                    r.menu_ticked = true;
                    edge = 0;
                    self.play_tick_overlay_sfx();
                    self.tick_field_party_hud();
                    continue;
                }
                TickOwner::PauseMenuOpened(scripted) => {
                    self.play_menu_attach_opened(scripted);
                    r.menu_ticked = true;
                    r.menu_opened = true;
                    edge = 0;
                    self.play_tick_overlay_sfx();
                    self.tick_field_party_hud();
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
        r.player_rig_rebuilt = self.player_rig_gen != rig_gen;
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
            "player_rig_rebuilt": r.player_rig_rebuilt,
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
