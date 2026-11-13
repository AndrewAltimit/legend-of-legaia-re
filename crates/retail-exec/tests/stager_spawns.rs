//! The slot-B spawn stagers (the routine a module's parts reach through move-VM
//! op `0x20`) run arm by arm off their module images against the
//! `cast_module_ticks::*_stager_spawns` kernels.
//!
//! Every call `FUN_80021B04` / `FUN_80050ED4` receives is a seat, read whole:
//! the point `a0` names, the rotation `a1` names, the record `a2`, the scale
//! `a3`. Each seat returns a fresh part buffer, so the stores an arm makes into
//! what it seated are read back as the whole buffer against the kernel's
//! `pokes` (and `halve_speed`). The module image is read back whole against the
//! kernel's `patches`, and the battle context and actor records must come back
//! untouched: a stager here moves no state the kernels do not carry, and a
//! write the port does not know about fails the test.

use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_battle_vm::battle_action::SummonFadeTemplate;
use legaia_engine_cast::cast_module_ticks::{self as t, StagerCaller, StagerSpawn};
use psx_retail_exec::{A0, A1, A2, A3, Cpu, Memory, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const BASE: u32 = 0x801F_69D8;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX_PTR: u32 = 0x8007_BD24;
const ACTORS: u32 = 0x801C_9370;
const CTX: u32 = 0x8012_0000;
const CTX_LEN: u32 = 0x1100;
const ACTOR: u32 = 0x8013_0000;
const STRIDE: u32 = 0x400;
const CALLER: u32 = 0x8014_0000;
const PARTS: u32 = 0x8016_0000;
const PART_LEN: u32 = 0x100;
/// Every stager dispatches at most nine arms (`sltiu a1, 9`); the rest are
/// run as the no-ops they are.
const ARMS: u8 = 10;
const SPEED_SEED: u16 = 0xF9A3;
const RAND: u32 = 0x8005_6798;
const HELD_FADE: u32 = 0x8015_E000;
const POOL_FLUSH: u32 = 0x8005_0E74;
/// The monster-AI counter word PROT 0964's arm 0 picks its record by.
const MONSTER_AI_COUNTER: u32 = 0x801C_8FE4;
const SEAT: u32 = 0x8002_1B04;
const SEAT_POOLED: u32 = 0x8005_0ED4;
const FADE: u32 = 0x8002_4E80;
const EFFECT: u32 = 0x801D_FDF0;

const NIGHTO: u32 = 0x801F_7FA8;
const ZENOIR: u32 = 0x801F_8310;
const GIZAM: u32 = 0x801F_7740;
const FREED: u32 = 0x801F_835C;
const HORN: u32 = 0x801F_7EA4;
const VERA: u32 = 0x801F_8078;
const SWORDIE: u32 = 0x801F_89D4;
const THEEDER: u32 = 0x801F_8EAC;
const ORB: u32 = 0x801F_7FE8;
const NOVA: u32 = 0x801F_864C;
const GOLA_GOLA: u32 = 0x801F_7A80;
const ALURU: u32 = 0x801F_88F8;
const SPOON: u32 = 0x801F_8578;
const MUSHURA: u32 = 0x801F_7F34;
const BARRA: u32 = 0x801F_82D8;
const IOTA: u32 = 0x801F_800C;
const GILIUM: u32 = 0x801F_8B90;
const PALMA: u32 = 0x801F_8E68;
const MULE: u32 = 0x801F_8C30;
const JEDO: u32 = 0x801F_8ADC;
const META: u32 = 0x801F_84A4;
const TERRA: u32 = 0x801F_8748;
const KEMARO: u32 = 0x801F_8AB0;
const ULTIMATE_RAVE: u32 = 0x801F_7820;
const CHAOS_BREATH: u32 = 0x801F_7AB8;
const BLADE_BREATH: u32 = 0x801F_813C;
const DOOMSDAY: u32 = 0x801F_7B74;
const GUILTY_CROSS: u32 = 0x801F_7F2C;
const PLASMA_STRIKE: u32 = 0x801F_86B0;
const BLAZING_SLASH: u32 = 0x801F_8D30;
const DEAD_END_CRISIS: u32 = 0x801F_78A4;
const MEGATON_PRESS: u32 = 0x801F_8250;
const OZMA: u32 = 0x801F_92AC;
/// PROT 0934's stager word: arms 1 and 2 step it (`0x801F9370..0x801F9394`).
const OZMA_WORD: u32 = 0x801F_A410;
const EARTHQUAKE: u32 = 0x801F_7FF0;
const HYPER_CRUSH: u32 = 0x801F_7BD0;
const HYPER_LIGHTNING: u32 = 0x801F_7850;
const STEAL: u32 = 0x801F_7DB0;
const POWER_UP: u32 = 0x801F_8118;
const CURSE: u32 = 0x801F_769C;
const WATER_COLUMN: u32 = 0x801F_776C;
const CHAOS_FLARE: u32 = 0x801F_81DC;
const TERIO_PUNCH: u32 = 0x801F_7624;
const WHITE_SHIELD: u32 = 0x801F_9370;
const PARALYZING_WAVE: u32 = 0x801F_7EC4;
const ELEMENT_CHANGE: u32 = 0x801F_8BF8;
const CALL_WAVE: u32 = 0x801F_76C4;
/// PROT 0946's sixteen-slot wave handle table (`addiu s0,v0,0x7f30`).
const CALL_WAVE_HANDLES: u32 = 0x801F_7F30;
const SPIKEFISH: u32 = 0x801F_7AE8;
/// PROT 0925's turn counter, ring count and ring handle table
/// (`-0x7fec(t3)`, `-0x7fe8(s2)`, `addiu v1,v1,-0x7f64` off `0x80200000`).
const SPIKEFISH_TURN: u32 = 0x801F_8014;
const SPIKEFISH_RING: u32 = 0x801F_8018;
const SPIKEFISH_HANDLES: u32 = 0x801F_809C;
const DEATH_GAME: u32 = 0x801F_99F4;
/// PROT 0957's four card handles and the dealt values beside them
/// (`addiu s1,v0,-0x5d48`, `addiu v0,v0,-0x5d78` off `0x80200000`).
const DEATH_GAME_HANDLES: u32 = 0x801F_A2B8;
const DEATH_GAME_TABLE: u32 = 0x801F_A288;
/// Context bytes past everything a stager reads, which the harness spends as
/// the seeds of the module words it plants.
const SEED_BYTES: u32 = CTX + 0x10F0;

/// How often the table stagers' gated arms seated: PROT 0946's wave into a
/// free handle, PROT 0925's ring part, PROT 0957's revealed face.
static TABLE_ARMS: [std::sync::atomic::AtomicU32; 3] =
    [const { std::sync::atomic::AtomicU32::new(0) }; 3];

fn note(arm: usize) {
    TABLE_ARMS[arm].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

fn lcg(s: u32) -> u32 {
    s.wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039)
}

/// What one seat call received.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Seated {
    record: u32,
    pos: [i16; 3],
    rot: [i16; 3],
    scale: u16,
}

