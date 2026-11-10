//! The fog particle pass off the disc: the per-frame walk `FUN_8003F348`, the
//! per-particle step `FUN_8003F3FC` and the half-sheet emitter `FUN_8003F86C`
//! against `FogPool::render_step`, over random cameras built by the field
//! matrix build `FUN_800172C0` (the matrix at `0x1F8003C8` the pass reads),
//! pools of live and dead records, windows, player boxes, tints, frame steps
//! and both arms of the overworld bit. The pass builds its own billboard
//! matrix at `0x1F800334` from the base matrix `0x8007BF10`, which is seeded
//! `0x6000 * I` as the field leaves it.
//!
//! Compared after each pass: every record, the free stack, the live count,
//! and every packet the pass wrote - which were linked into the ordering
//! table, in which bucket, with which corners, texels and colour.

use crate::common::{Rng, scus_cpu};
use legaia_engine_field::fog_particles::{
    FOG_POOL_SLOTS, FOG_UV_ROWS, FogFrameEnv, FogParticle, FogPool, FogQuad, FogView,
};
use legaia_render_kernels::gte::{FIELD_OFX, FieldGteView};
use legaia_render_kernels::overworld_curvature::{REBUILD_BIAS, curvature_table};
use psx_retail_exec::{Cpu, Stop, V0};

const PASS: u32 = 0x8003_F348;
const STEP: u32 = 0x8003_F3FC;
const HALF: u32 = 0x8003_F86C;
const BUILD: u32 = 0x8001_72C0;
const ANGLES: u32 = 0x8007_B790;
const EYE: u32 = 0x8008_40B8;
const FOCUS: u32 = 0x8008_9118;
const BASE: u32 = 0x8007_BF10;
const POOL_PTR: u32 = 0x8007_B7E0;
const POOL: u32 = 0x8015_0000;
const STRIDE: u32 = 0x18;
const PLAYER_PTR: u32 = 0x8007_C364;
const PLAYER: u32 = 0x8016_0000;
const PAD: u32 = 0x8007_B850;
const DT: u32 = 0x1F80_0393;
const Y_OFFSET: u32 = 0x8007_BCAC;
const LIVE: u32 = 0x8007_BCA8;
const TINT: u32 = 0x8007_BCB8;
const WINDOW: u32 = 0x1F80_0384;
const FLAGS: u32 = 0x1F80_0394;
const CURVE_PTR: u32 = 0x8007_BB04;
const CURVE: u32 = 0x8017_0000;
const UV_ROWS: u32 = 0x1F80_02D0;
const OT_PTR: u32 = 0x1F80_03F4;
const OT: u32 = 0x8018_0000;
const OT_LEN: u32 = 0x800;
const MASK: u32 = 0x1F80_03FC;
const PRIM_PTR: u32 = 0x1F80_03A0;
const PRIMS: u32 = 0x8019_0000;
const PACKET: u32 = 0x28;
const SENTINEL: u32 = 0x00E0_0000;
const OFY: i32 = 114;
const LOAD_MATRIX: u32 = 0x8003_D1A4;
const TRANSFORM: u32 = 0x8003_D344;
const CAMERA: u32 = 0x1F80_03C8;
const SHEET: u32 = 0x1F80_0334;
const SVEC: u32 = 0x8010_0000;

fn retail_cpu() -> Option<Cpu> {
    let mut cpu = scus_cpu()?;
    cpu.mem.wr32(0x8007_B81C, 0x8007_0A2C);
    cpu.mem.wr32(0x8007_B7F8, 0x8007_122C);
    for k in 0..3u32 {
        cpu.mem.wr16(BASE + 8 * k, 0x6000);
    }
    cpu.mem.wr32(POOL_PTR, POOL);
    cpu.mem.wr32(PLAYER_PTR, PLAYER);
    cpu.mem.wr32(CURVE_PTR, CURVE);
    cpu.mem.wr32(MASK, 0x00FF_FFFF);
    cpu.mem.wr32(OT_PTR, OT);
    for (r, row) in FOG_UV_ROWS.iter().enumerate() {
        for (k, w) in row.iter().enumerate() {
            cpu.mem.wr32(UV_ROWS + 16 * r as u32 + 4 * k as u32, *w);
        }
    }
    Some(cpu)
}

