//! Disc-gated: which scratchpad global bits (`_DAT_1F800394`) the disc's
//! scripts can raise.
//!
//! Two script VMs write that word with a bit index taken from their own
//! operand byte, so "does anything set bit N" is a question about authored
//! bytes, not about code:
//!
//! * the field VM's `0x2E` `GFLAG_SET` / `0x2F` `GFLAG_CLR` - `[op, bit]`, the
//!   arm at `0x801DED48` / `0x801DED70` in the field overlay, `1 << operand`
//!   `or`-ed into (`nor`/`and`-ed out of) the word; and
//! * the scripted-motion VM's `0x10` / `0x11` bit ops - `[op, b1]`, where
//!   `b1 & 0xC0 == 0x80` selects the word's low (`b1 & 0x30` non-zero) or
//!   high (`b1 & 0x30` zero) halfword and `b1 & 0x0F` the bit in it
//!   (`0x80039500` / `0x80039590` in `SCUS_942.54`).
//!
//! The field VM's side is counted by `asset field-op-census --only 2E`; this
//! test walks the motion-VM streams (MAN tail section 1) of every MAN the disc
//! carries - each scene bundle's type-3 descriptor and each DATA_FIELD
//! stream's type-3 chunk - and pins the one fact a port decision rests on:
//! **no motion stream raises global bit 23** (`0x00800000`, the fade-pipeline
//! fork op `0x34` sub-0 tests). On retail the answer is stronger than that:
//! no stream authors a `0x10` / `0x11` bit op in any bank, while the same walk
//! finds the `0x07` / `0x08` system-flag ops, so the zero is the disc's and not
//! the walk's. Skips + passes without `extracted/PROT`.

use legaia_asset::{man_motion, man_section, scene_asset_table};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn prot_dir() -> Option<PathBuf> {
    for c in [
        "extracted/PROT",
        "../extracted/PROT",
        "../../extracted/PROT",
    ] {
        let d = PathBuf::from(c);
        if d.is_dir() {
            return Some(d);
        }
    }
    eprintln!("[skip] extracted/PROT missing - run `legaia-extract` first");
    None
}

/// Every MAN payload an entry carries.
fn mans(buf: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    if let Some(r) = scene_asset_table::resolve(buf) {
        for d in r.table.used() {
            if d.type_byte != 0x03 {
                continue;
            }
            let off = r.table_base + d.data_offset as usize;
            if let Some(src) = buf.get(off..)
                && let Ok(m) = legaia_lzs::decompress(src, d.size as usize)
            {
                out.push(m);
            }
        }
    } else if let Ok(rep) = legaia_asset::parse_streaming(buf, 4096)
        && rep.terminated
    {
        for c in rep.chunks.iter().filter(|c| c.type_byte == 0x03) {
            let s = c.header_offset + 4;
            if let Some(m) = buf.get(s..s + c.size as usize) {
                out.push(m.to_vec());
            }
        }
    }
    out
}

#[test]
fn no_motion_stream_raises_global_bit_23() {
    let Some(dir) = prot_dir() else { return };
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .collect();
    paths.sort();
    // (op, global bit) -> sites
    let mut hits: BTreeMap<(u8, u8), usize> = BTreeMap::new();
    let (mut man_count, mut variants, mut bit_ops, mut sys_flag_ops) =
        (0usize, 0usize, 0usize, 0usize);
    for p in paths {
        let Ok(buf) = std::fs::read(&p) else { continue };
        for man in mans(&buf) {
            let Ok(mf) = man_section::parse(&man) else {
                continue;
            };
            man_count += 1;
            for rec in man_motion::motion_records(&man, &mf) {
                for var in man_motion::stream_variants(&man, &rec) {
                    variants += 1;
                    let mut pc = var.code_offset;
                    while pc < var.code_end && pc < man.len() {
                        let op = man[pc];
                        let Some(w) = man_motion::op_width(op) else {
                            break;
                        };
                        if matches!(op, 0x10 | 0x11) {
                            bit_ops += 1;
                        }
                        if matches!(op, 0x07 | 0x08) {
                            sys_flag_ops += 1;
                        }
                        if matches!(op, 0x10 | 0x11)
                            && let Some(&b1) = man.get(pc + 1)
                            && b1 & 0xC0 == 0x80
                        {
                            let high = b1 & 0x30 == 0;
                            let bit = (b1 & 0x0F) + if high { 16 } else { 0 };
                            *hits.entry((op, bit)).or_default() += 1;
                        }
                        pc += w;
                    }
                }
            }
        }
    }
    eprintln!(
        "[scratch-bits] {man_count} MANs, {variants} motion variants, {bit_ops} 0x10/0x11 bit ops \
         (any bank), {sys_flag_ops} 0x07/0x08 system-flag ops (control)"
    );
    for ((op, bit), n) in &hits {
        let kind = if *op == 0x10 { "set" } else { "clear" };
        eprintln!("[scratch-bits]   {kind} bit {bit}: {n} site(s)");
    }
    assert!(man_count > 90, "found only {man_count} MANs");
    // Non-vacuous: the same walk does reach the motion VM's OTHER flag ops,
    // the system-flag writes `man_motion::motion_flag_sites` reads.
    assert!(sys_flag_ops > 0, "the walk reached no 0x07/0x08 op either");
    assert!(
        !hits.contains_key(&(0x10, 23)),
        "a motion stream raises global bit 23: {hits:?}"
    );
}
