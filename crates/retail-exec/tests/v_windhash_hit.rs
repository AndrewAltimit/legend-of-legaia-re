//! PROT 0947's (V-Windhash `0x57`, Neo Windhash `0xA7`) hit pass off the disc,
//! arm 3 (`0x801F73A0..0x801F74E8`), against the band's pass and its fold
//! (`World::drive_capture_module_pass`), with the per-move wrapper
//! `FUN_801DD4B0` live beneath both sides: the hit (power `0x7B` or `0xEA`),
//! Neo Windhash's 1-in-4 Curse drawn ahead of it, the HP and the bar delta
//! `+0x10`, the wrapper finisher's spirit fill, and the reaction the arm
//! stages on its victim: the knockdown `+0x1F1` when the victim's gate byte
//! `+0x1F2` is set or the hit downs it, else the flinch `+0x1EF` (or `+0x1F0`
//! when that is empty), and the victim turned to face away from the caster
//! (`FUN_80019B28` plus a half turn). The arm's bump `+0x1DC = 1` is the
//! engine's reaction channel, which commits the staged entry itself.

use crate::cast_damage::{Cast, actor, gen_cast, rig, seed_port, seed_retail};
use crate::common::disc;
use legaia_engine_core::world::World;
use psx_retail_exec::Stop;
use std::cell::Cell;
use std::rc::Rc;

const BODY: u32 = 0x801F_69F0;
const MODULE_BASE: u32 = 0x801F_69D8;
const CTX: u32 = 0x8012_0000;
/// Arm 3's countdown word, drained by `scalar * step` a pass.
const COUNTDOWN: u32 = 0x801F_7B40;
const STEP: u32 = 0x1F80_0393;
const SCALAR: u32 = 0x1F80_037D;
const HIT_ARM: u8 = 3;
/// The routines the arm reaches that the comparison leaves out: the shot
/// builder, the part spawns, the popup push and the fade.
const STUBBED: [u32; 5] = [
    0x801D_829C,
    0x8002_1B04,
    0x8005_0ED4,
    0x801F_44A0,
    0x8002_4E80,
];

#[derive(Debug, PartialEq, Eq)]
struct Hit {
    hp: u16,
    bar: i32,
    spirit: u16,
    status: u16,
    staged: u8,
    facing: u16,
    rng: u32,
}

/// A port world seated as [`seed_port`] seats `c`, with the capture cast armed
/// on the band and its fold owed on `victim`.
fn port_world(c: &Cast, action: u8, victim: u8) -> Option<World> {
    use legaia_game_tables::spell_names::{CAPTURE_CLASS, SpellEntry, SpellNameTable};
    let disc = disc()?;
    let overlay = disc.read_entry(898).ok()?;
    let mut w = World::new();
    w.tables.element_affinity = legaia_game_tables::element_affinity::parse(&overlay);
    w.tables.move_power =
        legaia_engine_battle::move_power::MovePowerCatalog::from_overlay_0898(&overlay);
    seed_port(&mut w, c);
    let mut entries = vec![SpellEntry::default(); 0x100];
    entries[usize::from(action)].class = CAPTURE_CLASS;
    entries[usize::from(action)].sub_class = (947 - 935) as u8;
    w.menu.text = Some(legaia_engine_menus::pause_screens::MenuTextTables {
        spell_names: Some(SpellNameTable::from_entries(entries)),
        ..Default::default()
    });
    if w.cast_module_for(action) != Some(947) {
        return None;
    }
    w.battle_ctx.active_actor = c.attacker;
    w.actors[usize::from(c.attacker)].battle.active_target = victim;
    w.drive_arm_capture_cast_module(action);
    w.casting.pending_cast = Some(legaia_engine_battle_loop::cast_band::PendingCast {
        caster: c.attacker,
        spell_id: action,
        targets: vec![victim],
        on_flight: false,
    });
    Some(w)
}