/// A random record near `focus`, alive or dead.
fn record(rng: &mut Rng, slot: u8, focus: [i32; 3], spread: u32) -> FogParticle {
    let off: [i32; 3] = std::array::from_fn(|_| rng.below(2 * spread) as i32 - spread as i32);
    let age = match rng.below(4) {
        0 => rng.below(0x400) as u16,
        1 => 0x400 + rng.below(0x800) as u16,
        2 => 0xC00 + rng.below(0x420) as u16,
        _ => rng.next() as u16,
    };
    FogParticle {
        age,
        rate: if rng.below(16) == 0 {
            rng.next() as u16
        } else {
            8 + rng.below(8) as u16
        },
        slot,
        alive: rng.below(4) != 0,
        vx: rng.next() as i8,
        vz: rng.next() as i8,
        x: (focus[0] + off[0]) << 4 | rng.below(16) as i32,
        z: (focus[2] + off[2]) << 4 | rng.below(16) as i32,
        y: (focus[1] + off[1] / 8) as i16,
        grey: rng.below(0x80) as u8,
    }
}

fn write_record(cpu: &mut Cpu, r: &FogParticle) {
    let at = POOL + 0xA4 + u32::from(r.slot) * STRIDE;
    cpu.mem.wr16(at, r.age);
    cpu.mem.wr16(at + 2, r.rate);
    cpu.mem.wr8(at + 4, r.slot);
    cpu.mem.wr8(at + 5, u8::from(r.alive));
    cpu.mem.wr8(at + 6, r.vx as u8);
    cpu.mem.wr8(at + 7, r.vz as u8);
    cpu.mem.wr32(at + 8, r.x as u32);
    cpu.mem.wr32(at + 0xC, r.z as u32);
    cpu.mem.wr16(at + 0x10, r.y as u16);
    cpu.mem.wr16(at + 0x12, 1);
    for k in 0..3 {
        cpu.mem.wr8(at + 0x14 + k, r.grey);
    }
}

/// The packets the pass linked, in the order it wrote them, each with the
/// bucket it sits in.
fn linked_packets(cpu: &mut Cpu, written: u32) -> Vec<(u32, u32)> {
    let mut bucket_of = std::collections::HashMap::new();
    for i in 0..OT_LEN {
        let mut at = cpu.mem.rd32(OT + 4 * i) & 0x00FF_FFFF;
        let mut hops = 0;
        while at != SENTINEL | i {
            bucket_of.insert(at, i);
            at = cpu.mem.rd32(0x8000_0000 | at) & 0x00FF_FFFF;
            hops += 1;
            assert!(hops < 1000, "OT bucket {i} does not end");
        }
    }
    (0..written)
        .map(|k| PRIMS + k * PACKET)
        .filter_map(|p| bucket_of.get(&(p & 0x00FF_FFFF)).map(|&b| (p, b)))
        .collect()
}

