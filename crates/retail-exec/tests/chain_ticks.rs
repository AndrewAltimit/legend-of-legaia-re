//! The phase-chain tick bodies (`cast_module_ticks::chain_bodies`), each run
//! off its owning image from arm 0 to its Done, pass by pass, against the
//! port's pass: `run_chain_body`, the body's camera director
//! (`capture_camera_director`) with its seat writes, the band's caster
//! pre-pass (`CAPTURE_CASTER_STAGES`), its party-row rate (`party_rate_sweep`)
//! and the fold's hit-side rate (`ChainArm::hit_rate`).
//!
//! Every call out of the image returns `0` but `rand()` (a scripted stream),
//! the bearing (`bearing_12bit_approx`, its own differential) and the damage
//! wrappers, which return a small roll and log the seat they were handed. The
//! battle frame's scalar and step are seeded wide, so a countdown gate drains
//! in a pass or two, and every seat's playing clip `+0x1D9` reads idle before
//! each pass, so a settle wait passes. A pass that holds is not compared; on
//! each pass that leaves its arm the whole context and all eight actor records
//! are, but for the bytes another owner writes ([`owner`]): the fold's HP side
//! and the reaction it stages, the presentation stores (tint, hide / show,
//! render flags), the seats' poses, the cue byte and PROT 0963's lighting.
//! What is left - the phase walk, the Done, `ctx[+0x278]` / `+0x0D`, every
//! seat's `+0x1DA` / `+0x1DC` / `+0x21D` and any other byte - must match,
//! less the gaps [`KNOWN`] names with their reason.

use crate::cast_view::{read_actor, read_ctx, write_actor, write_ctx};
use crate::common::{Rng, disc, scus_cpu};
use legaia_engine_battle_vm::battle_target_group::GroupAim;
use legaia_engine_cast::cast_module_camera::{
    self as cam, CaptureSeatWrite, ModuleCamSeats, ModuleCamState, ModuleSeat,
};
use legaia_engine_cast::cast_module_ticks::{
    self as t, CastActorState, CasterClipRule, ChainActor, ChainBody, ChainClip, ChainRestage,
};
use psx_retail_exec::{A0, A1, A2, A3, Stop, V0};
use std::cell::RefCell;
use std::rc::Rc;

const BASE: u32 = 0x801F_69D8;
const OVERLAY_BASE: u32 = 0x801C_E818;
const CTX_PTR: u32 = 0x8007_BD24;
const ACTORS: u32 = 0x801C_9370;
const RECORDS: u32 = 0x801C_9348;
const CTX: u32 = 0x8012_0000;
const CTX_LEN: usize = 0x1100;
const ACTOR: u32 = 0x8013_0000;
const STRIDE: usize = 0x400;
const NODES: u32 = 0x8014_0000;
const MONSTER_RECORD: u32 = 0x8015_0000;
const RAND: u32 = 0x8005_6798;
const BEARING: u32 = 0x8001_9B28;
const WRAPPERS: [u32; 3] = [0x801D_D0AC, 0x801D_D4B0, 0x801D_D6B4];
const SCALAR: u32 = 0x1F80_037D;
const STEP: u32 = 0x1F80_0393;
const FORMATION: u32 = 0x8007_BD0C;
const ROSTER: u32 = 0x8007_BD10;
/// The roll PROT 0964's `0xAF` body last accepted, which its strike forks on.
const ELEMENT_ROLL: u32 = 0x801C_8FE4;
/// What every damage wrapper returns: a hit that lands and kills no one.
const ROLL: u32 = 7;
const CASTER: usize = 3;
/// The Battles vibration option word: non-zero turns the pad's rumble off.
const VIBRATION_OFF: u32 = 0x8008_45C8;
/// The rumble words: pulse, level and their rates (`0x8007B92C`,
/// `0x8007B930`, `gp+0xA30`, `gp+0xA34`), and what the walk seeds them with.
const RUMBLE: [u32; 4] = [0x8007_B92C, 0x8007_B930, 0x8007_BD48, 0x8007_BD4C];
const RUMBLE_SENTINEL: u32 = 0x5A5A_5A5A;
const CROSS_BEAM_PACKETS: u32 = 0x801F_726C;
/// The target-group aim `FUN_801DCEAC` and the buffers the rig hands it.
const AIM: u32 = 0x801D_CEAC;
const OUT_CENTRE: u32 = 0x8011_0000;
const OUT_EXTENT: u32 = 0x8011_0100;

/// Which owner a record byte belongs to when it is not the tick body's port.
fn owner(off: usize, struck: bool) -> Option<&'static str> {
    match off {
        // The fold lands the hit: HP, the bar delta, the combo total, the
        // status word, and on a struck seat the reaction and its restage.
        0x00..=0x03 | 0x10..=0x13 | 0x14C..=0x14D | 0x16E..=0x16F if struck => Some("fold"),
        0x1DA | 0x1DC if struck => Some("fold"),
        // Tint, blend, hide / show, the render flags.
        0x04..=0x0F | 0x21C | 0x21F | 0x225 => Some("presentation"),
        // The live and body pairs, the height and the heading.
        0x34..=0x41 | 0x46..=0x47 => Some("pose"),
        _ => None,
    }
}

/// The context bytes another owner writes.
fn ctx_owner(entry: u32, off: usize) -> Option<&'static str> {
    match (entry, off) {
        (_, 0x18) => Some("the cue byte"),
        (963, 0x27A..=0x27D | 0x286 | 0x32C) => Some("PROT 0963's lighting"),
        _ => None,
    }
}

/// The residuals the port carries on purpose, as `(entry, arm, seat, offset,
/// why)`; `seat` is `CASTER`, or `0` for any party seat.
const KNOWN: &[(u32, u8, usize, usize, &str)] = &[
    (
        964,
        7,
        7,
        0x14C,
        "the creature's HP zero as it retires (not ported)",
    ),
    (
        964,
        7,
        7,
        0x14D,
        "the creature's HP zero as it retires (not ported)",
    ),
    (964, 0x37, 7, 0x14C, "the creature's HP zero"),
    (964, 0x37, 7, 0x14D, "the creature's HP zero"),
    (964, 0x69, 7, 0x14C, "the creature's HP zero"),
    (964, 0x69, 7, 0x14D, "the creature's HP zero"),
    (
        960,
        4,
        CASTER,
        0x21D,
        "the rate rule: the caster's 0 is never restored in the body",
    ),
    (
        963,
        6,
        CASTER,
        0x21D,
        "the rate rule: arms 6..0xB alternate the caster 3 / 4",
    ),
    (963, 7, CASTER, 0x21D, "the rate rule"),
    (963, 8, CASTER, 0x21D, "the rate rule"),
    (963, 9, CASTER, 0x21D, "the rate rule"),
    (963, 0x0A, CASTER, 0x21D, "the rate rule"),
    (963, 0x0B, CASTER, 0x21D, "the rate rule"),
    (
        961,
        1,
        CASTER,
        0x21D,
        "the rate rule: arm 1's caster 3 is never restored in the body",
    ),
    (
        963,
        0x10,
        0,
        0x21D,
        "the party freeze beside the hide the port does not carry",
    ),
    (963, 0x11, 0, 0x21D, "the restore of that freeze"),
];

/// A director's seat write onto one record's bytes, as
/// `World::apply_capture_seat_writes` makes it.
fn apply_seat_write(rec: &mut [u8], w: &CaptureSeatWrite) {
    if let Some(r) = w.rate {
        rec[0x21D] = r;
    }
    if w.loop_release {
        rec[0x21B] = 0;
        rec[0x176..0x178].fill(0);
    }
    if let Some(c) = w.stage {
        rec[0x1DA] = c;
        if w.restage {
            rec[0x1DC] = rec[0x1DC].wrapping_add(1);
        }
    }
}

fn seat_of(rec: &[u8], s: usize) -> ModuleSeat {
    let o = s * STRIDE;
    let h = |k: usize| i16::from_le_bytes([rec[o + k], rec[o + k + 1]]);
    ModuleSeat {
        x: h(0x34),
        y: h(0x36),
        z: h(0x38),
        facing: h(0x46) as u16 & 0xFFF,
    }
}

/// Every `jal` target the image's words encode, outside the image.
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

/// The clip literals the band's pre-pass stages for this cast, if the body
/// has a row.
fn pre_pass_clips(entry: u32, action: u8, formation: u8) -> Vec<u8> {
    t::CAPTURE_CASTER_STAGES
        .iter()
        .filter(|r| {
            r.prot_entry == entry && (r.action_ids.is_empty() || r.action_ids.contains(&action))
        })
        .flat_map(|r| match r.rule {
            CasterClipRule::Fixed(c) => c.to_vec(),
            CasterClipRule::SeatMonster { id, hit, miss } => {
                vec![if formation == id { hit } else { miss }]
            }
            CasterClipRule::SeatMonsterBands(b) => b
                .iter()
                .filter(|&&(lo, hi, _)| (lo..=hi).contains(&formation))
                .map(|&(_, _, c)| c)
                .collect(),
            CasterClipRule::FirstMonster { id, hit, miss } => {
                vec![if formation == id { hit } else { miss }]
            }
            #[allow(unreachable_patterns)]
            _ => Vec::new(),
        })
        .collect()
}

struct Run {
    /// The arms the body left, in order.
    path: Vec<u8>,
    gaps: Vec<String>,
    known: usize,
    hits: usize,
    /// Passes whose split hits were compared.
    split_checked: usize,
    /// Director spawns matched against retail's.
    spawns_checked: usize,
    /// Passes a victim settle held, against retail's hold.
    settles_checked: usize,
    /// Passes retail stored a rumble on, compared.
    rumbles: usize,
}

/// The bodies whose victim settle the walk holds and releases.
const SETTLE_WALKED: [u32; 3] = [939, 947, 948];

/// Who a walk casts at and which seats stand: `target` a seat or a group code
/// (`None`: a random party seat), `dead` the seats at zero HP (party seats
/// play the fallen clip `8`, which a row's settle reads), `stoned` the seats
/// whose status word carries Stone (`+0x16E & 4`), which a row sweep skips.
#[derive(Debug, Clone, Copy, Default)]
struct Field {
    target: Option<u8>,
    dead: u8,
    stoned: u8,
    /// Seats standing on less HP than a hit's roll, which the hit kills.
    low: u8,
    /// Seats standing on more HP than every hit's roll together, which no
    /// hit downs.
    high: u8,
    /// Seats whose status word already carries all three Rot limbs
    /// (`+0x16E & 0x38`).
    rotted: u8,
    /// The `+0x6BC` word of every party seat's character record
    /// (`0x80084140 + (id - 1) * 0x414`), whose Rot guards PROT 0939 reads
    /// and whose Numb / Master Guard bits PROT 0956's mark reads.
    ability: u32,
    /// The victim's action category `+0x1DE` and initiative key `+0x16C`,
    /// which a turn steal reads.
    queued: Option<(u8, u16)>,
}

