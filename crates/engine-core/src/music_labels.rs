//! Human-readable labels for BGM ids - the `music_01` bank / sound-test join.
//!
//! Retail resolves a field-VM op-`0x35` BGM id PROT-relatively
//! (`FUN_800243F0`, see `docs/subsystems/script-vm.md` § BGM lookup table):
//! ids `< 2000` are scene-local slots at `scene_base + 6 + id`, ids
//! `>= 2000` index the **global BGM pool** at `_DAT_8007BC64 + (id - 2000)`.
//! There is no track-name table anywhere on the disc, so labels come from
//! the curated [`legaia_gamedata`] music table (sound-test id + context +
//! OST title; see `docs/reference/music-tracks.md`).
//!
//! The join is structural: the global pool is the `music_01` CDNAME block
//! (extraction entries `990..=1071`, 82 slots), and its slot order **is the
//! debug sound-test order** the curated table is keyed on. Pinned by:
//!
//! - the bank width matches the table (81 sound-test rows + one spare slot);
//! - the four pochi-filled slots (extraction `1066..=1069`) land exactly on
//!   the table's four dev-leftover rows (#76..=79: the M13 flute, `M117`,
//!   `MPIANO`, `LEVELUP` - working titles flagged as placeholders), removed
//!   from the retail NA disc but still holding their sound-test slots;
//! - the SEQ-content fingerprints of the battle themes (#26/#28..) and the
//!   title theme (#65) match their `sound_data2` boot/battle-bank copies
//!   (extraction 879/880..882/884), the banks those cues actually play from.
//!
//! Every SEQ stream on the disc lives in this bank + the `sound_data2`
//! boot/battle banks (+ the dev `monster_test` / `teien` copies) - scenes
//! carry **no local SEQ data**, so the scene-local id space (`< 2000`) is
//! the rare exception, not the rule.

use std::sync::OnceLock;

pub use legaia_gamedata::MusicTrack;

/// First extraction-space PROT entry of the `music_01` bank (raw TOC 992).
pub const MUSIC_BANK_EXTRACTION_BASE: u32 = 990;
/// Bank width in PROT slots. Slot 81 (extraction 1071) is the spare slot
/// past the last sound-test row.
pub const MUSIC_BANK_SLOTS: u32 = 82;

fn tracks() -> &'static [MusicTrack] {
    static DB: OnceLock<Vec<MusicTrack>> = OnceLock::new();
    DB.get_or_init(|| legaia_gamedata::Database::load().music_tracks().to_vec())
}

/// Map a field-VM op-`0x35` BGM id to its debug sound-test index.
///
/// Global-pool ids (`>= 2000`) index the `music_01` bank directly:
/// `2000 + i` = bank slot `i` = sound-test index `i`. Scene-local ids
/// (`< 2000`) resolve through the *scene's* PROT base, which this
/// scene-independent map cannot see - use
/// [`sound_test_index_for_prot_entry`] with the resolved entry instead.
pub fn sound_test_index_for_bgm_id(bgm_id: u16) -> Option<u32> {
    let idx = (bgm_id as u32).checked_sub(2000)?;
    (idx < MUSIC_BANK_SLOTS).then_some(idx)
}

/// Map an extraction-space PROT entry index to its sound-test index, when
/// the entry is a `music_01` bank slot.
pub fn sound_test_index_for_prot_entry(entry: u32) -> Option<u32> {
    let idx = entry.checked_sub(MUSIC_BANK_EXTRACTION_BASE)?;
    (idx < MUSIC_BANK_SLOTS).then_some(idx)
}

/// The curated music-table row for a sound-test index.
pub fn track_for_sound_test_index(index: u32) -> Option<&'static MusicTrack> {
    tracks().iter().find(|t| t.index == index)
}

/// The curated music-table row for a global-pool BGM id (`>= 2000`).
pub fn track_for_bgm_id(bgm_id: u16) -> Option<&'static MusicTrack> {
    track_for_sound_test_index(sound_test_index_for_bgm_id(bgm_id)?)
}

/// One-line display label for a track row: the debug sound-test id plus the
/// most informative human name available (in-game context, else the debug
/// title's English gloss, else the OST gloss). ASCII-composed so it renders
/// through the engine's ASCII text layout.
pub fn track_label(t: &MusicTrack) -> String {
    let name = t
        .context
        .as_deref()
        .or(t.debug_gloss.as_deref())
        .or(t.ost_gloss.as_deref())
        .unwrap_or("(unnamed)");
    match t.id.as_deref() {
        Some(id) => format!("{id} - {name}"),
        None => name.to_string(),
    }
}

/// One-line display label for a global-pool BGM id, `None` when the id is
/// scene-local or out of the table.
pub fn label_for_bgm_id(bgm_id: u16) -> Option<String> {
    track_for_bgm_id(bgm_id).map(track_label)
}

/// One-line display label for an extraction-space PROT entry inside the
/// `music_01` bank (the asset-viewer / SEQ-inspector side of the join).
pub fn label_for_prot_entry(entry: u32) -> Option<String> {
    track_for_sound_test_index(sound_test_index_for_prot_entry(entry)?).map(track_label)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Structural coverage: the curated table covers sound-test indices
    /// 0..=80 exactly once, so every bank slot except the spare resolves.
    #[test]
    fn table_covers_every_bank_slot_except_the_spare() {
        for i in 0..MUSIC_BANK_SLOTS - 1 {
            assert!(
                track_for_sound_test_index(i).is_some(),
                "sound-test index {i} missing from the music table"
            );
        }
        assert!(
            track_for_sound_test_index(MUSIC_BANK_SLOTS - 1).is_none(),
            "the spare 82nd bank slot has no sound-test row"
        );
    }

    #[test]
    fn bgm_id_join_is_the_global_pool_offset() {
        // #16 M14B is the Rim Elm theme.
        let t = track_for_bgm_id(2016).expect("global id 2016");
        assert_eq!(t.id.as_deref(), Some("M14B"));
        assert_eq!(sound_test_index_for_bgm_id(2016), Some(16));
        // Scene-local ids don't resolve here.
        assert_eq!(sound_test_index_for_bgm_id(5), None);
        assert_eq!(sound_test_index_for_bgm_id(1999), None);
        // Past the bank: no label.
        assert_eq!(label_for_bgm_id(2082), None);
    }

    #[test]
    fn prot_entry_join_covers_the_bank_range() {
        assert_eq!(sound_test_index_for_prot_entry(990), Some(0));
        assert_eq!(sound_test_index_for_prot_entry(1055), Some(65)); // title theme
        assert_eq!(sound_test_index_for_prot_entry(989), None);
        assert_eq!(sound_test_index_for_prot_entry(1072), None);
        let l = label_for_prot_entry(1055).expect("title theme label");
        assert!(l.starts_with("M65"), "got {l}");
    }
}
