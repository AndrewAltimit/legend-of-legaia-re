//! Muscle Dome methods of [`LegaiaMinigames`] - the browser twin of the
//! play-window's `start_muscle_minigame` (`window/minigames.rs`).
//!
//! The dome is not a card battle, and it is not turn-limited either: each
//! round stages one real monster off the course ladder into the ordinary
//! battle formation cell and is fought to a knockout. The
//! `"Turns Left: N   HP Left: P%"` strip belongs to the one fight whose
//! formation slot 0 is monster `0xB6` (Koru); the dome ladder tops out at
//! `0xAA`, so no dome round raises it.
//!
//! The rules are the ported [`legaia_engine_core::muscle_dome`] engine (the
//! four-direction deal, the AP-budget commit into the fighter's action queue,
//! the turn counter, the opponent-HP-left readout and the
//! win/lose bookkeeping). Damage resolves through the **shared retail
//! kernel** [`legaia_engine_core::muscle_dome::DomeDamageModel`], which the
//! native play-window host installs on its session too - the move-power
//! record via the `0x801F4E63` id → index map, the arts/physical damage roll
//! (`FUN_801dd0ac`), the element-affinity scale (`FUN_801dd864`) and the
//! damage finisher (`FUN_801ddb30`), on a PsyQ `rand()` stream with retail
//! draw order. This module holds no damage rule of its own.
//!
//! Fighter stats come from the visitor's own disc records:
//!
//! - the **opponent** is a monster of the PROT 867 archive
//!   ([`legaia_asset::monster_archive`]); its battle stats are the record's
//!   `battle_stats()` boosted profile (the `FUN_80054CB0` load-time boost),
//!   its round budget the record's AGL (the `+0x154` pool the dome's
//!   `ctx+0x6dc` budget seeds from), its element the record's `+0x1D` byte.
//! - the **player** fighter's record is built from the `SCUS_942.54`
//!   new-game starting-party template ([`legaia_asset::new_game`]) leveled
//!   through the per-level stat-growth curves
//!   ([`legaia_asset::level_up_tables`], jitter-free core gains - see the
//!   documented approximation on [`LegaiaMinigames::muscle_start_vs`]), then
//!   seeded into a battle-actor stat block by the ported battle-load init
//!   (`init_party_battle_stats`, `FUN_80053CB8`, no equipment). Card costs
//!   are the character's own player-battle-file swing records (`+0x74`, the
//!   bytes the Arts gauge reads), section defaults.
//!
//! Documented host models / approximations, each surfaced to the page instead
//! of silently invented: the opponent AI (greedy in-order commit - retail has
//! no dome-specific AI table), the player level (retail uses your save's
//! party; the page exposes a level control over the disc's own growth
//! tables), the jitter-free growth core (retail adds a `rand()` spread of
//! mean 0), and the awarded Seru index (the arena init's `ctx+0x269` write is
//! not table-pinned; a default of 1 is used and named via the SCUS spell
//! table).

use super::*;

use legaia_art::queue::{Character as ArtCharacter, Command as ArtCommand};
use legaia_asset::battle_char_assembly as bca;
use legaia_asset::element_affinity::ElementAffinity;
use legaia_asset::monster_archive;
use legaia_asset::move_power;
use legaia_asset::muscle_dome as md;
use legaia_asset::scene_tmd_stream;
use legaia_asset::sfx_table;
use legaia_engine_core::muscle_dome::{
    DomeCombatant, DomeDamageModel, MuscleCard, MuscleDomeSession, MusclePhase,
};
use legaia_engine_vm::battle_formulas::{RecordStats, init_party_battle_stats};

/// PROT entry of the monster stat archive (`0867_battle_data`).
const MONSTER_ARCHIVE_PROT_INDEX: u32 = 867;

/// PROT entry of the first player battle file (`data\battle\PLAYER1`,
/// extraction 863; `+ char_slot` for Noa / Gala / Terra).
const PLAYER_BATTLE_FILE_BASE: u32 = 863;

/// PROT entry (extraction space) of the Sol Muscle Dome **arena backdrop**
/// stream - the tail slot of the dome's `data\field\other6.lzs` file (CDNAME
/// `other6` = raw TOC 1222 -> extraction block 1220..=1225, loaded by the
/// arena door/init overlay at extraction 0977). It is the block's only
/// `scene_tmd_stream` - the battle-backdrop carrier format the battle init
/// walker `FUN_8001FE70` records into `_DAT_8007B864`: a leading arena-shell
/// TMD plus two type-0x01 TIM pages at framebuffer `(768, 0)` / `(832, 0)`
/// (CLUT rows 473 / 479) - `(832, 0)` + CLUT `(0, 479)` being exactly the
/// address the battle ground-grid renderer `func_0x801d02c0` samples. See
/// `docs/subsystems/minigame-muscle-dome.md` (Arena backdrop).
const ARENA_BACKDROP_PROT_INDEX: u32 = 1225;

/// The dome match SM's own UI cue ids as **called** (`FUN_801d0748` passes
/// these to the one-arg cue funnel `FUN_8004fcc8`, whose `< 0x40` leg
/// enqueues `id - 1` as the static descriptor row). 34 immediate call sites:
/// `0x21` x13, `0x22` x7, `0x23` x14.
const MUSCLE_UI_CUE_CALL_IDS: [u8; 3] = [0x21, 0x22, 0x23];

/// The physical-impact static cue row of the shared battle/duel bank
/// (descriptor row `0x09`: program 0, tones 9..=10, category 2 -> the PROT
/// 0869 VAB). Pinned as the melee hit at the top of the Baka duel damage
/// kernel (`FUN_801D3B18`); the dome resolves its card plays through the same
/// shared battle-action path and bank.
const MUSCLE_HIT_CUE_ROW: u8 = 0x09;

/// Flat per-card cost fallback (the native launcher's `FAVORED_COST`), used
/// when the character's swing records don't decode.
const FAVORED_COST: u16 = 0x1E;

/// The Seru index awarded on a win (`ctx+0x269`); reward spell id is
/// `REWARD_SPELL_ID_BASE + index`. The arena init's per-contest write is not
/// table-pinned - documented approximation.
const WEB_REWARD_SERU: u8 = 1;

/// Non-elemental element id (`element_affinity` id space).
const ELEMENT_NEUTRAL: u8 = 7;

/// The curated gamedata tables (baked-in TOML), parsed once per session -
/// the arts **kind** labels (regular / hyper / super / miracle) the banner
/// classifier joins onto the disc's own arts rows.
fn gamedata_db() -> &'static legaia_gamedata::Database {
    static DB: std::sync::OnceLock<legaia_gamedata::Database> = std::sync::OnceLock::new();
    DB.get_or_init(legaia_gamedata::Database::load)
}

/// One fighter's battle-formula inputs, resolved from disc records at contest
/// start. Field names follow the battle-actor offsets the damage kernel reads.
#[derive(Debug, Clone, Copy)]
pub(crate) struct MuscleFighter {
    /// Max HP (`+0x14e`); current HP lives in the rules session.
    hp_max: u16,
    /// Max MP (`+0x152`) - displayed on the retail battle status plate. The
    /// dome rules never spend it (the port has no cast path); `0` for a
    /// monster opponent, whose plate retail does not draw.
    mp_max: u16,
    /// AGL (`+0x154`) - the round-budget pool the dome seeds `ctx+0x6dc` from.
    budget_pool: u16,
    /// INT working value (`+0x168`) - the damage kernel's roll stat.
    int: u16,
    /// UDF (`+0x15c`) - defender roll term A.
    udf: u16,
    /// LDF (`+0x160`) - defender roll term B.
    ldf: u16,
    /// Element id (0..=7) for the affinity scale.
    element: u8,
}

impl MuscleFighter {
    /// The subset the shared retail damage kernel reads.
    fn combatant(&self) -> DomeCombatant {
        DomeCombatant {
            hp_max: self.hp_max,
            int: self.int,
            udf: self.udf,
            ldf: self.ldf,
            element: self.element,
        }
    }
}

/// Read a little-endian `u32` at a VA inside the as-loaded PROT 0898 image.
fn overlay_u32(image: &[u8], va: u32) -> Option<u32> {
    let off = va.checked_sub(md::MUSCLE_OVERLAY_BASE_VA)? as usize;
    Some(u32::from_le_bytes(
        image.get(off..off + 4)?.try_into().ok()?,
    ))
}

/// Read the NUL-terminated string at a VA inside the as-loaded PROT 0898
/// image. Bounded at 128 bytes; non-ASCII bytes are dropped, which keeps a
/// mis-resolved pointer from emitting binary into the page.
fn overlay_string(image: &[u8], va: u32) -> Option<String> {
    let off = va.checked_sub(md::MUSCLE_OVERLAY_BASE_VA)? as usize;
    let win = image.get(off..(off + 128).min(image.len()))?;
    let end = win.iter().position(|&b| b == 0)?;
    Some(
        win[..end]
            .iter()
            .filter(|&&b| (0x20..0x7F).contains(&b))
            .map(|&b| b as char)
            .collect(),
    )
}

/// The cached PROT 0898 battle tables the dome plays with.
pub(crate) struct MuscleTables {
    /// The four dealt hand command ids (deck table `DAT_801f4b8c`).
    pub hand: [u8; md::HAND_SLOTS],
    /// The 44-record move-power table (`0x801F4F5C`).
    move_power: Vec<move_power::MoveRecord>,
    /// The 128-byte move-id → power-index map (`0x801F4E63`).
    move_map: [u8; move_power::MOVE_ID_INDEX_MAP_LEN],
    /// The 8x8 element-affinity matrix + per-character elements
    /// (`0x801F53E8` / `0x801F5480`).
    affinity: Option<ElementAffinity>,
}

/// A running contest: the rules session plus everything the battle-formula
/// resolution needs alongside it.
pub(crate) struct MuscleContest {
    session: MuscleDomeSession,
    fighters: [MuscleFighter; 2],
    names: [String; 2],
    /// `"disc"` when the player record came from the SCUS template + growth
    /// tables, `"fallback"` when no executable was available.
    stats_source: &'static str,
    monster_id: u16,
    char_slot: usize,
    level: u32,
}

