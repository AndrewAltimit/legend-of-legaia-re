//! World-map sentinel-placement resolver.
//!
//! Cross-reference live actor positions (from a mednafen save state's
//! main RAM) with the static MAN-buffer records on disc, surfacing the
//! runtime-resolved positions of actors whose MAN record is the
//! ``(x_enc, z_enc) = (0x7F, 0x7F)`` sentinel (i.e. positioned by the
//! FieldVM prescript invoked from ``FUN_8003A1E4`` rather than by
//! literal coordinates in the MAN record).
//!
//! This is a thin Rust port of the placement-extraction half of
//! ``scripts/mednafen/resolve_bulk_terrain.py``. It deliberately does
//! NOT execute the FieldVM prescript - that would require a full port
//! of ``FUN_801DE840`` (see ``crates/engine-vm::field_vm``) plus the
//! actor-allocation environment it ticks against. Instead it captures
//! the *post-resolve* state from a save snapshot.
//!
//! Inputs:
//!   * 2 MiB PSX main-RAM blob (from ``legaia_mednafen::SaveState::main_ram``)
//!   * Disc-extracted, LZS-decompressed MAN buffer for the kingdom
//!   * Kingdom TMD-pack metadata (load base + per-slot byte offsets;
//!     produced by the existing extractor pipeline)
//!
//! Outputs:
//!   * One ``SentinelPlacement`` per matched live actor whose ``+0x90``
//!     pointer is in the MAN buffer's range. Each includes the
//!     resolved position, source MAN record index, and the resolved
//!     TMD pack slot(s) from the actor's mesh chain at ``+0x44``.
//!
//! See ``docs/subsystems/world-map.md`` ("MAN-record resolver chain"
//! section) for the byte-for-byte description of the resolver this
//! mirrors.

use std::ops::Range;

/// PSX kuseg base for main-RAM addresses.
pub const PSX_RAM_KSEG0: u32 = 0x8000_0000;

/// Size of the physical PSX main RAM that ``mednafen-state extract``
/// dumps. The legaia-mednafen ``main_ram`` helper returns this many
/// bytes.
pub const PSX_RAM_SIZE: usize = 2 * 1024 * 1024;

/// SCUS global pointer holding the decompressed MAN buffer; written by
/// ``FUN_8001F05C`` case 3 (the MAN asset loader) and read by every
/// MAN consumer (``FUN_8003A1E4``, ``FUN_8003AB2C``, etc.).
pub const MAN_BUFFER_PTR_ADDR: u32 = 0x8007_B898;

/// World-map actor-list heads scanned by the placement walker. Same
/// set as ``scripts/pcsx-redux/resolve_actor_tmds.py``. The pool head
/// pointers live in SCUS at these fixed addresses.
pub const LIST_HEAD_ADDRS: &[u32] = &[
    0x8007_C34C,
    0x8007_C350,
    0x8007_C354,
    0x8007_C358,
    0x8007_C35C,
    0x8007_C360,
    0x8007_C364,
    0x8007_C368,
    0x8007_C36C,
];

/// Tick function pointer that identifies the world-map atmospheric
/// actor (``FUN_801E3E00``, dumped at
/// ``ghidra/scripts/funcs/overlay_world_map_801e3e00.txt``). The
/// atmospheric script interpolates fog RGB into the actor's ``+0x74``
/// field per frame; capturing this u32 surfaces the kingdom's live
/// haze colour.
pub const ATMOSPHERIC_TICK: u32 = 0x801E_3E00;

/// Legaia TMD magic word (little-endian on disc).
const TMD_MAGIC: [u8; 4] = [0x02, 0x00, 0x00, 0x80];