#[derive(Default)]
struct Log {
    seats: Vec<Seated>,
    fades: Vec<([u16; 12], u32)>,
    effects: Vec<(u8, [i16; 3], i16)>,
    draws: u32,
}

fn trio(m: &mut Memory, a: u32) -> [i16; 3] {
    [m.rd16(a) as i16, m.rd16(a + 2) as i16, m.rd16(a + 4) as i16]
}

/// The inputs a kernel needs beyond its caller, read off the seeded RAM.
struct Env<'a> {
    entry: u32,
    arm: u8,
    caller: StagerCaller,
    mem: &'a mut Memory,
    rand: &'a mut dyn FnMut() -> u32,
    /// Module-image halfwords the kernel says the arm leaves, as `(VA, value)`.
    module_writes: Vec<(u32, u16)>,
    /// Caller halfwords the kernel says the arm leaves, as `(offset, value)`.
    caller_writes: Vec<(u32, u16)>,
}

/// Store a word into the module image and note it as one the arm leaves.
fn word_write(writes: &mut Vec<(u32, u16)>, va: u32, v: u32) {
    writes.push((va, v as u16));
    writes.push((va + 2, (v >> 16) as u16));
}

type Port = fn(&mut Env) -> Vec<StagerSpawn>;

struct Case {
    entry: u32,
    va: u32,
    /// The op `0x20` second operands this stager is reached with.
    args: std::ops::Range<i16>,
    port: Port,
    /// Module-image words a seat arm stores that no kernel carries, with why.
    unported: &'static [(u32, u32, &'static str)],
}

/// The stager arms that move battle state rather than seat a part: their
/// state is compared by `stager_states`, so this harness reads only their
/// seats.
const STATE_ARMS: [(u32, u8); 5] = [(906, 1), (906, 2), (923, 0), (923, 1), (959, 3)];