impl LegaiaMinigames {
    /// Decode the battle overlay (PROT 0898) into the cached dome tables,
    /// returning the status object `load_disc` folds into its report.
    pub(super) fn load_muscle_tables(&mut self) -> String {
        self.muscle = None;
        self.muscle_tables = None;
        // Hand ids live at VA-based offsets of the as-loaded image; the
        // move-power / affinity tables are pinned at raw-entry file offsets.
        // PROT 0898 is stored uncompressed, so both views see the same bytes.
        let loaded = overlay_image(
            &self.prot,
            &self.entries,
            md::MUSCLE_OVERLAY_PROT_INDEX as u32,
        );
        let raw = entry_bytes(
            &self.prot,
            &self.entries,
            md::MUSCLE_OVERLAY_PROT_INDEX as u32,
        );
        let tables = (|| {
            let hand = md::hand_command_ids(loaded.as_deref()?)?;
            let raw = raw?;
            let move_power = move_power::parse(raw)?;
            let move_map = move_power::parse_id_index_map(raw)?;
            let affinity = legaia_asset::element_affinity::parse(raw);
            Some(MuscleTables {
                hand,
                move_power,
                move_map,
                affinity,
            })
        })();
        match tables {
            Some(t) => {
                let list = t
                    .hand
                    .iter()
                    .map(|c| c.to_string())
                    .collect::<Vec<_>>()
                    .join(",");
                let stats = if self.scus.is_some() {
                    "disc"
                } else {
                    "fallback"
                };
                self.muscle_tables = Some(t);
                format!(r#"{{"ok":true,"cards":[{list}],"stats":{}}}"#, jstr(stats))
            }
            None => format!(
                r#"{{"ok":false,"why":{}}}"#,
                jstr("Muscle Dome battle overlay (PROT 0898) or its tables did not decode")
            ),
        }
    }

    fn monster_archive_entry(&self) -> Option<&[u8]> {
        entry_bytes(&self.prot, &self.entries, MONSTER_ARCHIVE_PROT_INDEX)
    }

    /// The player character's four swing-card AP costs (runtime slots
    /// `0xC..=0xF`), from their player battle file's **section-default**
    /// equipment records - the same `+0x74` bytes the Arts gauge reads.
    fn muscle_swing_costs(&self, char_slot: usize) -> Option<[u16; 4]> {
        let raw = entry_bytes(
            &self.prot,
            &self.entries,
            PLAYER_BATTLE_FILE_BASE + char_slot as u32,
        )?;
        let pack = legaia_asset::battle_data_pack::parse(raw).ok()?;
        // Section defaults (id 0 per slot) - the browser has no save to read
        // an equipped set from.
        let swings =
            legaia_asset::battle_char_assembly::swing_battle_animations(raw, &pack, &[0u8; 5])
                .ok()?;
        let mut costs = [0u16; 4];
        for s in &swings {
            let i = s.slot.checked_sub(0xC)? as usize;
            if i < 4 && s.cost > 0 {
                costs[i] = s.cost as u16;
            }
        }
        if costs.contains(&0) {
            return None;
        }
        Some(costs)
    }

    /// The player fighter's record stats: SCUS new-game template leveled
    /// through the growth curves (jitter-free core), battle-load initialised.
    fn muscle_player_fighter(
        &self,
        char_slot: usize,
        level: u32,
    ) -> Option<(MuscleFighter, String)> {
        let scus = self.scus.as_ref()?;
        let party = legaia_asset::new_game::StartingParty::from_scus(scus)?;
        let m = party.member(char_slot)?;
        let growth = legaia_asset::level_up_tables::growth_tables_from_scus(scus)?;
        let params = growth.char_params(char_slot)?;
        // Template stat order == growth-param record order:
        // [hp, mp, agl, atk, udf, ldf, spd, int].
        let base = [
            m.hp_max, m.mp_max, m.agl, m.atk, m.udf, m.ldf, m.spd, m.intel,
        ];
        let mut stats = base.map(u32::from);
        let level = level.clamp(1, legaia_asset::level_up_tables::MAX_LEVEL as u32);
        for from_level in 1..level as usize {
            for (i, p) in params.stats.iter().enumerate() {
                let gain = growth.level_gain_core(p, from_level).unwrap_or(0);
                stats[i] = (stats[i] + gain).min(p.max as u32);
            }
        }
        let record = RecordStats {
            hp_max: stats[0] as u16,
            hp_cur: stats[0] as u16,
            mp_max: stats[1] as u16,
            mp_cur: stats[1] as u16,
            spirit: 0,
            agl: stats[2] as u16,
            atk: stats[3] as u16,
            udf: stats[4] as u16,
            ldf: stats[5] as u16,
            spd: stats[6] as u16,
            int: stats[7] as u16,
        };
        // Battle-load stat init (FUN_80053CB8), no equipment bonuses.
        let actor = init_party_battle_stats(&record, &[None; 5]);
        let element = self
            .muscle_tables
            .as_ref()
            .and_then(|t| t.affinity.as_ref())
            .and_then(|a| a.character_element(char_slot as u8 + 1))
            .unwrap_or(ELEMENT_NEUTRAL);
        Some((
            MuscleFighter {
                hp_max: actor.hp_max,
                mp_max: actor.mp_max,
                budget_pool: actor.agl,
                int: actor.int,
                udf: actor.udf,
                ldf: actor.ldf,
                element,
            },
            m.name.clone(),
        ))
    }

    /// The arena backdrop entry's bytes, gated on the scene_tmd_stream shape
    /// (so a truncated / foreign image degrades to "no arena" rather than a
    /// garbage mesh).
    fn muscle_arena_entry(&self) -> Option<&[u8]> {
        let buf = entry_bytes(&self.prot, &self.entries, ARENA_BACKDROP_PROT_INDEX)?;
        scene_tmd_stream::detect(buf).map(|_| buf)
    }

    /// The arena-shell TMD built as a hybrid VRAM mesh (textured prims sample
    /// the backdrop pages; untextured prims keep their baked colour word).
    fn muscle_arena_hybrid(&self) -> Option<(legaia_tmd::mesh::VramMesh, Vec<u8>)> {
        let buf = self.muscle_arena_entry()?;
        let stream = scene_tmd_stream::detect(buf)?;
        let tmd_bytes = buf.get(stream.tmd_range())?;
        let tmd = legaia_tmd::parse(tmd_bytes).ok()?;
        let (mesh, oids, shading) =
            legaia_tmd::mesh::tmd_to_vram_mesh_field_hybrid(&tmd, tmd_bytes);
        let mut flat = Vec::with_capacity(shading.colors.len() * 4);
        for (c, &t) in shading.colors.iter().zip(shading.textured.iter()) {
            flat.extend_from_slice(&[c[0], c[1], c[2], if t != 0 { 255 } else { 0 }]);
        }
        // Drop TMD object 1 - the wall-base dust-decal object (12 ABE ABR-1
        // quads over the (128..190, 192..253) window of the (832, 0) page,
        // CLUT (16, 479)). Its texels are genuinely BRIGHT (whitish wisps up
        // to ~(208, 208, 248)), so even a correct additive draw reads as a
        // white cloud band ringing the arena - and the retail match capture
        // shows a mist-free interior, i.e. the retail backdrop path does not
        // draw this object as static geometry. The shell (object 0) keeps
        // its own ABE lamp-glow prims. See
        // docs/subsystems/minigame-muscle-dome.md (Arena backdrop).
        if oids.iter().any(|&o| o != 0) {
            let mut mesh2 = mesh.clone();
            mesh2.positions.clear();
            mesh2.uvs.clear();
            mesh2.cba_tsb.clear();
            mesh2.normals.clear();
            mesh2.colors.clear();
            mesh2.indices.clear();
            let mut remap = vec![u32::MAX; oids.len()];
            let mut flat2 = Vec::new();
            for (i, &o) in oids.iter().enumerate() {
                if o != 0 {
                    continue;
                }
                remap[i] = mesh2.positions.len() as u32;
                mesh2.positions.push(mesh.positions[i]);
                mesh2.uvs.push(mesh.uvs[i]);
                mesh2.cba_tsb.push(mesh.cba_tsb[i]);
                mesh2.normals.push(mesh.normals[i]);
                mesh2.colors.push(mesh.colors[i]);
                flat2.extend_from_slice(&flat[i * 4..i * 4 + 4]);
            }
            for t in mesh.indices.chunks_exact(3) {
                let (a, b, c) = (
                    remap[t[0] as usize],
                    remap[t[1] as usize],
                    remap[t[2] as usize],
                );
                if a != u32::MAX && b != u32::MAX && c != u32::MAX {
                    mesh2.indices.extend_from_slice(&[a, b, c]);
                }
            }
            return Some((mesh2, flat2));
        }
        Some((mesh, flat))
    }

    /// The player battle file (`data\battle\PLAYER1..3`) for a dome fighter
    /// slot (0 = Vahn, 1 = Noa, 2 = Gala).
    fn muscle_player_file(&self, char_slot: u32) -> Option<&[u8]> {
        entry_bytes(
            &self.prot,
            &self.entries,
            PLAYER_BATTLE_FILE_BASE + char_slot.min(2),
        )
    }

    /// Assemble the character's **battle form** (fighter form) - the same
    /// retail chain the arts viewer / native battles use: equipment-id
    /// sections spliced (`assemble_character`, all-default sections - the
    /// dome forbids equipment), TSB/CBA relocated to runtime band 0
    /// (`FUN_800513F0` registration pass). Returns the assembly (for
    /// `anm_bones`), the VRAM mesh and the per-vertex object ids.
    fn muscle_fighter_build(
        &self,
        char_slot: u32,
    ) -> Option<(
        bca::AssembledCharacter,
        legaia_tmd::mesh::VramMesh,
        Vec<u32>,
    )> {
        let raw = self.muscle_player_file(char_slot)?;
        let pack = legaia_asset::battle_data_pack::parse(raw).ok()?;
        let mut asm = bca::assemble_character(raw, &pack, &[0u8; 5]).ok()?;
        bca::relocate_tsb_cba(&mut asm.tmd, 0).ok()?;
        let tmd = legaia_tmd::parse(&asm.tmd).ok()?;
        let (mesh, oids) = legaia_tmd::mesh::tmd_to_vram_mesh_with_object_ids(&tmd, &asm.tmd);
        (!mesh.indices.is_empty()).then_some((asm, mesh, oids))
    }

    /// One battle-form action clip by **runtime action slot**, expanded per
    /// assembled TMD object so channel `i` drives object `i`:
    ///
    /// - slot `0` - the record[0] idle loop (`idle_battle_animation`);
    /// - slots `0xC..=0xF` - the per-command **swing records** of the
    ///   equipment sections (`swing_battle_animations`, section defaults) -
    ///   the same entries the dome's card ids `0xC..=0xF` name, so the
    ///   card -> clip pairing is the disc's own, not a fit;
    /// - other slots - the record[0] action table by index (the party
    ///   hit-reaction family: `FUN_80053CB8` writes the constant map
    ///   `[2, 3, 4, 5, 0xB]` to `+0x1EF..`, and the damage primitive
    ///   `FUN_800402F4` stages the light flinch from `+0x1EF` = slot 2).
    fn muscle_fighter_clip(
        &self,
        char_slot: u32,
        slot: u32,
    ) -> Option<monster_archive::MonsterAnimation> {
        let raw = self.muscle_player_file(char_slot)?;
        let (asm, _, _) = self.muscle_fighter_build(char_slot)?;
        let anim = if (0xC..=0xF).contains(&slot) {
            let pack = legaia_asset::battle_data_pack::parse(raw).ok()?;
            bca::swing_battle_animations(raw, &pack, &[0u8; 5])
                .ok()?
                .into_iter()
                .find(|s| s.slot as u32 == slot)?
                .anim
        } else if slot == 0 {
            bca::idle_battle_animation(raw).ok()??
        } else {
            bca::battle_animations(raw)
                .ok()?
                .into_iter()
                .find(|a| a.action_id as u32 == slot)?
        };
        Some(bca::expand_animation_for_objects(&anim, &asm.anm_bones))
    }

    /// The character's Tactical-Arts catalog for the queue -> art resolver:
    /// `(display name, kind, command string)` rows. Directions + names come
    /// from the disc's own SCUS arts-name table when an executable was
    /// loaded ([`legaia_art::arts_table`]); the curated
    /// [`legaia_gamedata`] arts table is the fallback catalog on a raw
    /// `PROT.DAT` load and, in both cases, the source of the **kind** label
    /// (regular / hyper / super / miracle - ground-truth walkthrough
    /// labels), joined by exact direction sequence.
    fn muscle_art_catalog(&self, char_slot: usize) -> Vec<(String, &'static str, Vec<ArtCommand>)> {
        let art_char = match char_slot {
            1 => ArtCharacter::Noa,
            2 => ArtCharacter::Gala,
            _ => ArtCharacter::Vahn,
        };
        let gd_char = match char_slot {
            1 => legaia_gamedata::Character::Noa,
            2 => legaia_gamedata::Character::Gala,
            _ => legaia_gamedata::Character::Vahn,
        };
        let db = gamedata_db();
        let kind_str = |k: legaia_gamedata::ArtKind| match k {
            legaia_gamedata::ArtKind::Regular => "regular",
            legaia_gamedata::ArtKind::Hyper => "hyper",
            legaia_gamedata::ArtKind::Super => "super",
            legaia_gamedata::ArtKind::Miracle => "miracle",
        };
        let disc_rows: Vec<(String, &'static str, Vec<ArtCommand>)> = self
            .scus
            .as_deref()
            .and_then(legaia_art::arts_table::parse_from_scus)
            .map(|entries| {
                entries
                    .into_iter()
                    .filter(|e| e.character == art_char && !e.commands.is_empty())
                    .map(|e| {
                        let dirs: Vec<u8> = e.commands.iter().map(|c| c.as_byte()).collect();
                        let kind = db
                            .find_art_by_directions(gd_char, &dirs)
                            .map(|a| kind_str(a.kind))
                            .unwrap_or("regular");
                        (e.name, kind, e.commands)
                    })
                    .collect()
            })
            .unwrap_or_default();
        if !disc_rows.is_empty() {
            return disc_rows;
        }
        db.arts_for(gd_char)
            .filter(|a| !a.directions.is_empty())
            .filter_map(|a| {
                let cmds: Option<Vec<ArtCommand>> = a
                    .directions
                    .iter()
                    .map(|&b| ArtCommand::from_byte(b))
                    .collect();
                Some((a.name.clone(), kind_str(a.kind), cmds?))
            })
            .collect()
    }

    /// Decode one static-table cue's voice layer to `(pcm, rate)`: descriptor
    /// row `row` of the SCUS SFX table keys VAB program `p` tones
    /// `t .. t+voices` at note `l` out of the bank its `+4` category routes to
    /// (slot 0 = PROT 0868, slot 2 = PROT 0869 - `docs/formats/sfx-table.md`).
    fn muscle_static_cue(&self, row: u8, voice: u8) -> Option<(Vec<i16>, u32)> {
        let scus = self.scus.as_ref()?;
        let table = sfx_table::SfxTable::from_scus(scus)?;
        let desc = *table.get(row)?;
        if voice >= desc.voice_count() {
            return None;
        }
        let bank_prot = sfx_table::prot_index_for_slot(desc.vab_slot())?;
        let entry = entry_bytes(&self.prot, &self.entries, bank_prot)?;
        let off = *legaia_vab::find_vabs(entry).first()?;
        let report = legaia_vab::parse(entry, off).ok()?;
        // Multi-voice cues span consecutive tone regions (`sfx-table.md`:
        // "the per-voice loop adds the voice index").
        let atr = report
            .tones
            .get(desc.program as usize)?
            .get(desc.tone as usize + voice as usize)?;
        if atr.vag <= 0 {
            return None;
        }
        let span = report.vag_samples.get(atr.vag as usize - 1)?;
        let body = entry.get(span.byte_offset..span.byte_offset + span.size)?;
        let pcm = legaia_vab::decode_vag_aligned(body).ok()?;
        let semitones = desc.note as f64 - atr.center as f64;
        let rate = (44100.0 * 2f64.powf(semitones / 12.0)).round();
        Some((pcm, rate.clamp(4000.0, 96_000.0) as u32))
    }

    /// An opponent fighter out of the monster archive: the record's boosted
    /// battle-stat profile, its AGL as the budget pool, its own element.
    fn muscle_monster_fighter(&self, monster_id: u16) -> Option<(MuscleFighter, String)> {
        let entry = self.monster_archive_entry()?;
        let rec = monster_archive::record(entry, monster_id).ok()??;
        // battle_stats() order: [AGL, ATK, UDF, LDF, INT, SPD] (boosted).
        let bs = rec.battle_stats();
        Some((
            MuscleFighter {
                hp_max: rec.hp,
                mp_max: 0,
                budget_pool: bs[0],
                int: bs[4],
                udf: bs[2],
                ldf: bs[3],
                element: rec.element.min(ELEMENT_NEUTRAL),
            },
            rec.name.clone(),
        ))
    }
}

#[wasm_bindgen]
impl LegaiaMinigames {
    /// Start a contest with defaults (Vahn at level 30 vs the archive's first
    /// decodable monster) - the compatibility entry the page's reset path and
    /// the older verification hooks call. Returns `false` when the tables
    /// didn't decode.
    pub fn muscle_start(&mut self) -> bool {
        let monster = self
            .monster_archive_entry()
            .and_then(|e| {
                let n = monster_archive::slot_count(e) as u16;
                (1..=n).find(|&id| {
                    monster_archive::record(e, id)
                        .ok()
                        .flatten()
                        .is_some_and(|r| r.hp > 0)
                })
            })
            .unwrap_or(1);
        self.muscle_start_vs(0, 30, monster, 0x2A)
    }

    /// Start a Muscle Dome contest: party character `char_slot` (0 = Vahn,
    /// 1 = Noa, 2 = Gala) at `level` versus monster `monster_id` (a PROT 867
    /// archive id), on PsyQ RNG seed `seed`.
    ///
    /// The player fighter's stats are the disc's own progression: the
    /// new-game template record leveled through the growth curves (the
    /// deterministic core gain per level - retail adds a `rand()` jitter of
    /// mean 0 on top, so the core is the expected retail stat line), then
    /// battle-load initialised (`FUN_80053CB8`). The opponent's stats are its
    /// monster record's boosted battle profile (`FUN_80054CB0`). Both round
    /// budgets seed from the fighters' AGL - the `+0x154` pool the dome's
    /// budget `ctx+0x6dc` reads. Returns `false` when the tables or the
    /// monster record don't resolve.
    pub fn muscle_start_vs(
        &mut self,
        char_slot: u32,
        level: u32,
        monster_id: u16,
        seed: u32,
    ) -> bool {
        self.muscle = None;
        let Some(tables) = self.muscle_tables.as_ref() else {
            return false;
        };
        let hand = tables.hand;
        let char_slot = (char_slot as usize).min(2);
        let Some((opponent, opp_name)) = self.muscle_monster_fighter(monster_id) else {
            return false;
        };
        let (player, player_name, stats_source) = match self.muscle_player_fighter(char_slot, level)
        {
            Some((f, n)) => (f, n, "disc"),
            None => (
                // No SCUS (raw PROT.DAT load): documented fallback
                // constants, surfaced as "fallback" in the state JSON.
                MuscleFighter {
                    hp_max: 500,
                    mp_max: 50,
                    budget_pool: 120,
                    int: 60,
                    udf: 40,
                    ldf: 40,
                    element: ELEMENT_NEUTRAL,
                },
                ["Vahn", "Noa", "Gala"][char_slot].to_string(),
                "fallback",
            ),
        };
        let player_costs = self
            .muscle_swing_costs(char_slot)
            .unwrap_or([FAVORED_COST; 4]);
        let player_hand = std::array::from_fn(|i| MuscleCard {
            command_id: hand[i],
            cost: player_costs[i],
        });
        // The opponent plays the same deck at the favored flat cost (a
        // monster has no player battle file to read swing costs from).
        let opp_hand = std::array::from_fn(|i| MuscleCard {
            command_id: hand[i],
            cost: FAVORED_COST,
        });
        let hp = [player.hp_max as i32, opponent.hp_max as i32];
        let mut session = MuscleDomeSession::new(
            player_hand,
            opp_hand,
            [player.budget_pool, opponent.budget_pool],
            hp,
            WEB_REWARD_SERU,
        );
        // Damage resolves through the shared retail kernel - the same
        // `DomeDamageModel` the native play-window host installs, so neither
        // host carries a damage rule of its own.
        session.install_damage_model(DomeDamageModel::new(
            tables.move_power.clone(),
            tables.move_map,
            tables.affinity.clone(),
            [player.combatant(), opponent.combatant()],
            hp,
            seed,
        ));
        self.muscle = Some(MuscleContest {
            session,
            fighters: [player, opponent],
            names: [player_name, opp_name],
            stats_source,
            monster_id,
            char_slot,
            level,
        });
        true
    }

    /// Commit one of the player's four hand cards (0..4) into the action queue,
    /// debiting the budget. Returns `false` when it can't be committed
    /// (overspend, queue full, or outside the selection phase).
    pub fn muscle_commit(&mut self, card_slot: usize) -> bool {
        self.muscle
            .as_mut()
            .is_some_and(|c| c.session.commit_card(0, card_slot))
    }

    /// Run the opponent's greedy in-order commit (the host AI model), then
    /// close the selection phase so the round is ready to resolve.
    pub fn muscle_end_selection(&mut self) {
        if let Some(c) = self.muscle.as_mut() {
            c.session.ai_commit_all(1);
            c.session.end_selection();
        }
    }

    /// Play the turn out through the shared retail damage kernel
    /// ([`legaia_engine_core::muscle_dome::DomeDamageModel`], installed at
    /// [`Self::muscle_start_vs`]) - the player's whole queued command string,
    /// then the opponent's, not interleaved. The native play-window host
    /// resolves through the same kernel; this method holds no damage rule of
    /// its own. No-op unless the turn is in the resolve phase.
    pub fn muscle_resolve(&mut self) {
        if let Some(c) = self.muscle.as_mut() {
            c.session.resolve_turn_retail();
        }
    }

    /// Take the next turn after a non-terminal resolution: reseed budgets,
    /// clear queues. No-op unless the contest is at a turn break - only a KO
    /// closes the leg for good.
    pub fn muscle_next_turn(&mut self) {
        if let Some(c) = self.muscle.as_mut() {
            c.session.next_turn();
        }
    }

    /// The last resolved turn's play-by-play, for the page's 3D playback:
    ///
    /// ```json
    /// [ { "attacker": 0, "cmd": 12, "power": 10, "damage": 55,
    ///     "hp": [500, 345] }, ... ]
    /// ```
    pub fn muscle_round_log_json(&self) -> String {
        let Some(c) = self.muscle.as_ref() else {
            return "[]".to_string();
        };
        let plays: Vec<serde_json::Value> = c
            .session
            .last_turn_plays()
            .iter()
            .map(|p| {
                serde_json::json!({
                    "attacker": p.attacker,
                    "cmd": p.cmd,
                    "power": p.power,
                    "damage": p.damage,
                    "hp": p.hp_after,
                })
            })
            .collect();
        serde_json::Value::Array(plays).to_string()
    }

    /// Live contest state: `live`, `phase` (`select` / `resolve` /
    /// `turn_over` / `won` / `lost`), `hp`, `hp_max`, `mp_max`,
    /// `budget`, `spent`, `queue`, `last_damage`, `hand`, `reward_spell`,
    /// `names`, `spirit` (the `+0x170` gauges the dome HUD bars display),
    /// `stats` (per-fighter INT/UDF/LDF/element the formulas used), `source`
    /// (`"disc"` / `"fallback"` player record), `char`, `level`, `monster`.
    ///
    /// `turn` is the battle turn counter and is **not** accompanied by a
    /// remaining-turns field: a dome leg is an ordinary battle and ends on a
    /// KO. `hp_left` is the opponent's HP percentage (the number retail
    /// stamps at x=`0xd2`), `hp_left_pct` carries both fighters' percentages
    /// for the page's bars, and `time_meter` / `time_meter_max` mirror the
    /// `FUN_801d3444` ramp.
    pub fn muscle_state_json(&self) -> String {
        let Some(c) = self.muscle.as_ref() else {
            return r#"{"live":false}"#.to_string();
        };
        let s = &c.session;
        let phase = match s.phase() {
            MusclePhase::Select => "select",
            MusclePhase::Resolve => "resolve",
            MusclePhase::TurnOver => "turn_over",
            MusclePhase::Won => "won",
            MusclePhase::Lost => "lost",
        };
        let hand: Vec<serde_json::Value> = s
            .hand(0)
            .iter()
            .map(|card| serde_json::json!({ "cmd": card.command_id, "cost": card.cost }))
            .collect();
        let stats: Vec<serde_json::Value> = c
            .fighters
            .iter()
            .map(|f| {
                serde_json::json!({
                    "int": f.int, "udf": f.udf, "ldf": f.ldf,
                    "budget_pool": f.budget_pool, "element": f.element,
                })
            })
            .collect();
        serde_json::json!({
            "live": true,
            "phase": phase,
            "turn": s.turn(),
            "hp": [s.hp(0), s.hp(1)],
            "hp_max": [c.fighters[0].hp_max, c.fighters[1].hp_max],
            "mp_max": [c.fighters[0].mp_max, c.fighters[1].mp_max],
            "budget": [s.budget(0), s.budget(1)],
            "spent": [s.spent(0), s.spent(1)],
            "hp_left": s.hp_left(),
            "hp_left_pct": [s.hp_left_percent(0), s.hp_left_percent(1)],
            "time_meter": s.time_meter(),
            "time_meter_max": legaia_engine_core::muscle_dome::TIME_METER_MAX,
            "queue": [s.queue(0), s.queue(1)],
            "last_damage": s.last_turn_damage(),
            "hand": hand,
            "reward_spell": s.reward_spell_id(),
            "names": c.names,
            "spirit": [s.spirit(0), s.spirit(1)],
            "stats": stats,
            "source": c.stats_source,
            "char": c.char_slot,
            "level": c.level,
            "monster": c.monster_id,
        })
        .to_string()
    }

    /// Name of spell id `id` from the SCUS spell-name table (the table the
    /// dome's victory banner reads at `DAT_800754d0`). Empty when no
    /// executable was loaded (raw `PROT.DAT` input).
    pub fn muscle_spell_name(&self, id: u8) -> String {
        self.scus
            .as_ref()
            .and_then(|s| legaia_asset::spell_names::SpellNameTable::from_scus(s))
            .and_then(|t| t.name(id).map(str::to_owned))
            .unwrap_or_default()
    }

    /// The monster archive roster, for the page's opponent picker:
    ///
    /// ```json
    /// [ { "id": 1, "name": "Gimard", "hp": 43, "agl": 60, "atk": 15,
    ///     "udf": 14, "ldf": 14, "int": 8, "spd": 12, "element": 2 }, ... ]
    /// ```
    ///
    /// Stats are the boosted battle profile (`battle_stats()`), i.e. the
    /// numbers the contest actually fights with. Only records with a
    /// decodable mesh + idle animation are listed (the dome renders its
    /// opponent in 3D). Names are the archive's own.
    pub fn muscle_roster_json(&self) -> String {
        let Some(entry) = self.monster_archive_entry() else {
            return "[]".to_string();
        };
        let Ok(records) = monster_archive::records(entry) else {
            return "[]".to_string();
        };
        let rows: Vec<serde_json::Value> = records
            .iter()
            .filter(|r| {
                r.hp > 0
                    && matches!(monster_archive::mesh(entry, r.id), Ok(Some(_)))
                    && matches!(monster_archive::idle_animation(entry, r.id), Ok(Some(_)))
            })
            .map(|r| {
                let bs = r.battle_stats();
                serde_json::json!({
                    "id": r.id, "name": r.name, "hp": r.hp,
                    "agl": bs[0], "atk": bs[1], "udf": bs[2], "ldf": bs[3],
                    "int": bs[4], "spd": bs[5], "element": r.element,
                })
            })
            .collect();
        serde_json::Value::Array(rows).to_string()
    }

    // ------------------------------------------------------------- dome 3D
    //
    // The opponent is the monster archive's own mesh + rigid-part animation
    // set (PROT 867), relocated to battle texture slot 0 exactly as the
    // battle loader does (`battle_render_mesh`, FUN_80055468); the player
    // fighter is the character's **assembled battle form** - retail fields
    // the party's normal fighter forms in the dome, not the Baka pack - the
    // same `battle_char_assembly` chain the arts viewer and the native
    // battles use: player battle file (PROT 863+char) equipment-id sections
    // assembled + TSB/CBA-relocated to band 0, posed from the file's own
    // record[0] action streams and per-command swing records. `muscle_vram`
    // merges the character's band-0 texture pool + battle palette with the
    // monster's texture pool so one TmdRenderer VRAM serves both bodies.

    /// Whether the dome's 3D scene decodes for `(monster_id, char_slot)`:
    /// the character's assembled battle form plus the monster's mesh + idle
    /// animation.
    pub fn muscle_scene_ready(&self, monster_id: u16, char_slot: u32) -> bool {
        let monster_ok = self.monster_archive_entry().is_some_and(|e| {
            matches!(monster_archive::mesh(e, monster_id), Ok(Some(_)))
                && matches!(monster_archive::idle_animation(e, monster_id), Ok(Some(_)))
        });
        monster_ok && self.muscle_fighter_build(char_slot).is_some()
    }

    /// Per-vertex positions of the character's assembled battle-form mesh
    /// (flat `f32`, 3 per vertex). Empty when the player file doesn't
    /// assemble on this image.
    pub fn muscle_fighter_positions(&self, char_slot: u32) -> Vec<f32> {
        let Some((_, mesh, _)) = self.muscle_fighter_build(char_slot) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.positions.len() * 3);
        for p in &mesh.positions {
            out.extend_from_slice(&[p[0], p[1], p[2]]);
        }
        out
    }

    /// Per-vertex `[u, v]` texel coords, parallel to the positions.
    pub fn muscle_fighter_uvs(&self, char_slot: u32) -> Vec<i32> {
        let Some((_, mesh, _)) = self.muscle_fighter_build(char_slot) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.uvs.len() * 2);
        for uv in &mesh.uvs {
            out.extend_from_slice(&[uv[0] as i32, uv[1] as i32]);
        }
        out
    }

