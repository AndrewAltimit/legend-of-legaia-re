//! The capture-class bodies' cameras and countdowns, arm by arm off their module
//! images, against the camera directors
//! (`cast_module_camera::capture_camera_director`).
//!
//! Each arm runs twice: once with its countdown word high, where a gated arm
//! must hold, and once with the word one drain from zero, where it must pass.
//! The run compares the tween builder call `FUN_801D829C` the arm makes (its
//! angle, translation and focus trios and its frame count) with the
//! director's `shot`, the countdown word it leaves with the director's, and a
//! hold with a phase the body left alone. Every call out of the module image
//! but the builder and `rand()` returns `0`.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_battle_vm::battle_action::bearing_12bit_approx;
use legaia_engine_battle_vm::battle_target_group::GroupAim;
use legaia_engine_cast::cast_arm_ticks::steal_sweep_tick;
use legaia_engine_cast::cast_module_camera::{
    self as cam, CaptureCamDirector, ModuleCamSeats, ModuleCamState, ModuleSeat, SPEED_SCALAR,
};
use legaia_engine_cast::cast_module_ticks as t;
use psx_retail_exec::{A0, A1, A2, A3, Cpu, Memory, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const BASE: u32 = 0x801F_69D8;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX_PTR: u32 = 0x8007_BD24;
const ACTORS: u32 = 0x801C_9370;
const CTX: u32 = 0x8012_0000;
const ACTOR: u32 = 0x8013_0000;
const STRIDE: u32 = 0x400;
const NODES: u32 = 0x8014_0000;
const RAND: u32 = 0x8005_6798;
const BUILDER: u32 = 0x801D_829C;
const BEARING: u32 = 0x8001_9B28;
const AIM: u32 = 0x801D_CEAC;
/// The range metric `FUN_8004E2F0` (its own differential is `range_metric`).
const RANGE: u32 = 0x8004_E2F0;
const OUT_CENTRE: u32 = 0x8011_0000;
const OUT_EXTENT: u32 = 0x8011_0100;
/// The battle frame's speed scalar and step (`0x1F800314 + 0x69` / `+ 0x7F`).
const SCALAR: u32 = 0x1F80_037D;
const STEP: u32 = 0x1F80_0393;
const FORMATION: u32 = 0x8007_BD0C;
const ROSTER: u32 = 0x8007_BD10;

/// One tween-builder call, as its arguments name it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Shot {
    angles: [i16; 3],
    tr: [i16; 3],
    focus: [i16; 3],
    frames: u16,
}

fn trio(m: &mut Memory, a: u32) -> [i16; 3] {
    [m.rd16(a) as i16, m.rd16(a + 2) as i16, m.rd16(a + 4) as i16]
}

struct Body {
    entry: u32,
    va: u32,
    action: u8,
    director: CaptureCamDirector,
}

fn bodies() -> Vec<Body> {
    let mut out = Vec::new();
    for action in 0..=0xFFu8 {
        if let Some((entry, va)) = t::capture_body_for_action(action)
            && let Some(director) = cam::capture_camera_director(entry, va)
            && !out.iter().any(|b: &Body| (b.entry, b.va) == (entry, va))
        {
            out.push(Body {
                entry,
                va,
                action,
                director,
            });
        }
    }
    // PROT 0940's split body casts `0xAE` as well as Slime Divide `0x50`, and
    // forks its cues and cuts on which (`bne` on `+0x1DF` at `0x801F7990`,
    // `0x801F79F0`, `0x801F7AF0`).
    if let Some(director) = cam::capture_camera_director(940, 0x801F_78B8)
        && t::capture_body_for_action(0xAE) == Some((940, 0x801F_78B8))
    {
        out.push(Body {
            entry: 940,
            va: 0x801F_78B8,
            action: 0xAE,
            director,
        });
    }
    // PROT 0949's Water Crystals (`0x59`) is a single-body module the
    // trampolines do not name: its body sits at the image's own entry.
    out.push(Body {
        entry: 949,
        va: 0x801F_6A10,
        action: 0x59,
        director: cam::capture_camera_director(949, cam::SINGLE_BODY).expect("directed"),
    });
    // Cort's Evil Seru Magic (`0xAD`) casts through the boss band rather
    // than the capture table, on the same single body and director shape.
    out.push(Body {
        entry: 966,
        va: EVIL_SERU_MAGIC,
        action: 0xAD,
        director: cam::capture_camera_director(966, cam::SINGLE_BODY).expect("directed"),
    });
    out
}

/// A world in battle seated as `cpu`'s actor records are, with `body`'s cast
/// armed on the band: the engine's own walk of the cast, every layer of its
/// seat writes included.
fn shadow_world(
    cpu: &mut Cpu,
    body: &Body,
    caster: u32,
    victim: u32,
    monster: u8,
) -> Option<legaia_engine_core::world::World> {
    use legaia_game_tables::spell_names::{CAPTURE_CLASS, SpellEntry, SpellNameTable};
    let mut w = legaia_engine_core::world::World::default();
    while w.actors.len() < 12 {
        w.actors.push(Default::default());
    }
    w.mode = legaia_engine_mode::mode::SceneMode::Battle;
    w.party.party_count = 3;
    for s in 0..8u32 {
        let a = ACTOR + s * STRIDE;
        let act = &mut w.actors[s as usize];
        act.active = true;
        act.battle.hp = cpu.mem.rd16(a + 0x14C);
        act.battle.max_hp = cpu.mem.rd16(a + 0x14E).max(act.battle.hp);
        act.battle.liveness = u16::from(act.battle.hp > 0);
        act.battle.render_color = cpu.mem.rd32(a + 4);
        act.battle.render_flag = cpu.mem.rd8(a + 0x21C);
        act.battle.facing_angle = cpu.mem.rd16(a + 0x46);
        act.battle.active_target = cpu.mem.rd8(a + 0x1DD);
        act.battle.anim_rate = legaia_engine_battle_vm::battle_anim_rate::AnimRate(8);
        act.move_state.world_x = cpu.mem.rd16(a + 0x34) as i16;
        act.move_state.world_y = cpu.mem.rd16(a + 0x36) as i16;
        act.move_state.world_z = cpu.mem.rd16(a + 0x38) as i16;
        // The AGL `+0x168` Glare's inline roll reads on both seats.
        w.battle.accuracy[s as usize] = cpu.mem.rd16(a + 0x168);
        if s >= 3 {
            act.battle_monster_id = Some(u16::from(if s == caster { monster.max(1) } else { 1 }));
        }
        // Glare's Stone cancels the victim's queued action and refunds an
        // Item; the guards it tests are the record's `+0x6BC` word.
        if body.action == 0x3C {
            act.battle.action_category = cpu.mem.rd8(a + 0x1DE);
            act.battle.init_key = cpu.mem.rd16(a + 0x16C);
            act.battle.params[0] = cpu.mem.rd8(a + 0x1DF);
            if s < 3 {
                w.party.character_ability_bits[s as usize] =
                    cpu.mem.rd32(CHAR_RECORDS + s * CHAR_STRIDE + 0x6BC);
            }
        }
    }
    w.battle_ctx.active_actor = caster as u8;
    let mut entries = vec![SpellEntry::default(); 0x100];
    entries[usize::from(body.action)].class = CAPTURE_CLASS;
    entries[usize::from(body.action)].sub_class = body.entry.checked_sub(935)? as u8;
    w.menu.text = Some(legaia_engine_menus::pause_screens::MenuTextTables {
        spell_names: Some(SpellNameTable::from_entries(entries)),
        ..Default::default()
    });
    if w.cast_module_for(body.action) != Some(body.entry) {
        return None;
    }
    w.drive_arm_capture_cast_module(body.action);
    // The cast the band folds as its sweep arms strike: the seats the
    // module's sweeps walk, the party row for a monster's cast.
    // A party caster's fold would land the band's own damage roll on its
    // victim, not retail's; it is left out.
    // Glare rolls its one victim (`caster[+0x1DD]`, `0x801F6F90`).
    let targets = if body.action == 0x3C {
        vec![victim as u8]
    } else {
        (0..3).collect()
    };
    w.casting.pending_cast =
        (caster >= 3).then_some(legaia_engine_battle_loop::cast_band::PendingCast {
            caster: caster as u8,
            spell_id: body.action,
            targets,
            on_flight: false,
        });
    Some(w)
}

/// The bodies whose seat hides and shows are compared: Terror Scream and Death
/// Game, whose hides and shows are all the director's. The other bodies split
/// theirs between the director, their tick kernel and the host's band paths,
/// which this walk does not run.
fn seat_writes_compared(body: &Body) -> bool {
    matches!(body.action, 0x70 | 0x76)
}

/// The bodies whose arms raise the caster's stat block: White Shield, Power
/// Charge (PROT 0955), Power Up (PROT 0942) and the surge (PROT 0945).
fn is_buff(body: &Body) -> bool {
    matches!(
        (body.entry, body.va),
        (955, t::WHITE_SHIELD_TICK)
            | (955, t::POWER_CHARGE_TICK)
            | (942, t::POWER_UP_TICK)
            | (945, t::ALL_STATS_SURGE_TICK)
    )
}

