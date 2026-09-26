//! The **Seru-absorb caption** the battle HUD's screen element `0x59` shows:
//! the two string pieces it is composed from, resident in the battle-action
//! overlay (PROT 0898).
//!
//! `FUN_801D8DE8(0x59, 0)` - the HUD element spawner's `0x59` arm, raised by
//! the action SM's Done band right after `FUN_801E92DC` prepends the absorbed
//! Seru's spell to the acting character's list - builds the line into the
//! battle context's message buffer `ctx + 0x1F9` in three steps
//! (`overlay_battle_action_801d8de8.txt`, `0x801D9154..0x801D91D0`):
//!
//! ```text
//! 801d9164  lbu   v1,0x13(a0)        ; ctx[+0x13], the acting seat
//! 801d9170  lbu   v0,0x0(v1)         ; 0x8007BD10[seat] - its character id
//! 801d9178  addiu v0,v0,-0x1         ; id - 1
//! 801d9184  lw    a1,0x0(v0)         ; PREFIX_TABLE_VA[id - 1]
//! 801d9188  jal   0x8003ca78         ; strcpy(ctx + 0x1F9, prefix)
//! 801d9198  lbu   v1,0x269(a0)       ; ctx[+0x269], the absorbed Seru
//! 801d91a0  addiu v1,v1,0x80         ; spell id = seru + 0x80
//! 801d91b4  lw    a1,0x8(v0)         ; 0x800754C8[id*12 + 8] = spell name
//! 801d91b8  jal   0x8003cac4         ; strcat(buf, name)
//! 801d91c8  addiu a1,a1,0x4c28       ; SUFFIX_VA
//! 801d91cc  jal   0x8003cac4         ; strcat(buf, suffix)
//! ```
//!
//! The prefix table is indexed by **character** (Vahn / Noa / Gala), and each
//! prefix names that character's Ra-Seru rather than the character - the
//! Ra-Seru is the one said to take the Seru's power. Only the arm's `mode == 0`
//! (raise) composes; the unload call (`mode == 1`) reuses the buffer.
//!
//! The same pointer table is the Muscle Dome's "victory message" in older
//! notes; it is the shared cast-caption table, reached by any cast
//! (`docs/subsystems/minigame-muscle-dome.md`).
//!
//! No bytes of the strings are carried here: [`parse`] reads them off the
//! caller's PROT 0898 image.

/// CDNAME / PROT index of the battle-action overlay.
pub const BATTLE_ACTION_OVERLAY_PROT_INDEX: usize = 898;

/// The battle-action overlay's link/load base (`VA - file_offset`).
pub const OVERLAY_LINK_BASE: u32 = 0x801C_E818;

/// Runtime VA of the per-character prefix pointer table (`lui a1,0x801f;
/// addiu a1,a1,0x4dfc` at `0x801D9158` / `0x801D9174`).
pub const PREFIX_TABLE_VA: u32 = 0x801F_4DFC;

/// Runtime VA of the suffix string (`addiu a1,a1,0x4c28` at `0x801D91C8`).
pub const SUFFIX_VA: u32 = 0x801F_4C28;

/// Prefix rows the table carries - one per playable character id `1..=3`.
/// The fourth word is zero on the disc, so a character id past Gala reads a
/// null prefix in retail.
pub const PREFIX_ROWS: usize = 3;

/// Longest string [`parse`] accepts before calling the read a mis-base.
const MAX_STRING_LEN: usize = 64;

/// The decoded caption pieces.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AbsorbCaption {
    /// Prefix per character id `1..=3` (index = id - 1).
    pub prefixes: Vec<String>,
    /// The suffix appended after the Seru's spell name.
    pub suffix: String,
}

impl AbsorbCaption {
    /// Compose the line retail builds for character `char_id` (`1..=3`)
    /// absorbing the Seru whose spell name is `seru_name`. `None` for a
    /// character id with no prefix row.
    pub fn compose(&self, char_id: u8, seru_name: &str) -> Option<String> {
        let prefix = self.prefixes.get(usize::from(char_id).checked_sub(1)?)?;
        Some(format!("{prefix}{seru_name}{}", self.suffix))
    }
}

fn va_to_off(va: u32, len: usize) -> Option<usize> {
    let off = va.checked_sub(OVERLAY_LINK_BASE)? as usize;
    (off < len).then_some(off)
}

fn c_string(image: &[u8], va: u32) -> Option<String> {
    let off = va_to_off(va, image.len())?;
    let tail = &image[off..];
    let end = tail.iter().take(MAX_STRING_LEN).position(|&b| b == 0)?;
    let bytes = &tail[..end];
    // Printable ASCII only: a mis-based read lands in code or table words.
    if !bytes.iter().all(|&b| (0x20..0x7F).contains(&b)) {
        return None;
    }
    Some(String::from_utf8_lossy(bytes).into_owned())
}

/// Read the caption pieces out of a PROT 0898 image. `None` when a pointer
/// leaves the image or a string is not a short printable run - the shape of
/// a wrong entry or a wrong base.
pub fn parse(overlay_0898: &[u8]) -> Option<AbsorbCaption> {
    let table = va_to_off(PREFIX_TABLE_VA, overlay_0898.len())?;
    let mut prefixes = Vec::with_capacity(PREFIX_ROWS);
    for row in 0..PREFIX_ROWS {
        let at = table + row * 4;
        let word = overlay_0898.get(at..at + 4)?;
        let ptr = u32::from_le_bytes(word.try_into().ok()?);
        let s = c_string(overlay_0898, ptr)?;
        if s.is_empty() {
            return None;
        }
        prefixes.push(s);
    }
    let suffix = c_string(overlay_0898, SUFFIX_VA)?;
    Some(AbsorbCaption { prefixes, suffix })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic image with the three pointers and four strings planted at
    /// their VAs. Placeholder text only.
    fn synthetic() -> Vec<u8> {
        let mut img = vec![0u8; (PREFIX_TABLE_VA - OVERLAY_LINK_BASE) as usize + 0x40];
        let put = |img: &mut Vec<u8>, va: u32, s: &[u8]| {
            let o = (va - OVERLAY_LINK_BASE) as usize;
            img[o..o + s.len()].copy_from_slice(s);
            img[o + s.len()] = 0;
        };
        let strs = [0x801F_4BD0u32, 0x801F_4BEC, 0x801F_4C0C];
        for (i, va) in strs.iter().enumerate() {
            put(&mut img, *va, format!("P{i} ").as_bytes());
            let o = (PREFIX_TABLE_VA - OVERLAY_LINK_BASE) as usize + i * 4;
            img[o..o + 4].copy_from_slice(&va.to_le_bytes());
        }
        put(&mut img, SUFFIX_VA, b"!");
        img
    }

    #[test]
    fn parses_three_prefixes_and_the_suffix() {
        let cap = parse(&synthetic()).expect("parses");
        assert_eq!(cap.prefixes, vec!["P0 ", "P1 ", "P2 "]);
        assert_eq!(cap.suffix, "!");
        assert_eq!(cap.compose(2, "X").as_deref(), Some("P1 X!"));
        assert_eq!(cap.compose(0, "X"), None);
        assert_eq!(cap.compose(4, "X"), None);
    }

    #[test]
    fn a_pointer_outside_the_image_refuses() {
        let mut img = synthetic();
        let o = (PREFIX_TABLE_VA - OVERLAY_LINK_BASE) as usize;
        img[o..o + 4].copy_from_slice(&0x8000_0000u32.to_le_bytes());
        assert!(parse(&img).is_none());
        assert!(parse(&[0u8; 16]).is_none());
    }
}
