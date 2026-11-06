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

#[cfg(test)]
mod tests {
    use super::selector_bands;

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
}