/// One MAN record as the placement-walker (``FUN_8003A1E4``) sees it.
/// The byte range is relative to the MAN buffer base; matching a live
/// actor's ``actor[+0x90]`` against ``byte_offset`` (or the containing
/// range when the walker advanced ``pcVar13`` mid-record) identifies
/// the source record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManRecord {
    /// Record index in the placement-walker iteration order
    /// (``s4 in [1, total-a)``, mapped to ``a3 in [a+1, total)``).
    pub id: u16,
    pub byte_range: Range<u32>,
    pub name_utf8: String,
    pub tmd_slot: u8,
    pub flag: u8,
    pub x_enc: u8,
    pub z_enc: u8,
    /// True iff ``(x_enc, z_enc) == (0x7F, 0x7F)``. The static walker
    /// decodes this to literal world coordinate ``(16320, 16320)`` (the
    /// world's NE corner); the actor's actual position is later set by
    /// the FieldVM prescript embedded in the trailing bytes.
    pub script_positioned: bool,
}

/// Parse the disc-extracted MAN buffer, mirroring
/// ``scripts/asset-investigation/extract-world-placements.py:parse_placements``.
///
/// The walker only emits records in the ``[a+1, total)`` range (the
/// ``a``-class records aren't drawn as actors).
pub fn parse_man_records(man: &[u8]) -> Vec<ManRecord> {
    let mut out = Vec::new();
    if man.len() < 0x2B {
        return out;
    }
    let hdr: usize = 0x22;
    let a = i16_at(man, hdr) as isize;
    let b = i16_at(man, hdr + 2) as isize;
    let c = i16_at(man, hdr + 4) as isize;
    let total = a + b + c;
    if total <= 0 {
        return out;
    }
    let total_u = total as usize;
    let off_tbl = hdr + 9;
    if off_tbl + total_u * 3 > man.len() {
        return out;
    }
    let mut offsets = Vec::with_capacity(total_u);
    for i in 0..total_u {
        let lo = man[off_tbl + i * 3] as u32;
        let mid = man[off_tbl + i * 3 + 1] as u32;
        let hi = man[off_tbl + i * 3 + 2] as u32;
        offsets.push(lo | (mid << 8) | (hi << 16));
    }
    let data_area = off_tbl + total_u * 3;
    for s4 in 1..(total - a) {
        let a3 = (a + s4) as usize;
        if a3 >= total_u {
            break;
        }
        let rec_off = data_area + offsets[a3] as usize;
        if rec_off >= man.len() {
            break;
        }
        let rec_end = if a3 + 1 < total_u {
            data_area + offsets[a3 + 1] as usize
        } else {
            man.len()
        };
        let n_chars = man[rec_off] as usize;
        let name_end = rec_off + 1 + 2 * n_chars;
        if name_end + 4 > man.len() {
            break;
        }
        // Best-effort UTF-8 of the Shift_JIS name. We don't pull in an
        // encoding-conversion dep just for the placement labels; the
        // name field is informational here and the Python tooling does
        // the proper Shift_JIS decode.
        let name_utf8 = String::from_utf8_lossy(&man[rec_off + 1..name_end]).into_owned();
        let tmd_slot = man[name_end];
        let flag = man[name_end + 1];
        let x_enc = man[name_end + 2];
        let z_enc = man[name_end + 3];
        out.push(ManRecord {
            id: s4 as u16,
            byte_range: rec_off as u32..rec_end as u32,
            name_utf8,
            tmd_slot,
            flag,
            x_enc,
            z_enc,
            script_positioned: x_enc == 0x7F && z_enc == 0x7F,
        });
    }
    out
}

fn i16_at(buf: &[u8], off: usize) -> i16 {
    if off + 2 > buf.len() {
        return 0;
    }
    i16::from_le_bytes([buf[off], buf[off + 1]])
}

/// One live actor's resolved placement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SentinelPlacement {
    /// PSX RAM address of the actor record.
    pub actor_node: u32,
    /// Live world position in the same convention the renderer reads
    /// (``actor[+0x14] / actor[+0x16] / actor[+0x18]``).
    pub pos: [i16; 3],
    /// Resolved kingdom-TMD-pack slot(s) from the actor's mesh chain
    /// at ``+0x44``.
    pub slots: Vec<usize>,
    /// Count of mesh-chain pointers that didn't resolve to the pack
    /// (typically party-character TMDs from the global pool).
    pub unresolved_chain_ptrs: usize,
    /// Tick function pointer (``actor[+0x0C]``).
    pub tick: u32,
    /// MAN record this actor was spawned from, when ``actor[+0x90]`` is
    /// within the MAN buffer's byte range.
    pub man_record_id: Option<u16>,
    /// Mirrors ``ManRecord::script_positioned`` for the matched record.
    pub script_positioned: bool,
    /// Live ``actor[+0x74]`` u24 RGB packed colour. Only meaningful
    /// when ``tick == ATMOSPHERIC_TICK``.
    pub fog_color_u24: Option<u32>,
}

