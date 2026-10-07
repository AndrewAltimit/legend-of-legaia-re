//! Disc-gated: the Muscle Dome's **course ladder** and its **hub-screen draw
//! lists** both come off a real disc and both agree with the bytes they claim
//! to have been read from.
//!
//! Three chains are asserted, and none asserts any Sony byte:
//!
//! 1. PROT 0977's course descriptor table decodes as three courses whose
//!    round counts match the populated-cell counts of its own score table,
//!    and every round's monster id resolves to a populated PROT 867 record.
//!    That is the opponent, pinned - not a stand-in.
//! 2. Every recovered hub-screen draw row names the VA of the `jal` it was
//!    read from, so the row is checkable: re-read that word out of the entry
//!    and assert it still encodes a `jal` to the emitter the row implies.
//!    A row whose call site drifted stops being documentation and starts
//!    being a test failure.
//! 3. A contest played to a win on the minigames page composes the page's
//!    victory banner, the one host path into `FUN_801DBA90`'s port.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` is unset.

#![cfg(not(target_arch = "wasm32"))]

use legaia_engine_core::muscle_dome as md;
use legaia_engine_ui::other_game_hud as hud;

/// The three retail emitters a draw row can name.
const EMITTER_CENTRED: u32 = 0x801D_050C;
const EMITTER_CORNER: u32 = 0x801D_08EC;
const EMITTER_DECIMAL: u32 = 0x801D_1308;
/// The ROUND banner's digit wrapper, which tails into the centred emitter.
const EMITTER_ROUND_DIGIT: u32 = 0x801D_15C8;

fn prot_entry(index: u32) -> Option<Vec<u8>> {
    let disc = std::env::var_os("LEGAIA_DISC_BIN")?;
    let host = match legaia_engine_core::scene::SceneHost::open_disc(&disc) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("[skip] open_disc failed: {e:#}");
            return None;
        }
    };
    host.index.entry_bytes_extended(index).ok()
}

fn arena_entry() -> Option<Vec<u8>> {
    prot_entry(md::ARENA_OVERLAY_PROT_INDEX as u32)
}

fn monster_archive() -> Option<Vec<u8>> {
    prot_entry(867)
}

/// Decode the `jal` at `va` in the entry, returning its absolute target.
///
/// A `jal` is opcode 3 in bits 31..26 and a word-address in the low 26, so
/// the target is a property of the bytes and independent of the load base
/// (see `docs/tooling/call-target-integrity.md`).
fn jal_target(entry: &[u8], va: u32) -> Option<u32> {
    let off = va.checked_sub(md::ARENA_OVERLAY_BASE_VA)? as usize;
    let w = u32::from_le_bytes(entry.get(off..off + 4)?.try_into().ok()?);
    if w >> 26 != 3 {
        return None;
    }
    Some((va & 0xF000_0000) | ((w & 0x03FF_FFFF) << 2))
}

#[test]
fn the_course_ladder_pins_a_real_monster_per_round() {
    let Some(entry) = arena_entry() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let ladder = md::parse_course_ladder(&entry).expect("PROT 0977 course descriptor decodes");
    assert_eq!(ladder.len(), md::COURSE_COUNT);
    let counts: Vec<usize> = ladder.iter().map(|c| c.rounds.len()).collect();

    // The score table is an independent table with the same (course, round)
    // shape; its populated cells must agree with the descriptor's counts.
    for (course, &n) in counts.iter().enumerate() {
        for round in 1..=n as u32 {
            let cell = md::course_score_cell(&entry, course, round)
                .unwrap_or_else(|| panic!("course {course} round {round} has a score cell"));
            assert!(cell > 0, "course {course} round {round} scores nothing");
        }
        // The cell one past the last round is the table's zero padding.
        if (n as u32) < md::MAX_ROUNDS_PER_COURSE as u32 {
            assert_eq!(
                md::course_score_cell(&entry, course, n as u32 + 1),
                Some(0),
                "course {course} has exactly {n} scored rounds"
            );
        }
    }

    // Every round's id is a real monster.
    let Some(archive) = monster_archive() else {
        return;
    };
    let mut ids = Vec::new();
    for course in &ladder {
        for round in &course.rounds {
            let rec = legaia_asset::monster_archive::record(&archive, round.monster_id as u16)
                .unwrap_or_else(|e| panic!("monster {:#04x}: {e:#}", round.monster_id));
            let rec = rec.unwrap_or_else(|| {
                panic!("monster {:#04x} has no archive record", round.monster_id)
            });
            assert!(
                rec.hp > 0 && !rec.name.is_empty(),
                "monster {:#04x} decodes to a populated record",
                round.monster_id
            );
            ids.push(round.monster_id);
        }
    }
    assert_eq!(ids.len(), counts.iter().sum::<usize>());

    // The timed-fight gate the port used to call "the dome battle type" is a
    // monster id, and no dome round is that monster - which is the whole
    // reason the four-turn strip is not this minigame's.
    assert!(
        !ids.contains(&md::TIMED_FIGHT_MONSTER_ID),
        "no arena round fields monster {:#04x}",
        md::TIMED_FIGHT_MONSTER_ID
    );
}

