//! The save rack behind the save-select / load screens: the two card ports,
//! each a mounted memory-card image - port 1 the window's own card in the
//! save directory, port 2 whatever `--card` names - plus the reader for the
//! LGSF slot files an older build wrote into that directory, which the
//! port-1 card imports the first time it is created.

use super::*;

/// The mounted-card type, shared with the browser rack.
///
/// Both hosts hold the same struct: it caches the detected
/// [`legaia_save::emu::CardView`], carries the dirty bit a host that writes
/// needs, and answers `save_at` / `block_is_save_start` / `dir_frame` off its
/// own bytes.
pub(crate) use legaia_save::emu::MountedCard;

/// The port-1 card's file inside the save directory: a raw 128 KiB `.mcr`,
/// the same container the browser play page keeps in its storage for port 1
/// and the one any PSX emulator or memory-card tool reads.
pub(crate) const PORT_ONE_CARD_FILE: &str = "memcard1.mcr";

/// Mount the window's own card for **port 1**.
///
/// Port 1 used to be the save directory itself, read as fifteen LGSF files
/// (`slot_NN`), while the browser page mounted a card image there - so the
/// two hosts kept the same save in two formats, and a save made in one could
/// not be put in the other's port. Both mount a memory-card image now, and
/// every Save / Load goes through the one card kernel
/// ([`legaia_engine_menu_runtime::card_write::write_save_into_card`], `save_at`).
///
/// When `save_dir` holds no card yet, a freshly formatted one is mounted and
/// every LGSF slot an older build left there is written into it, slot `N`
/// into grid cell `N` (block `N + 1`), and the card is written to disk at
/// once so the import happens one time. The LGSF files are left in place:
/// `--resume-save` still loads any of them directly.
pub(crate) fn open_port_one_card(
    save_dir: &Path,
    index: Option<&legaia_engine_scene::scene::ProtIndex>,
) -> MountedCard {
    let path = save_dir.join(PORT_ONE_CARD_FILE);
    if path.is_file() {
        match MountedCard::open(&path) {
            Ok(card) => return card,
            Err(e) => log::warn!(
                "save rack: port-1 card {} does not mount ({e:#}); using a blank card",
                path.display()
            ),
        }
        // A file that does not mount is kept, not overwritten: the blank
        // card below has no path, so nothing this session saves can clobber
        // it.
        return blank_card(None);
    }
    let mut card = blank_card(Some(path));
    let imported = import_legacy_slots(save_dir, &mut card, index);
    if imported > 0 {
        match std::fs::create_dir_all(save_dir)
            .map_err(anyhow::Error::from)
            .and_then(|_| card.persist())
        {
            Ok(_) => log::info!(
                "save rack: imported {imported} LGSF save(s) from {} into {}",
                save_dir.display(),
                PORT_ONE_CARD_FILE
            ),
            Err(e) => log::warn!("save rack: port-1 card not written: {e:#}"),
        }
    }
    card
}

/// A freshly formatted card, to be written to `path` on its first save.
fn blank_card(path: Option<std::path::PathBuf>) -> MountedCard {
    let mut card = MountedCard::from_bytes(
        legaia_save::card::formatted_card_image(),
        PORT_ONE_CARD_FILE,
    )
    .expect("a formatted card image mounts");
    card.path = path;
    card
}

/// Write every readable LGSF slot under `save_dir` into `card`, slot `N` into
/// grid cell `N`. Returns how many were written.
pub(crate) fn import_legacy_slots(
    save_dir: &Path,
    card: &mut MountedCard,
    index: Option<&legaia_engine_scene::scene::ProtIndex>,
) -> usize {
    let mut imported = 0;
    for (slot, snap) in scan_save_dir(save_dir).iter().enumerate() {
        if !snap.present {
            continue;
        }
        let Ok((sf, resume)) = read_slot_save(save_dir, slot as u8) else {
            continue;
        };
        match legaia_engine_menu_runtime::card_write::write_save_into_card(
            card,
            slot as u8 + 1,
            &sf,
            &resume,
            index,
        ) {
            Ok(_) => imported += 1,
            Err(e) => log::warn!("save rack: LGSF slot {slot} not imported: {e}"),
        }
    }
    imported
}

