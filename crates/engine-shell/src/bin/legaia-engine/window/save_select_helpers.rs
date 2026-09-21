//! Extracted from `window.rs` (mechanical split; behavior-preserving).
//!
//! Save-select / load-screen helpers: the save-directory scanner, the
//! per-slot info-view builder, the info-panel slide-in offset, and the
//! owned-string flavour of the renderer's `SlotInfoView`.

use super::*;

/// Walk `save_dir` and build per-slot `SlotSnapshot` entries from any
/// LGSF v1 / v2 files found there. Empty slots produce
/// `SlotSnapshot::empty(slot)`. Up to 8 slots are scanned (the retail
/// PSX memory card supports 15 blocks; engines wishing to scan more can
/// drive their own scanner and feed the result into `SaveSelectSession`).
/// Pluck the lead-character roster index out of a [`SlotSnapshot`] for
/// the load-screen portrait grid. The snapshot already exposes the
/// leader's char_id (scan_save_dir picks it from the parsed
/// [`legaia_save::SaveFile`]); this thin helper exists so render-time
/// call sites read clearly.
pub(crate) fn slot_leader_char_id(snap: &legaia_engine_core::save_select::SlotSnapshot) -> u8 {
    snap.leader_char_id
}

/// Build a per-frame [`legaia_engine_render::SlotInfoView`] for the
/// info panel shown at the bottom of the slot-preview screen.
/// Returns `None` for empty slots (the info panel renders only when
/// a save is present).
pub(crate) fn build_slot_info_view(
    slots: &[legaia_engine_core::save_select::SlotSnapshot],
    cursor_slot: u8,
) -> Option<SlotInfoOwned> {
    let snap = slots.get(cursor_slot as usize)?;
    if !snap.present {
        return None;
    }
    Some(SlotInfoOwned {
        slot_no: snap.slot.saturating_add(1),
        location: snap.location.clone(),
        play_time: snap.play_time_string(),
        leader_name: snap.leader_name.clone(),
        leader_level: snap.party_lv,
        leader_hp: snap.leader_hp,
        leader_mp: snap.leader_mp,
        leader_char_id: snap.leader_char_id,
    })
}

/// Compute the slide-in y-offset (delta from parked y) for the
/// bottom info panel. Mirrors retail FUN_801E08D8's inline
/// `local_34 = (anim_t * -0x100) / 0xFFF >> 12 + 0x18A`: the panel
/// slides from `INFO_PANEL_OFFSCREEN_Y = 394` (off-screen below) up
/// to `INFO_PANEL_PARKED_Y = 138` (parked under load chrome) as
/// `info_panel_slide_anim_t` ramps 0 → 4096. Returns the delta from
/// parked y, so 0 = fully landed.
pub(crate) fn info_panel_slide_offset(
    session: &legaia_engine_core::save_select::SaveSelectSession,
) -> i32 {
    let (_, y) = legaia_engine_core::save_select::interpolate_anim(
        (0, legaia_engine_core::save_select::INFO_PANEL_OFFSCREEN_Y),
        (0, legaia_engine_core::save_select::INFO_PANEL_PARKED_Y),
        session.info_panel_slide_anim_t(),
    );
    y - legaia_engine_core::save_select::INFO_PANEL_PARKED_Y
}

/// Retail title word for a save-select screen. The header tab is one
/// panel whose string toggles on the retail direction flag
/// `_DAT_801f0200` (`0` = the Save path, the branch that stamps a
/// product code into the chosen block; non-zero = Load) - mode 1 of
/// the slide-in primitive `FUN_801E1C1C`. The session's
/// [`SaveSelectMode`] carries the same bit, so the word must come from
/// it rather than from which host screen opened the session: the
/// field menu's Load row builds the same sub-session as its Save row,
/// only the mode differs.
pub(crate) fn save_select_title_word(
    session: &legaia_engine_core::save_select::SaveSelectSession,
) -> &'static str {
    match session.mode() {
        legaia_engine_core::save_select::SaveSelectMode::Load => "Load",
        legaia_engine_core::save_select::SaveSelectMode::Save => "Save",
    }
}

