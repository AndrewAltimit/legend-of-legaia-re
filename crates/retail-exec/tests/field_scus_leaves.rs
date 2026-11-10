//! Small `SCUS_942.54` leaves the field and the move VM reach, each run off
//! the disc against its port: the op-`0x43` camera-register ramp spawn
//! `FUN_8003C6A4`, the scene-entry reset `FUN_800353E0` with its setter
//! `FUN_8003C110`, the 32-byte block copy `FUN_8003D26C`, the CLUT-walk spawn
//! `FUN_80024CFC`, the field-VM model swap `FUN_80024E08` and the two bodies
//! that store nothing, move op `0x16`'s `FUN_80024C80` and the free-time
//! hook `FUN_80024DFC`.

use crate::common::{Rng, diff_offsets, scus_cpu};
use crate::field_vm::Inert;
use legaia_engine_field::float_tween::FIELD_SUBSYSTEM_RESET;
use legaia_engine_field::register_ramp::{RAMP_WIDTH, RampSlot, spawn_register_ramp};
use legaia_engine_vm::anim_vm::spawn_init;
use legaia_engine_vm::block_copy_32::copy_blocks_32;
use legaia_engine_vm::field::{FieldActorHost, FieldCtx};
use psx_retail_exec::{Cpu, Stop, V0};

const RAMP: u32 = 0x8003_C6A4;
const RESET: u32 = 0x8003_53E0;
const SET_MODE: u32 = 0x8003_C110;
const COPY: u32 = 0x8003_D26C;
const CLUT_SPAWN: u32 = 0x8002_4CFC;
const SET_MODEL: u32 = 0x8002_4E08;
const STUB_16: u32 = 0x8002_4C80;
const FREE_HOOK: u32 = 0x8002_4DFC;
const ALLOC: u32 = 0x8002_0DE0;
const RESTAGE: u32 = 0x8002_0F88;
const STACK_TOP: u32 = 0x801F_FF00;
const ACTOR: u32 = 0x8013_0000;
const GAME_MODE: u32 = 0x8007_B83C;

/// The allocator hooked to hand back `ACTOR` (or nothing), noting its
/// arguments.
fn hook_alloc(cpu: &mut Cpu, give: std::rc::Rc<std::cell::Cell<(bool, u32, u32)>>) {
    cpu.hook(ALLOC, move |r, _| {
        let (ok, _, _) = give.get();
        give.set((ok, r.r[psx_retail_exec::A0], r.r[psx_retail_exec::A0 + 1]));
        r.r[V0] = if ok { ACTOR } else { 0 };
    });
}

#[test]
fn the_camera_register_ramp_spawn_matches_retail() {
    differential!(scus(RAMP));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let give = std::rc::Rc::new(std::cell::Cell::new((true, 0, 0)));
    hook_alloc(&mut cpu, give.clone());
    let list = cpu.mem.rd32(0x8007_C34C);
    let mut rng = Rng(0x5241_4D50);
    let mut spawned = 0;
    for run in 0..4000 {
        let sub = 3 + rng.below(4) as u8;
        let slot = RampSlot::from_sub_op(sub).unwrap();
        let zone: [u8; 4] = std::array::from_fn(|_| rng.next() as u8);
        let (start, end) = (rng.next() as i16, rng.next() as i16);
        let ok = rng.below(8) != 0;
        give.set((ok, 0, 0));
        let junk: Vec<u8> = (0..0x100).map(|_| rng.next() as u8).collect();
        cpu.mem.load(ACTOR, &junk);
        let args = [
            u32::from(zone[2]),
            u32::from(zone[3]),
            i32::from(start) as u32,
            i32::from(end) as u32,
        ];
        for (i, a) in args.iter().enumerate() {
            cpu.mem.wr32(STACK_TOP + 0x10 + 4 * i as u32, *a);
        }
        assert_eq!(
            cpu.call(
                RAMP,
                &[
                    slot.retail_addr(),
                    u32::from(RAMP_WIDTH),
                    u32::from(zone[0]),
                    u32::from(zone[1])
                ]
            ),
            Stop::Returned
        );
        let (_, desc, head) = give.get();
        assert_eq!(
            (desc, head),
            (0x8007_4304, list),
            "run {run}: allocator args"
        );
        if !ok {
            assert_eq!(cpu.regs.r[V0], 0);
            assert_eq!(
                cpu.mem.read(ACTOR, 0x100),
                junk,
                "run {run}: no actor, no store"
            );
            continue;
        }
        spawned += 1;
        let r = spawn_register_ramp(sub, zone, start, end).unwrap();
        let mut port = junk.clone();
        let mut put = |off: usize, b: &[u8]| port[off..off + b.len()].copy_from_slice(b);
        put(0x94, &r.slot.retail_addr().to_le_bytes());
        put(0x8C, &u32::from(r.width).to_le_bytes());
        put(0x88, &r.x_lo.to_le_bytes());
        put(0x8A, &r.z_lo.to_le_bytes());
        put(0xC8, &r.x_hi.to_le_bytes());
        put(0xCA, &r.z_hi.to_le_bytes());
        put(0x80, &r.start.to_le_bytes());
        put(0x84, &r.end.to_le_bytes());
        let retail = cpu.mem.read(ACTOR, 0x100);
        assert_eq!(
            retail,
            port,
            "run {run} sub {sub} zone {zone:?}: {:?}",
            diff_offsets(&retail, &port)
        );
    }
    assert!(spawned > 3000, "{spawned}");
}

