//! Menu-overlay **window widget scripts** - the bytecode programs the
//! window-script VM `FUN_801D6628` interprets.
//!
//! The VM (docs/subsystems/actor-vm.md; port `legaia_engine_vm::run`) walks a
//! stream of fixed 4-byte instructions `[opcode u8][window u8][operand u16 LE]`
//! terminated by opcode `0x00`. The window byte indexes the menu overlay's
//! 52-record window descriptor table at VA `0x801E4738`
//! ([`crate::menu_windows`]): the VM computes `0x801E4738 + window * 0x10`
//! (`lui 0x801e / addiu 0x4738` at `0x801D6658..0x801D665C`) and takes the
//! record's `x`/`y` as the instruction's default coordinates. Opcode dispatch
//! is `sltiu v0, opcode-1, 0xd` over the 13-entry jump table at `0x801CED70`.
//! `see ghidra/scripts/funcs/overlay_menu_801d6628.txt`.
//!
//! The programs are **data resident in the menu overlay itself** (PROT 0899,
//! base `0x801CE818`) - a script table region in the overlay's data segment
//! (file `0x16260..0x16740`), not a scene MAN section or an effect bundle.
//! Each caller of the VM materialises a program pointer with `lui`+`addiu`
//! (or forwards one through a register) and calls `FUN_801D6628(&script)`.
//! [`scan`] recovers those programs from the as-loaded overlay image by
//! decoding every `jal 0x801D6628` site and its `a0` materialisation;
//! [`parse_at`] parses one program at a known VA.
//!
//! Known pinned programs (byte-verified against the disc image; the shop pair
//! is additionally pinned by the randomizer's seru-trading vendor, which
//! reuses exactly these scripts - `legaia_patcher::seru_overlay::consts`):
//!
//! | VA | contents | caller |
//! |---|---|---|
//! | `0x801E4E38` | `[05][01 21][01 2A][01 20][01 28][01 22][00]` | shop picker open (`FUN_801DAFD4`) |
//! | `0x801E4E54` | `[04 28][04 2A][04 22][00]` | shop Sell transition slide-away (`FUN_801DAFD4`) |
//! | `0x801E4A78` | `[05][00]` | menu open staging (multiple overlay callers) |
//!
//! Opcode semantics live with the interpreter port (`legaia_engine_vm`);
//! this module only resolves and validates the byte streams.

use anyhow::{Result, bail};
use legaia_bytes::u32_le;

use crate::menu_windows::{MENU_OVERLAY_BASE_VA, MENU_WINDOW_COUNT};

/// VA of the window-script VM `FUN_801D6628` in the resident menu overlay.
pub const WIDGET_VM_VA: u32 = 0x801D_6628;

/// The shop picker's window **open** script (`DAT_801E4E38`): global update,
/// then open windows `0x21` (vendor plate), `0x2A` (Buy/Sell picker),
/// `0x20` (gold), `0x28`, `0x22`. Run by the picker dispatcher
/// `FUN_801DAFD4` when the Buy/Sell/Quit picker comes up
/// (docs/subsystems/shop.md).
pub const SHOP_OPEN_SCRIPT_VA: u32 = 0x801E_4E38;

/// The shop **Sell transition** slide-away script (`DAT_801E4E54`): close
/// windows `0x28`, `0x2A`, `0x22`, keeping the gold (`0x20`) and vendor
/// plate (`0x21`) on screen.
pub const SHOP_SELL_AWAY_SCRIPT_VA: u32 = 0x801E_4E54;

/// The menu-open staging script (`DAT_801E4A78`): a single global update.
pub const MENU_OPEN_SCRIPT_VA: u32 = 0x801E_4A78;

/// Instruction size in bytes (fixed-width stream).
pub const INSN_SIZE: usize = 4;

/// Parse bound: no disc-resident program exceeds this many instructions
/// (the longest scanned program in PROT 0899 is 8).
pub const MAX_SCRIPT_INSNS: usize = 64;