/// Live y of the confirm messagebox: retail mode 3 of `FUN_801E1C1C`
/// slides it up from below the stage, `(160, 344) -> (160, 88)`,
/// against the same 12-bit timer family as the info panel.
pub(crate) fn confirm_dialog_slide_y(
    session: &legaia_engine_core::save_select::SaveSelectSession,
) -> i32 {
    legaia_engine_core::save_select::interpolate_anim(
        (0, legaia_engine_render::CONFIRM_DIALOG_SLIDE_START_Y),
        (0, legaia_engine_render::CONFIRM_DIALOG_SLIDE_TARGET_Y),
        session.info_panel_slide_anim_t(),
    )
    .1
}

/// Phase-dependent text overlays for a save-select session: the
/// "Now checking" dialog lines, the slot-preview info panel text (or
/// its "Able to save." / "No data" / "Not a Legend of Legaia save."
/// caption), and the confirm messagebox ("Do you wish to save?") with
/// its stacked Yes / No rows. Shared by the boot Continue → Load
/// screen and the field-menu Load / Save sub-screens so the two paths
/// cannot drift; the sprite half of the same overlays is emitted by
/// `save_select_chrome_sprite_draws`.
/// `chrome_present` = the system-UI atlas is resident, so the sprite
/// pass draws retail's LV / HP / MP label sprites and the text pass
/// must skip its ASCII stand-ins.
pub(crate) fn save_select_phase_text_draws(
    font: &legaia_font::Font,
    session: &legaia_engine_core::save_select::SaveSelectSession,
    flow: &legaia_engine_core::save_screen::SaveScreenFlow,
    stage_origin: (i32, i32),
    stage_scale: u32,
    chrome_present: bool,
) -> Vec<TextDraw> {
    use legaia_engine_core::save_select::{SelectPhase, SlotInfoMode};
    let mut out = Vec::new();
    let layout = legaia_engine_core::save_select::phase_layout(session.phase());
    match session.phase() {
        SelectPhase::NowChecking { .. } => {
            // Retail slide: dialog x slides from
            // NOW_CHECKING_SLIDE_START_X (416) to
            // NOW_CHECKING_SLIDE_TARGET_X (160) over 16 frames.
            let pos_x = legaia_engine_core::save_select::interpolate_anim(
                (legaia_engine_render::NOW_CHECKING_SLIDE_START_X, 0),
                (legaia_engine_render::NOW_CHECKING_SLIDE_TARGET_X, 0),
                session.slide_anim_t(),
            )
            .0;
            let slide_offset = (pos_x - legaia_engine_render::NOW_CHECKING_SLIDE_TARGET_X, 0);
            out.extend(legaia_engine_render::now_checking_text_draws_for(
                font,
                stage_origin,
                stage_scale,
                slide_offset,
            ));
        }
        // Every phase the shared layout marks as a preview draws the info
        // panel - which includes the two confirms: retail raises the
        // overwrite / delete prompt FROM the preview, not from the pill row
        // (`docs/subsystems/save-screen.md`), so the block grid and its
        // caption stay under the messagebox. This window used to drop both.
        _ if layout.preview => {
            // The grid previews the picked PORT's blocks, not the pill row -
            // the two are different lists in the two-stage rack, and reading
            // the pills here captioned the info panel with the card instead
            // of the save.
            let (blocks, cell) = flow.preview(session);
            let info = build_slot_info_view(blocks, cell);
            let view = info.as_ref().map(|i| i.as_view());
            let panel_y_offset = info_panel_slide_offset(session);
            out.extend(legaia_engine_render::slot_info_panel_text_draws_for(
                font,
                view.as_ref(),
                panel_y_offset,
                stage_origin,
                stage_scale,
                chrome_present,
            ));
            // Nothing loadable here: retail captions the panel rather
            // than leaving it empty.
            if view.is_none()
                && let Some(snap) = blocks.get(cell as usize)
                && let Some(caption) =
                    SlotInfoMode::for_grid_cell(cell, snap).caption(session.mode())
            {
                out.extend(legaia_engine_render::slot_info_caption_draws_for(
                    font,
                    caption,
                    panel_y_offset,
                    stage_origin,
                    stage_scale,
                ));
            }
        }
        _ => {}
    }
    // The confirm prompt is retail's centred messagebox (mode 3 of the
    // slide-in primitive), NOT an inline row under the pills: a near-full
    // width prompt bar + a small box with the Yes / No rows stacked, sliding
    // up from below the stage. It rides ON TOP of the preview drawn above.
    if layout.confirm {
        let (prompt, cursor) = match session.phase() {
            SelectPhase::ConfirmOverwrite { cursor, .. } => ("Do you wish to save?", cursor),
            SelectPhase::ConfirmDelete { cursor, .. } => ("Delete this save?", cursor),
            _ => ("", 0),
        };
        out.extend(legaia_engine_render::confirm_dialog_text_draws_for(
            font,
            prompt,
            cursor,
            confirm_dialog_slide_y(session),
            stage_origin,
            stage_scale,
        ));
    }
    out
}