/// The window's save rack, the block read behind a port and the
/// Continue-enable scan are the session's
/// ([`legaia_engine_session::title_continue`]), decided once for both play
/// hosts: each port is a [`MountedCard`] on either.
pub(crate) use legaia_engine_session::title_continue::{
    card_port_blocks, card_port_rack, rack_has_save,
};

/// Path of `slot`'s LGSF file under `save_dir` - the shape
/// `MenuRuntime::slot_path` writes (`slot_NN.<SAVE_EXT>`, zero-padded).
pub(crate) fn slot_file_path(save_dir: &Path, slot: u8) -> std::path::PathBuf {
    use legaia_engine_menu_runtime::menu_runtime::SAVE_EXT;
    save_dir.join(format!("slot_{slot:02}.{SAVE_EXT}"))
}

/// Read and parse `slot`'s LGSF file together with its resume point (empty
/// for a file written before the `LGX5` trailer existed).
pub(crate) fn read_slot_save(
    save_dir: &Path,
    slot: u8,
) -> anyhow::Result<(legaia_save::SaveFile, legaia_save::SaveResume)> {
    use anyhow::Context;
    let path = slot_file_path(save_dir, slot);
    let bytes = std::fs::read(&path)
        .with_context(|| format!("read save slot {slot} from {}", path.display()))?;
    legaia_save::SaveFile::parse_with_resume(&bytes)
        .with_context(|| format!("parse save slot {slot} ({} bytes)", bytes.len()))
}

/// Write `sf` + its resume point to `slot`'s LGSF file, creating `save_dir` -
/// the files an older build wrote, which the tests seed.
#[cfg(test)]
/// The counterpart of [`read_slot_save`]; the same file shape
/// `MenuRuntime::save_to_slot` writes, plus the trailer that runtime has no
/// scene to fill.
pub(crate) fn write_slot_save(
    save_dir: &Path,
    slot: u8,
    sf: &legaia_save::SaveFile,
    resume: &legaia_save::SaveResume,
) -> anyhow::Result<std::path::PathBuf> {
    use anyhow::Context;
    std::fs::create_dir_all(save_dir)
        .with_context(|| format!("create save dir {}", save_dir.display()))?;
    let path = slot_file_path(save_dir, slot);
    std::fs::write(&path, sf.write_with_resume(resume))
        .with_context(|| format!("write save slot {slot} to {}", path.display()))?;
    Ok(path)
}

/// The [`SlotSnapshot`](legaia_engine_menus::save_select::SlotSnapshot) a
/// parsed save presents on the load screen: the lead record's own name /
/// level / HP / MP through [`legaia_save::SaveFile::leader_summary`] - the
/// one derivation the browser's card rack uses too - and the resume point's
/// location row. A save with no party records is not loadable and reads as
/// foreign.
pub(crate) fn snapshot_for_save(
    slot: u8,
    sf: &legaia_save::SaveFile,
    resume: &legaia_save::SaveResume,
) -> legaia_engine_menus::save_select::SlotSnapshot {
    use legaia_engine_menus::save_select::{SlotContent, SlotSnapshot};
    let Some(leader) = sf.leader_summary() else {
        return SlotSnapshot::foreign(slot);
    };
    // The location row is the banner name retail prints; a save written
    // before the resume trailer existed (or in a scene whose MAN has no
    // printable banner) falls back to the scene label, then to nothing -
    // never to an invented kingdom.
    let location = if !resume.location.is_empty() {
        resume.location.clone()
    } else {
        resume.scene.clone()
    };
    SlotSnapshot {
        slot,
        present: true,
        damaged: false,
        content: SlotContent::LegaiaSave,
        label: if leader.name.is_empty() {
            format!("Slot {}", slot + 1)
        } else {
            leader.name.clone()
        },
        play_time_seconds: sf.ext_v2.play_time_seconds,
        party_lv: leader.level,
        location,
        money: sf.ext.money.max(0) as u32,
        leader_char_id: leader.char_id,
        leader_name: leader.name,
        leader_hp: leader.hp,
        leader_mp: leader.mp,
    }
}