/// Resolve every world-map actor in the given main-RAM blob whose mesh
/// chain points into the kingdom's TMD pack. Matches against the MAN
/// buffer (when ``actor[+0x90]`` lies in its byte range) to identify
/// the source record.
///
/// `man_buffer_disc` is the LZS-decompressed MAN buffer from disc -
/// the canonical record bytes - and need not be byte-identical to the
/// RAM copy (the FieldVM is allowed to mutate the RAM copy).
///
/// `pack_byte_offsets` is the per-slot byte offset table from the
/// kingdom's slot-1 TMD pack (slot k = ``word_offsets[k] * 4``).
/// `pack_load_base` is its address in main RAM (recovered by sample-
/// match in the caller; see ``find_landmark_load_base`` in the Python
/// tooling).
pub fn resolve_world_actors(
    ram: &[u8],
    man_buffer_disc: &[u8],
    pack_load_base: u32,
    pack_byte_offsets: &[u32],
    man_buffer_ram_base: u32,
) -> Vec<SentinelPlacement> {
    if ram.len() != PSX_RAM_SIZE {
        return Vec::new();
    }
    let records = parse_man_records(man_buffer_disc);
    let man_len = man_buffer_disc.len() as u32;
    let mut seen = Vec::new();
    let mut out = Vec::new();
    for &head_addr in LIST_HEAD_ADDRS {
        let mut node = match read_u32(ram, head_addr) {
            Some(v) => v,
            None => continue,
        };
        while node != 0 && node != 0xFFFF_FFFF && !seen.contains(&node) {
            seen.push(node);
            let nxt = read_u32(ram, node).unwrap_or(0);
            if let Some(p) = build_placement(
                ram,
                node,
                man_buffer_ram_base,
                man_len,
                &records,
                pack_load_base,
                pack_byte_offsets,
            ) {
                out.push(p);
            }
            node = nxt;
        }
    }
    out
}

fn build_placement(
    ram: &[u8],
    node: u32,
    man_buffer_ram_base: u32,
    man_len: u32,
    records: &[ManRecord],
    pack_load_base: u32,
    pack_byte_offsets: &[u32],
) -> Option<SentinelPlacement> {
    let tick = read_u32(ram, node + 0x0C)?;
    let mesh_head = read_u32(ram, node + 0x44)?;
    let script_ptr = read_u32(ram, node + 0x90)?;
    let x = read_i16_addr(ram, node + 0x14);
    let y = read_i16_addr(ram, node + 0x16);
    let z = read_i16_addr(ram, node + 0x18);
    let c74 = read_u32(ram, node + 0x74).unwrap_or(0) & 0x00FF_FFFF;

    // Walk the mesh chain at +0x44 and map each prim-group pointer back
    // to a pack slot.
    let mut slots: Vec<usize> = Vec::new();
    let mut unresolved = 0usize;
    if mesh_head >= PSX_RAM_KSEG0 {
        let count = read_u32(ram, mesh_head).unwrap_or(0);
        if count > 0 && count < 64 {
            for k in 0..count {
                let p = read_u32(ram, mesh_head + 4 + 4 * k).unwrap_or(0);
                if p < PSX_RAM_KSEG0 {
                    continue;
                }
                match find_containing_tmd(ram, p) {
                    Some(tmd_addr) => {
                        match tmd_addr_to_slot(tmd_addr, pack_load_base, pack_byte_offsets) {
                            Some(s) => slots.push(s),
                            None => unresolved += 1,
                        }
                    }
                    None => unresolved += 1,
                }
            }
        }
    }
    if slots.is_empty() {
        return None;
    }
    slots.sort_unstable();
    slots.dedup();

    // MAN-record cross-reference: actor[+0x90] is the MAN record start
    // pointer when FUN_8003A1E4 was the spawner.
    let (man_id, script_positioned) =
        if script_ptr >= man_buffer_ram_base && script_ptr - man_buffer_ram_base < man_len {
            let rel = script_ptr - man_buffer_ram_base;
            let rec = records
                .iter()
                .find(|r| r.byte_range.start == rel)
                .or_else(|| records.iter().find(|r| r.byte_range.contains(&rel)));
            match rec {
                Some(r) => (Some(r.id), r.script_positioned),
                None => (None, false),
            }
        } else {
            (None, false)
        };

    let fog_color_u24 = if tick == ATMOSPHERIC_TICK && c74 != 0 {
        Some(c74)
    } else {
        None
    };

    Some(SentinelPlacement {
        actor_node: node,
        pos: [x, y, z],
        slots,
        unresolved_chain_ptrs: unresolved,
        tick,
        man_record_id: man_id,
        script_positioned,
        fog_color_u24,
    })
}

