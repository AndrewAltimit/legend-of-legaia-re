//! The body pair a battle framing read when the displayed frame was built.
//!
//! The pose decoder `FUN_8004998C` leaves every drawn actor's body pair
//! `+0x3C` / `+0x40` - the live pair `+0x34` / `+0x38` plus the pose
//! centroid turned by the heading (`0x8004A3DC..0x8004A5F8`) - and the
//! framings that focus on a body read it from the action SM, which ticks
//! before the draw: a pass frames the pair the previous pass's decode left.
//! The frame on screen is two passes behind the RAM's cursor
//! ([`DISPLAY_LAG_PASSES`]), so its framing read the pair of the pose three
//! passes behind it - slot `3` of the actor's cursor ring `+0x17A`, which
//! the anim tick shifts once a pass with slot `0` the live cursor.
//!
//! That holds on both sides of the instant a save state is cut. A state
//! saved after the pass's draw holds the live pose's pair (the decode of
//! ring slot `0`: `zeto_big_wave_mid_cast`, Zeto's `(-1, 912)` at cursor
//! `60`); one saved between the pass's ticks and its draw holds the
//! previous pose's (slot `1`: `gizam_summon_mid_cast`, Noa's `(597, -795)`
//! at cursor `112` while the cursor reads `124`). Either way the displayed
//! frame is the table built two passes before the cursor's, framed on the
//! pose before that.

use super::*;

/// The cursor-ring slot whose pose the displayed frame's framing stood on.
const FRAMING_RING_SLOT: u32 = DISPLAY_LAG_PASSES as u32 + 1;
/// The actor's anim-cursor ring (`+0x17A`, one halfword a pass, slot `0`
/// live) and ring-id ring (`+0x1FB`, the clip each slot played).
const RING_CURSOR: u32 = 0x17A;
const RING_ID: u32 = 0x1FB;
/// The SCUS sine and cosine tables the decoder turns the centroid by
/// (`lw a2,-0x47E4(0x80080000)` / `lw a1,-0x4808(0x80080000)`).
const SIN_TABLE_PTR: u32 = 0x8007_B81C;
const COS_TABLE_PTR: u32 = 0x8007_B7F8;

/// The `[parts][frames][9-byte TRS]` stream's translation triple for part
/// `k` of frame `f`, sign-extended from 12 bits (`0x8004A2AC..0x8004A37C`).
fn part_translation(ram: &[u8], stream: u32, parts: u32, f: u32, k: u32) -> [i32; 3] {
    let a = stream + 2 + (f * parts + k) * 9;
    let b = |i: u32| i32::from(game_anchors::u8_at(ram, a + i));
    let t = [
        b(0) | (b(2) & 0xF) << 8,
        b(1) | (b(2) & 0xF0) << 4,
        b(3) | (b(5) & 0xF) << 8,
    ];
    t.map(|v| if v & 0x800 != 0 { v - 0x1000 } else { v })
}

/// The body pair `FUN_8004998C` leaves on battle actor `actor` when it
/// decodes the actor's committed stream at `cursor` (12.4 sixteenths of a
/// keyframe). `None` past the stream's last frame, on the queued-clip arm
/// of the next-entry rule, or with a tilt `+0x44` (the arm at `0x8004A53C`
/// that re-derives the pair through a second rotation).
pub(super) fn body_pair_at(ram: &[u8], actor: u32, cursor: u16) -> Option<[i16; 2]> {
    let node = game_anchors::u32_at(ram, actor + 0x22C);
    if !in_ram(node) {
        return None;
    }
    let entry = game_anchors::u32_at(ram, node + 0x4C);
    if !in_ram(entry) {
        return None;
    }
    let stream = game_anchors::u32_at(ram, entry + 0x88);
    if !in_ram(stream) || game_anchors::i16_at(ram, actor + 0x44) != 0 {
        return None;
    }
    let parts = u32::from(game_anchors::u8_at(ram, stream));
    let frames = u32::from(game_anchors::u8_at(ram, stream + 1));
    let f = u32::from(cursor >> 4);
    let frac = i32::from(cursor & 0xF);
    if parts == 0 || f >= frames {
        return None;
    }
    // The next-entry rule (`0x80049A28..0x80049BD0`): the loop window's
    // start on its last frame while the loop counter `+0x21B` runs, else the
    // next frame; the stream's last frame blends toward the queued clip,
    // which this reader leaves alone.
    let next = if frac == 0 {
        f
    } else if f + 1 == u32::from(game_anchors::u8_at(ram, entry + 0x86))
        && game_anchors::u8_at(ram, actor + 0x21B) != 0
    {
        u32::from(game_anchors::u8_at(ram, entry + 0x85))
    } else if f + 1 < frames {
        f + 1
    } else {
        return None;
    };
    // The per-part lerp (`sra` then `sh`), summed in halfwords
    // (`lhu` / `addu` / `sh` into scratch `+0x120..+0x124`) and divided by
    // the part count (`lh` / `div`, truncating).
    let mut sum = [0i16; 3];
    for k in 0..parts {
        let c = part_translation(ram, stream, parts, f, k);
        let n = part_translation(ram, stream, parts, next, k);
        for i in 0..3 {
            let v = c[i] + (((n[i] - c[i]) * frac) >> 4);
            sum[i] = sum[i].wrapping_add(v as i16);
        }
    }
    let [cx, _, cz] = sum.map(|s| i32::from(s) / parts as i32);
    let facing = i32::from(game_anchors::i16_at(ram, actor + 0x46));
    if !(0..=0xFFF).contains(&facing) {
        return None;
    }
    let sin = game_anchors::u32_at(ram, SIN_TABLE_PTR);
    let cos = game_anchors::u32_at(ram, COS_TABLE_PTR);
    if !in_ram(sin) || !in_ram(cos) {
        return None;
    }
    let at = |table: u32, i: i32| i32::from(game_anchors::i16_at(ram, table + (i as u32) * 2));
    let scale = i32::from(game_anchors::u16_at(ram, node + 0x72));
    let x = ((at(sin, facing) * cz) >> 12) + ((at(cos, 0xFFF - facing) * cx) >> 12);
    let z = ((at(sin, 0xFFF - facing) * cx) >> 12) + ((at(cos, facing) * cz) >> 12);
    let live_x = i32::from(game_anchors::u16_at(ram, actor + 0x34));
    let live_z = i32::from(game_anchors::u16_at(ram, actor + 0x38));
    Some([
        (live_x + ((x * scale) >> 12)) as i16,
        (live_z + ((z * scale) >> 12)) as i16,
    ])
}

