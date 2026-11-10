//! The world-map horizon emitter `FUN_801D7EA0` (field overlay, PROT 0897)
//! against `legaia_engine_field::world_map_horizon::emit_horizon`, compared at
//! packet level: every primitive the routine links (two `POLY_FT4`, a
//! `LINE_F2` and a `DR_MOVE` per band) is read back off the ordering-table
//! bucket it chains into and checked word for word against the packet the
//! port's band describes. libgpu's `SetDrawMove` and `AddPrim` run as the
//! retail executable has them.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_field::world_map_horizon::{BAND_COUNT, HorizonBand, emit_horizon};
use psx_retail_exec::Stop;

const OVERLAY_BASE: u32 = 0x801C_E818;
const HORIZON: u32 = 0x801D_7EA0;

const GATE: u32 = 0x801F_351C;
const SCALE: u32 = 0x801F_3520;
const ANGLE: u32 = 0x801F_3518;
const STEP: u32 = 0x801F_3524;
const LAYER: u32 = 0x801F_3528;
const ALT_BAND: u32 = 0x8007_B74C;
const TRIG_PTR: u32 = 0x8007_B81C;
const FRAME_STEP: u32 = 0x1F80_0393;
const CURSOR: u32 = 0x1F80_03A0;
const OT_BASE: u32 = 0x1F80_03F4;

const TRIG: u32 = 0x8011_0000;
const PACKETS: u32 = 0x8012_0000;
const OT: u32 = 0x8018_0000;
const OT_LEN: u32 = 16;
const OT_END: u32 = 0x00FF_FFFF;
const FILL: u8 = 0xA5;

/// The packets one band stands for, as the GPU reads them, with each tag's
/// link field left out (the chain is checked on its own).
fn band_packets(b: &HorizonBand) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    for q in &b.quads {
        let mut p = vec![0u8; 0x28];
        p[3] = 9;
        p[4..8].copy_from_slice(&q.code_colour.to_le_bytes());
        let xy = [
            (q.x_left, q.y_top),
            (q.x_right, q.y_top),
            (q.x_left, q.y_bottom),
            (q.x_right, q.y_bottom),
        ];
        for (v, ((x, y), (u, w))) in xy.iter().zip(q.uv).enumerate() {
            let at = 8 + 8 * v;
            p[at..at + 2].copy_from_slice(&x.to_le_bytes());
            p[at + 2..at + 4].copy_from_slice(&y.to_le_bytes());
            p[at + 4] = u;
            p[at + 5] = w;
        }
        // `+0xE` (the CLUT) and `+0x26` are never written.
        p[0xE] = FILL;
        p[0xF] = FILL;
        p[0x16..0x18].copy_from_slice(&q.tpage.to_le_bytes());
        p[0x1E] = FILL;
        p[0x1F] = FILL;
        p[0x26] = FILL;
        p[0x27] = FILL;
        out.push(p);
    }
    let mut l = vec![0u8; 0x10];
    l[3] = 3;
    l[4..8].copy_from_slice(&b.line.code_colour.to_le_bytes());
    l[8..10].copy_from_slice(&b.line.x0.to_le_bytes());
    l[10..12].copy_from_slice(&b.line.y.to_le_bytes());
    l[12..14].copy_from_slice(&b.line.x1.to_le_bytes());
    l[14..16].copy_from_slice(&b.line.y.to_le_bytes());
    out.push(l);
    let m = b.blit;
    let mut d = vec![0u8; 0x18];
    d[3] = if m.width == 0 || m.height == 0 { 0 } else { 5 };
    d[4..8].copy_from_slice(&0x0100_0000u32.to_le_bytes());
    d[8..12].copy_from_slice(&0x8000_0000u32.to_le_bytes());
    d[0xE..0x10].copy_from_slice(&m.src_y.to_le_bytes());
    d[0x10..0x12].copy_from_slice(&m.dst_x.to_le_bytes());
    d[0x12..0x14].copy_from_slice(&m.dst_y.to_le_bytes());
    d[0x14..0x16].copy_from_slice(&m.width.to_le_bytes());
    d[0x16..0x18].copy_from_slice(&m.height.to_le_bytes());
    out.push(d);
    out
}