/// The screen fades an arm spawns through `FUN_80024E80`, as the host spawns
/// them (`module_stager_spawns`, and `module_code` for PROT 0906 / 0923).
fn expected_fades(entry: u32, arm: u8) -> Vec<(SummonFadeTemplate, u32)> {
    let one = |f: SummonFadeTemplate| vec![(f, 1)];
    match (entry, arm) {
        (906, 0) => legaia_engine_cast::cast_module_camera::GIZAM_STAGER_FLASHES
            .iter()
            .map(|&(f, id)| (f, id as u32))
            .collect(),
        (912, 6) => one(t::FREED_STAGER_FADE),
        (916, 1) => one(t::ALURU_STAGER_FADE_IN),
        (916, 2) => one(t::ALURU_STAGER_FADE_OUT),
        (923, 1) => one(t::GILIUM_STAGER_FADE),
        (929, t::MULE_STAGER_FADE_ARM) => t::MULE_STAGER_FADES.iter().map(|&f| (f, 1)).collect(),
        (959, 2) => one(t::MEGATON_PRESS_WHITE_OUT),
        (959, 3) => one(t::MEGATON_PRESS_HELD_WHITE),
        _ => Vec::new(),
    }
}

/// A fade template as the halfword block `FUN_80024E80` reads; words `2` and
/// `6` are not part of it.
fn fade_block(f: &SummonFadeTemplate) -> [u16; 12] {
    let mut b = [0u16; 12];
    b[0] = f.kind as u16;
    b[1] = f.duration as u16;
    for i in 0..3 {
        b[3 + i] = f.start_rgb[i] as u16;
        b[7 + i] = f.end_rgb[i] as u16;
    }
    b[10] = f.delay as u16;
    b[11] = f.hold as u16;
    b
}

/// Stores an arm makes into its caller, which the host makes on the part it
/// holds: PROT 0905's arm 0 lifts its glow to the sparks' height.
/// PROT 0941's arm 0 clears its caller's `+0x42`, which the port does not
/// carry (`enemy_cast_stager_spawns` discloses it).
///
/// PROT 0957's arms 2 and 3 store the caller card's height `+0x16`
/// (`0x801F9B64`, `0x801F9B70`), which `death_game_stager_spawns` does not
/// carry either.
const CALLER_WRITES: [(u32, u8, u32, u16); 4] = [
    (905, 0, 0x16, t::VERA_SPARK_HEIGHT as u16),
    (941, 0, 0x42, 0),
    (957, 2, 0x16, -0x1C_i16 as u16),
    (957, 3, 0x16, -0x3C_i16 as u16),
];

fn plain(f: fn(u8, StagerCaller) -> Vec<StagerSpawn>) -> impl Fn(&mut Env) -> Vec<StagerSpawn> {
    move |e: &mut Env| f(e.arm, e.caller)
}

fn enemy(e: &mut Env) -> Vec<StagerSpawn> {
    let roll = e.mem.rd32(MONSTER_AI_COUNTER) as u8;
    t::enemy_cast_stager_spawns(e.entry, e.arm, e.caller, roll, e.rand)
}

