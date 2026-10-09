//! Who owns which spare bytes: one ledger translation and the mods share.
//!
//! Every feature that writes new bytes into a code image needs room that
//! the retail game never reads, and the disc has little of it. The code
//! hooks (`--shiny-seru`, `--show-super-arts`, `--seru-trade`, ...) each
//! write into a fixed, verified-dead region and refuse to write unless that
//! region is still all-zero on the disc being patched; a language pack's
//! relocated `ui_menu` / `system_text` strings need room too.
//!
//! [`REGIONS`] lists every such region once, with its owner:
//!
//! - [`Owner::Mods`] - one or more code hooks write here (the names are the
//!   `randomize` flags). Translation never places a string in these, even
//!   when they are zero on the disc at hand, so a mod applied **after** a
//!   language pack still finds its region untouched.
//! - [`Owner::Translation`] - reserved for relocated strings. No mod
//!   writes here.
//!
//! [`translation_spans`] is the allocator's input: the translation-owned
//! spans of one image, trimmed to the bytes that are still zero on this
//! disc - so anything already written there (by an earlier import, or by a
//! feature this ledger does not know) is never overwritten. The two rules
//! together are why a mod and a translation cannot overlap in either order:
//! the mod checks for zeros before it writes, and translation writes only
//! where no mod ever will.
//!
//! Each region's evidence is recorded with it (`why`); the table is the
//! committed half of `docs/tooling/translation/space-and-budgets.md`'s
//! memory map.

/// A code image a region lives in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
pub enum Image {
    /// `SCUS_942.54`, always resident.
    Scus,
    /// A PROT overlay entry (extraction index).
    Prot(usize),
}

/// Who may write a region.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Owner {
    /// Code hooks; the `randomize` flags that write here.
    Mods(&'static [&'static str]),
    /// Relocated translation strings (`ui_menu` / `system_text`).
    Translation,
}

/// One spare region.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct Region {
    /// The image.
    pub image: Image,
    /// First VA (inclusive).
    pub start_va: u32,
    /// One past the last VA.
    pub end_va: u32,
    /// Who writes it.
    pub owner: Owner,
    /// What the region is and why it is dead.
    pub why: &'static str,
}

impl Region {
    /// Size in bytes.
    pub fn len(&self) -> usize {
        (self.end_va - self.start_va) as usize
    }

    /// `true` for an empty region.
    pub fn is_empty(&self) -> bool {
        self.start_va == self.end_va
    }
}

/// Menu overlay (PROT 0899), the pause menu / shop / save screen image.
pub const MENU_OVERLAY: usize = 899;

/// The ledger. Mod regions carry the flags that claim them; see
/// `docs/tooling/randomizer.md` ("The injected-code arena budget") and each
/// module's constants.
pub const REGIONS: &[Region] = &[
    Region {
        image: Image::Scus,
        start_va: 0x8007_7728,
        end_va: 0x8007_7828,
        owner: Owner::Mods(&["--shiny-seru"]),
        why: "SCUS_GAP: verified-dead rodata between live tables",
    },
    Region {
        image: Image::Scus,
        start_va: 0x8007_8A88,
        end_va: 0x8007_8ACC,
        owner: Owner::Mods(&["--shiny-seru", "--show-super-arts"]),
        why: "SLOT6: a dead table slot",
    },
    Region {
        image: Image::Scus,
        start_va: 0x8007_AB38,
        end_va: 0x8007_AF40,
        owner: Owner::Mods(&[
            "--equipment-drops",
            "--enemy-ally",
            "--flee-exp",
            "--seru-trade",
            "--delilas-challenge",
            "--shiny-seru",
            "--show-super-arts",
        ]),
        why: "the rodata gap (ARENA1 at 0x8007AE00 inside it); ends where the SsAPI I/O table begins",
    },
    Region {
        image: Image::Scus,
        start_va: 0x8007_AFF8,
        end_va: 0x8007_B040,
        owner: Owner::Mods(&["--shiny-seru"]),
        why: "ARENA2",
    },
    Region {
        image: Image::Prot(MENU_OVERLAY),
        start_va: 0x801E_A440,
        end_va: 0x801E_B340,
        owner: Owner::Mods(&["--seru-trade", "--show-super-arts"]),
        why: "run-C: save-menu atlas rows 162..191, blank and never sampled; above both card \
              buffers, no instruction in any image forms an address inside it, zero in every \
              library capture with the overlay resident",
    },
    Region {
        image: Image::Prot(MENU_OVERLAY),
        start_va: 0x801E_B400,
        end_va: 0x801E_B94F,
        owner: Owner::Mods(&["--show-super-arts"]),
        why: "the Moves-page descriptions: atlas rows 193..203, the same blank band as run-C",
    },
    Region {
        image: Image::Prot(MENU_OVERLAY),
        start_va: 0x801E_D340,
        end_va: 0x801E_E120,
        owner: Owner::Translation,
        why: "zero fill between the save-menu atlas and the save-slot icon sheet: no instruction in \
              any image forms an address inside it, neither card buffer reaches it, and it reads \
              zero in every library capture with the overlay resident",
    },
];

