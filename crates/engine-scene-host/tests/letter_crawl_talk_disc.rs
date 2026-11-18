//! Disc-gated: the read-a-letter talks roll their letter and lift the dim.
//!
//! Three talk records read a letter: Maya's unfinished letter in Biron
//! (`bylon` `P1[47]`), Drake III's letter in the treasure chest (`dolk`
//! `P1[27]`) and Bon Saryu's letter on the desk in Ratayu (`retock`
//! `P1[33]`). Each asks "Do you wish to read it?" and on Yes dims the screen
//! (`34 01 r g b ..`), rolls the letter as a crawl on the talker's own
//! context (`4C 80 N` and `N` pages, the roller `FUN_80037174`), waits for
//! the roller on the talker's halt bit (`33 0A`), lifts the dim
//! (`34 01 00 00 00 ..`) and closes with one more line.
//!
//! The talk runner stepped the plain `4C 80` as an ordinary op: the pages
//! went to the actor pool as child records, no roller rolled, and the talk
//! ended on the halt-bit test with the dim left up until the next battle
//! (issue #634).
//!
//! REF: FUN_801DE840 (0x801E1ECC..0x801E1FBC), FUN_80037174
//!
//! Skip-passes without disc data (CLAUDE.md convention).

use legaia_engine_scene_host::scene_host::SceneHost;
use legaia_engine_system::input::PadButton;

const ANCHOR_FLAGS: &str = include_str!("fixtures/spine_anchor_flags.txt");

fn open_host() -> Option<SceneHost> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = std::path::PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return SceneHost::open_extracted(&d).ok();
        }
    }
    let disc = std::env::var_os("LEGAIA_DISC_BIN")?;
    SceneHost::open_disc(std::path::PathBuf::from(disc)).ok()
}

/// The flag ids of a spine milestone (`fixtures/spine_anchor_flags.txt`).
fn milestone(name: &str) -> Vec<u16> {
    ANCHOR_FLAGS
        .lines()
        .find_map(|line| {
            let mut it = line.split_whitespace();
            (it.next() == Some(name)).then(|| {
                it.nth(1)
                    .unwrap_or("")
                    .split(',')
                    .filter_map(|f| f.parse().ok())
                    .collect()
            })
        })
        .unwrap_or_default()
}

#[derive(Debug, Default)]
struct Reading {
    /// Pages the roller was opened with.
    pages: usize,
    /// Ticks the roller ran.
    crawl_ticks: u32,
    /// The dim was up while the roller ran.
    dimmed_under_crawl: bool,
    /// A box opened after the roller retired (the closing line).
    closing_line: bool,
    /// The talk ended inside the budget.
    ended: bool,
    /// Screen-tint pushes still live once the talk ended.
    tint_after: usize,
}

/// Talk to placement `slot` in `scene` under `flags`, answer Yes, and read
/// the letter to its end, paging with Cross.
fn read_letter(scene: &str, flags: &[u16], slot: u8) -> Option<Reading> {
    let mut host = open_host()?;
    for &f in flags {
        host.world.system_flag_set(f);
    }
    host.world.toggles.use_vm_dialogue = true;
    host.enter_field_scene(scene, 0).expect("enter scene");
    for _ in 0..60 {
        host.tick().expect("tick");
    }
    let (lx, lz) = *host
        .world
        .npcs
        .positions
        .get(&slot)
        .unwrap_or_else(|| panic!("{scene} P1[{slot}] is placed"));
    let (tx, tz) = (i32::from(lx) >> 7, i32::from(lz) >> 7);
    let stand = (-1..=1)
        .flat_map(|dz| (-1..=1).map(move |dx| (dx, dz)))
        .filter(|&d| d != (0, 0))
        .map(|(dx, dz)| (((tx + dx) * 128 + 64) as i16, ((tz + dz) * 128 + 64) as i16))
        .find(|&(x, z)| {
            host.world.field_tile_is_walk_visible(x, z) && !host.world.field_tile_is_wall(x, z)
        })
        .unwrap_or_else(|| panic!("{scene} P1[{slot}] has a standable neighbour"));
    assert!(host.debug_seat_standing(stand.0, stand.1));
    host.world.trigger_field_interact(0, slot);
    let mut r = Reading::default();
    let mut crawled = false;
    let mut boxed = false;
    for f in 0..6000u32 {
        let pad = if f % 10 == 0 {
            PadButton::Cross.mask()
        } else {
            0
        };
        host.world.set_pad(pad);
        host.tick().expect("tick");
        if let Some(n) = host.world.cutscene.narration.as_ref() {
            r.pages = r.pages.max(n.page_count());
            r.crawl_ticks += 1;
            crawled = true;
            r.dimmed_under_crawl |= !host.world.screen_tint_pushes().is_empty();
        }
        let panel = host
            .world
            .dialog
            .inline
            .as_ref()
            .is_some_and(|d| d.panel.is_some());
        if crawled && host.world.cutscene.narration.is_none() && panel && !boxed {
            r.closing_line = true;
        }
        boxed = panel;
        if crawled && host.world.dialog.inline.is_none() {
            r.ended = true;
            // Let the lift's tween run out.
            for _ in 0..60 {
                host.world.set_pad(0);
                host.tick().expect("tick");
            }
            r.tint_after = host.world.screen_tint_pushes().len();
            break;
        }
    }
    eprintln!("[{scene} P1[{slot}]] {r:?}");
    Some(r)
}

fn assert_reads(scene: &str, flags: &[u16], slot: u8, pages: usize) {
    let Some(r) = read_letter(scene, flags, slot) else {
        return;
    };
    assert_eq!(
        r.pages, pages,
        "the roller rolls the letter's {pages} pages"
    );
    assert!(
        r.crawl_ticks > 60 * pages as u32 / 4,
        "the talk waits for the roller on 33 0A ({} ticks)",
        r.crawl_ticks
    );
    assert!(r.dimmed_under_crawl, "the letter rolls over the dim");
    assert!(r.closing_line, "the closing line follows the letter");
    assert!(r.ended, "the talk ends and lets the player go");
    assert_eq!(r.tint_after, 0, "the dim is lifted with the talk");
}

/// Maya's unfinished letter (issue #634).
#[test]
fn maya_s_letter_rolls_and_the_dim_lifts() {
    let mut flags = milestone("west_voz_forest");
    flags.extend(0x297..=0x2A6);
    assert_reads("bylon", &flags, 47, 0x1C);
}

/// Drake III's letter in the treasure chest.
#[test]
fn drake_iii_s_letter_rolls_and_the_dim_lifts() {
    assert_reads("dolk", &milestone("drake_castle"), 27, 0x21);
}

/// Bon Saryu's letter on the desk.
#[test]
fn bon_saryu_s_letter_rolls_and_the_dim_lifts() {
    assert_reads("retock", &milestone("ratayu"), 33, 0x0C);
}
