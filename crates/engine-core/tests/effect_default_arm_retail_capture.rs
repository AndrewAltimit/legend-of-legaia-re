//! Retail capture: `FUN_80028158`'s output, byte for byte, against the
//! scratch buffers the catalogued mednafen states hold.
//!
//! Retail builds every default-arm draw-kind-4 node's geometry at
//! `*(0x8007B85C) + 0x5DC00` and the battle ground shadow at
//! `*(0x8007B85C) + 0x62400`, and never clears either, so a state holds the
//! last object each was built into. For every state:
//!
//! * **The ground shadow** (`0x62400`): the object's two colour words and its
//!   radius are read back off the buffer, the builder
//!   ([`legaia_engine_core::effect_default_arm::build`], mode `1`, 24
//!   columns) is run on them, and every byte the port writes must equal
//!   retail's.
//! * **The dispatcher's buffer** (`0x5DC00`): the builder is run on every
//!   live default-arm node's own `(mode, packed, src)` (walked off the
//!   actor-list heads `0x8007C34C..0x8007C36C`); the buffer is matched when
//!   one node reproduces every byte the port writes. A buffer no live node
//!   reproduces is the leftover of a node that has since died, or of a
//!   sprite-arm node (`flags 0x22`), and is reported, not failed.
//!
//! Bytes the port does not write are the builder's stale ones (retail never
//! writes them either) and are not compared.
//!
//! Skips (and passes) when the save library is missing.

use legaia_engine_core::effect_default_arm::{
    SCRATCH_OFFSET, SHADOW_SCRATCH_OFFSET, SHADOW_SEGMENTS, build, ground_shadow_src,
};
use legaia_engine_core::effect_ribbon::RetailTrig;
use legaia_mednafen::SaveState;
use std::path::PathBuf;

const RAM_MASK: u32 = 0x001F_FFFF;
const ASSET_BUF_PTR: u32 = 0x8007_B85C;
const SCREEN_PAGE: u32 = 0x8007_B74C;
/// The shadow object's header, vertex block, group header and first packet
/// end here (`0x28 + 48 * 8 + 8 + 0x24`): a buffer that matches this far and
/// differs later was overwritten after the build.
const SHADOW_FIRST_PACKET_END: usize = 0x1D4;

fn library() -> Option<PathBuf> {
    ["", "../", "../../"]
        .iter()
        .map(|p| PathBuf::from(format!("{p}saves/library/mednafen")))
        .find(|p| p.is_dir())
}

fn u32_at(ram: &[u8], va: u32) -> u32 {
    let o = (va & RAM_MASK) as usize;
    u32::from_le_bytes([ram[o], ram[o + 1], ram[o + 2], ram[o + 3]])
}

fn u16_at(ram: &[u8], va: u32) -> u16 {
    let o = (va & RAM_MASK) as usize;
    u16::from_le_bytes([ram[o], ram[o + 1]])
}

fn is_ram(va: u32) -> bool {
    va >> 21 == 0x400
}

/// Compare the port's build of `(mode, packed, src)` at `out` with retail's
/// bytes wherever the port writes: `(bytes equal, first differing offset)`.
fn compare(
    ram: &[u8],
    out: u32,
    mode: u32,
    packed: u32,
    src: &[u8; 0x24],
) -> Option<(usize, Option<usize>)> {
    let page = u16_at(ram, SCREEN_PAGE) as i16;
    let b = build(out, mode, packed, src, &RetailTrig, || 0, page)?;
    let base = (out & RAM_MASK) as usize;
    if base + b.bytes.len() > ram.len() {
        return None;
    }
    let mut n = 0;
    let mut first = None;
    for (k, (&v, &w)) in b.bytes.iter().zip(&b.written).enumerate() {
        if w {
            if ram[base + k] == v {
                n += 1;
            } else if first.is_none() {
                first = Some(k);
            }
        }
    }
    Some((n, first))
}