/// A span the running game itself writes, or reads as live data, while the
/// image is resident - so no [`Region`] may overlap it, whatever the file holds
/// there. Zero bytes in the file say nothing about these: a card buffer is
/// all-zero on the disc and full of save data a moment after the player saves.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct Buffer {
    /// The image.
    pub image: Image,
    /// First VA (inclusive).
    pub start_va: u32,
    /// One past the last VA.
    pub end_va: u32,
    /// What uses it, and the evidence.
    pub what: &'static str,
}

/// Runtime buffers and live data inside the images the ledger hands out room
/// in. See `docs/subsystems/save-screen.md` ("Which buffer the sum runs over")
/// and `docs/tooling/translation/space-and-budgets.md`.
pub const BUFFERS: &[Buffer] = &[
    Buffer {
        image: Image::Prot(MENU_OVERLAY),
        start_va: 0x801E_5120,
        end_va: 0x801E_7120,
        what: "save screen card-read buffer: FUN_801DD35C installs it (0x2000 bytes) and \
               the card read fills it - at the title's Continue, and from the pause menu's \
               Load row, which returns to the root menu with the overlay resident",
    },
    Buffer {
        image: Image::Prot(MENU_OVERLAY),
        start_va: 0x801E_7120,
        end_va: 0x801E_9120,
        what: "save compose buffer: FUN_801E1934 memsets its 0x2000 bytes and copies the \
               live game state over them on every save; FUN_801DAEF4 then returns a \
               pause-menu Save to the root menu with the overlay still resident",
    },
    Buffer {
        image: Image::Prot(MENU_OVERLAY),
        start_va: 0x801E_5120,
        end_va: 0x801E_A440,
        what: "save-menu atlas TIM header, CLUT and pixel rows 0..161: FUN_801DD35C uploads \
               it to VRAM (960, 0) at card-screen entry and the card screen samples rows up \
               to 160",
    },
    Buffer {
        image: Image::Prot(MENU_OVERLAY),
        start_va: 0x801E_E120,
        end_va: 0x801E_EB40,
        what: "save-slot icon sheet TIM, uploaded to VRAM (960, 224) right after the atlas",
    },
    Buffer {
        image: Image::Prot(MENU_OVERLAY),
        start_va: 0x801E_F070,
        end_va: 0x801F_3818,
        what: "the overlay's uninitialised data (menu state, card handles, staging), \
               written at runtime in every capture with the overlay resident",
    },
];

/// The first [`BUFFERS`] row `[start_va, end_va)` in `image` overlaps.
pub fn overlapping_buffer(image: Image, start_va: u32, end_va: u32) -> Option<&'static Buffer> {
    BUFFERS
        .iter()
        .find(|b| b.image == image && start_va < b.end_va && b.start_va < end_va)
}

