//! Save / load round trip over every save on the library memory cards.
//!
//! For each Legaia save on each card in `$LEGAIA_SAVES_LIBRARY/cards` (the
//! retail card saves the probe corpus is catalogued against), this drives the
//! paths a player's Load and Save take on both play hosts:
//!
//! 1. **Load fidelity.** Lift the block through `MountedCard::save_at` (the
//!    reader both hosts' card Load uses) and land it with
//!    `BootSession::resume_save` (the shared `resume_card_load` kernel the
//!    browser page runs too). Straight after the landing, `save_full` must
//!    give back every field the block carries: the four party records byte
//!    for byte, the story-flag window, the whole item array, the gold, the
//!    minigame purses, the present party, the field position, the audio
//!    levels and the play clock.
//! 2. **Save / load round trip.** After the scene has run a while, the save
//!    a player would write is composed into a blank card through
//!    `card_write::write_save_into_card` (both hosts' card Save), read back
//!    through `save_at`, and resumed again. The second `save_full` must equal
//!    the first field for field - the soak harness's `+rt` check, here over
//!    every library save rather than the scenes a random walk reaches.
//!
//! The same round trip also runs with each card's latest save entered into a
//! spread of scenes it was not written in (towns, overworld, dungeons), the
//! way the soak's `<scene>@<save>` runs do.
//!
//! Disc-gated: skips (and passes) without `LEGAIA_DISC_BIN`, and without the
//! library cards. No Sony bytes are asserted - only equalities between two
//! readings of the same save.

use std::path::{Path, PathBuf};

use legaia_engine_core::card_write::write_save_into_card;
use legaia_engine_core::world::SceneMode;
use legaia_engine_shell::boot::{BootConfig, BootSession, FieldLiveOpts};
use legaia_save::emu::MountedCard;
use legaia_save::{SaveFile, SaveResume};

/// Frames the landed scene runs before the save under test is taken: long
/// enough for entry scripts, placements and the first ambient spawns to have
/// re-stamped the actor table (`opurud`'s scripts take slots 1 and 2 within
/// the first second).
const SETTLE_FRAMES: u32 = 600;

/// Scenes every card's latest save is also entered into: towns, the three
/// kingdom overworlds and dungeons from each act.
const SPREAD_SCENES: &[&str] = &[
    "town01", "opurud", "map01", "map02", "map03", "rikuroa", "geremi", "jou",
];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn open_session() -> Option<BootSession> {
    let disc = std::env::var_os("LEGAIA_DISC_BIN").map(PathBuf::from)?;
    if !disc.exists() {
        return None;
    }
    let cfg = BootConfig {
        scene: legaia_engine_shell::boot::DEFAULT_BOOT_SCENE.to_string(),
        enable_audio: false,
    };
    let mut roots = Vec::new();
    if let Some(d) = std::env::var_os("LEGAIA_EXTRACTED_DIR") {
        roots.push(PathBuf::from(d));
    }
    roots.push(repo_root().join("extracted"));
    for d in roots {
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(BootSession::open(&d, &cfg).expect("open BootSession (extracted)"));
        }
    }
    Some(BootSession::open_disc(&disc, &cfg).expect("open BootSession (disc)"))
}

fn opts() -> FieldLiveOpts {
    FieldLiveOpts {
        // No random encounter may start while the scene settles: the check is
        // about the save, and a battle is not a frame a player saves on.
        live_loop: false,
        player_battle: true,
        battle_bgm: None,
    }
}

/// One Legaia save on a library card.
struct LibrarySave {
    card: String,
    name: String,
    save: SaveFile,
    resume: SaveResume,
    /// The block's own play-time counter (`0x80084570`, 60 ticks a second),
    /// read straight off the bytes rather than through the lift.
    play_ticks: u32,
}