pub(crate) fn scan_save_dir(
    save_dir: &Path,
) -> Vec<legaia_engine_menus::save_select::SlotSnapshot> {
    use legaia_engine_menus::save_select::SlotSnapshot;
    // Scan up to 15 slots (one per retail PSX memory-card block) so
    // the load-screen 5×3 grid can render every potential slot.
    const MAX_SLOTS: u8 = 15;
    let mut out = Vec::with_capacity(MAX_SLOTS as usize);
    for slot in 0..MAX_SLOTS {
        // Saves are written by the field menu via `MenuRuntime` as
        // `<dir>/slot_NN.<SAVE_EXT>` (zero-padded slot, see
        // `menu_runtime::slot_path`). The title-screen and
        // save-select scanners must use the same shape; an earlier
        // mismatch (`slot_N.lgsf`) made every save invisible at boot,
        // greying out Continue even with valid saves on disk.
        let path = slot_file_path(save_dir, slot);
        // Only a missing file proves the slot is free. Every other
        // outcome - an unreadable file, or one whose bytes don't parse -
        // means the slot is occupied by something we can't load, which
        // the info panel captions differently ("Not a Legend of Legaia
        // save." vs "Able to save."). Folding the two into one `None`
        // invites the Save screen to offer a slot whose write would then
        // clobber or fail.
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                out.push(SlotSnapshot::empty(slot));
                continue;
            }
            Err(_) => {
                out.push(SlotSnapshot::foreign(slot));
                continue;
            }
        };
        let snap = match legaia_save::SaveFile::parse_with_resume(&bytes) {
            Ok((sf, resume)) => snapshot_for_save(slot, &sf, &resume),
            Err(_) => SlotSnapshot::foreign(slot),
        };
        out.push(snap);
    }
    out
}

#[cfg(test)]
mod save_scan_tests {
    use super::scan_save_dir;
    use legaia_engine_menu_runtime::menu_runtime::SAVE_EXT;
    use legaia_engine_menus::save_select::{SaveSelectMode, SlotContent, SlotInfoMode};
    use legaia_save::{CharacterRecord, Party, SaveExt, SaveFile};

    fn slot_path(dir: &std::path::Path, slot: u8) -> std::path::PathBuf {
        dir.join(format!("slot_{slot:02}.{SAVE_EXT}"))
    }

    fn a_save() -> Vec<u8> {
        SaveFile {
            party: Party {
                members: vec![CharacterRecord::zeroed()],
            },
            ext: SaveExt {
                money: 100,
                ..SaveExt::default()
            },
            ..SaveFile::default()
        }
        .write()
    }

    /// The whole point of the split: an absent file and an unparseable one
    /// must not land in the same class. A corrupt save is not a free block,
    /// and offering it as one is how a Save overwrites what it never read.
    #[test]
    fn corrupt_save_is_foreign_missing_save_is_free() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(slot_path(dir.path(), 0), a_save()).unwrap();
        // Right extension, wrong bytes - fails the LGSF magic check.
        std::fs::write(slot_path(dir.path(), 1), b"not a save at all").unwrap();
        // A real save truncated mid-body: passes the magic, fails the parse.
        let mut torn = a_save();
        torn.truncate(6);
        std::fs::write(slot_path(dir.path(), 2), torn).unwrap();
        // Slot 3 is left absent.

        let slots = scan_save_dir(dir.path());

