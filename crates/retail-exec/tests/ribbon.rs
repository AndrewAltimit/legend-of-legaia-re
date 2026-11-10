//! The bolt-ribbon emitter `FUN_801CFA48`, with the overlay generator
//! `FUN_801D0290` live beneath it, against `build_ribbon` over
//! `OverlayRng` (seeded as `ribbon_mesh_for_actor` seeds it) and
//! `RetailTrig`: the three planes, packed counts with and without a
//! suppressed head, wander spreads, radii, step lengths, turn rates and
//! seeds. Compared: the object header the emitter writes at `out + 0xC`,
//! every vertex it writes (one step past the drawn count), the generator's
//! state word `0x801F6950`, and the packet chain's quads against
//! `ribbon_quads`. The generator also runs on its own against
//! `OverlayRng::draw`.

use crate::common::{Rng, diff_offsets, disc, scus_cpu};
use legaia_engine_battle_vm::battle_action::OverlayRng;
use legaia_engine_effects::effect_ribbon::{
    RIBBON_VERT_STRIDE, RIBBON_VERTS_PER_STEP, RetailTrig, RibbonParams, build_ribbon, ribbon_quads,
};
use psx_retail_exec::Stop;

const EMIT: u32 = 0x801C_FA48;
const DRAW: u32 = 0x801D_0290;
const OVERLAY_BASE: u32 = 0x801C_E818;
const RNG_STATE: u32 = 0x801F_6950;
const SIN_PTR: u32 = 0x8007_B81C;
const COS_PTR: u32 = 0x8007_B7F8;
const SINE_TABLE: u32 = 0x8007_0A2C;
const OUT: u32 = 0x8010_0000;
const SRC: u32 = 0x8013_0000;

#[test]
fn the_ribbon_emitter_walks_as_retail_walks() {
    differential!(prot(898, EMIT));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    cpu.mem.wr32(SIN_PTR, SINE_TABLE);
    cpu.mem.wr32(COS_PTR, SINE_TABLE + 0x800);
    // A suppressed head step zeroes the radius ahead of the far draw's
    // `divu` (`0x801CFD70`, `0x801CFDF0`), which has no `break` guard: the
    // remainder is the raw draw, and the step zeroes the offset it feeds.
    cpu.hardware_zero_div = true;
    let mut rng = Rng(0x5249_4242);
    let (mut suppressed, mut planes) = (0, [0u32; 3]);
    for n in 0..3000 {
        let mode = rng.below(3) | (rng.next() & 0xFFFC);
        let count = 1 + rng.below(0x20);
        let total = match rng.below(3) {
            0 => 0,
            _ => rng.below(0x30),
        };
        let packed = count | (total << 8);
        let params = RibbonParams {
            wander_spread: 1 + rng.below(0x200) as u16,
            radius: 2 + rng.below(0x200) as i16,
            step_len: 2 + rng.below(0x400) as i16,
            rng_seed: rng.next() as i16,
            turn_rate: rng.next() as i16 >> 4,
        };
        let core = rng.next();
        let flare = rng.next();
        cpu.mem.wr32(SRC + 4, core);
        cpu.mem.wr32(SRC + 8, flare);
        cpu.mem.wr16(SRC + 0x0C, params.wander_spread);
        cpu.mem.wr16(SRC + 0x18, params.radius as u16);
        cpu.mem.wr16(SRC + 0x1A, params.step_len as u16);
        cpu.mem.wr16(SRC + 0x1C, params.rng_seed as u16);
        cpu.mem.wr16(SRC + 0x1E, params.turn_rate as u16);
        cpu.mem.load(OUT, &[0xEE; 0x4000]);
        assert_eq!(
            cpu.call(EMIT, &[OUT, mode, packed, SRC]),
            Stop::Returned,
            "n {n}"
        );

        let mut overlay = OverlayRng::new((i32::from(params.rng_seed) >> 2) as u32);
        let ribbon = build_ribbon(mode, packed, params, &RetailTrig, || overlay.draw());
        let p = &ribbon.packets;
        let header = OUT + 0xC;
        assert_eq!(
            (
                cpu.mem.rd32(header) - OUT,
                cpu.mem.rd32(header + 4) as usize,
                cpu.mem.rd32(header + 0x10) - OUT,
                cpu.mem.rd32(header + 0x14) as usize,
            ),
            (
                p.vertex_base as u32,
                p.allocated_verts,
                p.packet_base as u32,
                p.emitted_verts
            ),
            "n {n} packed {packed:#x}"
        );
        // Every step walked, in its plane.
        let (ox, oy, oz) = ribbon.plane.component_offsets();
        let mut want = Vec::new();
        for step in &ribbon.steps {
            for (x, y) in step.verts {
                let mut v = [0u8; RIBBON_VERT_STRIDE];
                v[ox..ox + 2].copy_from_slice(&x.to_le_bytes());
                v[oy..oy + 2].copy_from_slice(&y.to_le_bytes());
                v[oz..oz + 2].copy_from_slice(&0i16.to_le_bytes());
                // The fourth halfword is not written.
                v[6..8].copy_from_slice(&[0xEE, 0xEE]);
                want.extend(v);
            }
        }
        let got = cpu.mem.read(OUT + p.vertex_base as u32, want.len());
        assert!(
            got == want,
            "n {n} mode {mode:#x} packed {packed:#x} {params:?}: {:?}",
            diff_offsets(&got, &want)
        );
        assert_eq!(cpu.mem.rd32(RNG_STATE), overlay.state(), "n {n}");

        // The packet chain: four vertex byte offsets per quad (index times the
        // vertex stride) at `+0x1C` / `+0x20`.
        let quads = ribbon_quads(&ribbon);
        let base = OUT + p.packet_base as u32 + p.packet_header_bytes as u32;
        for (k, q) in quads.iter().enumerate() {
            let at = base + (k * p.packet_stride) as u32;
            let idx = [
                cpu.mem.rd16(at + 0x1C),
                cpu.mem.rd16(at + 0x1E),
                cpu.mem.rd16(at + 0x20),
                cpu.mem.rd16(at + 0x22),
            ];
            let want = q.verts.map(|v| (v * RIBBON_VERT_STRIDE) as u16);
            assert_eq!(idx, want, "n {n} quad {k}");
        }
        assert_eq!(
            ribbon.steps.len() * RIBBON_VERTS_PER_STEP,
            p.emitted_verts + RIBBON_VERTS_PER_STEP
        );
        if total > count {
            suppressed += 1;
        }
        planes[(mode & 3) as usize] += 1;
    }
    assert!(
        suppressed > 500 && planes.iter().all(|&c| c > 500),
        "{suppressed} {planes:?}"
    );
}

#[test]
fn the_overlay_generator_steps_as_retail_steps() {
    differential!(prot(898, DRAW));
    let (Some(mut cpu), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    let mut rng = Rng(0x4F52_4E47);
    for n in 0..20_000 {
        let seed = rng.next();
        cpu.mem.wr32(RNG_STATE, seed);
        let mut port = OverlayRng::new(seed);
        for k in 0..4 {
            assert_eq!(cpu.call(DRAW, &[]), Stop::Returned);
            let drawn = port.draw();
            assert_eq!(
                (cpu.regs.r[psx_retail_exec::V0], cpu.mem.rd32(RNG_STATE)),
                (drawn, port.state()),
                "n {n} draw {k} seed {seed:#x}"
            );
        }
    }
}
