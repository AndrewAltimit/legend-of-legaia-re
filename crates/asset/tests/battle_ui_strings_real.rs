//! Disc-gated invariants for the battle command / prompt / banner labels.
//!
//! The point of the test is that the *shape* of the retail battle-open flow is
//! readable off the disc without an emulator: two chips before the round, four
//! on the ring, two on the attack-mode prompt, and a banner that only appears
//! when the formation roll gave one side the drop. Each assertion is about a
//! structural property, not about the English words - the words are Sony's and
//! stay on the user's image.
//!
//! What it pins:
//!
//! * every pinned SCUS address resolves to a short, printable, ASCII label;
//! * every pinned overlay-`0898` address resolves inside the same pool the
//!   translation module already covers (`0x801F4B98..0x801F4D2A`), except the
//!   two banner sentences, which carry the `0xC1` name token;
//!   the round prompt's two labels are distinct, and so are the ring's;
//! * the Ra-Seru run really is five 10-byte slots, ending on the one-character
//!   `-` placeholder that a character with no Ra-Seru magic draws;
//! * each label the placement table points at on the disc is the SCUS address
//!   this module pins - the join that says which chip carries which word.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` / `extracted/` are absent.

use std::path::PathBuf;

use legaia_asset::battle_ui_strings::{
    BattleUiLabel, BattleUiStrings, OVERLAY_BASE_VA, OVL_AMBUSHED, OVL_ESCAPE, OVL_SOLO_SURPRISED,
    OVL_SPIRIT, OVL_TEAM_SURPRISED, RASERU_LABEL_MAX, SCUS_ATTACK, SCUS_AUTO, SCUS_BEGIN,
    SCUS_COMMAND, SCUS_ITEM, SCUS_LABELS, SCUS_RUN,
};
use legaia_asset::screen_elements::ScreenElementTable;

fn extracted(name: &str) -> Option<Vec<u8>> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for dir in ["extracted", "../../extracted"] {
        let f = PathBuf::from(dir).join(name);
        if f.is_file() {
            return std::fs::read(f).ok();
        }
    }
    None
}

fn overlay_0898() -> Option<Vec<u8>> {
    extracted("PROT/0898_xxx_dat.BIN")
}

#[test]
fn every_pinned_battle_label_resolves_on_the_disc() {
    let (Some(scus), Some(ovl)) = (extracted("SCUS_942.54"), overlay_0898()) else {
        eprintln!("skip: no LEGAIA_DISC_BIN / extracted/");
        return;
    };
    let s = BattleUiStrings::from_images(&scus, &ovl, OVERLAY_BASE_VA);
    // Non-vacuity: both halves resolved, not just one.
    assert_eq!(
        s.len(),
        SCUS_LABELS.len() + 7,
        "every pinned label should have resolved"
    );

    // The chip labels are short printable ASCII words - a mis-pinned address
    // lands in code or in the middle of another string and fails this.
    for label in [
        BattleUiLabel::Begin,
        BattleUiLabel::Run,
        BattleUiLabel::Item,
        BattleUiLabel::Attack,
        BattleUiLabel::Spirit,
        BattleUiLabel::Auto,
        BattleUiLabel::Command,
        BattleUiLabel::Reselect,
    ] {
        let word = s.get(label).unwrap_or_else(|| panic!("{label:?} resolved"));
        assert!(
            (2..=10).contains(&word.len()),
            "{label:?} = {word:?} is not a chip-sized word"
        );
        assert!(
            word.chars().all(|c| c.is_ascii_graphic()),
            "{label:?} = {word:?} is not printable ASCII"
        );
    }

    // The two chips of a prompt never carry the same word.
    assert_ne!(s.get(BattleUiLabel::Begin), s.get(BattleUiLabel::Run));
    assert_ne!(s.get(BattleUiLabel::Auto), s.get(BattleUiLabel::Command));
    // ... and neither do the ring's four arms (the magic arm is per-character
    // and checked below).
    let ring = [
        s.get(BattleUiLabel::Item),
        s.get(BattleUiLabel::Attack),
        s.get(BattleUiLabel::Spirit),
    ];
    let mut uniq = ring.to_vec();
    uniq.sort();
    uniq.dedup();
    assert_eq!(uniq.len(), ring.len(), "the ring arms share a word");
}