/// The inventory find-or-insert a turn steal refunds an Item action through.
const REFUND: u32 = 0x8004_21D4;
/// The part-actor pool (`0x80` handles), and the two parts a PROT 0956 walk
/// seats in it, whose level `+0x78` arm 3's fade writes.
const PART_POOL: u32 = 0x801C_90F0;
const POOLED: [(u32, u32); 2] = [(5, 0x8016_0000), (0x41, 0x8016_0100)];

/// PROT 0939's hit count, `0x801F7AF8`.
const SPORE_GAS_COUNT: u32 = 0x801F_7AF8;
/// The readout flag PROT 0939's hits 1..3 store `0` into (`0x801F70FC`).
const CLUSTER_FLAG: u32 = 0x8007_B64C;

fn run_body(
    body: &ChainBody,
    action: u8,
    formation: u8,
    selector: u8,
    seed: u64,
    field: Field,
    step: u8,
) -> Option<Run> {
    let disc = disc()?;
    let mut cpu = scus_cpu()?;
    cpu.mem.load(OVERLAY_BASE, &disc.read_entry(898).ok()?);
    let module = disc.read_entry(body.prot_entry as usize).ok()?;
    cpu.mem.load(BASE, &module);
    cpu.mem.wr32(CTX_PTR, CTX);
    let stream = Rc::new(std::cell::Cell::new(seed as u32));
    let hits = Rc::new(RefCell::new(Vec::<usize>::new()));
    let rolls = Rc::new(RefCell::new(Vec::<u32>::new()));
    let draws = Rc::new(RefCell::new(Vec::<u32>::new()));
    let refunds = Rc::new(RefCell::new(Vec::<(u32, u32)>::new()));
    let spawns = Rc::new(RefCell::new(Vec::<(u32, [i16; 3], [i16; 3], u16)>::new()));
    for target in jal_targets(&module) {
        if target == RAND {
            let stream = stream.clone();
            let draws = draws.clone();
            cpu.hook(RAND, move |r, _| {
                let n = stream.get().wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
                stream.set(n);
                r.r[V0] = (n >> 16) & 0x7FFF;
                draws.borrow_mut().push(r.r[V0]);
            });
        } else if target == BEARING {
            cpu.hook(BEARING, |r, _| {
                let a = |i: usize| r.r[A0 + i] as i16;
                r.r[V0] = u32::from(
                    legaia_engine_battle_vm::battle_action::bearing_12bit_approx(
                        a(0),
                        a(1),
                        a(2),
                        a(3),
                    ),
                );
            });
        } else if WRAPPERS.contains(&target) {
            let hits = hits.clone();
            let rolls = rolls.clone();
            // A split-hit body (PROT 0937, 0939) rolls each hit for real and
            // keeps a share: its rolls vary, so the share's divisor shows.
            let split = t::split_hits_for(body.prot_entry, body.body).is_some();
            cpu.hook(target, move |r, _| {
                hits.borrow_mut().push((r.r[A2] as usize).min(7));
                let roll = if split {
                    [ROLL, 997, 64, 5000][rolls.borrow().len() % 4]
                } else {
                    ROLL
                };
                rolls.borrow_mut().push(roll);
                r.r[V0] = roll;
            });
        } else if target == REFUND {
            let refunds = refunds.clone();
            cpu.hook(REFUND, move |r, _| {
                refunds.borrow_mut().push((r.r[A0], r.r[A1]));
                r.r[V0] = 0;
            });
        } else if target == AIM {
            // The group aim runs live: a group framing needs it whole.
        } else if target == SPAWN_PART || target == SPAWN_POOLED {
            // The part spawn and its pool wrapper, logged as `(record, point,
            // angles, scale)`.
            let spawns = spawns.clone();
            cpu.hook(target, move |r, m| {
                let trio = |m: &mut psx_retail_exec::Memory, a: u32| {
                    [0u32, 2, 4].map(|o| m.rd16(a + o) as i16)
                };
                let p = trio(m, r.r[A0]);
                let a = trio(m, r.r[A1]);
                spawns.borrow_mut().push((r.r[A2], p, a, r.r[A3] as u16));
                r.r[V0] = 0;
            });
        } else {
            cpu.stub(target, 0);
        }
    }
    // PROT 0948's beam packet builder, reached by a `jal` inside the image:
    // GPU packets only (`legaia_render_kernels::cast_beam`).
    if body.prot_entry == 948 {
        cpu.stub(CROSS_BEAM_PACKETS, 0);
    }
    // The SCUS sine and cosine table pointers the arms' placements read.
    cpu.mem.wr32(0x8007_B81C, 0x8007_0A2C);
    cpu.mem.wr32(0x8007_B7F8, 0x8007_0A2C + 0x800);
    let mut rng = Rng(seed);
    for b in 0..CTX_LEN as u32 {
        cpu.mem.wr8(CTX + b, 0);
    }
    // Three party seats, three monsters, the first of them casting.
    let victim = field.target.map_or(rng.below(3) as usize, usize::from);
    cpu.mem.wr8(CTX, 3);
    cpu.mem.wr8(CTX + 1, 3);
    cpu.mem.wr8(CTX + 0x13, CASTER as u8);
    cpu.mem.wr16(CTX + 0x6D0, 0x900);
    for s in 0..8u32 {
        let a = ACTOR + s * STRIDE as u32;
        cpu.mem.wr32(ACTORS + s * 4, a);
        for o in (0..STRIDE as u32).step_by(2) {
            cpu.mem.wr16(a + o, 0);
        }
        let hp = if field.dead & (1 << s) != 0 {
            0
        } else if field.low & (1 << s) != 0 {
            (ROLL - 4) as u16
        } else if field.high & (1 << s) != 0 {
            3000
        } else {
            100 + rng.below(400) as u16
        };
        cpu.mem.wr16(a + 0x14C, hp);
        if field.stoned & (1 << s) != 0 {
            cpu.mem.wr16(a + 0x16E, 4);
        }
        if field.rotted & (1 << s) != 0 {
            let w = cpu.mem.rd16(a + 0x16E);
            cpu.mem.wr16(a + 0x16E, w | 0x38);
        }
        cpu.mem.wr16(a + 0x172, 1000);
        cpu.mem.wr8(a + 0x1DA, rng.below(0x10) as u8);
        cpu.mem.wr8(a + 0x1DC, rng.below(0x10) as u8);
        for (k, o) in (0x1EFu32..=0x1F3).enumerate() {
            cpu.mem
                .wr8(a + o, 0x10 + k as u8 * 0x10 + rng.below(0x10) as u8);
        }
        // A fallen seat holds the rate its last reaction left, so a sweep
        // that skips it shows.
        cpu.mem.wr8(a + 0x21D, if hp == 0 { 4 } else { 8 });
        cpu.mem.wr8(a + 0x1DF, action);
        cpu.mem.wr16(a + 0x46, rng.below(0x1000) as u16);
        for o in [0x34u32, 0x38, 0x3C, 0x40] {
            cpu.mem
                .wr16(a + o, (rng.below(0x1000) as u16).wrapping_sub(0x800));
        }
        let node = NODES + s * 0x100;
        cpu.mem.wr32(a + 0x22C, node);
        cpu.mem.wr16(node + 0x58, 0x80);
        cpu.mem.wr16(node + 0x72, 0x1000);
    }
    if let Some((category, key)) = field.queued
        && victim < 8
    {
        let a = ACTOR + (victim * STRIDE) as u32;
        cpu.mem.wr8(a + 0x1DE, category);
        cpu.mem.wr16(a + 0x16C, key);
    }
    let caster_rec = ACTOR + (CASTER * STRIDE) as u32;
    cpu.mem.wr8(caster_rec + 0x1DD, victim as u8);
    cpu.mem.wr8(caster_rec + 0x1DE, 2);
    for i in 0..5 {
        cpu.mem.wr32(RECORDS + i * 4, MONSTER_RECORD);
        cpu.mem.wr8(FORMATION + i, formation);
    }
    for i in 0..3 {
        cpu.mem.wr8(ROSTER + i, 1 + i as u8);
        // The record's Rot guards, and a head on both Auto strings.
        let r = CHAR_RECORDS + i * CHAR_STRIDE;
        cpu.mem.wr32(r + 0x6BC, field.ability);
        cpu.mem.wr8(r + 0x76F, 0x11 + i as u8);
        cpu.mem.wr8(r + 0x77F, 0x21 + i as u8);
    }
    cpu.mem.wr32(ELEMENT_ROLL, u32::from(selector));
    let vibration_off = seed & 1 == 1;
    cpu.mem.wr32(VIBRATION_OFF, u32::from(vibration_off));
    if body.prot_entry == 956 {
        for (slot, part) in POOLED {
            cpu.mem.wr32(PART_POOL + slot * 4, part);
        }
    }
    // `step` vsyncs a pass at the speed scalar.
    cpu.mem.wr8(SCALAR, cam::SPEED_SCALAR as u8);
    cpu.mem.wr8(STEP, step);

    let key = if t::capture_trampoline_for(body.prot_entry).is_some() {
        body.body
    } else {
        cam::SINGLE_BODY
    };
    let director = cam::capture_camera_director(body.prot_entry, key);
    let pre_pass = if body.caster_stages_replayed() {
        pre_pass_clips(body.prot_entry, action, formation)
    } else {
        Vec::new()
    };
    let mut st = ModuleCamState {
        lead_roster: 1,
        opens_pass: true,
        frame_step: step,
        victim_settled: true,
        caster_in_reach: true,
        party_settled: true,
        target_code: Some(victim as u8),
        ..ModuleCamState::default()
    };
    // The tint word a monster seat's aim counts it up by (`FUN_800513F0`'s
    // seating), on every standing monster.
    for s in 3..8u32 {
        if field.dead & (1 << s) == 0 {
            cpu.mem.wr32(ACTOR + s * STRIDE as u32 + 4, 0x1004_0100);
        }
    }
    let mut out = Run {
        path: Vec::new(),
        gaps: Vec::new(),
        known: 0,
        hits: 0,
        split_checked: 0,
        spawns_checked: 0,
        settles_checked: 0,
        rumbles: 0,
    };
    let mut held = 0;
    let mut settle_run: Option<usize> = None;
    let (mut quake_passes, mut quake_phase) = (0usize, 0xFEu8);
    let quake_hold = 1 + (usize::from(formation) + victim) % 4;
    for _ in 0..40_000 {
        for s in 0..8u32 {
            let a = ACTOR + s * STRIDE as u32;
            let fallen = s < 3 && cpu.mem.rd16(a + 0x14C) == 0;
            cpu.mem.wr8(a + 0x1D9, if fallen { 8 } else { 0 });
            cpu.mem.wr8(a + 0x21B, 0);
            cpu.mem.wr16(NODES + s * 0x100 + 0x68, 0x7FFF);
        }
        let phase = cpu.mem.rd8(CTX + 0x279);
        // Earthquake's two clip holds: arm 1 waits out the wind-up's first
        // run (`+0x21B == 0xFF`, the `beq` to its busy exit at `0x801F6C78`)
        // and arm 3 the caster's node cursor reaching `0x160`; each walk
        // holds them for a field-keyed number of passes.
        quake_passes = if phase == quake_phase {
            quake_passes + 1
        } else {
            0
        };
        quake_phase = phase;
        if body.prot_entry == 935 && quake_passes < quake_hold {
            let ca = ACTOR + (CASTER * STRIDE) as u32;
            match phase {
                1 => cpu.mem.wr8(ca + 0x21B, 0xFF),
                3 => cpu.mem.wr16(NODES + CASTER as u32 * 0x100 + 0x68, 0x100),
                _ => {}
            }
        }
        // A victim settle (PROT 0939's arm 5, 0947's and 0948's arm 5) waits
        // out the victim's clip, for the same field-keyed passes: a standing
        // victim's still playing (`0x801F7440`), a fallen one's not yet the
        // fallen clip `8` and its prim word still lit (`0x801F7458`,
        // `0x801F7110..0x801F7120` on PROT 0948). Then on odd-keyed walks a
        // fallen victim settles faded off its clip, else down on clip `8`.
        let settle = body.arm(phase).and_then(|a| a.settle).filter(|s| {
            victim < 8 && matches!(s, t::ChainSettle::Victim | t::ChainSettle::VictimOrFaded)
        });
        // The wait counts from the pass whose countdown runs out (the arm's
        // gate ahead of its settle), or from the arm's first pass.
        let spent = cam::capture_countdown_va(action)
            .is_none_or(|va| cpu.mem.rd32(va) as i32 <= cam::SPEED_SCALAR);
        settle_run = match settle_run {
            Some(n) if spent && quake_passes > 0 => Some(n + 1),
            _ if spent => Some(0),
            _ => None,
        };
        let settle_passes = settle_run.unwrap_or(0);
        if let Some(kind) = settle
            && SETTLE_WALKED.contains(&body.prot_entry)
        {
            let va = ACTOR + victim as u32 * STRIDE as u32;
            let fallen = cpu.mem.rd16(va + 0x14C) == 0;
            let faded = kind == t::ChainSettle::VictimOrFaded && seed & 1 == 1;
            match (settle_passes < quake_hold, fallen) {
                (true, false) => cpu.mem.wr8(va + 0x1D9, 1),
                (true, true) => {
                    cpu.mem.wr8(va + 0x1D9, 0);
                    cpu.mem.wr32(va + 4, 0x2008_0200);
                }
                (false, true) if faded => {
                    cpu.mem.wr8(va + 0x1D9, 0);
                    cpu.mem.wr32(va + 4, 0);
                }
                _ => {}
            }
        }
        let (ctx0, rec0) = (cpu.mem.read(CTX, CTX_LEN), cpu.mem.read(ACTOR, 8 * STRIDE));
        let vi = usize::from(rec0[CASTER * STRIDE + 0x1DD].min(7));
        // The group aim, as the host measures it each pass; a row the body
        // has hidden whole has none (`target_group_aim`'s `None`, where the
        // routine itself traps on its divide).
        if victim >= 8 {
            st.target_aim = (cpu.call(AIM, &[victim as u32, OUT_CENTRE, OUT_EXTENT])
                == Stop::Returned)
                .then(|| GroupAim {
                    centroid_x: cpu.mem.rd16(OUT_CENTRE) as i16,
                    centroid_z: cpu.mem.rd16(OUT_CENTRE + 4) as i16,
                    extent: cpu.mem.rd16(OUT_EXTENT + 4) as i16,
                });
        }
        // The director runs on every pass, held or not, as the host's does.
        let yaw0 = st.yaw_base;
        // The caster's display trio `+0x3C..+0x40`, as the host feeds it
        // (`feed_capture_cam`).
        st.caster_body = Some({
            let h = |k: usize| {
                i16::from_le_bytes([rec0[CASTER * STRIDE + k], rec0[CASTER * STRIDE + k + 1]])
            };
            ModuleSeat {
                x: h(0x3C),
                y: h(0x3E),
                z: h(0x40),
                facing: h(0x46) as u16 & 0xFFF,
            }
        });
        // The caster's loop byte and node cursor, as `feed_capture_cam`
        // feeds them.
        st.caster_loops = Some(rec0[CASTER * STRIDE + 0x21B]);
        st.caster_frame = Some(cpu.mem.rd16(NODES + CASTER as u32 * 0x100 + 0x68));
        // PROT 0964's last Element Change roll, which the host keeps.
        if body.prot_entry == 964 {
            st.module_words[0] = i32::from(selector);
        }
        // The director sees the module's countdown word as retail's pass
        // opens on it, where the module has one; a director with none
        // reads a spent word, and the walk paces it on retail's phase.
        let countdown_word = cam::capture_countdown_va(action).map(|va| cpu.mem.rd32(va) as i32);
        hits.borrow_mut().clear();
        spawns.borrow_mut().clear();
        draws.borrow_mut().clear();
        refunds.borrow_mut().clear();
        let spore_count = cpu.mem.rd32(SPORE_GAS_COUNT);
        let heads0: Vec<[u8; 2]> = (0..3u32)
            .map(|i| {
                let r = CHAR_RECORDS + i * CHAR_STRIDE;
                [cpu.mem.rd8(r + 0x76F), cpu.mem.rd8(r + 0x77F)]
            })
            .collect();
        cpu.mem.wr8(CLUSTER_FLAG, 1);
        for w in RUMBLE {
            cpu.mem.wr32(w, RUMBLE_SENTINEL);
        }
        let stop = cpu.call(body.body, &[]);
        let c0 = format!(
            "PROT {:04} {action:#04x} step {step} {field:?} arm {phase:#x}",
            body.prot_entry
        );
        if stop != Stop::Returned {
            out.gaps.push(format!("{c0}: {stop:?}"));
            return Some(out);
        }
        // A split-hit body lands each hit's share itself (`SplitHits::landed`):
        // HP and the bar delta on the victim, hit by hit.
        if let Some(split) = t::split_hits_for(body.prot_entry, body.body)
            && !hits.borrow().is_empty()
            && victim < 8
        {
            let all = rolls.borrow().clone();
            let first = all.len() - hits.borrow().len();
            let mut hp = u16::from_le_bytes([rec0[vi * STRIDE + 0x14C], rec0[vi * STRIDE + 0x14D]]);
            let mut bar = i32::from_le_bytes(
                rec0[vi * STRIDE + 0x10..vi * STRIDE + 0x14]
                    .try_into()
                    .unwrap(),
            );
            let stoned = rec0[vi * STRIDE + 0x16E] & 4 != 0;
            for (k, &roll) in all[first..].iter().enumerate() {
                if hp == 0 || stoned {
                    break;
                }
                let n = (first + k + 1) as u8;
                let d = split.landed(n, roll as i32, hp);
                hp -= d as u16;
                bar += d;
            }
            let a = ACTOR + (vi * STRIDE) as u32;
            let got = (cpu.mem.rd16(a + 0x14C), cpu.mem.rd32(a + 0x10) as i32);
            if got != (hp, bar) {
                out.gaps.push(format!(
                    "PROT {:04} {action:#04x} {field:?} arm {phase:#x}: split hits retail {got:?} port {:?}",
                    body.prot_entry,
                    (hp, bar)
                ));
            }
            out.split_checked += 1;
        }
        // PROT 0939's hit pass past the landing: the Rot roll behind the
        // record's guards, the Auto heads a newly rotted limb clears, and the
        // readout flag hits 1..3 shut.
        if body.prot_entry == 939 && phase == 4 && cpu.mem.rd32(SPORE_GAS_COUNT) != spore_count {
            let hit = (spore_count + 1) as u8;
            let a = ACTOR + (vi * STRIDE) as u32;
            let old = u16::from_le_bytes([rec0[vi * STRIDE + 0x16E], rec0[vi * STRIDE + 0x16F]]);
            let hp0 = u16::from_le_bytes([rec0[vi * STRIDE + 0x14C], rec0[vi * STRIDE + 0x14D]]);
            let landed = hp0 != 0 && old & 4 == 0;
            let r = CHAR_RECORDS + vi as u32 * CHAR_STRIDE;
            let ability = cpu.mem.rd32(r + 0x6BC);
            let mut used = 0usize;
            let limb = if landed {
                t::spore_gas_rot_roll(hit, ability, || {
                    used += 1;
                    draws.borrow().first().copied().unwrap_or(0)
                })
            } else {
                None
            };
            let want = old | limb.map_or(0, |l| 0x08 << l);
            let heads = if want != old { [0, 0] } else { heads0[vi] };
            let got = (
                draws.borrow().len(),
                cpu.mem.rd16(a + 0x16E),
                [cpu.mem.rd8(r + 0x76F), cpu.mem.rd8(r + 0x77F)],
                cpu.mem.rd8(CLUSTER_FLAG) == 0,
            );
            let port = (
                used,
                want,
                heads,
                landed && hit < t::SPORE_GAS_HITS.holds_cluster_below,
            );
            if got != port {
                out.gaps.push(format!(
                    "{c0}: hit {hit} (draws, status, Auto heads, flag shut) retail {got:x?} port {port:x?}"
                ));
            }
            out.split_checked += 1;
        }
        let busy = cpu.regs.r[V0] != 0;
        let after = cpu.mem.rd8(CTX + 0x279);
        let retail_held = busy && after == phase;
        // The rumble stores (`cast_module_rumble`), every pass, held or
        // not, behind the Battles vibration word `0x800845C8`: the odd-seeded
        // walks run with the row off, which stores nothing.
        {
            use legaia_engine_cast::cast_module_rumble::{RumbleStore, module_rumble_due};
            let words = RUMBLE.map(|w| cpu.mem.rd32(w));
            let stored = (words[0] != RUMBLE_SENTINEL).then(|| {
                (
                    words[0] as i32,
                    words[1] as i32,
                    words[2] as i32,
                    words[3] as i32,
                )
            });
            // A row keyed on the body (PROT 0963's) or on the image.
            let due: Vec<_> = [body.body, cam::SINGLE_BODY]
                .into_iter()
                .flat_map(|b| {
                    module_rumble_due(
                        body.prot_entry,
                        b,
                        phase,
                        !retail_held,
                        countdown_word.unwrap_or(0),
                    )
                })
                .filter(|_| !vibration_off)
                .collect();
            let port = match due.as_slice() {
                [] => None,
                [m] => match m.store {
                    RumbleStore::Write(w) => Some((w.pulse, w.level, w.pulse_rate, w.level_rate)),
                    _ => stored,
                },
                _ => Some((-1, -1, -1, -1)),
            };
            out.rumbles += usize::from(stored.is_some());
            if stored != port {
                out.gaps
                    .push(format!("{c0}: rumble retail {stored:x?} port {port:x?}"));
            }
        }
        // The host's pass (`World::module_pass`): `step` one-vsync ticks of
        // the director, the countdown's rest `step - 1 - i`, stopping on the
        // tick it lets through.
        let mut ticks = Vec::new();
        if let Some(d) = director {
            st.countdown.0 = countdown_word.unwrap_or(0);
            st.victim_on_gate_clip = rec0[vi * STRIDE + 0x1D9] == rec0[vi * STRIDE + 0x1F2];
            let seats = ModuleCamSeats {
                caster: seat_of(&rec0, CASTER),
                victim: seat_of(&rec0, vi),
                action,
                depth_raw: 0x900,
                victim_radius: 0x80,
                caster_scale: 0x1000,
                caster_monster: formation,
                first_monster: formation,
                ..ModuleCamSeats::default()
            };
            for i in 0..step {
                st.countdown.1 = step - 1 - i;
                st.opens_pass = i == 0;
                let a = d(&mut st, phase, seats);
                let hold = a.hold;
                ticks.push(a);
                // A director with no word of retail's reads a spent one and
                // is paced on retail's phase: a pass retail holds holds all
                // its ticks.
                if !hold && (countdown_word.is_some() || !retail_held) {
                    break;
                }
            }
        }
        let dir = ticks.last().cloned();
        // The arm's declared settle on the victim as the pass opened: a
        // victim it calls unsettled holds retail, and a pass retail lets
        // through finds it settled.
        if let Some(kind) = settle {
            let o = vi * STRIDE;
            let hp = u16::from_le_bytes([rec0[o + 0x14C], rec0[o + 0x14D]]);
            let clip = rec0[o + 0x1D9];
            let lit = rec0[o + 4..o + 8] != [0; 4];
            let settled = match (hp, kind) {
                (0, t::ChainSettle::VictimOrFaded) => clip == t::SETTLE_DOWN_CLIP || !lit,
                (0, _) => clip == t::SETTLE_DOWN_CLIP,
                _ => clip == 0,
            };
            let retail_held = busy && after == phase;
            if !settled && !retail_held {
                out.gaps.push(format!(
                    "{c0}: the {kind:?} settle holds where retail passed"
                ));
            }
            out.settles_checked += usize::from(!settled);
        }
        if busy && after == phase {
            held += 1;
            if held > 5000 {
                out.gaps.push(format!("{c0}: retail holds 5000 passes"));
                return Some(out);
            }
            continue;
        }
        held = 0;
        out.path.push(phase);
        // The records the director seats at points it works out (the
        // charge records, the per-pass pairs), each against a part spawn
        // retail's pass made with the same record, point, angles and scale.
        // An arm that seats on the point its pass's whole drain leaves
        // (`spawns_at_pass_end`, PROT 0947's trail) is compared where the
        // director drained retail's own countdown word.
        if let Some(d) = dir
            .as_ref()
            .filter(|d| !d.spawns_at_pass_end || countdown_word.is_some())
        {
            let _ = d;
            let mut left = spawns.borrow().clone();
            // The host stages a held tick's calls once a pass: on the tick
            // that opens it, or on the one that closes it for a call that
            // reads the pass's drained state; the passing tick's own always
            // (`World::capture_stager_tick`).
            let n = ticks.len();
            let staged = ticks.iter().enumerate().filter(|&(k, d)| {
                if !d.hold {
                    return true;
                }
                if d.spawns_at_pass_end {
                    k + 1 == usize::from(step) && n == usize::from(step)
                } else {
                    k == 0
                }
            });
            for sp in staged
                .flat_map(|(_, d)| d.charge_spawns.iter().chain(&d.pass_spawns))
                .flatten()
            {
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
                        out.spawns_checked += 1;
                    }
                    None => out.gaps.push(format!(
                        "{c0}: spawn {r:#x} at {pos:?} {rot:?} x{scale:#x}: retail {:x?}",
                        left.iter().filter(|x| x.0 == r).collect::<Vec<_>>()
                    )),
                }
            }
        }
        let mut struck: Vec<usize> = hits.borrow().clone();
        out.hits += struck.len();

        // The fold lands on the cast's victim whichever seat a wrapper was
        // handed (PROT 0959 aims its own at seat 0), and PROT 0919's arm 7 is
        // the party row's heal.
        if !struck.is_empty() && victim < 8 {
            struck.push(vi);
        }
        if body.prot_entry == 919 && phase == 7 {
            struck.extend(0..3);
        }

        // The port's pass: the body, then the director's seat writes.
        let Some(arm) = body.arm(phase) else {
            out.gaps.push(format!("{c0}: the port names no such arm"));
            return Some(out);
        };
        let mut v: Vec<CastActorState> = (0..8)
            .map(|s| read_actor(&mut cpu.mem, ACTOR + (s * STRIDE) as u32))
            .collect();
        for (s, a) in v.iter_mut().enumerate() {
            *a = read_actor_from(&rec0, s, *a);
        }
        let mut cview = read_ctx(&mut cpu.mem, CTX);
        cview = read_ctx_from(&ctx0, cview);
        let si = usize::from(t::SUMMON_SEAT);
        // A group code names no seat: the host's victim view is an empty
        // slot's, which nothing reads back.
        let group = victim >= 8;
        let (mut cc, mut vv, mut ss) = (
            v[CASTER],
            if group {
                CastActorState::default()
            } else {
                v[vi]
            },
            v[si],
        );
        // Numb / Master Guard, accessory passives `0x1B` / `0x1C` of a party
        // seat's record word, as the host's victim view carries them.
        if !group && vi < 3 {
            let w = cpu.mem.rd32(CHAR_RECORDS + vi as u32 * CHAR_STRIDE + 0x6BC);
            vv.numb_guard = w & (1 << 0x1B | 1 << 0x1C) != 0;
        }
        let tick = t::run_chain_body(body, &mut cview, &mut cc, &mut vv, &mut ss, selector);
        let refunded: Vec<(u32, u32)> = refunds.borrow().clone();
        let port_refund: Vec<(u32, u32)> =
            tick.refund.map(|i| (u32::from(i), 1)).into_iter().collect();
        if refunded != port_refund {
            out.gaps.push(format!(
                "{c0}: refunds retail {refunded:x?} port {port_refund:x?}"
            ));
        }
        v[si] = ss;
        if !group {
            v[vi] = vv;
        }
        v[CASTER] = cc;
        let mut ctx = ctx0.clone();
        write_ctx(&mut ctx, &cview);
        let mut want = rec0.clone();
        for (s, a) in v.iter().enumerate() {
            write_actor(&mut want[s * STRIDE..(s + 1) * STRIDE], a);
        }
        let rec = |s: usize| s * STRIDE..(s + 1) * STRIDE;
        // The band's pre-pass stages the caster's literals.
        for s in arm.stages.iter().filter(|s| s.actor == ChainActor::Caster) {
            if let ChainClip::Literal(k) = s.clip
                && pre_pass.contains(&k)
            {
                let r = &mut want[rec(CASTER)];
                r[0x1DA] = k;
                match s.restage {
                    ChainRestage::Bump => r[0x1DC] = r[0x1DC].wrapping_add(1),
                    ChainRestage::One => r[0x1DC] = 1,
                    ChainRestage::Untouched => {}
                }
            }
        }
        if let Some(rate) = arm.hit_rate {
            let row: Vec<usize> = if arm.row_rate {
                (0..3).collect()
            } else {
                struck.clone()
            };
            for s in row {
                want[s * STRIDE + 0x21D] = rate;
            }
        }
        if let Some(rate) = t::party_rate_sweep(body.prot_entry, Some(body.body), phase) {
            for s in 0..3 {
                want[s * STRIDE + 0x21D] = rate;
            }
        }
        if let Some(rate) = t::live_seat_rate_sweep(body.prot_entry, phase) {
            for s in 0..usize::from(t::LIVE_SEAT_SWEEP_ROWS) {
                let hp = u16::from_le_bytes([want[s * STRIDE + 0x14C], want[s * STRIDE + 0x14D]]);
                if hp > 0 {
                    want[s * STRIDE + 0x21D] = rate;
                }
            }
        }
        if let Some(level) = dir.as_ref().and_then(|d| d.parts_level)
            && body.prot_entry == 956
        {
            let got: Vec<u16> = POOLED
                .iter()
                .map(|&(_, p)| cpu.mem.rd16(p + 0x78))
                .collect();
            if got.iter().any(|&g| g != level) {
                out.gaps.push(format!(
                    "{c0}: pooled parts' level retail {got:x?} port {level:#x}"
                ));
            }
        }
        if let Some(last) = dir {
            if last.hold {
                out.gaps
                    .push(format!("{c0}: the director holds where retail passed"));
            }
            // Retail's passing pass leaves the word the director's own
            // drain and re-arm leave.
            if let Some(va) = cam::capture_countdown_va(action) {
                let word = cpu.mem.rd32(va) as i32;
                if word != st.countdown.0 {
                    out.gaps.push(format!(
                        "{c0}: countdown retail {word} port {}",
                        st.countdown.0
                    ));
                }
            }
        }
        for a in &ticks {
            if st.yaw_base != yaw0 {
                ctx[0x6DA..0x6DC].copy_from_slice(&(st.yaw_base as u16).to_le_bytes());
            }
            if let Some(b) = a.ctx_278 {
                ctx[0x278] = b;
            }
            apply_seat_write(&mut want[rec(CASTER)], &a.caster_write);
            if !group {
                apply_seat_write(&mut want[rec(vi)], &a.victim_write);
            }
            let hp = |w: &[u8], s: usize| {
                u16::from_le_bytes([w[s * STRIDE + 0x14C], w[s * STRIDE + 0x14D]])
            };
            for s in 0..3 {
                if !a.party_write.is_empty()
                    && (!a.party_live_only || hp(&want, s) > 0)
                    && !(a.party_skip_victim && s == vi)
                {
                    apply_seat_write(&mut want[rec(s)], &a.party_write);
                }
            }
            for s in 0..8 {
                let live = hp(&want, s) > 0;
                if !a.others_write.is_empty()
                    && ((s < 3 && (!a.others_live_only || live)) || (s >= 3 && live))
                {
                    apply_seat_write(&mut want[rec(s)], &a.others_write);
                }
            }
        }
        // The finishing arm's store into the yaw counter (`capture_exit_yaw_base`).
        if !busy && let Some(y) = cam::capture_exit_yaw_base(body.prot_entry, key) {
            ctx[0x6DA..0x6DC].copy_from_slice(&(y as u16).to_le_bytes());
        }
        if (tick.step == t::CastTickStep::Done) == busy {
            out.gaps
                .push(format!("{c0}: port {:?}, retail busy {busy}", tick.step));
        }
        if busy && cview.phase != after {
            out.gaps.push(format!(
                "{c0}: port leaves to {:#x}, retail {after:#x}",
                cview.phase
            ));
        }
        let got = cpu.mem.read(ACTOR, 8 * STRIDE);
        for (i, (g, w)) in got.iter().zip(&want).enumerate() {
            let (s, off) = (i / STRIDE, i % STRIDE);
            if g == w || owner(off, struck.contains(&s)).is_some() {
                continue;
            }
            let seat_kind = if s < 3 { 0 } else { s };
            if KNOWN
                .iter()
                .any(|k| (k.0, k.1, k.2, k.3) == (body.prot_entry, phase, seat_kind, off))
            {
                out.known += 1;
                continue;
            }
            out.gaps.push(format!(
                "{c0}: seat {s} +{off:#05x} retail {g:#04x} port {w:#04x} (was {:#04x})",
                rec0[i]
            ));
        }
        let got = cpu.mem.read(CTX, CTX_LEN);
        for (i, (g, w)) in got.iter().zip(&ctx).enumerate() {
            if g != w && i != 0x279 && ctx_owner(body.prot_entry, i).is_none() {
                out.gaps.push(format!(
                    "{c0}: ctx +{i:#05x} retail {g:#04x} port {w:#04x} (was {:#04x})",
                    ctx0[i]
                ));
            }
        }
        if !busy {
            return Some(out);
        }
    }
    out.gaps.push("no Done in 40000 passes".into());
    Some(out)
}

