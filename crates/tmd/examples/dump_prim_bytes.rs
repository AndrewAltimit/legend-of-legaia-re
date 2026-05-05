//! Dump the raw bytes of the first prim in every group of one TMD.
//!
//! Used to investigate where UV/CBA/TSB/normal-index live within Legaia's
//! custom per-prim layout. PSX standard FT3 puts UVs at offsets 0,1 / 4,5 / 8,9
//! and CBA/TSB at offsets 2-3 / 6-7. Legaia's primitive walker tells us where
//! the *vertex indices* are; the rest of the prim data layout is unverified.
//!
//! Run: `cargo run -p legaia-tmd --example dump_prim_bytes -- <tmd-path>`

use anyhow::{Context, Result};
use legaia_tmd as tmd;
use legaia_tmd::legaia_prims;

fn main() -> Result<()> {
    let path = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("usage: dump_prim_bytes <tmd-path>"))?;
    let bytes = std::fs::read(&path).with_context(|| format!("read {}", path))?;
    let parsed = tmd::parse(&bytes)?;
    println!("file: {}  ({} bytes)", path, bytes.len());
    println!(
        "header: id=0x{:08X} nobj={} flags=0x{:08X}",
        parsed.header.id, parsed.header.nobj, parsed.header.flags
    );

    for (obj_idx, obj) in parsed.objects.iter().enumerate() {
        println!(
            "\n=== object {} === verts={} norms={} prim_section=[0x{:X}..0x{:X}] ({} bytes)",
            obj_idx,
            obj.header.n_vert,
            obj.header.n_normal,
            obj.primitives_byte_offset,
            obj.primitives_byte_offset + obj.primitives_byte_size,
            obj.primitives_byte_size
        );
        let groups = legaia_prims::iter_groups(
            &bytes,
            obj.primitives_byte_offset,
            obj.primitives_byte_size,
        )?;
        for (g_idx, g) in groups.iter().enumerate() {
            let stride = g.header.prim_stride();
            let n_v = g.header.n_vertices();
            let v_off = legaia_prims::vertex_offset_bytes(g.header.flags);
            let kind = if (g.header.flags >> 1) & 1 == 1 {
                "quad"
            } else {
                "tri"
            };
            println!(
                "  group {:2}: count={:3} flags=0x{:04X} olen={} ilen={} flag=0x{:02X} mode=0x{:02X}  stride={}  {}({}v) v_off={:?}",
                g_idx,
                g.header.count,
                g.header.flags,
                g.header.olen,
                g.header.ilen,
                g.header.flag,
                g.header.mode,
                stride,
                kind,
                n_v,
                v_off,
            );
            // Dump bytes of the first prim. Annotate the slice that contains
            // the vertex indices so we can see what's around them.
            let prim_off = g.header_offset + legaia_prims::GROUP_HEADER_SIZE;
            let end = (prim_off + stride).min(bytes.len());
            let slab = &bytes[prim_off..end];
            print!("    bytes:");
            for (i, b) in slab.iter().enumerate() {
                let mark = match v_off {
                    Some(v) if i >= v && i < v + n_v * 2 => "[V]",
                    _ => "",
                };
                print!(" {:02X}{}", b, mark);
            }
            println!();
        }
    }
    Ok(())
}