fn read_u32(ram: &[u8], addr: u32) -> Option<u32> {
    if addr < PSX_RAM_KSEG0 {
        return None;
    }
    let off = (addr - PSX_RAM_KSEG0) as usize;
    if off + 4 > ram.len() {
        return None;
    }
    Some(u32::from_le_bytes([
        ram[off],
        ram[off + 1],
        ram[off + 2],
        ram[off + 3],
    ]))
}

fn read_i16_addr(ram: &[u8], addr: u32) -> i16 {
    if addr < PSX_RAM_KSEG0 {
        return 0;
    }
    let off = (addr - PSX_RAM_KSEG0) as usize;
    if off + 2 > ram.len() {
        return 0;
    }
    i16::from_le_bytes([ram[off], ram[off + 1]])
}

/// Walk backwards in 4-byte steps for up to 256 KiB from `addr`,
/// returning the PSX address of the nearest preceding TMD-magic word
/// (or ``None`` if no magic is in range). Mirrors
/// ``find_containing_tmd`` in ``resolve_actor_tmds.py``.
pub fn find_containing_tmd(ram: &[u8], addr: u32) -> Option<u32> {
    if addr < PSX_RAM_KSEG0 {
        return None;
    }
    let mut off = (addr - PSX_RAM_KSEG0) as usize;
    if off >= ram.len() {
        return None;
    }
    let limit = off.saturating_sub(0x4_0000);
    while off >= limit && off + 4 <= ram.len() {
        if ram[off..off + 4] == TMD_MAGIC {
            return Some(PSX_RAM_KSEG0 + off as u32);
        }
        if off < 4 {
            break;
        }
        off -= 4;
    }
    None
}

/// Resolve a TMD's RAM start address to its slot in the kingdom pack.
/// Returns ``None`` when the TMD didn't come from this pack (e.g. it
/// belongs to the global party-character pool).
pub fn tmd_addr_to_slot(tmd_ram_addr: u32, load_base: u32, byte_offsets: &[u32]) -> Option<usize> {
    if tmd_ram_addr < load_base {
        return None;
    }
    let pack_off = tmd_ram_addr - load_base;
    byte_offsets.iter().position(|&o| o == pack_off)
}

