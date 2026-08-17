//! Super Art damage-power edits. See [`crate::super_art_power`] for the pin: a
//! Super Art's per-strike power bytes live at `record+0x24` of its own
//! `0xD0`-stride art record in the character's player battle file `record0`
//! (PROT `0863`/`0864`/`0865`), addressed by the finisher action constant.

use super::*;

use legaia_art::queue::Character;

/// One applied Super Art power edit, for reporting.
#[derive(Debug, Clone)]
pub struct SuperArtPowerEditReport {
    pub character: Character,
    pub name: &'static str,
    pub finisher: u8,
    pub old_power: Vec<u8>,
    pub new_power: Vec<u8>,
}

/// Report of a `--super-art-power` batch.
#[derive(Debug, Clone, Default)]
pub struct SuperArtPowerReport {
    pub edits: Vec<SuperArtPowerEditReport>,
}

/// Apply a batch of `(super art, new power value)` edits. Each entry is a
/// `legaia_art::SuperArt` table row, so the character - and therefore the player
/// file to open - comes from the entry itself. Fails when a requested Super Art
/// could not be located on the disc at all (a wrong-disc / unrecognized-build
/// guard); a Super Art already at the requested value, or carrying no damage
/// byte, is reported as unchanged rather than as an error.
pub fn set_super_art_power(
    patcher: &mut DiscPatcher,
    edits: &[(&'static legaia_art::SuperArt, u8)],
) -> Result<SuperArtPowerReport> {
    let mut report = SuperArtPowerReport::default();
    if edits.is_empty() {
        return Ok(report);
    }
    let scus = legaia_iso::iso9660::read_file_in_image(patcher.image(), crate::arts::SCUS_NAME)
        .context("read SCUS_942.54 (arts-name table, needed to locate the art block)")?;

    for ch in Character::all() {
        let want: Vec<(u8, u8)> = edits
            .iter()
            .filter(|(s, _)| s.character == ch)
            .map(|(s, v)| (s.finisher, *v))
            .collect();
        if want.is_empty() {
            continue;
        }
        let index = crate::super_art_power::player_entry_index(ch);
        let entry = patcher
            .read_entry(index)
            .with_context(|| format!("read player file PROT {index}"))?;

        // Locate first, so a build whose records don't carry the expected names
        // is refused before anything is written.
        let located =
            crate::super_art_power::super_art_powers(&scus, &entry, ch).unwrap_or_default();
        for (s, _) in edits.iter().filter(|(s, _)| s.character == ch) {
            if !located.iter().any(|r| r.finisher == s.finisher) {
                anyhow::bail!(
                    "{:?}'s Super Art {} was not found in PROT {index} record0 \
                     (unrecognized build - nothing written)",
                    ch,
                    s.name
                );
            }
        }

        let Some((lzs_off, recompressed, applied)) =
            crate::super_art_power::patch_player_record0_super_power(&scus, &entry, ch, &want)
        else {
            continue; // every requested value already in place, or no damage byte
        };
        patcher
            .patch_prot_entry(index, lzs_off as u64, &recompressed)
            .with_context(|| format!("write player file PROT {index} record0 Super Art power"))?;
        for a in applied {
            // Re-key the applied edit back to its Super Art by record offset.
            let Some(row) = located.iter().find(|r| r.record_off == a.record_off) else {
                continue;
            };
            report.edits.push(SuperArtPowerEditReport {
                character: ch,
                name: row.name,
                finisher: row.finisher,
                old_power: a.old_power,
                new_power: a.new_power,
            });
        }
    }
    Ok(report)
}