/// Highest opcode the VM's `sltiu opcode-1, 0xd` range check dispatches
/// (opcodes above it fall through as no-ops; a parse treats them as
/// evidence the bytes are not a script).
pub const MAX_OPCODE: u8 = 0x0D;

/// One decoded 4-byte instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WidgetInsn {
    /// Opcode byte (`0x01..=0x0D`; `0x00` terminates and is not stored).
    pub opcode: u8,
    /// Window id - index into the menu window descriptor table
    /// ([`crate::menu_windows`], 52 records).
    pub window: u8,
    /// Little-endian u16 operand (packed position for opcodes `0x02`/`0x09`,
    /// style byte for `0x03`; zero elsewhere on disc).
    pub operand: u16,
}

/// One resolved program: its VA and decoded instructions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WidgetScript {
    /// VA of the first instruction in the resident overlay.
    pub va: u32,
    /// Decoded instructions, terminator excluded.
    pub insns: Vec<WidgetInsn>,
}

impl WidgetScript {
    /// Byte length of the program **including** the 4-byte terminator -
    /// the slice length the interpreter (`legaia_engine_vm::run`) consumes.
    pub fn byte_len(&self) -> usize {
        (self.insns.len() + 1) * INSN_SIZE
    }
}

/// A program recovered by [`scan`]: the script plus every `jal` call site
/// whose `a0` materialisation resolved to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WidgetScriptRef {
    pub script: WidgetScript,
    /// VAs of the `jal FUN_801D6628` instructions referencing this program.
    pub call_sites: Vec<u32>,
}

fn file_offset(overlay: &[u8], va: u32) -> Result<usize> {
    if va < MENU_OVERLAY_BASE_VA {
        bail!("VA {va:#010x} below menu overlay base {MENU_OVERLAY_BASE_VA:#010x}");
    }
    let off = (va - MENU_OVERLAY_BASE_VA) as usize;
    if off >= overlay.len() {
        bail!(
            "VA {va:#010x} (file {off:#x}) past overlay image end ({:#x})",
            overlay.len()
        );
    }
    Ok(off)
}

/// Parse the widget script at `va` out of the as-loaded menu overlay image
/// (PROT 0899 extended entry bytes). Rejects streams that run past
/// [`MAX_SCRIPT_INSNS`], carry an opcode above [`MAX_OPCODE`], or index a
/// window at or past the descriptor-table bound - the LZS-style "decodes
/// without error" trap applies here too, so a parse is a validity check,
/// not just a slice.
pub fn parse_at(overlay: &[u8], va: u32) -> Result<WidgetScript> {
    let base = file_offset(overlay, va)?;
    let mut insns = Vec::new();
    for k in 0..=MAX_SCRIPT_INSNS {
        let off = base + k * INSN_SIZE;
        if off + INSN_SIZE > overlay.len() {
            bail!("widget script at {va:#010x} runs past the overlay image");
        }
        let opcode = overlay[off];
        if opcode == 0 {
            return Ok(WidgetScript { va, insns });
        }
        if k == MAX_SCRIPT_INSNS {
            break;
        }
        let window = overlay[off + 1];
        if opcode > MAX_OPCODE {
            bail!("widget script at {va:#010x}: opcode {opcode:#04x} at +{k} out of range");
        }
        if (window as usize) >= MENU_WINDOW_COUNT {
            bail!("widget script at {va:#010x}: window {window:#04x} at +{k} out of range");
        }
        let operand = u16::from_le_bytes([overlay[off + 2], overlay[off + 3]]);
        insns.push(WidgetInsn {
            opcode,
            window,
            operand,
        });
    }
    bail!("widget script at {va:#010x}: no terminator within {MAX_SCRIPT_INSNS} instructions")
}

