//! The typewriter glyph count `FUN_80036044` against `typewriter_glyph_count`
//! over generated strings: the pre-walk length, plain glyphs, the two-byte
//! `0xCE` / `0xCF` and `0xC0` / `0xC6` forms and the read past the `NUL`. The
//! substitution forms, which reach the name tables, are left to the disc
//! strings' own oracle (`crates/engine-scene-host/tests/dialog_typewriter_pacing_disc.rs`).

use crate::common::{Rng, scus_cpu};
use legaia_font::glyph_count::typewriter_glyph_count;
use psx_retail_exec::{Stop, V0};

const COUNT: u32 = 0x8003_6044;
const TEXT: u32 = 0x8014_0000;

#[test]
fn the_glyph_count_is_retails() {
    differential!(scus(COUNT));
    let Some(mut cpu) = scus_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x474C_5950);
    let (mut escaped, mut failures) = (0u32, Vec::new());
    for _ in 0..60_000 {
        let mut s = Vec::new();
        let n = rng.below(40);
        for _ in 0..n {
            match rng.below(16) {
                0 => s.extend([0xCE, rng.below(0x40) as u8]),
                1 => s.extend([0xCF, 1 + rng.below(8) as u8]),
                2 => s.extend([0xC0, rng.below(0x20) as u8]),
                3 => s.extend([0xC6, 1 + rng.below(0x20) as u8]),
                4 => s.push(0x5E),
                5 => s.push(0x80 + rng.below(0x40) as u8),
                // Bytes below `0x1F` stop the walk at an embedded terminator.
                6 => s.push(1 + rng.below(0x1E) as u8),
                _ => s.push(0x20 + rng.below(0x5C) as u8),
            }
        }
        // The word after the NUL, which the loop runs into.
        let mut buf = s.clone();
        buf.push(0);
        buf.extend((0..16).map(|_| {
            if rng.below(3) == 0 {
                0
            } else {
                // No substitution forms past the end: each dereferences a name
                // table with whatever argument follows.
                match 0x20 + rng.below(0xD0) as u8 {
                    b @ 0xC1..=0xC5 | b @ 0xC7 => b & !0x40,
                    b => b,
                }
            }
        }));
        cpu.mem.load(TEXT, &buf);
        cpu.mem.load(TEXT + buf.len() as u32, &[0u8; 8]);
        cpu.regs.r[V0] = 0;
        assert_eq!(cpu.call(COUNT, &[TEXT]), Stop::Returned, "{s:02x?}");
        let retail = cpu.regs.r[V0];
        let port = typewriter_glyph_count(&buf, None).count;
        if retail != port && failures.len() < 16 {
            failures.push(format!("{buf:02x?}: retail {retail} port {port}"));
        }
        escaped += u32::from(s.iter().any(|b| b & 0xF0 == 0xC0));
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(escaped > 20_000, "{escaped}");
}