/// Every Legaia save on the library cards, de-duplicated by block content.
fn library_saves() -> Vec<LibrarySave> {
    let lib = std::env::var_os("LEGAIA_SAVES_LIBRARY")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("saves/library"));
    let Ok(rd) = std::fs::read_dir(lib.join("cards")) else {
        return Vec::new();
    };
    let mut cards: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
    cards.sort();
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for path in cards {
        let Ok(card) = MountedCard::open(&path) else {
            continue;
        };
        let file = path.file_name().unwrap().to_string_lossy().into_owned();
        for block in 1..=15u8 {
            let Some(frame) = card.dir_frame(block) else {
                continue;
            };
            let name: String = frame[0x0A..0x0A + 20]
                .iter()
                .take_while(|&&b| b != 0)
                .map(|&b| b as char)
                .collect();
            if !name.starts_with("BASCUS-94254") {
                continue;
            }
            let Some(sc) = card.sc_block(block) else {
                continue;
            };
            if !seen.insert(sc.to_vec()) {
                continue;
            }
            let play_ticks = u32::from_le_bytes(sc[0x430..0x434].try_into().unwrap());
            let Some((save, resume)) = card.save_at(block - 1) else {
                continue;
            };
            out.push(LibrarySave {
                card: file.clone(),
                name,
                save,
                resume,
                play_ticks,
            });
        }
    }
    out
}

/// Every field two saves disagree on, named.
///
/// `ext.story_flags` is the field VM's scratchpad word (`_DAT_1F800394`),
/// which no retail block carries: the card codec and a card resume are
/// compared with [`card_view`] applied, the LGSF codec without. The compact
/// `ext.inventory` list is compared as a multiset - the engine writes it
/// id-sorted, a block lifts it in slot order, and `ext.item_slots` is the
/// array both read back from.
fn save_diffs(a: &SaveFile, b: &SaveFile) -> Vec<String> {
    let mut out = Vec::new();
    let sorted = |v: &Vec<(u8, u8)>| {
        let mut v = v.clone();
        v.sort_unstable();
        v
    };
    if a.party.members.len() != b.party.members.len() {
        out.push(format!(
            "party.len {} -> {}",
            a.party.members.len(),
            b.party.members.len()
        ));
    }
    for (i, (x, y)) in a.party.members.iter().zip(&b.party.members).enumerate() {
        let offs: Vec<usize> = (0..x.raw.len()).filter(|&o| x.raw[o] != y.raw[o]).collect();
        if let Some(&o) = offs.first() {
            out.push(format!(
                "party[{i}]: {} bytes differ, first +{o:#05x} {:#04x} -> {:#04x}",
                offs.len(),
                x.raw[o],
                y.raw[o]
            ));
        }
    }
    macro_rules! field {
        ($name:literal, $x:expr, $y:expr) => {
            if $x != $y {
                let (dx, dy) = (format!("{:?}", $x), format!("{:?}", $y));
                let cut = |s: &str| s.chars().take(120).collect::<String>();
                out.push(format!("{}: {} -> {}", $name, cut(&dx), cut(&dy)));
            }
        };
    }
    if a.ext.story_flag_bits != b.ext.story_flag_bits {
        let n = a.ext.story_flag_bits.len().max(b.ext.story_flag_bits.len());
        let byte = |v: &Vec<u8>, i: usize| v.get(i).copied().unwrap_or(0);
        let offs: Vec<usize> = (0..n)
            .filter(|&i| byte(&a.ext.story_flag_bits, i) != byte(&b.ext.story_flag_bits, i))
            .collect();
        out.push(format!(
            "ext.story_flag_bits: len {} -> {}, {} bytes differ, first {:?}",
            a.ext.story_flag_bits.len(),
            b.ext.story_flag_bits.len(),
            offs.len(),
            offs.first().map(|&i| (
                format!("{i:#x}"),
                byte(&a.ext.story_flag_bits, i),
                byte(&b.ext.story_flag_bits, i)
            ))
        ));
    }
    field!("ext.story_flags", a.ext.story_flags, b.ext.story_flags);
    field!("ext.money", a.ext.money, b.ext.money);
    field!("ext.item_slots", a.ext.item_slots, b.ext.item_slots);
    field!(
        "ext.inventory",
        sorted(&a.ext.inventory),
        sorted(&b.ext.inventory)
    );
    field!("ext.minigames", a.ext.minigames, b.ext.minigames);
    field!(
        "ext_v2.active_party",
        a.ext_v2.active_party,
        b.ext_v2.active_party
    );
    field!(
        "ext_v2.field_position",
        a.ext_v2.field_position,
        b.ext_v2.field_position
    );
    field!(
        "ext_v2.audio_levels",
        a.ext_v2.audio_levels,
        b.ext_v2.audio_levels
    );
    field!(
        "ext_v2.play_time_seconds",
        a.ext_v2.play_time_seconds,
        b.ext_v2.play_time_seconds
    );
    field!("ext_v2.per_char", a.ext_v2.per_char, b.ext_v2.per_char);
    field!(
        "ext_v2.saved_chains",
        a.ext_v2.saved_chains,
        b.ext_v2.saved_chains
    );
    out
}

