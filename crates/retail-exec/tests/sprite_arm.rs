//! The render dispatcher's `0x4000` sprite arm `FUN_8002A5A4` off the disc:
//! the one-quad model it builds out of a draw node's `+0x9C` block, over
//! random blocks and every mode word, against `sprite_arm_quad`. The output
//! is compared whole: the object header, the four vertices, the primitive
//! group and its packet, and the zeroed tail, laid out from the port's quad.

use crate::common::{Rng, diff_offsets, scus_cpu};
use legaia_engine_vm::move_vm::ActorState;
use legaia_render_kernels::sprite_arm::{SpriteArmQuad, sprite_arm_quad};
use psx_retail_exec::Stop;

const ARM: u32 = 0x8002_A5A4;
const OUT: u32 = 0x8010_0000;
const BLOCK: u32 = 0x8011_0000;
const FILL: u8 = 0xA5;
const OUT_LEN: usize = 0x48 + 8 + 0x24 + 0x50 + 0x10;

/// The model `FUN_8002A5A4` writes for `q` at `OUT`, over `before`: what no
/// store reaches keeps its bytes.
fn lay_out(q: &SpriteArmQuad, before: &[u8]) -> Vec<u8> {
    let mut b = before.to_vec();
    let w32 = |b: &mut Vec<u8>, at: usize, v: u32| b[at..at + 4].copy_from_slice(&v.to_le_bytes());
    let w16 = |b: &mut Vec<u8>, at: usize, v: u16| b[at..at + 2].copy_from_slice(&v.to_le_bytes());
    // The object header: no normals, four vertices, one primitive group.
    w32(&mut b, 0x00, 0);
    w32(&mut b, 0x04, 0);
    w32(&mut b, 0x08, 0);
    w32(&mut b, 0x0C, OUT + 0x28);
    w32(&mut b, 0x10, 4);
    w32(&mut b, 0x18, 0);
    w32(&mut b, 0x1C, OUT + 0x48);
    w32(&mut b, 0x20, 1);
    for (i, v) in q.verts.iter().enumerate() {
        for (k, c) in v.iter().enumerate() {
            w16(&mut b, 0x28 + 8 * i + 2 * k, *c as u16);
        }
    }
    let (kind, words, code) = if q.gouraud {
        (0x26u16, 9u8, 0x3Cu8)
    } else {
        (0x22, 6, 0x2C)
    };
    w16(&mut b, 0x48, 1);
    w16(&mut b, 0x4A, kind);
    b[0x4D] = words;
    b[0x4F] = code;
    let colour = u32::from(q.rgb[0]) | u32::from(q.rgb[1]) << 8 | u32::from(q.rgb[2]) << 16;
    let colour = colour | u32::from(code) << 24;
    let uv = |k: usize| u32::from(q.uvs[k][0]) | u32::from(q.uvs[k][1]) << 8;
    let mut body = vec![colour];
    if q.gouraud {
        body.extend([colour; 3]);
    }
    body.extend([
        uv(0) | u32::from(q.clut) << 16,
        uv(1) | u32::from(q.tpage) << 16,
        uv(2) | uv(3) << 16,
        0x0008_0000,
        0x0018_0010,
    ]);
    let mut at = 0x50;
    for w in &body {
        w32(&mut b, at, *w);
        at += 4;
    }
    // The tail: twenty words zeroed past the packet.
    b[at..at + 0x50].fill(0);
    b
}

#[test]
fn the_sprite_arm_quad_matches_the_builder() {
    differential!(scus(ARM));
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    let mut rng = Rng(0x5350_5254);
    let (mut planes, mut flat) = ([0usize; 3], 0usize);
    for round in 0..6000 {
        let mut block = [0u8; 0x1C];
        for b in block.iter_mut() {
            *b = rng.next() as u8;
        }
        // Mostly texel halfwords inside a byte; every fourth round lets them
        // carry, as the unmasked `addu` chain does.
        if round % 4 != 0 {
            for off in [0x0C, 0x0E, 0x10, 0x12] {
                block[off + 1] = 0;
            }
        }
        let mode = match rng.below(4) {
            0 => rng.next() as u16,
            _ => (rng.below(3) | rng.below(8) << 3) as u16,
        };
        if mode & 3 == 3 {
            continue;
        }
        cpu.mem.load(BLOCK, &block);
        let before = vec![FILL; OUT_LEN];
        cpu.mem.load(OUT, &before);
        assert_eq!(
            cpu.call(ARM, &[OUT, u32::from(mode), 0, BLOCK]),
            Stop::Returned
        );

        let mut st = ActorState::new();
        for k in (0..block.len()).step_by(2) {
            st.set_actor_u16(0x9C + k, u16::from_le_bytes([block[k], block[k + 1]]));
        }
        let q = sprite_arm_quad(mode, &st).expect("planes 0..=2");
        let want = lay_out(&q, &before);
        let got = cpu.mem.read(OUT, OUT_LEN);
        assert_eq!(
            got,
            want,
            "round {round} mode {mode:#x}: {:?}",
            diff_offsets(&got, &want)
        );
        planes[usize::from(mode & 3)] += 1;
        flat += usize::from(!q.gouraud);
    }
    assert!(
        planes.iter().all(|p| *p > 0) && flat > 0,
        "{planes:?} flat {flat}"
    );
}
