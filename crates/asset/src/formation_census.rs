//! Disc-wide **formation census**: for every monster id, how many MAN
//! formation rows carry it, split by the row's scripted-fight header byte and
//! by whether a random-encounter region can roll the row.
//!
//! The battle engine has no per-monster "boss" flag. What marks a fight as
//! scripted is the formation row's header byte `record[+0]`: the entity SM's
//! confirm state raises bit `0x80` of `DAT_8007BD60` when it is non-zero
//! (`FUN_801DA51C` at `0x801DA5F8..0x801DA61C`), the battle-setup routine
//! latches that bit into `ctx[+0x287]`, and that byte then selects the enemy
//! stat-boost profile (`FUN_80054CB0`), gates the Seru-magic side-effect
//! stager (`FUN_801F3D3C`), and blocks the escape roll. So "which fights an
//! enemy is met in" is a property of the rows that name it, and this census
//! reads exactly those rows off the disc - every scene MAN, whether stored
//! LZS-compressed inside a `scene_asset_table` bundle or raw as a type-`0x03`
//! DATA_FIELD streaming chunk.
//!
//! Not every story fight carries the flag: a `3E FF <row>` arm points at a
//! row whose header byte is whatever the scene authored, and a few boss rows
//! author zero - such a fight runs under the random-encounter rules (the
//! flag-clear boost profile, no side-effect gates). The census reports what
//! the row says; it does not guess from the monster.
//!
//! One shape the census cannot see: an **inline-script** encounter, where the
//! field VM points the record slot at bytecode overlaying the install opcode
//! (`docs/formats/encounter.md`). Those always raise the bit (the opcode is
//! the non-zero header) and name ids that appear in no row. A monster with
//! **no** row at all is therefore met only that way - a scripted fight.
//!
//! Consumers: `asset formation-census` (CLI), the web viewer's enemy table
//! (per-row fight class + boost profile + side-effect susceptibility).

use std::collections::BTreeMap;

use serde::Serialize;

use crate::man_section;
use crate::scene_asset_table;

/// Asset type byte of a MAN descriptor / streaming chunk.
const MAN_TYPE: u8 = 0x03;

/// Per-monster row counts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct FightRows {
    /// Rows whose header byte `+0` is non-zero (scripted / boss fights).
    pub flagged: u32,
    /// Rows whose header byte is zero.
    pub clear: u32,
    /// Rows a `rate > 0` region references (random encounters). Always a
    /// subset of `clear` on the retail disc, but counted independently.
    pub random: u32,
}

impl FightRows {
    /// `true` when this monster is met in a scripted fight: it sits in a
    /// flagged row, or in no row at all (an inline-script install, which
    /// raises the flag by construction).
    pub fn met_scripted(&self) -> bool {
        self.flagged > 0 || (self.clear == 0 && self.random == 0)
    }

    /// `true` when this monster is met in a fight whose flag is clear
    /// (random encounters and script-engaged rows with a zero header).
    pub fn met_unflagged(&self) -> bool {
        self.clear > 0
    }

    /// The fight class the enemy table should present by default: the
    /// unflagged (random-encounter) rules when any such row exists, else the
    /// scripted rules.
    pub fn default_scripted(&self) -> bool {
        !self.met_unflagged()
    }
}

/// The census: monster id → row counts, plus how many scenes contributed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct FormationCensus {
    /// Per monster id (1-based archive id), sorted.
    pub monsters: BTreeMap<u8, FightRows>,
    /// MANs whose encounter section parsed (bundled + streaming).
    pub scenes: u32,
    /// Formation rows walked.
    pub rows: u32,
}

