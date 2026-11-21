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
use psx_retail_exec::{A0, A2, Stop, V0};
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
}

/// Who a walk casts at and which seats stand: `target` a seat or a group code
/// (`None`: a random party seat), `dead` the seats at zero HP (party seats
/// play the fallen clip `8`, which a row's settle reads), `stoned` the seats
/// whose status word carries Stone (`+0x16E & 4`), which a row sweep skips.
#[derive(Debug, Clone, Copy, Default)]
struct Field {
    target: Option<u8>,
    dead: u8,
    stoned: u8,
}

fn run_body(
    body: &ChainBody,
    action: u8,
    formation: u8,
    selector: u8,
    seed: u64,
    field: Field,
) -> Option<Run> {
    let disc = disc()?;
    let mut cpu = scus_cpu()?;
    cpu.mem.load(OVERLAY_BASE, &disc.read_entry(898).ok()?);
    let module = disc.read_entry(body.prot_entry as usize).ok()?;
    cpu.mem.load(BASE, &module);
    cpu.mem.wr32(CTX_PTR, CTX);
    let stream = Rc::new(std::cell::Cell::new(seed as u32));
    let hits = Rc::new(RefCell::new(Vec::<usize>::new()));
    for target in jal_targets(&module) {
        if target == RAND {
            let stream = stream.clone();
            cpu.hook(RAND, move |r, _| {
                let n = stream.get().wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
                stream.set(n);
                r.r[V0] = (n >> 16) & 0x7FFF;
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
            cpu.hook(target, move |r, _| {
                hits.borrow_mut().push((r.r[A2] as usize).min(7));
                r.r[V0] = ROLL;
            });
        } else if target == AIM {
            // The group aim runs live: a group framing needs it whole.
        } else {
            cpu.stub(target, 0);
        }
    }
    // PROT 0948's beam packet builder, reached by a `jal` inside the image:
    // GPU packets only (`legaia_render_kernels::cast_beam`).
    if body.prot_entry == 948 {
        cpu.stub(CROSS_BEAM_PACKETS, 0);
    }
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
        } else {
            100 + rng.below(400) as u16
        };
        cpu.mem.wr16(a + 0x14C, hp);
        if field.stoned & (1 << s) != 0 {
            cpu.mem.wr16(a + 0x16E, 4);
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
    let caster_rec = ACTOR + (CASTER * STRIDE) as u32;
    cpu.mem.wr8(caster_rec + 0x1DD, victim as u8);
    cpu.mem.wr8(caster_rec + 0x1DE, 2);
    for i in 0..5 {
        cpu.mem.wr32(RECORDS + i * 4, MONSTER_RECORD);
        cpu.mem.wr8(FORMATION + i, formation);
    }
    for i in 0..3 {
        cpu.mem.wr8(ROSTER + i, 1 + i as u8);
    }
    cpu.mem.wr32(ELEMENT_ROLL, u32::from(selector));
    // One vsync a pass at the speed scalar, the clock the directors keep.
    cpu.mem.wr8(SCALAR, cam::SPEED_SCALAR as u8);
    cpu.mem.wr8(STEP, 1);

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
        frame_step: 1,
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
    };
    let mut held = 0;
    for _ in 0..40_000 {
        for s in 0..8u32 {
            let a = ACTOR + s * STRIDE as u32;
            let fallen = s < 3 && cpu.mem.rd16(a + 0x14C) == 0;
            cpu.mem.wr8(a + 0x1D9, if fallen { 8 } else { 0 });
            cpu.mem.wr8(a + 0x21B, 0);
            cpu.mem.wr16(NODES + s * 0x100 + 0x68, 0x7FFF);
        }
        let phase = cpu.mem.rd8(CTX + 0x279);
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
        let dir = director.map(|d| {
            st.countdown.0 = 0;
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
            d(&mut st, phase, seats)
        });
        hits.borrow_mut().clear();
        let stop = cpu.call(body.body, &[]);
        let c0 = format!(
            "PROT {:04} {action:#04x} {field:?} arm {phase:#x}",
            body.prot_entry
        );
        if stop != Stop::Returned {
            out.gaps.push(format!("{c0}: {stop:?}"));
            return Some(out);
        }
        let busy = cpu.regs.r[V0] != 0;
        let after = cpu.mem.rd8(CTX + 0x279);
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
        let tick = t::run_chain_body(body, &mut cview, &mut cc, &mut vv, &mut ss, selector);
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
        if let Some(a) = dir {
            if a.hold {
                out.gaps
                    .push(format!("{c0}: the director holds where retail passed"));
            }
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
    };
    let mut out = vec![
        (0x4348_4149, Field::default()),
        (0x5449_434B, Field::default()),
        (0x5345_4154, f(Some(2), 0b001, 0)),
    ];
    if [935, 936, 942, 960, 961, 963, 964].contains(&entry) {
        out.push((0x4752_5031, f(Some(8), 0b010, 0b100)));
        out.push((0x4752_5032, f(Some(8), 0, 0)));
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
    for body in t::chain_bodies() {
        // PROT 0961 rolls its sweep in the evolved Cort's formation only; PROT
        // 0964 forks on the last Element Change roll.
        let formation = if body.prot_entry == 961 { 0xB5 } else { 0x2A };
        let selectors: &[u8] = if body.prot_entry == 964 {
            &[0, 1, 2]
        } else {
            &[0]
        };
        for &action in body.action_ids {
            for &sel in selectors {
                for (seed, field) in fields(body.prot_entry) {
                    let run = run_body(body, action, formation, sel, seed, field).expect("disc");
                    casts += 1;
                    arms += run.path.len();
                    hits += run.hits;
                    known += run.known;
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

fn summon_walk(entry: u32, va: u32, action: u8) -> Option<(Walk, bool)> {
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
    cpu.mem.wr8(STEP, 1);
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
    for (entry, va, action, port, fewest) in SUMMON_TICKS {
        let (path, done) = summon_walk(entry, va, action).expect("disc");
        eprintln!(
            "PROT {entry:04}: {:x?}{}",
            path.iter().map(|p| p.0).collect::<Vec<_>>(),
            if done { " Done" } else { "" }
        );
        if path.len() < fewest {
            failures.push(format!(
                "PROT {entry:04}: the walk left {} arms",
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
            let step = port(&mut ctx, &mut k, &mut v);
            if (step == t::CastTickStep::Done) == busy || (busy && ctx.phase != a) {
                failures.push(format!(
                    "PROT {entry:04} arm {p:#x}: retail -> {a:#x} busy {busy}, port -> {:#x} {step:?}",
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
    cpu.mem.wr8(STEP, 1);
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
