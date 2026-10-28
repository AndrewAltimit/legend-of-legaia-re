//! Capture coverage: what the retail comparison corpus *shows*, read off each
//! library state's RAM with no engine run, plus the disc's own lists of what
//! there is to show.
//!
//! The corpus ([`crate::retail_compare`]) scores the engine at every captured
//! moment, so its headline is only as broad as the moments it holds. This
//! module is the denominator half: for every corpus state it records the
//! scene, the class and - on a battle state - who is acting and with what
//! ([`CensusBattle`]); from the disc it lists every monster record with the
//! attacks it can use, every spell-table record with its cast class, and
//! every scene's MAN formation rows. `scripts/ci/capture-coverage.py` joins
//! the two into the per-item worklist (`docs/tooling/capture-coverage.md`).
//!
//! Nothing here writes disc bytes into a committed file: the census JSON
//! carries names read off the disc and lives under the gitignored report
//! directory.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use legaia_ram_image::ScenarioManifest;
use serde::Serialize;

use crate::retail_compare::{StateClass, enumerate_corpus, read_retail};

/// The battle-action state bands in which an action is on screen
/// (`docs/subsystems/battle-action.md`, outer dispatch): the attack chain
/// `0x14..=0x20`, the magic / item flow `0x28..=0x2E`, the summon flow
/// `0x32..=0x38`, the spirit bands `0x3C..=0x48` and the capture-class
/// module sequence `0x6E..=0x71`.
pub fn action_band(action_state: u8) -> Option<&'static str> {
    match action_state {
        0x14..=0x20 => Some("attack"),
        0x28..=0x2E => Some("magic"),
        0x32..=0x38 => Some("summon"),
        0x3C..=0x48 => Some("spirit"),
        0x50..=0x52 | 0x5A => Some("cleanup"),
        0x64..=0x6B => Some("run"),
        0x6E..=0x71 => Some("module"),
        _ => None,
    }
}

/// What a battle state shows.
#[derive(Debug, Clone, Serialize)]
pub struct CensusBattle {
    /// Formation-cell monster ids (1-based archive ids).
    pub monster_ids: Vec<u8>,
    /// `0x8007BD10[..]`: the present party, 1-based roster ids.
    pub party: Vec<u8>,
    /// `ctx[+0x06]`.
    pub flow: u8,
    /// `ctx[+0x07]`.
    pub action_state: u8,
    /// [`action_band`] of the action state.
    pub band: Option<&'static str>,
    /// `ctx[+0x13]`: the seat the action SM runs (`0..3` party, `3..` monster).
    pub active_seat: u8,
    /// The monster id standing on the active seat, when it is a monster seat.
    pub active_monster: Option<u8>,
    /// The active seat's committed category `+0x1DE`.
    pub category: u8,
    /// The active seat's queued action `+0x1DF` (a spell id on a cast).
    pub queued_action: u8,
    /// `0x8007B7FC`: non-zero on a fight entered by battle id.
    pub battle_id: u8,
    /// The scripted-fight bit.
    pub scripted: bool,
}

/// One corpus state as the census reads it.
#[derive(Debug, Clone, Serialize)]
pub struct CensusState {
    pub label: String,
    pub emulator: String,
    pub fingerprint: String,
    pub resident_patch: Option<String>,
    pub class: Option<StateClass>,
    /// The loaded scene (the label a pending scene-change packet wrote is
    /// [`Self::pending_scene`]).
    pub scene: String,
    pub pending_scene: Option<String>,
    pub game_mode: u8,
    /// Whether the state carries a displayed frame.
    pub has_frame: bool,
    /// A menu state's sub-screen word `DAT_801E46A4`.
    pub menu_subscreen: Option<u8>,
    /// Why a battle / menu state is not one the seed drives to, when it is not.
    pub not_seedable: Option<String>,
    pub battle: Option<CensusBattle>,
    /// Why the state did not read.
    pub error: Option<String>,
}

/// One monster record and what it can do.
#[derive(Debug, Clone, Serialize)]
pub struct CensusMonster {
    pub id: u16,
    pub name: String,
    /// Global spell ids the AI casts by name (record `+0x21..=+0x23`).
    pub magic_attacks: Vec<u8>,
    /// Rollable castable action entries (`MonsterSpell::is_castable`).
    pub castable_entries: usize,
    /// The Seru this monster is, when it is one.
    pub seru_id: u8,
}

