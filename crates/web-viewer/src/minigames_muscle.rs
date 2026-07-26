//! Muscle Dome card-battle methods of [`LegaiaMinigames`] - the browser twin
//! of the play-window's `start_muscle_minigame` (`window/minigames.rs`).
//!
//! The rules are the ported [`legaia_engine_core::muscle_dome`] engine (the
//! four-slot hand deal, the point-budget card commit into the fighter's action
//! queue, the HP-ratio score readout and the win/lose bookkeeping), and the
//! round now **resolves through the ported battle formulas** rather than a
//! stand-in: each committed card is a battle action id (`0xC..=0xF`) that
//! resolves exactly as retail plays it - the move-power record via the
//! `0x801F4E63` id → index map ([`legaia_asset::move_power`]), the
//! arts/physical damage roll ([`legaia_engine_vm::battle_formulas`]:
//! `arts_physical_predamage_lazy`, `FUN_801dd0ac`), the element-affinity scale
//! ([`legaia_asset::element_affinity`], `FUN_801dd864`) and the damage
//! finisher (`damage_finish_lazy`, `FUN_801ddb30`), on a PsyQ `rand()` stream
//! with retail draw order.
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

use legaia_asset::element_affinity::ElementAffinity;
use legaia_asset::monster_archive;
use legaia_asset::move_power;
use legaia_asset::muscle_dome as md;
use legaia_asset::scene_tmd_stream;
use legaia_asset::sfx_table;
use legaia_engine_core::muscle_dome::{MuscleCard, MuscleDomeSession, MusclePhase};
use legaia_engine_vm::battle_formulas::{
    DamageFinish, DefenderResist, RecordStats, SummonRollActor, arts_physical_predamage_lazy,
    damage_finish_lazy, init_party_battle_stats, psyq_rand_step, spirit_gauge_fill,
};

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

