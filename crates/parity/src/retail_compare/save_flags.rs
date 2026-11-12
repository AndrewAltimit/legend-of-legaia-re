//! The system-flag bank of a lifted save: reading the raised ids and
//! raising or lowering them.

/// Offset of the system-flag bank (`0x80085758`) inside the story-flag
/// bitmap a save carries (`0x80085600`).
pub(super) const SYSTEM_FLAG_WINDOW: usize = 0x158;

/// Lower the system flags `ids` in a lifted save.
pub fn clear_system_flags(save: &mut legaia_save::SaveFile, ids: &[u16]) {
    for &idx in ids {
        if let Some(b) = save
            .ext
            .story_flag_bits
            .get_mut(SYSTEM_FLAG_WINDOW + usize::from(idx >> 3))
        {
            *b &= !(0x80u8 >> (idx & 7));
        }
    }
}

/// Raise or lower one system flag in a lifted save.
pub fn set_system_flag(save: &mut legaia_save::SaveFile, idx: u16, on: bool) {
    if let Some(b) = save
        .ext
        .story_flag_bits
        .get_mut(SYSTEM_FLAG_WINDOW + usize::from(idx >> 3))
    {
        let bit = 0x80u8 >> (idx & 7);
        if on {
            *b |= bit;
        } else {
            *b &= !bit;
        }
    }
}

/// Every raised bit of the save's system-flag bank, as the flag ids
/// `World::system_flag_set` takes (MSB-first within a byte).
pub fn system_flag_ids(save: &legaia_save::SaveFile) -> Vec<u16> {
    let bits = save
        .ext
        .story_flag_bits
        .get(SYSTEM_FLAG_WINDOW..)
        .unwrap_or(&[]);
    let mut out = Vec::new();
    for (b, &byte) in bits.iter().enumerate() {
        for k in 0..8 {
            if byte & (0x80 >> k) != 0 {
                out.push((b * 8 + k) as u16);
            }
        }
    }
    out
}

/// The one-hot region-selector bands in a scene's field bytecode: a run of
/// at least four `Clear`s over consecutive system flags followed by the `Set`
/// of one of them. Every field scene's entry loop keeps one (`0x527..0x52E`,
/// the region-type band `0x19B..0x1AA`; `docs/subsystems/script-vm.md`):
/// each pass clears the band and raises the bit of the region the player stands in, so the
/// bit records where the loop's last pass ran.
pub fn selector_bands(bytecode: &[u8]) -> Vec<std::ops::RangeInclusive<u16>> {
    use legaia_scene_formats::field_disasm::{FlagKind, InsnInfo, LinearWalker};
    let mut bands: Vec<std::ops::RangeInclusive<u16>> = Vec::new();
    let mut run: Option<(u16, u16)> = None;
    for insn in LinearWalker::new(bytecode, 0).flatten() {
        let flag = match insn.info {
            InsnInfo::SystemFlag { kind, idx, .. } => Some((kind, idx)),
            _ => None,
        };
        match (flag, run) {
            (Some((FlagKind::Clear, idx)), Some((lo, hi))) if idx == hi.wrapping_add(1) => {
                run = Some((lo, idx));
                continue;
            }
            (Some((FlagKind::Set, idx)), Some((lo, hi)))
                if hi - lo >= 3 && (lo..=hi).contains(&idx) && !bands.contains(&(lo..=hi)) =>
            {
                bands.push(lo..=hi);
            }
            _ => {}
        }
        run = match flag {
            Some((FlagKind::Clear, idx)) => Some((idx, idx)),
            _ => None,
        };
    }
    bands
}

/// Land the save's own bits over every [`selector_bands`] band.
///
/// A seed's settle runs the entry loop with the player on the seat, and its
/// passes re-select the band there. Where a record took the player out of the
/// region the loop last selected, that pass is one retail never ran: the
/// system SM runs no pass while a record holds the player, so retail's bit
/// still names the region the player left. `nivora_cutscene_delilas_intro`
/// holds `0x52C` (`nilboa` `P1[0]`'s box `[26, 0 .. 126, 87]`) with the
/// cutscene `P2[3]` holding the player on tile `(112, 114)`, in `0x52B`'s box
/// `[16, 88 .. 126, 126]`. The settle is seeding, not game time, so the band
/// is the save's again once it has run, as the script counters are
/// ([`super::hold_slot_table`]).
pub fn hold_selector_bands(
    world: &mut legaia_engine_core::world::World,
    save: &legaia_save::SaveFile,
) {
    let raised = system_flag_ids(save);
    for band in selector_bands(&world.field_bytecode) {
        for idx in band {
            if raised.contains(&idx) {
                world.system_flag_set(idx);
            } else {
                world.system_flag_clear(idx);
            }
        }
    }
}