        assert_eq!(slots[0].content, SlotContent::LegaiaSave);
        assert!(slots[0].present);
        for slot in [1, 2] {
            assert_eq!(
                slots[slot].content,
                SlotContent::Foreign,
                "slot {slot}: an unparseable file occupies the block"
            );
            assert!(!slots[slot].present, "slot {slot} must not be loadable");
        }
        assert_eq!(slots[3].content, SlotContent::Free);
        assert!(!slots[3].present);
    }

    /// A path that exists but cannot be read is occupied, not free -
    /// the `Err(_)` arm that is not `NotFound`.
    #[test]
    fn unreadable_path_is_foreign() {
        let dir = tempfile::tempdir().unwrap();
        // A directory where a save file belongs: `read` fails with
        // IsADirectory, not NotFound.
        std::fs::create_dir(slot_path(dir.path(), 0)).unwrap();

        let slots = scan_save_dir(dir.path());

        assert_eq!(slots[0].content, SlotContent::Foreign);
        assert!(!slots[0].present);
    }

    /// The classification only matters because it picks the caption -
    /// pin the end-to-end mapping the player actually sees.
    #[test]
    fn corrupt_and_missing_caption_differently() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(slot_path(dir.path(), 0), b"junk").unwrap();

        let slots = scan_save_dir(dir.path());
        let corrupt = SlotInfoMode::for_slot(&slots[0]);
        let missing = SlotInfoMode::for_slot(&slots[1]);

        assert_eq!(corrupt, SlotInfoMode::NotLegaiaSave);
        assert_eq!(missing, SlotInfoMode::FreeBlock);
        for mode in [SaveSelectMode::Save, SaveSelectMode::Load] {
            assert_eq!(corrupt.caption(mode), Some("Not a Legend of Legaia save."));
            assert_ne!(missing.caption(mode), corrupt.caption(mode));
        }
        assert_eq!(missing.caption(SaveSelectMode::Save), Some("Able to save."));
        assert_eq!(missing.caption(SaveSelectMode::Load), Some("No data"));
    }
}

#[cfg(test)]
mod save_rack_tests {
    use super::{
        MountedCard, PORT_ONE_CARD_FILE, card_port_blocks, card_port_rack, open_port_one_card,
        scan_save_dir,
    };
    use legaia_engine_menu_runtime::menu_runtime::SAVE_EXT;
    use legaia_engine_menus::save_screen::{
        SLOT_GRID_CELLS, SaveCommit, SaveCommitKind, SaveScreenFlow,
    };
    use legaia_engine_menus::save_select::{
        SaveRack, SaveSelectMode, SaveSelectSession, SelectInput, SelectPhase,
    };
    use legaia_engine_session::title_continue::CARD_PORTS;
    use legaia_save::{CharacterRecord, Party, SaveFile};

    fn seeded_dir(slots: &[u8]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        // A lead with a name and HP: a zeroed record reads as a save in an
        // LGSF file but as no party at all in a card block, which is the
        // carrier the import writes.
        let mut lead = CharacterRecord::zeroed();
        lead.set_name("Vahn");
        lead.set_hp_mp_sp(legaia_save::HpMpSp {
            hp_cur: 40,
            hp_max: 40,
            mp_cur: 2,
            mp_max: 2,
            sp_cur: 0,
            sp_max: 0,
        });
        let bytes = SaveFile {
            party: Party {
                members: vec![lead],
            },
            ..SaveFile::default()
        }
        .write();
        for &slot in slots {
            std::fs::write(
                dir.path().join(format!("slot_{slot:02}.{SAVE_EXT}")),
                &bytes,
            )
            .unwrap();
        }
        dir
    }

    /// Retail's save screen is two pills. Feeding the pill row fifteen disk
    /// slots drew fifteen text rows under two pill sprites - the sprite
    /// chrome has always clamped to `min(2)`, so the mismatch was invisible
    /// in the sprite pass and glaring in the text pass.
    /// Port 1 of a save directory an older build wrote LGSF slots into.
    fn port_one(dir: &tempfile::TempDir) -> MountedCard {
        open_port_one_card(dir.path(), None)
    }

    #[test]
    fn the_rack_is_two_ports_with_the_window_card_in_port_one() {
        let dir = seeded_dir(&[0]);
        let card = port_one(&dir);
        let rack = card_port_rack([Some(&card), None]);
        assert!(
            rack.is_card_ports(),
            "the shell runs retail's two-stage flow"
        );
        assert_eq!(rack.slots().len(), CARD_PORTS);
        assert!(rack.slots()[0].present, "port 1 is the window's card");
        assert_eq!(rack.slots()[0].label, PORT_ONE_CARD_FILE);
        assert!(!rack.slots()[1].present, "port 2 holds no card");
    }

    /// The LGSF slots an older build wrote land in the port-1 card on the
    /// cell they had (slot `N` -> cell `N`); every other port is unmounted
    /// and previews as fifteen free cells rather than as port 1's.
    #[test]
    fn port_one_imports_the_legacy_slots_cell_for_cell() {
        let dir = seeded_dir(&[2]);
        let card = port_one(&dir);
        let blocks = card_port_blocks([Some(&card), None], 0);
        let legacy = scan_save_dir(dir.path());
        for (i, (b, l)) in blocks.iter().zip(&legacy).enumerate() {
            assert_eq!(b.present, l.present, "cell {i}");
        }
        assert!(blocks[2].present);
        let empty = card_port_blocks([Some(&card), None], 1);
        assert_eq!(empty.len(), SLOT_GRID_CELLS as usize);
        assert!(empty.iter().all(|b| !b.present));
    }

