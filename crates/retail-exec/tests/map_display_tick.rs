//! The L1 map display's enter `FUN_801CF4AC` and per-frame tick
//! `FUN_801CE850` (PROT 0981) against `MapDisplay::enter` / `MapDisplay::tick`,
//! session by session over generated label tables and pad streams: the mode
//! word, the fade timer, the cursor, the camera seat and its target, the
//! survey camera, the overlay cycle and the cue each frame raises.
//!
//! The tick is run up to the end of its state update (`0x801CF00C`); what
//! follows is the mini-map and label draw. The walk camera's own update
//! `FUN_800271A8` is stubbed, so the staging frame starts from the walk
//! frame's translation seeded as the port reads it, the negated player
//! position (harness, not game code), and once mode `3` has put the saved walk
//! camera back the survey camera is no longer compared.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_field::map_display::{MODE_INTERACTIVE, MapDisplay, MapLocation};
use psx_retail_exec::{Cpu, RA, RETURN_SENTINEL, Stop};

const ENTER: u32 = 0x801C_F4AC;
const TICK: u32 = 0x801C_E850;
/// The two paths out of the overlay-cycle arm meet the draw half here.
const STATE_END: [u32; 2] = [0x801C_F00C, 0x801C_F010];
const OVERLAY_BASE: u32 = 0x801C_E818;
const BLOCK: u32 = 0x801C_F70C;
const BLOCK_END: u32 = 0x801C_F790;
const MODE: u32 = 0x801C_F76C;
const TIMER: u32 = 0x801C_F778;
const CURSOR: u32 = 0x801C_F77C;
const SEAT: u32 = 0x801C_F784;
const OVERLAY: u32 = 0x801C_F73C;
const TARGET: u32 = 0x801C_F74C;
const POINTER: u32 = 0x801C_F788;
const CAM: u32 = 0x8008_9118;
const TABLE_PTR: u32 = 0x8007_3EE0;
const TABLE: u32 = 0x8014_0000;
const KINGDOM: u32 = 0x8007_B970;
const PLAYER_PTR: u32 = 0x8007_C364;
const PLAYER: u32 = 0x8013_0000;
const SPAWNED: u32 = 0x8013_1000;
const SPAWNED_TICK: u32 = 0x8000_1000;
const PAD_HELD: u32 = 0x8007_B850;
const PAD_EDGE: u32 = 0x8007_B874;
const SFX: u32 = 0x8007_B6D8;
const GAME_MODE: u32 = 0x8007_B83C;

fn retail_cpu() -> Option<Cpu> {
    let mut cpu = scus_cpu()?;
    cpu.mem
        .load(OVERLAY_BASE, &disc()?.read_entry(981).expect("PROT 0981"));
    for stub in [
        0x8001_72C0u32,
        0x8002_4EE4,
        0x8002_71A8,
        0x8005_67A8,
        0x801D_58F0,
        SPAWNED_TICK,
    ] {
        cpu.stub(stub, 0);
    }
    cpu.stub(0x8002_1B04, SPAWNED);
    for at in STATE_END {
        cpu.hook(at, |r, _| r.r[RA] = RETURN_SENTINEL);
    }
    Some(cpu)
}

fn state(cpu: &mut Cpu) -> (u32, i32, i32, u32, u32, i32, i32, i32, i32) {
    (
        cpu.mem.rd32(MODE),
        cpu.mem.rd32(TIMER) as i32,
        cpu.mem.rd32(CURSOR) as i32,
        cpu.mem.rd32(SEAT),
        cpu.mem.rd32(OVERLAY),
        cpu.mem.rd32(CAM) as i32,
        cpu.mem.rd32(CAM + 8) as i32,
        cpu.mem.rd32(TARGET) as i32,
        cpu.mem.rd32(TARGET + 8) as i32,
    )
}

fn port_state(m: &MapDisplay) -> (u32, i32, i32, u32, u32, i32, i32, i32, i32) {
    (
        m.mode, m.timer, m.cursor, m.seat, m.overlay, m.cam_x, m.cam_z, m.target_x, m.target_z,
    )
}