#[test]
fn the_windhash_hit_stages_what_retail_stages() {
    differential!(prot(947, BODY));
    let state = Rc::new(Cell::new(0));
    let Some((mut cpu, _)) = rig(&state) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let module = disc().unwrap().read_entry(947).expect("PROT 0947");
    for va in STUBBED {
        cpu.stub(va, 0);
    }
    cpu.mem.wr32(0x8007_B81C, 0x8007_0A2C);
    cpu.mem.wr32(0x8007_B7F8, 0x8007_0A2C + 0x800);
    let mut rng = crate::common::Rng(0x5657_4E44);
    let mut failures = Vec::new();
    let (mut rounds, mut knockdowns, mut gated, mut flinches, mut cursed) = (0, 0, 0, 0, 0);
    for round in 0..3000 {
        let step = 1 + (round % 3) as u8;
        let action = if rng.below(2) == 0 { 0x57u8 } else { 0xA7 };
        let mut c: Cast = gen_cast(&mut rng);
        // A monster's cast on a party seat.
        c.target = rng.below(3) as u8;
        c.special = false;
        for s in &mut c.seats {
            s.guarding = false;
        }
        let (caster, victim) = (c.attacker, c.target);
        // Low HP now and then, so the hit downs the victim.
        if rng.below(3) == 0 {
            c.seats[usize::from(victim)].hp = 1 + rng.below(40) as u16;
        }
        // The victim's reaction run: the flinch (sometimes empty), its
        // fallback, the knockdown and the knockdown gate.
        let run: [u8; 4] = [
            [0, 0x0B][rng.below(2) as usize],
            0x0C,
            0x0D,
            u8::from(rng.below(4) == 0),
        ];
        let ground: Vec<(i16, i16)> = (0..8)
            .map(|_| {
                (
                    -0x400 + rng.below(0x800) as i16,
                    -0x400 + rng.below(0x800) as i16,
                )
            })
            .collect();
        let passes = 1 + rng.below(3);
        let scalar = legaia_engine_cast::cast_module_camera::SPEED_SCALAR as u32;
        let word = scalar * u32::from(step) * passes - rng.below(scalar);

        // Retail side.
        cpu.mem.load(MODULE_BASE, &module);
        seed_retail(&mut cpu, &c);
        cpu.mem.wr8(CTX + 0x13, caster);
        cpu.mem.wr8(actor(caster) + 0x1DD, victim);
        cpu.mem.wr8(actor(caster) + 0x1DF, action);
        for (s, &(x, z)) in ground.iter().enumerate() {
            cpu.mem.wr16(actor(s as u8) + 0x34, x as u16);
            cpu.mem.wr16(actor(s as u8) + 0x38, z as u16);
        }
        let v = actor(victim);
        for (i, b) in run.iter().enumerate() {
            cpu.mem.wr8(v + 0x1EF + i as u32, *b);
        }
        cpu.mem.wr8(CTX + 0x279, HIT_ARM);
        cpu.mem.wr32(COUNTDOWN, word);
        cpu.mem.wr8(STEP, step);
        cpu.mem.wr8(SCALAR, scalar as u8);
        state.set(c.seed);
        let mut left = false;
        for pass in 0..8 {
            let stop = cpu.call(BODY, &[]);
            assert_eq!(stop, Stop::Returned, "round {round} pass {pass}");
            if cpu.mem.rd8(CTX + 0x279) != HIT_ARM {
                left = true;
                break;
            }
        }
        assert!(left, "round {round}: retail's arm 3 held");
        let retail = Hit {
            hp: cpu.mem.rd16(v + 0x14C),
            bar: cpu.mem.rd32(v + 0x10) as i32,
            spirit: cpu.mem.rd16(v + 0x170),
            status: cpu.mem.rd16(v + 0x16E),
            staged: cpu.mem.rd8(v + 0x1DA),
            facing: cpu.mem.rd16(v + 0x46) & 0xFFF,
            rng: state.get(),
        };

        // Port side: the band's passes on the same arm.
        let Some(mut w) = port_world(&c, action, victim) else {
            panic!("round {round}: no PROT 0947 band for {action:#x}");
        };
        for (s, &(x, z)) in ground.iter().enumerate() {
            w.actors[s].move_state.world_x = x;
            w.actors[s].move_state.world_z = z;
        }
        let p = &mut w.actors[usize::from(victim)].battle.params;
        for (i, b) in run.iter().enumerate() {
            p[0x10 + i] = *b;
        }
        w.casting.module_phase = HIT_ARM;
        w.casting.module_cam.countdown.0 = word as i32;
        w.rng_state = c.seed;
        w.clock.display_frames = 0;
        w.battle.frame_clock.span_extra = 0;
        for _ in 0..8 {
            w.clock.display_frames += u64::from(step);
            w.drive_capture_module_pass(action, step);
            if w.casting.module_phase != HIT_ARM {
                break;
            }
        }
        let pv = w.drive_cast_view(victim);
        let port = Hit {
            hp: w.actors[usize::from(victim)].battle.hp,
            bar: pv.hp_bar_delta,
            spirit: w.actors[usize::from(victim)].battle.spirit_gauge,
            status: w.raw_status_word(victim),
            staged: pv.staged_anim,
            facing: w.actors[usize::from(victim)].battle.facing_angle & 0xFFF,
            rng: w.rng_state,
        };
        rounds += 1;
        knockdowns += u32::from(retail.staged == run[2]);
        gated += u32::from(run[3] != 0 && retail.hp != 0);
        flinches += u32::from(retail.staged == run[0] || retail.staged == run[1]);
        cursed += u32::from(
            retail.status & 0x1000 != 0 && c.seats[usize::from(victim)].status & 0x1000 == 0,
        );
        if retail != port && failures.len() < 8 {
            failures.push(format!(
                "round {round} step {step} action {action:#x} victim {victim} run {run:x?}:\n retail {retail:x?}\n port   {port:x?}\n v {:?}",
                c.seats[usize::from(victim)]
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    eprintln!(
        "{rounds} hits: {knockdowns} knockdowns ({gated} gated), {flinches} flinches, {cursed} curses"
    );
    assert!(
        knockdowns > 200 && gated > 100 && flinches > 500 && cursed > 50,
        "{knockdowns} {gated} {flinches} {cursed}"
    );
}
