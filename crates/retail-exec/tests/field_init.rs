//! The field mode's initialiser `FUN_801D6704` (PROT 0897), run whole with
//! everything it calls stood in, against the kernels of `mode_entry_init`
//! that carry its tag: where it seats the player and the camera window
//! (`field_spawn`), the BGM slot and latch it resolves (`field_bgm_plan`)
//! and the primitive buffer it asks for (`field_prim_buffer_bytes`), over
//! every entry mode, anchor sign, recentre request and BGM id form.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_system::mode_entry_init::{
    FIELD_BGM_TWO_PART_ID, FieldEntryMode, field_bgm_plan, field_prim_buffer_bytes, field_spawn,
};
use psx_retail_exec::{A0, A1, A2, A3, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const FIELD_INIT: u32 = 0x801D_6704;
const FIELD_INIT_END: u32 = 0x801D_7518;
const OVERLAY_BASE: u32 = 0x801C_E818;
const MAN_DECODE: u32 = 0x8003_AEB0;
const WINDOW_INSTALL: u32 = 0x8001_7DD4;
const SPAWN_AT: u32 = 0x8002_4C88;
const STREAM_LOAD: u32 = 0x8001_FC00;
const PRIM_BUFFER: u32 = 0x8001_E3B8;
const ALLOC: u32 = 0x8001_7888;

const ENTRY_MODE: u32 = 0x8007_B8B8;
const DEV_FLAG: u32 = 0x8007_B868;
const DEV_FLAG_2: u32 = 0x8007_B8BE;
const SAVED_X: u32 = 0x8008_4568;
const SAVED_Z: u32 = 0x8008_456C;
const RECENTRE: u32 = 0x8007_BACC;
const ORIGIN_X: u32 = 0x8007_B76C;
const ORIGIN_Z: u32 = 0x8007_B770;
const BGM_ID: u32 = 0x8007_BAC8;
const BGM_SLOT: u32 = 0x8007_BAB8;
const BGM_PLAYING: u32 = 0x8007_BA9C;
const BGM_LATCH: u32 = 0x8007_BAD0;
const SEQ_BASE: u32 = 0x8008_4540;
const STAGED_BASE: u32 = 0x8007_B768;
const TWO_PART_LATCH: u32 = 0x8007_B9B8;
const SFX_LATCH: u32 = 0x8007_BAFC;
const HOLD_CLOCK: u32 = 0x8007_B8AC;
/// The actor lists' block; `+0x1C` is the player actor.
const LISTS: u32 = 0x8007_C348;
const PLAYER: u32 = 0x8010_0000;
const SPAWNED: u32 = 0x8010_1000;
const HEAP: u32 = 0x8012_0000;
const BUFFER: u32 = 0x8016_0000;

#[derive(Debug, Default)]
struct Seen {
    windows: Vec<[i32; 4]>,
    spawns: Vec<(u32, [i16; 3])>,
    streams: Vec<u32>,
    prim_bytes: Vec<u32>,
    player_spawns: Vec<u32>,
    sweeps: usize,
    resolver_calls: u32,
}

#[test]
fn the_field_initialiser_seats_what_the_port_seats() {
    differential!(prot(897, FIELD_INIT));
    let (Some(mut cpu), Some(d)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &d.read_entry(897).expect("PROT 0897"));
    cpu.stub_outside = Some((FIELD_INIT, FIELD_INIT_END));
    cpu.step_budget = 4_000_000;
    let seen: Rc<RefCell<Seen>> = Rc::default();
    let anchor: Rc<RefCell<([i16; 2], i16, u32)>> = Rc::default();
    {
        // The MAN decoder: the camera anchor into the caller's pair, and the
        // scene's TMD count back.
        let anchor = anchor.clone();
        cpu.hook(MAN_DECODE, move |r, m| {
            let (xz, tmds, bgm_id) = *anchor.borrow();
            // The scene's BGM id is the decoder's to set; the initialiser
            // clears the word ahead of the call.
            m.wr32(BGM_ID, bgm_id);
            m.wr16(r.r[A1], xz[0] as u16);
            m.wr16(r.r[A1] + 2, xz[1] as u16);
            r.r[V0] = tmds as u16 as u32;
        });
    }
    {
        let seen = seen.clone();
        cpu.hook(WINDOW_INSTALL, move |r, _| {
            seen.borrow_mut()
                .windows
                .push([r.r[A0], r.r[A1], r.r[A2], r.r[A3]].map(|v| v as i32));
        });
    }
    {
        let seen = seen.clone();
        cpu.hook(SPAWN_AT, move |r, m| {
            let v = std::array::from_fn(|k| m.rd16(r.r[A0] + 2 * k as u32) as i16);
            seen.borrow_mut().spawns.push((r.r[A1], v));
            r.r[V0] = SPAWNED;
        });
    }
    {
        let seen = seen.clone();
        cpu.hook(STREAM_LOAD, move |r, _| {
            seen.borrow_mut().streams.push(r.r[A0]);
            r.r[V0] = 0;
        });
    }
    {
        let seen = seen.clone();
        cpu.hook(PRIM_BUFFER, move |r, _| {
            seen.borrow_mut().prim_bytes.push(r.r[A0]);
        });
    }
    cpu.stub(ALLOC, HEAP);
    // The BGM resolver the wait barrier spins on, answering on its second
    // call: the requested pair becomes the playing pair.
    {
        let seen = seen.clone();
        cpu.hook(0x8002_43F0, move |_, m| {
            let mut s = seen.borrow_mut();
            s.resolver_calls += 1;
            if s.resolver_calls >= 2 {
                let (bank, slot) = (m.rd32(0x8007_BABC), m.rd32(BGM_SLOT));
                m.wr32(0x8007_BAA0, bank);
                m.wr32(BGM_PLAYING, slot);
            }
        });
    }
    // The player actor a non-warp entry spawns from the resident template.
    {
        let seen = seen.clone();
        cpu.hook(0x8002_0DE0, move |r, _| {
            seen.borrow_mut().player_spawns.push(r.r[A0]);
            r.r[V0] = PLAYER;
        });
    }
    {
        let seen = seen.clone();
        cpu.hook(0x801D_7518, move |_, _| seen.borrow_mut().sweeps += 1);
    }
    // The vsync callback's clock, which a cold entry waits on.
    cpu.hook(0x8005_FB84, |_, m| {
        let t = m.rd32(HOLD_CLOCK);
        m.wr32(HOLD_CLOCK, t + 1);
    });

    let mut rng = Rng(0x4649_4E49);
    let (mut cold, mut warp, mut other, mut recentred, mut two_part, mut latched) =
        (0, 0, 0, 0, 0, 0);
    for round in 0..3_000 {
        let raw_mode = [0u32, 0, 2, 2, 1, 3][rng.below(6) as usize];
        let coord = |rng: &mut Rng| match rng.below(4) {
            0 => -(rng.below(0x400) as i32),
            _ => rng.below(0x4000) as i32,
        };
        let xz = [coord(&mut rng) as i16, coord(&mut rng) as i16];
        let saved = (coord(&mut rng), coord(&mut rng));
        let recentre = rng.below(4) == 0;
        let origin = (rng.below(0x60) as i32, rng.below(0x60) as i32);
        let tmds = rng.below(0x100) as i16;
        let bgm_id = match rng.below(6) {
            0 => FIELD_BGM_TWO_PART_ID,
            1 => 0x7D0 + rng.below(0x60),
            2 => 0x7CF,
            _ => rng.below(0x40),
        };
        let staged_base = if rng.below(3) == 0 {
            0xFFFFu16
        } else {
            rng.below(0x400) as u16
        };
        let old_base = rng.below(0x400);
        let playing = match rng.below(4) {
            0 => u32::MAX,
            // The track this entry asks for, already up.
            1 => {
                let base = if (staged_base as i16) >= 0 {
                    u32::from(staged_base)
                } else {
                    old_base
                };
                field_bgm_plan(bgm_id, base, 0, true).slot
            }
            _ => rng.below(0x50),
        };
        let was_latched = rng.below(4) == 0;
        let (dev, dev2) = (rng.below(4) == 0, rng.below(2) == 0);

        *anchor.borrow_mut() = (xz, tmds, bgm_id);
        *seen.borrow_mut() = Seen::default();
        cpu.mem.load(PLAYER, &[0xA5; 0x100]);
        cpu.mem.load(SPAWNED, &[0; 0x100]);
        cpu.mem.load(HEAP, &[0; 0x900]);
        for k in 0..12 {
            cpu.mem.wr32(LISTS + 4 * k, 0);
        }
        cpu.mem.wr32(LISTS + 0x1C, PLAYER);
        cpu.mem.wr32(0x8007_B85C, BUFFER);
        cpu.mem.wr32(ENTRY_MODE, raw_mode);
        cpu.mem.wr32(DEV_FLAG, u32::from(dev));
        cpu.mem.wr16(DEV_FLAG_2, u16::from(dev2));
        cpu.mem.wr32(SAVED_X, saved.0 as u32);
        cpu.mem.wr32(SAVED_Z, saved.1 as u32);
        cpu.mem.wr32(RECENTRE, u32::from(recentre));
        cpu.mem.wr32(ORIGIN_X, origin.0 as u32);
        cpu.mem.wr32(ORIGIN_Z, origin.1 as u32);
        cpu.mem.wr32(BGM_PLAYING, playing);
        cpu.mem.wr32(BGM_LATCH, 0xDEAD);
        cpu.mem.wr16(STAGED_BASE, staged_base);
        cpu.mem.wr32(SEQ_BASE, old_base);
        cpu.mem.wr32(TWO_PART_LATCH, u32::from(was_latched));
        cpu.mem.wr32(SFX_LATCH, 1);
        cpu.mem.wr32(HOLD_CLOCK, 0x100);
        let stop = cpu.call(FIELD_INIT, &[]);
        assert_eq!(stop, Stop::Returned, "round {round}");
        let ctx = format!(
            "round {round}: mode {raw_mode} anchor {xz:?} saved {saved:?} recentre {recentre} \
             origin {origin:?} bgm {bgm_id:#x}"
        );
        let s = seen.borrow();

        // The seat.
        let mode = FieldEntryMode::from_raw(raw_mode);
        let port = field_spawn(mode, (xz[0], xz[1]), saved, recentre, origin);
        assert_eq!(s.windows.len(), 1, "{ctx}");
        assert_eq!(
            (s.windows[0][0], s.windows[0][1]),
            port.window_tile,
            "{ctx}: window tile"
        );
        assert_eq!(
            (s.windows[0][2], s.windows[0][3]),
            legaia_engine_system::mode_entry_init::FIELD_CAMERA_WINDOW_TILES,
            "{ctx}: window size"
        );
        let player = (
            cpu.mem.rd16(PLAYER + 0x14) as i16,
            cpu.mem.rd16(PLAYER + 0x18) as i16,
        );
        assert_eq!(player, port.player, "{ctx}: player seat");
        assert_eq!(cpu.mem.rd16(PLAYER + 0x16), 0, "{ctx}: player height");
        assert_eq!(cpu.mem.rd32(RECENTRE), 0, "{ctx}: the request is spent");
        let spawned: Vec<(i16, i16, i16)> = s
            .spawns
            .iter()
            .filter(|(template, _)| *template == 0x801F_271C)
            .map(|(_, v)| (v[0], v[1], v[2]))
            .collect();
        assert_eq!(
            spawned,
            port.extra_actor.into_iter().collect::<Vec<_>>(),
            "{ctx}: the cold arm's actor"
        );

        // A warp keeps its actors and sweeps the seven lists; any other
        // entry spawns the player.
        if mode == FieldEntryMode::Warp {
            assert_eq!(
                (s.sweeps, s.player_spawns.len()),
                (legaia_engine_system::mode_entry_init::FIELD_ACTOR_LIST_SWEEPS, 0),
                "{ctx}"
            );
        } else {
            assert_eq!((s.sweeps, &s.player_spawns[..]), (0, &[0x8007_05FC][..]), "{ctx}");
        }

        // The BGM slot.
        let seq_base = cpu.mem.rd32(SEQ_BASE);
        let want_base = if (staged_base as i16) >= 0 {
            u32::from(staged_base)
        } else {
            old_base
        };
        assert_eq!(seq_base, want_base, "{ctx}: sequence base");
        let plan = field_bgm_plan(bgm_id, seq_base, playing, was_latched);
        assert_eq!(cpu.mem.rd32(BGM_SLOT), plan.slot, "{ctx}: BGM slot");
        let two: Vec<u32> = s
            .streams
            .iter()
            .copied()
            .filter(|id| *id == 0x428 || *id == 0x422)
            .collect();
        match plan.two_part_streams {
            // The bank is read twice: its head and first part, then the rest.
            Some([seq, vab]) => {
                assert_eq!(two, [seq, vab, vab], "{ctx}: two-part streams");
                assert_eq!(cpu.mem.rd32(BGM_PLAYING), plan.slot, "{ctx}: staged as playing");
                two_part += 1;
            }
            None => {
                assert!(two.is_empty(), "{ctx}: {two:x?}");
                // The barrier sits behind the same one-shot as the
                // two-part read (`0x801D7190`).
                assert_eq!(
                    s.resolver_calls,
                    if was_latched { 0 } else { 2 },
                    "{ctx}: the wait barrier"
                );
                // The latch the wait barrier reads; the two-part arm
                // restages the playing slot after it.
                assert_eq!(
                    cpu.mem.rd32(BGM_LATCH),
                    u32::from(plan.already_playing),
                    "{ctx}: same-track latch"
                );
                latched += u32::from(plan.already_playing);
            }
        }

        // The primitive buffer.
        assert_eq!(
            s.prim_bytes,
            [field_prim_buffer_bytes(tmds, dev && dev2)],
            "{ctx}: primitive buffer"
        );
        match mode {
            FieldEntryMode::Cold => cold += 1,
            FieldEntryMode::Warp => warp += 1,
            FieldEntryMode::Other(_) => other += 1,
        }
        recentred += u32::from(recentre);
    }
    assert!(
        cold > 500 && warp > 500 && other > 500 && recentred > 400 && two_part > 100 && latched > 300,
        "{cold} {warp} {other} {recentred} {two_part} {latched}"
    );
}