    /// Per-vertex `[cba, tsb]` (band-0 relocated), parallel to the positions.
    pub fn muscle_fighter_cba_tsb(&self, char_slot: u32) -> Vec<u32> {
        let Some((_, mesh, _)) = self.muscle_fighter_build(char_slot) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.cba_tsb.len() * 2);
        for ct in &mesh.cba_tsb {
            out.extend_from_slice(&[ct[0] as u32, ct[1] as u32]);
        }
        out
    }

    /// Triangle indices of the assembled battle-form mesh.
    pub fn muscle_fighter_indices(&self, char_slot: u32) -> Vec<u32> {
        self.muscle_fighter_build(char_slot)
            .map(|(_, m, _)| m.indices)
            .unwrap_or_default()
    }

    /// Per-vertex TMD object index (the rigid part a vertex hangs from).
    pub fn muscle_fighter_object_ids(&self, char_slot: u32) -> Vec<u32> {
        self.muscle_fighter_build(char_slot)
            .map(|(_, _, oids)| oids)
            .unwrap_or_default()
    }

    /// Per-vertex `[r, g, b, textured_flag]` - the assembled form draws
    /// fully textured (kept for parity with the monster API).
    pub fn muscle_fighter_flat_rgba(&self, char_slot: u32) -> Vec<u8> {
        self.muscle_fighter_build(char_slot)
            .map(|(_, m, _)| vec![255u8; m.positions.len() * 4])
            .unwrap_or_default()
    }

    /// Assembled TMD object count (pose rig width).
    pub fn muscle_fighter_part_count(&self, char_slot: u32) -> u32 {
        self.muscle_fighter_build(char_slot)
            .map(|(asm, _, _)| asm.anm_bones.len() as u32)
            .unwrap_or(0)
    }

    /// Every battle-form clip the dome page plays, in runtime action-slot
    /// order: `[{"slot":0,"rate":r,"frame_count":f}, ...]` for the idle
    /// (slot 0), the light flinch (slot 2, the head of the party
    /// hit-reaction map `[2,3,4,5,0xB]` `FUN_80053CB8` writes to
    /// `+0x1EF..`), the knockdown-family entry (slot 4) and the four
    /// per-command swings (slots `0xC..=0xF` - the card ids themselves).
    /// A slot whose stream doesn't decode is omitted.
    pub fn muscle_fighter_anims_json(&self, char_slot: u32) -> String {
        let rows: Vec<String> = [0u32, 2, 4, 0xC, 0xD, 0xE, 0xF]
            .iter()
            .filter_map(|&slot| {
                let a = self.muscle_fighter_clip(char_slot, slot)?;
                Some(format!(
                    r#"{{"slot":{},"rate":{},"frame_count":{}}}"#,
                    slot, a.rate, a.frame_count
                ))
            })
            .collect();
        format!("[{}]", rows.join(","))
    }

    /// Battle-form clip `slot`'s pose frames: per (frame, part) absolute
    /// `[tx, ty, tz, rx, ry, rz]` (PSX 4096-unit angles), expanded per
    /// assembled object and padded to `target_part_count` - the same pose
    /// layout every other site animator consumes.
    pub fn muscle_fighter_pose_frames(
        &self,
        char_slot: u32,
        slot: u32,
        target_part_count: u32,
    ) -> Vec<i32> {
        let Some(anim) = self.muscle_fighter_clip(char_slot, slot) else {
            return Vec::new();
        };
        let parts = (target_part_count as usize).max(anim.part_count);
        let mut out = Vec::with_capacity(anim.frame_count * parts * 6);
        for frame in &anim.frames {
            for p in 0..parts {
                match frame.get(p) {
                    Some(t) => out.extend_from_slice(&[
                        t.tx as i32,
                        t.ty as i32,
                        t.tz as i32,
                        t.rx as i32,
                        t.ry as i32,
                        t.rz as i32,
                    ]),
                    None => out.extend_from_slice(&[0; 6]),
                }
            }
        }
        out
    }

    /// Resolve the player's **committed card queue** through the character's
    /// real Tactical-Arts tables: the greedy longest-match walk of the
    /// runtime art recognizer (`legaia_art::recognize`), with the span each
    /// matched art covers so the page can time the retail arts banner over
    /// the playback:
    ///
    /// ```json
    /// [ { "name": "Tornado Flame", "kind": "hyper", "start": 0, "len": 3 } ]
    /// ```
    ///
    /// `start`/`len` index the player's committed queue (= the player's
    /// playback events in order). Directions + names come from the disc's
    /// own SCUS arts-name table (curated-table fallback on a raw `PROT.DAT`
    /// load); the kind label joins the curated arts table by exact direction
    /// sequence. Empty when no contest is live or nothing in the queue
    /// performs an art.
    pub fn muscle_round_arts_json(&self) -> String {
        let Some(c) = self.muscle.as_ref() else {
            return "[]".to_string();
        };
        // Card ids 0xC..=0xF are the direction-command ids; the art tables'
        // direction bytes are 1..=4 in the same L/R/D/U order.
        let input: Vec<ArtCommand> = c
            .session
            .queue(0)
            .iter()
            .filter_map(|&cmd| ArtCommand::from_byte(cmd.wrapping_sub(0xB)))
            .collect();
        if input.is_empty() {
            return "[]".to_string();
        }
        let catalog = self.muscle_art_catalog(c.char_slot);
        // Greedy longest-match, left to right, connectors skipped - the
        // recognizer's documented walk (REF: legaia_art::recognize::
        // recognize_art_sequence), tracked here with span positions.
        let mut out: Vec<String> = Vec::new();
        let mut i = 0usize;
        while i < input.len() {
            let mut best: Option<(usize, usize)> = None;
            for (idx, (_, _, cmds)) in catalog.iter().enumerate() {
                if cmds.is_empty() || !input[i..].starts_with(cmds) {
                    continue;
                }
                if best.is_none_or(|(_, len)| len < cmds.len()) {
                    best = Some((idx, cmds.len()));
                }
            }
            match best {
                Some((idx, len)) => {
                    let (name, kind, _) = &catalog[idx];
                    out.push(format!(
                        r#"{{"name":{},"kind":{},"start":{},"len":{}}}"#,
                        jstr(name),
                        jstr(kind),
                        i,
                        len
                    ));
                    i += len;
                }
                None => i += 1,
            }
        }
        format!("[{}]", out.join(","))
    }

    /// The retail Triangle-list rows for the contest fighter: the character's
    /// arts out of the disc's own SCUS arts-name table (`DAT_80075EC4` -
    /// name, `+2` AP byte, directional command string), the exact source the
    /// retail battle input's "Hyper Arts list" overlay draws its
    /// name / arrow-string / AP columns from. Rows:
    ///
    /// ```json
    /// [ { "name": "Slash Kick", "ap": 40, "dirs": [4,3,2], "kind": "hyper" } ]
    /// ```
    ///
    /// `dirs` are the art-table direction bytes (1 Left, 2 Right, 3 Down,
    /// 4 Up). Miracle rows (marker-only command strings) are skipped, as the
    /// retail list skips them. The curated gamedata table is the fallback on
    /// a raw `PROT.DAT` load (no AP column there - `ap` reads 0) and the
    /// source of the `kind` label in both cases. The page does not model
    /// arts *learning*, so every table row lists (disclosed on the page);
    /// retail gates rows on the character's learned-art constant.
    pub fn muscle_arts_list_json(&self) -> String {
        let Some(c) = self.muscle.as_ref() else {
            return "[]".to_string();
        };
        let ap_by_name: std::collections::HashMap<String, u8> = self
            .scus
            .as_deref()
            .and_then(legaia_art::arts_table::parse_from_scus)
            .map(|entries| entries.into_iter().map(|e| (e.name, e.ap)).collect())
            .unwrap_or_default();
        let rows: Vec<String> = self
            .muscle_art_catalog(c.char_slot)
            .into_iter()
            .filter(|(name, _, _)| !name.is_empty())
            .map(|(name, kind, cmds)| {
                let dirs: Vec<String> = cmds.iter().map(|c| c.as_byte().to_string()).collect();
                format!(
                    r#"{{"name":{},"ap":{},"dirs":[{}],"kind":{}}}"#,
                    jstr(&name),
                    ap_by_name.get(&name).copied().unwrap_or(0),
                    dirs.join(","),
                    jstr(kind)
                )
            })
            .collect();
        format!("[{}]", rows.join(","))
    }

    /// Whether the player's selection is exhausted (no dealt direction is
    /// affordable): the retail auto-end of the command input.
    pub fn muscle_selection_exhausted(&self) -> bool {
        self.muscle
            .as_ref()
            .is_some_and(|c| c.session.selection_exhausted(0))
    }

    /// The retail confirm menu's "Reselect" arm: throw the player's committed
    /// queue away and restore the turn budget.
    pub fn muscle_reset_selection(&mut self) {
        if let Some(c) = self.muscle.as_mut() {
            c.session.reset_selection(0);
        }
    }

    /// Advance the round **time meter** one frame by the frame delta `dt`,
    /// returning the bar sprite's new Y offset (`-0x92` empty, `+0xE` full).
    /// The counter climbs while the direction-entry phase runs and drains
    /// otherwise - retail gates the ramp on `ctx+6 == 0x50`, the entry phase,
    /// **not** on the playback. Returns `0` with no contest up.
    ///
    /// PORT: FUN_801d3444, through
    /// [`MuscleDomeSession::tick_time_meter`](legaia_engine_core::muscle_dome::MuscleDomeSession::tick_time_meter)
    pub fn muscle_tick_time_meter(&mut self, dt: u8) -> i32 {
        self.muscle
            .as_mut()
            .map(|c| c.session.tick_time_meter(dt) as i32)
            .unwrap_or(0)
    }

    fn muscle_monster_render_mesh(&self, monster_id: u16) -> Option<legaia_tmd::mesh::VramMesh> {
        let entry = self.monster_archive_entry()?;
        let mesh = monster_archive::mesh(entry, monster_id).ok()??;
        // Scratch VRAM: the geometry accessors only need the relocated
        // CBA/TSB; `muscle_vram` repeats the texture injection for real.
        let mut scratch = legaia_tim::Vram::new();
        mesh.battle_render_mesh(0, &mut scratch)
    }

    /// Per-vertex positions of monster `monster_id`'s battle mesh.
    pub fn muscle_monster_positions(&self, monster_id: u16) -> Vec<f32> {
        let Some(mesh) = self.muscle_monster_render_mesh(monster_id) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.positions.len() * 3);
        for p in &mesh.positions {
            out.extend_from_slice(&[p[0], p[1], p[2]]);
        }
        out
    }

    /// Per-vertex `[u, v]` texel coords, parallel to the positions.
    pub fn muscle_monster_uvs(&self, monster_id: u16) -> Vec<i32> {
        let Some(mesh) = self.muscle_monster_render_mesh(monster_id) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.uvs.len() * 2);
        for uv in &mesh.uvs {
            out.extend_from_slice(&[uv[0] as i32, uv[1] as i32]);
        }
        out
    }

    /// Per-vertex `[cba, tsb]` (battle-slot relocated), parallel to the
    /// positions.
    pub fn muscle_monster_cba_tsb(&self, monster_id: u16) -> Vec<u32> {
        let Some(mesh) = self.muscle_monster_render_mesh(monster_id) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.cba_tsb.len() * 2);
        for ct in &mesh.cba_tsb {
            out.extend_from_slice(&[ct[0] as u32, ct[1] as u32]);
        }
        out
    }

    /// Triangle indices of the monster's battle mesh.
    pub fn muscle_monster_indices(&self, monster_id: u16) -> Vec<u32> {
        self.muscle_monster_render_mesh(monster_id)
            .map(|m| m.indices)
            .unwrap_or_default()
    }

    /// Per-vertex `[r, g, b, textured_flag]` - monsters draw fully textured,
    /// so every vertex samples VRAM (kept for parity with the fighter API).
    pub fn muscle_monster_flat_rgba(&self, monster_id: u16) -> Vec<u8> {
        let Some(mesh) = self.muscle_monster_render_mesh(monster_id) else {
            return Vec::new();
        };
        vec![255u8; mesh.positions.len() * 4]
    }

    /// Per-vertex TMD object index (the rigid part a vertex hangs from).
    pub fn muscle_monster_object_ids(&self, monster_id: u16) -> Vec<u32> {
        let Some(entry) = self.monster_archive_entry() else {
            return Vec::new();
        };
        let Some(Some(mesh)) = monster_archive::mesh(entry, monster_id).ok() else {
            return Vec::new();
        };
        let Ok(tmd) = legaia_tmd::parse(mesh.tmd_bytes()) else {
            return Vec::new();
        };
        legaia_tmd::mesh::tmd_to_vram_mesh_with_object_ids(&tmd, mesh.tmd_bytes()).1
    }

    /// TMD object count (pose rig width) of the monster's mesh.
    pub fn muscle_monster_part_count(&self, monster_id: u16) -> u32 {
        let Some(entry) = self.monster_archive_entry() else {
            return 0;
        };
        let Some(Some(mesh)) = monster_archive::mesh(entry, monster_id).ok() else {
            return 0;
        };
        legaia_tmd::parse(mesh.tmd_bytes())
            .map(|t| t.objects.len() as u32)
            .unwrap_or(0)
    }

    /// Every decodable action animation of the monster, in action-table
    /// order: `[{"action_id":0,"rate":1,"part_count":P,"frame_count":F},…]`.
    /// `action_id` is the semantic tag (`0` idle, `2`/`3` hit reactions,
    /// `4` knockdown, `0x20`/`0x21` the attack family - see
    /// `docs/formats/monster-animation.md`); the array index is the handle
    /// for [`Self::muscle_monster_pose_frames`].
    pub fn muscle_monster_anims_json(&self, monster_id: u16) -> String {
        let Some(entry) = self.monster_archive_entry() else {
            return "[]".to_string();
        };
        let anims = match monster_archive::animations(entry, monster_id) {
            Ok(Some(a)) => a,
            _ => return "[]".to_string(),
        };
        let rows: Vec<serde_json::Value> = anims
            .iter()
            .map(|a| {
                serde_json::json!({
                    "action_id": a.action_id,
                    "rate": a.rate,
                    "part_count": a.part_count,
                    "frame_count": a.frame_count,
                })
            })
            .collect();
        serde_json::Value::Array(rows).to_string()
    }

    /// Monster action animation `index` decoded to absolute per-(frame, part)
    /// `[tx, ty, tz, rx, ry, rz]` (PSX 4096-unit angles), padded to
    /// `target_part_count` parts - the same pose-stream shape every other
    /// site animator consumes (`baka_anim_pose_frames` and siblings).
    pub fn muscle_monster_pose_frames(
        &self,
        monster_id: u16,
        index: u32,
        target_part_count: u32,
    ) -> Vec<i32> {
        let Some(entry) = self.monster_archive_entry() else {
            return Vec::new();
        };
        let anims = match monster_archive::animations(entry, monster_id) {
            Ok(Some(a)) => a,
            _ => return Vec::new(),
        };
        let Some(anim) = anims.get(index as usize) else {
            return Vec::new();
        };
        let parts = (target_part_count as usize).max(anim.part_count);
        let mut out = Vec::with_capacity(anim.frame_count * parts * 6);
        for frame in &anim.frames {
            for p in 0..parts {
                match frame.get(p) {
                    Some(t) => out.extend_from_slice(&[
                        t.tx as i32,
                        t.ty as i32,
                        t.tz as i32,
                        t.rx as i32,
                        t.ry as i32,
                        t.rz as i32,
                    ]),
                    None => out.extend_from_slice(&[0; 6]),
                }
            }
        }
        out
    }

    /// The dome duel's 1 MB PSX VRAM: the character's **battle-form texture
    /// pool** at the pinned band-0 placement (`FUN_80052FA0` uploads +
    /// the decoded battle palette overlaid on the CLUT rows the assembled
    /// mesh samples - the arts viewer's chain), plus monster `monster_id`'s
    /// texture pool injected at battle slot 0's coordinates (CLUT row 484,
    /// 4bpp page at `(320, 256)`) - the layout the retail battle loader
    /// builds - plus the arena backdrop's own TIM pages (PROT 1225 tail
    /// chunks: 4bpp pages at `(768, 0)` / `(832, 0)`, CLUT rows 473 / 479;
    /// all three bands are disjoint, so upload order doesn't matter).
    pub fn muscle_vram(&self, monster_id: u16, char_slot: u32) -> Vec<u8> {
        let mut vram = legaia_tim::Vram::new();
        if let Some(raw) = self.muscle_player_file(char_slot)
            && let Ok(pack) = legaia_asset::battle_data_pack::parse(raw)
        {
            if let Ok(uploads) = bca::character_texture_uploads(raw, &pack, &[0u8; 5], 0) {
                for u in &uploads {
                    vram.write_block(u.fb_x(), u.fb_y(), u.rect.w, u.rect.h, &u.pixels);
                    if !u.clut.is_empty() {
                        vram.write_clut_row(u.clut_x, u.clut_row(), &u.clut_bytes());
                    }
                }
            }
            // Battle palette on the CLUT rows/columns the relocated mesh
            // samples (Vahn = the byte-exact fixed-stride record parse; the
            // others = the equipment-robust collector).
            if let Some((_, mesh, _)) = self.muscle_fighter_build(char_slot) {
                let mut rows: Vec<u16> = mesh.cba_tsb.iter().map(|c| (c[0] >> 6) & 0x1FF).collect();
                rows.sort_unstable();
                rows.dedup();
                let mut cols: Vec<u16> = mesh.cba_tsb.iter().map(|c| (c[0] & 0x3F) * 16).collect();
                cols.sort_unstable();
                cols.dedup();
                let pal = if char_slot == 0 {
                    legaia_asset::battle_char_palette::find_record0(raw).and_then(|rec0| {
                        legaia_asset::battle_char_palette::parse_record(raw, rec0).ok()
                    })
                } else {
                    legaia_asset::battle_char_palette::collect_palette(raw, 0, &cols).ok()
                };
                if let Some(pal) = pal {
                    for &row in &rows {
                        for band in &pal.bands {
                            let bytes: Vec<u8> = band
                                .vram_words()
                                .iter()
                                .flat_map(|w| w.to_le_bytes())
                                .collect();
                            vram.write_clut_row(band.base, row, &bytes);
                        }
                    }
                }
            }
        }
        if let Some(entry) = self.monster_archive_entry()
            && let Ok(Some(mesh)) = monster_archive::mesh(entry, monster_id)
        {
            // Injects the pool at slot 0's CLUT row + page origin.
            let _ = mesh.battle_render_mesh(0, &mut vram);
        }
        if let Some(buf) = self.muscle_arena_entry() {
            for chunk in scene_tmd_stream::battle_tim_chunks(buf) {
                if let Some(bytes) = buf.get(chunk.payload_offset..)
                    && let Ok(tim) = legaia_tim::parse(bytes)
                {
                    vram.upload_tim(&tim);
                }
            }
        }
        vram.as_bytes().to_vec()
    }

    // -------------------------------------------------------- arena backdrop

    /// Status of the dome's arena backdrop (PROT 1225 - see
    /// [`ARENA_BACKDROP_PROT_INDEX`] for the pin):
    /// `{"ok":true,"prot":1225,"verts":N,"tris":N,"tims":2}`, or
    /// `{"ok":false}` when the entry is absent / doesn't match the
    /// scene_tmd_stream shape on this image.
    pub fn muscle_arena_json(&self) -> String {
        let Some((mesh, _)) = self.muscle_arena_hybrid() else {
            return r#"{"ok":false}"#.to_string();
        };
        let tims = self
            .muscle_arena_entry()
            .map(|b| scene_tmd_stream::battle_tim_chunks(b).len())
            .unwrap_or(0);
        format!(
            r#"{{"ok":true,"prot":{},"verts":{},"tris":{},"tims":{}}}"#,
            ARENA_BACKDROP_PROT_INDEX,
            mesh.positions.len(),
            mesh.triangle_count(),
            tims,
        )
    }

    /// Arena-shell vertex positions (`[x, y, z, ...]`, retail Y-down world
    /// coordinates, world-fixed - the shell is authored at `X >= 0` with the
    /// open side facing `-X`, and retail seats the fighters near the world
    /// origin). Empty when the backdrop doesn't decode.
    pub fn muscle_arena_positions(&self) -> Vec<f32> {
        let Some((mesh, _)) = self.muscle_arena_hybrid() else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.positions.len() * 3);
        for p in &mesh.positions {
            out.extend_from_slice(&[p[0], p[1], p[2]]);
        }
        out
    }

    /// Per-vertex `[u, v]` texel coords for the arena shell.
    pub fn muscle_arena_uvs(&self) -> Vec<i32> {
        let Some((mesh, _)) = self.muscle_arena_hybrid() else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.uvs.len() * 2);
        for uv in &mesh.uvs {
            out.extend_from_slice(&[uv[0] as i32, uv[1] as i32]);
        }
        out
    }

    /// Per-vertex `[cba, tsb]` for the arena shell (its TIMs' own authored
    /// VRAM addresses - no battle-slot relocation applies to a backdrop).
    pub fn muscle_arena_cba_tsb(&self) -> Vec<u32> {
        let Some((mesh, _)) = self.muscle_arena_hybrid() else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(mesh.cba_tsb.len() * 2);
        for ct in &mesh.cba_tsb {
            out.extend_from_slice(&[ct[0] as u32, ct[1] as u32]);
        }
        out
    }

    /// Triangle indices for the arena shell.
    pub fn muscle_arena_indices(&self) -> Vec<u32> {
        self.muscle_arena_hybrid()
            .map(|(m, _)| m.indices)
            .unwrap_or_default()
    }

    /// Per-vertex `[r, g, b, textured_flag]` for the arena's hybrid textured /
    /// vertex-colour render (same convention as the fighter bodies).
    pub fn muscle_arena_flat_rgba(&self) -> Vec<u8> {
        self.muscle_arena_hybrid()
            .map(|(_, flat)| flat)
            .unwrap_or_default()
    }

    // ------------------------------------------------------------- dome SFX

    /// The dome's pinned sound-cue rows and whether each decodes on this
    /// image:
    ///
    /// ```json
    /// { "ok": true,
    ///   "ui": [32, 33, 34],   // FUN_801d0748's own blips (call ids 0x21..0x23
    ///                         // through FUN_8004fcc8's id-1 leg; PROT 0868)
    ///   "hit": 9,             // shared battle/duel melee impact (PROT 0869)
    ///   "hit_voices": 2 }
    /// ```
    ///
    /// `ok` is false without a SCUS (raw `PROT.DAT` load - the descriptor
    /// table lives in the executable).
    pub fn muscle_sfx_json(&self) -> String {
        let ui: Vec<String> = MUSCLE_UI_CUE_CALL_IDS
            .iter()
            .map(|&id| (id - 1).to_string())
            .collect();
        let hit_ok = self.muscle_static_cue(MUSCLE_HIT_CUE_ROW, 0).is_some();
        let hit_voices = self
            .scus
            .as_ref()
            .and_then(|s| sfx_table::SfxTable::from_scus(s))
            .and_then(|t| t.get(MUSCLE_HIT_CUE_ROW).map(|d| d.voice_count()))
            .unwrap_or(0);
        format!(
            r#"{{"ok":{},"ui":[{}],"hit":{},"hit_voices":{}}}"#,
            hit_ok,
            ui.join(","),
            MUSCLE_HIT_CUE_ROW,
            hit_voices,
        )
    }

    /// Decode one voice layer of static SFX descriptor row `row` to mono i16
    /// PCM (the dome's cues are static-table rows - see
    /// [`Self::muscle_sfx_json`]). Empty when the row / voice / bank doesn't
    /// resolve.
    pub fn muscle_sfx_pcm(&self, row: u8, voice: u8) -> Vec<i16> {
        self.muscle_static_cue(row, voice)
            .map(|(pcm, _)| pcm)
            .unwrap_or_default()
    }

    /// Playback rate for [`Self::muscle_sfx_pcm`] (`0` when absent).
    pub fn muscle_sfx_rate(&self, row: u8, voice: u8) -> u32 {
        self.muscle_static_cue(row, voice)
            .map(|(_, rate)| rate)
            .unwrap_or(0)
    }
}

