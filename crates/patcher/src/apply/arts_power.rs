//! Tactical-Art damage-power edits ("arts power-down"). See
//! [`crate::arts_power`] for the pin: each art's per-strike power bytes live at
//! `record+0x24` in the `0xD0`-stride art records of the character's player
//! battle file `record0` (PROT `0863`/`0864`/`0865`).

use super::*;

use legaia_art::queue::{Character, Command};

/// One applied art-power edit, for reporting.
#[derive(Debug, Clone)]
pub struct ArtPowerEditReport {
    pub character: Character,
    pub combo: Vec<Command>,
    pub old_power: Vec<u8>,
    pub new_power: Vec<u8>,
}

/// Report of an `--arts-power` batch.
#[derive(Debug, Clone, Default)]
pub struct ArtsPowerReport {
    pub edits: Vec<ArtPowerEditReport>,
}

/// Apply a batch of `(combo, new_power_value)` edits to the arts damage-power
/// bytes. Each combo is searched in **all three** player files; only the file
/// that owns the combo changes (a combo is unique to one character). Fails if
/// **no** player file carries a given combo (a typo / wrong-disc guard). The new
/// value is applied to every active power byte of the matched art, preserving the
/// hit count.
pub fn set_arts_power(
    patcher: &mut DiscPatcher,
    edits: &[(Vec<Command>, u8)],
) -> Result<ArtsPowerReport> {
    let mut report = ArtsPowerReport::default();
    // Track which requested combos matched somewhere, to flag typos.
    let mut matched = vec![false; edits.len()];

    for ch in Character::all() {
        let index = crate::arts_power::player_entry_index(ch);
        let entry = patcher
            .read_entry(index)
            .with_context(|| format!("read player file PROT {index}"))?;
        // Only pass the edits whose combo exists in this file.
        let dec = match crate::arts::player_record0_decoded(&entry) {
            Some(d) => d,
            None => continue,
        };
        let mut file_edits: Vec<(Vec<Command>, u8)> = Vec::new();
        for (ei, (combo, value)) in edits.iter().enumerate() {
            if !crate::arts_power::find_records_by_combo(&dec, combo).is_empty() {
                matched[ei] = true;
                file_edits.push((combo.clone(), *value));
            }
        }
        if file_edits.is_empty() {
            continue;
        }
        if let Some((lzs_off, recompressed, applied)) =
            crate::arts_power::patch_player_record0_power(&entry, &file_edits)
        {
            patcher
                .patch_prot_entry(index, lzs_off as u64, &recompressed)
                .with_context(|| format!("write player file PROT {index} record0 power"))?;
            for a in applied {
                report.edits.push(ArtPowerEditReport {
                    character: ch,
                    combo: a.combo,
                    old_power: a.old_power,
                    new_power: a.new_power,
                });
            }
        }
    }

    if let Some(i) = matched.iter().position(|m| !m) {
        let combo: String = edits[i]
            .0
            .iter()
            .map(crate::arts_power::command_glyph)
            .collect();
        anyhow::bail!("no Tactical Art has combo {combo} (nothing to power-edit)");
    }
    Ok(report)
}