/// The visible-tile window each selector arm applies: for every band bit a
/// [`selector_bands`] arm raises, the long-form op `0x46` (`46 24 x0 z0 x1
/// z1`) that follows its `Set` before the next band clear. `jouind` `P1[0]`'s
/// region-type arms end `0x19E` on `(-10, -6, 10, 13)` and `0x19F` on
/// `(-7, -7, 8, 39)`.
pub fn selector_arm_windows(bytecode: &[u8]) -> Vec<(u16, [i8; 4])> {
    use legaia_scene_formats::field_disasm::{FlagKind, InsnInfo, LinearWalker};
    let bands = selector_bands(bytecode);
    let in_band = |idx: u16| bands.iter().any(|b| b.contains(&idx));
    let mut out: Vec<(u16, [i8; 4])> = Vec::new();
    let mut armed: Option<u16> = None;
    for insn in LinearWalker::new(bytecode, 0).flatten() {
        match insn.info {
            InsnInfo::SystemFlag { kind, idx, .. } if in_band(idx) => {
                armed = match kind {
                    FlagKind::Set => Some(idx),
                    _ => None,
                };
            }
            InsnInfo::ViewWindow {
                long: true, bytes, ..
            } => {
                if let Some(bit) = armed.take()
                    && !out.iter().any(|(b, _)| *b == bit)
                {
                    out.push((
                        bit,
                        [bytes[0], bytes[1], bytes[2], bytes[3]].map(|v| v as i8),
                    ));
                }
            }
            _ => {}
        }
    }
    out
}

/// The window of the selector arm the save's band names, where the settle
/// took a different arm. The arm retail ran last ended on its own op `0x46`,
/// and a record holding the player since has kept the system SM
/// (`FUN_801DA51C`) from running another pass, so the window is that arm's
/// as much as the bit is. `before` is the engine's raised band bits ahead of
/// [`hold_selector_bands`].
pub fn held_selector_window(
    world: &legaia_engine_core::world::World,
    save: &legaia_save::SaveFile,
    before: &[u16],
) -> Option<[i8; 4]> {
    let raised = system_flag_ids(save);
    let windows = selector_arm_windows(&world.field_bytecode);
    windows
        .iter()
        .find(|(bit, _)| raised.contains(bit) && !before.contains(bit))
        .map(|&(_, w)| w)
}

/// The engine's raised bits over every [`selector_bands`] band.
pub fn raised_band_bits(world: &legaia_engine_core::world::World) -> Vec<u16> {
    selector_bands(&world.field_bytecode)
        .into_iter()
        .flatten()
        .filter(|&idx| world.system_flag_test(idx))
        .collect()
}

/// What an entry record's straight-line head raises on every load: the
/// system flags it sets (`50`..`57`) and whether it raises the ambient
/// particle gate (`4C 30`), before its first branch, wait or halt.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EntryHeadRaises {
    pub flags: Vec<u16>,
    pub particle_gate: bool,
    /// One past the head's last byte.
    pub end: usize,
}

/// The straight-line head of the entry record at `pc0`: every instruction
/// the install slice runs unconditionally on a load.
pub fn entry_head_raises(bytecode: &[u8], pc0: usize) -> EntryHeadRaises {
    use legaia_scene_formats::field_disasm::{FlagKind, InsnInfo, LinearWalker, MenuCtrlKind};
    let mut out = EntryHeadRaises {
        end: pc0,
        ..Default::default()
    };
    for insn in LinearWalker::new(bytecode, pc0) {
        let Ok(insn) = insn else { break };
        let flow = matches!(
            insn.info,
            InsnInfo::CondJmp { .. }
                | InsnInfo::JmpRel { .. }
                | InsnInfo::BBoxTest { .. }
                | InsnInfo::InventoryCmp { .. }
                | InsnInfo::Picker { .. }
                | InsnInfo::Yield { .. }
                | InsnInfo::WaitFrames { .. }
                | InsnInfo::SceneChange { .. }
                | InsnInfo::WarpOrInteract { .. }
                | InsnInfo::LFlag {
                    kind: FlagKind::Test,
                    ..
                }
                | InsnInfo::GFlag {
                    kind: FlagKind::Test,
                    ..
                }
                | InsnInfo::CFlag {
                    kind: FlagKind::Test,
                    ..
                }
                | InsnInfo::SystemFlag {
                    kind: FlagKind::Test,
                    ..
                }
        ) || insn.opcode == 0x21;
        if flow {
            break;
        }
        out.end = insn.pc + insn.size;
        if insn.extended.is_some() {
            continue;
        }
        match insn.info {
            InsnInfo::SystemFlag {
                kind: FlagKind::Set,
                idx,
                ..
            } if !out.flags.contains(&idx) => out.flags.push(idx),
            InsnInfo::MenuCtrl {
                op0: 0x30,
                kind: MenuCtrlKind::Nibble3 { sub: 0 },
            } => out.particle_gate = true,
            _ => {}
        }
    }
    out
}

/// The writes outside `head` that undo it: the system flags a `Clear`
/// elsewhere in the scene's bytecode lowers, and whether a `4C 31` lowers
/// the particle gate.
fn undoers(bytecode: &[u8], head: &EntryHeadRaises, pc0: usize) -> (Vec<u16>, bool) {
    use legaia_scene_formats::field_disasm::{FlagKind, InsnInfo, LinearWalker, MenuCtrlKind};
    let mut flags = Vec::new();
    let mut gate = false;
    for insn in LinearWalker::new(bytecode, 0).flatten() {
        if (pc0..head.end).contains(&insn.pc) {
            continue;
        }
        match insn.info {
            InsnInfo::SystemFlag {
                kind: FlagKind::Clear,
                idx,
                ..
            } if head.flags.contains(&idx) && !flags.contains(&idx) => flags.push(idx),
            InsnInfo::MenuCtrl {
                op0: 0x31,
                kind: MenuCtrlKind::Nibble3 { sub: 1 },
            } => gate = true,
            _ => {}
        }
    }
    (flags, gate)
}

