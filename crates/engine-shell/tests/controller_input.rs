//! A controller drives the native play window exactly as the keyboard does.
//!
//! The window keeps the controller's folded word (`window::gamepad`, over the
//! shared layout `legaia_engine_system::gamepad`) apart from the keyboard's
//! held word and ORs the two into every tick. These runs drive the real
//! frame path (`HeadlessPlayWindow::controller_frame`: the poll, then the
//! redraw's ticks) and compare whole host traces:
//!
//! - the same walk held on the controller alone and on the keyboard alone
//!   leaves the same world, frame for frame;
//! - a controller button released while a key still holds the same button
//!   does not drop it (the keyboard's word is not the controller's).
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset.

use legaia_engine_session::host_trace::{self, HostTraceFrame};
use legaia_engine_shell::window::HeadlessPlayWindow;
use legaia_engine_system::input::PadButton;

const TICKS: u32 = 300;

/// Up for a stretch, then Right: the walk `host_frame_parity` uses.
fn walk(t: u32) -> u16 {
    if (20..160).contains(&t) {
        PadButton::Up.mask()
    } else if (160..285).contains(&t) {
        PadButton::Right.mask()
    } else {
        0
    }
}

/// One run of `TICKS` one-tick frames, the keyboard holding `keys(t)` and the
/// controller `pad(t)`.
fn run(disc: &str, keys: impl Fn(u32) -> u16, pad: impl Fn(u32) -> u16) -> Vec<HostTraceFrame> {
    let mut w = HeadlessPlayWindow::open(std::path::Path::new(disc), "town01").expect("open");
    let mut out = vec![host_trace::sample_with_overlay(
        w.session(),
        w.menu_runtime(),
    )];
    for t in 1..=TICKS {
        w.set_controller(pad(t));
        w.controller_frame(keys(t), 1);
        out.push(host_trace::sample_with_overlay(
            w.session(),
            w.menu_runtime(),
        ));
    }
    out
}

fn lead(tr: &[HostTraceFrame], i: usize) -> Option<(i32, i32)> {
    tr.get(i).and_then(|f| f.actors.first()).map(|a| (a.x, a.z))
}

fn first_parting(a: &[HostTraceFrame], b: &[HostTraceFrame]) -> Option<usize> {
    a.iter().zip(b).position(|(x, y)| x != y)
}

#[test]
fn a_controller_walk_matches_the_keyboard_walk() {
    let Some(disc) = std::env::var("LEGAIA_DISC_BIN")
        .ok()
        .filter(|d| std::path::Path::new(d).is_file())
    else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let keyboard = run(&disc, walk, |_| 0);
    let (a, b) = (lead(&keyboard, 0), lead(&keyboard, TICKS as usize));
    let moved = matches!((a, b), (Some(a), Some(b)) if (a.0 - b.0).abs() + (a.1 - b.1).abs() > 128);
    assert!(
        moved,
        "the keyboard walk moved nobody ({a:?} -> {b:?}): nothing to compare"
    );

    let controller = run(&disc, |_| 0, walk);
    assert_eq!(
        first_parting(&keyboard, &controller),
        None,
        "controller walk parts from the keyboard walk"
    );

    // The controller lets go of Up two thirds of the way through the
    // stretch the keyboard also holds it: the union is still the keyboard's
    // walk, so the world must not notice.
    let both = run(&disc, walk, |t| {
        if (20..110).contains(&t) {
            PadButton::Up.mask()
        } else {
            0
        }
    });
    assert_eq!(
        first_parting(&keyboard, &both),
        None,
        "a controller release dropped a button the keyboard still held"
    );
}