#[test]
fn a_map_display_session_runs_as_retail_runs_it() {
    differential!(prot(981, ENTER), prot(981, TICK));
    let Some(mut cpu) = retail_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem.wr32(TABLE_PTR, TABLE);
    cpu.mem.wr32(PLAYER_PTR, PLAYER);
    cpu.mem.wr32(SPAWNED + 0xC, SPAWNED_TICK);
    let mut rng = Rng(0x4D41_5054);
    let (mut sessions, mut exits, mut steps, mut pans, mut cycles) = (0, 0, 0, 0, 0);
    for session in 0..400 {
        let count = 1 + rng.below(29) as usize;
        let kingdom = rng.below(3);
        let mut locations: Vec<MapLocation> = (0..count)
            .map(|_| MapLocation {
                region: rng.below(3) as u8,
                map_x: rng.below(0x80) as u8,
                map_y: rng.below(0x80) as u8,
                discovery_flag: 0,
                name: Vec::new(),
            })
            .collect();
        // The backward walk spins on a table without the kingdom.
        let i = rng.below(count as u32) as usize;
        locations[i].region = kingdom as u8;
        let mut table = vec![0u8; 1 + count * 0x20];
        table[0] = count as u8;
        for (i, l) in locations.iter().enumerate() {
            table[1 + i * 0x20] = l.region;
            table[2 + i * 0x20] = l.map_x;
            table[3 + i * 0x20] = l.map_y;
        }
        cpu.mem.load(TABLE, &table);
        cpu.mem.wr32(KINGDOM, kingdom);
        let player = (
            rng.below(0x4000) as i32 - 0x200,
            rng.below(0x4000) as i32 - 0x200,
        );
        cpu.mem.wr16(PLAYER + 0x14, player.0 as u16);
        cpu.mem.wr16(PLAYER + 0x18, player.1 as u16);

        for at in (BLOCK..BLOCK_END).step_by(4) {
            cpu.mem.wr32(at, 0);
        }
        assert_eq!(cpu.call(ENTER, &[]), Stop::Returned);
        let mut m = MapDisplay::enter();
        let (r, p) = (state(&mut cpu), port_state(&m));
        assert_eq!(
            (r.0, r.1, r.2, r.3, r.4),
            (p.0, p.1, p.2, p.3, p.4),
            "session {session} enter"
        );
        assert_eq!(cpu.mem.rd16(GAME_MODE), 0xD);
        sessions += 1;

        for frame in 0..200 {
            if m.mode == 1 {
                // The walk frame's camera, as the stubbed update leaves it.
                cpu.mem.wr32(CAM, (-player.0) as u32);
                cpu.mem.wr32(CAM + 8, (-player.1) as u32);
            }
            let held = (rng.below(16) as u16) << 12;
            let edge = match rng.below(12) {
                0 => [0x20u16, 0x01, 0x80, 0x10, 0x100][rng.below(5) as usize],
                1 => 0x80,
                2 => 0x10,
                _ => 0,
            } | if frame > 150 && rng.below(10) == 0 {
                0x01
            } else {
                0
            };
            cpu.mem.wr32(PAD_HELD, u32::from(held));
            cpu.mem.wr32(PAD_EDGE, u32::from(edge));
            cpu.mem.wr16(SFX, 0);
            if m.mode == 4 {
                cpu.mem.wr32(POINTER, SPAWNED);
            }
            let stop = cpu.call(TICK, &[]);
            assert_eq!(stop, Stop::Returned, "session {session} frame {frame}");
            let before = port_state(&m);
            let out = m.tick(held, edge, player, &locations, kingdom);
            let label = format!(
                "session {session} frame {frame} held {held:#x} edge {edge:#x} from {before:?}"
            );
            let (mut r, mut p) = (state(&mut cpu), port_state(&m));
            if !m.camera_staged {
                // Mode 3's last frame puts back the walk camera `FUN_801CF4AC`
                // saved (`0x801CEB14..0x801CEB3C`); the port recomputes its
                // walk camera instead.
                (r.5, r.6, p.5, p.6) = (0, 0, 0, 0);
            }
            assert_eq!(r, p, "{label}");
            assert_eq!(
                cpu.mem.rd16(SFX) as i16,
                out.sfx.last().copied().unwrap_or(0),
                "{label}"
            );
            if m.mode == MODE_INTERACTIVE {
                pans += u32::from(held != 0);
                steps += u32::from(edge & 0x80 != 0);
                cycles += u32::from(edge & 0x110 != 0);
            }
            if out.exit {
                assert_eq!(cpu.mem.rd16(GAME_MODE), 0xC, "{label}");
                exits += 1;
                break;
            }
        }
    }
    assert!(
        sessions == 400 && exits > 300 && steps > 1000 && pans > 10_000 && cycles > 1000,
        "{exits} {steps} {pans} {cycles}"
    );
}