/// Lower what the entry head raised where retail's state shows a later beat
/// of the same visit undid it.
///
/// A card load runs the entry record's straight-line head, so every flag and
/// the particle gate it raises are up after the seed's settle. A capture
/// taken later in the visit holds them down when a beat of the scene's own
/// script lowered them since: `rikuroa` `P1[0]` raises the gate and flag
/// `0x01F` (`4C 30`, `50 1F` at `+0x47`), and the Genesis Tree beat `P2[54]`
/// lowers both (`4C 31`, `60 1F` at `+0x1FC`). The save carries the flag
/// down and the gate word `0x8007B854` sits outside the save window, so the
/// beat is history a card load cannot replay. Only a raise the scene's
/// bytecode can undo elsewhere is lowered, and only where retail holds it
/// down; returns what was lowered.
pub fn hold_undone_entry_raises(
    world: &mut legaia_engine_core::world::World,
    bytecode: &[u8],
    pc0: usize,
    save: &legaia_save::SaveFile,
    retail_gate: bool,
) -> (Vec<u16>, bool) {
    let head = entry_head_raises(bytecode, pc0);
    let (clears, gate_clear) = undoers(bytecode, &head, pc0);
    let raised = system_flag_ids(save);
    let lowered: Vec<u16> = clears
        .into_iter()
        .filter(|idx| !raised.contains(idx) && world.system_flag_test(*idx))
        .collect();
    for &idx in &lowered {
        world.system_flag_clear(idx);
    }
    let gate = head.particle_gate && gate_clear && !retail_gate && world.fog.gate;
    if gate {
        world.set_ambient_particles_enabled(false);
    }
    (lowered, gate)
}

#[cfg(test)]
mod tests {
    use super::{entry_head_raises, selector_arm_windows, selector_bands, undoers};

    #[test]
    fn the_head_stops_at_the_first_branch() {
        // `rikuroa` `P1[0]`'s shape: `4C C1`, `4C 30`, `50 1F`, then a
        // `0x42` branch, and a set behind it that is not the head's.
        let bc = [
            0x4C, 0xC1, 0x4C, 0x30, 0x50, 0x1F, 0x42, 0x00, 0x00, 0x04, 0x00, 0x50, 0x20,
        ];
        let head = entry_head_raises(&bc, 0);
        assert_eq!(head.flags, vec![0x1F]);
        assert!(head.particle_gate);
        assert_eq!(head.end, 6);
    }

    #[test]
    fn only_a_writer_outside_the_head_undoes_it() {
        // Head raises `0x1F` and the gate; a later record lowers both.
        let mut bc = vec![0x4C, 0x30, 0x50, 0x1F, 0x21];
        let head = entry_head_raises(&bc, 0);
        assert_eq!(undoers(&bc, &head, 0), (vec![], false));
        bc.extend([0x4C, 0x31, 0x60, 0x1F, 0x21]);
        assert_eq!(undoers(&bc, &head, 0), (vec![0x1F], true));
    }

    #[test]
    fn a_run_of_clears_and_one_set_is_a_band() {
        // `nilboa` `P1[0]`'s arm: clear `0x527..=0x52E` (`65 27` .. `65 2E`),
        // set `0x52B`; then a lone clear and a set outside any run.
        let mut bc: Vec<u8> = (0x27..=0x2E).flat_map(|i| [0x65, i]).collect();
        bc.extend([0x55, 0x2B, 0x65, 0x10, 0x55, 0x10]);
        assert_eq!(selector_bands(&bc), vec![0x527..=0x52E]);
    }

    #[test]
    fn a_short_run_is_not_a_band() {
        let bc = [0x65, 0x27, 0x65, 0x28, 0x65, 0x29, 0x55, 0x28];
        assert!(selector_bands(&bc).is_empty());
    }

    #[test]
    fn each_arm_keeps_the_window_after_its_set() {
        // Two arms of a four-bit band: clear `0x19C..=0x19F`, set one, then
        // the arm's long-form op `0x46`.
        let clears: Vec<u8> = (0x9C..=0x9F).flat_map(|i| [0x61, i]).collect();
        let mut bc = clears.clone();
        bc.extend([0x51, 0x9E, 0x46, 0x24, 0xF6, 0xFA, 0x0A, 0x0D]);
        bc.extend(clears);
        bc.extend([0x51, 0x9F, 0x46, 0x24, 0xF9, 0xF9, 0x08, 0x27]);
        let w = selector_arm_windows(&bc);
        assert_eq!(
            w,
            vec![(0x19E, [-10, -6, 10, 13]), (0x19F, [-7, -7, 8, 39])]
        );
    }
}