/// The view of seat `s` as the pass opened.
fn read_actor_from(rec: &[u8], s: usize, _now: CastActorState) -> CastActorState {
    let mut m = psx_retail_exec::Memory::default();
    m.load(ACTOR, &rec[s * STRIDE..(s + 1) * STRIDE]);
    read_actor(&mut m, ACTOR)
}

/// The context view as the pass opened.
fn read_ctx_from(ctx: &[u8], _now: t::CastModuleCtx) -> t::CastModuleCtx {
    let mut m = psx_retail_exec::Memory::default();
    m.load(CTX, ctx);
    read_ctx(&mut m, CTX)
}

/// The walks a body takes: two at a standing party seat and one at a party
/// seat beside a fallen one; a body whose spell record's target shape (`+2`)
/// takes a whole side - Earthquake `0x4A` and Hyper Crush `0x4B` (`0x64`),
/// Power Up `0xAA`, Neo Star Slash `0xA6`, Dead End Crisis `0xB4` (and `0xA1`,
/// `0x64`), Genocidal Cannon `0xB3` and Element Strike `0xB0..0xB2` (`0x66`),
/// which the monster picker turns into the party
/// code `8` (`0x801EA934`) - also at the party row as a group, whole and with
/// a fallen and a stoned seat; and PROT 0935, whose sweep and settle take the
/// row its target code names ([`ChainSettle::TargetRow`](t::ChainSettle)), at
/// the monster row and at a monster seat, with a fallen and a stoned monster.
/// The rest carry `0x44`, one opponent, which the picker turns into a party
/// seat (`rand() % party_count`, `0x801EA8CC..0x801EA918`): no group code
/// reaches them.
fn fields(entry: u32) -> Vec<(u64, Field)> {
    let f = |target, dead, stoned| Field {
        target,
        dead,
        stoned,
        ..Field::default()
    };
    let mut out = vec![
        (0x4348_4149, Field::default()),
        (0x5449_434B, Field::default()),
        (0x5345_4154, f(Some(2), 0b001, 0)),
        // The target on less HP than the hit: it dies under it, and a settle
        // that waits on the victim waits out its fallen clip.
        (
            0x4B49_4C4C,
            Field {
                low: 0b100,
                ..f(Some(2), 0, 0)
            },
        ),
    ];
    // PROT 0939's four hits on a victim already down (`0x801F7094`), Stoned
    // (`0x801F70A8`), on HP no hit downs (the counts 3 and 4, `0x801F7190` /
    // `0x801F7198`, and arm 5's wait on a standing victim's clip), with every
    // limb already rotted (`0x801F7348`), and behind each Rot guard
    // (`0x801F71F0` / `0x801F71FC` and their hit-2 and hit-3 twins).
    if entry == 939 {
        let high = |seed, rotted, ability| {
            (
                seed,
                Field {
                    high: 0b100,
                    rotted,
                    ability,
                    ..f(Some(2), 0, 0)
                },
            )
        };
        out.push((0x444F_574E, f(Some(2), 0b100, 0)));
        out.push((0x5354_4F4E, f(Some(2), 0, 0b100)));
        out.push(high(0x4849_4748, 0, 0));
        out.push(high(0x4849_4749, 0, 0));
        out.push(high(0x524F_5454, 0b100, 0));
        out.push(high(0x524F_5455, 0b100, 0));
        out.push(high(0x524F_5456, 0b100, 0));
        out.push(high(0x4755_4131, 0, 0x0100_0000));
        out.push(high(0x4755_4132, 0, 0x1000_0000));
    }
    if [935, 936, 942, 960, 961, 963, 964].contains(&entry) {
        out.push((0x4752_5031, f(Some(8), 0b010, 0b100)));
        out.push((0x4752_5032, f(Some(8), 0, 0)));
    }
    // PROT 0956's mark on a victim behind Numb Guard and Master Guard
    // (`0x801F6F8C` / `0x801F6F98`), and its turn steal on a victim with an
    // Item action queued and its turn to come (`0x801F705C`, `0x801F706C`
    // and the refund), its turn spent, and another action's turn to come
    // (`0x801F7088`).
    // PROT 0947's and 0948's monster-row walks past a fallen monster
    // (`0x801F6C70` / `0x801F6C20`, `0x801F717C`), and their arm 5's settle on a
    // victim already down.
    if [947, 948].contains(&entry) {
        out.push((0x4D44_4544, f(Some(2), 0b0001_0000, 0)));
        out.push((0x5644_4544, f(Some(2), 0b100, 0)));
        out.push((0x5644_4545, f(Some(2), 0b100, 0)));
    }
    if entry == 956 {
        let at = |seed, ability, queued| {
            (
                seed,
                Field {
                    ability,
                    queued,
                    ..f(Some(2), 0, 0)
                },
            )
        };
        out.push(at(0x4E55_4D42, 1 << 0x1B, None));
        out.push(at(0x4D41_5354, 1 << 0x1C, None));
        out.push(at(0x4954_454D, 0, Some((1, 0x40))));
        out.push(at(0x5350_454E, 0, Some((1, 0))));
        out.push(at(0x4F54_4852, 0, Some((2, 0x40))));
        out.push(at(0x4755_4152, 1 << 0x1B, Some((1, 0x40))));
    }
    if entry == 935 {
        out.push((0x4752_5039, f(Some(9), 0b0010_0000, 0b0001_0000)));
        out.push((0x4D4F_4E34, f(Some(4), 0b0010_0000, 0)));
    }
    out
}