    /// The import happens once: the card is written to the directory the
    /// first time, and from then on the card file is port 1 - a slot file
    /// left beside it is not imported again, and is not deleted either
    /// (`--resume-save` still loads it).
    #[test]
    fn the_import_writes_the_card_once_and_keeps_the_slot_files() {
        let dir = seeded_dir(&[0]);
        let _ = port_one(&dir);
        let card_path = dir.path().join(PORT_ONE_CARD_FILE);
        assert!(card_path.is_file(), "the imported card is on disk");
        assert!(scan_save_dir(dir.path())[0].present, "the slot file stays");
        let before = std::fs::read(&card_path).unwrap();
        // A slot file written after the import is not pulled in.
        let later = seeded_dir(&[4]);
        std::fs::copy(
            later.path().join(format!("slot_04.{SAVE_EXT}")),
            dir.path().join(format!("slot_04.{SAVE_EXT}")),
        )
        .unwrap();
        let again = port_one(&dir);
        assert_eq!(again.bytes, before);
        assert!(!card_port_blocks([Some(&again), None], 0)[4].present);
    }

    /// A directory with no slot files mounts a blank card and writes nothing
    /// until the first Save.
    #[test]
    fn an_empty_directory_mounts_a_blank_card_and_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let card = open_port_one_card(dir.path(), None);
        assert!(!dir.path().join(PORT_ONE_CARD_FILE).exists());
        assert!(
            card_port_blocks([Some(&card), None], 0)
                .iter()
                .all(|b| !b.present)
        );
        assert_eq!(
            card.path.as_deref(),
            Some(dir.path().join(PORT_ONE_CARD_FILE).as_path())
        );
    }

    /// The whole two-stage walk, end to end: pick the port off the pills,
    /// cross the card-read beat, walk the block grid, confirm. What comes
    /// out must name the block the player was pointing at - the pre-rack
    /// code committed the *pill* slot, which in a two-port rack is only
    /// ever 0 or 1.
    #[test]
    fn picking_a_grid_cell_commits_that_slot_not_the_port() {
        let dir = seeded_dir(&[0, 3]);
        let card = port_one(&dir);
        let cards = [Some(&card), None];
        let mut session = SaveSelectSession::for_rack(SaveSelectMode::Load, &card_port_rack(cards));
        let mut flow = SaveScreenFlow::new();
        let step = |session: &mut SaveSelectSession, flow: &mut SaveScreenFlow, edge: u16| {
            if let Some(port) = flow.pending_read(session) {
                flow.install_blocks(port, card_port_blocks(cards, port));
            }
            let edge = flow.before_tick(session, edge);
            session.tick(SelectInput::from_pad_edge(edge));
        };
        // Confirm port 1, then run the "Now checking" beat out.
        step(
            &mut session,
            &mut flow,
            legaia_engine_system::input::PadButton::Cross.mask(),
        );
        for _ in 0..session.now_checking_frames() + 1 {
            step(&mut session, &mut flow, 0);
        }
        assert!(matches!(session.phase(), SelectPhase::SlotPreview { .. }));
        // Cell 0 holds a save, cell 1 does not: a Load confirm there is
        // refused, so the walk to cell 3 has to be uninterrupted.
        step(
            &mut session,
            &mut flow,
            legaia_engine_system::input::PadButton::Right.mask()
                | legaia_engine_system::input::PadButton::Cross.mask(),
        );
        assert!(
            matches!(session.phase(), SelectPhase::SlotPreview { .. }),
            "cell 1 is empty - retail refuses the load rather than failing it"
        );
        for _ in 0..2 {
            step(
                &mut session,
                &mut flow,
                legaia_engine_system::input::PadButton::Right.mask(),
            );
        }
        // Cross raises "Do you wish to load?" (defaulting to No); Yes runs
        // "Now Loading" / "Load successful." before the outcome lands.
        step(
            &mut session,
            &mut flow,
            legaia_engine_system::input::PadButton::Cross.mask(),
        );
        assert_eq!(flow.commit(&session), None, "the load asks first");
        step(
            &mut session,
            &mut flow,
            legaia_engine_system::input::PadButton::Left.mask(),
        );
        step(
            &mut session,
            &mut flow,
            legaia_engine_system::input::PadButton::Cross.mask(),
        );
        while !session.is_done() {
            step(&mut session, &mut flow, 0);
        }
        assert_eq!(
            flow.commit(&session),
            Some(SaveCommit {
                port: 0,
                cell: 3,
                kind: SaveCommitKind::Load,
            })
        );
    }

    /// A flat rack is what the shell used to build, and it is what the drift
    /// gate now refuses: pin the difference so a revert is a red test and not
    /// just a red gate.
    #[test]
    fn the_flat_rack_is_no_longer_what_the_shell_declares() {
        let dir = seeded_dir(&[0]);
        let flat = SaveRack::Blocks(scan_save_dir(dir.path()));
        assert!(!flat.is_card_ports());
        assert_eq!(flat.slots().len(), SLOT_GRID_CELLS as usize);
        let card = port_one(&dir);
        assert_ne!(
            card_port_rack([Some(&card), None]).slots().len(),
            flat.slots().len()
        );
    }
}

