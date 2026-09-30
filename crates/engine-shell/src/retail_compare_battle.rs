//! The battle half of the retail comparison corpus: read a battle save
//! state's encounter out of RAM, put the engine into the same fight through
//! its real encounter entry, and score the battle channels.
//!
//! Retail side (every address a battle-resident SCUS global or the battle
//! context it points at, `docs/subsystems/battle.md`):
//!
//! - **battle context** `*0x8007BD24` (`0x800EB654` while a fight is
//!   resident): `+0x00` party count, `+0x01` monster count, `+0x06` the
//!   command-flow byte, `+0x07` the action-state cursor;
//! - **formation cell** `0x8007BD0C[0..4]` - monster seat `m`'s id;
//! - **per-battle flags** `0x8007BD60` - bit `0x80` is the scripted-fight
//!   header bit the entity SM ORs in for a row whose `record[+0]` is set;
//! - **run state** `0x8007BD71` (`0xFF` running, `0xFE` ending);
//! - **combatants** through the eight-slot actor pointer table `0x801C9370`
//!   (party `0..2`, monsters `3..7`, fixed slots): HP `+0x14C` / max
//!   `+0x14E`, MP `+0x150` / max `+0x152`;
//! - **camera** the same rotation / translation globals the field uses
//!   (`0x8007B790/92`, `0x800840B8..C0`) plus GTE `H`.
//!
//! Engine side: the scene is entered through the card-load path and the
//! field settles [`crate::retail_compare::SETTLE_TICKS`] frames (the fight
//! is entered from a running field, as retail's was: the entry scripts start
//! the scene's track first). The retail formation is matched against the
//! scene's registered MAN rows (the id space a region roll or a scripted
//! carrier produces) and, when no row carries it, registered as a formation
//! of its own - then [`legaia_engine_core::world::World::force_encounter`]
//! arms it through the ordinary transition, exactly as `play-window
//! --battle` does. Once the mode flips to battle the retail combatants' live
//! HP / MP are written over the engine's (the capture is mid-fight; the seed
//! carries its damage), the opening runs to the first round prompt, and the
//! session settles [`BATTLE_SETTLE_TICKS`] frames with no input.
//!
//! The seed cannot resume an action in flight: a capture taken mid-strike or
//! mid-cast (flow `0xFF`) is compared against the engine parked on its round
//! prompt, so its `phase` channel reads the capture's timing, not the port.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result, bail};
use legaia_engine_core::battle_flow::BattleFlowState;
use legaia_engine_core::world::SceneMode;
use legaia_mednafen::game_anchors;
use serde::{Deserialize, Serialize};

use crate::boot::{BootConfig, BootSession, FieldLiveOpts};
use crate::retail_compare::{CameraObs, RetailObs};

/// Battle context pointer.
const BATTLE_CTX: u32 = 0x8007_BD24;
/// Formation cell, one monster id per seat.
const FORMATION_CELL: u32 = 0x8007_BD0C;
/// Per-battle flags; bit `0x80` = the formation row's header byte was set.
const PER_BATTLE_FLAGS: u32 = 0x8007_BD60;
/// Battle run state (`0xFF` running, `0xFE` ending, `0x00` opening).
const RUN_STATE: u32 = 0x8007_BD71;
/// Battle stage id (`0` none; `1` the Tetsu tutorial module).
const STAGE_ID: u32 = 0x8007_B64A;
/// Present-party list: pool slot -> roster character id (1-based; `4` is
/// the AI-companion seat).
const SEAT_CHARS: u32 = 0x8007_BD10;
/// Eight-slot battle actor pointer table.
const ACTOR_TABLE: u32 = 0x801C_9370;
/// Frames a forced encounter may take to reach battle mode (the intro
/// transition runs 132 display frames).
const ENTRY_TICKS: u32 = 400;
/// Engine ticks run between the battle-mode flip and sampling.
pub const BATTLE_SETTLE_TICKS: u64 = 60;
/// Frames the battle opening may take to reach the first round prompt. The
/// bound is the longest retail opening, not a typical one: the evolved-Cort
/// arrival (PROT 0968) parks the flow byte at `0x0C` for about 3200 vsyncs
/// before it hands round one back (`docs/subsystems/battle.md`, flow `0x0C`
/// is the boss stage module's baton). Every other fight leaves the loop on
/// its first prompt, so the bound costs nothing there.
const OPENING_TICKS: u32 = 4800;
/// Frames a replayed cast may take to reach the capture's phase. The summon
/// band's longest pre-creature stretch is the caster's clip plus the `0x78`
/// flash-out; a creature's own choreography (`0x36`) runs longer.
const INFLIGHT_TICKS: u32 = 1200;
/// The extra capture deadline a phase-gated `play-window` child gets.
pub const INFLIGHT_DEADLINE: u64 = INFLIGHT_TICKS as u64;
/// The battle projection's `H` (`FUN_8003D254`; `battle_cam_script::GTE_H`).
const BATTLE_H: i16 = 256;

/// One combatant's HP / MP, both sides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Combatant {
    pub hp: u16,
    pub hp_max: u16,
    pub mp: u16,
    pub mp_max: u16,
}