#[test]
fn the_scene_entry_reset_matches_retail() {
    differential!(scus(RESET), scus(SET_MODE));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x5253_4554);
    for _ in 0..16 {
        let ram_before: Vec<u8> = {
            for va in [0x8007_B460, 0x8007_B450, 0x8007_B454] {
                cpu.mem.wr32(va, rng.next());
            }
            cpu.mem.wr8(0x8007_3F20, rng.next() as u8);
            cpu.mem.read(0x8000_0000, 0x20_0000)
        };
        assert_eq!(cpu.call(RESET, &[]), Stop::Returned);
        let r = FIELD_SUBSYSTEM_RESET;
        assert_eq!(cpu.mem.rd32(0x8007_B460), r.list_head);
        assert_eq!(cpu.mem.rd32(0x8007_B450), r.gp_138);
        assert_eq!(cpu.mem.rd32(0x8007_B454), r.gp_13c);
        assert_eq!(cpu.mem.rd8(0x8007_3F20), r.mode_byte);
        // Nothing else in RAM (the stack frame aside) moved.
        let mut after = cpu.mem.read(0x8000_0000, 0x20_0000);
        let mut before = ram_before;
        for va in [0x8007_B460u32, 0x8007_B450, 0x8007_B454, 0x8007_3F20] {
            let o = (va - 0x8000_0000) as usize;
            let w = if va == 0x8007_3F20 { 1 } else { 4 };
            after[o..o + w].fill(0);
            before[o..o + w].fill(0);
        }
        let frame = (STACK_TOP - 0x8000_0000 - 0x18) as usize;
        after[frame..frame + 0x18].fill(0);
        before[frame..frame + 0x18].fill(0);
        assert_eq!(after, before, "{:?}", diff_offsets(&after, &before));

        // The setter on its own, any byte.
        let b = rng.next() as u8;
        assert_eq!(cpu.call(SET_MODE, &[u32::from(b) | 0xAB00]), Stop::Returned);
        assert_eq!(cpu.mem.rd8(0x8007_3F20), b);
    }
}

#[test]
fn the_block_copy_matches_retail() {
    differential!(scus(COPY));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    const SRC: u32 = 0x8010_0000;
    const DST: u32 = 0x8011_0000;
    let mut rng = Rng(0x434F_5059);
    for run in 0..500 {
        let count = rng.below(12) as i32 - 3;
        let src: Vec<u8> = (0..0x200).map(|_| rng.next() as u8).collect();
        let dst: Vec<u8> = (0..0x200).map(|_| rng.next() as u8).collect();
        cpu.mem.load(SRC, &src);
        cpu.mem.load(DST, &dst);
        assert_eq!(cpu.call(COPY, &[DST, SRC, count as u32]), Stop::Returned);
        let mut port = dst.clone();
        copy_blocks_32(&mut port, &src, count);
        let retail = cpu.mem.read(DST, 0x200);
        assert_eq!(
            retail,
            port,
            "run {run} count {count}: {:?}",
            diff_offsets(&retail, &port)
        );
    }
}

#[test]
fn the_clut_walk_spawn_matches_retail() {
    differential!(scus(CLUT_SPAWN));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    const PACK_PTR: u32 = 0x8007_B7C8;
    const PACK: u32 = 0x8014_0000;
    const DESC: u32 = 0x8015_0000;
    let give = std::rc::Rc::new(std::cell::Cell::new((true, 0, 0)));
    hook_alloc(&mut cpu, give.clone());
    let mut rng = Rng(0x434C_5554);
    let mut spawned = 0;
    for run in 0..2000 {
        let count = 1 + rng.below(12);
        let mut pack = count.to_le_bytes().to_vec();
        for _ in 0..count {
            pack.extend((rng.below(0x4000) & !3).to_le_bytes());
        }
        cpu.mem.load(PACK, &pack);
        cpu.mem.wr32(PACK_PTR, PACK);
        // The index truncates to 16 bits and sign-extends; keep it in the pack.
        let index = if rng.below(8) == 0 {
            -1
        } else {
            rng.below(count) as i16
        };
        let arg = (index as i32 as u32 & 0xFFFF) | (rng.next() & 0xFFFF_0000);
        let ok = rng.below(8) != 0;
        give.set((ok, 0, 0));
        let junk: Vec<u8> = (0..0x100).map(|_| rng.next() as u8).collect();
        cpu.mem.load(ACTOR, &junk);
        cpu.mem.wr16(DESC + 0x14, 0xBEEF);
        let list = rng.next();
        assert_eq!(cpu.call(CLUT_SPAWN, &[arg, DESC, list]), Stop::Returned);
        assert_eq!(give.get().1, DESC, "run {run}");
        assert_eq!(give.get().2, list, "run {run}");
        // Retail zeroes the descriptor's `+0x14` (the render mode the
        // allocator copies to `+0x56`) in the call's delay slot, spawn or not.
        assert_eq!(cpu.mem.rd16(DESC + 0x14), 0, "run {run}");
        if !ok {
            assert_eq!(cpu.mem.read(ACTOR, 0x100), junk);
            continue;
        }
        spawned += 1;
        let s = spawn_init(&pack, index).expect("index inside the pack");
        let mut port = junk.clone();
        let mut put = |off: usize, b: &[u8]| port[off..off + b.len()].copy_from_slice(b);
        put(0x4C, &(PACK + s.record_offset as u32).to_le_bytes());
        put(0x5C, &s.anim_cursor.to_le_bytes());
        put(0x56, &s.clip_kick.to_le_bytes());
        put(0x68, &s.frame_counter.to_le_bytes());
        put(0x6E, &s.field_6e.to_le_bytes());
        let retail = cpu.mem.read(ACTOR, 0x100);
        assert_eq!(
            retail,
            port,
            "run {run} index {index}: {:?}",
            diff_offsets(&retail, &port)
        );
    }
    assert!(spawned > 1500, "{spawned}");
}