fn cases() -> Vec<Case> {
    macro_rules! case {
        ($entry:expr, $va:expr, $args:expr, $port:expr) => {
            case!($entry, $va, $args, $port, &[])
        };
        ($entry:expr, $va:expr, $args:expr, $port:expr, $unported:expr) => {
            Case {
                entry: $entry,
                va: $va,
                args: $args,
                port: $port,
                unported: $unported,
            }
        };
    }
    vec![
        case!(907, NIGHTO, 0..1, |e| t::nighto_stager_spawns(
            e.arm, e.caller, e.rand
        )),
        case!(908, ZENOIR, 0..1, |e| {
            let summon = e.mem.rd32(ACTORS + 7 * 4);
            let body = trio(e.mem, summon + 0x3C);
            t::zenoir_stager_spawns(e.arm, e.caller, body, e.rand)
        }),
        case!(906, GIZAM, 0..6, |e| {
            let summon = e.mem.rd32(ACTORS + 7 * 4);
            let body = trio(e.mem, summon + 0x3C);
            t::gizam_stager_spawns(e.arm, e.caller, body, e.rand)
        }),
        case!(912, FREED, 0..1, |e| t::freed_stager_spawns(
            e.arm, e.caller, e.rand
        )),
        case!(930, HORN, 0..1, |e| t::horn_stager_spawns(
            e.arm, e.caller, e.rand
        )),
        case!(
            905,
            VERA,
            0..1,
            |e| t::vera_stager_spawns(e.arm, e.caller, e.rand),
            &[(
                0x801F_8830,
                0x801F_8850,
                "the handles arms 4 and 5 halt; the host tags the parts instead (`vera_arm_kills`)",
            )]
        ),
        case!(910, SWORDIE, 0..1, |e| plain(t::swordie_stager_spawns)(e)),
        case!(904, THEEDER, 0..1, |e| plain(t::theeder_stager_spawns)(e)),
        case!(911, ORB, 0..1, |e| plain(t::orb_stager_spawns)(e)),
        case!(913, NOVA, 0..1, |e| plain(t::nova_stager_spawns)(e)),
        case!(914, GOLA_GOLA, 0..1, |e| plain(t::gola_gola_stager_spawns)(
            e
        )),
        case!(916, ALURU, 0..1, |e| t::aluru_stager_spawns(
            e.arm, e.caller, e.rand
        )),
        case!(919, SPOON, 0..1, |e| plain(t::spoon_stager_spawns)(e)),
        case!(915, MUSHURA, 0..1, |e| plain(t::mushura_stager_spawns)(e)),
        case!(917, BARRA, 0..8, |e| {
            let mut counter = e.mem.rd16(t::BARRA_STAGER_COUNTER);
            let out = t::barra_stager_spawns(e.arm, e.caller, &mut counter);
            if e.arm == 1 {
                e.module_writes.push((t::BARRA_STAGER_COUNTER, counter));
            }
            out
        }),
        case!(934, OZMA, 0..1, |e| {
            let mut word = e.mem.rd32(OZMA_WORD) as i32;
            let before = word;
            let out = t::ozma_stager_spawns(e.arm, e.caller, &mut word, e.rand);
            if word != before {
                e.module_writes.push((OZMA_WORD, word as u16));
                e.module_writes.push((OZMA_WORD + 2, (word >> 16) as u16));
            }
            out
        }),
        case!(921, IOTA, 0..1, |e| t::iota_stager_spawns(
            e.arm, e.caller, e.rand
        )),
        case!(
            923,
            GILIUM,
            0..1,
            |e| plain(t::gilium_stager_spawns)(e),
            &[(
                0x801F_A4D0,
                0x801F_A4D4,
                "the bowl's handle (`GILIUM_BOWL_HANDLE`), which the host parks",
            )]
        ),
        case!(
            928,
            PALMA,
            0..1,
            |e| t::palma_stager_spawns(e.arm, e.caller, e.rand),
            &[(
                0x801F_A0E0,
                0x801F_A0E4,
                "arm 4's handle store, not carried",
            )]
        ),
        case!(929, MULE, 0..1, |e| t::mule_stager_spawns(
            e.arm, e.caller, e.rand
        )),
        case!(931, JEDO, 0..1, |e| plain(t::jedo_stager_spawns)(e)),
        case!(932, META, 0..1, |e| plain(t::meta_stager_spawns)(e)),
        case!(933, TERRA, 0..1, |e| plain(t::terra_stager_spawns)(e)),
        case!(
            918,
            KEMARO,
            0..1,
            |e| plain(t::kemaro_stager_spawns)(e),
            &[(
                0x801F_92C4,
                0x801F_92C8,
                "the lifted part's handle; the host finds the part by its record",
            )]
        ),
        case!(924, ULTIMATE_RAVE, 0..1, |e| plain(
            t::ultimate_rave_stager_spawns
        )(e)),
        case!(938, CHAOS_BREATH, 0..1, |e| plain(
            t::chaos_breath_cell_stager_spawns
        )(e)),
        case!(962, BLADE_BREATH, 0..1, |e| plain(
            t::blade_breath_cell_stager_spawns
        )(e)),
        case!(965, DOOMSDAY, 0..1, |e| plain(t::doomsday_stager_spawns)(e)),
        case!(944, GUILTY_CROSS, 0..1, |e| plain(
            t::guilty_cross_cell_stager_spawns
        )(e)),
        case!(960, PLASMA_STRIKE, 0..1, |e| plain(
            t::plasma_strike_stager_spawns
        )(e)),
        case!(958, BLAZING_SLASH, 0..1, |e| plain(
            t::blazing_slash_stager_spawns
        )(e)),
        case!(961, DEAD_END_CRISIS, 0..1, |e| {
            t::dead_end_crisis_stager_spawns(e.arm, e.caller, e.rand)
        }),
        case!(935, EARTHQUAKE, 0..1, enemy),
        case!(936, HYPER_CRUSH, 0..1, enemy),
        case!(937, HYPER_LIGHTNING, 0..1, enemy),
        case!(941, STEAL, 0..1, enemy),
        case!(942, POWER_UP, 0..1, enemy),
        case!(943, CURSE, 0..1, enemy),
        case!(945, WATER_COLUMN, 0..1, enemy),
        case!(951, CHAOS_FLARE, 0..1, enemy),
        case!(953, TERIO_PUNCH, 0..1, enemy),
        case!(955, WHITE_SHIELD, 0..1, enemy),
        case!(956, PARALYZING_WAVE, 0..1, enemy),
        case!(964, ELEMENT_CHANGE, 0..1, enemy),
        case!(959, MEGATON_PRESS, 0..1, |e| {
            t::megaton_press_stager_spawns(e.arm, e.caller, e.rand)
        }),
        case!(946, CALL_WAVE, 0..1, |e| {
            // The handle table fills from its head (nothing empties it during
            // the cast), so a run of `fill` live handles is every reachable
            // shape; `16` and past is the full table.
            let fill = u32::from(e.mem.rd8(SEED_BYTES)) % 18;
            for k in 0..16 {
                let h = if k < fill { 0x8017_0000 + k * 0x40 } else { 0 };
                e.mem.wr32(CALL_WAVE_HANDLES + k * 4, h);
            }
            let phase = e.mem.rd8(CTX + 0x279);
            let seated = fill.min(16) as usize;
            let out = t::call_wave_stager_spawns(e.arm, e.caller, phase, seated, e.rand);
            if e.arm == 1 && !out.is_empty() {
                word_write(&mut e.module_writes, CALL_WAVE_HANDLES + fill * 4, PARTS);
                note(0);
            }
            out
        }),
        case!(925, SPIKEFISH, 0..1, |e| {
            let turn = e.mem.rd32(SEED_BYTES);
            let ring = u32::from(e.mem.rd8(SEED_BYTES + 4)) % 18;
            e.mem.wr32(SPIKEFISH_TURN, turn);
            e.mem.wr32(SPIKEFISH_RING, ring);
            let phase = e.mem.rd8(CTX + 0x279);
            let mut words = t::SpikefishStagerWords { turn, ring };
            let mut caller = e.caller;
            let out = t::spikefish_stager_spawns(e.arm, &mut caller, phase, &mut words, e.rand);
            if e.arm == 1 {
                word_write(&mut e.module_writes, SPIKEFISH_TURN, words.turn);
                for (o, v) in [
                    (0x14, caller.pos[0]),
                    (0x16, caller.pos[1]),
                    (0x18, caller.pos[2]),
                    (0x28, caller.rot[2]),
                ] {
                    e.caller_writes.push((o, v as u16));
                }
            }
            if words.ring != ring {
                word_write(&mut e.module_writes, SPIKEFISH_RING, words.ring);
                // The ring part is the arm's second seat.
                word_write(
                    &mut e.module_writes,
                    SPIKEFISH_HANDLES + ring * 4,
                    PARTS + PART_LEN,
                );
                note(1);
            }
            out
        }),
        case!(957, DEATH_GAME, 0..1, |e| {
            // The caller is one of the four cards: its handle sits in the
            // table, and the port finds it by the card standing on its `x`.
            let at = u32::from(e.mem.rd8(SEED_BYTES)) % 4;
            let mut table = [0u8; 4];
            let mut cards = [[0i16; 3]; 4];
            for k in 0..4u32 {
                let v = 1 + e.mem.rd8(SEED_BYTES + 1 + k) % 4;
                table[k as usize] = v;
                e.mem.wr32(DEATH_GAME_TABLE + k * 4, u32::from(v));
                let h = if k == at {
                    CALLER
                } else {
                    0x8017_0000 + k * 0x40
                };
                e.mem.wr32(DEATH_GAME_HANDLES + k * 4, h);
                let x = if k == at {
                    e.caller.pos[0]
                } else {
                    e.caller.pos[0].wrapping_add(0x37 * (k as i16 + 1))
                };
                cards[k as usize] = [x, -0x1C0, 0];
            }
            let out = t::death_game_stager_spawns(e.arm, e.caller, table, cards);
            if e.arm <= 1 && !out.is_empty() {
                word_write(&mut e.module_writes, DEATH_GAME_HANDLES + at * 4, PARTS);
                if e.arm == 1 {
                    note(2);
                }
            }
            out
        }),
    ]
}