/// Everything the battle channels read off a retail battle state.
#[derive(Debug, Clone)]
pub struct RetailBattle {
    pub party_count: u8,
    pub monster_count: u8,
    /// `ctx[+0x06]`.
    pub flow: u8,
    /// `ctx[+0x07]`.
    pub action_state: u8,
    pub run_state: u8,
    pub stage_id: u8,
    pub scripted: bool,
    /// `0x8007BD60 & 0x1F` - the battle-stage variant the region reader
    /// stamped for the tile the fight started on; battle init loads the
    /// backdrop from entry `scene_index + variant` (`FUN_800513F0`).
    pub stage_variant: u8,
    /// `0x8007BD10[0..party_count]`.
    pub seat_chars: Vec<u8>,
    /// Formation-cell ids, trimmed to the monster count.
    pub monster_ids: Vec<u8>,
    /// Party slots `0..party_count` (fixed pool slots).
    pub party: Vec<Option<Combatant>>,
    /// Monster seats (pool slots `3..3 + monster_count`).
    pub monsters: Vec<Option<Combatant>>,
    /// `ctx[+0x13]` - the seat the action SM is running.
    pub active_actor: u8,
    /// The active seat's queued action id `+0x1DF` (a spell id on a cast).
    pub queued_action: u8,
    /// The active seat's target byte `+0x1DD`.
    pub target_code: u8,
    /// The summon band's live flash, when one is up ([`RetailFade`]).
    pub summon_fade: Option<RetailFade>,
    /// `ctx[+0x279]` - the resident summon module's phase byte.
    pub module_phase: u8,
}

/// The summon band's live full-screen flash in a capture: which of the two
/// templates it is and how many vsyncs it has run. Read off the SCUS fade
/// actor's `+0x7C` block ([`legaia_engine_core::fade_ramp`]), whose
/// countdowns give the age back exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetailFade {
    /// `true` for the `0x33` flash-in (black to white), `false` for the
    /// `0x34` flash-out.
    pub to_white: bool,
    /// Vsyncs since the spawn, the start delay included.
    pub age: u16,
}

/// The SCUS fade actor's tick (`FUN_80025000`), stored at actor `+0x0C`.
const FADE_ACTOR_TICK: u32 = 0x8002_5000;
/// The actor-list "done" bit (`actor[+0x10] |= 8`) a killed fade carries.
const ACTOR_DONE: u32 = 0x8;
/// Where the effect-actor pool lives (`FUN_80020DE0`'s allocations).
const ACTOR_POOL: std::ops::Range<u32> = 0x8007_0000..0x800A_0000;

/// The block's per-frame red delta for a template (`FUN_80020B00`).
fn template_delta(t: &legaia_engine_vm::battle_action::SummonFadeTemplate) -> i16 {
    (((i32::from(t.end_rgb[0]) - i32::from(t.start_rgb[0])) * 0x40) / i32::from(t.duration)) as i16
}

/// Find the live summon flash among the fade actors: tick word
/// `FUN_80025000`, not done, kind `1`, id `1`, and the delta of one of the
/// two summon templates (the creature's own fades share the actor and the
/// kind, never the delta and duration pair).
pub fn summon_fade(ram: &[u8]) -> Option<RetailFade> {
    use legaia_engine_vm::battle_action::{SUMMON_FADE_ID, SUMMON_FLASH_IN, SUMMON_FLASH_OUT};
    let s16 = |va: u32| game_anchors::u16_at(ram, va) as i16;
    let mut a = ACTOR_POOL.start;
    while a + 0xA0 < ACTOR_POOL.end {
        let base = a;
        a += 4;
        if game_anchors::u32_at(ram, base + 0x0C) != FADE_ACTOR_TICK
            || game_anchors::u32_at(ram, base + 0x10) & ACTOR_DONE != 0
        {
            continue;
        }
        let b = base + 0x7C;
        if s16(b + 0x18) != 1 || s16(b + 0x22) != SUMMON_FADE_ID {
            continue;
        }
        let (delta, delay, duration) = (s16(b + 0x10), s16(b + 0x1C), s16(b + 0x20));
        for (t, to_white) in [(&SUMMON_FLASH_IN, true), (&SUMMON_FLASH_OUT, false)] {
            if delta != template_delta(t) {
                continue;
            }
            // `FUN_80020C14` counts the delay down first; the frame it lands
            // also steps the duration, so a landed block's age is one short
            // of the two countdowns' sum.
            let age = if delay > 0 {
                i32::from(t.delay) - i32::from(delay)
            } else if t.delay > 0 {
                i32::from(t.delay) + i32::from(t.duration) - i32::from(duration) - 1
            } else {
                i32::from(t.duration) - i32::from(duration)
            };
            return Some(RetailFade {
                to_white,
                age: age.clamp(0, i32::from(u16::MAX)) as u16,
            });
        }
    }
    None
}