/// One fighter's battle-formula inputs, resolved from disc records at contest
/// start. Field names follow the battle-actor offsets the damage kernel reads.
#[derive(Debug, Clone, Copy)]
pub(crate) struct MuscleFighter {
    /// Max HP (`+0x14e`); current HP lives in the rules session.
    hp_max: u16,
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

/// One resolved card play of the last round, for the page's 3D playback.
#[derive(Debug, Clone, Copy)]
pub(crate) struct MusclePlay {
    attacker: usize,
    cmd: u8,
    power: i32,
    damage: i32,
    hp_after: [i32; 2],
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
    /// Per-fighter spirit gauge (`actor+0x170`, 0..=100) - the value the
    /// dome HUD's per-fighter bars display (`FUN_801d8de8` elems 0x52/0x53),
    /// accrued from damage taken by the ported `spirit_gauge_fill`.
    spirit: [u16; 2],
    /// PsyQ `rand()` cursor for the whole contest.
    rng_seed: u32,
    /// Play-by-play of the last resolved round.
    log: Vec<MusclePlay>,
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
        let (mesh, _oids, shading) =
            legaia_tmd::mesh::tmd_to_vram_mesh_field_hybrid(&tmd, tmd_bytes);
        let mut flat = Vec::with_capacity(shading.colors.len() * 4);
        for (c, &t) in shading.colors.iter().zip(shading.textured.iter()) {
            flat.extend_from_slice(&[c[0], c[1], c[2], if t != 0 { 255 } else { 0 }]);
        }
        Some((mesh, flat))
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
        let session = MuscleDomeSession::new(
            player_hand,
            opp_hand,
            [player.budget_pool, opponent.budget_pool],
            [player.hp_max as i32, opponent.hp_max as i32],
            WEB_REWARD_SERU,
        );
        self.muscle = Some(MuscleContest {
            session,
            fighters: [player, opponent],
            names: [player_name, opp_name],
            spirit: [0, 0],
            rng_seed: seed,
            log: Vec::new(),
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

    /// Play the round out through the ported battle formulas. Each queued
    /// card resolves exactly as a retail battle action: move-power record via
    /// the id map, the arts/physical predamage roll (`FUN_801dd0ac`), the
    /// element-affinity scale (`FUN_801dd864`) and the damage finisher
    /// (`FUN_801ddb30`), drawing from the contest's PsyQ `rand()` stream in
    /// retail call order (3 draws, +2 when the bonus arm fires, +1 when
    /// mitigation floors the hit). The defender's spirit gauge accrues from
    /// each hit (`spirit_gauge_fill`). No-op unless the round is in the
    /// resolve phase.
    pub fn muscle_resolve(&mut self) {
        let Some(tables) = self.muscle_tables.as_ref() else {
            return;
        };
        let Some(c) = self.muscle.as_mut() else {
            return;
        };
        if c.session.phase() != MusclePhase::Resolve {
            return;
        }
        let MuscleContest {
            session,
            fighters,
            spirit,
            rng_seed,
            log,
            ..
        } = c;
        log.clear();
        // Shadow HP mirror: the kernel reads the defender's live +0x14c,
        // which drops mid-round; the session applies the same damage in the
        // same order, so the mirror stays in sync with it.
        let mut hp = [session.hp(0), session.hp(1)];
        session.resolve_round(|attacker, cmd| {
            let defender = attacker ^ 1;
            let power = move_power::record_for_move_id(&tables.move_power, &tables.move_map, cmd)
                .map(|r| r.power())
                .unwrap_or(0);
            let actor = |slot: usize| SummonRollActor {
                hp: hp[slot].clamp(0, u16::MAX as i32) as u16,
                agl: fighters[slot].int,
                stat_a: fighters[slot].udf,
                stat_b: fighters[slot].ldf,
                status: 0,
                guard: 0,
            };
            let affinity_pct = tables
                .affinity
                .as_ref()
                .and_then(|a| {
                    a.affinity_pct(fighters[attacker].element, fighters[defender].element)
                })
                .unwrap_or(100);
            let rng3 = [
                psyq_rand_step(rng_seed),
                psyq_rand_step(rng_seed),
                psyq_rand_step(rng_seed),
            ];
            let (att_roll, def_roll) = arts_physical_predamage_lazy(
                power,
                &actor(attacker),
                &actor(defender),
                affinity_pct,
                rng3,
                || [psyq_rand_step(rng_seed), psyq_rand_step(rng_seed)],
            );
            let finish = DamageFinish {
                predamage: att_roll.saturating_sub(def_roll),
                attacker_slot: if attacker == 0 { 0 } else { 3 },
                defender_slot: if defender == 0 { 0 } else { 3 },
                attacker_element: fighters[attacker].element,
                defender_resist: DefenderResist::default(),
                defender_guarding: false,
                enemy_defender_halve: false,
                bypass_party_resist: false,
                summon_power_pct: 100,
                floor_rand: 0,
            };
            let damage = damage_finish_lazy(&finish, || psyq_rand_step(rng_seed)) as i32;
            hp[defender] = (hp[defender] - damage).max(0);
            spirit[defender] = spirit_gauge_fill(
                damage as u32,
                fighters[defender].hp_max,
                spirit[defender],
                DefenderResist::default(),
                defender == 0,
            );
            log.push(MusclePlay {
                attacker,
                cmd,
                power,
                damage,
                hp_after: hp,
            });
            damage
        });
    }

    /// Start the next round after a non-terminal resolution: reseed budgets,
    /// clear queues. No-op unless the contest is at a round break.
    pub fn muscle_next_round(&mut self) {
        if let Some(c) = self.muscle.as_mut() {
            c.session.next_round();
        }
    }

    /// The last resolved round's play-by-play, for the page's 3D playback:
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
            .log
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

    /// Live contest state (superset of the older shape - `live`, `phase`,
    /// `round`, `hp`, `hp_max`, `budget`, `spent`, `score`, `queue`,
    /// `last_damage`, `hand`, `reward_spell` keep their meaning). New keys:
    /// `names`, `spirit` (the `+0x170` gauges the dome HUD bars display),
    /// `stats` (per-fighter INT/UDF/LDF/element the formulas used), `source`
    /// (`"disc"` / `"fallback"` player record), `char`, `level`, `monster`.
    pub fn muscle_state_json(&self) -> String {
        let Some(c) = self.muscle.as_ref() else {
            return r#"{"live":false}"#.to_string();
        };
        let s = &c.session;
        let phase = match s.phase() {
            MusclePhase::Select => "select",
            MusclePhase::Resolve => "resolve",
            MusclePhase::RoundOver => "round_over",
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
            "round": s.round(),
            "hp": [s.hp(0), s.hp(1)],
            "hp_max": [c.fighters[0].hp_max, c.fighters[1].hp_max],
            "budget": [s.budget(0), s.budget(1)],
            "spent": [s.spent(0), s.spent(1)],
            "score": [s.score_percent(0), s.score_percent(1)],
            "queue": [s.queue(0), s.queue(1)],
            "last_damage": s.last_round_damage(),
            "hand": hand,
            "reward_spell": s.reward_spell_id(),
            "names": c.names,
            "spirit": c.spirit,
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
    // fighter reuses the Baka surface's battle-form party accessors
    // (`baka_fighter_*` side 0 - the same PROT 1204 pack + PROT 1203 anim
    // bank the turn-based battles draw). `muscle_vram` merges the party
    // atlases with the monster's texture pool so one TmdRenderer VRAM serves
    // both bodies.

    /// Whether the dome's 3D scene decodes for `monster_id`: the battle-form
    /// party pack plus the monster's mesh + idle animation.
    pub fn muscle_scene_ready(&self, monster_id: u16) -> bool {
        let monster_ok = self.monster_archive_entry().is_some_and(|e| {
            matches!(monster_archive::mesh(e, monster_id), Ok(Some(_)))
                && matches!(monster_archive::idle_animation(e, monster_id), Ok(Some(_)))
        });
        monster_ok && !self.baka_fighter_positions(0, 0).is_empty()
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

    /// The dome duel's 1 MB PSX VRAM: the battle-form party atlases (PROT
    /// 1205, their bundled CLUT strips) plus monster `monster_id`'s texture
    /// pool injected at battle slot 0's coordinates (CLUT row 484, 4bpp page
    /// at `(320, 256)`) - the same layout the retail battle loader builds -
    /// plus the arena backdrop's own TIM pages (PROT 1225 tail chunks: 4bpp
    /// pages at `(768, 0)` / `(832, 0)`, CLUT rows 473 / 479; disjoint from
    /// the fighter bands, so upload order doesn't matter).
    pub fn muscle_vram(&self, monster_id: u16) -> Vec<u8> {
        let mut vram = legaia_tim::Vram::new();
        if let Some(raw) = entry_bytes(
            &self.prot,
            &self.entries,
            legaia_asset::battle_char_pack::ATLAS_PROT_ENTRY_INDEX,
        ) && let Ok(atlases) = legaia_asset::battle_char_pack::parse_atlases(raw)
        {
            for atlas in &atlases {
                if let Ok(tim) = legaia_tim::parse(&atlas.tim_bytes) {
                    vram.upload_tim(&tim);
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