fn check_quad(cpu: &mut Cpu, packet: u32, bucket: u32, q: &FogQuad, label: &str) {
    let rgb = cpu.mem.rd32(packet + 4);
    assert_eq!(rgb >> 24, 0x2E, "{label}: command");
    assert_eq!(
        [rgb as u8, (rgb >> 8) as u8, (rgb >> 16) as u8],
        q.rgb,
        "{label}: colour"
    );
    let xy: Vec<(i16, i16)> = (0..4)
        .map(|k| {
            let w = cpu.mem.rd32(packet + 8 + 8 * k);
            (w as i16, (w >> 16) as i16)
        })
        .collect();
    assert_eq!(xy, q.xy, "{label}: corners");
    let uv: Vec<u32> = (0..4).map(|k| cpu.mem.rd32(packet + 0xC + 8 * k)).collect();
    let row = FOG_UV_ROWS
        .iter()
        .position(|r| r[..] == uv[..])
        .expect("a staged UV row");
    let want: [(u8, u8); 4] = FOG_UV_ROWS[row].map(|w| (w as u8, (w >> 8) as u8));
    assert_eq!(want, q.uv, "{label}: texels");
    assert_eq!(
        ((uv[0] >> 16) as u16, (uv[1] >> 16) as u16),
        (q.clut, q.tpage),
        "{label}: clut / tpage"
    );
    assert_eq!(bucket, q.ot_index, "{label}: bucket");
}