/// Where in the summon band a capture sits, as an engine frame has to be
/// gated to match it: the action-SM state, and - while a flash is up - the
/// same flash the same number of vsyncs in. `play-window` reads it as
/// `LEGAIA_CAPTURE_GATE` and captures the first frame it holds; the headless
/// seed samples on the same predicate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhaseGate {
    pub action_state: u8,
    pub fade: Option<RetailFade>,
    /// The summon module's phase `ctx[+0x279]`, gated on only in `0x35` /
    /// `0x36` and only for a module whose arms the engine paces on retail's
    /// own countdown (`cast_module_camera::module_director`): there the band's
    /// length is the module's, so the state alone does not place the frame.
    pub module_phase: Option<u8>,
}

impl PhaseGate {
    /// `state[,white|black,age]`.
    pub fn to_env(&self) -> String {
        let base = self.env_state_and_fade();
        match self.module_phase {
            Some(p) => format!("{base},m{p}"),
            None => base,
        }
    }

    fn env_state_and_fade(&self) -> String {
        match self.fade {
            None => format!("{}", self.action_state),
            Some(f) => format!(
                "{},{},{}",
                self.action_state,
                if f.to_white { "white" } else { "black" },
                f.age
            ),
        }
    }

    pub fn from_env(s: &str) -> Option<Self> {
        let mut parts: Vec<&str> = s.split(',').map(str::trim).collect();
        let module_phase = match parts.last() {
            Some(t) if t.starts_with('m') => {
                let p = t[1..].parse().ok()?;
                parts.pop();
                Some(p)
            }
            _ => None,
        };
        let mut it = parts.into_iter();
        let action_state = it.next()?.parse().ok()?;
        let fade = match (it.next(), it.next()) {
            (Some(dir), Some(age)) => Some(RetailFade {
                to_white: dir == "white",
                age: age.parse().ok()?,
            }),
            _ => None,
        };
        Some(Self {
            action_state,
            fade,
            module_phase,
        })
    }

    /// Whether `world` is at this phase: the action SM on the same state
    /// and, when the capture had a flash up, the same flash at least as far
    /// in.
    pub fn met(&self, world: &legaia_engine_core::world::World) -> bool {
        if world.mode != SceneMode::Battle || world.battle_ctx.action_state != self.action_state {
            return false;
        }
        if let Some(p) = self.module_phase
            && world.casting.module_phase < p
        {
            return false;
        }
        let Some(want) = self.fade else {
            return true;
        };
        world.presentation.fade.as_ref().is_some_and(|f| {
            f.kind == 1
                && (f.target_rgb()[0] == 0xFF) == want.to_white
                && f.age_vsyncs() >= want.age
        })
    }
}

impl RetailBattle {
    /// The in-flight cast this capture holds, when the engine can replay it:
    /// a party seat running the summon band (`0x32..=0x36`) on a spell id.
    pub fn inflight_cast(&self) -> Option<legaia_engine_core::world::InflightCastSeed> {
        let in_band = (0x32..=0x36).contains(&self.action_state);
        (in_band
            && self.flow == 0xFF
            && self.active_actor < self.party_count
            && self.queued_action >= legaia_engine_vm::battle_action::SPELL_TRIGGER_SUMMON_MIN_ID)
            .then_some(legaia_engine_core::world::InflightCastSeed {
                caster: self.active_actor,
                spell_id: self.queued_action,
                target: self.target_code,
            })
    }

    /// The phase an in-flight capture's engine frame is gated on.
    pub fn phase_gate(&self) -> Option<PhaseGate> {
        self.inflight_cast()?;
        // The player summon block is linear: PROT `903 + (id - 0x81)`.
        let entry = u32::from(self.queued_action).wrapping_sub(0x81) + 903;
        let directed = (0x81..=0xA0).contains(&self.queued_action)
            && legaia_engine_vm::cast_module_camera::module_profile(entry)
                .is_some_and(|p| p.paces_band());
        let module_phase =
            (directed && (0x35..=0x36).contains(&self.action_state)).then_some(self.module_phase);
        Some(PhaseGate {
            action_state: self.action_state,
            fade: self.summon_fade,
            module_phase,
        })
    }
}

fn in_ram(p: u32) -> bool {
    (0x8000_0000..0x8020_0000).contains(&p)
}

fn combatant(ram: &[u8], slot: u32) -> Option<Combatant> {
    let p = game_anchors::u32_at(ram, ACTOR_TABLE + slot * 4);
    if !in_ram(p) {
        return None;
    }
    Some(Combatant {
        hp: game_anchors::u16_at(ram, p + 0x14C),
        hp_max: game_anchors::u16_at(ram, p + 0x14E),
        mp: game_anchors::u16_at(ram, p + 0x150),
        mp_max: game_anchors::u16_at(ram, p + 0x152),
    })
}