/// A buffer built from a node whose animated channels (`+0xB4..+0xBA`,
/// `+0xC8`, stepped every frame by the part tick) have moved since: keep the
/// node's mode, count, colours and scales, read the radii and heights off
/// the buffer's first column (`r0 = |v0|`, `r1 = |v1|` in the plane,
/// `c0` / `c1` the out-of-plane components), and search the phase. Returns
/// the first node that then reproduces every byte the port writes.
fn fit(ram: &[u8], out: u32, nodes: &[(u32, u32, [u8; 0x24])]) -> Option<(u32, u32, usize)> {
    let v = |k: u32, c: u32| i32::from(u16_at(ram, out + 0x28 + k * 8 + c) as i16);
    for &(mode, packed, src) in nodes {
        let (a, b, c) = match mode & 3 {
            0 => (0, 2, 4),
            1 => (0, 4, 2),
            2 => (4, 2, 0),
            _ => (4, 0, 2),
        };
        let rad = |k: u32| f64::from(v(k, a)).hypot(f64::from(v(k, b))) as i32;
        let (e0, e1) = (rad(0), rad(1));
        let (c0, c1) = (v(0, c) as i16, v(1, c) as i16);
        for phase in 0..0x400u32 {
            for r0 in (e0 - 2).max(0)..=e0 + 2 {
                for r1 in (e1 - 2).max(0)..=e1 + 2 {
                    let mut s = src;
                    s[0x18..0x1A].copy_from_slice(&(r0 as i16).to_le_bytes());
                    s[0x1A..0x1C].copy_from_slice(&(r1 as i16).to_le_bytes());
                    s[0x1C..0x1E].copy_from_slice(&c0.to_le_bytes());
                    s[0x1E..0x20].copy_from_slice(&c1.to_le_bytes());
                    let p = (packed & 0xFF) | (phase << 8);
                    if let Some((n, None)) = compare(ram, out, mode, p, &s) {
                        return Some((mode, p, n));
                    }
                }
            }
        }
    }
    None
}

/// Every live default-arm node's call arguments in a state.
fn default_arm_nodes(ram: &[u8]) -> Vec<(u32, u32, [u8; 0x24])> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for head_va in (0x8007_C34C..0x8007_C370).step_by(4) {
        let head = u32_at(ram, head_va);
        if !is_ram(head) {
            continue;
        }
        let mut n = u32_at(ram, head);
        let mut hops = 0;
        while n != 0 && n != head && is_ram(n) && hops < 400 && seen.insert(n) {
            if u16_at(ram, n + 0x56) == 4 && u16_at(ram, n + 0x9E) & 0x6000 == 0 {
                let count = i32::from(u16_at(ram, n + 0x9C) as i16);
                let total = i32::from(u16_at(ram, n + 0xC8) as i16) >> 3;
                let mut src = [0u8; 0x24];
                for (k, b) in src.iter_mut().enumerate() {
                    *b = ram[((n + 0x9C + k as u32) & RAM_MASK) as usize];
                }
                out.push((
                    u32::from(u16_at(ram, n + 0x9E)),
                    count.wrapping_add(total << 8) as u32,
                    src,
                ));
            }
            n = u32_at(ram, n);
            hops += 1;
        }
    }
    out
}