fn seat_hook(cpu: &mut Cpu, va: u32, log: Rc<RefCell<Log>>) {
    cpu.hook(va, move |r, m| {
        let mut l = log.borrow_mut();
        let part = PARTS + l.seats.len() as u32 * PART_LEN;
        let s = Seated {
            pos: trio(m, r.r[A0]),
            rot: trio(m, r.r[A1]),
            record: r.r[A2],
            scale: r.r[A3] as u16,
        };
        l.seats.push(s);
        for b in 0..PART_LEN {
            m.wr8(part + b, 0);
        }
        // What the seat's own move-VM run leaves in the speed word.
        m.wr16(part + 0x98, SPEED_SEED);
        r.r[V0] = part;
    });
}

fn caller_view(m: &mut Memory, arg: i16) -> StagerCaller {
    StagerCaller {
        pos: trio(m, CALLER + 0x14),
        rot: trio(m, CALLER + 0x24),
        radius_b4: m.rd16(CALLER + 0xB4) as i16,
        arg,
        scale: m.rd16(CALLER + 0x72),
        disp: trio(m, CALLER + 0x3C),
        walk_successor: false,
    }
}

/// The part buffer the kernel says a seat leaves.
fn expected_part(s: &StagerSpawn) -> Vec<u8> {
    let mut b = vec![0u8; PART_LEN as usize];
    let mut put = |o: usize, v: u16| b[o..o + 2].copy_from_slice(&v.to_le_bytes());
    put(0x98, SPEED_SEED);
    if s.halve_speed {
        put(0x98, ((SPEED_SEED as i16) / 2) as u16);
    }
    for &(o, v) in &s.pokes {
        put(usize::from(o), v);
    }
    b
}