/// The fields a retail block carries, from a full engine save - what a
/// load-fidelity comparison against a lifted retail block may look at.
fn retail_carried(sf: &SaveFile) -> SaveFile {
    let mut out = sf.clone();
    out.ext_v2.per_char.clear();
    out.ext_v2.saved_chains.clear();
    out
}

/// A save as a retail block can hold it: without the scratchpad word.
fn card_view(sf: &SaveFile) -> SaveFile {
    let mut out = sf.clone();
    out.ext.story_flags = 0;
    out
}

fn blank_card() -> MountedCard {
    use legaia_save::card::{CARD_MAGIC, CARD_SIZE, DIR_FRAME_SIZE, DIR_FRAMES, state};
    let mut buf = vec![0u8; CARD_SIZE];
    buf[..2].copy_from_slice(&CARD_MAGIC);
    for i in 1..=DIR_FRAMES {
        let off = DIR_FRAME_SIZE * i;
        buf[off..off + 4].copy_from_slice(&state::FREE.to_le_bytes());
    }
    MountedCard::from_bytes(buf, "blank").expect("blank card")
}

/// A frame a player could save on: free field roam, nothing modal.
fn save_ready(s: &BootSession) -> bool {
    let w = &s.host.world;
    matches!(w.mode, SceneMode::Field | SceneMode::WorldMap)
        && s.field_menu.is_none()
        && w.cutscene.timeline.is_none()
        && w.dialog.current.is_none()
        && w.dialog.inline.is_none()
        && !w.name_entry_active()
        && !w.shops.shop_open
        && w.active_fmv().is_none()
}

fn step(session: &mut BootSession, pad: u16) {
    session.host.world.set_pad(pad);
    let _ = session.tick();
    let w = &mut session.host.world;
    let _ = w.drain_field_events();
    let _ = w.drain_battle_events();
}

/// Run the landed scene the way a player would before saving: walk a square
/// (so walk-on triggers, placements and their scripts run against the
/// party), stand for [`SETTLE_FRAMES`], then tap Cross through whatever the
/// walk opened until the field is free again. `false` when it never is.
fn settle(session: &mut BootSession) -> bool {
    const UP: u16 = 0x0010;
    const RIGHT: u16 = 0x0020;
    const DOWN: u16 = 0x0040;
    const LEFT: u16 = 0x0080;
    const CROSS: u16 = 0x4000;
    for dir in [UP, RIGHT, DOWN, LEFT, DOWN, LEFT, UP, RIGHT] {
        for _ in 0..45 {
            step(session, dir);
        }
    }
    for _ in 0..SETTLE_FRAMES {
        step(session, 0);
    }
    for f in 0..3000u32 {
        if save_ready(session) {
            return true;
        }
        step(session, if f % 20 == 0 { CROSS } else { 0 });
    }
    save_ready(session)
}

