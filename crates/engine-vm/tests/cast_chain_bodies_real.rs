//! Disc-gated: every phase-chain tick body
//! (`legaia_engine_vm::cast_module_ticks::CHAIN_BODIES`) checked against its
//! **owning** image's own bytes at slot-B base `0x801F69D8`.
//!
//! The descriptors are hand-read off the disassembly, and a band image ends
//! in a byte-identical copy of a sibling's tail, so a VA read in the wrong
//! image looks just as plausible. This test re-derives, per descriptor:
//!
//! 1. the body VA is the arm PROT 0898 names for it - the `0x801CF4EC` /
//!    `0x801CF56C` arm's own `jal` for a module with no trampoline, or a
//!    `jal` of the module's trampoline otherwise;
//! 2. every arm's landing VA is reached by the head - a branch / jump target
//!    inside the body, or a word of its dispatch table;
//! 3. every stage site is an `sb` to `+0x1DA` (or, for a restage-only stage,
//!    `+0x1DC`), every rate site an `sb` to `+0x21D`, and a literal clip is
//!    the immediate the stored register was last loaded with;
//! 4. every wrapper site is a `jal` into `FUN_801DD0AC` / `FUN_801DD4B0` /
//!    `FUN_801DD6B4`, and the body holds no wrapper call the descriptors do
//!    not name an arm for.
//!
//! Skips (and passes) without `LEGAIA_DISC_BIN` / `extracted/`.

use legaia_engine_vm::cast_module_ticks::{
    CAST_MODULE_LINK_BASE, CHAIN_BODIES, ChainBody, ChainClip, capture_trampoline_for,
};
use std::path::PathBuf;

const WRAPPERS: [u32; 3] = [0x801D_D0AC, 0x801D_D4B0, 0x801D_D6B4];
const BATTLE_BASE: u32 = 0x801C_E818;
const TICK_TABLE: u32 = 0x801C_F4EC;
const CAPTURE_TABLE: u32 = 0x801C_F56C;

fn extracted_dir() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    for base in ["extracted", "../../extracted"] {
        let p = PathBuf::from(base);
        if p.join("PROT.DAT").is_file() {
            return Some(p);
        }
    }
    eprintln!("[skip] extracted/PROT.DAT missing");
    None
}

struct Image {
    bytes: Vec<u8>,
    base: u32,
}

impl Image {
    fn word(&self, va: u32) -> u32 {
        let o = (va - self.base) as usize;
        u32::from_le_bytes(self.bytes[o..o + 4].try_into().expect("in image"))
    }
}

fn read_image(archive: &mut legaia_prot::archive::Archive, idx: u32, base: u32) -> Image {
    let entry = archive.entries[idx as usize].clone();
    let mut bytes = Vec::new();
    archive
        .read_entry(&entry, &mut bytes)
        .unwrap_or_else(|e| panic!("read PROT {idx}: {e:#}"));
    Image { bytes, base }
}

fn jal_target(w: u32) -> Option<u32> {
    (w >> 26 == 3).then_some(((w & 0x03FF_FFFF) << 2) | 0x8000_0000)
}

/// Every VA a branch / `j` / `jal` inside `[start, end)` lands on.
fn targets(img: &Image, start: u32, end: u32) -> Vec<u32> {
    let mut out = Vec::new();
    let mut va = start;
    while va < end {
        let w = img.word(va);
        let op = w >> 26;
        match op {
            // beq bne blez bgtz, and REGIMM (bltz/bgez)
            1 | 4..=7 => {
                let off = (w & 0xFFFF) as i16 as i32;
                out.push((va as i32 + 4 + (off << 2)) as u32);
            }
            2 | 3 => out.push(((w & 0x03FF_FFFF) << 2) | ((va + 4) & 0xF000_0000)),
            _ => {}
        }
        va += 4;
    }
    out
}

/// The body's extent: up to the next `addiu sp, sp, -N` past its start, or
/// the end of the image.
fn body_end(img: &Image, start: u32) -> u32 {
    let end = img.base + img.bytes.len() as u32;
    let mut va = start + 8;
    while va < end {
        let w = img.word(va);
        if w >> 16 == 0x27BD && w & 0x8000 != 0 {
            return va;
        }
        va += 4;
    }
    end
}

/// `sb rt, imm(base)` -> `(rt, imm)`.
fn sb_fields(w: u32) -> Option<(u32, u16)> {
    (w >> 26 == 0x28).then_some(((w >> 16) & 31, (w & 0xFFFF) as u16))
}

/// The immediate `rt` was last loaded with by `addiu rt, zero, K` (or `ori
/// rt, zero, K`) in the sixteen words before `site`, or in its delay slot when
/// the store sits in one. The walk stops at `floor` (the arm's landing VA):
/// above it is another arm's code, or the head's compare literals.
fn literal_in(img: &Image, site: u32, rt: u32, floor: u32) -> Option<u16> {
    if rt == 0 {
        return Some(0);
    }
    for back in 1..=16u32 {
        let va = site - 4 * back;
        if va < floor {
            break;
        }
        let w = img.word(va);
        let (op, rs, t) = (w >> 26, (w >> 21) & 31, (w >> 16) & 31);
        if t == rt && rs == 0 && (op == 9 || op == 0xD) {
            return Some((w & 0xFFFF) as u16);
        }
    }
    None
}