#[test]
fn every_hub_draw_row_still_names_its_call_site() {
    let Some(entry) = arena_entry() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let screens: Vec<(&str, Vec<hud::HubDraw>)> = vec![
        ("intro", hud::HUB_INTRO_CARD.to_vec()),
        ("title", hud::HUB_TITLE_ART.to_vec()),
        ("interval", hud::HUB_INTERVAL_HEADING.to_vec()),
        ("round", hud::round_banner_draws(12)),
        ("tally", hud::HUB_SCORE_TALLY_LABELS.to_vec()),
        // `FUN_801D042C`'s six calls, and the backdrop's six wall tiles.
        ("course", hud::course_card_draws(1)),
        (
            "wall",
            legaia_engine_ui::ringside_backdrop::first_visit_tile_draws(),
        ),
    ];
    for (name, draws) in &screens {
        for d in draws {
            let target = jal_target(&entry, d.call_site)
                .unwrap_or_else(|| panic!("{name}: {:#010x} is not a jal", d.call_site));
            let want = match d.anchor {
                hud::HubAnchor::Corner => EMITTER_CORNER,
                hud::HubAnchor::RoundDigit(_) => EMITTER_ROUND_DIGIT,
                hud::HubAnchor::Centre => EMITTER_CENTRED,
            };
            assert_eq!(
                target, want,
                "{name}: the jal at {:#010x} targets {target:#010x}, not the \
                 emitter the row names",
                d.call_site
            );
        }
    }
    // The decimal readout's own call sites belong to the tally's values.
    for site in [0x801C_F560u32, 0x801C_F5A4, 0x801C_F834] {
        assert_eq!(
            jal_target(&entry, site),
            Some(EMITTER_DECIMAL),
            "{site:#010x} is a decimal-readout call"
        );
    }
}

#[test]
fn a_hub_screen_lands_inside_the_retail_frame() {
    let Some(entry) = arena_entry() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let mut table = hud::parse_sprite_table(&entry);
    assert_eq!(table.len(), hud::SPRITE_TABLE_LEN);
    for (name, draws) in [
        ("intro", hud::HUB_INTRO_CARD.to_vec()),
        ("interval", hud::HUB_INTERVAL_HEADING.to_vec()),
        ("round", hud::round_banner_draws(7)),
    ] {
        let quads = hud::hub_screen_quads(&mut table, &draws, 0x100);
        assert_eq!(quads.len(), draws.len(), "{name}: one quad per draw");
        for q in &quads {
            let (x0, y0) = q.xy[0];
            let (x1, y1) = q.xy[3];
            assert!(x1 > x0 && y1 > y0, "{name}: quad has positive extent");
            assert!(
                (0..320).contains(&x0) && (0..320).contains(&x1),
                "{name}: quad spans {x0}..{x1} inside the 320-wide frame"
            );
            assert!(
                (0..240).contains(&y0) && (0..240).contains(&y1),
                "{name}: quad spans {y0}..{y1} inside the 240-tall frame"
            );
        }
    }
    // The score tally's six values render through the decimal emitter.
    let tally = hud::score_tally_quads(&mut table, [1234, 0, 0, 0, 0, 999], [0x100; 6]);
    assert!(
        tally.len() > hud::HUB_SCORE_TALLY_LABELS.len(),
        "the tally draws its labels and its digits"
    );
}

/// A contest played to a win on the browser minigames page, then the page's
/// own victory-banner read - the call `site/js/minigame-muscle.js` makes when
/// `state.phase === 'won'`.
///
/// The banner's index half is `MuscleDomeSession::reward_banner`, which
/// decodes through `FUN_801DBA90`'s port (`battle_cast_dispatch::reward_banner`).
/// Retail reaches that standalone composer from nowhere - its in-battle twin is
/// `FUN_801D8DE8` case `0x59` - so no retail situation drives it; this page
/// does, on every won contest, and this is the rung that drives the page
/// there. Each turn commits every card the budget allows, which is the page's
/// own commit/close/resolve sequence, and a level-99 Vahn against the
/// archive's first populated monster settles it within the turn budget.
#[test]
fn a_won_contest_composes_the_page_reward_banner() {
    use legaia_web_viewer::minigames::LegaiaMinigames;
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let Ok(bytes) = std::fs::read(&disc) else {
        eprintln!("[skip] disc unreadable");
        return;
    };
    let mut mg = LegaiaMinigames::new();
    mg.load_disc(bytes).expect("load_disc");
    let json = |s: String| -> serde_json::Value { serde_json::from_str(&s).expect("json") };
    assert_eq!(
        json(mg.muscle_reward_banner_json())["ok"],
        false,
        "no banner before a contest"
    );
    let roster = json(mg.muscle_roster_json());
    let monster = roster[0]["id"].as_u64().expect("roster non-empty") as u16;
    assert!(mg.muscle_start_vs(0, 99, monster, 0x2A), "contest starts");

    let mut phase = String::new();
    for _ in 0..64 {
        for slot in 0..4 {
            mg.muscle_commit(slot);
        }
        mg.muscle_end_selection();
        mg.muscle_resolve();
        phase = json(mg.muscle_state_json())["phase"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        if phase == "won" || phase == "lost" {
            break;
        }
        mg.muscle_next_turn();
    }
    assert_eq!(phase, "won", "a level-99 lead settles the contest");

    let banner = json(mg.muscle_reward_banner_json());
    assert_eq!(
        banner["ok"], true,
        "a live contest composes a banner: {banner}"
    );
    let spell = banner["spell"].as_str().unwrap_or_default();
    let text = banner["text"].as_str().unwrap_or_default();
    assert!(
        !spell.is_empty(),
        "the reward spell names off the SCUS table"
    );
    assert!(
        text.contains(spell),
        "the banner text carries the reward spell: {banner}"
    );
    eprintln!(
        "[ok] reward banner composed ({} chars)",
        text.chars().count()
    );
}
