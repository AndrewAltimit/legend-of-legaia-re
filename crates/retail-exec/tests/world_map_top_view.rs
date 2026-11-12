//! The world-map controller `FUN_801E76D4` (PROT 0897) frame by frame on a
//! generated pad, against `WorldMapController::tick`: the top-view debug
//! chord, the two anim-flag toggles and the L1-modified camera block (focus
//! X / Z `0x80089118` / `0x80089120`, azimuth `0x8007B794`, height
//! `0x8007B6F4`). Every call out of the routine is stubbed (its debug text
//! and the chord's save); the panel phase `+0x54` the routine's later arms
//! raise is put back to `0` each frame, the port's panels being a host of
//! their own (`World::tick_world_map_panels`).

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_field::world_map::WorldMapController;
use psx_retail_exec::Stop;

const OVERLAY_BASE: u32 = 0x801C_E818;
const CONTROLLER: u32 = 0x801E_76D4;
const CONTROLLER_END: u32 = 0x801E_9B3C;
const DEBUG_WORD: u32 = 0x8007_B98C;
const HELD: u32 = 0x8007_B850;
const EDGE: u32 = 0x8007_B874;
const VIEW_MODE: u32 = 0x801F_2B94;
const ANIM_FLAGS: u32 = 0x801F_2B95;
const FOCUS_X: u32 = 0x8008_9118;
const FOCUS_Z: u32 = 0x8008_9120;
const AZIMUTH: u32 = 0x8007_B794;
const HEIGHT: u32 = 0x8007_B6F4;
const PLAYER_PTR: u32 = 0x8007_C364;
const ACTOR: u32 = 0x8012_0000;
const PLAYER: u32 = 0x8012_1000;

#[test]
fn the_top_view_debug_camera_matches_retail() {
    differential!(prot(897, CONTROLLER));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(897).expect("PROT 0897"));
    cpu.stub_outside = Some((CONTROLLER, CONTROLLER_END));
    cpu.mem.wr32(DEBUG_WORD, 1);
    cpu.mem.wr32(PLAYER_PTR, PLAYER);
    let mut rng = Rng(0x544F_5056);
    let mut toggles = 0;
    let mut moved = 0;
    for round in 0..40 {
        for a in [VIEW_MODE, ANIM_FLAGS] {
            cpu.mem.wr8(a, 0);
        }
        for a in [FOCUS_X, FOCUS_Z] {
            cpu.mem.wr32(a, 0);
        }
        cpu.mem.wr16(AZIMUTH, 0);
        cpu.mem.wr16(HEIGHT, 0);
        let mut port = WorldMapController {
            debug_enabled: true,
            map_display_enabled: false,
            ..Default::default()
        };
        for frame in 0..300 {
            cpu.mem.wr16(ACTOR + 0x54, 0);
            // Packed words: the top-view chord now and then, else a mix of
            // the modifier, the d-pad, the face buttons and the shoulders.
            let (held, edge): (u16, u16) = match rng.below(12) {
                0 => (0x4A, 0x40),
                1 => (0x4A, 0x40 | (rng.next() as u16 & 0x20)),
                _ => {
                    let mut h = rng.next() as u16 & 0xF0AF;
                    if rng.below(2) == 0 {
                        h |= 4;
                    }
                    (h, rng.next() as u16 & h)
                }
            };
            cpu.mem.wr32(HELD, u32::from(held));
            cpu.mem.wr32(EDGE, u32::from(edge));
            let at = format!("round {round} frame {frame} held {held:#06x} edge {edge:#06x}");
            assert_eq!(cpu.call(CONTROLLER, &[ACTOR]), Stop::Returned, "{at}");
            toggles += usize::from(port.tick(held.swap_bytes(), edge.swap_bytes()));
            assert_eq!(cpu.mem.rd8(VIEW_MODE), port.view_mode, "view mode, {at}");
            assert_eq!(cpu.mem.rd8(ANIM_FLAGS), port.anim_flags, "anim flags, {at}");
            assert_eq!(
                cpu.mem.rd32(FOCUS_X) as i32,
                port.camera_x,
                "camera x, {at}"
            );
            assert_eq!(
                cpu.mem.rd32(FOCUS_Z) as i32,
                port.camera_z,
                "camera z, {at}"
            );
            assert_eq!(cpu.mem.rd16(AZIMUTH), port.azimuth as u16, "azimuth, {at}");
            assert_eq!(cpu.mem.rd16(HEIGHT), port.zoom as u16, "height, {at}");
            moved += usize::from(port.view_mode != 0 && held & 4 != 0 && held & 0xF000 != 0);
        }
    }
    assert!(
        toggles > 100 && moved > 1000,
        "toggles {toggles} moved {moved}"
    );
}