/// The save a player would write now, through a blank card and back, then
/// resumed: every disagreement between the two `save_full`s, plus the card
/// codec's own (the block read back vs the save written into it).
fn card_round_trip(session: &mut BootSession) -> Vec<String> {
    let mut problems = Vec::new();
    let before = session.host.world.save_full();
    let resume = session.current_resume();
    let mut card = blank_card();
    if let Err(e) = write_save_into_card(&mut card, 1, &before, &resume, None) {
        return vec![format!("card write: {e}")];
    }
    let Some((read, read_resume)) = card.save_at(0) else {
        return vec!["card read: block 1 holds no save".into()];
    };
    for d in save_diffs(&card_view(&before), &card_view(&read)) {
        problems.push(format!("card codec {d}"));
    }
    if read_resume.scene != resume.scene {
        problems.push(format!(
            "card codec resume scene {:?} -> {:?}",
            resume.scene, read_resume.scene
        ));
    }
    // The LGSF file the native window's slot saves write.
    match SaveFile::parse_with_resume(&before.write_with_resume(&resume)) {
        Ok((lgsf, _)) => {
            for d in save_diffs(&before, &lgsf) {
                problems.push(format!("LGSF codec {d}"));
            }
        }
        Err(e) => problems.push(format!("LGSF parse: {e:#}")),
    }
    let _ = session.resume_save(read, &read_resume.scene, &opts());
    let after = session.host.world.save_full();
    for d in save_diffs(&card_view(&before), &card_view(&after)) {
        problems.push(format!("resume {d}"));
    }
    problems
}

#[test]
fn every_library_save_survives_load_and_a_save_load_round_trip() {
    let Some(mut session) = open_session() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or missing (disc-gated)");
        return;
    };
    let saves = library_saves();
    if saves.is_empty() {
        eprintln!("[skip] no library card saves (LEGAIA_SAVES_LIBRARY)");
        return;
    }
    let mut failures = Vec::new();
    let mut checked = 0usize;
    for ls in &saves {
        let (card, name, sf, resume) = (&ls.card, &ls.name, &ls.save, &ls.resume);
        session.begin_new_game();
        let landing = session.resume_save(sf.clone(), &resume.scene, &opts());
        let tag = format!("{card} {name} ({})", resume.scene);
        if !landing.entered_scene() {
            failures.push(format!("{tag}: landing {} did not enter", landing.kind()));
            continue;
        }
        // 1. Load fidelity.
        if session.host.world.clock.play_time_seconds != ls.play_ticks / 60 {
            failures.push(format!(
                "{tag}: load play clock {} s, the block's counter says {} s",
                session.host.world.clock.play_time_seconds,
                ls.play_ticks / 60
            ));
        }
        let loaded = session.host.world.save_full();
        for d in save_diffs(
            &card_view(&retail_carried(sf)),
            &card_view(&retail_carried(&loaded)),
        ) {
            failures.push(format!("{tag}: load {d}"));
        }
        // 2. Save / load round trip after the scene has run.
        if !settle(&mut session) {
            eprintln!(
                "[note] {tag}: never free to save ({:?} in {}); round trip taken there",
                session.host.world.mode, session.host.world.active_scene_label
            );
        }
        for p in card_round_trip(&mut session) {
            failures.push(format!("{tag}: {p}"));
        }
        checked += 1;
    }
    // 3. Each card's latest save across a spread of scenes.
    let mut latest: std::collections::BTreeMap<&str, &LibrarySave> = Default::default();
    for s in &saves {
        let newer = latest
            .get(s.card.as_str())
            .is_none_or(|l| s.play_ticks > l.play_ticks);
        if newer {
            latest.insert(s.card.as_str(), s);
        }
    }
    let mut spread = 0usize;
    for ls in latest.values() {
        let (card, name, sf) = (&ls.card, &ls.name, &ls.save);
        for &scene in SPREAD_SCENES {
            session.begin_new_game();
            if let Err(e) = session.enter_field_live_from_save(scene, &opts(), sf.clone()) {
                failures.push(format!("{card} {name} @{scene}: enter: {e:#}"));
                continue;
            }
            if !settle(&mut session) {
                eprintln!(
                    "[note] {card} {name} @{scene}: never free to save ({:?} in {})",
                    session.host.world.mode, session.host.world.active_scene_label
                );
            }
            for p in card_round_trip(&mut session) {
                failures.push(format!("{card} {name} @{scene}: {p}"));
            }
            spread += 1;
        }
    }
    eprintln!(
        "[ran] {checked} library saves round-tripped, {spread} save x scene spread runs, \
         {} problem(s)",
        failures.len()
    );
    assert!(
        failures.is_empty(),
        "save / load mismatches:\n{}",
        failures.join("\n")
    );
}