/// Pick the first non-zero fog colour observed across a placement list.
///
/// Atmospheric actors interpolate fog RGB into their ``+0x74`` field
/// each frame, so multiple captures might disagree on the exact value;
/// the first non-zero sample tends to converge on the kingdom's mean
/// haze and is a fine summary for the viewer's per-kingdom default.
pub fn pick_fog_color(placements: &[SentinelPlacement]) -> Option<u32> {
    placements
        .iter()
        .filter_map(|p| p.fog_color_u24)
        .find(|&c| c != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_u32(ram: &mut [u8], addr: u32, v: u32) {
        let off = (addr - PSX_RAM_KSEG0) as usize;
        ram[off..off + 4].copy_from_slice(&v.to_le_bytes());
    }

    fn write_i16(ram: &mut [u8], addr: u32, v: i16) {
        let off = (addr - PSX_RAM_KSEG0) as usize;
        ram[off..off + 2].copy_from_slice(&v.to_le_bytes());
    }

    #[test]
    fn parse_empty_man_returns_empty() {
        assert!(parse_man_records(&[]).is_empty());
        assert!(parse_man_records(&[0u8; 0x20]).is_empty());
    }

    #[test]
    fn parse_one_sentinel_record() {
        // Synthesise the minimum MAN-buffer shape the placement walker
        // would accept: a=0, b=0, c=2 (so total=2; walker iterates
        // s4 in [1, total-a) = [1, 2) -> a3 = 1).
        let mut man = vec![0u8; 256];
        man[0x22] = 0;
        man[0x23] = 0; // a (i16)
        man[0x24] = 0;
        man[0x25] = 0; // b
        man[0x26] = 2;
        man[0x27] = 0; // c
        // Offset table at 0x2B (= 0x22 + 9), 3 bytes per entry x 2
        // entries = 6 bytes. data_area = 0x2B + 6 = 0x31.
        // Place record-1 at offset 0 from data_area.
        man[0x2B] = 0;
        man[0x2C] = 0;
        man[0x2D] = 0; // offsets[0] = 0
        man[0x2E] = 0;
        man[0x2F] = 0;
        man[0x30] = 0; // offsets[1] = 0
        // Record-1 at 0x31: n_chars=0, then tmd=3, flag=0x20,
        // x_enc=0x7F, z_enc=0x7F.
        man[0x31] = 0; // n_chars
        man[0x32] = 3; // tmd_slot
        man[0x33] = 0x20; // flag
        man[0x34] = 0x7F;
        man[0x35] = 0x7F; // sentinel coords
        let recs = parse_man_records(&man);
        assert_eq!(recs.len(), 1);
        let r = &recs[0];
        assert_eq!(r.id, 1);
        assert_eq!(r.tmd_slot, 3);
        assert_eq!(r.flag, 0x20);
        assert_eq!(r.x_enc, 0x7F);
        assert_eq!(r.z_enc, 0x7F);
        assert!(r.script_positioned);
        assert_eq!(r.byte_range.start, 0x31);
    }

    #[test]
    fn parse_non_sentinel_record() {
        let mut man = vec![0u8; 256];
        man[0x22] = 0;
        man[0x24] = 0;
        man[0x26] = 2;
        man[0x27] = 0;
        man[0x2B] = 0;
        man[0x2C] = 0;
        man[0x2D] = 0;
        man[0x2E] = 0;
        man[0x2F] = 0;
        man[0x30] = 0;
        // Record-1: tmd=7, flag=0x50, x_enc=0x10, z_enc=0x20
        man[0x31] = 0;
        man[0x32] = 7;
        man[0x33] = 0x50;
        man[0x34] = 0x10;
        man[0x35] = 0x20;
        let recs = parse_man_records(&man);
        assert_eq!(recs.len(), 1);
        assert!(!recs[0].script_positioned);
    }

    #[test]
    fn find_containing_tmd_walks_back() {
        let mut ram = vec![0u8; PSX_RAM_SIZE];
        // Plant a TMD magic at 0x80100000.
        let magic_addr = 0x8010_0000;
        let magic_off = (magic_addr - PSX_RAM_KSEG0) as usize;
        ram[magic_off..magic_off + 4].copy_from_slice(&TMD_MAGIC);
        // A pointer mid-TMD at 0x80100200 must resolve to 0x80100000.
        assert_eq!(find_containing_tmd(&ram, 0x8010_0200), Some(magic_addr));
        // A pointer at the magic itself also resolves.
        assert_eq!(find_containing_tmd(&ram, magic_addr), Some(magic_addr));
        // A pointer way before any magic - no match.
        assert_eq!(find_containing_tmd(&ram, 0x8000_0100), None);
    }

    #[test]
    fn tmd_addr_to_slot_returns_index() {
        let offsets = vec![0x00, 0x400, 0x800];
        let load_base = 0x8010_0000;
        // First TMD: pack offset 0.
        assert_eq!(tmd_addr_to_slot(0x8010_0000, load_base, &offsets), Some(0));
        // Second TMD: pack offset 0x400.
        assert_eq!(tmd_addr_to_slot(0x8010_0400, load_base, &offsets), Some(1));
        // Third TMD: pack offset 0x800.
        assert_eq!(tmd_addr_to_slot(0x8010_0800, load_base, &offsets), Some(2));
        // Not in pack (no matching offset).
        assert_eq!(tmd_addr_to_slot(0x8010_0100, load_base, &offsets), None);
    }

    #[test]
    fn resolve_world_actors_matches_man_record() {
        // Build a 2 MiB RAM blob with one actor whose mesh chain points
        // at a TMD in the kingdom pack, and whose +0x90 lands inside a
        // synthetic MAN buffer.
        let mut ram = vec![0u8; PSX_RAM_SIZE];

        // Kingdom pack: 2 TMDs at RAM 0x80130000 and 0x80130200.
        let pack_load_base = 0x8013_0000;
        let pack_offsets = vec![0x00, 0x200];
        let off0 = (pack_load_base - PSX_RAM_KSEG0) as usize;
        ram[off0..off0 + 4].copy_from_slice(&TMD_MAGIC);
        ram[off0 + 0x200..off0 + 0x204].copy_from_slice(&TMD_MAGIC);

        // MAN buffer at RAM 0x80140000 with one sentinel record.
        let man_ram_base = 0x8014_0000;
        let mut man = vec![0u8; 0x40];
        man[0x22] = 0;
        man[0x24] = 0;
        man[0x26] = 2;
        man[0x27] = 0;
        man[0x2B] = 0;
        man[0x2C] = 0;
        man[0x2D] = 0;
        man[0x2E] = 0;
        man[0x2F] = 0;
        man[0x30] = 0;
        man[0x31] = 0;
        man[0x32] = 1;
        man[0x33] = 0x20;
        man[0x34] = 0x7F;
        man[0x35] = 0x7F;
        let man_off = (man_ram_base - PSX_RAM_KSEG0) as usize;
        ram[man_off..man_off + man.len()].copy_from_slice(&man);

        // Actor at RAM 0x80082000:
        //  +0x00 next-link = 0
        //  +0x0C tick = 0x801E3E00 (atmospheric)
        //  +0x14/+0x18 world pos = (1234, 5678)
        //  +0x44 mesh head = 0x80083000
        //  +0x74 fog rgb = 0x00CC5511
        //  +0x90 script ptr = man_ram_base + 0x31 (= record 1's start)
        let actor = 0x8008_2000u32;
        write_u32(&mut ram, actor, 0); // next
        write_u32(&mut ram, actor + 0x0C, ATMOSPHERIC_TICK); // tick
        write_i16(&mut ram, actor + 0x14, 1234); // x
        write_i16(&mut ram, actor + 0x16, -7); // y
        write_i16(&mut ram, actor + 0x18, 5678); // z
        write_u32(&mut ram, actor + 0x44, 0x8008_3000); // mesh head
        write_u32(&mut ram, actor + 0x74, 0x00CC_5511); // fog rgb
        write_u32(&mut ram, actor + 0x90, man_ram_base + 0x31); // script

        // Mesh-chain head at 0x80083000: count=1, ptrs[0] = 0x80130010
        // (inside TMD slot 0).
        write_u32(&mut ram, 0x8008_3000, 1);
        write_u32(&mut ram, 0x8008_3004, 0x8013_0010);

        // Hook the actor in via list head LIST_HEAD_ADDRS[0].
        write_u32(&mut ram, LIST_HEAD_ADDRS[0], actor);

        let placements =
            resolve_world_actors(&ram, &man, pack_load_base, &pack_offsets, man_ram_base);
        assert_eq!(placements.len(), 1);
        let p = &placements[0];
        assert_eq!(p.actor_node, actor);
        assert_eq!(p.pos, [1234, -7, 5678]);
        assert_eq!(p.slots, vec![0]);
        assert_eq!(p.man_record_id, Some(1));
        assert!(p.script_positioned);
        assert_eq!(p.fog_color_u24, Some(0x00CC_5511));
        assert_eq!(p.tick, ATMOSPHERIC_TICK);

        assert_eq!(pick_fog_color(&placements), Some(0x00CC_5511));
    }
}
