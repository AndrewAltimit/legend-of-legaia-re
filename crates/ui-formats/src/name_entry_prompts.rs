//! The new-game name-entry screen's prompt lines, read off the field overlay
//! (PROT 0897) through the pointer table its code indexes.
//!
//! The prompts are not reached by `lui` / `addiu` pairs: the field overlay
//! carries a seven-word pointer table at [`PROMPT_TABLE_VA`]
//! (`0x801F3324..0x801F3340`), and each prompt's only reference is its word
//! there (the translation pipeline's reference scan of the retail image).
//! Reading through the table is what makes the reader follow a prompt a
//! language pack moved: the import rewrites the word.
//!
//! Slots, in table order: `Select your name.`, `Tell me my name.`, the
//! Biron monk's question, `Cannot enter that name.`, `Is this name okay?`,
//! and the confirm's `Yes` / `No`. Coordinates only; the text is read from
//! the image the user supplied.

/// Load base of the field overlay (`crates/asset/data/static-overlays.toml`).
pub const FIELD_OVERLAY_BASE_VA: u32 = 0x801C_E818;
/// PROT entry the field overlay is extracted from.
pub const FIELD_OVERLAY_PROT_INDEX: usize = 897;
/// First word of the prompt pointer table.
pub const PROMPT_TABLE_VA: u32 = 0x801F_3324;

/// Table slot of each prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum NamePrompt {
    /// The editing state's line.
    Select = 0,
    /// The default-name variant's line.
    TellMe = 1,
    /// The Biron monk's question.
    BironMonk = 2,
    /// A refused name.
    CannotEnter = 3,
    /// The confirm state's question.
    Confirm = 4,
    /// The confirm's first choice.
    Yes = 5,
    /// The confirm's second choice.
    No = 6,
}

/// Every prompt the table carries, read off one field overlay image.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NameEntryPrompts {
    lines: [Option<String>; 7],
}

impl NameEntryPrompts {
    /// Read the table out of a PROT 0897 image loaded at `base_va` (normally
    /// [`FIELD_OVERLAY_BASE_VA`]). A word that points outside the image, or
    /// at an empty string, leaves that slot `None`.
    pub fn from_field_overlay(overlay: &[u8], base_va: u32) -> Self {
        let mut out = Self::default();
        for (slot, line) in out.lines.iter_mut().enumerate() {
            let at = |va: u32| va.checked_sub(base_va).map(|o| o as usize);
            let Some(word_off) = at(PROMPT_TABLE_VA + slot as u32 * 4) else {
                continue;
            };
            let Some(w) = overlay.get(word_off..word_off + 4) else {
                continue;
            };
            let va = u32::from_le_bytes(w.try_into().unwrap());
            let Some(tail) = at(va).and_then(|o| overlay.get(o..)) else {
                continue;
            };
            let end = tail.iter().position(|&b| b == 0).unwrap_or(tail.len());
            if end > 0 {
                // Latin-1: a high glyph byte comes back as the char of its
                // value, the way the font layout takes it.
                *line = Some(tail[..end].iter().map(|&b| char::from(b)).collect());
            }
        }
        out
    }

    /// One prompt, if the image carried it.
    pub fn get(&self, p: NamePrompt) -> Option<&str> {
        self.lines[p as usize].as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompts_read_through_the_table_words() {
        let base = FIELD_OVERLAY_BASE_VA;
        let mut img = vec![0u8; 0x25000];
        let put = |img: &mut Vec<u8>, va: u32, b: &[u8]| {
            let o = (va - base) as usize;
            img[o..o + b.len()].copy_from_slice(b);
        };
        // The confirm question moved past the pool; its word follows it.
        put(&mut img, 0x801F_0000, b"Ce nom te va\x82 ?\0");
        put(
            &mut img,
            PROMPT_TABLE_VA + 4 * 4,
            &0x801F_0000u32.to_le_bytes(),
        );
        put(&mut img, 0x801C_F10C, b"Oui\0");
        put(
            &mut img,
            PROMPT_TABLE_VA + 5 * 4,
            &0x801C_F10Cu32.to_le_bytes(),
        );
        // A word outside the image.
        put(
            &mut img,
            PROMPT_TABLE_VA + 6 * 4,
            &0x8020_0000u32.to_le_bytes(),
        );
        let p = NameEntryPrompts::from_field_overlay(&img, base);
        assert_eq!(p.get(NamePrompt::Confirm), Some("Ce nom te va\u{82} ?"));
        assert_eq!(p.get(NamePrompt::Yes), Some("Oui"));
        assert_eq!(p.get(NamePrompt::No), None);
        assert_eq!(p.get(NamePrompt::Select), None);
        assert_eq!(
            NameEntryPrompts::from_field_overlay(&[], base),
            NameEntryPrompts::default()
        );
    }
}