/// Raw program bytes at `va`, **including** the 4-byte terminator - the
/// exact disc bytes to hand `legaia_engine_vm::run`. Validates via
/// [`parse_at`] first.
pub fn script_bytes_at(overlay: &[u8], va: u32) -> Result<Vec<u8>> {
    let script = parse_at(overlay, va)?;
    let base = file_offset(overlay, va)?;
    Ok(overlay[base..base + script.byte_len()].to_vec())
}

/// Encoding of `jal FUN_801D6628`: opcode `0x03` in the top 6 bits, the
/// 26-bit instruction index in the low bits (target region bits come from
/// the delay-slot PC, which for overlay code is the same `0x8xxxxxxx`
/// segment).
fn jal_word(target: u32) -> u32 {
    0x0C00_0000 | ((target >> 2) & 0x03FF_FFFF)
}

/// Recover the `a0` argument at the `jal` in word slot `i`, reading the
/// delay slot plus a small backward window for the standard
/// `lui a0, hi` / `addiu a0, a0, lo` (or `ori`) materialisation. Any
/// other write into `a0` in between (register move, load, `addiu` from a
/// different base) invalidates the pair - those call sites forward a
/// pointer computed elsewhere and are reported unresolved.
fn recover_a0(words: &[u32], i: usize) -> Option<u32> {
    const A0: u32 = 4;
    const BACK_WINDOW: usize = 8;
    let mut hi: Option<u32> = None;
    let mut val: Option<u32> = None;
    let lo_bound = i.saturating_sub(BACK_WINDOW);
    // Program order: the backward window, then the delay slot.
    let seq = (lo_bound..i).chain(std::iter::once(i + 1));
    for j in seq {
        let Some(&w) = words.get(j) else { continue };
        let op = w >> 26;
        let rs = (w >> 21) & 31;
        let rt = (w >> 16) & 31;
        let imm = w & 0xFFFF;
        match op {
            0x0F if rt == A0 => {
                // lui a0, imm
                hi = Some(imm << 16);
                val = hi;
            }
            0x09 if rt == A0 && rs == A0 && hi.is_some() => {
                // addiu a0, a0, simm
                let simm = imm as i16 as i32;
                val = Some((hi.unwrap() as i32).wrapping_add(simm) as u32);
            }
            0x0D if rt == A0 && rs == A0 && hi.is_some() => {
                // ori a0, a0, imm
                val = Some(hi.unwrap() | imm);
            }
            // Any other I-type write to a0 (addiu from another base, load,
            // ...) breaks the pair.
            0x08 | 0x09 | 0x0A | 0x0B | 0x0C | 0x0D | 0x0E | 0x20..=0x26 | 0x30..=0x32
                if rt == A0 =>
            {
                hi = None;
                val = None;
            }
            // R-type with rd == a0 (move / addu / ...) breaks the pair.
            0x00 if ((w >> 11) & 31) == A0 && (w & 0x3F) != 0 => {
                hi = None;
                val = None;
            }
            _ => {}
        }
    }
    val
}

