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
/// `_DAT_8007B83C`, the game mode word.
const GAME_MODE: u32 = 0x8007_B83C;
/// `_DAT_8007B750`, the sound flag word; bit 2 is op 0x35 sub-op 6's.
const SOUND_FLAGS: u32 = 0x8007_B750;
/// `_DAT_8007B7C8`, the scene object list the status word's bit `0x20`
/// spawns, and the stand-in list (its count word first).
const OBJECT_LIST_PTR: u32 = 0x8007_B7C8;
const OBJECT_LIST: u32 = 0x8013_0000;

#[derive(Debug, Default)]
struct Seen {
    windows: Vec<[i32; 4]>,
    spawns: Vec<(u32, [i16; 3])>,
    streams: Vec<u32>,
    prim_bytes: Vec<u32>,
    player_spawns: Vec<u32>,
    sweeps: usize,
    resolver_calls: u32,
    /// `FUN_80062004(source, channel, ticks)`: the field-BGM ramp.
    ramps: Vec<u32>,
    /// Voices keyed off (`FUN_800653C8`).
    voice_offs: Vec<u32>,
    /// Field-BGM source closes after the ramp (`FUN_800266E0` at `0x801D7164`).
    closes: usize,
    /// The streamed-pack reads (`FUN_8002541C`) by descriptor flag.
    packs: Vec<u32>,
    /// The scene object spawns (`FUN_80024CFC`).
    objects: Vec<u32>,
    /// The MAN decoder's first argument.
    man_a0: Vec<u32>,
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
    // Whether the MAN decoder takes its party-wipe arm, which stores game
    // mode `0x16` (CARD INIT) at `0x8003B5D4`, and whether the resolver
    // settles the bank and the slot on separate passes.
    let wipe: Rc<RefCell<(bool, bool)>> = Rc::default();
    let read_cost: Rc<RefCell<u32>> = Rc::default();
    let anchor: Rc<RefCell<([i16; 2], i16, u32)>> = Rc::default();
    {
        // The MAN decoder: the camera anchor into the caller's pair, and the
        // scene's TMD count back.
        let anchor = anchor.clone();
        let seen = seen.clone();
        let wipe = wipe.clone();
        cpu.hook(MAN_DECODE, move |r, m| {
            seen.borrow_mut().man_a0.push(r.r[A0]);
            if wipe.borrow().0 {
                m.wr16(GAME_MODE, 0x16);
            }
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
        let cost = read_cost.clone();
        cpu.hook(STREAM_LOAD, move |r, m| {
            seen.borrow_mut().streams.push(r.r[A0]);
            // A read takes its vsyncs off the clock the floor tests.
            let t = m.rd32(HOLD_CLOCK);
            m.wr32(HOLD_CLOCK, t + *cost.borrow());
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
        let wipe = wipe.clone();
        cpu.hook(0x8002_43F0, move |_, m| {
            let mut s = seen.borrow_mut();
            s.resolver_calls += 1;
            let staggered = wipe.borrow().1;
            if s.resolver_calls >= 2 {
                let bank = m.rd32(0x8007_BABC);
                m.wr32(0x8007_BAA0, bank);
            }
            if s.resolver_calls >= 2 + u32::from(staggered) {
                let slot = m.rd32(BGM_SLOT);
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
    // The audio and loader calls the environment words gate.
    {
        let seen1 = seen.clone();
        cpu.hook(0x8006_2004, move |r, _| {
            seen1.borrow_mut().ramps.push(r.r[A2])
        });
        let seen2 = seen.clone();
        cpu.hook(0x8006_53C8, move |r, _| {
            seen2.borrow_mut().voice_offs.push(r.r[A0])
        });
        let seen3 = seen.clone();
        // The ramp's close (`0x801D7164`); the two-part arm closes the
        // source on its own (`0x801D71C8`).
        cpu.hook(0x8002_66E0, move |r, _| {
            seen3.borrow_mut().closes += usize::from(r.r[31] == 0x801D_716C);
        });
        let seen4 = seen.clone();
        cpu.hook(0x8002_541C, move |r, _| {
            seen4.borrow_mut().packs.push(r.r[A0]);
            r.r[V0] = 0;
        });
        let seen5 = seen.clone();
        cpu.hook(0x8002_4CFC, move |r, _| {
            seen5.borrow_mut().objects.push(r.r[A0])
        });
    }
    // The descriptor walker's status word (`FUN_80020224`): bit `4` hands the
    // MAN decoder its first argument, bit `0x20` spawns the scene objects,
    // the high byte names a streamed pack.
    let status = Rc::new(RefCell::new(0u32));
    {
        let status = status.clone();
        cpu.hook(0x8002_0224, move |r, _| r.r[V0] = *status.borrow());
    }
    // The vsync callback's clock, which a cold entry waits on.
    cpu.hook(0x8005_FB84, |_, m| {
        let t = m.rd32(HOLD_CLOCK);
        m.wr32(HOLD_CLOCK, t + 1);
    });

    let mut rng = Rng(0x4649_4E49);
    let (mut cold, mut warp, mut other, mut recentred, mut two_part, mut latched) =
        (0, 0, 0, 0, 0, 0);
    let (mut ramps, mut kept, mut battle_ramps, mut wipes) = (0, 0, 0, 0);
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
        // The environment words: sound-flag bit 2 (op 0x35 sub-op 6), the
        // field-BGM source word, the scene record's BGM halfword, the
        // walker's status, the objects to spawn, the MAN reload word, the
        // streaming buffer's carried sectors and the stale buffer.
        let keep_music = rng.below(2) == 0;
        let source_up = rng.below(4) != 0;
        let record_bgm = rng.below(4) != 0;
        let st = [0u32, 4, 0x20, 0x24][rng.below(4) as usize]
            | [0u32, 0x14, 0x0A][rng.below(3) as usize] << 8;
        let objects = rng.below(4);
        *status.borrow_mut() = st;
        let bank_latched = rng.below(2) == 0;
        let wiped = rng.below(5) == 0;
        let staggered = rng.below(2) == 0;
        *wipe.borrow_mut() = (wiped, staggered);
        *read_cost.borrow_mut() = [0, 8, 0x30, 0x80][rng.below(4) as usize];
        // The battle source: its loaded index (`_DAT_8007B880`, negative
        // with none) and the scene record's battle-track halfword.
        let battle_source = rng.below(3) != 0;
        let record_battle = rng.below(4) != 0;

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
        cpu.mem.wr32(SFX_LATCH, u32::from(bank_latched));
        cpu.mem.wr32(HOLD_CLOCK, 0x100);
        cpu.mem.wr16(GAME_MODE, 2);
        cpu.mem.wr32(SOUND_FLAGS, if keep_music { 4 } else { 0 });
        cpu.mem.wr32(0x8007_BB10, u32::from(source_up));
        cpu.mem.wr16(0x8007_0534, u16::from(record_bgm) * 0x7D3);
        cpu.mem.wr16(0x8007_0536, 0);
        cpu.mem.wr32(
            0x8007_B880,
            if battle_source {
                rng.below(0x40)
            } else {
                u32::MAX
            },
        );
        cpu.mem.wr16(0x8007_0574, u16::from(record_battle));
        cpu.mem.wr16(0x8007_0576, 1);
        cpu.mem.wr32(OBJECT_LIST_PTR, OBJECT_LIST);
        cpu.mem.wr32(OBJECT_LIST, objects);
        cpu.mem.wr16(0x8007_B8C0, rng.below(2) as u16);
        cpu.mem.wr32(0x8007_B6FC, rng.below(2) * rng.below(8));
        cpu.mem.wr32(0x8007_BAF0, rng.below(2) * HEAP);
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
                (
                    legaia_engine_system::mode_entry_init::FIELD_ACTOR_LIST_SWEEPS,
                    0
                ),
                "{ctx}"
            );
        } else {
            assert_eq!(
                (s.sweeps, &s.player_spawns[..]),
                (0, &[0x8007_05FC][..]),
                "{ctx}"
            );
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
                assert_eq!(
                    cpu.mem.rd32(BGM_PLAYING),
                    plan.slot,
                    "{ctx}: staged as playing"
                );
                two_part += 1;
            }
            None => {
                assert!(two.is_empty(), "{ctx}: {two:x?}");
                // The barrier sits behind the same one-shot as the
                // two-part read (`0x801D7190`).
                // A wiped party's init skips it (`0x801D72DC`); a resolver
                // that settles the slot a pass after the bank takes one
                // more (`0x801D731C`).
                assert_eq!(
                    s.resolver_calls,
                    if was_latched || wiped {
                        0
                    } else {
                        2 + u32::from(staggered)
                    },
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

        // The field-BGM ramp, its close and the voice teardown against
        // `World::release_field_init_voices`. The engine's field entry is
        // always cold, so the warp gate is retail's alone; the ramp and the
        // close also need a source up and a scene track, which the engine's
        // close finds or not on its own.
        let mut w = legaia_engine_core::world::World::new();
        if keep_music {
            w.raise_keep_audio_across_field_init();
        }
        w.release_field_init_voices();
        let port_close = w.pending_field_events.iter().any(|e| {
            matches!(e, legaia_engine_field::field_events::FieldEvent::Bgm { sub_op, .. }
                if *sub_op == legaia_engine_core::world::BGM_SUB_OP_ENGINE_FIELD_CLOSE)
        });
        let port_mutes = w.take_sfx_voice_mutes();
        let cold_entry = raw_mode == 0;
        let ramped = cold_entry && source_up && record_bgm && !keep_music;
        // A battle return (`_DAT_8007B8B8 == 2`) ramps the battle source
        // instead, over `0xB4` (`0x801D6B30..0x801D6B80`), which
        // `World::restore_field_bgm` closes.
        let battle_ramp = raw_mode == 2 && battle_source && record_battle;
        let want: Vec<u32> = [(ramped, 0x78), (battle_ramp, 0xB4)]
            .into_iter()
            .filter_map(|(on, t)| on.then_some(t))
            .collect();
        assert_eq!(s.ramps, want, "{ctx}: ramp");
        battle_ramps += u32::from(battle_ramp);
        assert_eq!(s.closes, usize::from(ramped), "{ctx}: close");
        if cold_entry {
            assert_eq!(port_close, !keep_music, "{ctx}: the port's close");
            assert_eq!(
                s.voice_offs,
                port_mutes.iter().map(|&v| u32::from(v)).collect::<Vec<_>>(),
                "{ctx}: voices"
            );
        } else {
            assert!(s.voice_offs.is_empty(), "{ctx}: a warp keeps its voices");
        }
        assert_eq!(cpu.mem.rd32(SOUND_FLAGS) & 4, 0, "{ctx}: bit 2 is spent");
        assert!(
            !w.keeps_audio_across_field_init(),
            "{ctx}: the port spends it"
        );
        // The floor: an ordinary entry holds until the clock the init zeroed
        // reads `FIELD_INIT_FLOOR_VSYNCS`, however fast its reads ran.
        if cold_entry && !dev {
            let floor = u32::from(legaia_engine_system::mode_entry_init::FIELD_INIT_FLOOR_VSYNCS);
            assert!(cpu.mem.rd32(HOLD_CLOCK) >= floor, "{ctx}: the floor");
        }
        // The party wipe (`0x801D7474..`): the init keeps CARD INIT, which
        // hands to the title, and closes both BGM sources with their loaded
        // indices at `-1`, so no field track sounds; any other entry goes on
        // to MAIN MODE. The engine takes the wipe off the battle instead
        // (`World::game_over`, `NativeSession::take_party_wipe`), and the
        // hold it resolves queues no field track
        // (`World::resolve_game_over_hold`).
        if wiped {
            assert_eq!(cpu.mem.rd16(GAME_MODE), 0x16, "{ctx}: CARD INIT kept");
            assert_eq!(
                (cpu.mem.rd32(0x8007_BABC), cpu.mem.rd32(0x8007_BAA0)),
                (u32::MAX, u32::MAX),
                "{ctx}: the sources' indices"
            );
            let mut w = legaia_engine_core::world::World::new();
            w.game_over_hold = true;
            w.audio.battle_bgm_active = true;
            w.resolve_game_over_hold();
            assert!(!w.game_over_hold, "{ctx}");
            assert!(w.pending_field_events.is_empty(), "{ctx}");
            wipes += 1;
        } else {
            assert_eq!(cpu.mem.rd16(GAME_MODE), 3, "{ctx}: MAIN MODE");
        }
        // The walker's status: the pack read, the objects, the MAN decode.
        assert_eq!(
            s.packs,
            if st >> 8 != 0 { vec![st >> 8] } else { vec![] },
            "{ctx}: pack read"
        );
        let want_objects = if cold_entry && st & 0x20 != 0 {
            objects
        } else {
            0
        };
        assert_eq!(
            s.objects,
            (0..want_objects).collect::<Vec<_>>(),
            "{ctx}: scene objects"
        );
        assert_eq!(s.man_a0, [u32::from(st & 4 != 0)], "{ctx}: MAN decode");
        // The field SFX bank (`0x801D6FF4..0x801D7048`): loaded raw `0x36E`
        // into slot 6 while the latch is clear, against
        // `SfxBankResidency::field_init`.
        let mut bank = legaia_engine_mode::sfx_residency::SfxBankResidency::default();
        bank.field_bank_latch = bank_latched;
        bank.field_init();
        let loaded = s.streams.contains(&0x36E);
        assert_eq!(loaded, bank.slot_open(6), "{ctx}: field bank");
        if loaded {
            let field = legaia_engine_mode::sfx_residency::SharedRegionBank::FIELD;
            assert_eq!(
                (bank.shared, field.prot_entry + 2),
                (Some(field), 0x36E),
                "{ctx}"
            );
        }
        assert_eq!(cpu.mem.rd32(SFX_LATCH), 1, "{ctx}: the latch");
        assert!(bank.field_bank_latch, "{ctx}");
        ramps += u32::from(ramped);
        kept += u32::from(keep_music);

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
        cold > 500
            && warp > 500
            && other > 500
            && recentred > 400
            && two_part > 100
            && latched > 300
            && ramps > 100
            && kept > 1000
            && battle_ramps > 100
            && wipes > 300,
        "{cold} {warp} {other} {recentred} {two_part} {latched} {ramps} {kept} {battle_ramps}"
    );
}
