//! Extracted from `window.rs` (mechanical split; behavior-preserving).

use super::*;

#[path = "event_handler/keyboard.rs"]
mod keyboard;
#[path = "event_handler/redraw.rs"]
mod redraw;

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
            WindowEvent::RedrawRequested => {
                self.handle_redraw();
            }
            _ => {}
        }
    }
}