/// The translation-owned spans of `image`, as `(start_va, end_va)`,
/// trimmed to the bytes `bytes` (the image loaded at `base`) still holds as
/// zero. A span starts four bytes past any non-zero byte, so a string
/// placed there never runs on from someone else's text, and starts and ends
/// 4-aligned.
pub fn translation_spans(image: Image, bytes: &[u8], base: u32) -> Vec<(u32, u32)> {
    let spans: Vec<(u32, u32)> = REGIONS
        .iter()
        .filter(|r| r.image == image && r.owner == Owner::Translation)
        .map(|r| (r.start_va, r.end_va))
        .collect();
    zero_spans(bytes, base, &spans)
}

/// The parts of `spans` (`(start_va, end_va)`) that `bytes` (loaded at
/// `base`) holds as zero, 4-aligned, each starting four bytes past any
/// non-zero byte - the room a string may be placed in without touching
/// anything already written.
pub fn zero_spans(bytes: &[u8], base: u32, spans: &[(u32, u32)]) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    for &(sv, ev) in spans {
        let (Some(s), Some(e)) = (
            sv.checked_sub(base).map(|d| d as usize),
            ev.checked_sub(base).map(|d| d as usize),
        ) else {
            continue;
        };
        let e = e.min(bytes.len());
        let mut i = s;
        while i < e {
            if bytes[i] != 0 {
                i += 1;
                continue;
            }
            let mut j = i;
            while j < e && bytes[j] == 0 {
                j += 1;
            }
            // Leave a zero after a preceding non-zero byte (its terminator
            // may be the one this run starts with).
            let start = if i > 0 && bytes[i - 1] != 0 {
                (i + 4) & !3
            } else {
                (i + 3) & !3
            };
            let end = j & !3;
            if start < end {
                out.push((base + start as u32, base + end as u32));
            }
            i = j;
        }
    }
    out
}

/// Every region of `image`.
pub fn regions_of(image: Image) -> impl Iterator<Item = &'static Region> {
    REGIONS.iter().filter(move |r| r.image == image)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regions_do_not_overlap() {
        for (i, a) in REGIONS.iter().enumerate() {
            for b in &REGIONS[i + 1..] {
                assert!(
                    a.image != b.image || a.end_va <= b.start_va || b.end_va <= a.start_va,
                    "{a:?} overlaps {b:?}"
                );
            }
        }
    }

    #[test]
    fn no_region_overlaps_a_runtime_buffer() {
        for r in REGIONS {
            let hit = overlapping_buffer(r.image, r.start_va, r.end_va);
            assert!(hit.is_none(), "{r:?} overlaps {hit:?}");
        }
    }

    #[test]
    fn overlap_check_sees_the_old_menu_layout() {
        // The two spans `--seru-trade` / `--show-super-arts` once used, inside
        // the card buffers; the translation region is clear of every buffer.
        let menu = Image::Prot(MENU_OVERLAY);
        assert!(overlapping_buffer(menu, 0x801E_74E0, 0x801E_83E0).is_some());
        assert!(overlapping_buffer(menu, 0x801E_65F4, 0x801E_6B43).is_some());
        assert!(overlapping_buffer(menu, 0x801E_D340, 0x801E_E120).is_none());
    }

    #[test]
    fn spans_skip_written_bytes() {
        let base = 0x801C_E818;
        let r = REGIONS
            .iter()
            .find(|r| r.owner == Owner::Translation)
            .unwrap();
        let mut img = vec![0u8; (r.end_va - base) as usize + 16];
        let whole = translation_spans(r.image, &img, base);
        assert_eq!(whole, vec![(r.start_va, r.end_va)]);
        // Something already written 0x10 bytes in: the span resumes past it.
        let at = (r.start_va - base) as usize + 0x10;
        img[at..at + 3].copy_from_slice(b"abc");
        let split = translation_spans(r.image, &img, base);
        assert_eq!(split[0], (r.start_va, r.start_va + 0x10));
        assert_eq!(split[1].0, r.start_va + 0x14);
    }
}
