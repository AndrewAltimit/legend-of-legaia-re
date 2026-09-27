//! Disc-gated census behind the commit's clip-tag ladder
//! (`FUN_8004AD80` `0x8004BE30..0x8004BF4C`, engine
//! `world::battle::clip_ladder`).
//!
//! 1. **Only the tag-2 rewrite's own monster lacks a knockdown.** One monster
//!    in PROT 0867 carries no tag-4 entry - `0xB3`, one of the two first
//!    monsters the ladder rewrites a flinch into a knockdown for. Without the
//!    rewrite its `+0x1F1` fallback (the flinch entry) would commit tag 2 and
//!    its death arm, keyed on the previous entry's tag 4, would never run.
//! 2. **Entries 7 / 8 are the downed chain.** In every player file that
//!    carries them, entry 7 opens on the knockdown's final root height (it
//!    continues the fall) and both carry a zero root speed - entry 8 is not a
//!    recover backstep.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset (disc-gated convention).

use legaia_patcher::disc::{DiscPatcher, MONSTER_ARCHIVE_ENTRY};

fn archive() -> Option<Vec<u8>> {
    let path = std::env::var_os("LEGAIA_DISC_BIN")?;
    let disc = std::fs::read(path).ok()?;
    DiscPatcher::open(disc)
        .ok()?
        .read_entry(MONSTER_ARCHIVE_ENTRY)
        .ok()
}

#[test]
fn only_monster_0xb3_lacks_a_knockdown_entry() {
    let Some(archive) = archive() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let n = legaia_asset::monster_archive::slot_count(&archive) as u16;
    let mut decoded = 0;
    let mut without = Vec::new();
    for id in 1..=n {
        let Ok(Some(clips)) = legaia_asset::monster_archive::animations_by_entry(&archive, id)
        else {
            continue;
        };
        decoded += 1;
        if !clips.iter().flatten().any(|c| c.action_id == 4) {
            without.push(id);
        }
    }
    eprintln!("[ok] {decoded} monsters decoded; without a tag-4 entry: {without:02x?}");
    assert!(decoded > 100, "the archive decodes");
    assert_eq!(without, vec![0xB3]);
}

#[test]
fn party_entries_seven_and_eight_continue_the_knockdown() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let dir = ["extracted/PROT", "../../extracted/PROT"]
        .iter()
        .map(std::path::PathBuf::from)
        .find(|p| p.is_dir());
    let Some(dir) = dir else {
        eprintln!("[skip] extracted/PROT missing");
        return;
    };
    let mut checked = 0;
    for file in [
        "0863_edstati3.BIN",
        "0864_edstati3.BIN",
        "0865_battle_data.BIN",
        "0866_battle_data.BIN",
    ] {
        let raw = std::fs::read(dir.join(file)).expect("read");
        let clips = legaia_asset::battle_char_assembly::battle_animations(&raw).expect("clips");
        let by = |slot: u8| clips.iter().find(|c| c.action_id == slot);
        let root_y = |c: &legaia_asset::monster_archive::MonsterAnimation, last: bool| {
            let f = if last {
                c.frames.last()
            } else {
                c.frames.first()
            };
            f.and_then(|f| f.first()).map(|p| i32::from(p.ty))
        };
        let (Some(knock), Some(seven), Some(eight)) = (by(4), by(7), by(8)) else {
            continue;
        };
        let fall_end = root_y(knock, true).expect("knockdown frames");
        let seven_start = root_y(seven, false).expect("entry 7 frames");
        assert!(
            (fall_end - seven_start).abs() <= 4,
            "{file}: entry 7 opens at {seven_start}, the knockdown ends at {fall_end}"
        );
        assert_eq!(seven.entry_root_speed(), Some(0), "{file} entry 7");
        assert_eq!(eight.entry_root_speed(), Some(0), "{file} entry 8");
        checked += 1;
    }
    eprintln!("[ok] {checked} party files carry the downed chain");
    assert_eq!(checked, 3, "Vahn, Noa and Gala carry entries 7 / 8");
}