impl RetailBattle {
    /// Read the battle observables; `Err` names why the state is not a
    /// seedable battle.
    pub fn from_ram(ram: &[u8]) -> std::result::Result<Self, String> {
        let ctx = game_anchors::u32_at(ram, BATTLE_CTX);
        if !in_ram(ctx) {
            return Err(format!(
                "battle context pointer 0x{ctx:08X} not resident (battle still loading)"
            ));
        }
        let party_count = game_anchors::u8_at(ram, ctx);
        let monster_count = game_anchors::u8_at(ram, ctx + 1);
        if !(1..=3).contains(&party_count) || !(1..=5).contains(&monster_count) {
            return Err(format!(
                "battle context counts out of range (party {party_count}, monsters {monster_count})"
            ));
        }
        let monster_ids: Vec<u8> = (0..u32::from(monster_count).min(4))
            .map(|m| game_anchors::u8_at(ram, FORMATION_CELL + m))
            .collect();
        if monster_ids.iter().all(|&id| id == 0) {
            return Err("formation cell empty".into());
        }
        let active_actor = game_anchors::u8_at(ram, ctx + 0x13);
        let active = Some(game_anchors::u32_at(
            ram,
            ACTOR_TABLE + u32::from(active_actor.min(7)) * 4,
        ))
        .filter(|&p| in_ram(p));
        Ok(Self {
            party_count,
            monster_count,
            flow: game_anchors::u8_at(ram, ctx + 6),
            action_state: game_anchors::u8_at(ram, ctx + 7),
            run_state: game_anchors::u8_at(ram, RUN_STATE),
            stage_id: game_anchors::u8_at(ram, STAGE_ID),
            scripted: game_anchors::u32_at(ram, PER_BATTLE_FLAGS) & 0x80 != 0,
            stage_variant: game_anchors::u8_at(ram, PER_BATTLE_FLAGS) & 0x1F,
            monster_ids,
            seat_chars: (0..u32::from(party_count))
                .map(|s| game_anchors::u8_at(ram, SEAT_CHARS + s))
                .collect(),
            party: (0..u32::from(party_count))
                .map(|s| combatant(ram, s))
                .collect(),
            monsters: (0..u32::from(monster_count))
                .map(|m| combatant(ram, 3 + m))
                .collect(),
            active_actor,
            queued_action: active.map_or(0, |p| game_anchors::u8_at(ram, p + 0x1DF)),
            target_code: active.map_or(0, |p| game_anchors::u8_at(ram, p + 0x1DD)),
            summon_fade: summon_fade(ram),
            module_phase: game_anchors::u8_at(ram, ctx + 0x279),
        })
    }
}

/// What the engine shows after the battle seed.
pub struct EngineBattle {
    pub scene: Option<String>,
    pub mode: SceneMode,
    /// How the formation was reached: `man row N` or `synthesized`.
    pub formation_source: String,
    /// The scene's MAN formation row the fight was entered through, when one
    /// carries the retail cell (what `play-window --battle` can name).
    pub man_row: Option<u16>,
    /// Ticks from the battle-mode flip to the first command prompt; `None`
    /// when the opening never reached one.
    pub prompt_tick: Option<u32>,
    /// `Some` when the capture's cast was replayed: the ticks from the
    /// dispatch to the capture's phase, `None` inside when the engine never
    /// reached it.
    pub inflight: Option<Option<u32>>,
    /// The engine's action-SM state when sampled.
    pub action_state: u8,
    pub monster_ids: Vec<Option<u16>>,
    pub party: Vec<Combatant>,
    pub monsters: Vec<Combatant>,
    pub flow: BattleFlowState,
    pub camera: CameraObs,
    pub bgm_id: Option<u16>,
    /// The engine's track word and `World::audio.current_bgm` just before
    /// the encounter was forced.
    pub field_word: Option<u16>,
    pub field_current: Option<u16>,
    /// The track the engine plays during the fight.
    pub battle_bgm: Option<u16>,
    pub save: legaia_save::SaveFile,
}

/// The first registered formation row whose monster list equals `ids`.
fn matching_row(world: &legaia_engine_core::world::World, ids: &[u8]) -> Option<u16> {
    world.registered_formation_ids().into_iter().find(|fid| {
        world
            .tables
            .formation_table
            .formation(*fid)
            .is_some_and(|d| {
                d.slots.len() == ids.len()
                    && d.slots
                        .iter()
                        .zip(ids)
                        .all(|(s, &id)| s.monster_id == u16::from(id))
            })
    })
}