/// The body pair the displayed frame's framing read for battle actor
/// `actor`: the decode of its cursor-ring slot [`FRAMING_RING_SLOT`],
/// when that slot played the clip the actor still plays (a clip change
/// inside the window puts the older pose on another stream).
pub(super) fn displayed_framing_body(ram: &[u8], actor: u32) -> Option<[i16; 2]> {
    let id_now = game_anchors::u8_at(ram, actor + RING_ID);
    if (1..=FRAMING_RING_SLOT).any(|s| game_anchors::u8_at(ram, actor + RING_ID + s) != id_now) {
        return None;
    }
    let cursor = game_anchors::u16_at(ram, actor + RING_CURSOR + FRAMING_RING_SLOT * 2);
    body_pair_at(ram, actor, cursor)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(ram: &mut [u8], va: u32, bytes: &[u8]) {
        let o = (va & 0x1F_FFFF) as usize;
        ram[o..o + bytes.len()].copy_from_slice(bytes);
    }

    const ACTOR: u32 = 0x800E_0000;

    /// Two parts over two frames, facing `0`, the scratch sine tables'
    /// `0` / `0xFFF` rows retail's tables carry (`sin 0xFFF = -6`).
    fn fight() -> Vec<u8> {
        let mut ram = vec![0u8; 0x20_0000];
        let (node, entry, stream) = (0x800E_1000u32, 0x800E_2000u32, 0x800E_3000u32);
        let (sin, cos) = (0x800E_4000u32, 0x800E_6000u32);
        put(&mut ram, ACTOR + 0x22C, &node.to_le_bytes());
        put(&mut ram, node + 0x4C, &entry.to_le_bytes());
        put(&mut ram, entry + 0x88, &stream.to_le_bytes());
        put(&mut ram, node + 0x72, &4096u16.to_le_bytes());
        put(&mut ram, ACTOR + 0x34, &100u16.to_le_bytes());
        put(&mut ram, ACTOR + 0x38, &200u16.to_le_bytes());
        put(&mut ram, SIN_TABLE_PTR, &sin.to_le_bytes());
        put(&mut ram, COS_TABLE_PTR, &cos.to_le_bytes());
        put(&mut ram, sin + 0xFFF * 2, &(-6i16).to_le_bytes());
        put(&mut ram, cos, &4096i16.to_le_bytes());
        put(&mut ram, cos + 0xFFF * 2, &4096i16.to_le_bytes());
        put(&mut ram, stream, &[2, 2]);
        // (x, 0, z) per part, small and positive: no high nibbles.
        let frames = [[(10u8, 20u8), (30, 40)], [(18, 28), (38, 48)]];
        for (f, parts) in frames.iter().enumerate() {
            for (k, &(x, z)) in parts.iter().enumerate() {
                let a = stream + 2 + ((f * 2 + k) * 9) as u32;
                put(&mut ram, a, &[x, 0, 0, z, 0, 0, 0, 0, 0]);
            }
        }
        ram
    }

    /// The decode's body pair: the parts' lerped translations summed and
    /// divided by the part count, turned by the heading through the
    /// tables, added to the live pair.
    #[test]
    fn a_cursor_decodes_to_the_body_pair_the_draw_leaves() {
        let ram = fight();
        // Frame 0 alone: centroid (20, 30).
        assert_eq!(body_pair_at(&ram, ACTOR, 0), Some([120, 229]));
        // Half way to frame 1: centroid (24, 34); the `sin 0xFFF` row
        // turns the X term into Z by `(-6 * 24) >> 12 = -1`.
        assert_eq!(body_pair_at(&ram, ACTOR, 8), Some([124, 233]));
        // The last frame blends toward the queued clip: not read.
        assert_eq!(body_pair_at(&ram, ACTOR, 0x18), None);
    }

    /// The displayed frame's framing reads ring slot `3`, and only while
    /// the ring played one clip across the window.
    #[test]
    fn the_displayed_framing_reads_the_pose_three_passes_back() {
        let mut ram = fight();
        let ring = |s: u32| ACTOR + RING_CURSOR + s * 2;
        for (s, c) in [(0u32, 20u16), (1, 16), (2, 12), (3, 8)] {
            put(&mut ram, ring(s), &c.to_le_bytes());
        }
        assert_eq!(displayed_framing_body(&ram, ACTOR), Some([124, 233]));
        put(&mut ram, ACTOR + RING_ID + 2, &[9]);
        assert_eq!(displayed_framing_body(&ram, ACTOR), None);
    }
}