fn run_case(
    cpu: &mut Cpu,
    module: &[u8],
    case: &Case,
    log: &Rc<RefCell<Log>>,
    rng: &mut Rng,
) -> u32 {
    let mut seated = 0;
    for round in 0..48 {
        for arm in 0..=ARMS {
            for arg in case.args.clone() {
                cpu.mem.load(BASE, module);
                for b in 0..CTX_LEN {
                    cpu.mem.wr8(CTX + b, rng.next() as u8);
                }
                let roll = rng.below(4);
                cpu.mem.wr32(MONSTER_AI_COUNTER, roll);
                // The held fade PROT 0959's arm 2 halts.
                cpu.mem.wr32(CTX + 0x102C, HELD_FADE);
                for s in 0..8u32 {
                    let a = ACTOR + s * STRIDE;
                    cpu.mem.wr32(ACTORS + s * 4, a);
                    for o in (0..STRIDE).step_by(2) {
                        cpu.mem.wr16(a + o, (rng.next() % 0x1000) as u16);
                    }
                }
                for o in (0..0x200).step_by(2) {
                    let v = (rng.next() % 0x4000) as u16;
                    cpu.mem.wr16(CALLER + o, v.wrapping_sub(0x2000));
                }
                cpu.mem
                    .wr16(CALLER + 0x72, 0x400 + (rng.next() % 0x1800) as u16);
                if matches!(case.entry, 925 | 946) {
                    // The module phase the arm gates on: half the rounds in
                    // the one it tests for, the rest anywhere near it.
                    let p = if rng.below(2) == 0 { 7 } else { rng.below(10) };
                    cpu.mem.wr8(CTX + 0x279, p as u8);
                }
                let caller = caller_view(&mut cpu.mem, arg);
                let ctx_before = cpu.mem.read(CTX, CTX_LEN as usize);
                let actors_before = cpu.mem.read(ACTOR, (8 * STRIDE) as usize);
                let caller_before = cpu.mem.read(CALLER, 0x200);

                let seed = rng.next();
                let mut s = seed;
                let mut port_draws = 0u32;
                let mut port_rand = || {
                    port_draws += 1;
                    s = lcg(s);
                    (s >> 16) & 0x7FFF
                };
                if case.entry == 917 {
                    cpu.mem.wr16(t::BARRA_STAGER_COUNTER, rng.below(6) as u16);
                }
                if case.entry == 934 {
                    cpu.mem.wr32(OZMA_WORD, rng.below(10));
                }
                let mut env = Env {
                    entry: case.entry,
                    arm,
                    caller,
                    mem: &mut cpu.mem,
                    rand: &mut port_rand,
                    module_writes: Vec::new(),
                    caller_writes: Vec::new(),
                };
                let want = (case.port)(&mut env);
                let module_writes = env.module_writes;
                let caller_writes = env.caller_writes;
                let module_seeded = cpu.mem.read(BASE, module.len());
                *log.borrow_mut() = Log::default();
                {
                    let stream = Rc::new(std::cell::Cell::new(seed));
                    let log = log.clone();
                    cpu.hook(RAND, move |r, _| {
                        let n = lcg(stream.get());
                        stream.set(n);
                        log.borrow_mut().draws += 1;
                        r.r[V0] = (n >> 16) & 0x7FFF;
                    });
                }
                let stop = cpu.call(case.va, &[CALLER, u32::from(arm), arg as u16 as u32, 0]);
                let c = format!("PROT {:04} arm {arm} arg {arg} round {round}", case.entry);
                assert_eq!(stop, Stop::Returned, "{c}");
                let l = log.borrow();
                let got: Vec<Seated> = l.seats.clone();
                let want_seats: Vec<Seated> = want
                    .iter()
                    .map(|w| Seated {
                        record: w.record,
                        pos: w.pos,
                        rot: w.rot,
                        scale: w.scale,
                    })
                    .collect();
                assert_eq!(got, want_seats, "seats: {c}");
                assert_eq!(l.draws, port_draws, "rand draws: {c}");
                let want_fades: Vec<([u16; 12], u32)> = expected_fades(case.entry, arm)
                    .iter()
                    .map(|(f, id)| (fade_block(f), *id))
                    .collect();
                assert_eq!(l.fades, want_fades, "fades: {c}");
                let want_fx: Vec<(u8, [i16; 3])> = t::stager_effect(case.entry, arm, caller)
                    .map(|f| (f.id, f.pos))
                    .into_iter()
                    .collect();
                let got_fx: Vec<(u8, [i16; 3])> = l.effects.iter().map(|e| (e.0, e.1)).collect();
                assert_eq!(got_fx, want_fx, "effect spawns: {c}");
                let got_angles: Vec<i16> = l.effects.iter().map(|e| e.2).collect();
                let want_angles: Vec<i16> = t::stager_effect(case.entry, arm, caller)
                    .map(|f| f.angle)
                    .into_iter()
                    .collect();
                assert_eq!(got_angles, want_angles, "effect angles: {c}");
                for (k, w) in want.iter().enumerate() {
                    let part = cpu.mem.read(PARTS + k as u32 * PART_LEN, PART_LEN as usize);
                    let exp = expected_part(w);
                    assert_eq!(
                        part,
                        exp,
                        "part {k} ({:#010x}): {c}: {:?}",
                        w.record,
                        crate::common::diff_offsets(&part, &exp)
                    );
                }
                // The module image, against the record patches and the
                // disclosed stores.
                let after = cpu.mem.read(BASE, module.len());
                let mut exp = module_seeded;
                let patches = want.iter().flat_map(|w| w.patches.iter());
                for &(va, v) in patches.chain(module_writes.iter()) {
                    let o = (va - BASE) as usize;
                    exp[o..o + 2].copy_from_slice(&v.to_le_bytes());
                }
                let mut after_masked = after.clone();
                for &(lo, hi, _) in case.unported {
                    for va in lo..hi {
                        let o = (va - BASE) as usize;
                        after_masked[o] = exp[o];
                    }
                }
                let stray: Vec<String> = (0..exp.len())
                    .step_by(4)
                    .filter(|&o| after_masked[o..o + 4] != exp[o..o + 4])
                    .map(|o| format!("{:#010x}", BASE + o as u32))
                    .collect();
                assert!(stray.is_empty(), "module image: {c}: words {stray:?}");
                if STATE_ARMS.contains(&(case.entry, arm)) {
                    seated += got.len() as u32;
                    continue;
                }
                assert!(
                    cpu.mem.read(CTX, CTX_LEN as usize) == ctx_before,
                    "ctx: {c}"
                );
                let actors_after = cpu.mem.read(ACTOR, (8 * STRIDE) as usize);
                assert!(
                    actors_after == actors_before,
                    "actors: {c}: {:?}",
                    crate::common::diff_offsets(&actors_after, &actors_before)
                );
                let mut caller_before = caller_before;
                for &(e, a, o, v) in &CALLER_WRITES {
                    if (e, a) == (case.entry, arm) {
                        caller_before[o as usize..o as usize + 2].copy_from_slice(&v.to_le_bytes());
                    }
                }
                for &(o, v) in &caller_writes {
                    caller_before[o as usize..o as usize + 2].copy_from_slice(&v.to_le_bytes());
                }
                let caller_after = cpu.mem.read(CALLER, 0x200);
                assert!(
                    caller_after == caller_before,
                    "caller: {c}: {:?}",
                    crate::common::diff_offsets(&caller_after, &caller_before)
                );
                seated += got.len() as u32;
            }
        }
    }
    seated
}