/// Seed the engine into `retail`'s battle and sample it.
pub fn run_engine_battle(
    extracted: &Path,
    retail: &RetailObs,
    battle: &RetailBattle,
) -> Result<EngineBattle> {
    let cfg = BootConfig {
        scene: retail.scene.clone(),
        enable_audio: false,
    };
    let mut session = BootSession::open(extracted, &cfg).context("open boot session")?;
    // Player-driven: retail parks every party turn on the command flow, so an
    // auto-resolving engine would spend the settle window fighting.
    let opts = FieldLiveOpts {
        live_loop: true,
        player_battle: true,
        ..Default::default()
    };
    let save = retail
        .save
        .clone()
        .context("retail state has no liftable save window")?;
    let _landing = session.resume_save(save, &retail.scene, &opts);
    let mut director = crate::retail_compare::RecordingDirector::default();
    session.host.route_bgm_events(&mut director)?;
    // The fight is entered from a settled field, as retail's was: the
    // scene's entry scripts start its track and raise its entry state first.
    for _ in 0..crate::retail_compare::SETTLE_TICKS {
        session.tick()?;
        session.host.route_bgm_events(&mut director)?;
    }

    let world = &mut session.host.world;
    let source;
    let man_row = matching_row(world, &battle.monster_ids);
    let fid = match man_row {
        Some(row) => {
            source = format!("man row {row}");
            // A scripted carrier's record raises its one-shot system-flag
            // arm on the way into the fight (the sparring tutorial's `50 19`);
            // the forced entry replays it, as `play-window --battle` does.
            world.replay_scripted_battle_arm(row);
            row
        }
        None => {
            // No row of this scene carries the retail cell: a scripted
            // fight installed from another table, or a scene whose rows the
            // engine does not register. Register the cell as its own
            // formation, with the archive's stats for its ids.
            let rec = legaia_engine_core::encounter_record::EncounterRecord::new(
                battle.monster_ids.len() as u8,
                &battle.monster_ids,
            )
            .context("formation cell does not fit a record")?;
            let mut def = rec.to_formation_def(retail.scene.clone());
            let mut id = def.formation_id;
            while world.tables.formation_table.formation(id).is_some() {
                id = id.wrapping_add(1);
            }
            def.formation_id = id;
            def.header_flags = u8::from(battle.scripted);
            world.tables.formation_table.insert(def);
            source = "synthesized".to_string();
            id
        }
    };
    let ids: Vec<u16> = battle.monster_ids.iter().map(|&i| u16::from(i)).collect();
    if ids
        .iter()
        .any(|id| session.host.world.tables.monster_catalog.get(*id).is_none())
    {
        let archive = session.host.index.entry_bytes_extended(867)?;
        let cat = legaia_engine_core::monster_catalog::catalog_from_monster_archive(&archive, &ids);
        for def in cat.by_id.into_values() {
            session.host.world.tables.monster_catalog.insert(def);
        }
    }
    // The fight's own composition: retail's present list `0x8007BD10` is
    // the battle's seat table, not the save window's field party. They
    // differ on a guest seat (id `4`) and on a battle-id fight, whose init
    // (`FUN_80055B6C` with `DAT_8007B7FC != 0`) re-seeds the fallback trio
    // `{1, 2, 3}` through `FUN_80055B20` whatever the field party was. The
    // image side passes the same list as `play-window --party`.
    // The stage the fight is staged in: the region reader's variant for the
    // tile it started on. The capture's player actor is no longer the field
    // walker, so the seed cannot stand on that tile; it stamps the variant.
    session
        .host
        .world
        .seed_battle_stage_variant(battle.stage_variant);
    let seats = retail_roster_slots(battle);
    if !seats.is_empty() && seats != session.host.world.party.active_party {
        session.host.world.set_active_party(seats);
    }
    // The field's own track word and the world's current track, as the
    // battle swap will find them.
    let field_word = session.host.bgm_track_word.or(director.last);
    let field_current = session.host.world.audio.current_bgm;
    if !session.host.world.force_encounter(fid) {
        bail!("force_encounter({fid}) refused ({source})");
    }
    let mut entered = false;
    for _ in 0..ENTRY_TICKS {
        session.tick()?;
        session.host.route_bgm_events(&mut director)?;
        if session.host.world.mode == SceneMode::Battle {
            entered = true;
            break;
        }
    }
    if !entered {
        bail!(
            "forced encounter ({source}) never reached battle mode in {ENTRY_TICKS} ticks (mode {:?})",
            session.host.world.mode
        );
    }
    // Reconstruct the mid-fight HP / MP (the engine seeds full bars).
    {
        let world = &mut session.host.world;
        let pc = world.party.party_count.clamp(1, 3) as usize;
        let seeds = battle
            .party
            .iter()
            .enumerate()
            .chain(battle.monsters.iter().enumerate().map(|(m, c)| (pc + m, c)));
        for (slot, c) in seeds {
            let (Some(c), Some(a)) = (c, world.actors.get_mut(slot)) else {
                continue;
            };
            a.battle.hp = c.hp;
            a.battle.liveness = a.battle.hp;
            if a.battle.hp_display.is_some() {
                a.battle.hp_display = Some(a.battle.hp);
            }
            a.battle.mp = c.mp;
        }
    }
    // Run the opening (banner, intro camera, initiative) to the first round
    // prompt, the earliest point a retail capture of a running fight can
    // share with a fresh entry; then settle.
    //
    // A capture taken mid-cast is replayed instead of parked: the cast is
    // seeded to dispatch the moment that prompt opens, and the session runs
    // until it reaches the capture's phase (`PhaseGate`) rather than a fixed
    // settle.
    let seed = battle.inflight_cast();
    session.host.world.battle.inflight_seed = seed;
    let mut prompt_tick = None;
    for t in 0..OPENING_TICKS {
        let w = &session.host.world;
        let reached = match seed {
            Some(_) => w.battle.inflight_seed.is_none(),
            None => w.battle.flow != BattleFlowState::Idle,
        };
        if reached {
            prompt_tick = Some(t);
            break;
        }
        session.tick()?;
        session.host.route_bgm_events(&mut director)?;
    }
    let mut phase_tick = None;
    match battle.phase_gate() {
        Some(gate) if prompt_tick.is_some() => {
            for t in 0..INFLIGHT_TICKS {
                if gate.met(&session.host.world) {
                    phase_tick = Some(t);
                    break;
                }
                session.tick()?;
                session.host.route_bgm_events(&mut director)?;
            }
        }
        _ => {
            for _ in 0..BATTLE_SETTLE_TICKS {
                session.tick()?;
                session.host.route_bgm_events(&mut director)?;
            }
        }
    }
    let world = &mut session.host.world;
    let pc = world.party.party_count.clamp(1, 3) as usize;
    // Battle ordinal `s` holds roster record `party_roster_slot(s)` (the
    // present-party list): a solo duel's Gala is ordinal 0 but record 2, and
    // retail's battle init copies the max from *that* record (`+0x108`,
    // `FUN_80053CB8` at `0x80053E50`).
    let roster = world.save_full().party;
    let party_maxes: Vec<(u16, u16)> = (0..pc)
        .map(|s| {
            roster
                .members
                .get(world.party_roster_slot(s))
                .map_or((0, 0), |m| {
                    let v = m.hp_mp_sp();
                    (v.hp_max, v.mp_max)
                })
        })
        .collect();
    let comb = |a: &legaia_engine_core::world::Actor, mp_max: u16| Combatant {
        hp: a.battle.hp,
        hp_max: a.battle.max_hp,
        mp: a.battle.mp,
        mp_max,
    };
    let party = (0..pc)
        .filter_map(|s| {
            world
                .actors
                .get(s)
                .map(|a| comb(a, party_maxes.get(s).map_or(0, |m| m.1)))
        })
        .collect();
    let mcount = world
        .battle
        .active_formation
        .as_ref()
        .map_or(0, |f| f.slots.len().min(5));
    let monster_ids = (0..mcount)
        .map(|m| world.actors.get(pc + m).and_then(|a| a.battle_monster_id))
        .collect();
    let monsters = (0..mcount)
        .filter_map(|m| {
            let a = world.actors.get(pc + m)?;
            let mp_max = a
                .battle_monster_id
                .and_then(|id| world.tables.monster_catalog.get(id))
                .map_or(0, |d| d.mp);
            Some(comb(a, mp_max))
        })
        .collect();
    let pose = world.battle_cam_pose();
    let camera = CameraObs {
        pitch: pose.pitch.round() as i16,
        yaw: (pose.yaw.round() as i32).rem_euclid(4096) as i16,
        h: BATTLE_H,
        eye: pose.tr.map(|v| v.round() as i32),
        // The engine holds the focus un-negated; retail's words are negated.
        focus: [
            -(pose.focus[0].round() as i32),
            -(pose.focus[2].round() as i32),
        ],
    };
    Ok(EngineBattle {
        scene: session.host.scene.as_ref().map(|s| s.name.clone()),
        mode: world.mode,
        formation_source: source,
        man_row,
        prompt_tick,
        inflight: seed.map(|_| phase_tick),
        action_state: world.battle_ctx.action_state,
        monster_ids,
        party,
        monsters,
        flow: world.battle.flow,
        camera,
        // Retail's track-select word keeps the field track through a fight:
        // the battle theme is started without the op-0x35 store, so the word
        // the fight holds is the one the field resumes. The engine routes its
        // battle swap through the op-0x35 start (which rewrites its copy of
        // the word), so the comparand is the track it stashed to resume.
        bgm_id: if world.audio.battle_bgm_active {
            world.audio.field_bgm_resume
        } else {
            session.host.bgm_track_word.or(director.last)
        },
        field_word,
        field_current,
        battle_bgm: world.audio.current_bgm,
        save: world.save_full(),
    })
}