#[test]
fn the_chain_bodies_walk_as_retail_does() {
    differential!(
        prot(919, SPOON),
        prot(935, EARTHQUAKE),
        prot(936, HYPER_CRUSH),
        prot(937, HYPER_LIGHTNING),
        prot(939, SPORE_GAS),
        prot(942, POWER_UP_AA),
        prot(947, V_WINDHASH),
        prot(948, CROSS_BEAM),
        prot(956, PARALYZING_WAVE),
        prot(959, MEGATON_PRESS),
        prot(960, NEO_STAR_SLASH),
        prot(961, DEAD_END_CRISIS),
        prot(963, GENOCIDAL_CANNON),
        prot(964, ELEMENT_STRIKE)
    );
    if disc().is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let mut failures = Vec::new();
    let (mut casts, mut arms, mut hits, mut known) = (0, 0, 0, 0);
    let mut split = 0usize;
    let mut spawns_checked = 0usize;
    let mut settles_checked = 0usize;
    let mut rumbles = 0usize;
    for body in t::chain_bodies() {
        // PROT 0961 rolls its sweep in the evolved Cort's formation only; PROT
        // 0964 forks on the last Element Change roll.
        // PROT 0935 also runs with Van Saryu (`0x89`) first in the formation,
        // whose records stand higher and whose quake window starts sooner
        // (`0x801F6D80`, `0x801F7068`).
        let formations: &[u8] = match body.prot_entry {
            961 => &[0xB5],
            935 => &[0x2A, 0x89],
            _ => &[0x2A],
        };
        let selectors: &[u8] = if body.prot_entry == 964 {
            &[0, 1, 2]
        } else {
            &[0]
        };
        for (&action, &formation) in body
            .action_ids
            .iter()
            .flat_map(|a| formations.iter().map(move |f| (a, f)))
        {
            for &sel in selectors {
                for ((seed, field), step) in fields(body.prot_entry)
                    .into_iter()
                    .flat_map(|f| (1..=4u8).map(move |s| (f, s)))
                {
                    let run =
                        run_body(body, action, formation, sel, seed, field, step).expect("disc");
                    casts += 1;
                    arms += run.path.len();
                    hits += run.hits;
                    known += run.known;
                    split += run.split_checked;
                    spawns_checked += run.spawns_checked;
                    settles_checked += run.settles_checked;
                    rumbles += run.rumbles;
                    let terminal = run.path.last().and_then(|&p| body.arm(p));
                    if !terminal.is_some_and(|a| a.exit == t::ChainExit::Finish) {
                        failures.push(format!(
                            "PROT {:04} {action:#04x}: path {:x?} ends off the terminal arm",
                            body.prot_entry, run.path
                        ));
                    }
                    failures.extend(run.gaps);
                }
            }
        }
    }
    eprintln!("{casts} casts, {arms} arms left, {hits} wrapper hits, {known} known gaps hit");
    assert!(hits > 20, "{hits} hits");
    assert!(split >= 4, "{split} split-hit passes compared");
    eprintln!("{spawns_checked} director spawns matched, {settles_checked} settle holds");
    assert!(rumbles >= 100, "{rumbles} rumble stores compared");
    assert!(
        settles_checked >= 20,
        "{settles_checked} settle holds compared"
    );
    assert!(
        spawns_checked >= 900,
        "{spawns_checked} director spawns matched"
    );
    failures.dedup();
    let n = failures.len();
    failures.truncate(200);
    assert!(n == 0, "{n} mismatches:\n{}", failures.join("\n"));
}