/// One spell-table record.
#[derive(Debug, Clone, Serialize)]
pub struct CensusSpell {
    pub id: u8,
    pub name: Option<String>,
    /// Record `+0`: `0x32` player summon, `0x14` plain cast, `0x63` capture class.
    pub class: u8,
    pub sub_class: u8,
    /// The slot-B module the cast pages, when it pages one: the summon band
    /// `0x81..=0xA0` pages `id - 0x79 + 0x37F` (PROT `0903..0934`), the
    /// capture class `0x28 + sub + 0x37F` (PROT `0935..0966`).
    pub module_prot: Option<u16>,
}

/// One scene's MAN formation rows.
#[derive(Debug, Clone, Serialize)]
pub struct CensusScene {
    pub label: String,
    pub define: u32,
    /// Each rollable row's monster ids.
    pub formations: Vec<Vec<u16>>,
}

/// The disc's lists.
#[derive(Debug, Clone, Serialize)]
pub struct CensusDisc {
    pub monsters: Vec<CensusMonster>,
    pub spells: Vec<CensusSpell>,
    pub scenes: Vec<CensusScene>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Census {
    pub states: Vec<CensusState>,
    pub disc: CensusDisc,
}

/// The slot-B module a spell record pages (see [`CensusSpell::module_prot`]).
///
/// The summon band pages by id, whatever the record's class byte: the two
/// ally-side Seru (`0x83` Vera, `0x89` Orb) carry another class and still run
/// the summon flow `0x32..=0x38` on their own module.
pub fn spell_module_prot(id: u8, class: u8, sub_class: u8) -> Option<u16> {
    match (id, class) {
        // `FUN_8003EC70(id - 0x79)`, extraction = param + 0x37F.
        (0x81..=0xA0, _) => Some(id as u16 - 0x79 + 0x37F),
        (_, 0x63) => Some(sub_class as u16 + 0x28 + 0x37F),
        _ => None,
    }
}

fn read_state(
    entry: &crate::retail_compare::CorpusEntry,
    scus: &[u8],
    cdname: &legaia_prot::cdname::IndexMap,
) -> CensusState {
    let mut st = CensusState {
        label: entry.label.clone(),
        emulator: entry.emulator.to_string(),
        fingerprint: entry.fingerprint.chars().take(16).collect(),
        resident_patch: entry.resident_patch.clone(),
        class: None,
        scene: String::new(),
        pending_scene: None,
        game_mode: 0,
        has_frame: false,
        menu_subscreen: None,
        not_seedable: None,
        battle: None,
        error: None,
    };
    let mut obs = match read_retail(entry, scus) {
        Ok(o) => o,
        Err(e) => {
            st.error = Some(format!("{e:#}"));
            return st;
        }
    };
    obs.settle_on_loaded_scene(cdname);
    st.class = Some(obs.class);
    st.scene = obs.scene.clone();
    st.pending_scene = obs.pending_scene.clone();
    st.game_mode = obs.game_mode;
    st.has_frame = obs.frame.is_some();
    st.menu_subscreen = obs.menu_word;
    if let Some(Err(why)) = &obs.menu {
        st.not_seedable = Some(why.clone());
    }
    match &obs.battle {
        Some(Ok(b)) => {
            let active_monster = (b.active_actor >= 3)
                .then(|| b.monster_ids.get(b.active_actor as usize - 3).copied())
                .flatten();
            st.battle = Some(CensusBattle {
                monster_ids: b.monster_ids.clone(),
                party: b.seat_chars.clone(),
                flow: b.flow,
                action_state: b.action_state,
                band: action_band(b.action_state),
                active_seat: b.active_actor,
                active_monster,
                category: b.queued_category,
                queued_action: b.queued_action,
                battle_id: b.battle_id,
                scripted: b.scripted,
            });
        }
        Some(Err(why)) => st.not_seedable = Some(why.clone()),
        None => {}
    }
    st
}

fn disc_lists(
    extracted: &Path,
    scus: &[u8],
    cdname: &legaia_prot::cdname::IndexMap,
) -> Result<CensusDisc> {
    let archive_path = extracted.join("PROT").join("0867_battle_data.BIN");
    let archive =
        std::fs::read(&archive_path).with_context(|| format!("read {}", archive_path.display()))?;
    let monsters = legaia_actor_formats::monster_archive::records(&archive)?
        .into_iter()
        .map(|r| CensusMonster {
            id: r.id,
            castable_entries: r.spells.iter().filter(|s| s.is_castable()).count(),
            name: r.name,
            magic_attacks: r.magic_attacks,
            seru_id: r.seru_id,
        })
        .collect();
    let table =
        legaia_game_tables::spell_names::SpellNameTable::from_scus(scus).context("spell table")?;
    let spells = (0..table.len())
        .filter_map(|i| {
            let id = u8::try_from(i).ok()?;
            let e = table.entry(id)?;
            Some(CensusSpell {
                id,
                name: e.name.clone(),
                class: e.class,
                sub_class: e.sub_class,
                module_prot: spell_module_prot(id, e.class, e.sub_class),
            })
        })
        .collect();
    let index = legaia_engine_scene::scene::ProtIndex::open_extracted(extracted)?;
    let mut scenes = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for (&define, label) in cdname {
        if !seen.insert(label.clone()) {
            continue;
        }
        let Ok(scene) = legaia_engine_scene::scene::Scene::load(&index, label) else {
            continue;
        };
        let Ok(Some(man)) = scene.field_man_payload(&index) else {
            continue;
        };
        let formations = legaia_engine_battle::encounter_man::formation_defs_from_man(&man)
            .into_iter()
            .map(|f| f.slots.iter().map(|s| s.monster_id).collect())
            .collect();
        scenes.push(CensusScene {
            label: label.clone(),
            define,
            formations,
        });
    }
    Ok(CensusDisc {
        monsters,
        spells,
        scenes,
    })
}

/// Read every corpus state and the disc's lists.
pub fn census(
    extracted: &Path,
    library: &Path,
    manifest: &ScenarioManifest,
    filter: Option<&str>,
) -> Result<Census> {
    let scus = std::fs::read(extracted.join("SCUS_942.54"))
        .with_context(|| format!("read {}/SCUS_942.54", extracted.display()))?;
    let cdname = legaia_prot::cdname::parse(&extracted.join("CDNAME.TXT"))?;
    let mut states = Vec::new();
    for entry in enumerate_corpus(manifest, library) {
        if let Some(f) = filter
            && !f
                .split(',')
                .any(|alt| !alt.is_empty() && entry.label.contains(alt))
        {
            continue;
        }
        crate::retail_compare_image::set_capture_emulator(entry.emulator);
        states.push(read_state(&entry, &scus, &cdname));
    }
    let disc = disc_lists(extracted, &scus, &cdname)?;
    Ok(Census { states, disc })
}

/// Per-class counts of a census, for the CLI's one-line summary.
pub fn class_counts(c: &Census) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for s in &c.states {
        let k = s
            .class
            .and_then(|c| serde_json::to_value(c).ok())
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_else(|| "unreadable".into());
        *out.entry(k).or_default() += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summon_and_capture_modules_land_on_their_bands() {
        // Gimard, the first summon-class id, pages PROT 0903; Ozma (0xA0)
        // the band's last, 0934.
        assert_eq!(spell_module_prot(0x81, 0x32, 0), Some(903));
        assert_eq!(spell_module_prot(0xA0, 0x32, 0), Some(934));
        // Capture class: sub-id 0 is 0935, sub-id 31 is 0966.
        assert_eq!(spell_module_prot(0x4A, 0x63, 0), Some(935));
        assert_eq!(spell_module_prot(0xAD, 0x63, 31), Some(966));
        // Vera's class byte is not the summon class; it still pages 0905.
        assert_eq!(spell_module_prot(0x83, 0x00, 0x03), Some(905));
        // A plain cast pages nothing.
        assert_eq!(spell_module_prot(0x27, 0x14, 0), None);
    }

    #[test]
    fn action_bands_follow_the_outer_dispatch() {
        assert_eq!(action_band(0x35), Some("summon"));
        assert_eq!(action_band(0x6F), Some("module"));
        assert_eq!(action_band(0x19), Some("attack"));
        assert_eq!(action_band(0xFF), None);
        assert_eq!(action_band(0x00), None);
    }
}