fn stager_cpu() -> Option<(Cpu, Rc<RefCell<Log>>)> {
    let (mut cpu, disc) = (scus_cpu()?, disc()?);
    cpu.mem.load(OVERLAY_BASE, &disc.read_entry(898).ok()?);
    cpu.mem.wr32(CTX_PTR, CTX);
    // The runtime pointers to the SCUS cosine / sine tables.
    cpu.mem.wr32(0x8007_B81C, 0x8007_0A2C);
    cpu.mem.wr32(0x8007_B7F8, 0x8007_0A2C + 0x800);
    let log = Rc::new(RefCell::new(Log::default()));
    seat_hook(&mut cpu, SEAT, log.clone());
    seat_hook(&mut cpu, SEAT_POOLED, log.clone());
    {
        let log = log.clone();
        cpu.hook(FADE, move |r, m| {
            let mut b = [0u16; 12];
            for (i, w) in b.iter_mut().enumerate() {
                *w = m.rd16(r.r[A0] + 2 * i as u32);
            }
            b[2] = 0;
            b[6] = 0;
            log.borrow_mut().fades.push((b, r.r[A1]));
            r.r[V0] = 0x8015_F000;
        });
    }
    cpu.stub(POOL_FLUSH, 0);
    {
        let log = log.clone();
        cpu.hook(EFFECT, move |r, m| {
            log.borrow_mut()
                .effects
                .push((r.r[A0] as u8, trio(m, r.r[A1]), r.r[A2] as i16));
            r.r[V0] = 0;
        });
    }
    Some((cpu, log))
}