const SPOON: u32 = 0x801F_69D8;
const EARTHQUAKE: u32 = 0x801F_69D8;
const HYPER_CRUSH: u32 = 0x801F_69D8;
const HYPER_LIGHTNING: u32 = 0x801F_69D8;
const SPORE_GAS: u32 = 0x801F_69D8;
const POWER_UP_AA: u32 = 0x801F_69F4;
const V_WINDHASH: u32 = 0x801F_69F0;
const CROSS_BEAM: u32 = 0x801F_69F0;
const PARALYZING_WAVE: u32 = 0x801F_69D8;
const MEGATON_PRESS: u32 = 0x801F_69F0;
const NEO_STAR_SLASH: u32 = 0x801F_69D8;
const DEAD_END_CRISIS: u32 = 0x801F_69D8;
const GENOCIDAL_CANNON: u32 = 0x801F_6A20;
const ELEMENT_STRIKE: u32 = 0x801F_69D8;

/// A summon creature's tick body, by its port.
type SummonTick =
    fn(&mut t::CastModuleCtx, &mut CastActorState, &mut CastActorState) -> t::CastTickStep;

/// Walk one summon tick body off its image: the arms it left, each as
/// `(phase, phase after, busy)`, and whether it reached its Done.
/// Each arm a walk left: `(phase, phase after, busy, the stage byte
/// ctx[+0x278] after it)`.
type Walk = Vec<(u8, u8, bool, u8)>;

