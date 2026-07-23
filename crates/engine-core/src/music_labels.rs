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
//! The join is structural, but the bank is **not** a single linear PROT run.
//! The debug sound-test order (the order the curated table is keyed on) maps
//! onto two contiguous extraction ranges with a 2-entry gap between them:
//!
//! - sound-test indices `0..=67`  -> extraction PROT `988 + index` (`988..=1055`);
//! - a 2-entry gap - extraction `1056` (a VAB-only sound bank, no score) and
//!   `1057` (an entry not in the sound-test list);
//! - sound-test indices `68..=80` -> extraction PROT `990 + index` (`1058..=1070`).
//!
//! The low range starts at extraction **988**, not 990: the first two tracks
//! (`M01`/`M02`) live in the entries whose `prot-extract` filename label reads
//! `monster_test`, because the `music_01` CDNAME `#define` number is a *raw*
//! TOC index and the named content sits two entries earlier (the +2 filename
//! skew, see `docs/formats/cdname.md`). Pinned by the on-disc SEQ residency of
//! every slot: the four pochi-filled dev slots (extraction `1066..=1069`) land
//! exactly on the table's dev-leftover rows (#76..=79 - the M13 flute, `M117`,
//! `MPIANO`, `LEVELUP`); the ten `sound_data2` battle-bank SEQ copies resolve
//! to the ten battle/boss themes; the per-scene op-`0x35` census matches each
//! id to its scene's known music; and the minigame BGM constants land on the
//! Sol disco set (the dance loads #60/#66, extraction `1048`/`1054`).
//!
//! Every SEQ stream on the disc lives in this bank + the `sound_data2`
//! boot/battle banks (+ the dev `monster_test` / `teien` copies) - scenes
//! carry **no local SEQ data**, so the scene-local id space (`< 2000`) is
//! the rare exception, not the rule.

use std::sync::OnceLock;

pub use legaia_gamedata::MusicTrack;

/// Extraction-space PROT base for sound-test indices `0..=67` (`988 + index`).
pub const MUSIC_BANK_LOW_BASE: u32 = 988;
/// Extraction-space PROT base for sound-test indices `68..=80` (`990 + index`)
/// - the high range, past the 2-entry gap at extraction `1056`/`1057`.
pub const MUSIC_BANK_HIGH_BASE: u32 = 990;
/// First sound-test index in the high range (the +2 gap starts here).
pub const MUSIC_BANK_SPLIT_INDEX: u32 = 68;
/// Number of curated sound-test rows - indices `0..=80`.
pub const MUSIC_TRACK_COUNT: u32 = 81;

/// The extraction-space PROT entry that holds a sound-test index's
/// `[VAB][SEQ]` pair, honoring the 2-entry gap at index 68. `None` past the
/// last row. This is the inverse of [`sound_test_index_for_prot_entry`] and
/// the single source of truth for "which PROT entry plays sound-test track N".
pub fn prot_entry_for_sound_test_index(index: u32) -> Option<u32> {
    match index {
        0..=67 => Some(MUSIC_BANK_LOW_BASE + index),
        68..=80 => Some(MUSIC_BANK_HIGH_BASE + index),
        _ => None,
    }
}

/// The extraction-space PROT entry that plays a **global-pool** BGM id
/// (`>= 2000`): `2000 + sound-test index`, resolved through the piecewise
/// bank map. `None` for scene-local ids or ids past the last row.
pub fn prot_entry_for_bgm_id(bgm_id: u16) -> Option<u32> {
    prot_entry_for_sound_test_index(sound_test_index_for_bgm_id(bgm_id)?)
}

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
    (idx < MUSIC_TRACK_COUNT).then_some(idx)
}

/// Map an extraction-space PROT entry index to its sound-test index, when
/// the entry is a `music_01` bank slot. Honors the 2-entry gap - extraction
/// `1056`/`1057` are not sound-test tracks and resolve to `None`.
pub fn sound_test_index_for_prot_entry(entry: u32) -> Option<u32> {
    match entry {
        988..=1055 => Some(entry - MUSIC_BANK_LOW_BASE),
        1058..=1070 => Some(entry - MUSIC_BANK_HIGH_BASE),
        _ => None,
    }
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
    /// 0..=80 exactly once, and there is no row 81.
    #[test]
    fn table_covers_every_sound_test_row() {
        for i in 0..MUSIC_TRACK_COUNT {
            assert!(
                track_for_sound_test_index(i).is_some(),
                "sound-test index {i} missing from the music table"
            );
        }
        assert!(
            track_for_sound_test_index(MUSIC_TRACK_COUNT).is_none(),
            "there is no sound-test row past #80"
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
        assert_eq!(label_for_bgm_id(2081), None);
    }

    /// The bank is piecewise: indices 0..=67 sit at `988+i`, a 2-entry gap at
    /// extraction 1056/1057, then indices 68..=80 at `990+i`. The forward and
    /// inverse maps must round-trip and exclude the gap.
    #[test]
    fn prot_entry_join_is_piecewise() {
        // Low range: index 0 is extraction 988 (the first "monster_test"-
        // labelled entry), the title #65 is extraction 1053, #67 is 1055.
        assert_eq!(prot_entry_for_sound_test_index(0), Some(988));
        assert_eq!(sound_test_index_for_prot_entry(988), Some(0));
        assert_eq!(prot_entry_for_sound_test_index(65), Some(1053)); // title
        assert_eq!(prot_entry_for_sound_test_index(67), Some(1055));
        assert_eq!(sound_test_index_for_prot_entry(1055), Some(67));
        // The 2-entry gap is not a sound-test track.
        assert_eq!(sound_test_index_for_prot_entry(1056), None);
        assert_eq!(sound_test_index_for_prot_entry(1057), None);
        // High range resumes at index 68 = extraction 1058 (Credits).
        assert_eq!(prot_entry_for_sound_test_index(68), Some(1058));
        assert_eq!(sound_test_index_for_prot_entry(1058), Some(68));
        assert_eq!(prot_entry_for_sound_test_index(80), Some(1070));
        // Out of range.
        assert_eq!(prot_entry_for_sound_test_index(81), None);
        assert_eq!(sound_test_index_for_prot_entry(987), None);
        assert_eq!(sound_test_index_for_prot_entry(1071), None);
        // Round-trip every row.
        for i in 0..MUSIC_TRACK_COUNT {
            let p = prot_entry_for_sound_test_index(i).unwrap();
            assert_eq!(sound_test_index_for_prot_entry(p), Some(i), "row {i}");
        }
        // The title label resolves at its true entry now.
        let l = label_for_prot_entry(1053).expect("title theme label");
        assert!(l.starts_with("M65"), "got {l}");
    }
}
