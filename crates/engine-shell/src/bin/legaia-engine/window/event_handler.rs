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
            // Left-mouse drag-orbit: horizontal drag rotates the field
            // camera around the player (`Camera::manual_orbit`). The
            // movement compass reads the same field, so the d-pad remap
            // tracks the orbited view (see `field_follow_camera_mvp`).
            // Field free-roam only - world map / battle / menus keep their
            // own cameras.
            WindowEvent::CursorMoved { position, .. } => {
                if let Some(last) = self.orbit_drag_last_x {
                    let dx = (position.x - last) as f32;
                    if dx != 0.0
                        && !self.boot_ui.is_active()
                        && self.session.host.world.mode == SceneMode::Field
                    {
                        const ORBIT_RAD_PER_PX: f32 = 0.008;
                        self.session.camera.manual_orbit = (self.session.camera.manual_orbit
                            + dx * ORBIT_RAD_PER_PX)
                            .rem_euclid(std::f32::consts::TAU);
                    }
                    self.orbit_drag_last_x = Some(position.x);
                }
                self.cursor_x = position.x;
            }
            WindowEvent::MouseInput {
                state,
                button: winit::event::MouseButton::Left,
                ..
            } => {
                self.orbit_drag_last_x = (state == ElementState::Pressed).then_some(self.cursor_x);
            }
            WindowEvent::RedrawRequested => {
                self.handle_redraw();
            }
            _ => {}
        }
    }
}