fn summon_walk(entry: u32, va: u32, action: u8, step: u8) -> Option<(Walk, bool)> {
    let disc = disc()?;
    let mut cpu = scus_cpu()?;
    cpu.mem.load(OVERLAY_BASE, &disc.read_entry(898).ok()?);
    let module = disc.read_entry(entry as usize).ok()?;
    cpu.mem.load(BASE, &module);
    cpu.mem.wr32(CTX_PTR, CTX);
    let stream = Rc::new(std::cell::Cell::new(0x1234_5679u32));
    for target in jal_targets(&module) {
        if target == RAND {
            let stream = stream.clone();
            cpu.hook(RAND, move |r, _| {
                let n = stream.get().wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
                stream.set(n);
                r.r[V0] = (n >> 16) & 0x7FFF;
            });
        } else if WRAPPERS.contains(&target) {
            cpu.stub(target, ROLL);
        } else if target == SPAWN_PART && entry == 922 {
            // PROT 0922 arm 18 keeps the handles of the 23 crystals it
            // spawns and arm 19 moves them: each spawn gets an object.
            let next = Rc::new(std::cell::Cell::new(PARTS));
            cpu.hook(target, move |r, m| {
                let at = next.get();
                next.set(at + 0x80);
                for o in (0..0x80).step_by(4) {
                    m.wr32(at + o, 0);
                }
                r.r[V0] = at;
            });
        } else {
            cpu.stub(target, 0);
        }
    }
    cpu.mem.wr32(0x8007_B81C, 0x8007_0A2C);
    cpu.mem.wr32(0x8007_B7F8, 0x8007_0A2C + 0x800);
    for b in 0..CTX_LEN as u32 {
        cpu.mem.wr8(CTX + b, 0);
    }
    cpu.mem.wr8(CTX, 3);
    cpu.mem.wr8(CTX + 1, 3);
    for s in 0..8u32 {
        let a = ACTOR + s * STRIDE as u32;
        cpu.mem.wr32(ACTORS + s * 4, a);
        for o in (0..STRIDE as u32).step_by(2) {
            cpu.mem.wr16(a + o, 0);
        }
        cpu.mem.wr16(a + 0x14C, 300);
        cpu.mem.wr8(a + 0x21D, 8);
        cpu.mem.wr8(a + 0x1DF, action);
        cpu.mem.wr8(a + 0x1DD, 4);
        let node = NODES + s * 0x100;
        cpu.mem.wr32(a + 0x22C, node);
        cpu.mem.wr16(node + 0x72, 0x1000);
    }
    for i in 0..5 {
        cpu.mem.wr32(RECORDS + i * 4, MONSTER_RECORD);
    }
    cpu.mem.wr8(SCALAR, cam::SPEED_SCALAR as u8);
    cpu.mem.wr8(STEP, step);
    let mut path = Vec::new();
    let mut held = 0;
    for _ in 0..40_000 {
        // An arm that plays its clip backward (PROT 0922 arm 8 rewinds the
        // cursor to the start) waits on a cursor near zero: a long hold
        // gets one pass with every cursor at the start.
        let cursor = if held > 64 { 0 } else { 0x7FFF };
        for s in 0..8u32 {
            cpu.mem.wr8(ACTOR + s * STRIDE as u32 + 0x1D9, 0);
            cpu.mem.wr16(NODES + s * 0x100 + 0x68, cursor);
        }
        let phase = cpu.mem.rd8(CTX + 0x279);
        if cpu.call(va, &[]) != Stop::Returned {
            return Some((path, false));
        }
        let busy = cpu.regs.r[V0] != 0;
        let after = cpu.mem.rd8(CTX + 0x279);
        if busy && after == phase {
            held += 1;
            continue;
        }
        held = 0;
        path.push((phase, after, busy, cpu.mem.rd8(CTX + 0x278)));
        if !busy {
            return Some((path, true));
        }
    }
    Some((path, false))
}

/// The summon tick bodies the walk holds to their ports, as `(entry, body,
/// action, the port, the fewest arms the walk must leave)`. PROT 0922's and
/// 0958's walks stop where an arm reaches a part handle the stubbed seat call
/// never made; the arms before it are compared.
const SUMMON_TICKS: [(u32, u32, u8, SummonTick, usize); 7] = [
    (
        949,
        WATER_CRYSTALS,
        0x9E,
        |c, _, v| t::water_crystals_tick(c, v, None),
        6,
    ),
    (925, SPIKEFISH, 0x97, |c, k, _| t::spikefish_tick(c, k), 10),
    (
        924,
        ULTIMATE_RAVE,
        0x96,
        |c, k, v| t::ultimate_rave_tick(c, k, v, false),
        12,
    ),
    (922, PUERA, 0x94, |c, _, v| t::puera_tick(c, v, None), 8),
    (
        927,
        JUGGERNAUT,
        0x99,
        |c, _, v| t::juggernaut_tick(c, v, None),
        20,
    ),
    (
        958,
        BLAZING_SLASH,
        0x79,
        |c, _, v| t::blazing_slash_tick(c, v, None),
        24,
    ),
    (
        918,
        KEMARO,
        0x95,
        |c, _, v| t::kemaro_tick(c, v, None).unwrap_or(t::CastTickStep::Done),
        21,
    ),
];