fn check(body: &ChainBody, img: &Image, battle: &Image) -> usize {
    let e = body.prot_entry;
    let end = body_end(img, body.body);
    // 1. PROT 0898 names this body for this entry.
    let row = e - if e < 935 { 903 } else { 935 };
    let table = if e < 935 { TICK_TABLE } else { CAPTURE_TABLE };
    let arm_va = battle.word(table + 4 * row);
    let arm_jal = (0..4)
        .filter_map(|i| jal_target(battle.word(arm_va + 4 * i)))
        .next()
        .unwrap_or_else(|| panic!("PROT {e}: 0898 arm {arm_va:#x} holds no jal"));
    match capture_trampoline_for(e) {
        None => assert_eq!(arm_jal, body.body, "PROT {e}: the tick arm calls the body"),
        Some(t) => {
            assert_eq!(
                arm_jal, t.trampoline,
                "PROT {e}: the tick arm calls the trampoline"
            );
            let tr_end = body_end(img, t.trampoline);
            let calls: Vec<u32> = (t.trampoline..tr_end)
                .step_by(4)
                .filter_map(|va| jal_target(img.word(va)))
                .collect();
            assert!(
                calls.contains(&body.body),
                "PROT {e}: trampoline {:#x} never calls {:#x}",
                t.trampoline,
                body.body
            );
        }
    }

    // 2. Every arm entry is reached by the head.
    let tg = targets(img, body.body, end);
    let words: Vec<u32> = (img.base..img.base + img.bytes.len() as u32 - 3)
        .step_by(4)
        .map(|va| img.word(va))
        .collect();
    let mut sites = 0;
    let mut wrapper_arms = Vec::new();
    for arm in body.arms {
        assert!(
            (body.body..end).contains(&arm.entry),
            "PROT {e} arm {:#x}: {:#x} outside the body {:#x}..{end:#x}",
            arm.phase,
            arm.entry,
            body.body
        );
        assert!(
            tg.contains(&arm.entry) || words.contains(&arm.entry),
            "PROT {e} arm {:#x}: nothing in the head reaches {:#x}",
            arm.phase,
            arm.entry
        );
        // 3. Stage and rate sites.
        for st in arm.stages {
            let (rt, imm) = sb_fields(img.word(st.site))
                .unwrap_or_else(|| panic!("PROT {e} {:#x}: not an sb", st.site));
            let want = if st.clip == ChainClip::Keep {
                0x1DC
            } else {
                0x1DA
            };
            assert_eq!(imm, want, "PROT {e} {:#x}: stage offset", st.site);
            if let ChainClip::Literal(k) = st.clip {
                assert_eq!(
                    literal_in(img, st.site, rt, arm.entry),
                    Some(u16::from(k)),
                    "PROT {e} {:#x}: staged literal",
                    st.site
                );
            }
            sites += 1;
        }
        for r in arm.rates {
            let (rt, imm) = sb_fields(img.word(r.site))
                .unwrap_or_else(|| panic!("PROT {e} {:#x}: not an sb", r.site));
            assert_eq!(imm, 0x21D, "PROT {e} {:#x}: rate offset", r.site);
            if let Some(k) = literal_in(img, r.site, rt, arm.entry) {
                assert_eq!(k, u16::from(r.rate), "PROT {e} {:#x}: rate literal", r.site);
            }
            sites += 1;
        }
        // 4. Wrapper sites.
        if let Some(w) = arm.wrapper_site {
            let t = jal_target(img.word(w)).unwrap_or_else(|| panic!("PROT {e} {w:#x}: not a jal"));
            assert!(
                WRAPPERS.contains(&t),
                "PROT {e} {w:#x}: jal {t:#x} is no wrapper"
            );
            wrapper_arms.push(w);
            sites += 1;
        }
    }
    // No wrapper call the descriptors leave unnamed for a whole arm.
    let named_arms = body
        .arms
        .iter()
        .filter(|a| a.wrapper_site.is_some())
        .count();
    let calls = (body.body..end)
        .step_by(4)
        .filter(|&va| jal_target(img.word(va)).is_some_and(|t| WRAPPERS.contains(&t)))
        .count();
    assert!(
        calls >= named_arms && (calls == 0) == (named_arms == 0),
        "PROT {e}: {calls} wrapper calls in the body, {named_arms} arms name one"
    );
    sites
}

#[test]
fn every_chain_body_matches_its_owning_image() {
    let Some(dir) = extracted_dir() else {
        return;
    };
    let mut archive =
        legaia_prot::archive::Archive::open(&dir.join("PROT.DAT")).expect("open PROT.DAT");
    let battle = read_image(&mut archive, 898, BATTLE_BASE);
    let mut sites = 0;
    for body in &CHAIN_BODIES {
        let img = read_image(&mut archive, body.prot_entry, CAST_MODULE_LINK_BASE);
        sites += check(body, &img, &battle);
    }
    println!(
        "[ran] {} chain bodies, {sites} stage / rate / wrapper sites matched",
        CHAIN_BODIES.len()
    );
}