/// Every `jal` target the image's words encode.
fn jal_targets(module: &[u8]) -> Vec<u32> {
    let mut out: Vec<u32> = module
        .as_chunks::<4>()
        .0
        .iter()
        .map(|w| u32::from_le_bytes(*w))
        .filter(|w| w >> 26 == 3)
        .map(|w| ((w & 0x03FF_FFFF) << 2) | 0x8000_0000)
        .filter(|&t| !(BASE..BASE + 0x8000).contains(&t))
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

fn seat_at(m: &mut Memory, a: u32, display: bool) -> ModuleSeat {
    let o = if display { 0x3C } else { 0x34 };
    ModuleSeat {
        x: m.rd16(a + o) as i16,
        y: m.rd16(a + o + 2) as i16,
        z: m.rd16(a + o + 4) as i16,
        facing: m.rd16(a + 0x46) & 0xFFF,
    }
}

/// Seed the context, the eight seats and the frame globals for one pass of
/// `body`'s arm `phase`, and return the caster's and victim's seats.
fn seed(cpu: &mut Cpu, rng: &mut Rng, module: &[u8], body: &Body, phase: u8) -> (u32, u32) {
    cpu.mem.load(BASE, module);
    for b in 0..0x1100 {
        cpu.mem.wr8(CTX + b, 0);
    }
    let caster = 3 + rng.below(5);
    let victim = rng.below(3);
    cpu.mem.wr8(CTX, 3);
    cpu.mem.wr8(CTX + 1, 5);
    cpu.mem.wr8(CTX + 0x13, caster as u8);
    cpu.mem.wr8(CTX + 0x279, phase);
    cpu.mem.wr16(CTX + 0x6D0, 0x800 + rng.below(0x1000) as u16);
    cpu.mem.wr16(CTX + 0x6D8, rng.below(0x100) as u16);
    for s in 0..8u32 {
        let a = ACTOR + s * STRIDE;
        cpu.mem.wr32(ACTORS + s * 4, a);
        for o in (0..STRIDE).step_by(4) {
            cpu.mem.wr32(a + o, 0);
        }
        for o in [0x34u32, 0x36, 0x38, 0x3C, 0x3E, 0x40] {
            cpu.mem
                .wr16(a + o, (rng.below(0x1000) as u16).wrapping_sub(0x800));
        }
        cpu.mem.wr16(a + 0x46, rng.below(0x1000) as u16);
        cpu.mem.wr16(a + 0x14C, 1 + rng.below(500) as u16);
        cpu.mem.wr16(a + 0x168, 1 + rng.below(200) as u16);
        cpu.mem.wr16(a + 0x16A, 1 + rng.below(200) as u16);
        cpu.mem.wr8(a + 0x1DF, body.action);
        let node = NODES + s * 0x100;
        cpu.mem.wr32(a + 0x22C, node);
        cpu.mem.wr16(node + 0x58, 0x40 + rng.below(0x100) as u16);
        cpu.mem.wr16(node + 0x72, 0x1000);
    }
    cpu.mem.wr8(ACTOR + caster * STRIDE + 0x1DD, victim as u8);
    cpu.mem.wr8(SCALAR, SPEED_SCALAR as u8);
    cpu.mem.wr8(STEP, 1);
    cpu.mem.wr8(FORMATION, 0);
    for i in 0..3 {
        cpu.mem.wr8(ROSTER + i, 1 + i as u8);
    }
    (caster, victim)
}

#[test]
fn the_capture_cameras_frame_and_gate_as_retail_does() {
    differential!(
        prot(938, MYSTIC_CIRCLE),
        prot(938, CHAOS_BREATH),
        prot(940, MYSTIC_SHIELD),
        prot(940, GLARE),
        prot(941, STONE_CIRCLE),
        prot(941, STEAL),
        prot(942, POWER_UP),
        prot(942, DARK_TYPHOON),
        prot(943, LAPIS_WAVE),
        prot(944, GUILTY_CROSS),
        prot(944, CURSE_ALL),
        prot(945, WATER_COLUMN),
        prot(945, JUGGER_POWER),
        prot(949, 0x801F_6A10),
        prot(950, SHADOW_BREAK),
        prot(950, ROLLING_FLARE),
        prot(951, SCYTHE_WIND),
        prot(951, CHAOS_FLARE),
        prot(952, BLOODY_HORNS),
        prot(952, ASTRAL_SLASH),
        prot(955, KISS_OF_DEATH),
        prot(955, TERROR_SCREAM),
        prot(955, MELT_SPRAY),
        prot(955, VOID_ACCESSORIES),
        prot(955, WHITE_SHIELD),
        prot(955, POWER_CHARGE),
        prot(956, PARALYZING_WAVE),
        prot(956, WATER_HAZARD),
        prot(957, DEATH_GAME),
        prot(957, THUNDER_STORM),
        prot(958, BLAZING_SLASH),
        prot(959, MEGATON_PRESS),
        prot(960, PLASMA_STRIKE),
        prot(960, NEO_STAR_SLASH),
        prot(961, DEAD_END_CRISIS),
        prot(962, ULTRA_CHARGE),
        prot(962, BLADE_BREATH),
        prot(962, THUNDER_NEEDLE),
        prot(962, GIGATON_PRESS),
        prot(964, ELEMENT_CHANGE),
        prot(964, ELEMENT_STRIKE),
        prot(965, DOOMSDAY),
        prot(966, EVIL_SERU_MAGIC)
    );
    let (Some(_), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let shots = Rc::new(RefCell::new(Vec::<Shot>::new()));
    let stream = Rc::new(std::cell::Cell::new(1u32));
    // A fresh machine per body, so no body runs on what another's stubbed
    // calls left in RAM.
    let fresh = || {
        let mut cpu = scus_cpu().expect("SCUS");
        cpu.mem
            .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
        cpu.mem.wr32(CTX_PTR, CTX);
        cpu.mem.wr32(0x8007_B81C, 0x8007_0A2C);
        cpu.mem.wr32(0x8007_B7F8, 0x8007_0A2C + 0x800);
        {
            let shots = shots.clone();
            cpu.hook(BUILDER, move |r, m| {
                shots.borrow_mut().push(Shot {
                    angles: trio(m, r.r[A0]),
                    tr: trio(m, r.r[A1]),
                    focus: trio(m, r.r[A2]),
                    frames: r.r[A3] as u16,
                });
            });
        }
        {
            let stream = stream.clone();
            cpu.hook(RAND, move |r, _| {
                let n = stream.get().wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
                stream.set(n);
                r.r[V0] = (n >> 16) & 0x7FFF;
            });
        }
        // The 12-bit bearing, as its own differential holds it (`bearing`).
        cpu.hook(BEARING, |r, _| {
            let a = |i: usize| r.r[A0 + i] as i16;
            r.r[V0] = u32::from(bearing_12bit_approx(a(0), a(1), a(2), a(3)));
        });
        cpu
    };
    let polls = Rc::new(std::cell::Cell::new(0u32));
    // The pass's first poll answers "out of reach": PROT 0941's arm 1 holds
    // while its caster has not yet walked in (`0x801F75E0..0x801F75EC`).
    let short = Rc::new(std::cell::Cell::new(false));
    let mut rng = Rng(0x4341_4D53);
    let mut failures = Vec::new();
    let (mut passes, mut casts) = (0u32, 0u32);
    let mut outcomes = std::collections::BTreeSet::new();
    let mut unsettled_holds = 0usize;
    let (mut storm_seats, mut storm_stoned, mut storm_clamps) = (0usize, 0usize, 0usize);
    let mut storm_levels = 0usize;
    let mut victim_settle_holds = 0usize;
    let (mut results, mut heal_capped) = (0usize, false);
    let mut debuffs = 0usize;
    let mut buffs = 0usize;
    let mut seat_passes = 0usize;
    let mut steals = 0usize;
    let mut glare_rolls = 0usize;
    let mut kiss_halts = 0usize;
    let mut spray_clears = 0usize;
    let (mut horn_hits, mut horn_clamps) = (0usize, 0usize);
    let (mut doom_hits, mut doom_clamps, mut doom_holds) = (0usize, 0usize, 0usize);
    let mut status_draws = 0usize;
    let mut steal_voices = 0usize;
    let mut cursor_floors = 0usize;
    let mut plasma_gates = 0usize;
    let mut combo_lands = 0usize;
    let (mut shadow_passes, mut shadow_diverged) = (0usize, 0usize);
    let mut status_marks = 0usize;
    let mut shadow_failures: Vec<String> = Vec::new();
    let mut typhoon_poses = 0usize;
    let mut shove_poses = 0usize;
    let mut venom_marks = 0usize;
    let spawn_log = Rc::new(RefCell::new(Vec::<(u32, [i16; 3], [i16; 3], u16)>::new()));
    let (mut spawns_matched, mut spawn_failures) = (0usize, Vec::<String>::new());
    let all = bodies();
    for body in &all {
        let module = disc.read_entry(body.entry as usize).expect("module");
        let mut cpu = fresh();
        // Every routine the image calls returns `0`, but the group aim,
        // which the bodies' group framings need whole.
        for target in jal_targets(&module) {
            if ![AIM, RANGE, BUILDER, BEARING, RAND].contains(&target) {
                cpu.stub(target, 0);
            }
        }
        // Bloody Horns' hit rolls through the ATK wrapper `FUN_801DD6B4`
        // (`0x801F7948`), which returns the round's roll here so the clamp
        // to the victim's HP (`0x801F796C`) runs both ways.
        let horn_roll = Rc::new(std::cell::Cell::new(0u32));
        // Blazing Slash's six hits roll through the same wrapper and land on
        // seat 0 (`actor_table[0]`, `0x801F750C` and the five like it).
        let horn_calls = Rc::new(std::cell::Cell::new(0u32));
        // So does Plasma Strike's arm 0x0D burst (`0x801F8168`), on the victim.
        if matches!(body.entry, 952 | 958 | 960) {
            let roll = horn_roll.clone();
            let calls = horn_calls.clone();
            cpu.hook(0x801D_D6B4, move |r, _| {
                calls.set(calls.get() + 1);
                r.r[2] = roll.get();
            });
        }
        // Water Column's hit rolls through `FUN_801DD4B0` (`0x801F74A4`),
        // clamped to the victim's HP (`0x801F74C8`).
        let water_column = (body.entry, body.va) == (945, WATER_COLUMN);
        // Water Crystals' one hit rolls the same wrapper (`0x801F7318`),
        // clamped to the victim's HP (`0x801F733C..0x801F7348`).
        let crystals = body.entry == 949;
        // Thunder Storm's party sweep rolls the same wrapper (`0x801F7514`),
        // logged by seat: a small roll, or on odd rounds one past every HP.
        let storm = (body.entry, body.va) == (957, THUNDER_STORM);
        let storm_hits = Rc::new(RefCell::new(Vec::<u32>::new()));
        let storm_roll = Rc::new(std::cell::Cell::new(30u32));
        if storm {
            let hits = storm_hits.clone();
            let roll = storm_roll.clone();
            cpu.hook(0x801D_D4B0, move |r, _| {
                hits.borrow_mut().push(r.r[A2]);
                r.r[2] = roll.get();
            });
        }
        if water_column || crystals {
            let roll = horn_roll.clone();
            let calls = horn_calls.clone();
            cpu.hook(0x801D_D4B0, move |r, _| {
                calls.set(calls.get() + 1);
                r.r[2] = roll.get();
            });
        }
        // Doomsday's arm 0x0B and Chaos Breath's arm 2 roll each hittable
        // party seat through `FUN_801DD4B0(power, caster, seat)`
        // (`0x801F77B4`, `0x801F77C8`), which returns a roll keyed on the
        // round and the seat here, so the clamp to the seat's HP
        // (`0x801F77E0`, `0x801F77EC`) runs both ways.
        let doom_round = Rc::new(std::cell::Cell::new(0usize));
        let sweeps = party_sweep_arm(body).is_some();
        if sweeps {
            let round = doom_round.clone();
            cpu.hook(0x801D_D4B0, move |r, m| {
                let seat = r.r[A2];
                let hp = u32::from(m.rd16(ACTOR + seat * STRIDE + 0x14C));
                r.r[V0] = doomsday_roll(round.get(), seat, hp);
            });
        }
        // Steal's thief voices through the sound funnel `FUN_8004FE5C(id,
        // seat)`, logged rather than run.
        let voices = Rc::new(RefCell::new(Vec::<(u32, u32)>::new()));
        if body.action == 0x51 {
            let voices = voices.clone();
            cpu.hook(SOUND_FUNNEL, move |r, _| {
                voices.borrow_mut().push((r.r[A0], r.r[A1]));
                r.r[V0] = 0;
            });
        }
        // Terror Scream's refund goes through the bag's find-or-insert
        // `FUN_800421D4`, logged rather than run.
        let refunds = Rc::new(RefCell::new(Vec::<(u32, u32)>::new()));
        if matches!(body.action, 0x70 | 0x3C) {
            let refunds = refunds.clone();
            cpu.hook(BAG_INSERT, move |r, _| {
                refunds.borrow_mut().push((r.r[A0], r.r[A1]));
                r.r[V0] = 0;
            });
        }
        // Death Game keeps its card handles (`0x801FA2B8`) and walks them
        // through the shuffle and the pick: each spawn gets a part.
        let death_game = body.action == 0x76;
        // Every other body's part spawns, logged as `(record, point, angles,
        // scale)`, for the director's literal-anchored records.
        // Kiss of Death keeps the handles of the records arms 1 and 2 seat
        // (`0x801F9B28`, count `0x801F9D2C`), drops a handle whose part has
        // stopped (`+0x10 & 8`, `0x801F88F0..0x801F8910`) and stops the rest
        // as arm 2 passes (`0x801F8B14..0x801F8B60`): each spawn of its gets
        // a part, kept as `(record, part)`.
        let seated = Rc::new(RefCell::new(Vec::<(u32, u32)>::new()));
        if !death_game {
            let kiss = body.action == 0x6E;
            let next = Rc::new(std::cell::Cell::new(PARTS));
            for spawn in [SPAWN_PART, SPAWN_SEATED] {
                let log = spawn_log.clone();
                let seated = seated.clone();
                let next = next.clone();
                cpu.hook(spawn, move |r, m| {
                    let trio = |m: &mut Memory, a: u32| [0u32, 2, 4].map(|o| m.rd16(a + o) as i16);
                    let p = trio(m, r.r[A0]);
                    let a = trio(m, r.r[A1]);
                    log.borrow_mut().push((r.r[A2], p, a, r.r[A3] as u16));
                    r.r[V0] = 0;
                    if kiss {
                        let at = next.get();
                        next.set(at + 0x80);
                        for o in (0..0x80).step_by(4) {
                            m.wr32(at + o, 0);
                        }
                        seated.borrow_mut().push((r.r[A2], at));
                        r.r[V0] = at;
                    }
                });
            }
        }
        if death_game {
            let next = Rc::new(std::cell::Cell::new(PARTS));
            for spawn in [SPAWN_PART, SPAWN_SEATED] {
                let next = next.clone();
                cpu.hook(spawn, move |r, m| {
                    let at = next.get();
                    next.set(at + 0x80);
                    for o in (0..0x80).step_by(4) {
                        m.wr32(at + o, 0);
                    }
                    // The spawn seats the part on the point `a0` names
                    // (`FUN_80021B04`, `0x80021B94..0x80021BB8`), which the
                    // shuffle's slides read back.
                    for o in [0u32, 2, 4] {
                        let v = m.rd16(r.r[A0] + o);
                        m.wr16(at + 0x14 + o, v);
                    }
                    r.r[V0] = at;
                });
            }
            cpu.mem.wr32(CONFIRM_MASK, 0x20);
        }
        // The range poll answers "in reach", the approach being the host's,
        // but every fourth poll of a pass answers "out of reach": PROT
        // 0941's backstep (`0x801F7620..0x801F76E4`) walks until it is.
        {
            let polls = polls.clone();
            let short = short.clone();
            let backstep = body.action == 0x51;
            cpu.hook(RANGE, move |r, _| {
                polls.set(polls.get() + 1);
                let far = short.get() && polls.get() == 1;
                r.r[V0] = u32::from(backstep && (far || polls.get().is_multiple_of(4)));
            });
        }
        let countdown_va =
            cam::capture_countdown_va(body.action).or_else(|| sweep_countdown_va(body.action));
        let rounds = match body.action {
            // Death Game's heal (`0x801F971C..0x801F9794`) caps each seat's
            // quarter at its missing HP; enough rounds that the picked card
            // lands the heal on both a capped and an uncapped seat.
            0x76 => 72,
            0x3C => 8,
            0x71 => 14,
            0x70 => 12,
            // Steal's caster on every thief band and either side of each.
            0x51 => 12,
            // Chaos Breath's two status draws per seat land one time in
            // eight each.
            0x4E => 24,
            _ => 6,
        };
        // The rounds past a body's first count draw on a stream of their
        // own, so the bodies after it keep the seeds they always had.
        let own_rounds = match body.action {
            0x76 => 30,
            0x51 => 6,
            0x71 => 12,
            _ => rounds,
        };
        let mut shared_rng = None;
        for round in 0..rounds {
            if round == own_rounds {
                shared_rng = Some(rng.0);
            }
            seated.borrow_mut().clear();
            let (mut caster, mut victim) = seed(&mut cpu, &mut rng, &module, body, 0);
            // Water Hazard's last rounds cast from seat 7, which its `0xFF`
            // arm's seven-seat show leaves on arm 0's `+0x21C = 9`
            // (`sltiu 7` at `0x801F7E08`).
            if body.action == 0x71 && round >= own_rounds {
                caster = 7;
                cpu.mem.wr8(CTX + 0x13, 7);
                cpu.mem.wr8(ACTOR + 7 * STRIDE + 0x1DD, victim as u8);
            }
            // Lu Delilas casts Plasma Strike in a duel, on the lone hero in
            // seat 0, which its burst hits by the table (`actor_table[0]`,
            // `0x801F8184`) whatever the victim byte names.
            if body.entry == 960 {
                victim = 0;
                cpu.mem.wr8(ACTOR + caster * STRIDE + 0x1DD, 0);
            }
            // Ultra Charge is cast by two monsters alone, at fixed seats
            // (`engine_battle::monster_ai`): the evolved Cort `0xB5` on
            // himself, `0xB6`'s mode 3 on monster seat `3` (`li v0,0x3` /
            // `sb v0,0x1dd`).
            if body.action == 0xA5 {
                victim = if round % 2 == 1 { caster } else { 3 };
                cpu.mem.wr8(ACTOR + caster * STRIDE + 0x1DD, victim as u8);
            }
            // A spell whose target shape has `& 0x60 == 0` aims at the
            // monster row: the picker names the caster itself or a wounded
            // ally (`0x801EA5E4..0x801EA718`), never a party seat.
            let shape = cpu.mem.rd8(SPELL_STATS + u32::from(body.action) * 0xC + 2) & 0x60;
            if shape == 0 {
                victim = caster;
                cpu.mem.wr8(ACTOR + caster * STRIDE + 0x1DD, caster as u8);
            }
            // State `0x6F` drains the framing depth by `16 * step` a vsync
            // while it waits on the module's seek (`0x801E5008..0x801E5014`,
            // no clamp), so a long enough seek hands the module a depth below
            // zero: every fifth round of a body starts on one.
            if round % 5 == 4 {
                cpu.mem
                    .wr16(CTX + 0x6D0, (-1 - rng.below(0x800) as i32) as u16);
            }
            let (ca, va_) = (ACTOR + caster * STRIDE, ACTOR + victim * STRIDE);
            // A wide victim: Water Crystals' framing caps its TR z at `0xC00`
            // (`slti 0xc01` at 0x801F6FC0) on three radii.
            if crystals && round % 5 == 2 && victim < 8 {
                cpu.mem.wr16(NODES + victim * 0x100 + 0x58, 0x480);
            }
            // The caster's formation id, which a body can fork on: Terror
            // Scream's owners (Terror Face `0x7F`, the `0x28` row).
            let monster = match body.action {
                // The three caster bands its arm 0 forks on (`0x801F7764`,
                // `0x801F78B8`): Terror Face `0x7F`, the `0x28` row and the
                // `0x83` row, each picking its cue and cast clip.
                0x70 => [0x28u8, 0x7F, 0x83, 0x29, 0x84, 0x2A, 0x85][round % 7],
                // The two casters that deal a stacked table.
                0x76 => [0u8, 0x9D, 0x9E][round % 3],
                // Glare forks its cue, cut, clip and caster rate on Zora
                // (`0xA9`, `0x801F6AE0`, `0x801F6B34`, `0x801F6C94`,
                // `0x801F700C`).
                0x3C => [0u8, GLARE_ZORA][round % 2],
                // Ultra Charge forks its cue and its cuts on the evolved Cort
                // (`0x8007BD0C[0] == 0xB5`, `0x801F6A84`, `0x801F6AE0`,
                // `0x801F6C28`).
                // The two casters the picker gives it (`0xB6`'s mode 3 and
                // `0xB5`'s opening turn).
                0xA5 => [0xB6u8, 0xB5][round % 2],
                // Steal's voices fork on three bands of thieves, `4..=6`,
                // `0x43..=0x45` and `0x90..=0x92` (`0x801F770C`, `0x801F7740`,
                // `0x801F7774`; again at `0x801F7C18..0x801F7C88`).
                0x51 => [0u8, 4, 0x44, 0x92, 6, 0x43, 0x90, 5, 0x45, 0x91, 3, 0x93][round % 12],
                _ => 0,
            };
            // The formation cells `0x8007BD0C[seat - 3]`. A seat-7 caster's
            // cell is `0x8007BD10`, the first party roster byte, which the
            // hide walks read as "seat 0 is seated": never zero.
            for i in 0..4 {
                cpu.mem.wr8(FORMATION + i, monster);
            }
            cpu.mem.wr8(ROSTER, monster.max(1));
            // Megaton Press forks three arms on the lead's character id
            // (`0x8007BD10[0] == 1`, `0x801F74C4`, `0x801F76A4`, `0x801F80A0`):
            // each lead in turn.
            let lead = if body.entry == 959 {
                1 + (round % 3) as u8
            } else {
                1
            };
            if body.entry == 959 {
                cpu.mem.wr8(ROSTER, lead);
            }
            // Death Game's heal reads each seat's maximum `+0x14E` and caps
            // the quarter at the missing HP: a seat at full, one short and
            // one wounded.
            // The record table `0x801C9348[seat - 3]` White Shield's defence
            // and Power Up's / the surge's AGL read (`+0x14` / `+0x16`,
            // `+0x0E`), and the caster's stat block the buffs raise.
            if is_buff(body) {
                for s in 0..5u32 {
                    let r = MONSTER_RECORDS + s * 0x100;
                    cpu.mem.wr32(RECORDS + 4 * s, r);
                    for o in [0x0Eu32, 0x14, 0x16] {
                        cpu.mem.wr16(
                            r + o,
                            [0, 1, 0x7FFF, 0xAAAA][round % 4]
                                .max(rng.below(1000) as u16 * u16::from(round % 4 > 1)),
                        );
                    }
                }
                for (k, o) in (0x154u32..0x16C).step_by(2).enumerate() {
                    let v = match (round + k) % 5 {
                        0 => 0,
                        1 => 998,
                        2 => 0xFFFF,
                        _ => rng.below(1000) as u16,
                    };
                    cpu.mem.wr16(ca + o, v);
                }
            }
            // Terror Scream's victim with every queued category and with its
            // initiative key spent or not (`0x801F7E18..0x801F7E4C`).
            if body.action == 0x70 {
                cpu.mem.wr8(va_ + 0x1DE, (round % 3) as u8);
                cpu.mem.wr8(va_ + 0x1DF, 0x10 + round as u8);
                cpu.mem
                    .wr16(va_ + 0x16C, if (round / 3) % 2 == 0 { 0x40 } else { 0 });
                cpu.mem.wr8(CTX + 0x1A, round as u8);
            }
            // A fallen party seat and a fallen monster on odd rounds, which
            // the hide and show walks skip (`lhu 0x14c`, `beq` at
            // `0x801F7B90`, `0x801F7824`, `0x801F7F48`).
            // Every other body takes the same on one round in four, the
            // monster faded out (`+0x04 = 0`) as its defeat leaves it: a show
            // whose colour store sits in a `beq` delay slot relights it.
            let fell = if seat_writes_compared(body) {
                round % 2 == 1
            } else {
                round % 4 == 1
            };
            if fell {
                let p = (0..3u32).find(|&s| s != victim).expect("a party seat");
                let m = (3..7u32)
                    .find(|&s| s != caster && s != victim)
                    .expect("a monster seat");
                for s in [p, m] {
                    cpu.mem.wr16(ACTOR + s * STRIDE + 0x14C, 0);
                }
                // The fallen member lies on its down clip, as the settle
                // waits read it.
                if !seat_writes_compared(body) {
                    cpu.mem.wr8(ACTOR + p * STRIDE + 0x1D9, 8);
                }
            }
            // Melt Spray's five stats on the victim, both halves, the floor's
            // edge values among them (`0`, `1`, `2`).
            if body.action == 0x6F {
                for (k, o) in (0x158u32..0x16C).step_by(2).enumerate() {
                    let v = match (round + k) % 6 {
                        0 => 0,
                        1 => 1,
                        2 => 2,
                        _ => rng.below(1000) as u16,
                    };
                    cpu.mem.wr16(va_ + o, v);
                }
                cpu.mem.wr8(va_ + 0x1F2, (round % 2) as u8);
            }
            // Glare's roll on a slow victim beside a quick caster from round
            // 2 on, so it lands: behind Stone Guard (rounds 2 and 6) and
            // Master Guard (3), each record's `+0x6BC` (`0x801F7054..0x801F7070`),
            // on a victim with an Item queued and its turn to come (4, 6),
            // its turn spent (5) and another action queued (7), which the
            // Stone's cancel and refund read (`0x801F70A8..0x801F70EC`).
            if body.action == 0x3C {
                if round >= 2 {
                    cpu.mem.wr16(va_ + 0x168, 1);
                    cpu.mem.wr16(ca + 0x168, 200);
                }
                let guard = match round {
                    2 | 6 => 1u32 << 0x1A,
                    3 => 1 << 0x1C,
                    _ => 0,
                };
                for i in 0..3 {
                    cpu.mem.wr32(CHAR_RECORDS + i * CHAR_STRIDE + 0x6BC, guard);
                }
                if let Some((category, key)) = match round {
                    4 | 6 => Some((1u8, 0x40u16)),
                    5 => Some((1, 0)),
                    7 => Some((2, 0x40)),
                    _ => None,
                } {
                    cpu.mem.wr8(va_ + 0x1DE, category);
                    cpu.mem.wr16(va_ + 0x16C, key);
                    cpu.mem.wr8(va_ + 0x1DF, 0x10 + round as u8);
                }
            }
            // Kiss of Death's one-point hit (`0x801F8C68..`) downs a victim on
            // one HP, which its settle then waits on as fallen.
            if body.action == 0x6E && round % 2 == 1 {
                cpu.mem.wr16(va_ + 0x14C, 1);
            }
            doom_round.set(round);
            // The seat the wrapper's roll lands on.
            let hit_at = if matches!(body.entry, 958 | 960) {
                ACTOR
            } else {
                va_
            };
            // A hero the burst downs takes Plasma Strike's own wipe leg
            // (`0x801F8588`, phase `0x11`, which parks the module for the
            // battle's end) unless its record carries `+0x6C0 & 0x80`
            // (`0x801F8554`): on odd rounds it does.
            if body.entry == 960 {
                let rec = CHAR_RECORDS + 0x6C0;
                let bits = cpu.mem.rd32(rec) & !0x80;
                cpu.mem
                    .wr32(rec, bits | if round % 2 == 1 { 0x80 } else { 0 });
            }
            // Plasma Strike's arm 0x0C lands the flurry's combo total, the
            // victim's `+0x00` (`0x801F8098..0x801F80C8`): none, short of the
            // HP, past it.
            if body.action == 0x7B {
                let hp = u32::from(cpu.mem.rd16(va_ + 0x14C));
                cpu.mem.wr32(va_, [0, hp / 2, hp + 7][round % 3]);
            }
            if matches!(body.entry, 952 | 958 | 960) || water_column || crystals {
                let hp = u32::from(cpu.mem.rd16(hit_at + 0x14C));
                horn_roll.set([0, hp - 1, hp, hp + 1, hp * 3][round % 5]);
            }
            if death_game {
                for a in [ca, va_] {
                    let hp = cpu.mem.rd16(a + 0x14C);
                    let short = [0, 1, hp / 8, hp][(round + (a == va_) as usize) % 4];
                    cpu.mem.wr16(a + 0x14E, hp + short);
                }
            }
            // Curse All takes the party as a group.
            // Water Hazard sweeps the party or the monster row as well as a
            // seat, and its arm 3 waits on the row its code names.
            let water_hazard = body.action == 0x71;
            // A spell whose target shape (the record's `+2` at `0x800754C8`)
            // has `& 0x60 == 0x60` aims at the party: the monster picker
            // stores the party code `8` (`0x801EA5D4` -> `sb` at
            // `0x801EA934`), Curse All and Doomsday among them.
            let party_shape =
                cpu.mem.rd8(SPELL_STATS + u32::from(body.action) * 0xC + 2) & 0x60 == 0x60;
            let group = match body.action {
                // Water Hazard's shape `0x64` (`& 0x60 == 0x60`) is the party
                // code `8` the picker stores (`0x801EA5D4` -> `0x801EA934`);
                // a confused caster's retarget `FUN_801E7320` keeps a group
                // a group and crosses it to the monster row's `9`.
                0x71 => [Some(8u8), Some(9)][round % 2],
                _ if party_shape => Some(8u8),
                _ => None,
            };
            if let Some(g) = group {
                cpu.mem.wr8(ca + 0x1DD, g);
            }
            // The tint word the group aim counts a monster seat up by
            // (`FUN_800513F0`'s seating, `0x80051874`).
            // Every other seat rests on retail's neutral word
            // `0x20080200` (a shake or a fade eases from it, Slime Divide's
            // arm 1 among them).
            for s in 0..8 {
                let faded = fell && s >= 3 && cpu.mem.rd16(ACTOR + s * STRIDE + 0x14C) == 0;
                let word = match () {
                    _ if faded && !seat_writes_compared(body) => 0,
                    _ if water_hazard && s >= 3 => 0x1004_0100,
                    _ => 0x2008_0200,
                };
                cpu.mem.wr32(ACTOR + s * STRIDE + 4, word);
            }
            // Plasma Strike's arm 5 waits for its strike clip `0x0D` past
            // a cursor, and arm 6 for the clip's loops to run out.
            if body.action == 0x7B {
                cpu.mem.wr8(ca + 0x1D9, 0x0D);
            }
            // PROT 0958's arm 24 holds while the victim's playing clip
            // `+0x1D9` is its knockdown slot `+0x1F1` (`0x801F8B0C..0x801F8B18`).
            // The clip's end is the animation driver's, which is stubbed, so
            // the rig plays it: on the clip for `round` passes of the arm,
            // then off it.
            const KNOCKDOWN: u8 = 5;
            let blazing = (body.entry, body.va) == (958, BLAZING_SLASH);
            if blazing {
                cpu.mem.wr8(va_ + 0x1F1, KNOCKDOWN);
                // The victim's node cursor (12.4) arms 10, 13 and 16 hold at
                // or past `0x10` (`0x801F78EC..0x801F7908` and the two like
                // it): short of it, on it and well past it.
                cpu.mem
                    .wr16(NODES + victim * 0x100 + 0x68, [0, 9, 0x10, 0x40][round % 4]);
            }
            // Dark Typhoon's pull leaves a seat whose arm-2 mark already
            // stands within `0x200` of the caster at that reach (`slti 0x201`,
            // taken at `0x801F7578`). An action leaves its combatants where it
            // put them, so a party member that struck the caster last stands
            // beside it; rounds 2 and 4 (step-1 rounds, whose swirl the
            // shadow compares) seat one a reach of about `0x130` off. The
            // random seats find it about one seat-round in twenty.
            if (body.entry, body.va) == (942, DARK_TYPHOON) && matches!(round, 2 | 4) {
                let seat = ACTOR + (round / 2) as u32 * STRIDE;
                let (x, z) = (cpu.mem.rd16(ca + 0x34), cpu.mem.rd16(ca + 0x38));
                cpu.mem.wr16(seat + 0x34, x.wrapping_add(0xC0));
                cpu.mem.wr16(seat + 0x38, z.wrapping_sub(0xE0));
            }
            // Thunder Storm: on every other pair of rounds a party member
            // stands in Stone (`+0x16E & 4`), on both sides.
            let storm_stone = (storm && round % 4 >= 2).then_some((round % 3) as u32);
            if let Some(s) = storm_stone {
                let a = ACTOR + s * STRIDE;
                let w0 = cpu.mem.rd16(a + 0x16E);
                cpu.mem.wr16(a + 0x16E, w0 | 4);
            }
            storm_roll.set(if round % 2 == 1 { 0x7FFF } else { 30 });
            // A part standing in the pool, whose level `+0x78` arm 6 sets
            // on every pass (`0x801F78B8..0x801F78E0`).
            if storm {
                for o in (0..0x80).step_by(4) {
                    cpu.mem.wr32(STORM_PART + o, 0);
                }
                cpu.mem.wr32(PART_POOL + 4 * 0x30, STORM_PART);
            }
            let mut shadow = shadow_world(&mut cpu, body, caster, victim, monster);
            if let (Some(s), Some(w)) = (storm_stone, shadow.as_mut()) {
                w.actors[s as usize].battle.field_flags |= 4;
            }
            let mut shadow_live = shadow.is_some();
            // A Stoned party seat (`+0x16E & 4`), which a party sweep skips
            // (`0x801F77A4` in Chaos Breath), on every third round. The
            // shadow is seated without the bit, so it sits those rounds out.
            if party_sweep_arm(body).is_some() && round % 3 == 2 {
                let a = ACTOR + ((round / 3) % 3) as u32 * STRIDE;
                let f = cpu.mem.rd16(a + 0x16E);
                cpu.mem.wr16(a + 0x16E, f | t::FLAG_NON_TARGETABLE);
                shadow_live = false;
            }
            let mut shadow_after: Vec<u32> = Vec::new();
            // The seats whose status the shadow stops comparing for the cast,
            // once its damage left them on other HP than retail's.
            let mut status_left = [false; 8];
            let mut arm_24_passes = 0usize;
            let (mut arm_passes, mut last_phase) = (0usize, 0xFEu8);
            let mut arm3_passes = 0usize;
            let mut arm1_passes = 0usize;
            let mut settle_passes = 0usize;
            let mut doom_settle = 0usize;
            let (mut plasma_phase, mut plasma_passes) = (0xFEu8, 0usize);
            let settle_arm = party_settle_arm(body);
            let pass_step: u8 = if round % 2 == 1 {
                [2u8, 3, 4][(round / 2) % 3]
            } else {
                1
            };
            let mut settle_world = legaia_engine_core::world::World::default();
            // Every strike-frame wait (PROT 0960's arm 5, PROT 0962's arm 3)
            // finds the caster's node cursor past its mark.
            cpu.mem.wr16(NODES + caster * 0x100 + 0x68, 0x7FFF);
            let mut st = ModuleCamState {
                lead_roster: lead,
                opens_pass: true,
                frame_step: 1,
                victim_settled: true,
                caster_in_reach: true,
                party_settled: true,
                target_code: Some(victim as u8),
                ..ModuleCamState::default()
            };
            stream.set(if death_game {
                1 + 0x9E37 * round as u32
            } else if sweeps {
                1 + 0x2F1D * round as u32
            } else if (body.entry, body.va) == (945, WATER_COLUMN) {
                // Water Column's Venom draw ahead of its hit, `rand() & 7 == 0`
                // (`0x801F7460..0x801F746C`), is the round's first draw: a
                // stream a round whose first draw lands the bit on rounds 2
                // and 5, so the shadow's status word is held to it.
                1 + 0x1001 * round as u32
            } else {
                1
            });
            let c0 = format!(
                "PROT {:04} body {:#010x} ({:#04x}) round {round}",
                body.entry, body.va, body.action
            );
            let mut finished = false;
            // Evil Seru Magic's gates re-arm in vsyncs and add up to past
            // three thousand.
            let cap = if body.entry == 966 { 8000 } else { 3000 };
            for pass in 0..cap {
                // The frame's primitive cursor `0x1F8003A0`, which a body's
                // sprites advance by their size (Death Game's digit and
                // pointer, `0x801F8828..0x801F8834`), back at the buffer's
                // head each pass as the frame resets it: left to run, it
                // walks up through RAM and over the spell table.
                cpu.mem.wr32(PRIM_CURSOR, PRIMS);
                let phase = cpu.mem.rd8(CTX + 0x279);
                // PROT 0961's arm 0 stands the seats before its cut; the host
                // runs the line-up ahead of the director
                // (`World::dead_end_crisis_seats`), so the director frames the
                // stood caster.
                let lined = (body.entry, body.va, phase) == (961, DEAD_END_CRISIS, 0);
                let caster_seat = if lined {
                    let mut poses =
                        [cam::LineUpPose::default(); cam::DEAD_END_CRISIS_LINE_UP_SEATS];
                    for (s, p) in poses.iter_mut().enumerate() {
                        let a = ACTOR + s as u32 * STRIDE;
                        p.pos = [0u32, 2, 4, 6].map(|o| cpu.mem.rd16(a + 0x34 + o) as i16);
                        p.facing = cpu.mem.rd16(a + 0x46);
                    }
                    let row_x = std::array::from_fn(|s| {
                        legaia_engine_battle::battle_seats::monster_seat(1, s, false).x
                    });
                    cam::dead_end_crisis_line_up(&mut poses, row_x);
                    let mut seat = seat_at(&mut cpu.mem, ca, false);
                    if let Some(p) = poses.get(caster as usize) {
                        (seat.x, seat.z, seat.facing) = (p.pos[0], p.pos[2], p.facing & 0xFFF);
                    }
                    seat
                } else {
                    seat_at(&mut cpu.mem, ca, false)
                };
                let seats = ModuleCamSeats {
                    caster: caster_seat,
                    victim: seat_at(&mut cpu.mem, va_, false),
                    band_timer: i32::from(cpu.mem.rd16(CTX + 0x6D8) as i16),
                    action: body.action,
                    depth_raw: i32::from(cpu.mem.rd16(CTX + 0x6D0) as i16),
                    victim_radius: cpu.mem.rd16(NODES + victim * 0x100 + 0x58) as i16,
                    caster_scale: 0x1000,
                    caster_monster: monster,
                    first_monster: monster,
                    ..ModuleCamSeats::default()
                };
                // Water Hazard aims once, at arm 0 (`0x801F7394..0x801F73E4`),
                // before it hides the row the aim counts.
                if let Some(g) = group.filter(|_| !water_hazard || phase == 0) {
                    assert_eq!(
                        cpu.call(AIM, &[u32::from(g), OUT_CENTRE, OUT_EXTENT]),
                        Stop::Returned,
                        "{c0} pass {pass} arm {phase}: the group aim {g}"
                    );
                    st.target_aim = Some(GroupAim {
                        centroid_x: cpu.mem.rd16(OUT_CENTRE) as i16,
                        centroid_z: cpu.mem.rd16(OUT_CENTRE + 4) as i16,
                        extent: cpu.mem.rd16(OUT_EXTENT + 4) as i16,
                    });
                    st.target_code = Some(g);
                }
                // What the host feeds a director every pass
                // (`feed_capture_cam`).
                // Melt Spray's arm 3 clears the staged spray clip once it plays
                // (`0x801F828C..0x801F82A0`): on odd rounds the caster is on it.
                if body.action == 0x6F && phase == 3 && round % 2 == 1 {
                    cpu.mem.wr8(ca + 0x1D9, 7);
                }
                // Plasma Strike's clip gates on odd rounds: arm 5 waits for
                // the caster's strike clip `0x0D` (`0x801F7B60`) and its
                // cursor past `0x90` (`0x801F7B78`), arm 6 for its loops to
                // stop reading `0xFF` (`0x801F7C84`), arm 0x0D for its cursor
                // past `0x160` (`0x801F8148`): each not yet for a few passes
                // of the arm, then ready.
                if body.action == 0x7B {
                    plasma_passes = if phase == plasma_phase {
                        plasma_passes + 1
                    } else {
                        0
                    };
                    plasma_phase = phase;
                    let early = round % 2 == 1 && plasma_passes < 4;
                    let cursor = NODES + caster * 0x100 + 0x68;
                    match phase {
                        5 => {
                            let clip_early = early && plasma_passes < 2;
                            cpu.mem
                                .wr8(ca + 0x1D9, if clip_early { 0x0C } else { 0x0D });
                            cpu.mem.wr16(cursor, if early { 0x40 } else { 0x7FFF });
                        }
                        6 => cpu.mem.wr8(ca + 0x21B, if early { 0xFF } else { 0 }),
                        0x0D => cpu.mem.wr16(cursor, if early { 0x100 } else { 0x7FFF }),
                        _ => {}
                    }
                    plasma_gates += usize::from(early && matches!(phase, 5 | 6 | 0x0D));
                }
                st.caster_clip = Some(cpu.mem.rd8(ca + 0x1D9));
                st.caster_frame = Some(cpu.mem.rd16(NODES + caster * 0x100 + 0x68));
                st.caster_loops = Some(cpu.mem.rd8(ca + 0x21B));
                polls.set(0);
                // Steal's caster walks in for the arm's first few passes of
                // each round, which the host feeds as `caster_in_reach`.
                if body.action == 0x51 {
                    arm1_passes = if phase == 1 { arm1_passes + 1 } else { 0 };
                    short.set(phase == 1 && arm1_passes <= round % 3);
                    st.caster_in_reach = !short.get();
                }
                st.caster_body = Some(seat_at(&mut cpu.mem, ca, true));
                st.victim_body = Some(seat_at(&mut cpu.mem, va_, true));
                if blazing {
                    let on = phase != 24 || arm_24_passes < round;
                    arm_24_passes += usize::from(phase == 24);
                    cpu.mem.wr8(va_ + 0x1D9, if on { KNOCKDOWN } else { 0 });
                    st.victim_on_knockdown_clip = on;
                }
                // Evil Seru Magic's arm 0 saves the caster's pose before its
                // re-seat, and arm 27's shot frames it restored; the host
                // keeps the saved pose (`evil_seru_magic_row`).
                if body.entry == 966 && phase == 0 {
                    st.caster_saved = Some((caster_seat.x, caster_seat.z, caster_seat.facing));
                }
                // Death Game's host feeds: the deal and each swap's draws off
                // the stream ahead of the pass, the cards' live points, the
                // pad, and the seats the closing arms wait on.
                if death_game {
                    let lcg = |seed: &mut u32| {
                        *seed = seed.wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
                        (*seed >> 16) & 0x7FFF
                    };
                    let mut seed = stream.get();
                    let g = &mut st.death_game;
                    if phase == 0 {
                        g.deal(monster, || lcg(&mut seed));
                    }
                    if phase == 4 && g.counter == 0 {
                        g.draw_swap(|| lcg(&mut seed));
                    }
                    for i in 0..4u32 {
                        let h = cpu.mem.rd32(CARDS + 4 * i);
                        if h != 0 {
                            g.cards[i as usize] =
                                [0u32, 2, 4].map(|o| cpu.mem.rd16(h + 0x14 + o) as i16);
                        }
                    }
                    arm_passes = if phase == last_phase {
                        arm_passes + 1
                    } else {
                        0
                    };
                    last_phase = phase;
                    // A left press at the home stop, runs of rights to the
                    // far stop (`0x801F8910..0x801F8980`), and every fourth
                    // round no confirm, so the timer picks (`0x801F8B44`).
                    let press = match (phase, arm_passes) {
                        (5, n) if n == round + 3 && round % 4 != 3 => 0x20,
                        (5, n) if n % 5 == 1 => {
                            [0x8000, 0x2000, 0x2000, 0x2000, 0x2000][(n / 5) % 5]
                        }
                        _ => 0,
                    };
                    cpu.mem.wr32(PRESSED, press);
                    g.pad.right = press & 0x2000 != 0;
                    g.pad.left = press & 0x8000 != 0;
                    g.pad.confirm = press & 0x20 != 0;
                    // The closing arms: the victim busy, then down on clip 8;
                    // the caster on a clip, then idle and faded.
                    // The victim settles first; a bled victim that lives
                    // stands back on clip 0 (`0x801F98DC`). The caster then
                    // settles idle on even rounds and fades out on its clip
                    // on odd ones (`0x801F98F0..0x801F9908`).
                    if (10..=12).contains(&phase) {
                        let vdone = arm_passes >= round + 2;
                        let cdone = arm_passes >= round + 2 + round % 3;
                        let survivor = phase == 12 && round % 2 == 1;
                        let rest = if survivor { 0 } else { 8 };
                        let idle = cdone && round % 2 == 0;
                        let faded = cdone && round % 2 == 1;
                        cpu.mem.wr8(va_ + 0x1D9, if vdone { rest } else { 5 });
                        cpu.mem.wr8(ca + 0x1D9, if idle { 0 } else { 1 });
                        cpu.mem
                            .wr32(ca + 4, if idle || faded { 0 } else { 0x2008_0200 });
                        st.victim_settled = vdone;
                        st.caster_settled = idle;
                        st.death_game.caster_faded = idle || faded;
                    }
                }
                // Water Hazard's arm 3: a seat of the target row plays its
                // reaction (or, dead, has not reached its rest) for a few
                // passes, then settles; the host's flag off the same bytes
                // (`World::capture_target_settled`).
                if water_hazard {
                    // Passes of arm 3 since its word ran out (it drains
                    // `scalar` a pass off the `1 << 7` arm 2 left).
                    arm3_passes = if phase == 3 && st.countdown.0 <= SPEED_SCALAR {
                        arm3_passes + 1
                    } else {
                        0
                    };
                    let code = cpu.mem.rd8(ca + 0x1DD);
                    let party = (0..3u32).collect::<Vec<_>>();
                    let row = (3..8u32).collect::<Vec<_>>();
                    let rows = match code {
                        0..=2 => vec![u32::from(code)],
                        8 => party,
                        _ => row,
                    };
                    if phase == 3 {
                        // Not the caster: nothing downs a monster in its
                        // own cast, and its colour and flag are arm 0's.
                        let s = rows
                            .iter()
                            .copied()
                            .cycle()
                            .skip(round % rows.len())
                            .take(rows.len())
                            .find(|&s| s != caster)
                            .unwrap_or(rows[0]);
                        let a = ACTOR + s * STRIDE;
                        let dead = round % 2 == 1;
                        let busy = arm3_passes <= round + 1;
                        if dead {
                            cpu.mem.wr16(a + 0x14C, 0);
                        }
                        let rest = if dead && s < 3 { 8 } else { 0 };
                        cpu.mem
                            .wr8(a + 0x1D9, if busy && !(dead && s >= 3) { 5 } else { rest });
                        if dead && s >= 3 {
                            cpu.mem.wr32(a + 4, if busy { 0x2008_0200 } else { 0 });
                        }
                    }
                    let settled = |cpu: &mut Cpu, s: u32| {
                        let a = ACTOR + s * STRIDE;
                        let clip = cpu.mem.rd8(a + 0x1D9);
                        match (cpu.mem.rd16(a + 0x14C) != 0, s < 3) {
                            (true, _) => clip == 0,
                            (false, true) => clip == 8,
                            (false, false) => cpu.mem.rd32(a + 4) == 0,
                        }
                    };
                    st.target_unsettled = !rows.iter().all(|&s| settled(&mut cpu, s));
                    unsettled_holds += usize::from(phase == 3 && st.target_unsettled);
                }
                // Kiss of Death's arm 5, Melt Spray's arm 4 and Scythe Wind's
                // arm 5 wait, once their
                // word runs out, on the victim's clip: a standing victim's
                // still playing (`0x801F8E60` / `0x801F8644`), a fallen one's
                // not yet the fallen clip `8` (`0x801F8E38` / `0x801F861C`),
                // for round-keyed passes; the host's flag off the same bytes
                // (`World::chain_seat_settled`).
                let victim_settle = match body.action {
                    0x6E => Some(5),
                    0x6F => Some(4),
                    // Scythe Wind's arm 5 (`0x801F80E8..0x801F8128`), and
                    // Gigaton Press's arm 4 (`0x801F7440..0x801F7454`, a clip of
                    // `0` or `8` whatever the HP).
                    0x5B if body.va == SCYTHE_WIND => Some(5),
                    _ if (body.entry, body.va) == (962, GIGATON_PRESS) => Some(4),
                    // Blade Breath's arm 4, the same poll (`0x801F7AE4` body).
                    _ if (body.entry, body.va) == (962, BLADE_BREATH) => Some(4),
                    // Bloody Horns' arm 6 (`0x801F7AB4..0x801F7AE4`): a live
                    // victim's clip `0`, a fallen one's `8`.
                    0x5C if body.va == BLOODY_HORNS => Some(6),
                    // Power Charge's arm 3 (`0x801F758C..0x801F75F4`), on the
                    // monster seat it aims at, which stands.
                    0x72 => Some(3),
                    // Water Column's arm 7 (`0x801F7654..0x801F769C`), once
                    // its word runs out.
                    _ if water_column => Some(7),
                    // Water Crystals' arm 5 (`0x801F7438..0x801F7478`): a
                    // fallen victim settles on clip `8` or faded out.
                    _ if body.entry == 949 => Some(5),
                    _ => None,
                };
                if let Some(arm_at) = victim_settle {
                    // Gigaton Press polls on every pass of its arm, its shake
                    // word drained beside the poll rather than gating it.
                    let gated = body.entry != 962;
                    // Water Column's word drains in the band, not its director:
                    // the settle opens once retail's word `0x801F7E5C` is spent.
                    let spent = if water_column {
                        cpu.mem.rd32(cam::WATER_COLUMN_COUNTDOWN) as i32 <= 0
                    } else {
                        st.countdown.0 <= SPEED_SCALAR
                    };
                    settle_passes = if phase == arm_at && (!gated || spent) {
                        settle_passes + 1
                    } else {
                        0
                    };
                    if phase == arm_at {
                        // A victim the strike downed on every fourth round
                        // (Kiss of Death's own hit downs its one-HP victims).
                        if !matches!(body.action, 0x6E | 0x72)
                            && round % 4 == 3
                            && settle_passes == 1
                        {
                            cpu.mem.wr16(va_ + 0x14C, 0);
                        }
                        let hp = cpu.mem.rd16(va_ + 0x14C);
                        let dead = hp == 0;
                        let busy = settle_passes <= round % 3 + 1;
                        // While busy a standing victim plays a reaction (or,
                        // on odd rounds, the down clip `8`) and a fallen one a
                        // knockdown (or, on odd rounds, idle `0`): the clips
                        // the two rest rules read apart.
                        let clip = match (busy, dead) {
                            (true, false) => [5, 8][round % 2],
                            (true, true) => [3, 0][round % 2],
                            (false, false) => 0,
                            (false, true) => 8,
                        };
                        cpu.mem.wr8(va_ + 0x1D9, clip);
                        // Water Crystals' fallen victim stands lit while
                        // busy, then on odd rounds fades out off its clip.
                        if body.entry == 949 && dead {
                            let word = if busy { 0x2008_0200 } else { 0 };
                            if busy || round % 2 == 1 {
                                cpu.mem.wr32(va_ + 4, word);
                            }
                            if !busy && round % 2 == 1 {
                                cpu.mem.wr8(va_ + 0x1D9, 0);
                            }
                        }
                        let clip = cpu.mem.rd8(va_ + 0x1D9);
                        while settle_world.actors.len() <= victim as usize {
                            settle_world.actors.push(Default::default());
                        }
                        let sa = &mut settle_world.actors[victim as usize];
                        sa.active = true;
                        sa.battle.render_color = cpu.mem.rd32(va_ + 4);
                        // The host's own flag (`World::capture_victim_settled`)
                        // off the same bytes.
                        st.victim_settled = settle_world.drive_capture_victim_settled(
                            body.entry,
                            body.va,
                            victim as u8,
                            hp,
                            clip,
                        );
                        victim_settle_holds += usize::from(!st.victim_settled && settle_passes > 0);
                    } else {
                        st.victim_settled = true;
                    }
                }
                // The bodies whose held arms re-aim every pass
                // (`capture_shot_rearms_every_pass`) run their odd rounds at a
                // battle frame step of `2`, `3` or `4`: retail's one call
                // drains the pass's `scalar * step` in one `subu`, which a
                // countdown off a step that does not divide it takes below
                // zero (the `bgez` / `addiu 7` fix-ups of the shot's `cd / 8`,
                // `0x801F7C8C`, `0x801F7DC0`). The director runs a tick a
                // vsync as the host's pass does (`World::module_pass`): `step`
                // ticks, the countdown's rest `step - 1 - i`, stopping on the
                // tick an arm hands over, the spawns its opening tick seats.
                // Retail's step moves from frame to frame: the odd rounds walk
                // 2, 3, 4, 3, so a word the steps share no divisor with goes
                // below zero on its last pass.
                // Death Game's pick timer `0x960` is `8 * step` times a whole
                // number of passes at any one step, and at that walk too; its
                // odd rounds walk 4, 3, whose sums pass `0x960 / 8` by one or
                // two, so the timer's last drain goes below zero and is
                // clamped (`0x801F880C..0x801F8814`).
                let step = if pass_step > 1 && death_game {
                    [4u8, 3][(pass + usize::from(pass_step)) % 2]
                } else if pass_step > 1 {
                    [2u8, 3, 4, 3][(pass + usize::from(pass_step)) % 4]
                } else {
                    1
                };
                cpu.mem.wr8(STEP, step);
                st.frame_step = step;
                // Doomsday's arm 0x0C and Chaos Breath's arm 3, once their
                // word runs out, wait on every `ctx[+0]` party seat
                // (`0x801F797C..0x801F79F4` in both images): a live seat
                // until idle, a downed one until on clip `8`. Each seat plays
                // for round-keyed passes (on odd rounds one is downed as the
                // wait opens), then rests; the host's flag off the same bytes
                // (`World::capture_party_settled`).
                if let Some((at, word_va, drain)) = settle_arm {
                    let word = cpu.mem.rd32(word_va) as i32;
                    // Doomsday and Neo Star Slash drain and walk on one pass;
                    // Chaos Breath walks on the pass after its word reads `0`.
                    let ready = if drain > 0 {
                        word <= drain * i32::from(step)
                    } else {
                        word == 0
                    };
                    doom_settle = if phase == at && ready {
                        doom_settle + 1
                    } else {
                        0
                    };
                    st.party_settled = true;
                    if phase == at {
                        let seats: Vec<(u16, u8)> = (0..3u32)
                            .map(|s| {
                                let a = ACTOR + s * STRIDE;
                                if doom_settle == 1
                                    && round % 2 == 1
                                    && s as usize == (round / 2) % 3
                                {
                                    cpu.mem.wr16(a + 0x14C, 0);
                                }
                                let hp = cpu.mem.rd16(a + 0x14C);
                                let busy =
                                    doom_settle > 0 && doom_settle <= (round + s as usize) % 4;
                                let clip = match (busy, hp == 0) {
                                    (true, false) => [5, 8][round % 2],
                                    (true, true) => [3, 0][round % 2],
                                    (false, false) => 0,
                                    (false, true) => 8,
                                };
                                cpu.mem.wr8(a + 0x1D9, clip);
                                (hp, clip)
                            })
                            .collect();
                        st.party_settled = settle_world.drive_capture_party_settled(&seats);
                        doom_holds += usize::from(!st.party_settled);
                    }
                }
                let mut arm = cam::CaptureCamArm::default();
                let mut opened_spawns = None;
                for i in 0..step {
                    st.countdown.1 = step - 1 - i;
                    st.opens_pass = i == 0;
                    // The pad's edge reaches the pass's first tick alone, as
                    // the host feeds it (`World::feed_death_game`).
                    if i > 0 {
                        st.death_game.pad = Default::default();
                    }
                    arm = (body.director)(&mut st, phase, seats);
                    if i == 0 {
                        opened_spawns = Some(arm.pass_spawns);
                    }
                    if !arm.hold {
                        break;
                    }
                }
                if let Some(sp) = opened_spawns {
                    arm.pass_spawns = sp;
                }
                st.countdown.1 = 0;
                st.opens_pass = true;
                // The band's own pass on the shadow world, while it walks
                // the phase retail walks.
                let shadow_phase = shadow.as_ref().map(|w| w.casting.module_phase);
                if shadow_live && shadow_phase != Some(phase) {
                    shadow_live = false;
                    shadow_diverged += 1;
                }
                let mut storm_port: Option<Vec<(u16, bool)>> = None;
                let mut storm_before: Vec<(u16, u16)> = Vec::new();
                storm_hits.borrow_mut().clear();
                if shadow_live && let Some(w) = shadow.as_mut() {
                    // The seats' HP as retail's pass opens on it: the band's
                    // damage rolls are not the stubbed wrapper's, and a seat
                    // write keyed on HP should read retail's.
                    // A colour word or clip this harness stages between
                    // passes (a defeat fade, a settle) is the battle's, not
                    // the module's: it reaches the shadow as it reached
                    // retail.
                    for s in 0..8u32 {
                        let a = ACTOR + s * STRIDE;
                        let act = &mut w.actors[s as usize];
                        act.battle.hp = cpu.mem.rd16(a + 0x14C);
                        let word = cpu.mem.rd32(a + 4);
                        if shadow_after.get(s as usize).is_some_and(|&was| was != word) {
                            act.battle.render_color = word;
                        }
                    }
                    // Water Crystals' settle reads the victim's clip: the clip
                    // the harness stands it on reaches the shadow as well.
                    if body.entry == 949 && victim_settle == Some(phase) && victim < 8 {
                        let act = &mut w.actors[victim as usize];
                        act.battle.current_anim = cpu.mem.rd8(va_ + 0x1D9);
                        act.battle_reaction = None;
                        act.battle_reaction_entry = None;
                        act.battle_pose = None;
                        act.battle_action_clips = Some(std::sync::Arc::new(Vec::new()));
                    }
                    w.clock.display_frames += u64::from(step);
                    // Retail's `rand()` stream as this pass opens on it, so a
                    // rolled status lands on the seat retail's roll lands.
                    w.rng_state = stream.get();
                    // The band's pass (`0x70`), its ticks as the engine runs
                    // them at the pass's step (`World::module_pass`), with
                    // the after-pass seam (`World::caster_stages`): Glare's
                    // roll on the pass that counts its arm 2 out.
                    storm_before = (0..3)
                        .map(|s| (w.actors[s].battle.hp, w.raw_status_word(s as u8)))
                        .collect();
                    w.drive_capture_module_pass(body.action, step);
                    if storm {
                        storm_port = Some(
                            (0..3)
                                .map(|s| {
                                    (w.actors[s].battle.hp, w.actors[s].battle_reaction.is_none())
                                })
                                .collect::<Vec<_>>(),
                        );
                    }
                }
                let ctx_before = crate::cast_view::read_ctx(&mut cpu.mem, CTX);
                // The two seats as the pass opens, for Death Game's result.
                let before = [ca, va_].map(|a| crate::cast_view::read_actor(&mut cpu.mem, a));
                let before_party: Vec<_> = (0..3u32)
                    .map(|s| crate::cast_view::read_actor(&mut cpu.mem, ACTOR + s * STRIDE))
                    .collect();
                let horn_before = (cpu.mem.rd16(hit_at + 0x14C), cpu.mem.rd32(hit_at + 0x10));
                let horn_calls_before = horn_calls.get();
                // Arms 12 and 15 send the victim into its knockdown, whose
                // clip the stubbed driver never restarts: on odd rounds the
                // rig rewinds the cursor ahead of arms 13 and 16, as a fresh
                // clip would leave it.
                if blazing && matches!(phase, 13 | 16) && round % 2 == 1 && pass % 3 == 0 {
                    cpu.mem
                        .wr16(NODES + victim * 0x100 + 0x68, (pass % 0x10) as u16);
                }
                let victim_cursor = cpu.mem.rd16(NODES + victim * 0x100 + 0x68);
                // Every seat's tint and render flag as the pass opens, with
                // what decides which seats a hide or show reaches: HP, Stone.
                let shown: Vec<(u32, u8, u16, u16)> = (0..8u32)
                    .map(|s| {
                        let a = ACTOR + s * STRIDE;
                        (
                            cpu.mem.rd32(a + 4),
                            cpu.mem.rd8(a + 0x21C),
                            cpu.mem.rd16(a + 0x14C),
                            cpu.mem.rd16(a + 0x16E),
                        )
                    })
                    .collect();
                // A seated part that runs out on its own while arms 1 and 2
                // still hold its handle: every third, on odd rounds.
                if body.action == 0x6E && (phase == 1 || phase == 2) && round % 2 == 1 {
                    for (i, &(_, part)) in seated.borrow().iter().enumerate() {
                        if i % 3 == 2 {
                            let f = cpu.mem.rd32(part + 0x10);
                            cpu.mem.wr32(part + 0x10, f | 8);
                        }
                    }
                }
                shots.borrow_mut().clear();
                spawn_log.borrow_mut().clear();
                let stage_before = cpu.mem.rd8(ca + 0x1DA);
                let stream_before = stream.get();
                let stop = cpu.call(body.va, &[]);
                let c = format!("{c0} pass {pass} arm {phase} step {step}");
                if stop != Stop::Returned {
                    failures.push(format!("{c}: {stop:?}"));
                    break;
                }
                // The records the director seats at points it works out,
                // each against a spawn retail's pass made with the same
                // record, point, angles and scale.
                if !death_game {
                    let mut left = spawn_log.borrow().clone();
                    for sp in arm.charge_spawns.iter().chain(&arm.pass_spawns).flatten() {
                        let (
                            cam::SpawnRecord::Module(r),
                            cam::SpawnAnchor::LiteralScaled { pos, rot, scale },
                        ) = (sp.record, sp.anchor)
                        else {
                            continue;
                        };
                        match left.iter().position(|&x| x == (r, pos, rot, scale)) {
                            Some(i) => {
                                left.remove(i);
                                spawns_matched += 1;
                            }
                            None => spawn_failures.push(format!(
                                "{c}: spawn {r:#x} at {pos:?} {rot:?} x{scale:#x}: retail {:x?}",
                                left.iter().filter(|x| x.0 == r).collect::<Vec<_>>()
                            )),
                        }
                    }
                }
                passes += 1;
                // Steal's voice on the pass that leaves arm 1 or 2, against
                // the kernel the host rings (`steal_voice`).
                if body.action == 0x51 {
                    let got: Vec<(u32, u32)> = std::mem::take(&mut *voices.borrow_mut());
                    let left = cpu.mem.rd8(CTX + 0x279) != phase;
                    let want: Vec<(u32, u32)> = cam::steal_voice(phase, monster)
                        .filter(|_| left)
                        .map(|id| (u32::from(id), caster))
                        .into_iter()
                        .collect();
                    if got != want {
                        failures.push(format!("{c}: voices retail {got:x?} port {want:x?}"));
                    }
                    steal_voices += got.len();
                }
                let busy = cpu.regs.r[V0] != 0;
                // The last builder call of a pass is the tween the camera runs.
                let got = shots.borrow().last().copied();
                let want = arm.shot.map(|s| Shot {
                    angles: s.angles,
                    tr: s.tr,
                    focus: s.focus,
                    frames: s.frames,
                });
                // The builder masks the yaw; compare it as it is held.
                let held = |s: Option<Shot>| {
                    s.map(|mut s| {
                        s.angles[1] &= 0xFFF;
                        s
                    })
                };
                let known = KNOWN_GAPS
                    .iter()
                    .any(|g| (g.0, g.1, g.2) == (body.entry, body.va, phase));
                if held(got) != held(want) && !known {
                    failures.push(format!("{c}: shot retail {got:?} port {want:?}"));
                }
                let after = cpu.mem.rd8(CTX + 0x279);
                // The buffs' stat writes, on the pass that leaves their arm,
                // against the kernels the host runs there.
                if after != phase && is_buff(body) {
                    let mut pc = before[0];
                    let mut pctx = crate::cast_view::read_ctx(&mut cpu.mem, CTX);
                    pctx.phase = phase;
                    let rec = cpu.mem.rd32(RECORDS + 4 * (caster - 3));
                    let agl = cpu.mem.rd16(rec + 0x0E);
                    match body.va {
                        t::WHITE_SHIELD_TICK => {
                            let d = (cpu.mem.rd16(rec + 0x14), cpu.mem.rd16(rec + 0x16));
                            t::white_shield_tick(&mut pctx, &mut pc, d);
                        }
                        t::POWER_CHARGE_TICK => {
                            t::power_charge_tick(&mut pctx, &mut pc);
                        }
                        t::POWER_UP_TICK => {
                            t::power_up_tick(&mut pctx, &mut pc, agl);
                        }
                        _ => {
                            t::all_stats_surge_tick(&mut pctx, &mut pc, agl);
                        }
                    }
                    let got = crate::cast_view::read_actor(&mut cpu.mem, ca);
                    let key = |x: &t::CastActorState| {
                        (
                            [x.agl, x.atk, x.udf, x.ldf, x.spd, x.intel],
                            [
                                x.agl_base,
                                x.atk_base,
                                x.udf_base,
                                x.ldf_base,
                                x.spd_base,
                                x.intel_base,
                            ],
                        )
                    };
                    if key(&got) != key(&pc) {
                        failures.push(format!(
                            "{c}: buff retail {:?} port {:?}",
                            key(&got),
                            key(&pc)
                        ));
                    }
                    buffs += 1;
                }
                // Every seat's tint and render flag as the band left them
                // against retail's, while the two walk one phase.
                // Death Game's wheel stops on the pad, which reaches retail's
                // pass and not the shadow's: the shadow follows it to the
                // spin (arm 9).
                if body.action == 0x76 && phase >= 9 {
                    shadow_live = false;
                }
                // A party sweep's rolls here are the hooked wrapper's, not
                // the band's own, and so is its share of the `rand()` stream:
                // its status draws are compared against the tick kernel
                // below, not on the shadow.
                if party_sweep_arm(body) == Some(phase) && after != phase {
                    status_left[..3].fill(true);
                }
                if shadow_live && let Some(w) = shadow.as_ref() {
                    for (s, left) in status_left.iter_mut().enumerate() {
                        // PROT 0964's and 0966's creature stands in seat 7
                        // through `FUN_801F19EC`, which the image here runs
                        // stubbed.
                        if s == 7 && matches!(body.entry, 964 | 966) {
                            continue;
                        }
                        let a = ACTOR + s as u32 * STRIDE;
                        let got = (cpu.mem.rd32(a + 4), cpu.mem.rd8(a + 0x21C));
                        let port = (
                            w.actors[s].battle.render_color,
                            w.actors[s].battle.render_flag,
                        );
                        if got != port {
                            shadow_failures.push(format!(
                                "{c}: band seat {s} tint/flag retail {got:x?} port {port:x?} hp retail {} port {}",
                                cpu.mem.rd16(a + 0x14C),
                                w.actors[s].battle.hp
                            ));
                        }
                        // The status word `+0x16E` a body's arms OR into
                        // (Curse All's mark, Glare's roll, Paralyzing Wave's
                        // and Kiss of Death's bits), against the packed word
                        // the engine's consumers read. A seat the band's own
                        // damage roll left on other HP than retail's stubbed
                        // wrapper did is not compared: a mark that tests the
                        // victim's life (PROT 0956's arm 2) reads a different
                        // victim, for the rest of the cast.
                        let status = (cpu.mem.rd16(a + 0x16E), w.raw_status_word(s as u8));
                        if cpu.mem.rd16(a + 0x14C) != w.actors[s].battle.hp {
                            *left = true;
                        }
                        if !*left && status.0 != 0 {
                            status_marks += 1;
                        }
                        if !*left && status.0 != status.1 {
                            shadow_failures.push(format!(
                                "{c}: band seat {s} status retail {:#06x} port {:#06x} hp retail {} port {}",
                                status.0,
                                status.1,
                                cpu.mem.rd16(a + 0x14C),
                                w.actors[s].battle.hp
                            ));
                        }
                    }
                    // Water Column's Venom bit, drawn ahead of its hit: the
                    // fold's damage roll leaves the victim on other HP than
                    // the stubbed wrapper's, which stops the status compare
                    // above for the seat, so the bit the draw writes is held
                    // on its own.
                    if (body.entry, body.va) == (945, WATER_COLUMN) {
                        let a = ACTOR + victim * STRIDE;
                        let got = cpu.mem.rd16(a + 0x16E) & 1;
                        let port = w.raw_status_word(victim as u8) & 1;
                        if got != port {
                            shadow_failures.push(format!(
                                "{c}: Water Column victim {victim} Venom retail {got} port {port}"
                            ));
                        }
                        venom_marks += usize::from(got != 0);
                    }
                    // Dark Typhoon's swirl (`World::dark_typhoon_party_swirl`):
                    // each party seat's plane pair, height and heading against
                    // retail's arms 2..=5 (the marks `0x801F708C..0x801F70F8`,
                    // the pull `0x801F7488..0x801F76AC`, the let-down
                    // `0x801F7AA8..0x801F7B94`), at every step: the pull loop
                    // runs once on the pass's opening tick at the pass's step,
                    // as retail's one call grows it once by `2 * step`.
                    // Chaos Breath's arm-1 shove (`0x801F74B4..0x801F7568`):
                    // each party seat's plane pair, pushed once a pass by
                    // `(table * 2 * step) >> 12`, at every step.
                    if (body.entry, body.va) == (938, CHAOS_BREATH) && phase == 1 {
                        for s in 0..3u32 {
                            let a = ACTOR + s * STRIDE;
                            let m = &w.actors[s as usize].move_state;
                            let got =
                                [cpu.mem.rd16(a + 0x34) as i16, cpu.mem.rd16(a + 0x38) as i16];
                            let port = [m.world_x, m.world_z];
                            if got != port {
                                shadow_failures.push(format!(
                                    "{c}: Chaos Breath seat {s} pair retail {got:?} port {port:?}"
                                ));
                            }
                            shove_poses += 1;
                        }
                    }
                    if (body.entry, body.va) == (942, DARK_TYPHOON) {
                        for s in 0..3u32 {
                            let a = ACTOR + s * STRIDE;
                            let m = &w.actors[s as usize];
                            let got = [0x34u32, 0x36, 0x38].map(|o| cpu.mem.rd16(a + o) as i16);
                            let port = [
                                m.move_state.world_x,
                                m.move_state.world_y,
                                m.move_state.world_z,
                            ];
                            let facing = (
                                cpu.mem.rd16(a + 0x46) & 0xFFF,
                                m.battle.facing_angle & 0xFFF,
                            );
                            if got != port || facing.0 != facing.1 {
                                shadow_failures.push(format!(
                                    "{c}: Dark Typhoon seat {s} pose retail {got:?} {:#x} port {port:?} {:#x}",
                                    facing.0, facing.1
                                ));
                            }
                            typhoon_poses += 1;
                        }
                    }
                    shadow_passes += 1;
                }
                shadow_after = (0..8u32)
                    .map(|s| cpu.mem.rd32(ACTOR + s * STRIDE + 4))
                    .collect();
                // Thunder Storm's party sweep (`0x801F74E0..0x801F75D0`)
                // against the host's fold: a seat retail's sweep rolls (alive,
                // out of Stone, `0x801F74E8` / `0x801F74FC`) the fold strikes,
                // and a Stoned one it leaves whole and unreacting; a roll past
                // HP leaves it on `0` (`0x801F7540..0x801F754C`), and the
                // fold's own clamp never wraps a seat past its HP.
                if let Some(port) = storm_port.as_ref() {
                    let rolled: Vec<u32> = storm_hits.borrow().clone();
                    if !rolled.is_empty() {
                        for (s, (&(hp, status), &(port_hp, calm))) in
                            storm_before.iter().zip(port).enumerate()
                        {
                            let stoned = status & 4 != 0;
                            let hit = rolled.contains(&(s as u32));
                            let want_hit = hp != 0 && !stoned;
                            let retail_hp = cpu.mem.rd16(ACTOR + s as u32 * STRIDE + 0x14C);
                            let ok = hit == want_hit
                                && (!stoned || (port_hp == hp && calm))
                                && port_hp <= hp;
                            storm_seats += 1;
                            storm_stoned += usize::from(stoned);
                            // The fold rolls the port's own damage, not the
                            // stubbed roll: its clamp shows as a struck seat
                            // left on `0`, never wrapped past its HP.
                            storm_clamps +=
                                usize::from(hit && retail_hp == 0 && port_hp == 0 && hp != 0);
                            if !ok {
                                shadow_failures.push(format!(
                                    "round {round} arm {phase}: Thunder Storm seat {s} hp {hp} stoned {stoned}: retail rolled {hit} (hp {retail_hp}), port hp {port_hp} calm {calm}"
                                ));
                            }
                        }
                    }
                }
                // Thunder Storm's pool level against the director's.
                if storm && phase == 6 {
                    let got = cpu.mem.rd16(STORM_PART + 0x78);
                    storm_levels += 1;
                    if Some(got) != arm.parts_level {
                        shadow_failures.push(format!(
                            "round {round} arm 6: Thunder Storm pool level retail {got:#x} port {:x?}",
                            arm.parts_level
                        ));
                    }
                }
                // Bloody Horns' hit against the port's clamp
                // (`cast_module_ticks::apply_hit_floor_zero`): the roll, cut
                // to the victim's HP, onto `+0x10` and off `+0x14C`.
                if (body.entry == 952 && body.va == BLOODY_HORNS)
                    || water_column
                    || crystals
                    || (body.entry, body.va) == (958, BLAZING_SLASH)
                    || (body.entry, body.va) == (960, PLASMA_STRIKE)
                {
                    let (hp, acc) = horn_before;
                    if horn_calls.get() != horn_calls_before {
                        let mut v = legaia_engine_cast::cast_module_ticks::CastActorState {
                            hp,
                            hp_bar_delta: acc as i32,
                            ..Default::default()
                        };
                        legaia_engine_cast::cast_module_ticks::apply_hit_floor_zero(
                            &mut v,
                            horn_roll.get() as i32,
                        );
                        let got = (cpu.mem.rd16(hit_at + 0x14C), cpu.mem.rd32(hit_at + 0x10));
                        if got != (v.hp, v.hp_bar_delta as u32) {
                            failures.push(format!(
                                "{c}: horn hit hp/+0x10 retail {got:?} port {:?}",
                                (v.hp, v.hp_bar_delta as u32)
                            ));
                        }
                        horn_hits += 1;
                        horn_clamps += usize::from(horn_roll.get() > u32::from(hp));
                    }
                }
                // Plasma Strike's combo landing on the pass that leaves arm
                // 0x0C, against `plasma_strike_tick`.
                if body.action == 0x7B && phase == 0x0C && after != phase {
                    let mut pctx = ctx_before;
                    pctx.phase = phase;
                    let (mut pc, mut pv) = (before[0], before[1]);
                    pv.hits_pending = false;
                    t::plasma_strike_tick(&mut pctx, &mut pc, &mut pv, None);
                    let got = crate::cast_view::read_actor(&mut cpu.mem, va_);
                    if (got.hp, got.combo_total) != (pv.hp, pv.combo_total) {
                        failures.push(format!(
                            "{c}: combo landing hp/+0x00 retail {:?} port {:?}",
                            (got.hp, got.combo_total),
                            (pv.hp, pv.combo_total)
                        ));
                    }
                    combo_lands += usize::from(before[1].combo_total > u32::from(before[1].hp));
                }
                // Blazing Slash's victim cursor stores (the floor and the
                // resets), against the director's victim write.
                if blazing {
                    let w = &arm.victim_write;
                    let mut want = if w.cursor_reset { 0 } else { victim_cursor };
                    if let Some(f) = w.seek_floor
                        && (want as i16) < f as i16
                    {
                        want = f;
                    }
                    let got = cpu.mem.rd16(NODES + victim * 0x100 + 0x68);
                    if got != want {
                        failures.push(format!(
                            "{c}: victim cursor retail {got:#x} port {want:#x} (from {victim_cursor:#x})"
                        ));
                    }
                    cursor_floors += usize::from(w.seek_floor.is_some() && victim_cursor < 0x10);
                }
                // Doomsday's party sweep on the pass that leaves arm 0x0B,
                // against `doomsday_tick` on the same rolls: each live seat's
                // roll cut to its HP onto `+0x10` and off `+0x14C`, its
                // knockdown staged (`0x801F77E0..0x801F7840`).
                if party_sweep_arm(body) == Some(phase) && after != phase {
                    let rolls: Vec<i32> = before_party
                        .iter()
                        .enumerate()
                        .map(|(s, v)| doomsday_roll(round, s as u32, u32::from(v.hp)) as i32)
                        .collect();
                    let mut pctx = ctx_before;
                    pctx.phase = phase;
                    let mut seats = before_party.clone();
                    let hits = if body.entry == 965 {
                        t::doomsday_tick(&mut pctx, &mut seats, true, |s| rolls[usize::from(s)]).1
                    } else if body.entry == 941 {
                        steal_sweep_tick(&mut pctx, &mut seats, true, |s| rolls[usize::from(s)]).1
                    } else {
                        // Chaos Breath's two 1-in-8 draws per hittable seat
                        // (`0x801F7888`, the second only on a miss at
                        // `0x801F78B0`), off the stream as the pass opened.
                        let mut seed = stream_before;
                        let mut draw = || {
                            seed = seed.wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
                            (seed >> 16) & 0x7FFF
                        };
                        let draws: Vec<(u32, u32)> = before_party
                            .iter()
                            .map(|v| {
                                if !t::aoe_seat_is_hittable(v) {
                                    return (1, 1);
                                }
                                let a = draw();
                                (a, if a & 7 != 0 { draw() } else { 1 })
                            })
                            .collect();
                        status_draws += draws
                            .iter()
                            .filter(|d| d.0 & 7 == 0 || d.1 & 7 == 0)
                            .count();
                        let mut caster = before[0];
                        t::chaos_breath_tick(
                            &mut pctx,
                            &mut caster,
                            &mut seats,
                            |s| rolls[usize::from(s)],
                            |s| draws[usize::from(s)],
                        )
                        .1
                    };
                    for (s, port) in seats.iter().enumerate() {
                        let got =
                            crate::cast_view::read_actor(&mut cpu.mem, ACTOR + s as u32 * STRIDE);
                        let key = |v: &t::CastActorState| {
                            (
                                v.hp,
                                v.hp_bar_delta,
                                v.staged_anim,
                                v.restage,
                                v.flags,
                                // Doomsday's `+0x04` store is its director's party
                                // show (`0x801F76E4..0x801F76F0`).
                                if body.entry == 965 { 0 } else { v.present_04 },
                            )
                        };
                        if key(&got) != key(port) {
                            failures.push(format!(
                                "{c}: sweep seat {s} hp/+0x10/+0x1DA/+0x1DC/+0x16E/+0x04 retail {:x?} port {:x?}",
                                key(&got),
                                key(port)
                            ));
                        }
                        doom_clamps += usize::from(
                            before_party[s].hp != 0 && rolls[s] > i32::from(before_party[s].hp),
                        );
                    }
                    doom_hits += hits.len();
                }
                // That store against the director's caster write.
                if body.action == 0x6F && phase == 3 {
                    let staged = cpu.mem.rd8(ca + 0x1DA);
                    let port = arm.caster_write.stage.unwrap_or(stage_before);
                    if staged != port {
                        failures.push(format!(
                            "{c}: caster stage retail {staged:#x} port {port:#x}"
                        ));
                    }
                    spray_clears += usize::from(arm.caster_write.stage == Some(0));
                }
                // Kiss of Death's arm 2 leaving: every part of a record the
                // port halts on the arm stopped (`+0x10 & 8`), the rest not.
                if body.action == 0x6E && phase == 2 && after != phase {
                    let halts = cam::capture_arm_halts_records(body.entry, phase);
                    for &(record, part) in seated.borrow().iter() {
                        let got = cpu.mem.rd32(part + 0x10) & 8 != 0;
                        if got != halts.contains(&record) {
                            failures.push(format!(
                                "{c}: part {part:#x} of record {record:#x} stopped retail {got} port {}",
                                !got
                            ));
                        }
                        kiss_halts += 1;
                    }
                }
                // Glare's Stone on the pass that leaves arm 2: the queued
                // action it cancels and the Item it refunds, against the
                // shadow's (`World::stone_cancels_queued_action`).
                if body.action == 0x3C
                    && phase == 2
                    && after != phase
                    && let Some(w) = shadow.as_ref().filter(|_| shadow_live)
                {
                    let called = refunds.borrow_mut().drain(..).collect::<Vec<_>>();
                    let mut bag: Vec<(u32, u32)> = w
                        .party
                        .inventory
                        .iter()
                        .filter(|&(_, &n)| n > 0)
                        .map(|(&id, &n)| (u32::from(id), u32::from(n)))
                        .collect();
                    bag.sort_unstable();
                    let got = (called, cpu.mem.rd8(va_ + 0x1DE));
                    let port = (bag, w.actors[victim as usize].battle.action_category);
                    if got != port {
                        failures.push(format!(
                            "{c}: Stone's cancel and refund retail {got:x?} port {port:x?}"
                        ));
                    }
                    glare_rolls += 1;
                }
                // Terror Scream's turn steal on the pass that leaves arm 3,
                // against `terror_scream_tick`.
                if body.action == 0x70 && phase == 3 && after != phase {
                    let mut pv = before[1];
                    let mut pctx = ctx_before;
                    pctx.phase = phase;
                    let (_, refund) = t::terror_scream_tick(&mut pctx, &mut pv);
                    let got = crate::cast_view::read_actor(&mut cpu.mem, va_);
                    let key = |v: &t::CastActorState, cursor: u8, refund: Option<u8>| {
                        (v.action_category, v.init_key, cursor, refund)
                    };
                    let called = refunds.borrow_mut().drain(..).collect::<Vec<_>>();
                    let retail_refund = match called.as_slice() {
                        [] => None,
                        [(item, 1)] => Some(*item as u8),
                        other => {
                            failures.push(format!("{c}: refund calls {other:?}"));
                            None
                        }
                    };
                    let g = key(&got, cpu.mem.rd8(CTX + 0x1A), retail_refund);
                    let w = key(&pv, pctx.turn_cursor, refund);
                    if g != w {
                        failures.push(format!("{c}: turn steal retail {g:?} port {w:?}"));
                    }
                    steals += 1;
                }
                // The seat hides and shows (`World::module_code_run`'s order:
                // the others, the caster, the victim, the party), on the tint
                // `+0x04` and the render flag `+0x21C`.
                if seat_writes_compared(body) {
                    let mut want: Vec<(u32, u8)> =
                        shown.iter().map(|&(c, f, _, _)| (c, f)).collect();
                    let put = |w: &cam::CaptureSeatWrite, v: &mut (u32, u8)| {
                        if let Some(c) = w.colour {
                            v.0 = c;
                        }
                        if let Some(f) = w.render_flag {
                            v.1 = f;
                        }
                    };
                    // The modules walk seats `0..7` (`sltiu 7`); seat 7 is the
                    // creature's, empty on a monster's cast, which the host's
                    // HP test skips.
                    if (!arm.hold || arm.others_every_pass) && !arm.others_write.is_empty() {
                        for s in 0..7u32 {
                            let skip = arm.others_skip_pair && (s == caster || s == victim);
                            let live = shown[s as usize].2 > 0;
                            let reach = if s < 3 {
                                !arm.others_live_only || live
                            } else {
                                live
                            };
                            if !skip && reach {
                                put(&arm.others_write, &mut want[s as usize]);
                            }
                        }
                    }
                    put(&arm.caster_write, &mut want[caster as usize]);
                    put(&arm.victim_write, &mut want[victim as usize]);
                    if !arm.party_write.is_empty() {
                        for s in 0..3u32 {
                            let (_, _, hp, st) = shown[s as usize];
                            if (!arm.party_live_only || hp > 0)
                                && !(arm.party_skip_stoned && st & 4 != 0)
                                && !(arm.party_skip_victim && s == victim)
                            {
                                put(&arm.party_write, &mut want[s as usize]);
                            }
                        }
                    }
                    for s in 0..8u32 {
                        let a = ACTOR + s * STRIDE;
                        let got = (cpu.mem.rd32(a + 4), cpu.mem.rd8(a + 0x21C));
                        if got != want[s as usize] {
                            failures.push(format!(
                                "{c}: seat {s} tint/flag retail {got:x?} port {:x?}",
                                want[s as usize]
                            ));
                        }
                    }
                    seat_passes += 1;
                }
                // Melt Spray's debuff, on the pass arm 3's countdown runs out
                // (`0x801F82E0`, then `0x801F8314..0x801F8598`), against the
                // kernel the host runs on that pass (`melt_spray_tick`).
                if body.action == 0x6F && phase == t::MELT_SPRAY_DEBUFF_ARM && after != phase {
                    let mut pv = before[1];
                    let mut pctx = crate::cast_view::read_ctx(&mut cpu.mem, CTX);
                    pctx.phase = phase;
                    t::melt_spray_tick(&mut pctx, &mut pv, true);
                    let got = crate::cast_view::read_actor(&mut cpu.mem, va_);
                    let key = |x: &t::CastActorState| {
                        (
                            [x.atk, x.udf, x.ldf, x.spd, x.intel],
                            [x.atk_base, x.udf_base, x.ldf_base, x.spd_base, x.intel_base],
                            x.staged_anim,
                            x.restage,
                        )
                    };
                    if key(&got) != key(&pv) {
                        failures.push(format!(
                            "{c}: Melt Spray retail {:?} port {:?}",
                            key(&got),
                            key(&pv)
                        ));
                    }
                    debuffs += 1;
                }
                if death_game {
                    // The word `0x801FA278`, which `capture_countdown_va`
                    // does not list.
                    let word = cpu.mem.rd32(DEATH_GAME_WORD) as i32;
                    if word != st.countdown.0 {
                        failures.push(format!("{c}: word retail {word} port {}", st.countdown.0));
                        st.countdown.0 = word;
                    }
                    if phase == 0 {
                        let table: Vec<u8> = (0..4u32)
                            .map(|i| cpu.mem.rd32(TABLE + 4 * i) as u8)
                            .collect();
                        if table != st.death_game.table {
                            failures.push(format!(
                                "{c}: deal retail {table:?} port {:?}",
                                st.death_game.table
                            ));
                        }
                    }
                    if !arm.hold && arm.next.is_some_and(|n| n != after) {
                        failures.push(format!(
                            "{c}: retail moved to {after:#x}, port {:?}",
                            arm.next
                        ));
                    }
                    // The picked card's result (`0x801F922C..0x801F9794`)
                    // on the seats it lands on, against the kernel the host
                    // runs (`death_game_outcome`).
                    if let Some(o) = st.death_game.outcome {
                        outcomes.insert(format!("{o:?}"));
                        let [mut pc, mut pv] = before;
                        let max = [ca, va_].map(|a| cpu.mem.rd16(a + 0x14E));
                        t::death_game_outcome(o, &mut pc, &mut pv, max);
                        // The arm's seat writes beside the kernel's: the
                        // caster quieted, the victim shown.
                        for (v, w) in [(&mut pc, &arm.caster_write), (&mut pv, &arm.victim_write)] {
                            if let Some(x) = w.stage {
                                v.staged_anim = x;
                            }
                            if let Some(x) = w.rate {
                                v.anim_rate = x;
                            }
                            if let Some(x) = w.render_flag {
                                v.render_flag = x;
                            }
                            if let Some(x) = w.colour {
                                v.present_04 = x;
                            }
                        }
                        for (who, a, port) in [("caster", ca, pc), ("victim", va_, pv)] {
                            let got = crate::cast_view::read_actor(&mut cpu.mem, a);
                            let key = |x: &t::CastActorState| {
                                (
                                    x.hp,
                                    x.hp_bar_delta,
                                    x.staged_anim,
                                    x.restage,
                                    x.anim_rate,
                                    x.render_flag,
                                    x.present_04,
                                )
                            };
                            if key(&got) != key(&port) {
                                failures.push(format!(
                                    "{c}: {o:?} {who} retail {:?} port {:?}",
                                    key(&got),
                                    key(&port)
                                ));
                            }
                        }
                        results += 1;
                        heal_capped |= o == cam::DeathGameOutcome::BothHeal
                            && before
                                .iter()
                                .zip(max)
                                .any(|(b, m)| u32::from(m - b.hp) < (u32::from(m) + 3) >> 2);
                    }
                }
                // The band's own settle hold on the arm (`t::gated_hit_arm`,
                // `World::run_cast_module_code`), beside the director's.
                let band_settle = t::gated_hit_arm(body.entry, Some(body.va))
                    .is_some_and(|(_, at, _)| at == phase)
                    && !st.victim_settled;
                if victim_settle == Some(phase)
                    && !arm.hold
                    && !band_settle
                    && busy
                    && after == phase
                {
                    failures.push(format!("{c}: port passes the victim settle, retail holds"));
                }
                if body.action == 0x7B
                    && round % 2 == 1
                    && plasma_passes < 4
                    && matches!(phase, 5 | 6 | 0x0D)
                    && !arm.hold
                    && busy
                    && after == phase
                {
                    failures.push(format!("{c}: port passes the clip gate, retail holds"));
                }
                // The band holds a party-row settle of its own
                // (`gated_hit_arm`, a chain arm's `settle`) beside the
                // director's, on the same host flag.
                let band_party_hold =
                    !st.party_settled && band_party_settle_arm(body).is_some_and(|at| at == phase);
                if settle_arm.is_some_and(|(at, ..)| at == phase)
                    && doom_settle > 0
                    && !arm.hold
                    && !band_party_hold
                    && busy
                    && after == phase
                {
                    failures.push(format!("{c}: port passes the party settle, retail holds"));
                }
                if water_hazard && phase == 3 && !arm.hold && after == phase {
                    failures.push(format!("{c}: port passes, retail holds"));
                }
                if arm.hold && after != phase {
                    failures.push(format!("{c}: port holds, retail moved to {after}"));
                }
                if let Some(va) = countdown_va {
                    let word = cpu.mem.rd32(va) as i32 * cam::capture_countdown_units(body.action);
                    if word != st.countdown.0 {
                        failures.push(format!(
                            "{c}: countdown retail {word} port {}",
                            st.countdown.0
                        ));
                        st.countdown.0 = word;
                    }
                }
                if !busy || (body.entry == 960 && after == 0x11) {
                    finished = true;
                    break;
                }
            }
            if !finished {
                failures.push(format!(
                    "{c0}: no finish in {cap} passes at arm {}",
                    cpu.mem.rd8(CTX + 0x279)
                ));
            }
            casts += 1;
        }
        if let Some(r) = shared_rng {
            rng.0 = r;
        }
    }
    eprintln!("{} bodies, {casts} casts, {passes} passes", all.len());
    eprintln!("Death Game outcomes {outcomes:?}");
    assert_eq!(outcomes.len(), 4, "Death Game outcomes {outcomes:?}");
    assert!(debuffs >= 4, "Melt Spray debuffs compared {debuffs}");
    eprintln!("buff passes compared {buffs}, seat-write passes {seat_passes}");
    assert!(seat_passes > 100, "seat-write passes {seat_passes}");
    assert!(steals >= 12, "Terror Scream steals compared {steals}");
    assert!(steal_voices >= 10, "Steal voices compared {steal_voices}");
    assert!(glare_rolls >= 6, "Glare Stone rolls compared {glare_rolls}");
    assert!(kiss_halts >= 6, "Kiss of Death parts compared {kiss_halts}");
    assert!(
        spray_clears >= 3,
        "Melt Spray stage clears compared {spray_clears}"
    );
    eprintln!(
        "band shadow: {shadow_passes} passes compared, {shadow_diverged} casts left retail's phase walk, {} mismatches",
        shadow_failures.len()
    );
    eprintln!(
        "director spawns: {spawns_matched} matched, {} mismatches",
        spawn_failures.len()
    );
    assert!(
        spawns_matched >= 400,
        "director spawns matched {spawns_matched}"
    );
    assert!(
        spawn_failures.is_empty(),
        "{} director spawn mismatches:\n{}",
        spawn_failures.len(),
        spawn_failures[..spawn_failures.len().min(40)].join("\n")
    );
    eprintln!("band shadow: {status_marks} seat passes with a status bit set compared");
    eprintln!(
        "Bloody Horns and Blazing Slash: {horn_hits} hits compared, {horn_clamps} clamped to the victim's HP"
    );
    eprintln!(
        "Doomsday and Chaos Breath: {doom_hits} sweep hits compared, {doom_clamps} clamped, \
         {status_draws} status draws landed, {doom_holds} party-settle holds"
    );
    assert!(
        cursor_floors >= 4,
        "Blazing Slash cursor floors {cursor_floors}"
    );
    assert!(
        plasma_gates >= 12,
        "Plasma Strike clip gates held {plasma_gates}"
    );
    assert!(
        combo_lands >= 1,
        "Plasma Strike combos past the HP {combo_lands}"
    );
    assert!(
        doom_hits > 20 && doom_clamps >= 4 && doom_holds > 20 && status_draws >= 2,
        "sweep hits {doom_hits} clamps {doom_clamps} draws {status_draws} holds {doom_holds}"
    );
    assert!(
        horn_clamps >= 6 && horn_hits > horn_clamps,
        "horn hits {horn_hits} clamps {horn_clamps}"
    );
    assert!(status_marks >= 1000, "status marks compared {status_marks}");
    // Half the rounds run at a step of 2 to 4, whose casts take fewer passes.
    assert!(
        shadow_passes >= 110_000,
        "band shadow passes {shadow_passes}"
    );
    eprintln!("Dark Typhoon swirl: {typhoon_poses} party seat poses compared");
    eprintln!("Chaos Breath shove: {shove_poses} party seat pairs compared");
    assert!(
        shove_poses >= 100,
        "Chaos Breath shove pairs compared: {shove_poses}"
    );
    eprintln!("Water Column: {venom_marks} passes with the victim's Venom bit compared");
    assert!(
        venom_marks > 0,
        "no Water Column round landed its Venom draw"
    );
    assert!(
        typhoon_poses >= 100,
        "Dark Typhoon swirl seat poses compared: {typhoon_poses}"
    );
    assert!(
        shadow_failures.is_empty(),
        "{} band seat mismatches:\n{}",
        shadow_failures.len(),
        shadow_failures[..shadow_failures.len().min(40)].join("\n")
    );
    assert!(buffs >= 12, "buff passes compared {buffs}");
    eprintln!(
        "Thunder Storm: {storm_seats} swept seats compared, {storm_stoned} Stoned, {storm_clamps} clamped"
    );
    assert!(
        storm_seats >= 12 && storm_stoned >= 2 && storm_clamps >= 2 && storm_levels >= 20,
        "{storm_seats} {storm_stoned} {storm_clamps}"
    );
    assert!(
        results >= 4 && heal_capped,
        "Death Game results compared {results}, a heal capped by the missing HP {heal_capped}"
    );
    assert!(
        victim_settle_holds > 10,
        "Kiss of Death's and Melt Spray's victim settles held {victim_settle_holds} passes"
    );
    assert!(
        unsettled_holds > 10,
        "Water Hazard's settle held {unsettled_holds} passes"
    );
    let n = failures.len();
    failures.truncate(4000);
    assert!(n == 0, "{n} mismatches:\n{}", failures.join("\n"));
}

/// The countdown words of the directed bodies [`cam::capture_countdown_va`]
/// leaves out (it names only the words a save state's seed reads), off each
/// body's director table in `cast-module.md`: compared here at every step,
/// so a pass that hands over on its opening tick and drops the rest of its
/// drain shows.
fn sweep_countdown_va(action: u8) -> Option<u32> {
    match action {
        0x4B => Some(0x801F_8174),
        0x4C => Some(0x801F_8090),
        0xAA => Some(0x801F_88B4),
        0x75 => Some(0x801F_86A0),
        0x51 => Some(0x801F_83EC),
        0x53 => Some(0x801F_8360),
        0x7B => Some(0x801F_8E50),
        0x79 => Some(0x801F_93B8),
        0x7A => Some(0x801F_9624),
        0x4E => Some(0x801F_8040),
        _ => None,
    }
}

/// Shots the port is known to frame differently, as `(entry, body, arm,
/// why)`; the rest of each arm is still compared.
const KNOWN_GAPS: [(u32, u32, u8, &str); 1] = [(
    961,
    DEAD_END_CRISIS,
    5,
    "the no-one-standing branch frames nothing; the director cuts on it (documented \
     on dead_end_crisis_camera)",
)];

const MYSTIC_CIRCLE: u32 = 0x801F_69EC;
const CHAOS_BREATH: u32 = 0x801F_726C;
const MYSTIC_SHIELD: u32 = 0x801F_7240;
const GLARE: u32 = 0x801F_69F8;
const STONE_CIRCLE: u32 = 0x801F_6A04;
const STEAL: u32 = 0x801F_730C;
const POWER_UP: u32 = 0x801F_7D34;
const DARK_TYPHOON: u32 = 0x801F_69F4;
const LAPIS_WAVE: u32 = 0x801F_6A04;
const GUILTY_CROSS: u32 = 0x801F_6A04;
const CURSE_ALL: u32 = 0x801F_7470;
const WATER_COLUMN: u32 = 0x801F_6EDC;
const JUGGER_POWER: u32 = 0x801F_69F8;
const SHADOW_BREAK: u32 = 0x801F_6A24;
const ROLLING_FLARE: u32 = 0x801F_79F8;
const SCYTHE_WIND: u32 = 0x801F_77E8;
const CHAOS_FLARE: u32 = 0x801F_6A20;
const BLOODY_HORNS: u32 = 0x801F_7118;
const ASTRAL_SLASH: u32 = 0x801F_6A0C;
const KISS_OF_DEATH: u32 = 0x801F_86A4;
const TERROR_SCREAM: u32 = 0x801F_767C;
const MELT_SPRAY: u32 = 0x801F_7FA4;
const VOID_ACCESSORIES: u32 = 0x801F_6A28;
const WHITE_SHIELD: u32 = 0x801F_8F0C;
const POWER_CHARGE: u32 = 0x801F_7158;
const PARALYZING_WAVE: u32 = 0x801F_69D8;
const WATER_HAZARD: u32 = 0x801F_7298;
const DEATH_GAME: u32 = 0x801F_798C;
const THUNDER_STORM: u32 = 0x801F_6A14;
/// The part pool, and the part the walk stands in it for Thunder Storm.
const PART_POOL: u32 = 0x801C_90F0;
const STORM_PART: u32 = 0x8017_9000;
const BLAZING_SLASH: u32 = 0x801F_6DD8;
const MEGATON_PRESS: u32 = 0x801F_69F0;
const PLASMA_STRIKE: u32 = 0x801F_74E4;
const NEO_STAR_SLASH: u32 = 0x801F_69D8;
const DEAD_END_CRISIS: u32 = 0x801F_69D8;
const ULTRA_CHARGE: u32 = 0x801F_69D8;
const BLADE_BREATH: u32 = 0x801F_7AE4;
const THUNDER_NEEDLE: u32 = 0x801F_74A0;
const GIGATON_PRESS: u32 = 0x801F_6D54;
const ELEMENT_CHANGE: u32 = 0x801F_88EC;
const ELEMENT_STRIKE: u32 = 0x801F_69D8;
const DOOMSDAY: u32 = 0x801F_69D8;
/// The spell records (`0x800754C8`, `0xC` bytes each), whose `+2` is the
/// target shape the monster picker reads (`0x801EA59C`).
const SPELL_STATS: u32 = 0x8007_54C8;
/// Doomsday's countdown word, in vsyncs (`0x801F8370`).
const DOOMSDAY_WORD: u32 = 0x801F_8370;

/// The bodies whose last arm waits on every party seat once its word is
/// spent, as `(arm, word, drain)`: Doomsday's arm 0x0C drains the word by
/// the frame step and walks on that pass (`0x801F78E0..0x801F79F4`), Chaos
/// Breath's arm 3 walks on the pass after its gauge drain left the word `0`
/// (`0x801F7910..0x801F79F4`, `drain` `0`).
fn party_settle_arm(body: &Body) -> Option<(u8, u32, i32)> {
    match (body.entry, body.va) {
        (965, DOOMSDAY) => Some((0x0C, DOOMSDAY_WORD, 1)),
        (938, CHAOS_BREATH) => Some((3, 0x801F_8040, 0)),
        // Neo Star Slash's arm 6 drains `scalar * step` and walks on one pass
        // (`0x801F73F8..0x801F7484`).
        (960, NEO_STAR_SLASH) => Some((6, 0x801F_8E50, SPEED_SCALAR)),
        _ => None,
    }
}

/// The arm whose party-row settle the band holds itself, off the engine's
/// own tables: `gated_hit_arm`'s terminal wait (Chaos Breath's arm 3) or a
/// chain descriptor arm's `settle` (Neo Star Slash's arm 6).
fn band_party_settle_arm(body: &Body) -> Option<u8> {
    use t::ChainSettle::PartyRow;
    if let Some((_, at, PartyRow)) = t::gated_hit_arm(body.entry, Some(body.va)) {
        return Some(at);
    }
    t::chain_body_for(body.entry, body.va)?
        .arms
        .iter()
        .find(|a| a.settle == Some(PartyRow))
        .map(|a| a.phase)
}

/// The bodies whose sweep arm rolls each hittable party seat, by arm.
fn party_sweep_arm(body: &Body) -> Option<u8> {
    match (body.entry, body.va) {
        (965, DOOMSDAY) => Some(t::DOOMSDAY_SWEEP_ARM),
        (938, CHAOS_BREATH) => Some(t::CHAOS_BREATH_SWEEP_ARM),
        // Stone Circle's arm 2 (`0x801F6F68..0x801F7064`).
        (941, STONE_CIRCLE) => Some(2),
        _ => None,
    }
}

/// The roll Doomsday's `FUN_801DD4B0` returns for party seat `seat` on HP
/// `hp` in round `round`: none, short of the HP, the HP, past it, far past.
fn doomsday_roll(round: usize, seat: u32, hp: u32) -> u32 {
    [0, hp.saturating_sub(1), hp, hp + 1, hp * 3][(round + seat as usize) % 5]
}
const EVIL_SERU_MAGIC: u32 = 0x801F_6A74;
/// The part spawn `FUN_80021B04` and the pool the walk hands parts from.
const SPAWN_PART: u32 = 0x8002_1B04;
/// The frame's primitive cursor and the buffer it is reset to each pass.
const PRIM_CURSOR: u32 = 0x1F80_03A0;
const PRIMS: u32 = 0x8018_0000;
const PARTS: u32 = 0x8016_0000;
/// The bag's find-or-insert `FUN_800421D4`, which a turn steal refunds through.
const BAG_INSERT: u32 = 0x8004_21D4;
/// The battle sound funnel `FUN_8004FE5C(id, category)`.
const SOUND_FUNNEL: u32 = 0x8004_FE5C;
/// Zora, the monster Glare's body forks on (`0x8007BD0C[seat - 3] == 0xA9`).
const GLARE_ZORA: u8 = 0xA9;
/// The character records (`0x80084140 + (id - 1) * 0x414`).
const CHAR_RECORDS: u32 = 0x8008_4140;
const CHAR_STRIDE: u32 = 0x414;
/// The monster-record pointer table and the records the buff bodies read.
const RECORDS: u32 = 0x801C_9348;
const MONSTER_RECORDS: u32 = 0x8017_0000;
/// The seated part spawn `FUN_80050ED4` Death Game's faces come from
/// (`0x801F7F08`).
const SPAWN_SEATED: u32 = 0x8005_0ED4;
/// Death Game's card handles and dealt table (`0x801FA2B8`, `0x801FA288`),
/// the pad's press word and the confirm button's mask.
const CARDS: u32 = 0x801F_A2B8;
const DEATH_GAME_WORD: u32 = 0x801F_A278;
const TABLE: u32 = 0x801F_A288;
const PRESSED: u32 = 0x8007_B874;
const CONFIRM_MASK: u32 = 0x8008_46D0;

/// Water Hazard's arm 3 (`0x801F7B90`): once its countdown runs out it walks
/// the seats the caster's target code names and passes (phase `0xFF`) only
/// when every one has settled (`0x801F7C2C..0x801F7DA4`): a code below `8`
/// one seat, `8` the `ctx[+0]` party seats, any other the `ctx[+1]` monster
/// seats from seat `3`; a live seat idle (`+0x1D9 == 0`), a downed party seat
/// on clip `8`, a dead monster faded (`+0x04 == 0`). Against
/// `World::capture_target_settled` through `BattleArmDrivers`, on a world
/// seated from the same table: small parties (the engine's compacted row),
/// short rows, and now and then an active actor past the seated row that the
/// walk must not reach.
#[test]
fn water_hazard_waits_for_the_seats_it_names_as_retail_does() {
    use legaia_engine_core::world::World;
    use legaia_engine_core::world::arm_drivers::BattleArmDrivers;
    use legaia_save::Party;
    differential!(prot(956, WATER_HAZARD));
    let (Some(_), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // Arm 3's countdown word (`lw v1,-0x7960(a0)` at `0x801F7C10`).
    const COUNTDOWN: u32 = 0x801F_86A0;
    let body = bodies()
        .into_iter()
        .find(|b| b.action == 0x71)
        .expect("Water Hazard");
    let module = disc.read_entry(956).expect("PROT 0956");
    let mut cpu = scus_cpu().expect("SCUS");
    cpu.mem
        .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
    cpu.mem.wr32(CTX_PTR, CTX);
    for target in jal_targets(&module) {
        cpu.stub(target, 0);
    }
    let mut rng = Rng(0x5741_5452);
    let (mut passed, mut held, mut extras) = (0u32, 0u32, 0u32);
    let mut codes = std::collections::BTreeSet::new();
    for round in 0..20_000u32 {
        let (_, _) = seed(&mut cpu, &mut rng, &module, &body, 3);
        let pc = 1 + rng.below(3) as u8;
        let m = 1 + rng.below(5) as u8;
        let caster = 3 + rng.below(u32::from(m)) as u8;
        let code = match rng.below(4) {
            0 => rng.below(u32::from(pc)) as u8,
            1 => 3 + rng.below(u32::from(m)) as u8,
            2 => 8,
            _ => 9,
        };
        cpu.mem.wr8(CTX, pc);
        cpu.mem.wr8(CTX + 1, m);
        cpu.mem.wr8(CTX + 0x13, caster);
        cpu.mem.wr32(COUNTDOWN, 1);
        // (hp, clip, colour word) per retail seat; the empty party seats
        // are the zeroed structs the loader leaves.
        let mut seats = [(0u16, 0u8, 0u32); 8];
        for (s, seat) in seats.iter_mut().enumerate() {
            let used = s < usize::from(pc) || (3..3 + usize::from(m)).contains(&s);
            if !used {
                continue;
            }
            let alive = rng.below(3) != 0;
            *seat = (
                if alive { 1 + rng.below(300) as u16 } else { 0 },
                [0u8, 0, 5, 8][rng.below(4) as usize],
                if rng.below(2) == 0 { 0 } else { 0x1004_0100 },
            );
        }
        for (s, &(hp, clip, colour)) in seats.iter().enumerate() {
            let a = ACTOR + s as u32 * STRIDE;
            cpu.mem.wr16(a + 0x14C, hp);
            cpu.mem.wr8(a + 0x1D9, clip);
            cpu.mem.wr32(a + 4, colour);
        }
        cpu.mem
            .wr8(ACTOR + u32::from(caster) * STRIDE + 0x1DD, code);
        assert_eq!(cpu.call(WATER_HAZARD, &[]), Stop::Returned, "{round}");
        let retail = cpu.mem.rd8(CTX + 0x279) == 0xFF;

        let mut w = World::new();
        w.load_party(Party::zeroed(pc as usize));
        w.enter_battle(pc, m);
        let engine = |r: usize| if r < 3 { r } else { r - 3 + usize::from(pc) };
        for (r, &(hp, clip, colour)) in seats.iter().enumerate() {
            if !(r < usize::from(pc) || (3..3 + usize::from(m)).contains(&r)) {
                continue;
            }
            let a = &mut w.actors[engine(r)];
            a.active = true;
            a.battle.hp = hp;
            a.battle.current_anim = clip;
            a.battle.render_color = colour;
            a.battle_action_clips = Some(std::sync::Arc::new(Vec::new()));
            if r >= 3 {
                a.battle_monster_id = Some(0x20 + r as u16);
            }
        }
        // An actor past the seated row: no seat the walk counts.
        let past = usize::from(pc + m);
        let extra = rng.below(4) == 0 && past < w.actors.len();
        if extra {
            let a = &mut w.actors[past];
            a.active = true;
            a.battle_monster_id = None;
            a.battle.hp = 0;
            a.battle.render_color = 0x1004_0100;
            extras += 1;
        }
        let ce = engine(usize::from(caster));
        w.actors[ce].battle.active_target = match code {
            3..=7 => engine(usize::from(code)) as u8,
            c => c,
        };
        let port = w.capture_target_settled(ce as u8);
        assert_eq!(
            port, retail,
            "round {round}: party {pc} row {m} code {code} seats {seats:x?} extra {extra}"
        );
        passed += u32::from(retail);
        held += u32::from(!retail);
        codes.insert(code.min(8 + u8::from(code > 8)));
    }
    assert!(
        passed > 3000 && held > 3000 && extras > 2000 && codes.len() >= 4,
        "{passed} {held} {extras} {codes:?}"
    );
}

/// Every formation id each battle MAN on the disc can seat: the bundle MAN
/// and every type-3 streaming chunk, each formation's `count` ids.
fn formation_ids_disc_wide(disc: &legaia_disc_patch::disc::DiscPatcher) -> Vec<u8> {
    use legaia_scene_formats::{man_section, scene_asset_table};
    let ids_of = |man: &[u8], out: &mut Vec<u8>| {
        let Ok(file) = man_section::parse(man) else {
            return;
        };
        let Some(body) = file.encounter_section_body(man) else {
            return;
        };
        let Ok(sec) = man_section::parse_encounter_section(body) else {
            return;
        };
        let base = file.encounter_section().body_offset() + sec.formation_range.0;
        for f in 0..usize::from(sec.formation_count) {
            let rec = base + f * usize::from(sec.formation_stride);
            let Some(&count) = man.get(rec + 3) else {
                return;
            };
            let n = usize::from(count.min(4));
            out.extend(man.get(rec + 4..rec + 4 + n).unwrap_or(&[]));
        }
    };
    let mut out = Vec::new();
    for idx in 0..disc.entry_count() {
        let Ok(entry) = disc.read_entry(idx) else {
            continue;
        };
        if let Some(table) = scene_asset_table::detect(&entry)
            && let Some(man) = table.used().iter().find(|d| d.type_byte == 3)
            && man.size != 0
            && man.data_offset != 0
            && let Some(body) = entry.get(man.data_offset as usize..)
            && let Ok(decoded) = psx_lzs::decompress(body, man.size as usize)
        {
            ids_of(&decoded, &mut out);
        }
        if let Ok(report) = legaia_container_formats::parse_streaming(&entry, 4096) {
            for chunk in report.chunks.iter().filter(|c| c.type_byte == 3) {
                let start = chunk.header_offset + 4;
                if let Some(man) = entry.get(start..start + chunk.size as usize) {
                    ids_of(man, &mut out);
                }
            }
        }
    }
    out
}

/// Terror Scream's arm 0 (`0x801F7758..0x801F78F0`) and arm 1 fork on the
/// caster's monster id: `0x7D..=0x7F` play cue `0x190`, `0x83..=0x85` `0x191`,
/// `0x28..=0x2A` `0x197`, and only `0x7F`, `0x28..=0x2A` and `0x83..=0x85`
/// frame a shot; `0x7D` and `0x7E` play the cue and leave the camera where it
/// is, and any other id does neither. The camera director frames the near
/// shot for every id but `0x7F`, which is retail's only while the caster is
/// in a framed band. This holds that it always is.
///
/// Action `0x70` reaches a monster two ways, both in the AI picker
/// `FUN_801E9FD4` (PROT 0898): the record's magic window `+0x20..+0x27`
/// (`lbu v0,0x21(v0)` at `0x801EA538`, stored at `0x801EA544`), and the
/// monster-id switch, whose only `0x70` (`li v0,0x70` at `0x801EB3FC`, the one
/// immediate `0x70` stored to `+0x1DF` on the disc) closes the arm
/// `0x801EB2EC..0x801EB408` that the jump table `0x801CF1CC` (index id - 4,
/// bound `0xB3` at `0x801EA9D8`) enters for `0x28`, `0x29` (two `addiu s2`
/// later), `0x2A`, `0x7F` and `0x85`. Gold Face `0x7D` and Lava Face `0x7E`
/// sit in formations (Lava Face in random ones) but take the switch's common
/// tail, and no record's window holds `0x70`; Medusa `0x83` and Nurga `0x84`
/// do not cast it either.
#[test]
fn terror_scream_casters_all_sit_in_a_framed_band() {
    not_a_differential!(
        "censuses PROT 0898's AI jump table, the PROT 0867 records' magic window and every \
         formation on the disc against Terror Scream's framed caster bands; runs no routine"
    );
    let Some(disc) = disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    const AI_TABLE: u32 = 0x801C_F1CC;
    const TERROR_ARM: std::ops::RangeInclusive<u32> = 0x801E_B2EC..=0x801E_B2F4;
    let overlay = disc.read_entry(898).expect("PROT 0898");
    let word = |va: u32| {
        let o = (va - OVERLAY_BASE) as usize;
        u32::from_le_bytes(overlay[o..o + 4].try_into().unwrap())
    };
    // The one immediate `0x70` the picker stores to `+0x1DF`.
    assert_eq!(word(0x801E_B3FC), 0x2402_0070, "li v0,0x70");
    assert_eq!(word(0x801E_B400), 0xA282_01DF, "sb v0,0x1df(s4)");
    let switch_casters: Vec<u8> = (0..0xB3u32)
        .filter(|i| TERROR_ARM.contains(&word(AI_TABLE + 4 * i)))
        .map(|i| (i + 4) as u8)
        .collect();
    assert_eq!(switch_casters, [0x28, 0x29, 0x2A, 0x7F, 0x85]);
    let archive = disc.read_entry(867).expect("PROT 0867");
    let records = legaia_actor_formats::monster_archive::records(&archive).expect("records");
    let window_casters: Vec<u16> = records
        .iter()
        .filter(|r| r.ai_bytes.contains(&0x70))
        .map(|r| r.id)
        .collect();
    assert!(window_casters.is_empty(), "{window_casters:#x?}");
    let framed = |id: u8| matches!(id, 0x28..=0x2A | 0x7F | 0x83..=0x85);
    assert!(switch_casters.iter().all(|&id| framed(id)));
    // The two cue-only ids are seated, so it is the AI, not the formations,
    // that keeps them off the cast.
    let seated = formation_ids_disc_wide(disc);
    assert!(seated.len() > 500, "{} seated ids", seated.len());
    for id in [0x7Du8, 0x7E, 0x28, 0x7F, 0x85] {
        assert!(seated.contains(&id), "{id:#x} in no formation");
    }
}

/// `FUN_80050BB8`, the pairwise separation the walk-in's arrival runs
/// thirty-two times.
const SEPARATION: u32 = 0x8005_0BB8;

/// The walk-in arms of every body whose walk the host's approach leg stands
/// in for (`cam::capture_approach_follows`), against that leg: while the range
/// poll `FUN_8004E2F0` answers "out of reach" the arm holds with the caster on
/// the walk clip (`+0x1DA = 1`, e.g. `0x801F72C8` in PROT 0952) as the leg
/// holds the module with the caster on `CAPTURE_WALK_ENTRY`; on the pass it
/// answers "in reach" the arm runs `FUN_80050BB8(caster, victim)` thirty-two
/// times, live here, and leaves the arm, and the caster's live pair stands
/// where the leg's arrival pushes put it (`World::settle_capture_approach`).
#[test]
fn the_walk_in_arrives_and_stands_off_as_the_hosts_leg_does() {
    differential!(
        prot(950, ROLLING_FLARE),
        prot(951, CHAOS_FLARE),
        prot(951, SCYTHE_WIND),
        prot(952, ASTRAL_SLASH),
        prot(952, BLOODY_HORNS),
        prot(955, MELT_SPRAY),
        prot(962, BLADE_BREATH),
        prot(962, THUNDER_NEEDLE),
        prot(962, GIGATON_PRESS)
    );
    let (Some(_), Some(disc)) = (scus_cpu(), disc()) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x5741_4C4B);
    let mut failures = Vec::new();
    let (mut holds, mut arrivals, mut pushed) = (0usize, 0usize, 0usize);
    let mut walked = std::collections::BTreeSet::new();
    for body in bodies() {
        if !cam::capture_approach_follows(body.entry, Some(body.va)) {
            continue;
        }
        walked.insert((body.entry, body.va));
        let module = disc.read_entry(body.entry as usize).expect("module");
        for round in 0..24usize {
            let mut cpu = scus_cpu().expect("SCUS");
            cpu.mem
                .load(OVERLAY_BASE, &disc.read_entry(898).expect("PROT 0898"));
            cpu.mem.wr32(CTX_PTR, CTX);
            cpu.mem.wr32(0x8007_B81C, 0x8007_0A2C);
            cpu.mem.wr32(0x8007_B7F8, 0x8007_0A2C + 0x800);
            cpu.hook(BEARING, |r, _| {
                let a = |i: usize| r.r[A0 + i] as i16;
                r.r[V0] = u32::from(bearing_12bit_approx(a(0), a(1), a(2), a(3)));
            });
            for target in jal_targets(&module) {
                if ![AIM, RANGE, BUILDER, BEARING, SEPARATION].contains(&target) {
                    cpu.stub(target, 0);
                }
            }
            // The polls left to answer "out of reach", and what each pass's
            // polls answered.
            let out = Rc::new(std::cell::Cell::new((round % 4) as u32));
            let answered = Rc::new(RefCell::new(Vec::<bool>::new()));
            {
                let (out, answered) = (out.clone(), answered.clone());
                cpu.hook(RANGE, move |r, _| {
                    let far = out.get() > 0;
                    out.set(out.get().saturating_sub(1));
                    answered.borrow_mut().push(far);
                    r.r[V0] = u32::from(far);
                });
            }
            let (caster, victim) = seed(&mut cpu, &mut rng, &module, &body, 0);
            let (ca, va_) = (ACTOR + caster * STRIDE, ACTOR + victim * STRIDE);
            // Most rounds stand the caster's body pair on the victim's, inside
            // the two radii's sixth, so the pushes run.
            if round % 3 != 2 {
                for o in [0x3Cu32, 0x40] {
                    let near = cpu.mem.rd16(va_ + o).wrapping_add(rng.below(0x30) as u16);
                    cpu.mem.wr16(ca + o, near.wrapping_sub(0x18));
                }
            }
            let mut held_at = None;
            for pass in 0..12 {
                let phase = cpu.mem.rd8(CTX + 0x279);
                let c = format!(
                    "PROT {:04} body {:#x} round {round} pass {pass} arm {phase}",
                    body.entry, body.va
                );
                // The leg's arrival, off the seats as this pass opens on them.
                let port = shadow_world(&mut cpu, &body, caster, victim, 1).map(|mut w| {
                    for s in 0..8u32 {
                        let a = ACTOR + s * STRIDE;
                        let node = cpu.mem.rd32(a + 0x22C);
                        let act = &mut w.actors[s as usize];
                        act.battle.seat =
                            Some((cpu.mem.rd16(a + 0x3C) as i16, cpu.mem.rd16(a + 0x40) as i16));
                        act.battle.render_radius = Some(cpu.mem.rd16(node + 0x58) as i16);
                    }
                    let settled =
                        w.drive_settle_capture_approach(body.action, caster as u8, victim as u8);
                    let m = &w.actors[caster as usize].move_state;
                    (settled, m.world_x, m.world_z)
                });
                let pair = (cpu.mem.rd16(ca + 0x34), cpu.mem.rd16(ca + 0x38));
                answered.borrow_mut().clear();
                // The walk clip the first out-of-reach pass stages stays on
                // the caster through the arm's holds.
                if pass == 0 {
                    cpu.mem.wr8(ca + 0x1DA, 0);
                }
                let stop = cpu.call(body.va, &[]);
                if !matches!(stop, Stop::Returned) {
                    failures.push(format!("{c}: {stop:?}"));
                    break;
                }
                let after = cpu.mem.rd8(CTX + 0x279);
                let polls = answered.borrow().clone();
                let Some(&far) = polls.last() else {
                    // An arm ahead of the walk, or one past it.
                    if held_at.is_some() || arrivals > 0 && after > 1 {
                        break;
                    }
                    continue;
                };
                if far {
                    holds += 1;
                    if cpu.mem.rd8(ca + 0x1DA) != 1 {
                        failures.push(format!(
                            "{c}: out of reach, caster stage {:#x}, the leg's walk clip",
                            cpu.mem.rd8(ca + 0x1DA)
                        ));
                    }
                    if held_at.is_some_and(|h| h != after) {
                        failures.push(format!("{c}: out of reach, retail moved to {after}"));
                    }
                    held_at = Some(after);
                    continue;
                }
                arrivals += 1;
                if held_at.is_some_and(|h| after <= h) {
                    failures.push(format!("{c}: in reach, retail held on {after}"));
                }
                let got = (
                    cpu.mem.rd16(ca + 0x34) as i16,
                    cpu.mem.rd16(ca + 0x38) as i16,
                );
                pushed += usize::from(got != (pair.0 as i16, pair.1 as i16));
                match port {
                    Some((true, x, z)) if (x, z) == got => {}
                    other => failures.push(format!(
                        "{c}: arrival pair retail {got:?} from {:?} port {other:?}",
                        (pair.0 as i16, pair.1 as i16)
                    )),
                }
                break;
            }
        }
    }
    eprintln!(
        "walk-in: {} bodies, {holds} out-of-reach holds, {arrivals} arrivals ({pushed} pushed)",
        walked.len()
    );
    assert!(
        failures.is_empty(),
        "{} mismatches:\n{}",
        failures.len(),
        failures[..failures.len().min(40)].join("\n")
    );
    assert!(walked.len() >= 9, "approach bodies walked {}", walked.len());
    assert!(holds >= 100 && arrivals >= 150 && pushed >= 60);
}