#[test]
fn the_summon_tick_bodies_walk_their_phases_as_retail_does() {
    differential!(
        prot(949, WATER_CRYSTALS),
        prot(925, SPIKEFISH),
        prot(924, ULTIMATE_RAVE),
        prot(922, PUERA),
        prot(927, JUGGERNAUT),
        prot(958, BLAZING_SLASH),
        prot(918, KEMARO)
    );
    if disc().is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    }
    let mut failures = Vec::new();
    // At frame steps 1 to 4 (`0x1F800393`): a console's battle CD timing
    // runs frames above one vsync, and the bodies' arms do not read it.
    let runs = [1u8, 2, 3, 4]
        .into_iter()
        .flat_map(|step| SUMMON_TICKS.map(|t| (t, step)));
    for ((entry, va, action, port, fewest), step) in runs {
        let (path, done) = summon_walk(entry, va, action, step).expect("disc");
        eprintln!(
            "PROT {entry:04} step {step}: {:x?}{}",
            path.iter().map(|p| p.0).collect::<Vec<_>>(),
            if done { " Done" } else { "" }
        );
        if path.len() < fewest {
            failures.push(format!(
                "PROT {entry:04} step {step}: the walk left {} arms",
                path.len()
            ));
        }
        // PROT 0922's port carries the stage byte its arms 0, 11, 12 and 23
        // store; its walk holds it arm by arm. (The wash block beside it,
        // `ctx[+0x27A..]`, is the presentation decoder's, not the tick's.)
        let mut stage = t::CastModuleCtx::default();
        for &(p, a, busy, bytes) in &path {
            let mut ctx = t::CastModuleCtx {
                phase: p,
                ..Default::default()
            };
            let (mut k, mut v) = (CastActorState::default(), CastActorState::default());
            let tick = port(&mut ctx, &mut k, &mut v);
            if (tick == t::CastTickStep::Done) == busy || (busy && ctx.phase != a) {
                failures.push(format!(
                    "PROT {entry:04} step {step} arm {p:#x}: retail -> {a:#x} busy {busy}, port -> {:#x} {tick:?}",
                    ctx.phase
                ));
            }
            if entry == 922 {
                stage.phase = p;
                port(&mut stage, &mut k, &mut v);
                if stage.ctx_278 != bytes {
                    failures.push(format!(
                        "PROT 0922 arm {p:#x}: stage byte retail {bytes}, port {}",
                        stage.ctx_278
                    ));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The part spawn `FUN_80021B04` and the pool the walk hands its objects
/// from.
const SPAWN_PART: u32 = 0x8002_1B04;
/// The part pool's wrapper round [`SPAWN_PART`], on the same arguments.
const SPAWN_POOLED: u32 = 0x8005_0ED4;
const PARTS: u32 = 0x8016_0000;
const WATER_CRYSTALS: u32 = 0x801F_6A10;
const SPIKEFISH: u32 = 0x801F_6A00;
const ULTIMATE_RAVE: u32 = 0x801F_6A18;
const PUERA: u32 = 0x801F_6A3C;
const JUGGERNAUT: u32 = 0x801F_6A84;
const BLAZING_SLASH: u32 = 0x801F_6DD8;
const KEMARO: u32 = 0x801F_6C70;

/// The character records (`0x80084140 + (id - 1) * 0x414`): the spell list
/// `+0x705`, its levels `+0x729` and the spell-XP words `+0x5D0`.
const CHAR_RECORDS: u32 = 0x8008_4140;
const CHAR_STRIDE: u32 = 0x414;
/// The cure selector `FUN_801F3D3C` latches (`0x801F6960`).
const CURE_SELECTOR: u32 = 0x801F_6960;
/// The special-battle word that gates every spell-XP bump (`0x8007BAC0`).
const SPECIAL_BATTLE: u32 = 0x8007_BAC0;
/// PROT 0919's arm countdown word.
const SPOON_COUNTDOWN: u32 = 0x801F_8E5C;

/// A machine with PROT 0898 and `entry`'s image placed, every call out of
/// the image stubbed.
fn heal_rig(entry: u32) -> Option<(psx_retail_exec::Cpu, Vec<u8>)> {
    let disc = disc()?;
    let mut cpu = scus_cpu()?;
    cpu.mem.load(OVERLAY_BASE, &disc.read_entry(898).ok()?);
    let module = disc.read_entry(entry as usize).ok()?;
    for target in jal_targets(&module) {
        cpu.stub(target, 0);
    }
    cpu.mem.wr32(CTX_PTR, CTX);
    cpu.mem.wr32(0x8007_B81C, 0x8007_0A2C);
    cpu.mem.wr32(0x8007_B7F8, 0x8007_0A2C + 0x800);
    Some((cpu, module))
}

/// One heal-arm case: the image fresh, the context on `phase` with a party
/// caster casting `action` at `target`, eight seats of random HP (fallen,
/// whole or hurt), max HP, status words (Stone on one in four) and AP, the
/// caster's record listing the spell at a random place (or nowhere) with a
/// random level beside every entry, the cure selector and the special-battle
/// word. Returns the caster's record and the list index the walk finds
/// (`0x20` past the list when it is missing).
struct HealCase {
    rec: u32,
    idx: u32,
    level: u8,
    selector: u8,
    special: bool,
}

fn seed_heal_case(
    cpu: &mut psx_retail_exec::Cpu,
    module: &[u8],
    rng: &mut Rng,
    phase: u8,
    action: u8,
    target: u8,
) -> HealCase {
    cpu.mem.load(BASE, module);
    for b in 0..CTX_LEN as u32 {
        cpu.mem.wr8(CTX + b, 0);
    }
    let caster = rng.below(3);
    cpu.mem.wr8(CTX, 3);
    cpu.mem.wr8(CTX + 1, 3);
    cpu.mem.wr8(CTX + 0x13, caster as u8);
    cpu.mem.wr8(CTX + 0x279, phase);
    for s in 0..8u32 {
        let a = ACTOR + s * STRIDE as u32;
        cpu.mem.wr32(ACTORS + s * 4, a);
        for o in (0..STRIDE as u32).step_by(4) {
            cpu.mem.wr32(a + o, 0);
        }
        let max = 200 + rng.below(3000) as u16;
        let hp = match rng.below(5) {
            0 => 0,
            1 => max,
            _ => 1 + rng.below(u32::from(max)) as u16,
        };
        cpu.mem.wr16(a + 0x14C, hp);
        cpu.mem.wr16(a + 0x14E, max);
        let mut status = rng.next() as u16 & 0x0FFF;
        if rng.below(4) != 0 {
            status &= !4;
        }
        cpu.mem.wr16(a + 0x16E, status);
        cpu.mem.wr16(a + 0x170, rng.below(101) as u16);
        cpu.mem.wr8(a + 0x1DD, target);
        cpu.mem.wr8(a + 0x1DF, action);
        cpu.mem.wr8(a + 0x21D, 8);
        let node = NODES + s * 0x100;
        cpu.mem.wr32(a + 0x22C, node);
        cpu.mem.wr16(node + 0x72, 0x1000);
    }
    for i in 0..3 {
        cpu.mem.wr8(ROSTER + i, 1 + i as u8);
    }
    let rec = CHAR_RECORDS + caster * CHAR_STRIDE;
    let at = rng.below(0x22);
    for i in 0..0x24u32 {
        cpu.mem
            .wr8(rec + 0x705 + i, if i == at { action } else { 0x30 });
        cpu.mem.wr8(rec + 0x729 + i, rng.below(10) as u8);
        cpu.mem.wr32(rec + 0x5D0 + 4 * i, 0);
    }
    let selector = [1u8, 2, 3, 4, 5, 10][rng.below(6) as usize];
    cpu.mem.wr32(CURE_SELECTOR, u32::from(selector));
    let special = rng.below(4) == 0;
    cpu.mem.wr32(SPECIAL_BATTLE, u32::from(special));
    cpu.mem.wr8(SCALAR, cam::SPEED_SCALAR as u8);
    // Any frame step: the heal arm makes its stores once, on its pass.
    cpu.mem.wr8(STEP, 1 + rng.below(4) as u8);
    // The list walk stops on the first match or at 0x20 (`sltiu 0x20`).
    let idx = at.min(0x20);
    HealCase {
        rec,
        idx,
        level: cpu.mem.rd8(rec + 0x729 + idx),
        selector,
        special,
    }
}

/// Run the arm on `phase` until it leaves, its countdown word drained each
/// pass.
fn leave_arm(cpu: &mut psx_retail_exec::Cpu, phase: u8, countdown: u32, what: &str) {
    for _ in 0..200 {
        cpu.mem.wr32(countdown, 0);
        assert_eq!(cpu.call(BASE, &[]), Stop::Returned, "{what}");
        if cpu.mem.rd8(CTX + 0x279) != phase {
            return;
        }
    }
    panic!("{what}: arm {phase} never left");
}

fn actors(cpu: &mut psx_retail_exec::Cpu) -> Vec<CastActorState> {
    (0..8)
        .map(|s| read_actor(&mut cpu.mem, ACTOR + s * STRIDE as u32))
        .collect()
}

/// PROT 0919's arm 7 (`0x801F7F48..0x801F83D8`), Spoon's party heal, against
/// its ports over [`seed_heal_case`]'s cases: each party seat's HP is held to
/// `spoon_heal_amount` under the missing-HP cap on a standing, un-stoned
/// seat; its status word and AP to `spoon_cure_sweep`; and the caster's
/// spell-XP word to the sum of `module_heal_xp_gain` over the standing seats.
#[test]
fn spoons_heal_arm_heals_cures_and_credits_as_the_ports_do() {
    differential!(prot(919, SPOON));
    use legaia_engine_battle::magic_xp::module_heal_xp_gain;
    use legaia_engine_battle_vm::battle_formulas::spoon_heal_amount;
    use legaia_engine_cast::seru_magic_ticks::{ORB_CLEANSE_MIN_LEVEL, spoon_cure_sweep};
    let Some((mut cpu, module)) = heal_rig(919) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x5350_4F4F);
    let mut seen = [0usize; 6];
    for case in 0..1500 {
        let c = seed_heal_case(&mut cpu, &module, &mut rng, 7, 0x91, 8);
        let before = actors(&mut cpu);
        leave_arm(&mut cpu, 7, SPOON_COUNTDOWN, &format!("case {case}"));
        let mut want = before.clone();
        let mut xp = 0u32;
        for (s, v) in want.iter_mut().take(3).enumerate() {
            if v.hp == 0 {
                continue;
            }
            let max = cpu.mem.rd16(ACTOR + s as u32 * STRIDE as u32 + 0x14E);
            if !c.special {
                xp += module_heal_xp_gain(0x91, c.level, v.hp, max);
            }
            if v.flags & 4 == 0 {
                v.hp += spoon_heal_amount(c.level).min(u32::from(max - v.hp)) as u16;
            }
        }
        let cleanse = (c.level >= ORB_CLEANSE_MIN_LEVEL).then_some(c.selector);
        spoon_cure_sweep(&mut want, cleanse);
        let ctx = format!(
            "case {case}: level {} selector {} special {}",
            c.level, c.selector, c.special
        );
        let got = actors(&mut cpu);
        for s in 0..3 {
            assert_eq!(
                (got[s].hp, got[s].flags, got[s].spirit_gauge),
                (want[s].hp, want[s].flags, want[s].spirit_gauge),
                "{ctx}: seat {s} (was {:?})",
                (before[s].hp, before[s].flags, before[s].spirit_gauge)
            );
        }
        assert_eq!(
            cpu.mem.rd32(c.rec + 0x5D0 + 4 * c.idx),
            xp,
            "{ctx}: the spell-XP word"
        );
        seen[usize::from(cleanse.unwrap_or(0).min(5))] += 1;
    }
    assert!(seen.iter().all(|&n| n > 50), "{seen:?}");
}

/// PROT 0905's countdown word (`cast_module_camera::VERA_COUNTDOWN_VA`).
const VERA_COUNTDOWN: u32 = 0x801F_8818;

/// PROT 0905's arm 9 (`0x801F7BB8..0x801F7F4C`), Vera's restore, against
/// `vera_tick` with the restore applied and `module_heal_xp_gain`: the target
/// a random party seat, fallen, stoned, hurt or whole, over
/// [`seed_heal_case`]'s levels, selectors and special-battle word. The target's
/// HP, HP-bar delta, status word and AP and the caster's spell-XP word are
/// compared.
#[test]
fn veras_restore_arm_heals_cures_and_credits_as_the_ports_do() {
    differential!(prot(905, VERA));
    use legaia_engine_battle::magic_xp::module_heal_xp_gain;
    use legaia_engine_cast::seru_magic_ticks::{SeruSeats, VeraRestore, vera_tick};
    let Some((mut cpu, module)) = heal_rig(905) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x5645_5241);
    let mut seen = [0usize; 4];
    for case in 0..1500 {
        let target = rng.below(3) as u8;
        let c = seed_heal_case(&mut cpu, &module, &mut rng, 9, 0x83, target);
        let before = actors(&mut cpu);
        let t = usize::from(target);
        let max = cpu
            .mem
            .rd16(ACTOR + u32::from(target) * STRIDE as u32 + 0x14E);
        leave_arm(&mut cpu, 9, VERA_COUNTDOWN, &format!("case {case}"));
        let mut want = before.clone();
        let mut ctx = t::CastModuleCtx {
            phase: 9,
            ..Default::default()
        };
        let caster = cpu.mem.rd8(CTX + 0x13);
        vera_tick(
            &mut ctx,
            &mut want,
            SeruSeats {
                caster,
                victim: target,
                summon: 7,
            },
            Some(VeraRestore {
                magic_level: c.level,
                max_hp: max,
                cure_tier: c.selector,
                apply_hp: true,
            }),
        );
        let xp = if c.special {
            0
        } else {
            module_heal_xp_gain(0x83, c.level, before[t].hp, max)
        };
        let got = actors(&mut cpu);
        let what = format!(
            "case {case}: level {} selector {} special {} target {:?}",
            c.level,
            c.selector,
            c.special,
            (before[t].hp, max, before[t].flags)
        );
        assert_eq!(
            (
                got[t].hp,
                got[t].hp_bar_delta,
                got[t].flags,
                got[t].spirit_gauge
            ),
            (
                want[t].hp,
                want[t].hp_bar_delta,
                want[t].flags,
                want[t].spirit_gauge
            ),
            "{what}"
        );
        assert_eq!(
            cpu.mem.rd32(c.rec + 0x5D0 + 4 * c.idx),
            xp,
            "{what}: the spell-XP word"
        );
        seen[usize::from(before[t].hp == 0) + 2 * usize::from(before[t].flags & 4 != 0)] += 1;
    }
    assert!(seen.iter().all(|&n| n > 20), "{seen:?}");
}

const VERA: u32 = 0x801F_69D8;

/// PROT 0908's latched victim seat (`lw s3,-0x7458` at `0x801F6A5C`).
const ZENOIR_VICTIM: u32 = 0x801F_8BA8;

/// PROT 0908's arm 12 shatter loop (`0x801F81C8..0x801F8264`), one pass,
/// against `zenoir_shatter_pass`: random monster rows (fallen or standing, the
/// colour word drained or not), the scripted-fight flag, the latched victim's
/// record `+0x20` byte, the formation's first id (a Berserker or not) and the
/// frame product. Every seat's shatter word `+0x1BA` is compared.
#[test]
fn zenoirs_shatter_pass_raises_the_seats_the_port_raises() {
    differential!(prot(908, ZENOIR));
    use legaia_engine_cast::seru_magic_ticks::{
        ZENOIR_SHATTER_ARM, ZenoirShatterGate, zenoir_shatter_pass,
    };
    let Some((mut cpu, module)) = heal_rig(908) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut rng = Rng(0x5A45_4E4F);
    let mut raised = [0usize; 2];
    for case in 0..3000 {
        seed_heal_case(&mut cpu, &module, &mut rng, ZENOIR_SHATTER_ARM, 0x88, 4);
        let victim = 3 + rng.below(4);
        cpu.mem.wr32(ZENOIR_VICTIM, victim);
        for s in 3..8u32 {
            let a = ACTOR + s * STRIDE as u32;
            if rng.below(3) == 0 {
                cpu.mem.wr32(a + 4, 0);
            } else {
                cpu.mem.wr32(a + 4, 0x2008_0200);
            }
            cpu.mem.wr16(a + 0x1BA, rng.below(0x400) as u16);
        }
        let scripted = rng.below(2) == 0;
        cpu.mem.wr8(CTX + 0x287, u8::from(scripted));
        cpu.mem.wr16(CTX + 0x6D8, 0x7000);
        let records: Vec<u8> = (0..4).map(|_| rng.below(2) as u8).collect();
        for (k, b) in records.iter().enumerate() {
            let r = MONSTER_RECORD + 0x40 * k as u32;
            cpu.mem.wr32(RECORDS + 4 * k as u32, r);
            cpu.mem.wr8(r + 0x20, *b);
        }
        let first = [0x4Du8, 0xAD, 0xAE, 0x10, 0x2A, 0x10][rng.below(6) as usize];
        cpu.mem.wr8(FORMATION, first);
        let (scalar, step) = (1 + rng.below(8) as u8, 1 + rng.below(4) as u8);
        cpu.mem.wr8(SCALAR, scalar);
        cpu.mem.wr8(STEP, step);
        let before = actors(&mut cpu);
        assert_eq!(cpu.call(BASE, &[]), Stop::Returned, "case {case}");
        let mut want = before.clone();
        let n = zenoir_shatter_pass(
            &mut want,
            ZenoirShatterGate {
                scripted,
                victim_record_20: records[(victim - 3) as usize],
                formation_first: first,
            },
            u16::from(scalar) * u16::from(step),
            |_, _| false,
        );
        let got = actors(&mut cpu);
        for s in 3..7 {
            assert_eq!(
                got[s].shatter, want[s].shatter,
                "case {case}: seat {s} scripted {scripted} record {records:?} victim {victim} first {first:#x} hp {} colour {:#x}",
                before[s].hp, before[s].present_04
            );
        }
        raised[usize::from(n > 0)] += 1;
    }
    assert!(raised.iter().all(|&n| n > 300), "{raised:?}");
}

const ZENOIR: u32 = 0x801F_69D8;

/// PROT 0907's verdict words: the kill roll's remainder and the resist flag.
const NIGHTO_KILL_WORD: u32 = 0x801F_8534;
const NIGHTO_RESIST_WORD: u32 = 0x801F_853C;

/// PROT 0907's arm 0 (`0x801F6B50..0x801F6F28`), Nighto's verdict, against
/// `nighto_outcome` and `NightoOutcome::arm0_credit`: a scripted `rand()`
/// stream, the caster's level for the spell, the caster's character index
/// (the third takes the extra throw), the scripted-fight flag and the
/// victim's record `+0x20` byte (the immune target draws no resist throw),
/// and the special-battle word. The draws retail takes, its kill and resist
/// words, the spell-XP word and the Monsters Defeated word (`+0x664`) are
/// compared.
#[test]
fn nightos_verdict_rolls_and_credits_as_the_port_does() {
    differential!(prot(907, NIGHTO));
    use legaia_engine_cast::seru_magic_ticks::{NightoRoll, nighto_outcome};
    let Some((mut cpu, module)) = heal_rig(907) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let stream = Rc::new(std::cell::Cell::new(1u32));
    let draws = Rc::new(RefCell::new(Vec::<u32>::new()));
    {
        let (stream, draws) = (stream.clone(), draws.clone());
        cpu.hook(RAND, move |r, _| {
            let n = stream.get().wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
            stream.set(n);
            r.r[V0] = (n >> 16) & 0x7FFF;
            draws.borrow_mut().push(r.r[V0]);
        });
    }
    let mut rng = Rng(0x4E49_4748);
    let mut seen = [0usize; 4];
    for case in 0..4000 {
        let victim = 3 + rng.below(4) as u8;
        let c = seed_heal_case(&mut cpu, &module, &mut rng, 0, 0x87, victim);
        // The roster's character ids, the caster's among them (1-based).
        let caster = u32::from(cpu.mem.rd8(CTX + 0x13));
        let character = 1 + rng.below(4) as u8;
        cpu.mem.wr8(ROSTER + caster, character);
        let rec = CHAR_RECORDS + u32::from(character - 1) * CHAR_STRIDE;
        if rec != c.rec {
            for i in 0..0x24u32 {
                let (id, lv) = (
                    cpu.mem.rd8(c.rec + 0x705 + i),
                    cpu.mem.rd8(c.rec + 0x729 + i),
                );
                cpu.mem.wr8(rec + 0x705 + i, id);
                cpu.mem.wr8(rec + 0x729 + i, lv);
                cpu.mem.wr32(rec + 0x5D0 + 4 * i, 0);
            }
        }
        cpu.mem.wr32(rec + 0x664, 0);
        let scripted = rng.below(2) == 0;
        cpu.mem.wr8(CTX + 0x287, u8::from(scripted));
        let immune_byte = rng.below(2) as u8;
        for k in 0..4u32 {
            let r = MONSTER_RECORD + 0x40 * k;
            cpu.mem.wr32(RECORDS + 4 * k, r);
            cpu.mem.wr8(r + 0x20, immune_byte);
        }
        stream.set(rng.next());
        draws.borrow_mut().clear();
        assert_eq!(cpu.call(NIGHTO, &[]), Stop::Returned, "case {case}");
        let d = draws.borrow().clone();
        let target_immune = scripted && immune_byte != 0;
        let mut k = 0;
        let mut next = || {
            k += 1;
            d.get(k - 1).copied()
        };
        let kill_roll = next().expect("the kill roll");
        let resist_roll = if target_immune {
            0
        } else {
            next().expect("the throw")
        };
        let extra_roll = (character == 3).then(|| next().expect("the extra throw"));
        let want = nighto_outcome(&NightoRoll {
            kill_roll,
            resist_roll,
            magic_level: c.level,
            target_immune,
            extra_roll,
        });
        let credit = want.arm0_credit();
        let what = format!(
            "case {case}: level {} character {character} scripted {scripted} immune byte {immune_byte} special {} draws {d:?}",
            c.level, c.special
        );
        assert_eq!(k, d.len(), "{what}: the draws");
        assert_eq!(
            (
                cpu.mem.rd32(NIGHTO_KILL_WORD) == 0,
                cpu.mem.rd32(NIGHTO_RESIST_WORD) != 0
            ),
            (want.deadly(), want.resisted()),
            "{what}: the verdict"
        );
        let xp = if c.special { 0 } else { credit.spell_xp };
        assert_eq!(
            cpu.mem.rd32(rec + 0x5D0 + 4 * c.idx),
            xp,
            "{what}: the spell-XP word"
        );
        assert_eq!(
            cpu.mem.rd32(rec + 0x664),
            u32::from(credit.defeated),
            "{what}: Monsters Defeated"
        );
        seen[usize::from(want.deadly()) + 2 * usize::from(want.resisted())] += 1;
    }
    assert!(seen.iter().all(|&n| n > 100), "{seen:?}");
}

const NIGHTO: u32 = 0x801F_69E8;

/// PROT 0916's resist word (`0x801F9010`): set where the cast does not kill.
const ALURU_RESIST_WORD: u32 = 0x801F_9010;

/// PROT 0916's arm 0 verdict (`0x801F6D44..0x801F6E88`), Aluru's instant kill,
/// against `aluru_kill_verdict`: a scripted `rand()` stream, the caster's
/// level for the spell, the scripted-fight flag, the victim's record `+0x20`
/// byte and Stone on the victim (an immune record skips the draw unless the
/// victim is stoned), and the special-battle word. The draws retail takes, its
/// resist word and the spell-XP word (`ALURU_KILL_XP` on a kill) are compared.
#[test]
fn alurus_kill_verdict_rolls_and_credits_as_the_port_does() {
    differential!(prot(916, ALURU));
    use legaia_engine_cast::cast_module_camera::{ALURU_KILL_XP, aluru_kill_verdict};
    let Some((mut cpu, module)) = heal_rig(916) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let stream = Rc::new(std::cell::Cell::new(1u32));
    let draws = Rc::new(RefCell::new(Vec::<u32>::new()));
    {
        let (stream, draws) = (stream.clone(), draws.clone());
        cpu.hook(RAND, move |r, _| {
            let n = stream.get().wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
            stream.set(n);
            r.r[V0] = (n >> 16) & 0x7FFF;
            draws.borrow_mut().push(r.r[V0]);
        });
    }
    let mut rng = Rng(0x414C_5552);
    let mut seen = [0usize; 3];
    for case in 0..4000 {
        let victim = 3 + rng.below(4) as u8;
        let c = seed_heal_case(&mut cpu, &module, &mut rng, 0, 0x8E, victim);
        let scripted = rng.below(2) == 0;
        cpu.mem.wr8(CTX + 0x287, u8::from(scripted));
        let wide = rng.below(2) as u8;
        for k in 0..4u32 {
            let r = MONSTER_RECORD + 0x40 * k;
            cpu.mem.wr32(RECORDS + 4 * k, r);
            cpu.mem.wr8(r + 0x20, wide);
        }
        let stoned = cpu
            .mem
            .rd16(ACTOR + u32::from(victim) * STRIDE as u32 + 0x16E)
            & 4
            != 0;
        stream.set(rng.next());
        draws.borrow_mut().clear();
        cpu.mem.wr32(ALURU_RESIST_WORD, 0xAAAA);
        assert_eq!(cpu.call(ALURU, &[]), Stop::Returned, "case {case}");
        let d = draws.borrow().clone();
        let skipped = scripted && wide != 0 && !stoned;
        let roll = (!skipped).then(|| d.first().copied().expect("the roll"));
        let kill = aluru_kill_verdict(roll, c.level);
        let what = format!(
            "case {case}: level {} scripted {scripted} wide {wide} stoned {stoned} special {} draws {d:?}",
            c.level, c.special
        );
        assert_eq!(d.len(), usize::from(!skipped), "{what}: the draws");
        assert_eq!(
            cpu.mem.rd32(ALURU_RESIST_WORD) == 0,
            kill,
            "{what}: the verdict"
        );
        let xp = if kill && !c.special { ALURU_KILL_XP } else { 0 };
        assert_eq!(
            cpu.mem.rd32(c.rec + 0x5D0 + 4 * c.idx),
            xp,
            "{what}: the spell-XP word"
        );
        seen[if skipped { 2 } else { usize::from(kill) }] += 1;
    }
    assert!(seen.iter().all(|&n| n > 100), "{seen:?}");
}

const ALURU: u32 = 0x801F_69F8;