// ------------------------------------------------------------- retail HUD
//
// The dome presents as a standard battle, and its whole on-screen chrome is
// disc data reachable without a save: the chip/plate art + D-pad live in a
// boot-time TIM in the unindexed pre-`init_data` gap of PROT.DAT (pixels at
// VRAM (896, 256), CLUT bank packed into row 511 as 16 sub-palettes), the
// chip-label font is the gap's 256x256 ASCII TIM at (896, 0) (drawn through
// the menu-glyph atlas's CLUT bank on row 510, sub-palette 13), the small
// digits are the menu-glyph atlas itself at (960, 256), and the arts-banner
// words / big damage numerals / red cross-out X are the third TIM of the
// battle-effect bank `etim` (extraction 0870) at (448, 0) with CLUT row 476.
// The dome-hub art (the "Welcome to the Muscle Dome!" cursive, INTERVAL /
// ROUND headings, hub digit strip) is the two-TIM LZS payload of the dome's
// own data file (extraction 1220, `other6.lzs` slot 0), whose on-screen
// geometry is the PROT 0977 overlay's sprite descriptor table
// (`legaia_engine_ui::other_game_hud::parse_sprite_table`).
//
// Every piece rect below is capture-pinned: a live PCSX-Redux Muscle Dome
// battle (the `minigame_muscle_dome_pcsx` scenario driven into the match)
// was snapshotted at the command cluster, an enemy art, and a player HYPER
// ARTS!! playback, and the GP0 packet stream + VRAM were read out of the
// savestates (`scripts/pcsx-redux/autorun_muscle_hud_capture.lua`). The
// sprite geometry (screen anchors, glide endpoints, widths) additionally
// lives in the SCUS-static screen-element placement table at `0x80076C10`
// (24-byte stride; 80 of its records are the dome's HUD), exported raw below.
// That base carries one table under several historical names - see
// `docs/reference/memory-map.md`.

