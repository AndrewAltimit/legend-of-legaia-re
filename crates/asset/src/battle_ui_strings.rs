//! The battle screen's **command / prompt / banner labels** - pinned disc
//! coordinates, read off the user's own image.
//!
//! The battle command UI is not one string pool. It is two, in two different
//! images, and which one a label lives in is a property of *who writes it into
//! the [screen-element record](crate::screen_elements)*:
//!
//! * The labels the placement table points at **on the disc** are static
//!   `SCUS_942.54` rodata - `Attack` / `Item` / `Begin` / `Run` / `Auto` /
//!   `Command` / `Reselect`. Their record's `+0x14` payload pointer already
//!   carries the VA before the battle overlay is even loaded.
//! * The labels the battle overlay **writes at runtime** live in overlay
//!   `0898`'s own pool at `0x801F4B98..0x801F4D2A` - `Spirit`, the per-Ra-Seru
//!   magic-command name, `Escape`, and the formation banner lines. The overlay
//!   stores the pointer into the record's `+0x14` as it raises the element.
//!
//! Nothing here is the text: every entry is a coordinate, and the strings are
//! read from the caller's image - the same rule the translation string pools
//! and the battle-tutorial script follow. Provenance for each address is the
//! `lui`+`addiu` pair that loads it, or the placement record that points at it.
//!
//! ## The magic command's label is the character's Ra-Seru
//!
//! The command ring's right arm is not labelled `Magic`. `0x801D8F30` reads the
//! acting slot's character id out of `DAT_8007BD10 + ctx[+0x13]` and indexes a
//! **10-byte-stride** table at [`RASERU_LABEL_TABLE_VA`], so the word on the
//! chip is `Meta` (Vahn), `Terra` (Noa) or `Ozma` (Gala) - and index `4` is the
//! single `-` a character with no Ra-Seru magic draws instead (the same
//! "unavailable keeps its plate" law the rest of the chips follow). The
//! `ctx[+0x25F + slot]` gate above it picks the dash directly.
//!
//! ## Where each label is consumed
//!
//! | Flow state | Cluster | Chips |
//! |---|---|---|
//! | `ctx[+0x06] == 0x1E` | round prompt | [`SCUS_BEGIN`] / [`SCUS_RUN`] |
//! | `ctx[+0x06] == 0x28` | command ring | [`SCUS_ITEM`] / [`SCUS_ATTACK`] / Ra-Seru / [`OVL_SPIRIT`] |
//! | `ctx[+0x06] == 0x78` | attack mode | [`SCUS_AUTO`] / [`SCUS_COMMAND`] |
//! | `ctx[+0x06] == 0x6E` | commit confirm | runtime `Begin`/`Escape` / [`SCUS_RESELECT`] |
//!
//! See [`battle.md`](../../../docs/subsystems/battle.md) for the state machine
//! these sit on.

use crate::screen_elements::ExeMap;

// --------------------------------------------------------------- SCUS labels

/// `Auto` - the attack-mode prompt's left chip (auto-target the swing).
pub const SCUS_AUTO: u32 = 0x8007_B658;
/// `Command` - the attack-mode prompt's right chip (open the arts entry).
pub const SCUS_COMMAND: u32 = 0x8007_B660;
/// `Attack` - the command ring's left arm.
pub const SCUS_ATTACK: u32 = 0x8007_B674;
/// `Item` - the command ring's up arm.
pub const SCUS_ITEM: u32 = 0x8007_B67C;
/// `Run` - the round prompt's right chip.
pub const SCUS_RUN: u32 = 0x8007_B684;
/// `Begin` - the round prompt's left chip.
pub const SCUS_BEGIN: u32 = 0x8007_B688;
/// `Reselect` - the commit-confirm menu's right chip.
pub const SCUS_RESELECT: u32 = 0x8001_52D4;

/// Every SCUS-resident battle label, in address order.
pub const SCUS_LABELS: [(u32, BattleUiLabel); 7] = [
    (SCUS_RESELECT, BattleUiLabel::Reselect),
    (SCUS_AUTO, BattleUiLabel::Auto),
    (SCUS_COMMAND, BattleUiLabel::Command),
    (SCUS_ATTACK, BattleUiLabel::Attack),
    (SCUS_ITEM, BattleUiLabel::Item),
    (SCUS_RUN, BattleUiLabel::Run),
    (SCUS_BEGIN, BattleUiLabel::Begin),
];

// ------------------------------------------------------------ overlay labels

/// PROT entry the battle overlay is extracted from.
pub const OVERLAY_PROT_INDEX: usize = 898;
/// Load base of the battle overlay (`crates/asset/data/static-overlays.toml`).
pub const OVERLAY_BASE_VA: u32 = 0x801C_E818;