/// Fraction of equal `(hp, hp_max, mp, mp_max)` fields over the retail
/// combatants; `mp_max` is left out where the engine has none (`0`).
fn combatant_score(retail: &[Option<Combatant>], engine: &[Combatant], tag: &str) -> (f64, String) {
    let mut total = 0usize;
    let mut equal = 0usize;
    let mut diffs = Vec::new();
    for (i, r) in retail.iter().enumerate() {
        let Some(r) = r else { continue };
        let e = engine.get(i);
        let fields = [
            ("hp", r.hp, e.map(|e| e.hp)),
            ("hp_max", r.hp_max, e.map(|e| e.hp_max)),
            ("mp", r.mp, e.map(|e| e.mp)),
            ("mp_max", r.mp_max, e.map(|e| e.mp_max)),
        ];
        for (name, want, got) in fields {
            if name == "mp_max" && got == Some(0) {
                continue;
            }
            total += 1;
            if got == Some(want) {
                equal += 1;
            } else {
                diffs.push(format!("{tag}{i}.{name} retail={want} engine={got:?}"));
            }
        }
    }
    let score = if total == 0 {
        1.0
    } else {
        equal as f64 / total as f64
    };
    let mut d = format!("{equal}/{total} fields");
    if !diffs.is_empty() {
        d.push_str("; ");
        d.push_str(&diffs.iter().take(4).cloned().collect::<Vec<_>>().join(", "));
    }
    (score, d)
}