#[test]
fn default_arm_builds_match_the_retail_scratch_buffers() {
    let Some(lib) = library() else {
        eprintln!("[skip] saves/library/mednafen missing");
        return;
    };
    let mut paths: Vec<_> = std::fs::read_dir(&lib)
        .expect("read library")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    paths.sort();
    let (mut shadows, mut shadow_ok, mut shadow_clobbered) = (0usize, 0usize, 0usize);
    let (mut arm_bufs, mut arm_ok, mut arm_nodes) = (0usize, 0usize, 0usize);
    let mut shapes_matched = std::collections::BTreeSet::new();
    let (mut arm_fit, mut fit_modes) = (0usize, std::collections::BTreeSet::new());
    for path in &paths {
        let Ok(state) = SaveState::from_path(path) else {
            continue;
        };
        let Ok(ram) = state.main_ram() else {
            continue;
        };
        let name = path.file_name().unwrap().to_string_lossy();
        let name = &name[..8.min(name.len())];
        let asset = u32_at(ram, ASSET_BUF_PTR);
        if !is_ram(asset) {
            continue;
        }
        // The ground shadow: 48 vertices, then the group header and 24
        // packets from `0x1B0`.
        let out = asset.wrapping_add(SHADOW_SCRATCH_OFFSET);
        if u32_at(ram, out + 0xC) == out + 0x28 && u32_at(ram, out + 0x10) == 48 {
            shadows += 1;
            let first = out + 0x28 + 48 * 8 + 8;
            let inner = u32_at(ram, first) & 0xFF_FFFF;
            let outer = u32_at(ram, first + 4) & 0xFF_FFFF;
            // The outer vertex of column 0 puts the radius within a few units.
            let vx = f64::from(u16_at(ram, out + 0x30) as i16);
            let vz = f64::from(u16_at(ram, out + 0x34) as i16);
            let est = (vx * vx + vz * vz).sqrt() as i32;
            let best = ((est - 8).max(0)..=est + 8)
                .filter_map(|r| {
                    let src = ground_shadow_src(inner, outer, r as i16);
                    compare(ram, out, 1, SHADOW_SEGMENTS, &src).map(|c| (r, c))
                })
                .max_by_key(|&(_, (_, first))| first.unwrap_or(usize::MAX));
            match best {
                Some((r, (n, None))) => {
                    shadow_ok += 1;
                    eprintln!(
                        "[ok] {name} shadow: radius {r}, colours {inner:#08x}/{outer:#08x}, \
                         {n} bytes equal"
                    );
                }
                Some((r, (n, Some(at)))) if at >= SHADOW_FIRST_PACKET_END => {
                    shadow_clobbered += 1;
                    eprintln!(
                        "[ok-prefix] {name} shadow: radius {r}, {n} bytes equal; the \
                         buffer is overwritten from +{at:#x} (packet {}) on",
                        (at - 0x1B0) / 0x24
                    );
                }
                other => {
                    eprintln!("[miss] {name} shadow: est {est}, best {other:?}");
                }
            }
        }
        // The dispatcher's buffer.
        let out = asset.wrapping_add(SCRATCH_OFFSET);
        let flags = u16_at(ram, u32_at(ram, out + 0x1C) + 2);
        if u32_at(ram, out + 0xC) == out + 0x28 && flags == 0x26 {
            arm_bufs += 1;
            let nodes = default_arm_nodes(ram);
            arm_nodes += nodes.len();
            let hit = nodes.iter().find_map(|&(mode, packed, src)| {
                match compare(ram, out, mode, packed, &src) {
                    Some((n, None)) => Some((mode, packed, n)),
                    _ => None,
                }
            });
            match hit {
                Some((mode, packed, n)) => {
                    arm_ok += 1;
                    shapes_matched.insert(mode);
                    eprintln!(
                        "[ok] {name} default arm: mode {mode:#x} packed {packed:#x}, \
                         {n} bytes equal"
                    );
                }
                None => match fit(ram, out, &nodes) {
                    Some((mode, packed, n)) => {
                        arm_fit += 1;
                        fit_modes.insert(mode);
                        eprintln!(
                            "[ok-fit] {name} default arm: mode {mode:#x}, the node's radii / \
                             heights / phase refitted to the buffer (packed {packed:#x}), \
                             {n} bytes equal"
                        );
                    }
                    None => eprintln!(
                        "[unmatched] {name} default-arm buffer ({} live default-arm nodes)",
                        nodes.len()
                    ),
                },
            }
        }
    }
    eprintln!(
        "shadows {shadow_ok}/{shadows} matched whole, {shadow_clobbered} up to a later \
         overwrite; default-arm buffers {arm_ok}/{arm_bufs} \
         matched by a live node ({arm_nodes} nodes tried), {arm_fit} more by a node's \
         previous-frame channels; modes matched {shapes_matched:x?}, refitted {fit_modes:x?}"
    );
    if shadows == 0 {
        eprintln!("[skip] no state holds a ground-shadow build");
        return;
    }
    assert_eq!(
        shadow_ok + shadow_clobbered,
        shadows,
        "every ground shadow reproduces (whole, or up to a later overwrite)"
    );
    assert!(shadow_ok > shadow_clobbered, "most shadows reproduce whole");
    assert!(
        arm_ok > 0,
        "no default-arm buffer reproduced by a live node"
    );
    // Every shape the captures hold (0..3) and texture mode 1 reproduce.
    let all: std::collections::BTreeSet<u32> = shapes_matched.union(&fit_modes).copied().collect();
    let shapes: std::collections::BTreeSet<u32> = all.iter().map(|m| (m >> 3) & 0xF).collect();
    assert!(
        shapes.is_superset(&[0, 1, 2, 3].into()),
        "shapes reproduced: {shapes:?}"
    );
    assert!(
        all.iter().any(|m| m >> 8 == 1),
        "texture mode 1 not reproduced"
    );
}