/// `Spirit` - the command ring's down arm. Written by `0x801D8F98`.
pub const OVL_SPIRIT: u32 = 0x801F_4B98;
/// `Defense` - the guard result line.
pub const OVL_DEFENSE: u32 = 0x801F_4BA0;
/// `Ambushed!` - the back-attack banner. Written by `0x801DA260`.
pub const OVL_AMBUSHED: u32 = 0x801F_4D10;
/// `Begin` - stamped into the commit-confirm chip by `0x801D1060`.
pub const OVL_BEGIN: u32 = 0x801F_4D1C;
/// `Escape` - stamped into the same chip by `0x801D10D8` when the player runs.
pub const OVL_ESCAPE: u32 = 0x801F_4D24;
/// `<name>'s team surprised the enemy.` - the multi-member pre-emptive banner.
pub const OVL_TEAM_SURPRISED: u32 = 0x801F_4CD8;
/// `<name> surprised the enemy.` - the solo pre-emptive banner.
pub const OVL_SOLO_SURPRISED: u32 = 0x801F_4CF8;

/// Base of the per-Ra-Seru magic-command label table (`0x801D8F0C`).
pub const RASERU_LABEL_TABLE_VA: u32 = 0x801F_4B9E;
/// Bytes between two Ra-Seru labels (`id*5` then `<< 1`).
pub const RASERU_LABEL_STRIDE: u32 = 10;
/// Highest Ra-Seru index the table carries; `4` is the `-` placeholder a
/// character with no Ra-Seru magic draws.
pub const RASERU_LABEL_MAX: u8 = 4;

/// Every overlay-resident battle label, in address order.
pub const OVERLAY_LABELS: [(u32, BattleUiLabel); 7] = [
    (OVL_SPIRIT, BattleUiLabel::Spirit),
    (OVL_DEFENSE, BattleUiLabel::Defense),
    (OVL_TEAM_SURPRISED, BattleUiLabel::TeamSurprised),
    (OVL_SOLO_SURPRISED, BattleUiLabel::SoloSurprised),
    (OVL_AMBUSHED, BattleUiLabel::Ambushed),
    (OVL_BEGIN, BattleUiLabel::CommitBegin),
    (OVL_ESCAPE, BattleUiLabel::CommitEscape),
];

/// One label the battle screen can put on a chip or a banner.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum BattleUiLabel {
    /// Round prompt, left chip.
    Begin,
    /// Round prompt, right chip.
    Run,
    /// Command ring, up arm.
    Item,
    /// Command ring, left arm.
    Attack,
    /// Command ring, down arm.
    Spirit,
    /// Attack-mode prompt, left chip.
    Auto,
    /// Attack-mode prompt, right chip.
    Command,
    /// Commit-confirm menu, right chip.
    Reselect,
    /// Commit-confirm chip after the player chose to fight.
    CommitBegin,
    /// Commit-confirm chip after the player chose to flee.
    CommitEscape,
    /// The guard result line.
    Defense,
    /// Back-attack banner.
    Ambushed,
    /// Pre-emptive banner, party of two or more.
    TeamSurprised,
    /// Pre-emptive banner, solo party.
    SoloSurprised,
}

/// The battle UI labels read off one image pair.
///
/// Absent entries mean the address was not resolvable in the bytes handed in
/// (a truncated image, a different build) - callers fall back rather than fail,
/// exactly as the tutorial script does.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BattleUiStrings {
    labels: std::collections::BTreeMap<BattleUiLabel, String>,
    raseru: Vec<String>,
}

impl BattleUiStrings {
    /// Read the SCUS-resident half out of a `SCUS_942.54` image.
    pub fn from_scus(scus: &[u8]) -> Self {
        let mut out = Self::default();
        out.merge_scus(scus);
        out
    }

    /// Add the SCUS-resident half in place.
    pub fn merge_scus(&mut self, scus: &[u8]) {
        let Some(map) = ExeMap::parse(scus) else {
            return;
        };
        for (va, label) in SCUS_LABELS {
            if let Some(off) = map.off(va)
                && let Some(s) = cstr_at(scus, off)
            {
                self.labels.insert(label, s);
            }
        }
    }

    /// Add the overlay-resident half in place. `bytes` is PROT entry
    /// [`OVERLAY_PROT_INDEX`] as extracted; `base_va` is normally
    /// [`OVERLAY_BASE_VA`].
    pub fn merge_overlay(&mut self, bytes: &[u8], base_va: u32) {
        for (va, label) in OVERLAY_LABELS {
            if let Some(s) = va
                .checked_sub(base_va)
                .and_then(|o| cstr_at(bytes, o as usize))
            {
                self.labels.insert(label, s);
            }
        }
        // The Ra-Seru command labels are one 10-byte-stride run, not separate
        // pointers - index 0 is the empty slot, 4 the `-` placeholder.
        self.raseru.clear();
        for n in 0..=u32::from(RASERU_LABEL_MAX) {
            let va = RASERU_LABEL_TABLE_VA + n * RASERU_LABEL_STRIDE;
            match va
                .checked_sub(base_va)
                .and_then(|o| cstr_at(bytes, o as usize))
            {
                Some(s) => self.raseru.push(s),
                None => {
                    // A short read past the pool leaves the run at whatever it
                    // resolved; nothing downstream indexes past its length.
                    self.raseru.push(String::new());
                }
            }
        }
    }