/// Score one seeded battle state.
pub fn compare_battle(
    retail: &RetailObs,
    battle: &RetailBattle,
    engine: &EngineBattle,
) -> (BTreeMap<String, f64>, BTreeMap<String, String>) {
    use crate::retail_compare::{camera_score, flags_score, inventory_score, round3};
    let mut ch = BTreeMap::new();
    let mut det = BTreeMap::new();
    let mut put = |name: &str, score: f64, detail: String| {
        ch.insert(name.to_string(), round3(score));
        det.insert(name.to_string(), detail);
    };
    let scene_ok = engine.scene.as_deref() == Some(retail.scene.as_str());
    put(
        "scene",
        f64::from(u8::from(scene_ok)),
        format!("retail={} engine={:?}", retail.scene, engine.scene),
    );
    put(
        "mode",
        f64::from(u8::from(engine.mode == SceneMode::Battle)),
        format!("retail=0x{:02X} engine={:?}", retail.game_mode, engine.mode),
    );
    let n = battle
        .monster_ids
        .len()
        .max(engine.monster_ids.len())
        .max(1);
    let same = battle
        .monster_ids
        .iter()
        .zip(&engine.monster_ids)
        .filter(|(r, e)| **e == Some(u16::from(**r)))
        .count();
    put(
        "enemies",
        same as f64 / n as f64,
        format!(
            "retail={:02X?} engine={:?} ({})",
            battle.monster_ids, engine.monster_ids, engine.formation_source
        ),
    );
    let (s, d) = combatant_score(&battle.monsters, &engine.monsters, "m");
    put("enemy_hp", s, d);
    let (s, mut d) = combatant_score(&battle.party, &engine.party, "p");
    if battle.party.len() != engine.party.len() {
        d.push_str(&format!(
            "; retail seats {:?} vs engine party of {}",
            battle.seat_chars,
            engine.party.len()
        ));
    }
    put("battle_party", s, d);
    let want = BattleFlowState::from_raw(battle.flow);
    // A replayed cast is scored on the action-SM state as well: the flow
    // byte reads `Idle` for every in-flight action alike.
    let phase_ok = engine.flow == want
        && (engine.inflight.is_none() || engine.action_state == battle.action_state);
    let inflight = match engine.inflight {
        None => String::new(),
        Some(Some(t)) => format!("; cast replayed, phase reached at +{t}"),
        Some(None) => "; cast replayed, phase never reached".to_string(),
    };
    put(
        "phase",
        f64::from(u8::from(phase_ok)),
        format!(
            "retail flow=0x{:02X} ({want:?}) action=0x{:02X} run=0x{:02X}; engine {:?} action=0x{:02X} (first prompt at +{:?}){inflight}",
            battle.flow,
            battle.action_state,
            battle.run_state,
            engine.flow,
            engine.action_state,
            engine.prompt_tick
        ),
    );
    let (s, d) = camera_score(&retail.camera, &engine.camera);
    put("camera", s, d);
    put(
        "bgm",
        f64::from(u8::from(engine.bgm_id == Some(retail.bgm_id))),
        format!(
            "retail word={} engine field track={:?} (engine battle track {:?}; pre-battle word {:?}, current {:?})",
            retail.bgm_id,
            engine.bgm_id,
            engine.battle_bgm,
            engine.field_word,
            engine.field_current
        ),
    );
    if let Some(rs) = &retail.save {
        let (s, d) = flags_score(&rs.ext.story_flag_bits, &engine.save.ext.story_flag_bits);
        put("flags", s, d);
        let (s, d) = inventory_score(rs, &engine.save);
        put("inventory", s, d);
    }
    (ch, det)
}

/// Ticks `play-window --battle` runs before the capture: the intro
/// transition, the opening to the first round prompt, and the same settle
/// the headless side takes.
pub const BATTLE_CAPTURE_TICK: u64 = 320;

/// The `play-window` arguments that reproduce this fight: the MAN row and
/// the retail party composition (`0x8007BD10` ids are 1-based roster ids).
/// Retail's present list as 0-based roster slots (`0x8007BD10` holds 1-based
/// roster ids; `4` is the guest / AI-companion record).
fn retail_roster_slots(battle: &RetailBattle) -> Vec<u8> {
    battle
        .seat_chars
        .iter()
        .filter(|&&c| (1..=4).contains(&c))
        .map(|&c| c - 1)
        .collect()
}