/// Sweep the as-loaded menu overlay image for `jal FUN_801D6628` call
/// sites, recover each site's `a0` program pointer, and return every
/// distinct program that parses as a valid widget script (sorted by VA).
///
/// Call sites whose pointer is forwarded through a register (the shop
/// pair among them - `FUN_801DAFD4` stages both scripts in saved
/// registers) are not resolvable by this pass; the pinned constants above
/// cover the ones production code needs.
pub fn scan(overlay: &[u8]) -> Vec<WidgetScriptRef> {
    let n = overlay.len() / 4;
    let mut words = Vec::with_capacity(n);
    for i in 0..n {
        // In-bounds by construction; u32_le only fails past the end.
        words.push(u32_le(overlay, i * 4).unwrap_or(0));
    }
    let jal = jal_word(WIDGET_VM_VA);
    let mut by_va: std::collections::BTreeMap<u32, Vec<u32>> = std::collections::BTreeMap::new();
    for i in 0..n {
        if words[i] != jal {
            continue;
        }
        if let Some(va) = recover_a0(&words, i) {
            by_va
                .entry(va)
                .or_default()
                .push(MENU_OVERLAY_BASE_VA + (i as u32) * 4);
        }
    }
    by_va
        .into_iter()
        .filter_map(|(va, call_sites)| {
            parse_at(overlay, va)
                .ok()
                .map(|script| WidgetScriptRef { script, call_sites })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a synthetic overlay image: `code` words at the image start,
    /// `script` bytes at `script_off`. No Sony bytes - hand-authored.
    fn synth(code: &[u32], script_off: usize, script: &[u8]) -> Vec<u8> {
        let mut img = vec![0u8; script_off + script.len() + 16];
        for (i, w) in code.iter().enumerate() {
            img[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
        }
        img[script_off..script_off + script.len()].copy_from_slice(script);
        img
    }

    #[test]
    fn parse_accepts_terminated_script() {
        let script = [
            0x05, 0x00, 0x00, 0x00, // GlobalUpdate
            0x01, 0x21, 0x00, 0x00, // open window 0x21
            0x00, 0x00, 0x00, 0x00, // End
        ];
        let img = synth(&[], 0x100, &script);
        let va = MENU_OVERLAY_BASE_VA + 0x100;
        let s = parse_at(&img, va).unwrap();
        assert_eq!(s.insns.len(), 2);
        assert_eq!(s.insns[1].window, 0x21);
        assert_eq!(s.byte_len(), 12);
        assert_eq!(script_bytes_at(&img, va).unwrap(), script);
    }

    #[test]
    fn parse_rejects_bad_opcode_and_window() {
        let img = synth(&[], 0x100, &[0x0E, 0x00, 0x00, 0x00, 0, 0, 0, 0]);
        assert!(parse_at(&img, MENU_OVERLAY_BASE_VA + 0x100).is_err());
        let img = synth(&[], 0x100, &[0x01, 0x40, 0x00, 0x00, 0, 0, 0, 0]);
        assert!(parse_at(&img, MENU_OVERLAY_BASE_VA + 0x100).is_err());
    }

    #[test]
    fn parse_rejects_unterminated() {
        let img = synth(
            &[],
            0x100,
            &[0x01u8, 0x02, 0, 0].repeat(MAX_SCRIPT_INSNS + 2),
        );
        assert!(parse_at(&img, MENU_OVERLAY_BASE_VA + 0x100).is_err());
    }

    #[test]
    fn scan_recovers_lui_addiu_site() {
        // Script placed at file 0x200 => VA base + 0x200.
        let script_va = MENU_OVERLAY_BASE_VA + 0x200;
        let hi = (script_va >> 16) + u32::from(script_va & 0x8000 != 0);
        let lo = script_va & 0xFFFF;
        let code = [
            0x3C04_0000 | hi,       // lui a0, hi
            0x2484_0000 | lo,       // addiu a0, a0, lo
            jal_word(WIDGET_VM_VA), // jal FUN_801D6628
            0x0000_0000,            // nop (delay slot)
        ];
        let img = synth(
            &code,
            0x200,
            &[0x01, 0x05, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
        );
        let refs = scan(&img);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].script.va, script_va);
        assert_eq!(refs[0].call_sites, vec![MENU_OVERLAY_BASE_VA + 8]);
        assert_eq!(refs[0].script.insns[0].window, 0x05);
    }

    #[test]
    fn scan_skips_clobbered_a0() {
        let script_va = MENU_OVERLAY_BASE_VA + 0x200;
        let hi = (script_va >> 16) + u32::from(script_va & 0x8000 != 0);
        let lo = script_va & 0xFFFF;
        let code = [
            0x3C04_0000 | hi, // lui a0, hi
            0x2484_0000 | lo, // addiu a0, a0, lo
            0x0000_2021,      // move a0, zero (addu a0, zero, zero)
            jal_word(WIDGET_VM_VA),
            0x0000_0000,
        ];
        let img = synth(
            &code,
            0x200,
            &[0x01, 0x05, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
        );
        assert!(scan(&img).is_empty());
    }
}
