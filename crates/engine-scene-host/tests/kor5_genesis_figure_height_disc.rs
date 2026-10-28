//! Disc-gated: `kor5`'s Genesis Tree beat (`P2[5]`) lifts the floating
//! figure's own context, not only its draw.
//!
//! The beat writes context `0x15`'s `+0x8E = 480` (`CC 15 42 E0 01 00 00`)
//! before it raises `0x20000000` (`B1 15 1D`), so the immediate arm's mirror
//! does not fire; the height arm (`FUN_8003BC08`, store `0x8003BCF4`) then
//! writes `+0x16 = -(+0x8E)` every actor tick. The move-VM mode-7 grab the
//! beat fires (`B4 15 30 ..`) is placed from that context's position, so a
//! context left at `Y = 0` aims the grab at the party instead of the figure.
//! The beat then tweens the slot to 250 over 80 frames (`CC 15 42 FA 00 50
//! 00`), which the context follows too.
//!
//! Skip-passes when `LEGAIA_DISC_BIN` / `extracted/` are missing.

use std::path::PathBuf;

use legaia_engine_scene_host::scene_host::SceneHost;
use legaia_engine_system::input::PadButton;

fn extracted_root() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    for p in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(p);
        if d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
    None
}

#[test]
fn the_genesis_figure_context_floats_or_skip() {
    let Some(root) = extracted_root() else { return };
    let mut host = SceneHost::open_extracted(&root).expect("open SceneHost");
    host.enter_field_scene("kor5", 0).expect("enter kor5");
    for _ in 0..4 {
        host.tick().expect("tick");
    }
    // `P2[5]`, global `N0 + N1 + 5` with kor5's counts `[7, 30, 9]`.
    host.world.field_vm.pending_record_spawns.push(7 + 30 + 5);
    let mut ys = Vec::new();
    for i in 0..1500 {
        let pad = if i % 16 < 2 {
            PadButton::Cross.mask()
        } else {
            0
        };
        host.world.input.set_pad(pad);
        host.tick().expect("tick");
        let c = host
            .world
            .field_vm
            .channels
            .iter()
            .find(|c| c.ctx.script_id == 0x15)
            .expect("context 0x15");
        let y = c.ctx.world_y as i16;
        if ys.last() != Some(&y) {
            ys.push(y);
        }
    }
    eprintln!("[ran] context 0x15 Ys: {ys:?}");
    assert!(
        ys.contains(&-480),
        "the figure's context floats at -480: {ys:?}"
    );
    let at = |y: i16| ys.iter().position(|&v| v == y);
    let (Some(high), Some(low)) = (at(-480), at(-250)) else {
        panic!("the figure floats at -480, then tweens to -250: {ys:?}");
    };
    assert!(high < low, "the 80-frame tween follows the hold: {ys:?}");
    assert!(
        low - high > 10,
        "the tween passes through intermediate heights: {ys:?}"
    );
}