/// `PROT.DAT` file offset of the battle-chrome widget TIM (plates, D-pad,
/// AP-plate art, HP/MP badges) in the unindexed pre-`init_data` gap.
/// Uploads to VRAM (896, 256) 256x192; CLUT bank -> row 511.
const HUD_WIDGET_TIM_OFFSET: usize = 0x18E0;

/// `PROT.DAT` gap offset of the 256x256 ASCII battle font TIM -> (896, 0).
const HUD_FONT_TIM_OFFSET: usize = 0x7F40;

/// `PROT.DAT` gap offset of the menu-glyph atlas TIM -> (960, 256); its
/// CLUT bank packs into VRAM row 510 (16 sub-palettes).
const HUD_MENU_ATLAS_TIM_OFFSET: usize = 0x11218;

/// PROT entry of the battle-effect TIM bank (`etim`, `befect_data`).
const HUD_ETIM_PROT_INDEX: u32 = 870;

/// Offset of `etim`'s third TIM - the arts-banner / damage-numeral / red-X
/// page -> VRAM (448, 0), CLUT row 476.
const HUD_BANNER_TIM_OFFSET: usize = 0x10450;

/// PROT entry (extraction space) of the dome data container
/// (`other6.lzs` slot 0): LZS section 0 carries the two hub-page TIMs.
const HUD_HUB_CONTAINER_PROT_INDEX: u32 = 1220;