#[test]
fn the_fog_pass_matches_retail() {
    differential!(scus(PASS), scus(STEP), scus(HALF));
    let Some(mut cpu) = retail_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x464F_4752);
    let (mut drawn, mut kept, mut freed, mut behind, mut overworld_drawn) = (0, 0, 0, 0, 0);
    let (mut steps, mut halves) = (0, 0);
    for round in 0..1500 {
        // The camera, built by retail and by the port from the same globals.
        let angles: [u16; 3] = [
            rng.below(0x400) as u16 + 0xE00,
            rng.next() as u16 & 0xFFF,
            if rng.below(4) == 0 {
                rng.next() as u16
            } else {
                0
            },
        ];
        let eye = [
            rng.below(0x200) as i32 - 0x100,
            rng.below(0x400) as i32 - 0x200,
            0x4000 + rng.below(0x10000) as i32,
        ];
        let focus = [
            rng.below(0x3000) as i32,
            -(rng.below(0x200) as i32),
            rng.below(0x3000) as i32,
        ];
        let neg_focus = focus.map(|c| -c);
        for k in 0..3 {
            cpu.mem.wr16(ANGLES + 2 * k as u32, angles[k]);
            cpu.mem.wr32(EYE + 4 * k as u32, eye[k] as u32);
            cpu.mem.wr32(FOCUS + 4 * k as u32, neg_focus[k] as u32);
        }
        assert_eq!(cpu.call(BUILD, &[]), Stop::Returned);
        let h = 0x100 + rng.below(0x200) as u16;
        cpu.gte.write_ctrl(24, (FIELD_OFX << 16) as u32);
        cpu.gte.write_ctrl(25, (OFY << 16) as u32);
        cpu.gte.write_ctrl(26, u32::from(h));
        let gte = FieldGteView::from_globals(
            angles,
            eye,
            neg_focus,
            &FieldGteView::field_base(),
            h,
            FIELD_OFX,
            OFY,
        );

        // The frame's inputs.
        let overworld = round % 3 == 0;
        let curve_bias = if overworld { REBUILD_BIAS } else { 0 };
        let tile = |c: i32| (c >> 7).clamp(0, 0xFF) as u8;
        let window = [
            tile(focus[0] - 0x800 - rng.below(0x400) as i32),
            tile(focus[2] - 0x800 - rng.below(0x400) as i32),
            tile(focus[0] + 0x800 + rng.below(0x400) as i32),
            tile(focus[2] + 0x800 + rng.below(0x400) as i32),
        ];
        let env = FogFrameEnv {
            dt: 1 + rng.below(4) as u8,
            player: [
                focus[0] + rng.below(0x400) as i32 - 0x200,
                focus[1] + rng.below(0x100) as i32 - 0x80,
                focus[2] + rng.below(0x400) as i32 - 0x200,
            ],
            dpad_held: rng.below(2) == 0,
            y_offset: rng.below(0x100) as i32 - 0x80,
            tint: std::array::from_fn(|_| [0x80, rng.next() as u8][rng.below(2) as usize]),
            window,
        };
        cpu.mem.wr8(DT, env.dt);
        for k in 0..3 {
            cpu.mem
                .wr16(PLAYER + 0x14 + 2 * k, env.player[k as usize] as u16);
            cpu.mem.wr8(TINT + k, env.tint[k as usize]);
        }
        cpu.mem.wr32(
            PAD,
            if env.dpad_held {
                0x1000 << rng.below(4)
            } else {
                0
            },
        );
        cpu.mem.wr32(Y_OFFSET, env.y_offset as u32);
        cpu.mem.load(WINDOW, &window);
        cpu.mem.wr32(FLAGS, u32::from(overworld));
        if overworld && round == 0 {
            for (i, v) in curvature_table().iter().enumerate() {
                cpu.mem
                    .wr16(CURVE + 2 * i as u32, v.wrapping_add(curve_bias) as u16);
            }
        }

        // The pool.
        let spread = [0x400u32, 0x1000, 0x4000][rng.below(3) as usize];
        let records: Vec<FogParticle> = (0..FOG_POOL_SLOTS as u8)
            .map(|slot| record(&mut rng, slot, focus, spread))
            .collect();
        let live: Vec<FogParticle> = records.iter().filter(|r| r.alive).copied().collect();
        let mut pool = FogPool::new();
        pool.install_snapshot(&live);
        for r in &records {
            if !r.alive {
                pool.records[usize::from(r.slot)] = *r;
            }
        }
        pool.overworld = overworld;
        pool.curve_bias = curve_bias;
        let (top, table) = pool.free_stack();
        cpu.mem.wr16(POOL, top as u16);
        cpu.mem.wr16(POOL + 2, FOG_POOL_SLOTS as u16);
        for (i, v) in table.iter().enumerate() {
            cpu.mem.wr16(POOL + 4 + 2 * i as u32, *v as u16);
        }
        for r in &pool.records {
            write_record(&mut cpu, r);
        }
        for i in 0..OT_LEN {
            cpu.mem.wr32(OT + 4 * i, SENTINEL | i);
        }
        cpu.mem.wr32(PRIM_PTR, PRIMS);

        if round % 2 == 0 {
            assert_eq!(cpu.call(PASS, &[]), Stop::Returned, "round {round}");
        } else {
            // The pass's setup alone (an empty walk), then the step on each
            // live record in slot order, summed as the pass sums it.
            cpu.mem.wr16(POOL + 2, 0);
            assert_eq!(cpu.call(PASS, &[]), Stop::Returned, "round {round}");
            cpu.mem.wr16(POOL + 2, FOG_POOL_SLOTS as u16);
            let mut live = 0u32;
            for slot in 0..FOG_POOL_SLOTS as u32 {
                let at = POOL + 0xA4 + slot * STRIDE;
                if cpu.mem.rd8(at + 5) != 0 {
                    assert_eq!(cpu.call(STEP, &[at]), Stop::Returned, "round {round}");
                    live += cpu.regs.r[V0];
                }
            }
            cpu.mem.wr32(LIVE, live);
            steps += 1;
        }
        let view = FogView::from_vp(std::array::from_fn(|i| f32::from(u8::from(i % 5 == 0))))
            .with_gte(gte);
        let quads = pool.render_step(&view, &env).to_vec();
        let label = format!("round {round} overworld {overworld} angles {angles:x?} eye {eye:?}");

        for r in &pool.records {
            let at = POOL + 0xA4 + u32::from(r.slot) * STRIDE;
            let retail = (
                cpu.mem.rd16(at),
                cpu.mem.rd8(at + 5) != 0,
                cpu.mem.rd32(at + 8) as i32,
                cpu.mem.rd32(at + 0xC) as i32,
            );
            assert_eq!(
                retail,
                (r.age, r.alive, r.x, r.z),
                "{label} record {}",
                r.slot
            );
        }
        assert_eq!(cpu.mem.rd32(LIVE), u32::from(pool.live), "{label}: live");
        let (top, table) = pool.free_stack();
        assert_eq!(cpu.mem.rd16(POOL) as i16, top, "{label}: stack top");
        for (i, v) in table.iter().enumerate().take((top + 1).max(0) as usize) {
            assert_eq!(
                cpu.mem.rd16(POOL + 4 + 2 * i as u32) as i16,
                *v,
                "{label}: stack {i}"
            );
        }
        let written = (cpu.mem.rd32(PRIM_PTR) - PRIMS) / PACKET;
        let linked = linked_packets(&mut cpu, written);
        assert_eq!(linked.len(), quads.len(), "{label}: quads drawn");
        for (k, ((packet, bucket), q)) in linked.iter().zip(&quads).enumerate() {
            check_quad(&mut cpu, *packet, *bucket, q, &format!("{label} quad {k}"));
        }
        drawn += quads.len();
        overworld_drawn += if overworld { quads.len() } else { 0 };
        kept += written as usize - quads.len();
        freed += live.len() - usize::from(pool.live);
        behind += live
            .iter()
            .filter(|r| gte.rtps([(r.x >> 4) as i16, 0, (r.z >> 4) as i16]).mac[2] <= 0)
            .count();

        // The emitter on its own: one particle through the camera into the
        // billboard's translation (`FUN_8003D344`), then one half.
        for _ in 0..4 {
            let r = record(&mut rng, 0, focus, spread);
            let p = [r.x >> 4, i32::from(r.y) - env.y_offset, r.z >> 4];
            let hw = FogParticle::half_width(r.age, r.rate);
            let left = rng.below(2) == 0;
            let (a0, dx0, dx1, row) = if left {
                (-hw, -2 * hw, 0, 1)
            } else {
                (hw, 0, 2 * hw, 0)
            };
            for (k, v) in p.iter().enumerate() {
                cpu.mem.wr16(SVEC + 2 * k as u32, *v as u16);
            }
            assert_eq!(cpu.call(LOAD_MATRIX, &[CAMERA]), Stop::Returned);
            assert_eq!(cpu.call(TRANSFORM, &[SVEC, SHEET + 0x14]), Stop::Returned);
            assert_eq!(cpu.call(LOAD_MATRIX, &[SHEET]), Stop::Returned);
            for i in 0..OT_LEN {
                cpu.mem.wr32(OT + 4 * i, SENTINEL | i);
            }
            cpu.mem.wr32(PRIM_PTR, PRIMS + PACKET);
            let packet = PRIMS;
            cpu.mem.wr32(packet, 0x0900_0000);
            assert_eq!(
                cpu.call(HALF, &[a0 as u32, 0, hw as u32, UV_ROWS + 16 * row as u32]),
                Stop::Returned
            );
            let ret = cpu.regs.r[V0];
            let ow = overworld.then_some(curve_bias);
            let (keep, quad) =
                legaia_engine_field::fog_particles::emit_half(&view, p, dx0, dx1, row, [0; 3], ow);
            let label = format!("{label} half {p:?} hw {hw} left {left}");
            assert_eq!(ret != 0, keep, "{label}: kept");
            let linked = linked_packets(&mut cpu, 1);
            assert_eq!(linked.len(), usize::from(quad.is_some()), "{label}: drawn");
            if let (Some((packet, bucket)), Some(q)) = (linked.first(), quad) {
                cpu.mem.wr32(packet + 4, 0x2E00_0000);
                check_quad(&mut cpu, *packet, *bucket, &q, &label);
            }
            halves += 1;
        }
    }
    assert!(steps > 500 && halves > 5000, "{steps} {halves}");
    assert!(
        drawn > 20_000 && overworld_drawn > 3_000 && kept > 5_000 && freed > 5_000 && behind > 500,
        "drawn {drawn} overworld {overworld_drawn} undrawn {kept} freed {freed} behind {behind}"
    );
}