#[test]
fn the_horizon_emitter_matches_retail_at_packet_level() {
    differential!(prot(897, HORIZON));
    let Some(disc) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut cpu = scus_cpu().expect("disc present");
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(897).expect("PROT 0897"));
    let mut rng = Rng(0x484F_5249);
    let trig: Vec<i16> = (0..0x1000)
        .map(|i| match i % 7 {
            0 => 0x1000,
            1 => -0x1000,
            _ => rng.next() as i16,
        })
        .collect();
    for (i, s) in trig.iter().enumerate() {
        cpu.mem.wr16(TRIG + 2 * i as u32, *s as u16);
    }
    cpu.mem.wr32(TRIG_PTR, TRIG);
    let sizes = [0x28u32, 0x28, 0x10, 0x18];
    let band_bytes: u32 = sizes.iter().sum();

    for round in 0..60 {
        let scale = match rng.below(4) {
            0 => rng.below(0x800) as i32,
            1 => -(rng.below(0x800) as i32),
            2 => rng.below(0x10_0000) as i32,
            _ => rng.next() as i32 >> rng.below(12),
        };
        let angle = rng.next();
        let step = rng.next() >> rng.below(24);
        let delta = rng.below(5) as u8;
        let layer = rng.below(OT_LEN);
        let alt = rng.below(2) == 0;

        cpu.mem.wr32(GATE, 1);
        cpu.mem.wr32(SCALE, scale as u32);
        cpu.mem.wr32(ANGLE, angle);
        cpu.mem.wr32(STEP, step);
        cpu.mem.wr32(LAYER, layer);
        cpu.mem
            .wr16(ALT_BAND, if alt { rng.below(0x100) as u16 + 1 } else { 0 });
        cpu.mem.wr8(FRAME_STEP, delta);
        cpu.mem.wr32(CURSOR, PACKETS);
        cpu.mem.wr32(OT_BASE, OT);
        for i in 0..OT_LEN {
            cpu.mem.wr32(OT + 4 * i, OT_END);
        }
        for o in 0..band_bytes * BAND_COUNT as u32 + 0x40 {
            cpu.mem.wr8(PACKETS + o, FILL);
        }
        assert_eq!(cpu.call(HORIZON, &[]), Stop::Returned, "round {round}");

        let batch = emit_horizon(scale, angle, step, delta, layer, alt, &|i| {
            trig[usize::from(i)]
        });
        let ctx =
            format!("round {round} scale {scale:#x} angle {angle:#x} step {step:#x} delta {delta}");
        assert_eq!(cpu.mem.rd32(GATE), 0, "the gate clears: {ctx}");
        assert_eq!(cpu.mem.rd32(ANGLE), batch.angle_after, "angle: {ctx}");
        assert_eq!(batch.ot_layer, layer);
        let end = cpu.mem.rd32(CURSOR);
        assert_eq!(
            end,
            PACKETS + band_bytes * BAND_COUNT as u32,
            "cursor: {ctx}"
        );
        assert_eq!(cpu.mem.rd8(end), FILL, "nothing past the cursor: {ctx}");

        // The chain: newest first, all in the one bucket.
        let mut at = Vec::new();
        let mut p = PACKETS;
        for _ in 0..BAND_COUNT {
            for s in sizes {
                at.push(p);
                p += s;
            }
        }
        let mut chain = Vec::new();
        let mut link = cpu.mem.rd32(OT + 4 * layer) & 0xFF_FFFF;
        while link != OT_END {
            chain.push(0x8000_0000 | link);
            link = cpu.mem.rd32(0x8000_0000 | link) & 0xFF_FFFF;
            assert!(chain.len() <= at.len(), "a loop in the chain: {ctx}");
        }
        let newest_first: Vec<u32> = at.iter().rev().copied().collect();
        assert_eq!(chain, newest_first, "the bucket's chain: {ctx}");
        for i in (0..OT_LEN).filter(|&i| i != layer) {
            assert_eq!(cpu.mem.rd32(OT + 4 * i), OT_END, "bucket {i}: {ctx}");
        }

        let want: Vec<Vec<u8>> = batch.bands.iter().flat_map(band_packets).collect();
        for (k, (&va, w)) in at.iter().zip(&want).enumerate() {
            let mut got: Vec<u8> = (0..w.len() as u32).map(|o| cpu.mem.rd8(va + o)).collect();
            got[..3].fill(0);
            assert_eq!(got, *w, "band {} packet {}: {ctx}", k / 4, k % 4);
        }
    }
}