fn run(entries: &[u32]) {
    let Some((mut cpu, log)) = stager_cpu() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let disc = disc().expect("disc");
    let mut rng = Rng(0x5354_4147);
    let mut failures = Vec::new();
    for case in cases().iter().filter(|c| entries.contains(&c.entry)) {
        let module = disc.read_entry(case.entry as usize).expect("module");
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run_case(&mut cpu, &module, case, &log, &mut rng)
        }));
        match r {
            Ok(0) => failures.push(format!("PROT {:04}: no arm seated anything", case.entry)),
            Ok(_) => {}
            Err(e) => failures.push(
                e.downcast_ref::<String>()
                    .cloned()
                    .unwrap_or_else(|| format!("PROT {:04}: panicked", case.entry)),
            ),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn the_summon_stagers_seat_what_retail_seats() {
    differential!(
        prot(907, NIGHTO),
        prot(908, ZENOIR),
        prot(906, GIZAM),
        prot(912, FREED),
        prot(930, HORN),
        prot(905, VERA),
        prot(910, SWORDIE),
        prot(904, THEEDER),
        prot(911, ORB),
        prot(913, NOVA)
    );
    run(&[907, 908, 906, 912, 930, 905, 910, 904, 911, 913]);
}

#[test]
fn the_creature_stagers_seat_what_retail_seats() {
    differential!(
        prot(914, GOLA_GOLA),
        prot(916, ALURU),
        prot(919, SPOON),
        prot(915, MUSHURA),
        prot(921, IOTA),
        prot(923, GILIUM),
        prot(928, PALMA),
        prot(929, MULE),
        prot(931, JEDO),
        prot(932, META),
        prot(933, TERRA),
        prot(918, KEMARO),
        prot(924, ULTIMATE_RAVE),
        prot(917, BARRA),
        prot(934, OZMA)
    );
    run(&[
        914, 916, 919, 915, 921, 923, 928, 929, 931, 932, 933, 918, 924, 917, 934,
    ]);
}

#[test]
fn the_capture_cell_stagers_seat_what_retail_seats() {
    differential!(
        prot(938, CHAOS_BREATH),
        prot(962, BLADE_BREATH),
        prot(965, DOOMSDAY),
        prot(944, GUILTY_CROSS),
        prot(960, PLASMA_STRIKE),
        prot(961, DEAD_END_CRISIS),
        prot(959, MEGATON_PRESS),
        prot(958, BLAZING_SLASH)
    );
    run(&[938, 962, 965, 944, 960, 961, 959, 958]);
}

#[test]
fn the_enemy_cast_stagers_seat_what_retail_seats() {
    differential!(
        prot(935, EARTHQUAKE),
        prot(936, HYPER_CRUSH),
        prot(937, HYPER_LIGHTNING),
        prot(941, STEAL),
        prot(942, POWER_UP),
        prot(943, CURSE),
        prot(945, WATER_COLUMN),
        prot(951, CHAOS_FLARE),
        prot(953, TERIO_PUNCH),
        prot(955, WHITE_SHIELD),
        prot(956, PARALYZING_WAVE),
        prot(964, ELEMENT_CHANGE)
    );
    run(&[935, 936, 937, 941, 942, 943, 945, 951, 953, 955, 956, 964]);
}

#[test]
fn the_table_stagers_seat_what_retail_seats() {
    differential!(
        prot(946, CALL_WAVE),
        prot(925, SPIKEFISH),
        prot(957, DEATH_GAME)
    );
    run(&[946, 925, 957]);
    if disc().is_some() {
        let n = TABLE_ARMS
            .each_ref()
            .map(|a| a.load(std::sync::atomic::Ordering::Relaxed));
        assert!(n.iter().all(|&k| k > 5), "gated arms seated {n:?}");
    }
}