#[cfg(test)]
mod slot_model_tests {
    use super::{read_slot_save, scan_save_dir, write_slot_save};
    use legaia_engine_menus::save_select::SlotContent;
    use legaia_save::{CharacterRecord, HpMpSp, Party, SaveFile, SaveResume};

    fn a_save() -> SaveFile {
        let mut r = CharacterRecord::zeroed();
        r.set_name("Vahn");
        r.set_magic_rank(7);
        r.set_hp_mp_sp(HpMpSp {
            hp_cur: 90,
            hp_max: 120,
            mp_cur: 3,
            mp_max: 9,
            sp_cur: 0,
            sp_max: 0,
        });
        SaveFile {
            party: Party { members: vec![r] },
            ..SaveFile::default()
        }
    }

    /// The panel used to print `"Vahn"` / `"Drake Kingdom"` / `"Slot N"`
    /// whatever the file held. It reads the record and the resume point now.
    #[test]
    fn the_load_screen_prints_the_record_and_the_saved_scene() {
        let dir = tempfile::tempdir().unwrap();
        let resume = SaveResume {
            scene: "town01".into(),
            location: "Rim Elm".into(),
        };
        write_slot_save(dir.path(), 2, &a_save(), &resume).unwrap();
        let slots = scan_save_dir(dir.path());
        let snap = &slots[2];
        assert_eq!(snap.content, SlotContent::LegaiaSave);
        assert_eq!(snap.leader_name, "Vahn");
        assert_eq!(snap.label, "Vahn");
        assert_eq!(snap.party_lv, 7);
        assert_eq!(snap.leader_hp, (90, 120));
        assert_eq!(snap.leader_mp, (3, 9));
        assert_eq!(snap.location, "Rim Elm");
        let (_, back) = read_slot_save(dir.path(), 2).unwrap();
        assert_eq!(back, resume);
    }

    /// A file written before the trailer existed: the location row is the
    /// scene label if any, else empty - not an invented kingdom.
    #[test]
    fn a_save_without_a_resume_point_prints_no_location() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(super::slot_file_path(dir.path(), 0), a_save().write()).unwrap();
        let slots = scan_save_dir(dir.path());
        assert!(slots[0].present);
        assert_eq!(slots[0].location, "");
        let scene_only = SaveResume {
            scene: "town01".into(),
            location: String::new(),
        };
        write_slot_save(dir.path(), 1, &a_save(), &scene_only).unwrap();
        assert_eq!(scan_save_dir(dir.path())[1].location, "town01");
    }

    /// A zeroed record has no displayed level and no XP: it prints as level
    /// 1 under the shared law, and a save with no records is not loadable.
    #[test]
    fn zeroed_record_prints_level_one_and_no_records_is_foreign() {
        let dir = tempfile::tempdir().unwrap();
        let sf = SaveFile {
            party: Party {
                members: vec![CharacterRecord::zeroed()],
            },
            ..SaveFile::default()
        };
        write_slot_save(dir.path(), 0, &sf, &SaveResume::default()).unwrap();
        write_slot_save(dir.path(), 1, &SaveFile::default(), &SaveResume::default()).unwrap();
        let slots = scan_save_dir(dir.path());
        assert_eq!(slots[0].party_lv, 1);
        assert_eq!(slots[1].content, SlotContent::Foreign);
    }
}