/// `PROT.DAT` gap offset of the small pad-button-glyph TIM (the four
/// button circles + the R1/R2/L1/L2 labels): image -> VRAM (928, 352)
/// (page (896,256) local texels (128,96)..(192,128)), own 16-entry CLUT ->
/// (304, 511). The arts-input caption's green Triangle circle is its local
/// rect (48, 0, 16, 16) - the recomp GP0 capture of the input screen draws
/// it as `uv (176,96) clut (304,511)` on the widget page.
const HUD_BUTTON_TIM_OFFSET: usize = 0x7B00;

/// The arts command-input piece rects (recomp GP0 packet capture of a live
/// dome input screen + Triangle arts list; every rect and palette index is
/// byte-read out of the captured SPRT/FT4/shaded-quad words -
/// `docs/subsystems/minigame-muscle-dome.md` "Arts command input"). Split
/// out of [`LegaiaMinigames::muscle_hud_json`]'s `json!` so the macro stays
/// under the recursion limit.
fn arts_input_pieces() -> serde_json::Value {
    serde_json::json!({
        "cmd_chip": {"body": [215,96,24,26], "cap_l": [200,96,15,26],
                      "cap_r": [239,96,15,26], "pal": 6},
        "cmd_label": {"u": 104, "w": 24, "h": 18, "pal": 5,
                       "v": {"high": 104, "left": 20, "right": 84,
                             "low": 40, "arms": 0, "raseru": 64}},
        "chip_diamond_l": {"r": [192,24,9,18], "pal": 5},
        "chip_diamond_r": {"r": [204,24,9,18], "pal": 5},
        "pennant_cap_l": {"r": [192,24,9,18], "pal": 5},
        "pennant_cap_r": {"r": [216,24,9,18], "pal": 5},
        "bar_end_l": {"r": [240,0,16,18], "pal": 6},
        "bar_body": {"r": [224,0,16,18], "pal": 6},
        "bar_arrow": {"r": [192,44,18,18], "pal": 6},
        "list_win": {"interior": [128,0,32,32], "edge_top": [164,0,24,4],
                      "edge_bottom": [164,28,24,4], "edge_l": [160,4,4,24],
                      "edge_r": [188,4,4,24], "corner_tl": [160,0,4,4],
                      "corner_tr": [188,0,4,4], "corner_bl": [160,28,4,4],
                      "corner_br": [188,28,4,4], "pal": 2,
                      "grad": [0x40, 0x88]},
        "arts_arrows": {"v": 208, "w": 12, "h": 12, "pal": 15,
                         "u": {"up": 208, "down": 220, "right": 232,
                               "left": 244}},
        "arts_text_pal": 15,
        "tri_button": {"r": [48,0,16,16]},
        "ap_input_fill": {"rect": [235,177,50,6],
                           "rgb": [[192,160,64],[128,32,16]]},
    })
}