pub fn play_window_args(battle: &RetailBattle, row: u16) -> Vec<String> {
    let mut args = vec!["--battle".to_string(), row.to_string()];
    let party: Vec<String> = retail_roster_slots(battle)
        .iter()
        .map(|c| c.to_string())
        .collect();
    if !party.is_empty() {
        args.push("--party".into());
        args.push(party.join(","));
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put16(ram: &mut [u8], va: u32, v: u16) {
        let o = (va & 0x1F_FFFF) as usize;
        ram[o..o + 2].copy_from_slice(&v.to_le_bytes());
    }
    fn put32(ram: &mut [u8], va: u32, v: u32) {
        let o = (va & 0x1F_FFFF) as usize;
        ram[o..o + 4].copy_from_slice(&v.to_le_bytes());
    }
    fn put8(ram: &mut [u8], va: u32, v: u8) {
        ram[(va & 0x1F_FFFF) as usize] = v;
    }

    /// A synthetic two-on-one fight: the reader takes the counts off the
    /// context, the ids off the cell and the combatants off the fixed pool
    /// slots (monster 0 is slot 3 whatever the party size).
    #[test]
    fn reads_a_fight_out_of_ram() {
        let mut ram = vec![0u8; 0x20_0000];
        let ctx = 0x800E_B654;
        put32(&mut ram, BATTLE_CTX, ctx);
        put8(&mut ram, ctx, 2);
        put8(&mut ram, ctx + 1, 1);
        put8(&mut ram, ctx + 6, 0x1E);
        put8(&mut ram, FORMATION_CELL, 0x4F);
        put8(&mut ram, SEAT_CHARS, 1);
        put8(&mut ram, SEAT_CHARS + 1, 2);
        for (slot, base, hp) in [(0u32, 0x800E_C9E8u32, 180u16), (3, 0x800E_D000, 999)] {
            put32(&mut ram, ACTOR_TABLE + slot * 4, base);
            put16(&mut ram, base + 0x14C, hp - 1);
            put16(&mut ram, base + 0x14E, hp);
        }
        let b = RetailBattle::from_ram(&ram).expect("seedable");
        assert_eq!(b.monster_ids, vec![0x4F]);
        assert_eq!(b.seat_chars, vec![1, 2]);
        assert_eq!(b.party.len(), 2);
        assert_eq!(b.party[0].map(|c| (c.hp, c.hp_max)), Some((179, 180)));
        assert_eq!(b.party[1], None, "an empty pool slot reads as no combatant");
        assert_eq!(b.monsters[0].map(|c| c.hp_max), Some(999));
        assert_eq!(
            BattleFlowState::from_raw(b.flow),
            BattleFlowState::TurnPrompt
        );
    }

    /// A live summon flash-in block, as `FUN_80024E80` leaves it in the
    /// actor pool: the reader finds it by tick word, kind, id and delta, and
    /// takes its age off the two countdowns.
    #[test]
    fn reads_the_summon_flash_age_off_the_fade_block() {
        use legaia_engine_vm::battle_action::SUMMON_FLASH_IN;
        let mut ram = vec![0u8; 0x20_0000];
        let actor = 0x8008_2BC4;
        put32(&mut ram, actor + 0x0C, FADE_ACTOR_TICK);
        let b = actor + 0x7C;
        put16(&mut ram, b + 0x10, template_delta(&SUMMON_FLASH_IN) as u16);
        put16(&mut ram, b + 0x18, 1);
        put16(&mut ram, b + 0x22, 1);
        // Still in the start delay: 14 of 20 left -> 6 vsyncs in.
        put16(&mut ram, b + 0x1C, 14);
        put16(&mut ram, b + 0x20, 20);
        assert_eq!(
            summon_fade(&ram),
            Some(RetailFade {
                to_white: true,
                age: 6
            })
        );
        // Landed and 14 into the hold: 20 + 20 + 14 vsyncs, less the one the
        // landing frame counts twice.
        put16(&mut ram, b + 0x1C, 0);
        put16(&mut ram, b + 0x20, (-14i16) as u16);
        assert_eq!(summon_fade(&ram).map(|f| f.age), Some(53));
        // A killed block is not the live flash.
        put32(&mut ram, actor + 0x10, ACTOR_DONE);
        assert_eq!(summon_fade(&ram), None);
    }

    #[test]
    fn the_phase_gate_round_trips_through_its_env_form() {
        for g in [
            PhaseGate {
                action_state: 0x33,
                fade: None,
                module_phase: None,
            },
            PhaseGate {
                action_state: 0x35,
                fade: Some(RetailFade {
                    to_white: false,
                    age: 24,
                }),
                module_phase: None,
            },
            PhaseGate {
                action_state: 0x36,
                fade: None,
                module_phase: Some(6),
            },
        ] {
            assert_eq!(PhaseGate::from_env(&g.to_env()), Some(g));
        }
    }

    #[test]
    fn a_loading_fight_names_its_reason() {
        let ram = vec![0u8; 0x20_0000];
        let err = RetailBattle::from_ram(&ram).unwrap_err();
        assert!(err.contains("not resident"), "{err}");
    }

    #[test]
    fn combatant_score_skips_a_missing_engine_max_mp() {
        let r = Combatant {
            hp: 10,
            hp_max: 20,
            mp: 5,
            mp_max: 9,
        };
        let e = Combatant { mp_max: 0, ..r };
        let (s, _) = combatant_score(&[Some(r)], &[e], "m");
        assert_eq!(s, 1.0);
        let (s, d) = combatant_score(&[Some(r)], &[], "p");
        assert_eq!(s, 0.0, "{d}");
    }
}