impl FormationCensus {
    /// Walk every PROT entry `(index, bytes)` and fold its formation rows in.
    pub fn from_entries<'a>(entries: impl IntoIterator<Item = (usize, &'a [u8])>) -> Self {
        let mut census = FormationCensus::default();
        for (_idx, bytes) in entries {
            for man in scene_mans(bytes) {
                census.fold_man(&man);
            }
        }
        census
    }

    /// Fold one decoded MAN's formation rows into the census. A MAN without
    /// a parseable encounter section contributes nothing.
    pub fn fold_man(&mut self, man: &[u8]) {
        let Ok(manfile) = man_section::parse(man) else {
            return;
        };
        let Some(body) = manfile.encounter_section_body(man) else {
            return;
        };
        let Ok(sec) = man_section::parse_encounter_section(body) else {
            return;
        };
        self.scenes += 1;
        // Random reachability: any rate>0 region's `[base, base+count)`.
        let mut random_mask = vec![false; sec.formation_count as usize];
        for r in man_section::region_records(body, &sec).flatten() {
            if r.rate_increment == 0 {
                continue;
            }
            let base = r.formation_range_base as usize;
            let count = r.formation_range_count as usize;
            for slot in random_mask.iter_mut().skip(base).take(count) {
                *slot = true;
            }
        }
        for (i, f) in man_section::formation_records(body, &sec).enumerate() {
            let Some(f) = f else {
                continue;
            };
            if f.monster_count == 0 {
                continue;
            }
            self.rows += 1;
            let n = (f.monster_count as usize).min(4);
            let flagged = f.header_bytes[0] != 0;
            let random = random_mask.get(i).copied().unwrap_or(false);
            for &id in &f.monster_ids[..n] {
                if id == 0 {
                    continue;
                }
                let e = self.monsters.entry(id).or_default();
                if flagged {
                    e.flagged += 1;
                } else {
                    e.clear += 1;
                }
                if random {
                    e.random += 1;
                }
            }
        }
    }

    /// Row counts for one monster id (`FightRows::default()` when the id is
    /// in no row - see [`FightRows::met_scripted`]).
    pub fn rows_for(&self, id: u8) -> FightRows {
        self.monsters.get(&id).copied().unwrap_or_default()
    }
}

/// Every decoded MAN in one PROT entry: the LZS-compressed type-`0x03`
/// descriptor of a `scene_asset_table` bundle, and each raw type-`0x03`
/// DATA_FIELD streaming chunk (the v12-family dungeons). Empty for an entry
/// that is neither.
pub fn scene_mans(entry: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    if let Some(table) = scene_asset_table::detect(entry) {
        if let Some(man) = table
            .descriptors
            .iter()
            .find(|d| d.type_byte == MAN_TYPE)
            .copied()
        {
            let start = man.data_offset as usize;
            if start < entry.len()
                && let Ok((decoded, _)) =
                    legaia_lzs::decompress_tracked(&entry[start..], man.size as usize)
            {
                out.push(decoded);
            }
        }
        return out;
    }
    if let Ok(report) = crate::parse_streaming(entry, 4096) {
        for chunk in &report.chunks {
            if chunk.type_byte != MAN_TYPE {
                continue;
            }
            let start = chunk.header_offset + 4;
            let Some(payload) = entry.get(start..start.saturating_add(chunk.size as usize)) else {
                continue;
            };
            out.push(payload.to_vec());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn met_scripted_reads_flagged_rows_or_no_rows() {
        let none = FightRows::default();
        assert!(none.met_scripted());
        assert!(!none.met_unflagged());
        assert!(none.default_scripted());
        let boss = FightRows {
            flagged: 2,
            clear: 0,
            random: 0,
        };
        assert!(boss.met_scripted());
        assert!(boss.default_scripted());
        let random = FightRows {
            flagged: 0,
            clear: 5,
            random: 5,
        };
        assert!(!random.met_scripted());
        assert!(random.met_unflagged());
        assert!(!random.default_scripted());
        let both = FightRows {
            flagged: 1,
            clear: 3,
            random: 3,
        };
        assert!(both.met_scripted());
        assert!(both.met_unflagged());
        assert!(!both.default_scripted());
    }

    #[test]
    fn a_non_scene_entry_yields_no_mans() {
        assert!(scene_mans(&[0u8; 64]).is_empty());
        let census = FormationCensus::from_entries([(0usize, &[0u8; 64][..])]);
        assert_eq!(census.scenes, 0);
        assert!(census.monsters.is_empty());
    }
}