/// SCUS VA of the screen-element placement table (24-byte stride); the
/// dome's HUD is the first 80 records.
const HUD_ELEMENT_TABLE_VA: u32 = 0x8007_6C10;

/// Element records in the table.
const HUD_ELEMENT_COUNT: usize = 80;

/// Sub-palette (of the menu-glyph atlas CLUT bank) the battle font and the
/// small digits draw through (capture: SPRT clut word `0x7F8D` = row 510,
/// x 208 = bank sub-palette 13).
const HUD_TEXT_SUB_PALETTE: usize = 13;

impl LegaiaMinigames {
    /// Parse a plain TIM out of the raw `PROT.DAT` image at `offset`
    /// (the boot-gap system TIMs are not PROT entries).
    fn hud_gap_tim(&self, offset: usize) -> Option<legaia_tim::Tim> {
        legaia_tim::parse(self.prot.get(offset..)?).ok()
    }

    /// The `etim` banner TIM (third TIM of PROT 0870).
    fn hud_banner_tim(&self) -> Option<legaia_tim::Tim> {
        let entry = entry_bytes(&self.prot, &self.entries, HUD_ETIM_PROT_INDEX)?;
        legaia_tim::parse(entry.get(HUD_BANNER_TIM_OFFSET..)?).ok()
    }

    /// The two dome-hub page TIMs out of the extraction-1220 LZS container
    /// (section 0 = `[u32 tag][u32 count][u32 size]` + TIM at `0xC` + TIM
    /// immediately after).
    fn hud_hub_tims(&self) -> Option<(legaia_tim::Tim, legaia_tim::Tim)> {
        let entry = entry_bytes(&self.prot, &self.entries, HUD_HUB_CONTAINER_PROT_INDEX)?;
        let sections = legaia_lzs::decompress_container(entry).ok()?;
        let blob = sections.first()?;
        let t0 = legaia_tim::parse(blob.get(0xC..)?).ok()?;
        let t1 = legaia_tim::parse(blob.get(0xC + t0.byte_extent()..)?).ok()?;
        Some((t0, t1))
    }

    /// Decode a 4bpp TIM through 16-colour palette `pal` to RGBA8
    /// (texel index 0 = transparent, everything else opaque).
    fn hud_tim_rgba(tim: &legaia_tim::Tim, pal: &[u16]) -> Vec<u8> {
        let w = tim.pixel_width();
        let h = tim.pixel_height();
        let mut out = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let byte = tim.image.data[y * (w / 2) + x / 2];
                let idx = if x % 2 == 0 { byte & 0xF } else { byte >> 4 } as usize;
                if idx == 0 {
                    continue;
                }
                let c = legaia_tim::bgr555_to_rgba8(*pal.get(idx).unwrap_or(&0));
                let o = (y * w + x) * 4;
                out[o..o + 3].copy_from_slice(&c[..3]);
                out[o + 3] = 255;
            }
        }
        out
    }

    /// The SCUS battle HUD element table rows, or empty without a SCUS.
    fn hud_elements_json(&self) -> Vec<serde_json::Value> {
        let Some(scus) = self.scus.as_deref() else {
            return Vec::new();
        };
        if scus.len() < 0x20 || &scus[0..8] != b"PS-X EXE" {
            return Vec::new();
        }
        let t_addr = u32::from_le_bytes(scus[0x18..0x1C].try_into().unwrap());
        let Some(off) = (HUD_ELEMENT_TABLE_VA.checked_sub(t_addr))
            .map(|d| d as usize + 0x800)
            .filter(|o| o + HUD_ELEMENT_COUNT * 24 <= scus.len())
        else {
            return Vec::new();
        };
        let i16_at = |p: usize| i16::from_le_bytes(scus[p..p + 2].try_into().unwrap());
        (0..HUD_ELEMENT_COUNT)
            .map(|i| {
                let r = off + i * 24;
                serde_json::json!({
                    "id": i,
                    "spr": [scus[r], scus[r + 1]],
                    "a": [i16_at(r + 2), i16_at(r + 4)],
                    "w": i16_at(r + 6), "h": i16_at(r + 8),
                    "b": [i16_at(r + 10), i16_at(r + 12)],
                    "style": [scus[r + 0xE], scus[r + 0xF]],
                    "kind": scus[r + 0x10],
                })
            })
            .collect()
    }

    /// Per-character advance widths of the ASCII battle font. The pen
    /// advance retail uses equals the glyph's occupied texel width for every
    /// character measured in the captured chip-label runs (`Begin`, `Carl`,
    /// `Attack`, `Item`, `Spirit`, `Meta`, `Run`, `Ironman`, `Fire Blow`,
    /// `Auto`, `Command`) except three - `i`, `m`, `M` advance one texel
    /// wider - and the space advances 5. A retail width *table* was not
    /// found statically (SCUS + battle overlay byte-scanned), so this is
    /// texel-derived with the capture-measured exceptions baked in.
    fn hud_font_advances(&self) -> Vec<u8> {
        let Some(tim) = self.hud_gap_tim(HUD_FONT_TIM_OFFSET) else {
            return Vec::new();
        };
        let w = tim.pixel_width();
        let mut out = Vec::with_capacity(96);
        for ch in 0..96usize {
            let (cx, cy) = ((ch % 16) * 16, (ch / 16) * 16);
            let mut maxc = 0usize;
            for y in 0..16 {
                for x in 0..16 {
                    let (px, py) = (cx + x, cy + y);
                    let byte = tim.image.data[py * (w / 2) + px / 2];
                    let idx = if px % 2 == 0 { byte & 0xF } else { byte >> 4 };
                    if idx != 0 && x + 1 > maxc {
                        maxc = x + 1;
                    }
                }
            }
            let adv = match (ch + 0x20) as u8 {
                b' ' => 5,
                b'i' | b'm' | b'M' => (maxc + 1).min(16),
                _ if maxc == 0 => 5,
                _ => maxc.min(16),
            };
            out.push(adv as u8);
        }
        out
    }
}

#[wasm_bindgen]
impl LegaiaMinigames {
    /// The live contest's **victory banner**, composed the way retail
    /// composes it and resolved to real strings.
    ///
    /// Retail assembles three pieces into the battle context's text buffer
    /// (`ctx + 0x1F9`): the winning fighter's lead-in line out of the
    /// victory-message pointer table at `0x801F4DFC` indexed `char_id - 1`,
    /// the reward spell's name from the shared spell-name table, and a fixed
    /// suffix at `0x801F4C28`. The index half is
    /// [`MuscleDomeSession::reward_banner`](legaia_engine_core::muscle_dome::MuscleDomeSession::reward_banner);
    /// this resolves the two overlay strings off the as-loaded PROT 0898
    /// image and the spell name off `SCUS_942.54`.
    ///
    /// `{"ok":true,"lead_in_index":n,"lead_in":"…","spell_id":n,
    /// "spell":"…","suffix":"…","text":"…"}` - `text` is the three joined in
    /// retail's order. `ok` is false with no live contest.
    pub fn muscle_reward_banner_json(&self) -> String {
        let Some(contest) = self.muscle.as_ref() else {
            return r#"{"ok":false}"#.to_string();
        };
        // Retail's `DAT_8007BD10[slot]` is the 1-based character id.
        let char_id = contest.char_slot as u8 + 1;
        let banner = contest.session.reward_banner(char_id);
        let loaded = overlay_image(
            &self.prot,
            &self.entries,
            md::MUSCLE_OVERLAY_PROT_INDEX as u32,
        );
        let lead_in = loaded
            .as_deref()
            .and_then(|img| {
                overlay_string(
                    img,
                    overlay_u32(
                        img,
                        md::VICTORY_MSG_TABLE_VA + banner.lead_in_index as u32 * 4,
                    )?,
                )
            })
            .unwrap_or_default();
        let suffix = loaded
            .as_deref()
            .and_then(|img| {
                overlay_string(
                    img,
                    legaia_engine_vm::battle_cast_dispatch::BANNER_SUFFIX_VA,
                )
            })
            .unwrap_or_default();
        let spell = self.muscle_spell_name(banner.spell_id as u8);
        serde_json::json!({
            "ok": true,
            "lead_in_index": banner.lead_in_index,
            "lead_in": lead_in,
            "spell_id": banner.spell_id,
            "spell": spell,
            "suffix": if banner.suffix { suffix.clone() } else { String::new() },
            "text": format!("{lead_in}{spell}{}", if banner.suffix { suffix } else { String::new() }),
        })
        .to_string()
    }