#[test]
fn the_formation_banner_lines_carry_the_name_token() {
    let Some(ovl) = overlay_0898() else {
        eprintln!("skip: no LEGAIA_DISC_BIN / extracted/");
        return;
    };
    let mut s = BattleUiStrings::default();
    s.merge_overlay(&ovl, OVERLAY_BASE_VA);

    // The back-attack banner is a plain sentence - no substitution.
    let ambush = s.get(BattleUiLabel::Ambushed).expect("ambush banner");
    assert!(ambush.ends_with('!'), "{ambush:?} is not an exclamation");
    assert!(ambush.chars().all(|c| c.is_ascii_graphic()));

    // Both pre-emptive lines are built by `FUN_8003CBF8` substituting the
    // `0xC1` token, so both must carry one and the team line must be longer.
    let team = s.get(BattleUiLabel::TeamSurprised).expect("team banner");
    let solo = s.get(BattleUiLabel::SoloSurprised).expect("solo banner");
    for line in [team, solo] {
        assert!(
            line.contains('\u{c1}'),
            "{line:?} carries no 0xC1 name token"
        );
        assert!(line.ends_with('.'), "{line:?} is not a sentence");
    }
    assert!(
        team.len() > solo.len(),
        "the team line should be the longer of the pair"
    );
}

#[test]
fn the_raseru_command_labels_are_five_slots_ending_on_a_dash() {
    let Some(ovl) = overlay_0898() else {
        eprintln!("skip: no LEGAIA_DISC_BIN / extracted/");
        return;
    };
    let mut s = BattleUiStrings::default();
    s.merge_overlay(&ovl, OVERLAY_BASE_VA);

    // Slot 0 is the empty one (a character with no entry draws nothing);
    // 1..=3 are the three player Ra-Seru; 4 is the `-` placeholder.
    assert_eq!(s.raseru_label(0), Some(""));
    for n in 1..RASERU_LABEL_MAX {
        let word = s.raseru_label(n).unwrap_or_else(|| panic!("slot {n}"));
        assert!(
            (3..=9).contains(&word.len()) && word.chars().all(|c| c.is_ascii_alphabetic()),
            "Ra-Seru slot {n} = {word:?} is not a name"
        );
    }
    assert_eq!(
        s.raseru_label(RASERU_LABEL_MAX),
        Some("-"),
        "the unavailable-magic slot is the one-glyph dash"
    );

    // The three names are distinct - one per player Ra-Seru.
    let mut names: Vec<&str> = (1..RASERU_LABEL_MAX)
        .filter_map(|n| s.raseru_label(n))
        .collect();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), (RASERU_LABEL_MAX - 1) as usize);
}

/// The join that makes the pins mean something: the screen-element records the
/// battle chrome draws point at exactly these SCUS addresses. Record indices
/// are `screen_elements`' own (`8..=11` = the ring's up/left/right/down arms).
#[test]
fn the_placement_records_point_at_the_pinned_scus_labels() {
    let Some(scus) = extracted("SCUS_942.54") else {
        eprintln!("skip: no LEGAIA_DISC_BIN / extracted/");
        return;
    };
    let table = ScreenElementTable::from_scus(&scus).expect("placement table decodes");
    let ptr = |i: usize| table.records().get(i).map(|r| r.payload);

    // Command ring: up = Item, left = Attack. The right (magic) and down
    // (Spirit) arms are null on the disc - the overlay writes them at runtime,
    // which is exactly why their words are in the overlay pool and not here.
    assert_eq!(ptr(8), Some(SCUS_ITEM));
    assert_eq!(ptr(9), Some(SCUS_ATTACK));
    assert_eq!(ptr(10), Some(0));
    assert_eq!(ptr(11), Some(0));

    // Round prompt: the live seat of each slide triple carries the label.
    assert_eq!(ptr(1), Some(SCUS_BEGIN));
    assert_eq!(ptr(4), Some(SCUS_RUN));

    // Attack-mode prompt: the pair that re-uses the ring's left / right seats.
    assert_eq!(ptr(85), Some(SCUS_AUTO));
    assert_eq!(ptr(84), Some(SCUS_COMMAND));

    // ... and the overlay-written chips really are inside the pool the
    // translation module pins, so a language pack reaches them.
    for va in [OVL_SPIRIT, OVL_AMBUSHED, OVL_ESCAPE] {
        assert!((0x801F_4B98..0x801F_4D2A).contains(&va));
    }
    for va in [OVL_TEAM_SURPRISED, OVL_SOLO_SURPRISED] {
        assert!((0x801F_4B98..0x801F_4D2A).contains(&va));
    }
}