#[cfg(test)]
mod mounted_card_tests {
    use super::{MountedCard, card_port_blocks, card_port_rack};
    use legaia_engine_menus::save_screen::SLOT_GRID_CELLS;
    use legaia_engine_menus::save_select::SlotContent;
    use legaia_engine_session::title_continue::CARD_PORTS;
    use legaia_save::{CharacterRecord, HpMpSp, Party, SaveFile, SaveResume};

    /// A card image built in memory: the `MC` header, one block claimed for a
    /// Legaia save, and that block composed by the writer the engine's own
    /// card Save uses. Every other frame is left zero, so no other block
    /// reads as a chain start. Nothing here is disc- or card-sourced.
    fn synthetic_card_for_tests(block: u8, sf: &SaveFile, resume: &SaveResume) -> Vec<u8> {
        let mut card = vec![0u8; legaia_save::card::CARD_SIZE];
        card[..2].copy_from_slice(&legaia_save::card::CARD_MAGIC);
        let view = legaia_save::emu::detect(&card).expect("MC header makes it a raw card");
        view.claim_block(&mut card, block, "BASCUS-94254PRO-00")
            .expect("block is addressable");
        let sc = view
            .sc_block_mut(&mut card, block)
            .expect("block is addressable");
        sf.write_into_retail_sc_block(sc)
            .expect("compose the block");
        resume
            .write_into_retail_sc_block(sc)
            .expect("stamp the resume point");
        card
    }

    /// The save a synthetic card's block carries. Nothing here comes off a
    /// disc or a real card - the block is composed by the same writer the
    /// engine's own card Save uses.
    fn a_save() -> SaveFile {
        let mut r = CharacterRecord::zeroed();
        r.set_name("Noa");
        r.set_magic_rank(23);
        r.set_hp_mp_sp(HpMpSp {
            hp_cur: 210,
            hp_max: 240,
            mp_cur: 11,
            mp_max: 30,
            sp_cur: 0,
            sp_max: 0,
        });
        SaveFile {
            party: Party { members: vec![r] },
            ..SaveFile::default()
        }
    }

    fn a_resume() -> SaveResume {
        SaveResume {
            scene: "town01".into(),
            location: "Rim Elm".into(),
        }
    }

