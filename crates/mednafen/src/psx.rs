//! Typed accessors over the PSX `MAIN` section's MIPS-CPU and GTE register
//! state.
//!
//! Mednafen's PSX module stores the cop0 / cop2 register file inside the
//! `MAIN` section as named sub-entries — `CPU.PC`, `CPU.GPR`, `GTE.MAC`, etc.
//! Naming and layout aren't strictly stable across mednafen versions, so this
//! module is conservative: it walks sub-entries by name and parses what's
//! present, returning `None` for entries the build doesn't expose.
//!
//! For overlay-resident reverse-engineering work the most useful accessor is
//! `cpu_pc` — given a save state taken right after a function-call boundary,
//! the PC pinpoints the function we want to dump from Ghidra.

use crate::container::SaveState;

/// CPU register-file accessors. All fields are populated lazily — read what
/// you need, miss what you don't.
#[derive(Debug, Clone, Default)]
pub struct CpuRegs {
    pub pc: Option<u32>,
    pub hi: Option<u32>,
    pub lo: Option<u32>,
    pub gpr: Option<[u32; 32]>,
}

/// Top-level helpers over the `MAIN` section.
#[derive(Debug, Clone)]
pub struct PsxMain<'a> {
    save: &'a SaveState,
}

impl<'a> PsxMain<'a> {
    pub fn new(save: &'a SaveState) -> Self {
        Self { save }
    }

    /// Read the CPU register file, falling back to default for any entry
    /// the save state doesn't expose.
    pub fn cpu_regs(&self) -> CpuRegs {
        let mut out = CpuRegs::default();
        if let Some(bytes) = self.save.entry_bytes("MAIN", "CPU.PC") {
            out.pc = read_u32_le(bytes);
        }
        if let Some(bytes) = self.save.entry_bytes("MAIN", "CPU.HI") {
            out.hi = read_u32_le(bytes);
        }
        if let Some(bytes) = self.save.entry_bytes("MAIN", "CPU.LO") {
            out.lo = read_u32_le(bytes);
        }
        if let Some(bytes) = self.save.entry_bytes("MAIN", "CPU.GPR")
            && bytes.len() >= 32 * 4
        {
            let mut gpr = [0u32; 32];
            for (i, gpr_slot) in gpr.iter_mut().enumerate() {
                let off = i * 4;
                *gpr_slot = u32::from_le_bytes([
                    bytes[off],
                    bytes[off + 1],
                    bytes[off + 2],
                    bytes[off + 3],
                ]);
            }
            out.gpr = Some(gpr);
        }
        out
    }
}

fn read_u32_le(bytes: &[u8]) -> Option<u32> {
    if bytes.len() < 4 {
        return None;
    }
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::{MDFN_HEADER_LEN, MDFN_MAGIC, SECTION_NAME_LEN};

    fn build_main_section_save(entries: &[(&str, Vec<u8>)]) -> Vec<u8> {
        let mut body = Vec::new();
        for (name, value) in entries {
            body.push(name.len() as u8);
            body.extend_from_slice(name.as_bytes());
            body.extend_from_slice(&(value.len() as u32).to_le_bytes());
            body.extend_from_slice(value);
        }
        let mut name_buf = [0u8; SECTION_NAME_LEN];
        name_buf[..4].copy_from_slice(b"MAIN");
        let mut payload = Vec::new();
        payload.extend_from_slice(MDFN_MAGIC);
        payload.extend_from_slice(&[0u8; MDFN_HEADER_LEN - MDFN_MAGIC.len()]);
        payload.extend_from_slice(&name_buf);
        payload.extend_from_slice(&(body.len() as u32).to_le_bytes());
        payload.extend_from_slice(&body);
        payload
    }

    #[test]
    fn reads_cpu_pc_when_present() {
        let payload = build_main_section_save(&[("CPU.PC", 0x80010000u32.to_le_bytes().to_vec())]);
        let save = SaveState::from_decompressed(payload).unwrap();
        let main = PsxMain::new(&save);
        let regs = main.cpu_regs();
        assert_eq!(regs.pc, Some(0x80010000));
        assert_eq!(regs.gpr, None);
    }

    #[test]
    fn reads_full_gpr_array() {
        let mut buf = Vec::with_capacity(32 * 4);
        for i in 0..32u32 {
            buf.extend_from_slice(&(i * 0x1000).to_le_bytes());
        }
        let payload = build_main_section_save(&[("CPU.GPR", buf)]);
        let save = SaveState::from_decompressed(payload).unwrap();
        let regs = PsxMain::new(&save).cpu_regs();
        assert!(regs.gpr.is_some());
        let gpr = regs.gpr.unwrap();
        assert_eq!(gpr[0], 0);
        assert_eq!(gpr[31], 31 * 0x1000);
    }

    #[test]
    fn missing_entries_default_to_none() {
        let payload = build_main_section_save(&[("X", vec![1, 2, 3, 4])]);
        let save = SaveState::from_decompressed(payload).unwrap();
        let regs = PsxMain::new(&save).cpu_regs();
        assert_eq!(regs.pc, None);
        assert_eq!(regs.hi, None);
        assert_eq!(regs.lo, None);
        assert_eq!(regs.gpr, None);
    }
}