/// Owned-string flavour of [`legaia_engine_render::SlotInfoView`] used
/// to keep the strings alive across the render call. The borrowed
/// view referenced by the renderer is taken via [`Self::as_view`].
pub(crate) struct SlotInfoOwned {
    slot_no: u8,
    location: String,
    play_time: String,
    leader_name: String,
    leader_level: u8,
    leader_hp: (u16, u16),
    leader_mp: (u16, u16),
    leader_char_id: u8,
}

impl SlotInfoOwned {
    pub(crate) fn as_view(&self) -> legaia_engine_render::SlotInfoView<'_> {
        legaia_engine_render::SlotInfoView {
            slot_no: self.slot_no,
            location: &self.location,
            play_time: &self.play_time,
            leader_name: &self.leader_name,
            leader_level: self.leader_level,
            leader_hp: self.leader_hp,
            leader_mp: self.leader_mp,
            leader_char_id: self.leader_char_id,
        }
    }
}

/// Memory-card ports the console has, and so the pill rows this shell draws.
/// Retail's save screen is `SLOT 1` / `SLOT 2` and nothing else; the sprite
/// chrome has always clamped to two, which is what made a 15-entry pill row
/// draw fifteen text rows under two pills.
pub(crate) const CARD_PORTS: u8 = 2;

/// The mounted-card type, shared with the browser rack.
///
/// Both hosts hold the same struct now: it caches the detected
/// [`legaia_save::emu::CardView`], carries the dirty bit a host that writes
/// needs, and answers `save_at` / `block_is_save_start` / `dir_frame` off its
/// own bytes. The two hosts used to carry near-identical copies that drifted.
pub(crate) use legaia_save::emu::MountedCard;

/// The native shell's save rack: retail's two console ports, with the
/// engine's own save directory standing in for the card in **port 1**.
///
/// The shell's saves are LGSF files rather than SC blocks on a real card, but
/// the screen around them is retail's: two pills, a card-read beat, then the
/// 5x3 block grid of `slot_00 … slot_14`. Modelling the directory as the
/// mounted card is what lets that screen be the same screen the browser
/// draws - one [`SaveRack`] kind, one
/// [`legaia_engine_core::save_screen::SaveScreenFlow`], no per-host flag.
/// Port 2 is empty here; [`disk_save_rack_with_card`] is the same rack with a
/// real card image in it.
/// The windowed host mounts a card in port 2 whenever `--card` names one, so
/// this unmounted form is what the unit tests below stand on.
#[cfg(test)]
pub(crate) fn disk_save_rack(save_dir: &Path) -> legaia_engine_core::save_select::SaveRack {
    disk_save_rack_with_card(save_dir, None)
}

/// [`disk_save_rack`] with a [`MountedCard`] in **port 2**.
///
/// Both pills are built through
/// [`legaia_engine_core::save_screen::card_port_snapshot`], the one place a
/// port's pill fields are decided, so a mounted card carries its own name
/// into the row exactly as the browser's imported `.mcr` does.
pub(crate) fn disk_save_rack_with_card(
    save_dir: &Path,
    card: Option<&MountedCard>,
) -> legaia_engine_core::save_select::SaveRack {
    use legaia_engine_core::save_screen::card_port_snapshot;
    let dir_label = save_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("SAVE DATA");
    let mounted = [Some(dir_label), card.map(|c| c.label.as_str())];
    legaia_engine_core::save_select::SaveRack::CardPorts(
        (0..CARD_PORTS)
            .map(|port| card_port_snapshot(port, mounted.get(port as usize).copied().flatten()))
            .collect(),
    )
}

/// The fifteen blocks behind a port of [`disk_save_rack`] - the answer to
/// [`legaia_engine_core::save_screen::SaveScreenFlow::pending_read`]. Port 1
/// is the save directory; every other port is unmounted and reads empty.
#[cfg(test)]
pub(crate) fn disk_port_blocks(
    save_dir: &Path,
    port: u8,
) -> Vec<legaia_engine_core::save_select::SlotSnapshot> {
    disk_port_blocks_with_card(save_dir, None, port)
}