    fn mount(name: &str, block: u8) -> (tempfile::TempDir, MountedCard) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(name);
        std::fs::write(
            &path,
            synthetic_card_for_tests(block, &a_save(), &a_resume()),
        )
        .unwrap();
        let card = MountedCard::open(&path).unwrap();
        (dir, card)
    }

    /// Port 2 is the mounted card, and its pill carries the image's own name:
    /// the row has to say *which* card is in the port, the same way the
    /// browser's does.
    #[test]
    fn a_mounted_card_fills_port_two_with_its_own_name() {
        let (_dir, card) = mount("player.mcr", 3);
        let rack = card_port_rack([None, Some(&card)]);
        assert!(rack.is_card_ports());
        assert_eq!(rack.slots().len(), CARD_PORTS);
        assert!(rack.slots()[1].present, "port 2 holds the mounted card");
        assert_eq!(rack.slots()[1].label, "player.mcr");
    }

    /// Down on the pill row reaches port 2 through the flow and the session
    /// together, the way the window ticks them.
    #[test]
    fn down_on_the_pill_row_picks_the_mounted_card() {
        use legaia_engine_menus::save_screen::SaveScreenFlow;
        use legaia_engine_menus::save_select::{SaveSelectMode, SaveSelectSession, SelectInput};
        let (_dir, card) = mount("player.mcr", 3);
        let rack = card_port_rack([None, Some(&card)]);
        let mut session = SaveSelectSession::for_rack(SaveSelectMode::Load, &rack);
        let mut flow = SaveScreenFlow::new();
        for edge in [0x0040u16, 0] {
            let edge = flow.before_tick(&mut session, edge);
            session.tick(SelectInput::from_pad_edge(edge));
        }
        assert_eq!(session.current_slot(), 1);
    }

    /// The grid behind port 2 is the card's own fifteen blocks: grid cell `i`
    /// is card block `i + 1`, and only the claimed block reads as a save.
    #[test]
    fn the_card_grid_reads_the_claimed_block_and_nothing_else() {
        let (_dir, card) = mount("player.mcr", 3);
        let blocks = card_port_blocks([None, Some(&card)], 1);
        assert_eq!(blocks.len(), SLOT_GRID_CELLS as usize);
        // Block 3 is grid cell 2 - the off-by-one that makes the directory
        // block addressable would put this on cell 3.
        let cell = &blocks[2];
        assert!(cell.present);
        assert_eq!(cell.content, SlotContent::LegaiaSave);
        assert_eq!(cell.leader_name, "Noa");
        assert_eq!(cell.party_lv, 23);
        assert_eq!(cell.leader_hp, (210, 240));
        assert_eq!(cell.leader_mp, (11, 30));
        assert_eq!(cell.location, "Rim Elm");
        for (i, b) in blocks.iter().enumerate() {
            if i != 2 {
                assert!(!b.present, "cell {i} claims a save the card does not hold");
                assert_eq!(b.content, SlotContent::Free);
            }
        }
    }

    /// A file that is not a container `legaia_save::emu` recognises must
    /// **fail to mount** rather than mount as a blank card: an empty port and
    /// a wrong file are different mistakes and only one of them is silent.
    #[test]
    fn an_unrecognised_container_refuses_to_mount() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("not-a-card.bin");
        std::fs::write(&path, b"this is not a memory card").unwrap();
        // Not `unwrap_err`: a mounted card owns the container bytes, so the
        // `Debug` that would print would be the whole card image.
        let Err(err) = MountedCard::open(&path) else {
            panic!("a non-container file must not mount");
        };
        assert!(
            format!("{err:#}").contains("unrecognised save container"),
            "the mount failure must name the container problem: {err:#}"
        );
        assert!(MountedCard::open(&dir.path().join("absent.mcr")).is_err());
    }

    /// With nothing mounted, port 2 is exactly what it was before the port
    /// existed - an empty port previewing fifteen free cells, never port 1's.
    #[test]
    fn port_two_reads_empty_with_no_card_mounted() {
        let rack = card_port_rack([None, None]);
        assert!(!rack.slots()[1].present);
        let blocks = card_port_blocks([None, None], 1);
        assert_eq!(blocks.len(), SLOT_GRID_CELLS as usize);
        assert!(blocks.iter().all(|b| !b.present));
    }

    /// The container kinds `legaia_save::emu` normalises all address through
    /// the same view, so a DexDrive wrapper must read the same blocks as the
    /// raw card inside it - the wrapper offset is the thing a re-derived
    /// layout would miss.
    #[test]
    fn a_dexdrive_wrapper_reads_the_same_blocks_as_the_raw_card() {
        let raw = synthetic_card_for_tests(3, &a_save(), &a_resume());
        let mut gme = vec![0u8; legaia_save::emu::DEXDRIVE_HEADER_SIZE];
        gme[..legaia_save::emu::DEXDRIVE_MAGIC.len()]
            .copy_from_slice(legaia_save::emu::DEXDRIVE_MAGIC);
        gme.extend_from_slice(&raw);

        let dir = tempfile::tempdir().unwrap();
        let raw_path = dir.path().join("player.mcr");
        let gme_path = dir.path().join("player.gme");
        std::fs::write(&raw_path, &raw).unwrap();
        std::fs::write(&gme_path, &gme).unwrap();

        let from_raw = legaia_engine_menus::save_select::card_block_snapshots(
            &MountedCard::open(&raw_path).unwrap(),
        );
        let from_gme = legaia_engine_menus::save_select::card_block_snapshots(
            &MountedCard::open(&gme_path).unwrap(),
        );
        assert_eq!(from_raw, from_gme);
        assert!(from_gme[2].present);
    }
}