    /// The arena's **course ladder**, straight off the disc.
    ///
    /// PROT 0977 carries a 3-entry course descriptor table (`0x801D1A08`,
    /// `{ i32 rounds; ptr first }`) over a run of 29
    /// `{ u32 label_va; u32 monster_id }` round records (`0x801D1920`), and
    /// `FUN_801D1510` stores the round's `monster_id` into formation slot 0
    /// at `0x8007BD0C` - so the arena's opponent is an ordinary battle
    /// monster with an ordinary PROT 867 record.
    ///
    /// Rows: `{ "course": c, "rounds": [{ "round": 1-based, "id": monster
    /// id, "name": archive name, "hp": archive HP, "score": the score cell
    /// clearing it adds }] }`. `name`/`hp` are null when the archive slot
    /// does not decode.
    pub fn muscle_course_ladder_json(&self) -> String {
        use legaia_engine_core::muscle_dome as md;
        let Some(raw) = entry_bytes(&self.prot, &self.entries, 977) else {
            return "[]".to_string();
        };
        let Some(ladder) = md::parse_course_ladder(raw) else {
            return "[]".to_string();
        };
        let archive = self.monster_archive_entry();
        let rows: Vec<serde_json::Value> = ladder
            .iter()
            .enumerate()
            .map(|(c, course)| {
                let rounds: Vec<serde_json::Value> = course
                    .rounds
                    .iter()
                    .enumerate()
                    .map(|(r, round)| {
                        let rec = archive.and_then(|a| {
                            monster_archive::record(a, round.monster_id as u16)
                                .ok()
                                .flatten()
                        });
                        serde_json::json!({
                            "round": r + 1,
                            "id": round.monster_id,
                            "name": rec.as_ref().map(|m| m.name.clone()),
                            "hp": rec.as_ref().map(|m| m.hp),
                            "score": md::course_score_cell(raw, c, r as u32 + 1),
                        })
                    })
                    .collect();
                serde_json::json!({ "course": c, "rounds": rounds })
            })
            .collect();
        serde_json::Value::Array(rows).to_string()
    }

    /// One PROT 0977 **hub screen** as retail-placed quads.
    ///
    /// `screen`: 0 = intro card, 1 = course-title art, 2 = INTERVAL
    /// heading, 3 = ROUND banner (`round` is the displayed number),
    /// 4 = the six-row score tally. `brightness` is the emitter's colour
    /// scale (`0x100` = the record's own colour).
    ///
    /// Every row's screen rect comes out of the retail emitters
    /// ([`legaia_engine_ui::other_game_hud::hub_screen_quads`]) fed the draw
    /// list recovered from that entry's own call sites, so the page places
    /// nothing itself: `x`/`y`/`dw`/`dh` are the quad's, `u`/`v`/`w`/`h`
    /// its texels, `sheet` 4/5 the hub page and `pal` the row sub-palette.
    pub fn muscle_hub_quads_json(&self, screen: u32, round: i32, brightness: i32) -> String {
        use legaia_engine_ui::other_game_hud as hud;
        let Some(raw) = entry_bytes(&self.prot, &self.entries, 977) else {
            return r#"{"ok":false}"#.to_string();
        };
        let mut table = hud::parse_sprite_table(raw);
        if table.is_empty() {
            return r#"{"ok":false}"#.to_string();
        }
        let quads = match screen {
            0 => hud::hub_screen_quads(&mut table, hud::HUB_INTRO_CARD, brightness),
            1 => hud::hub_screen_quads(&mut table, hud::HUB_TITLE_ART, hud::TITLE_ART_BRIGHTNESS),
            2 => hud::hub_screen_quads(&mut table, hud::HUB_INTERVAL_HEADING, brightness),
            3 => hud::hub_screen_quads(&mut table, &hud::round_banner_draws(round), brightness),
            _ => hud::score_tally_quads(
                &mut table,
                [round, 0, 0, 0, 0, 0],
                [brightness; hud::SCORE_TALLY_ROWS],
            ),
        };
        let rows: Vec<serde_json::Value> = quads
            .iter()
            .map(|q| {
                serde_json::json!({
                    // tpage bit 4 is the VRAM Y base; the emitter's page
                    // byte lands in the ABR field, so it never disturbs it.
                    "sheet": if q.tpage & 0x10 != 0 { 5 } else { 4 },
                    "pal": q.clut & 0x3F,
                    "u": q.uv[0].0, "v": q.uv[0].1,
                    "w": q.uv[1].0 as i32 - q.uv[0].0 as i32 + 1,
                    "h": q.uv[2].1 as i32 - q.uv[0].1 as i32 + 1,
                    "x": q.xy[0].0, "y": q.xy[0].1,
                    "dw": q.xy[1].0 as i32 - q.xy[0].0 as i32 + 1,
                    "dh": q.xy[2].1 as i32 - q.xy[0].1 as i32 + 1,
                    "semi": q.semi_transparent,
                })
            })
            .collect();
        serde_json::json!({ "ok": true, "quads": rows }).to_string()
    }

    /// One HUD sprite sheet decoded to RGBA8 (row-major, texel index 0
    /// transparent). `source`: 0 = battle-chrome widget page (own CLUT-bank
    /// sub-palette `palette`), 1 = ASCII battle font (through the
    /// menu-glyph atlas bank sub-palette `palette`), 2 = menu-glyph atlas,
    /// 3 = `etim` banner page (CLUT-row sub-palette), 4/5 = dome hub pages
    /// (320,0)/(320,256), 6 = the pad-button-glyph TIM (own CLUT). Empty
    /// when the source doesn't decode on this image. Sheet dimensions ride
    /// in [`Self::muscle_hud_json`].
    pub fn muscle_hud_sheet_rgba(&self, source: u32, palette: u32) -> Vec<u8> {
        let pal_idx = palette as usize;
        let (tim, pal_tim) = match source {
            0 => {
                let t = self.hud_gap_tim(HUD_WIDGET_TIM_OFFSET);
                (t.clone(), t)
            }
            1 => (
                self.hud_gap_tim(HUD_FONT_TIM_OFFSET),
                self.hud_gap_tim(HUD_MENU_ATLAS_TIM_OFFSET),
            ),
            2 => {
                let t = self.hud_gap_tim(HUD_MENU_ATLAS_TIM_OFFSET);
                (t.clone(), t)
            }
            3 => {
                let t = self.hud_banner_tim();
                (t.clone(), t)
            }
            4 => {
                let t = self.hud_hub_tims().map(|(a, _)| a);
                (t.clone(), t)
            }
            5 => {
                let t = self.hud_hub_tims().map(|(_, b)| b);
                (t.clone(), t)
            }
            6 => {
                let t = self.hud_gap_tim(HUD_BUTTON_TIM_OFFSET);
                (t.clone(), t)
            }
            _ => (None, None),
        };
        let (Some(tim), Some(pal_tim)) = (tim, pal_tim) else {
            return Vec::new();
        };
        let Some(pal) = pal_tim
            .clut
            .as_ref()
            .and_then(|c| c.entries.get(pal_idx * 16..pal_idx * 16 + 16))
        else {
            return Vec::new();
        };
        Self::hud_tim_rgba(&tim, pal)
    }

    /// The retail-HUD description the page renders from: sheet dimensions,
    /// the capture-pinned piece rects (chips, plates, D-pad, red X, AP
    /// plate, HP/MP badges, arts-banner strips, damage numerals), the
    /// SCUS element-table rows (screen anchors + glide endpoints), the
    /// PROT 0977 hub sprite records, and the font advance table. `ok` is
    /// false when the chrome TIMs don't decode on this image.
    pub fn muscle_hud_json(&self) -> String {
        let widget = self.hud_gap_tim(HUD_WIDGET_TIM_OFFSET);
        let font = self.hud_gap_tim(HUD_FONT_TIM_OFFSET);
        let atlas = self.hud_gap_tim(HUD_MENU_ATLAS_TIM_OFFSET);
        let banner = self.hud_banner_tim();
        let button = self.hud_gap_tim(HUD_BUTTON_TIM_OFFSET);
        if widget.is_none() || font.is_none() || atlas.is_none() {
            return r#"{"ok":false}"#.to_string();
        }
        let dims = |t: &Option<legaia_tim::Tim>| {
            t.as_ref()
                .map(|t| serde_json::json!([t.pixel_width(), t.pixel_height()]))
                .unwrap_or(serde_json::Value::Null)
        };
        let hub = self.hud_hub_tims();
        let hub_sprites: Vec<serde_json::Value> = entry_bytes(&self.prot, &self.entries, 977)
            .map(legaia_engine_ui::other_game_hud::parse_sprite_table)
            .unwrap_or_default()
            .iter()
            .enumerate()
            .map(|(i, s)| {
                serde_json::json!({
                    "i": i,
                    // tpage 0x0005 -> hub page 0 (VRAM (320,0)),
                    // 0x0015 -> hub page 1 ((320,256)).
                    "sheet": if s.tpage & 0x10 != 0 { 5 } else { 4 },
                    // CLUT word: bits 0..5 = x/16 (the row sub-palette).
                    "pal": s.clut & 0x3F,
                    "uv": [s.u0, s.v0], "wh": [s.w, s.h],
                    "semi": s.semi_transparent,
                })
            })
            .collect();
        serde_json::json!({
            "ok": true,
            "sheets": {
                "widget": dims(&widget), "font": dims(&font),
                "atlas": dims(&atlas), "banner": dims(&banner),
                "hub0": dims(&hub.as_ref().map(|(a, _)| a.clone())),
                "hub1": dims(&hub.as_ref().map(|(_, b)| b.clone())),
                "button": dims(&button),
            },
            // Capture-pinned piece rects: [u, v, w, h] on the named sheet,
            // "pal" = the sub-palette observed in the live packets.
            "pieces": {
                "plate_blue": {"cap_l": [208,0,8,20], "body": [192,0,16,20],
                                "cap_r": [216,0,8,20], "pal": 4},
                "plate_gold": {"cap_l": [208,64,8,20], "body": [192,64,16,20],
                                "cap_r": [216,64,8,20], "pal": 12},
                "dpad": {"r": [0,112,16,16], "pal": 7},
                "slash": {"r": [96,64,8,16], "pal": 5},
                "hp_badge": {"r": [208,86,16,10], "pal": 1},
                "mp_badge": {"r": [224,86,16,10], "pal": 1},
                "ap_label": {"r": [128,64,24,16], "pal": 4},
                "ap_trough": {"r": [128,80,56,16], "pal": 4},
                "ap_end": {"r": [176,64,16,16], "pal": 4},
                "ap_cap": {"r": [184,80,8,16], "pal": 4},
                "gauge_fill": {"r": [64,136,16,6], "pal": 1},
                "red_x": {"r": [0,96,64,16], "pal": 4},
                "digit24_v": 64,
                "word_super": {"r": [3,152,105,24], "pal": 3},
                "word_hyper": {"r": [3,176,105,24], "pal": 3},
                "word_arts": {"r": [115,176,97,24], "pal": 3},
                "word_miracle": {"r": [0,200,127,24], "pal": 3},
                "word_new": {"r": [132,200,64,24], "pal": 3},
                "word_damage": {"r": [0,224,52,14], "pal": 3},
                "word_hit": {"r": [0,240,32,16], "pal": 3},
                "word_total": {"r": [32,240,48,16], "pal": 3},
                "atlas_digits": {"v": 208, "x0": 0, "cell": 8, "h": 12, "pal": HUD_TEXT_SUB_PALETTE},
                "font_pal": HUD_TEXT_SUB_PALETTE,
            },
            "arts_input": arts_input_pieces(),
            "elements": self.hud_elements_json(),
            "hub": hub_sprites,
            "advance": self.hud_font_advances(),
        })
        .to_string()
    }
}