/// [`disk_port_blocks`] for a rack built by [`disk_save_rack_with_card`]:
/// port 1 is the save directory, port 2 the mounted card's own fifteen
/// blocks. A port with nothing in it still reads fifteen free cells rather
/// than the other port's - the grid previews the port the player picked.
pub(crate) fn disk_port_blocks_with_card(
    save_dir: &Path,
    card: Option<&MountedCard>,
    port: u8,
) -> Vec<legaia_engine_core::save_select::SlotSnapshot> {
    use legaia_engine_core::save_screen::SLOT_GRID_CELLS;
    use legaia_engine_core::save_select::SlotSnapshot;
    match (port, card) {
        (0, _) => scan_save_dir(save_dir),
        (1, Some(card)) => legaia_engine_core::save_select::card_block_snapshots(card),
        _ => (0..SLOT_GRID_CELLS).map(SlotSnapshot::empty).collect(),
    }
}

/// Path of `slot`'s LGSF file under `save_dir` - the shape
/// `MenuRuntime::slot_path` writes (`slot_NN.<SAVE_EXT>`, zero-padded).
pub(crate) fn slot_file_path(save_dir: &Path, slot: u8) -> std::path::PathBuf {
    use legaia_engine_core::menu_runtime::SAVE_EXT;
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

/// Write `sf` + its resume point to `slot`'s LGSF file, creating `save_dir`.
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

/// The [`SlotSnapshot`](legaia_engine_core::save_select::SlotSnapshot) a
/// parsed save presents on the load screen: the lead record's own name /
/// level / HP / MP through [`legaia_save::SaveFile::leader_summary`] - the
/// one derivation the browser's card rack uses too - and the resume point's
/// location row. A save with no party records is not loadable and reads as
/// foreign.
pub(crate) fn snapshot_for_save(
    slot: u8,
    sf: &legaia_save::SaveFile,
    resume: &legaia_save::SaveResume,
) -> legaia_engine_core::save_select::SlotSnapshot {
    use legaia_engine_core::save_select::{SlotContent, SlotSnapshot};
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

pub(crate) fn scan_save_dir(save_dir: &Path) -> Vec<legaia_engine_core::save_select::SlotSnapshot> {
    use legaia_engine_core::save_select::SlotSnapshot;
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
mod title_word_tests {
    use super::save_select_title_word;
    use legaia_engine_core::save_select::{SaveSelectMode, SaveSelectSession, SlotSnapshot};

    /// The header word must follow the session's MODE - the field
    /// menu's Load row builds the same `FieldMenuSubsession::Save`
    /// shape as its Save row, and a hardcoded "Save" at the draw site
    /// is exactly the bug that put the Save title on the in-game Load
    /// screen.
    #[test]
    fn title_word_follows_session_mode_not_host_screen() {
        let slots = vec![SlotSnapshot::empty(0)];
        let load = SaveSelectSession::new(SaveSelectMode::Load, slots.clone());
        let save = SaveSelectSession::new(SaveSelectMode::Save, slots);
        assert_eq!(save_select_title_word(&load), "Load");
        assert_eq!(save_select_title_word(&save), "Save");
    }
}

#[cfg(test)]
mod save_scan_tests {
    use super::scan_save_dir;
    use legaia_engine_core::menu_runtime::SAVE_EXT;
    use legaia_engine_core::save_select::{SaveSelectMode, SlotContent, SlotInfoMode};
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
    use super::{CARD_PORTS, disk_port_blocks, disk_save_rack, scan_save_dir};
    use legaia_engine_core::menu_runtime::SAVE_EXT;
    use legaia_engine_core::save_screen::{
        SLOT_GRID_CELLS, SaveCommit, SaveCommitKind, SaveScreenFlow,
    };
    use legaia_engine_core::save_select::{
        SaveRack, SaveSelectMode, SaveSelectSession, SelectInput, SelectPhase,
    };
    use legaia_save::{CharacterRecord, Party, SaveFile};

    fn seeded_dir(slots: &[u8]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let bytes = SaveFile {
            party: Party {
                members: vec![CharacterRecord::zeroed()],
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
    #[test]
    fn the_rack_is_two_ports_with_the_save_dir_in_port_one() {
        let dir = seeded_dir(&[0]);
        let rack = disk_save_rack(dir.path());
        assert!(
            rack.is_card_ports(),
            "the shell runs retail's two-stage flow"
        );
        assert_eq!(rack.slots().len(), CARD_PORTS as usize);
        assert!(rack.slots()[0].present, "port 1 is the save directory");
        assert!(!rack.slots()[1].present, "port 2 holds no card");
    }

    /// The grid behind port 1 is the save directory; every other port is
    /// unmounted and previews as fifteen free cells rather than as port 1's.
    #[test]
    fn port_one_reads_the_save_dir_and_port_two_reads_empty() {
        let dir = seeded_dir(&[2]);
        let blocks = disk_port_blocks(dir.path(), 0);
        assert_eq!(blocks, scan_save_dir(dir.path()));
        assert!(blocks[2].present);
        let empty = disk_port_blocks(dir.path(), 1);
        assert_eq!(empty.len(), SLOT_GRID_CELLS as usize);
        assert!(empty.iter().all(|b| !b.present));
    }

    /// The whole two-stage walk, end to end: pick the port off the pills,
    /// cross the card-read beat, walk the block grid, confirm. What comes
    /// out must name the block the player was pointing at - the pre-rack
    /// code committed the *pill* slot, which in a two-port rack is only
    /// ever 0 or 1.
    #[test]
    fn picking_a_grid_cell_commits_that_slot_not_the_port() {
        let dir = seeded_dir(&[0, 3]);
        let mut session =
            SaveSelectSession::for_rack(SaveSelectMode::Load, &disk_save_rack(dir.path()));
        let mut flow = SaveScreenFlow::new();
        let step = |session: &mut SaveSelectSession, flow: &mut SaveScreenFlow, edge: u16| {
            if let Some(port) = flow.pending_read(session) {
                flow.install_blocks(port, disk_port_blocks(dir.path(), port));
            }
            let edge = flow.before_tick(session, edge);
            session.tick(SelectInput::from_pad_edge(edge));
        };
        // Confirm port 1, then run the "Now checking" beat out.
        step(
            &mut session,
            &mut flow,
            legaia_engine_core::input::PadButton::Cross.mask(),
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
            legaia_engine_core::input::PadButton::Right.mask()
                | legaia_engine_core::input::PadButton::Cross.mask(),
        );
        assert!(
            matches!(session.phase(), SelectPhase::SlotPreview { .. }),
            "cell 1 is empty - retail refuses the load rather than failing it"
        );
        for _ in 0..2 {
            step(
                &mut session,
                &mut flow,
                legaia_engine_core::input::PadButton::Right.mask(),
            );
        }
        step(
            &mut session,
            &mut flow,
            legaia_engine_core::input::PadButton::Cross.mask(),
        );
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
        assert_ne!(disk_save_rack(dir.path()).slots().len(), flat.slots().len());
    }
}

#[cfg(test)]
mod slot_model_tests {
    use super::{read_slot_save, scan_save_dir, write_slot_save};
    use legaia_engine_core::save_select::SlotContent;
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
    use super::{CARD_PORTS, MountedCard, disk_port_blocks_with_card, disk_save_rack_with_card};
    use legaia_engine_core::save_screen::SLOT_GRID_CELLS;
    use legaia_engine_core::save_select::SlotContent;
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
        let (dir, card) = mount("player.mcr", 3);
        let rack = disk_save_rack_with_card(dir.path(), Some(&card));
        assert!(rack.is_card_ports());
        assert_eq!(rack.slots().len(), CARD_PORTS as usize);
        assert!(rack.slots()[1].present, "port 2 holds the mounted card");
        assert_eq!(rack.slots()[1].label, "player.mcr");
    }

    /// The grid behind port 2 is the card's own fifteen blocks: grid cell `i`
    /// is card block `i + 1`, and only the claimed block reads as a save.
    #[test]
    fn the_card_grid_reads_the_claimed_block_and_nothing_else() {
        let (dir, card) = mount("player.mcr", 3);
        let blocks = disk_port_blocks_with_card(dir.path(), Some(&card), 1);
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
        let dir = tempfile::tempdir().unwrap();
        let rack = disk_save_rack_with_card(dir.path(), None);
        assert!(!rack.slots()[1].present);
        let blocks = disk_port_blocks_with_card(dir.path(), None, 1);
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

        let from_raw = legaia_engine_core::save_select::card_block_snapshots(
            &MountedCard::open(&raw_path).unwrap(),
        );
        let from_gme = legaia_engine_core::save_select::card_block_snapshots(
            &MountedCard::open(&gme_path).unwrap(),
        );
        assert_eq!(from_raw, from_gme);
        assert!(from_gme[2].present);
    }
}
