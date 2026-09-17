//! Extracted from `window.rs` (mechanical split; behavior-preserving).

use super::*;

#[path = "event_handler/keyboard.rs"]
mod keyboard;
#[path = "event_handler/redraw.rs"]
mod redraw;
#[path = "event_handler/redraw_passes.rs"]
mod redraw_passes;

impl ApplicationHandler for PlayWindowApp {
    fn resumed(&mut self, evl: &ActiveEventLoop) {
        if !self.win.open(evl, "legaia-engine") {
            return;
        }
        // Opt-in PSX-faithful rendering: affine (perspective-incorrect) UV
        // warp + sub-pixel vertex jitter + 15-bit BGR555 ordered dithering on
        // the 3D mesh pipelines. Off by default (clean modern output); enable
        // with `LEGAIA_PSX_RENDER=1`.
        if std::env::var_os("LEGAIA_PSX_RENDER").is_some()
            && let Some(r) = self.win.renderer.as_ref()
        {
            r.set_psx_mode(true);
            log::info!("play-window: PSX-faithful render mode enabled");
        }
        // Opt-in dynamic-lighting enhancement (`--dynamic-lighting`, or the
        // `I` key at runtime): soft warm directional light + screen-centred
        // light pool over the baked shading. Off by default - retail has no
        // field light source, and the disabled path is pixel-identical.
        if self.dynamic_lighting
            && let Some(r) = self.win.renderer.as_ref()
        {
            r.set_dynamic_lighting(true);
            log::info!("play-window: dynamic-lighting enhancement enabled (I toggles)");
        }
        // Shadow sub-toggle (`--no-dyn-shadows` / the `Y` key): the derived
        // per-scene point lights + their PCF shadow maps. Inert while
        // dynamic lighting is off, so this is safe to stage unconditionally.
        // Camera-occlusion fade (`--no-occlusion-fade` / the `F4` key):
        // default-on see-through walls; inert until the field redraw pass
        // stages a player focus, so staging the toggle here is safe too.
        if let Some(r) = self.win.renderer.as_ref() {
            r.set_dyn_shadows(self.dyn_shadows);
            r.set_occlusion_fade(self.occlusion_fade);
        }
        self.upload_assets();
        self.win.request_redraw();
    }

    fn window_event(&mut self, evl: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                // Flush any pending record log before exiting so an Escape /
                // window-close mid-session produces a usable replay file.
                if let Some(log) = self.record_log.as_mut()
                    && let Err(e) = log.flush()
                {
                    log::error!("record: flush on CloseRequested failed: {e:#}");
                }
                evl.exit();
            }
            WindowEvent::Resized(size) => self.win.handle_resize(size.width, size.height),
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: PhysicalKey::Code(code),
                        state,
                        ..
                    },
                ..
            } => {
                self.handle_keyboard(evl, code, state);
            }
            // Left-mouse drag-orbit. Horizontal drag rotates the field camera
            // around the player (`Camera::manual_orbit`); the movement
            // compass reads the same field, so the d-pad remap tracks the
            // orbited view (see `field_follow_camera_mvp`). Field free-roam
            // only - world map / battle / menus keep their own cameras.
            //
            // Vertical drag pitches the `F3` debug orbit vantage, which is
            // the one camera this window owns. Both rates and both clamps are
            // the browser play page's (`window::camera::debug_orbit`), so a
            // given gesture moves the camera by the same amount on either
            // host - the yaw rate used to differ by a third, on a field the
            // *simulation* reads.
            WindowEvent::CursorMoved { position, .. } => {
                if let Some(last) = self.orbit_drag_last_x {
                    let dx = (position.x - last) as f32;
                    if dx != 0.0
                        && !self.boot_ui.is_active()
                        && self.session.host.world.mode == SceneMode::Field
                    {
                        self.session.camera.manual_orbit = (self.session.camera.manual_orbit
                            + dx * camera::debug_orbit::YAW_RAD_PER_PX)
                            .rem_euclid(std::f32::consts::TAU);
                    }
                    self.orbit_drag_last_x = Some(position.x);
                }
                if let Some(last) = self.orbit_drag_last_y {
                    let dy = (position.y - last) as f32;
                    if dy != 0.0 && !self.boot_ui.is_active() {
                        self.debug_orbit_pitch = (self.debug_orbit_pitch
                            + dy * camera::debug_orbit::PITCH_RAD_PER_PX)
                            .clamp(
                                camera::debug_orbit::PITCH_MIN,
                                camera::debug_orbit::PITCH_MAX,
                            );
                    }
                    self.orbit_drag_last_y = Some(position.y);
                }
                self.cursor_x = position.x;
                self.cursor_y = position.y;
            }
            WindowEvent::MouseInput {
                state,
                button: winit::event::MouseButton::Left,
                ..
            } => {
                let held = state == ElementState::Pressed;
                self.orbit_drag_last_x = held.then_some(self.cursor_x);
                self.orbit_drag_last_y = held.then_some(self.cursor_y);
            }
            // Wheel: the debug orbit's continuous zoom, the page's `halfWidth`
            // knob. A notch out widens the framing box and a notch in narrows
            // it, by the page's own factors; the `T` distance preset stays a
            // separate coarse multiplier under it, as it is on both hosts.
            WindowEvent::MouseWheel { delta, .. } => {
                let notches = match delta {
                    winit::event::MouseScrollDelta::LineDelta(_, y) => y,
                    winit::event::MouseScrollDelta::PixelDelta(p) => (p.y as f32) / 120.0,
                };
                if notches != 0.0 && !self.boot_ui.is_active() {
                    let f = if notches < 0.0 {
                        camera::debug_orbit::ZOOM_OUT
                    } else {
                        camera::debug_orbit::ZOOM_IN
                    };
                    self.debug_orbit_zoom = (self.debug_orbit_zoom * f).clamp(0.05, 20.0);
                }
            }
            WindowEvent::RedrawRequested => {
                self.handle_redraw();
            }
            _ => {}
        }
    }
}
