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

/// The substitution arms (`0xC1`..`0xC5`, `0xC7`, jump table `0x80010EA0`)
/// and the nested `0xC1` pass: random party names (`0x800849AF + X *
/// 0x414`, `X = 0x63` through `0x80084597`), item and spell name pointers,
/// the inline names at `0x80073F24` and an arts table at `0x80075EC4`, each
/// name carrying two-byte pairs and nested `0xC1` tokens. The expander hands
/// the port the bytes retail's arm resolves to (the arts arm's table search
/// is the expander's, mirrored from `0x80036308..0x80036384`); the count is
/// the port's.
#[test]
fn the_substituted_names_count_as_retail_counts_them() {
    differential!(scus(COUNT));
    let Some(mut cpu) = scus_cpu() else {
        return;
    };
    const PARTY: u32 = 0x8008_49AF;
    const PARTY_PICK: u32 = 0x8008_4597;
    const ITEMS: u32 = 0x8007_4368;
    const SPELLS: u32 = 0x8007_54C8;
    const INLINE: u32 = 0x8007_3F24;
    const ARTS: u32 = 0x8007_5EC4;
    const NAMES: u32 = 0x8015_0000;
    let mut rng = Rng(0x5355_4253);
    let name = |rng: &mut Rng, nest: bool| -> Vec<u8> {
        let mut s = Vec::new();
        for _ in 0..rng.below(10) {
            match rng.below(10) {
                0 if nest => s.extend([0xC1, [0, 1, 2, 3, 0x63][rng.below(5) as usize]]),
                1 => s.extend([
                    0xC0 | rng.below(16) as u8 & 0x0F,
                    0x20 + rng.below(0x40) as u8,
                ]),
                2 => s.push(1 + rng.below(0x1E) as u8),
                _ => s.push(0x20 + rng.below(0x5C) as u8),
            }
        }
        s.push(0);
        s
    };
    let mut seen = [0u32; 8];
    for case in 0..20_000 {
        let mut next = NAMES;
        let mut place = |cpu: &mut psx_retail_exec::Cpu, s: &[u8]| -> u32 {
            let at = next;
            cpu.mem.load(at, s);
            next += 0x40;
            at
        };
        for k in 0..4u32 {
            let s = name(&mut rng, false);
            cpu.mem.load(PARTY + k * 0x414, &s[..s.len().min(16)]);
            cpu.mem.wr8(PARTY + k * 0x414 + 15, 0);
        }
        cpu.mem.wr8(PARTY_PICK, rng.below(4) as u8);
        for id in 0..8u32 {
            let s = name(&mut rng, true);
            let at = place(&mut cpu, &s);
            cpu.mem.wr32(ITEMS + 12 * id + 4, at);
            let s = name(&mut rng, true);
            let at = place(&mut cpu, &s);
            cpu.mem.wr32(SPELLS + 12 * id + 8, at);
            let s = name(&mut rng, true);
            cpu.mem.load(INLINE + 8 * id, &s[..s.len().min(8)]);
        }
        cpu.mem.wr8(INLINE + 8 * 8, 0);
        // Arts records sorted on their first key byte, a sentinel past them.
        let mut key = 0u8;
        for k in 0..8u32 {
            key += rng.below(2) as u8;
            let s = name(&mut rng, true);
            let at = place(&mut cpu, &s);
            cpu.mem.wr8(ARTS + 0x14 * k, key);
            cpu.mem.wr8(ARTS + 0x14 * k + 1, rng.below(4) as u8);
            cpu.mem.wr32(ARTS + 0x14 * k + 0xC, at);
        }
        cpu.mem.wr8(ARTS + 0x14 * 8, 0xFF);
        let ram = cpu.mem.ram().to_vec();
        let rd = |a: u32| ram[(a & 0x1F_FFFF) as usize];
        let rd32 = |a: u32| u32::from_le_bytes(std::array::from_fn(|k| rd(a + k as u32)));
        let bytes_at = |a: u32| -> Vec<u8> {
            let mut v: Vec<u8> = (0..64).map(|k| rd(a + k)).collect();
            v.push(0);
            v
        };
        let expand = |op: u8, arg: u8| -> Option<Vec<u8>> {
            let at = match op {
                0xC1 => {
                    let who = if arg == 0x63 { rd(PARTY_PICK) } else { arg };
                    PARTY + u32::from(who) * 0x414
                }
                0xC2 | 0xC4 => rd32(ITEMS + 12 * u32::from(arg) + 4),
                0xC3 => rd32(SPELLS + 12 * u32::from(arg) + 8),
                0xC7 => INLINE + 8 * u32::from(arg),
                0xC5 => {
                    let (hi, lo) = (arg >> 6, arg & 0x3F);
                    let mut k = 0u32;
                    if hi >= rd(ARTS) {
                        loop {
                            let r = ARTS + 0x14 * k;
                            if rd(r) == hi && rd(r + 1) == lo {
                                break;
                            }
                            k += 1;
                            if hi < rd(ARTS + 0x14 * k) {
                                break;
                            }
                        }
                    }
                    rd32(ARTS + 0x14 * k + 0xC)
                }
                _ => return None,
            };
            Some(bytes_at(at))
        };
        let mut s = Vec::new();
        for _ in 0..rng.below(8) {
            match rng.below(4) {
                0 => s.push(0x20 + rng.below(0x5C) as u8),
                _ => {
                    let op =
                        [0xC1u8, 0xC2, 0xC3, 0xC4, 0xC5, 0xC7, 0xC0, 0xC6][rng.below(8) as usize];
                    let arg = match op {
                        0xC1 => [0, 1, 2, 3, 0x63][rng.below(5) as usize],
                        0xC5 => (rng.below(5) as u8) << 6 | rng.below(4) as u8,
                        _ => rng.below(8) as u8,
                    };
                    seen[usize::from(op - 0xC0)] += 1;
                    s.extend([op, arg]);
                }
            }
        }
        if s.first().is_some_and(|&b| b < 0x1F) || s.is_empty() {
            s.insert(0, b'A');
        }
        s.push(0);
        s.extend([b'x'; 8]);
        s.push(0);
        cpu.mem.load(TEXT, &s);
        assert_eq!(cpu.call(COUNT, &[TEXT]), Stop::Returned, "case {case}");
        let retail = cpu.regs.r[V0];
        let port = typewriter_glyph_count(&s, Some(&expand));
        assert!(
            port.unresolved.is_empty(),
            "case {case}: {:?}",
            port.unresolved
        );
        assert_eq!(retail, port.count, "case {case}: {s:02x?}");
    }
    assert!(seen.iter().all(|&n| n > 1000), "{seen:?}");
}