    /// Read both halves at once.
    pub fn from_images(scus: &[u8], overlay_0898: &[u8], base_va: u32) -> Self {
        let mut out = Self::from_scus(scus);
        out.merge_overlay(overlay_0898, base_va);
        out
    }

    /// One label, if this image carried it.
    pub fn get(&self, label: BattleUiLabel) -> Option<&str> {
        self.labels.get(&label).map(String::as_str)
    }

    /// The magic-command chip's word for a character whose Ra-Seru index is
    /// `raseru_id` (`0` = none, `4` = the `-` placeholder).
    pub fn raseru_label(&self, raseru_id: u8) -> Option<&str> {
        self.raseru.get(raseru_id as usize).map(String::as_str)
    }

    /// How many labels were resolved - the non-vacuity handle for callers and
    /// tests, so an empty read can't pass as a successful one.
    pub fn len(&self) -> usize {
        self.labels.len()
    }

    /// `true` when nothing resolved.
    pub fn is_empty(&self) -> bool {
        self.labels.is_empty()
    }
}

/// NUL-terminated ASCII at `off`, or `None` when the offset is past the end or
/// the string is empty. Non-ASCII bytes (the `0xC1` name token the banner lines
/// carry) survive as their Latin-1 characters so a caller can still find them.
fn cstr_at(bytes: &[u8], off: usize) -> Option<String> {
    let tail = bytes.get(off..)?;
    let end = tail.iter().position(|&b| b == 0).unwrap_or(tail.len());
    if end == 0 {
        return None;
    }
    Some(tail[..end].iter().map(|&b| b as char).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The overlay reader is offset arithmetic against the load base; a
    /// synthetic entry with the pool at its pinned VAs must round-trip.
    #[test]
    fn overlay_reader_resolves_the_pinned_pool() {
        let base = OVERLAY_BASE_VA;
        let mut bytes = vec![0u8; 0x28800];
        let put = |b: &mut Vec<u8>, va: u32, s: &str| {
            let off = (va - base) as usize;
            b[off..off + s.len()].copy_from_slice(s.as_bytes());
            b[off + s.len()] = 0;
        };
        put(&mut bytes, OVL_SPIRIT, "SPIRIT");
        put(&mut bytes, OVL_AMBUSHED, "AMBUSH");
        put(
            &mut bytes,
            RASERU_LABEL_TABLE_VA + RASERU_LABEL_STRIDE,
            "AAAA",
        );
        put(
            &mut bytes,
            RASERU_LABEL_TABLE_VA + 4 * RASERU_LABEL_STRIDE,
            "-",
        );
        let mut s = BattleUiStrings::default();
        s.merge_overlay(&bytes, base);
        assert_eq!(s.get(BattleUiLabel::Spirit), Some("SPIRIT"));
        assert_eq!(s.get(BattleUiLabel::Ambushed), Some("AMBUSH"));
        assert_eq!(s.raseru_label(1), Some("AAAA"));
        assert_eq!(s.raseru_label(4), Some("-"));
        // Index 0 is the empty slot: no label, not a missing read.
        assert_eq!(s.raseru_label(0), Some(""));
        assert!(!s.is_empty());
    }

    /// A truncated image degrades to fewer labels instead of panicking.
    #[test]
    fn a_short_image_yields_nothing_rather_than_panicking() {
        let mut s = BattleUiStrings::default();
        s.merge_overlay(&[0u8; 16], OVERLAY_BASE_VA);
        assert!(s.is_empty());
        s.merge_scus(&[0u8; 16]);
        assert!(s.is_empty());
    }

    /// The Ra-Seru run really is a 10-byte stride over five slots - the
    /// arithmetic `0x801D8F0C` performs (`id*5 << 1`).
    #[test]
    fn raseru_table_is_five_slots_at_stride_ten() {
        let last = RASERU_LABEL_TABLE_VA + u32::from(RASERU_LABEL_MAX) * RASERU_LABEL_STRIDE;
        assert_eq!(RASERU_LABEL_TABLE_VA, 0x801F_4B9E);
        assert_eq!(last, 0x801F_4BC6);
        // The run sits between `Spirit` and the pre-emptive banner lines.
        const _: () = assert!(OVL_SPIRIT < RASERU_LABEL_TABLE_VA);
        const _: () = assert!(
            RASERU_LABEL_TABLE_VA + (RASERU_LABEL_MAX as u32) * RASERU_LABEL_STRIDE
                < OVL_TEAM_SURPRISED
        );
        assert_eq!(last, RASERU_LABEL_TABLE_VA + 4 * RASERU_LABEL_STRIDE);
    }
}
