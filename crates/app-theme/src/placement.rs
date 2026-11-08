//! Where a desktop window opens: the position it had when it last closed, if
//! that still lands on a connected monitor, else wherever the desktop puts a
//! new window. Explorer, the game launcher and the play window all use it.
//!
//! The position is kept in physical pixels (the desktop's own coordinates,
//! which span every monitor). Monitors come from the windowing system when
//! the event loop starts (each program reads them just before it creates its
//! window), so a rectangle saved on a monitor that has since been unplugged
//! is recognised and dropped rather than opening the window off screen.

/// One connected monitor, in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Monitor {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    /// Physical pixels per logical point.
    pub scale: f64,
}

impl Monitor {
    fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x
            && y >= self.y
            && (x - self.x) < self.width as i32
            && (y - self.y) < self.height as i32
    }
}

/// How much of the window's top-left corner must sit on one monitor for
/// the saved position to count as reachable: enough of the title bar to
/// grab and drag.
const GRAB: (i32, i32) = (120, 24);

/// Where to put the window (`ViewportBuilder::with_position`, logical points
/// before the UI zoom), given the saved outer position in physical pixels,
/// the UI zoom and the connected monitors. `None` leaves the placement to
/// the centring default: nothing saved, no monitor known, or the saved
/// corner on no monitor.
pub fn restore(saved_px: Option<[i32; 2]>, zoom: f32, monitors: &[Monitor]) -> Option<[f32; 2]> {
    let [x, y] = saved_px?;
    let on = monitor_under(x, y, monitors)?;
    let per_point = on.scale * f64::from(zoom.max(0.1));
    Some([
        (f64::from(x) / per_point) as f32,
        (f64::from(y) / per_point) as f32,
    ])
}

/// The saved outer position in physical pixels, kept only when it still
/// lands on a connected monitor ([`restore`]'s test): what a winit window
/// takes as a `PhysicalPosition`.
pub fn restore_physical(saved_px: Option<[i32; 2]>, monitors: &[Monitor]) -> Option<[i32; 2]> {
    let [x, y] = saved_px?;
    monitor_under(x, y, monitors).map(|_| [x, y])
}

/// The monitor that holds enough of a window whose outer corner is `x, y`
/// to grab its title bar.
fn monitor_under(x: i32, y: i32, monitors: &[Monitor]) -> Option<&Monitor> {
    monitors
        .iter()
        .find(|m| m.contains(x, y) && m.contains(x + GRAB.0, y + GRAB.1))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEFT: Monitor = Monitor {
        x: 0,
        y: 0,
        width: 1920,
        height: 1080,
        scale: 1.0,
    };
    const RIGHT: Monitor = Monitor {
        x: 1920,
        y: 0,
        width: 2560,
        height: 1440,
        scale: 2.0,
    };

    #[test]
    fn a_position_on_a_monitor_is_kept() {
        assert_eq!(
            restore(Some([100, 50]), 1.0, &[LEFT, RIGHT]),
            Some([100.0, 50.0])
        );
        // On a 2x monitor the builder takes logical points.
        assert_eq!(
            restore(Some([2000, 100]), 1.0, &[LEFT, RIGHT]),
            Some([1000.0, 50.0])
        );
        // The UI zoom scales points too.
        assert_eq!(restore(Some([200, 100]), 2.0, &[LEFT]), Some([100.0, 50.0]));
    }

    #[test]
    fn a_position_off_every_monitor_is_dropped() {
        // The right monitor was unplugged.
        assert_eq!(restore(Some([2000, 100]), 1.0, &[LEFT]), None);
        // Only a sliver of the title bar would be on screen.
        assert_eq!(restore(Some([1900, 100]), 1.0, &[LEFT]), None);
        assert_eq!(restore(Some([-500, 10]), 1.0, &[LEFT]), None);
        assert_eq!(restore(Some([10, 10]), 1.0, &[]), None);
        assert_eq!(restore(None, 1.0, &[LEFT]), None);
    }

    #[test]
    fn the_physical_position_follows_the_same_test() {
        assert_eq!(
            restore_physical(Some([2000, 100]), &[LEFT, RIGHT]),
            Some([2000, 100])
        );
        assert_eq!(restore_physical(Some([2000, 100]), &[LEFT]), None);
        assert_eq!(restore_physical(None, &[LEFT]), None);
    }
}