#[test]
fn the_field_model_swap_matches_retail() {
    differential!(scus(SET_MODEL));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // The re-stage is the host's (`World` re-binds the live model); retail's
    // side records the flags word it was handed.
    let seen = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let s = seen.clone();
    cpu.hook(RESTAGE, move |r, m| {
        s.set(m.rd32(r.r[psx_retail_exec::A0] + 0x10));
        m.wr32(r.r[psx_retail_exec::A0] + 0x10, 0x5A5A_5A5A);
    });
    let mut rng = Rng(0x4D4F_4445);
    for run in 0..3000 {
        let flags = rng.next();
        let value = rng.next() as i16;
        let junk: Vec<u8> = (0..0x100).map(|_| rng.next() as u8).collect();
        cpu.mem.load(ACTOR, &junk);
        cpu.mem.wr32(ACTOR + 0x10, flags);
        // The field arm; the world-map arm (mode `0xF`) also seats `+0x60`
        // and keeps the `0x00108000` bits for the re-stage, which the
        // engine's world-map object path owns.
        cpu.mem
            .wr16(GAME_MODE, [3u16, 1, 0xE][rng.below(3) as usize]);
        assert_eq!(
            cpu.call(
                SET_MODEL,
                &[ACTOR, value as u16 as u32 | (rng.next() & 0xFFFF_0000)]
            ),
            Stop::Returned
        );
        let mut ctx = FieldCtx {
            flags,
            model_id: u16::from_le_bytes([junk[0x64], junk[0x65]]),
            move_id: u16::from_le_bytes([junk[0x5C], junk[0x5D]]),
            ..FieldCtx::default()
        };
        Inert { hit: true }.set_actor_model(&mut ctx, value, rng.below(2) == 0);
        let mut port = junk.clone();
        port[0x10..0x14].copy_from_slice(&ctx.flags.to_le_bytes());
        port[0x5C..0x5E].copy_from_slice(&ctx.move_id.to_le_bytes());
        port[0x64..0x66].copy_from_slice(&ctx.model_id.to_le_bytes());
        let retail = cpu.mem.read(ACTOR, 0x100);
        assert_eq!(
            retail,
            port,
            "run {run}: {:?}",
            diff_offsets(&retail, &port)
        );
        // The re-stage sees the draw bit cleared and `0x00108000` masked off.
        assert_eq!(seen.get(), ctx.flags & !0x0010_8000, "run {run}");
    }
}

/// Move op `0x16`'s `FUN_80024C80` (`jr ra; nop`) and the free-time hook
/// `FUN_80024DFC` (one load into `v0` / `v1`) store nothing; their ports
/// (`MoveHost::stub_16`, `ActorAllocator::on_actor_cleanup`) default to no
/// store either. Held as: the whole of RAM is unchanged by either call.
#[test]
fn the_two_storeless_bodies_store_nothing() {
    differential!(scus(STUB_16), scus(FREE_HOOK));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x4E4F_5045);
    for _ in 0..8 {
        let junk: Vec<u8> = (0..0x100).map(|_| rng.next() as u8).collect();
        cpu.mem.load(ACTOR, &junk);
        for entry in [STUB_16, FREE_HOOK] {
            let before = cpu.mem.read(0x8000_0000, 0x20_0000);
            assert_eq!(cpu.call(entry, &[ACTOR, rng.next()]), Stop::Returned);
            assert!(cpu.steps <= 3, "{entry:#x}: {} steps", cpu.steps);
            assert_eq!(cpu.mem.read(0x8000_0000, 0x20_0000), before, "{entry:#x}");
        }
        assert_eq!(
            cpu.regs.r[V0],
            u32::from(u16::from_le_bytes([junk[0x56], junk[0x57]]))
        );
    }
}
