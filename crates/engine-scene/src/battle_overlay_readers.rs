//! The battle overlay's disc readers: the battle screen's labels and the
//! cast trigger's anim-pair lists, read off the user's `PROT.DAT` through a
//! [`crate::scene::ProtIndex`].
//!
//! The formation banner, its hold timers and wording live in
//! `legaia_engine_battle_menus::battle_open`; these readers name no kernel, so they
//! sit with the `ProtIndex` they read through.

use legaia_ui_formats::battle_ui_strings::{BattleUiStrings, OVERLAY_BASE_VA};

/// Read the battle screen's overlay-resident labels - the banner sentences,
/// `Spirit`, `Escape` and the per-character Ra-Seru magic-command names - off
/// the user's own `PROT.DAT`.
///
/// The SCUS-resident half (`Begin` / `Run` / `Attack` / `Item` / `Auto` /
/// `Command`) is not read here: those words are already the port's own chip
/// labels, so the only thing a disc read would add is a second copy. Hosts
/// that want them can call
/// [`legaia_ui_formats::battle_ui_strings::BattleUiStrings::merge_scus`] on top.
///
/// An entry that will not resolve yields an empty table rather than an error,
/// so a host with a partial extraction falls back to the port's wording.
pub fn battle_ui_strings_from_prot(index: &crate::scene::ProtIndex) -> BattleUiStrings {
    let mut out = BattleUiStrings::default();
    let Some(rec) = legaia_overlay_images::static_overlay::overlay_map().by_label("battle_action")
    else {
        return out;
    };
    let Ok(bytes) = index.entry_bytes(rec.prot_index) else {
        return out;
    };
    match legaia_overlay_images::static_overlay::as_loaded(&bytes, rec) {
        Ok(loaded) => out.merge_overlay(&loaded, rec.base_va),
        Err(_) => out.merge_overlay(&bytes, OVERLAY_BASE_VA),
    }
    out
}

/// Both halves of the battle labels, built the one way every host installs
/// them: the overlay half off PROT 0898 ([`battle_ui_strings_from_prot`]),
/// then the SCUS half on top
/// ([`legaia_ui_formats::battle_ui_strings::BattleUiStrings::merge_scus`]).
///
/// Retail has no merge: each label is a string the drawing routine addresses
/// in its own image, so the only order that could matter is which half wins a
/// key both carry - and the two label sets share none (`SCUS_LABELS` holds the
/// chip words, the steal captions and the sparring caption; `OVERLAY_LABELS`
/// the banner sentences, `Spirit` / `Defense` / `Escape` and the commit-screen
/// `Begin`; the Ra-Seru names are overlay-only). One builder keeps it that way
/// on every host rather than relying on it: the native window used to merge
/// the SCUS half at boot and the overlay half on top only when a player battle
/// was requested, the page the other way round at disc load.
pub fn battle_ui_strings_for_disc(
    index: &crate::scene::ProtIndex,
    scus: Option<&[u8]>,
) -> BattleUiStrings {
    let mut out = battle_ui_strings_from_prot(index);
    if let Some(scus) = scus {
        out.merge_scus(scus);
    }
    out
}

/// Read the party cast trigger's per-spell anim-pair lists
/// (`FUN_801DBF9C`'s `< 0x25` arm) off the user's own `PROT.DAT` - the same
/// battle-overlay image [`battle_ui_strings_from_prot`] reads. Empty when the
/// entry will not resolve.
pub fn spell_anim_pairs_from_prot(
    index: &crate::scene::ProtIndex,
) -> legaia_game_tables::spell_anim_pairs::SpellAnimPairs {
    use legaia_game_tables::spell_anim_pairs::SpellAnimPairs;
    let Some(rec) = legaia_overlay_images::static_overlay::overlay_map().by_label("battle_action")
    else {
        return SpellAnimPairs::default();
    };
    let Ok(bytes) = index.entry_bytes(rec.prot_index) else {
        return SpellAnimPairs::default();
    };
    match legaia_overlay_images::static_overlay::as_loaded(&bytes, rec) {
        Ok(loaded) => SpellAnimPairs::parse(&loaded, rec.base_va),
        Err(_) => SpellAnimPairs::parse(&bytes, OVERLAY_BASE_VA),
    }
}
