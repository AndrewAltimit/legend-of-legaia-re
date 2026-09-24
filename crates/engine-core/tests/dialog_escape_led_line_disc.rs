//! A text line that OPENS on a `0xC0..=0xCF` escape is still the first line.
//!
//! `town01` `P1[16]` opens its conversation (record `+0x4C`) on the line
//! `1F C1 00 ...`: the party-name escape `C1` with argument `00` (the lead
//! character's name), then the rest of the line, then a second `0x1F` line
//! for the same box. The segment finder used to cut a line at its first
//! `0x00`, which here is the escape's *argument*: the line measured one byte,
//! failed the printable gate, and the scan resumed inside the line's text, so
//! every consumer (the field-VM runner's fallback segment, the simplified
//! panel's inline buffer) started one line late - the box typed the second
//! line alone and page-broke after it.
//!
//! This pins the fix on the exact record, through the conversation path both
//! hosts share (`World::step_inline_dialogue` under `use_vm_dialogue`): the
//! first box opens on the escape-led line, types the substituted name at the
//! head of row 0, and carries both rows. The name is a test string seeded into
//! the party roster, so no disc text is asserted.
//!
//! It also prints the disc-wide census of placements whose first line opens
//! on an escape (diagnostic only; the counts are disc data).
//!
//! Disc-gated: skip-passes when `LEGAIA_DISC_BIN` is unset or `extracted/`
//! is absent.

use std::path::PathBuf;

use legaia_engine_core::man_field_scripts::placement_inline_prologue;
use legaia_engine_core::scene::{SceneHost, is_world_map_scene};

fn extracted_dir() -> Option<PathBuf> {
    ["extracted", "../extracted", "../../extracted"]
        .iter()
        .map(PathBuf::from)
        .find(|d| d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists())
}

fn gate() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let d = extracted_dir();
    if d.is_none() {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
    }
    d
}

/// The `town01` placement whose conversation opens on `1F C1 00`.
const SLOT: u8 = 16;
/// A roster name that is not disc text.
const NAME: &str = "Zed";

#[test]
fn town01_first_box_keeps_its_name_escape_line() {
    let Some(extracted) = gate() else { return };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.world.toggles.use_vm_dialogue = true;
    host.enter_field_scene("town01", 0).expect("enter town01");
    host.world.party.party_names = vec![NAME.to_string()];

    let prologue = host
        .world
        .npcs
        .dialog_prologue
        .get(&SLOT)
        .cloned()
        .expect("town01 P1[16] is a talkable placement");
    let lead = prologue.first_segment;
    assert_eq!(
        &prologue.body[lead..lead + 3],
        &[0x1F, 0xC1, 0x00],
        "the first segment is the escape-led line, not the line after it"
    );
    let inline = host
        .world
        .npcs
        .dialog
        .get(&SLOT)
        .expect("simplified inline");
    assert_eq!(
        &inline[..3],
        &[0x1F, 0xC1, 0x00],
        "the simplified panel's inline buffer starts on the same line"
    );

    host.world.trigger_field_interact(0, SLOT);
    let mut opened = None;
    for _ in 0..240 {
        host.world.set_pad(0);
        host.tick().expect("tick");
        let Some(runner) = host.world.dialog.inline.as_ref() else {
            continue;
        };
        if let Some(panel) = runner.panel.as_ref() {
            opened = Some((panel.row_leads(), runner.waiting()));
            if runner.waiting() {
                break;
            }
        }
    }
    let (leads, waiting) = opened.expect("the conversation opened a box");
    assert!(waiting, "the first box finished typing");
    assert_eq!(
        leads.first().copied(),
        Some(lead),
        "the box's row 0 is the escape-led line"
    );
    assert!(
        leads.len() >= 2,
        "the escape-led line shares its box with the next line (rows {leads:?})"
    );
    let page = host
        .world
        .dialog
        .inline
        .as_ref()
        .map(|r| r.page_bytes())
        .unwrap_or_default();
    assert!(
        page.starts_with(NAME.as_bytes()),
        "row 0 types the substituted name first, got {:?}",
        String::from_utf8_lossy(&page[..page.len().min(NAME.len() + 4)])
    );
    eprintln!(
        "[ok] town01 P1[16] first box: {} rows, row 0 at +{lead:#x}",
        leads.len()
    );
}

/// Diagnostic census: every placement whose first line opens on an escape.
#[test]
fn census_of_escape_led_first_lines() {
    let Some(extracted) = gate() else { return };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    let cdname = legaia_prot::cdname::parse(&extracted.join("CDNAME.TXT")).expect("CDNAME");
    let mut names: Vec<String> = cdname.values().cloned().collect();
    names.sort();
    names.dedup();
    let (mut records, mut escape_led) = (0usize, 0usize);
    for name in &names {
        if is_world_map_scene(name) || host.enter_field_scene(name, 0).is_err() {
            continue;
        }
        let Some(man) = host.world.field_vm.channels_man.clone() else {
            continue;
        };
        let Ok(mf) = legaia_asset::man_section::parse(&man) else {
            continue;
        };
        for p in mf.actor_placements(&man) {
            let Some(pr) = placement_inline_prologue(&mf, &man, &p) else {
                continue;
            };
            records += 1;
            let first = pr.first_segment;
            if pr
                .body
                .get(first + 1)
                .is_some_and(|b| (0xC0..=0xCF).contains(b))
            {
                escape_led += 1;
                eprintln!("[escape-led] {name} P1[{}] +{first:#x}", p.index);
            }
        }
    }
    eprintln!("[census] {escape_led} of {records} talkable records open on an escape-led line");
    assert!(records > 0, "the census walked no records");
}
