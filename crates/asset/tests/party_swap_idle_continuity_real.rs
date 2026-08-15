//! Disc-gated joint-continuity oracle for the rebuilt battle idle
//! (`party_swap::winpose::rebuild_idle_stream`).
//!
//! Battle poses are FLAT: every part carries an absolute `R * v + T`
//! about the object origin, so nothing in the stream forces the skeleton
//! to hold together - a translation written to one channel alone moves
//! that part and nothing else. The measurement is therefore forward
//! kinematics against the stream's own frame 0: for each parent/child
//! edge of the canonical skeleton, where the parent's rotation puts the
//! child's rest attachment, against where the child's pivot actually is.
//!
//! Two retail streams are the control - the host's own battle idle and
//! the sibling's enemy-table idle - because "some gap" is normal (retail
//! authors these by hand and the parts overlap generously). The rebuilt
//! stream must not exceed what retail itself shows.
//!
//! NB a gap probe is blind to rotation: a part rolled about its own bone
//! axis flags no edge. This test claims only that the parts stay
//! ATTACHED, never that the idle is correct.
//!
//! Skips silently when `extracted/PROT/` or `LEGAIA_DISC_BIN` is missing.

use std::path::PathBuf;

use legaia_asset::monster_archive::PartPose;
use legaia_asset::party_swap::{self, PlayerRig, winpose};
use legaia_asset::{battle_char_assembly, monster_archive};

fn prot_dir() -> Option<PathBuf> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    ["extracted/PROT", "../../extracted/PROT"]
        .into_iter()
        .map(PathBuf::from)
        .find(|p| p.is_dir())
}

/// Every attachment of the canonical skeleton `[head, torso, pelvis,
/// armA(u,f,h), armB(u,f,h), legA(t,s,f), legB(t,s,f)]` as
/// `(parent, child)` - the chain edges of `CANONICAL_CHILD` plus the
/// four socket edges (shoulders on the torso, hips on the pelvis) that
/// are not chain edges but are still joints a viewer sees open.
const EDGES: [(usize, usize); 14] = [
    (1, 0),   // torso -> head
    (1, 2),   // torso -> pelvis
    (1, 3),   // torso -> armA upper
    (3, 4),   // armA upper -> fore
    (4, 5),   // armA fore -> hand
    (1, 6),   // torso -> armB upper
    (6, 7),   // armB upper -> fore
    (7, 8),   // armB fore -> hand
    (2, 9),   // pelvis -> legA thigh
    (9, 10),  // legA thigh -> shin
    (10, 11), // legA shin -> foot
    (2, 12),  // pelvis -> legB thigh
    (12, 13), // legB thigh -> shin
    (13, 14), // legB shin -> foot
];

/// The four edges of [`EDGES`] that are limb SOCKETS - a chain root
/// attaching to the torso or the pelvis. `retarget_clip` places these on
/// the baked socket (the sibling's own shoulder / hip, mapped onto the
/// host through the carrier's bake) rather than at the host's rest
/// offset, so their displacement from the host rest is authored, not a
/// defect. Held to constancy instead.
const SOCKETS: [(usize, usize); 4] = [(1, 3), (1, 6), (2, 9), (2, 12)];

const PART_NAMES: [&str; 15] = [
    "head", "torso", "pelvis", "armA.u", "armA.f", "armA.h", "armB.u", "armB.f", "armB.h",
    "legA.t", "legA.s", "legA.f", "legB.t", "legB.s", "legB.f",
];

/// PSX rotation order `Rz * Ry * Rx`, angles in 1/4096 turns (a copy of
/// the crate-private `party_swap::rot_matrix` - the probe has to redo the
/// pose math the engine does).
fn rot_matrix(p: &PartPose) -> [[f32; 3]; 3] {
    let rad = |r: u16| (r as f32) * std::f32::consts::TAU / 4096.0;
    let (sx, cx) = rad(p.rx).sin_cos();
    let (sy, cy) = rad(p.ry).sin_cos();
    let (sz, cz) = rad(p.rz).sin_cos();
    [
        [cy * cz, sx * sy * cz - cx * sz, cx * sy * cz + sx * sz],
        [cy * sz, sx * sy * sz + cx * cz, cx * sy * sz - sx * cz],
        [-sy, sx * cy, cx * cy],
    ]
}

fn apply(m: &[[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
    [
        m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
        m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
        m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
    ]
}

fn apply_t(m: &[[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
    [
        m[0][0] * v[0] + m[1][0] * v[1] + m[2][0] * v[2],
        m[0][1] * v[0] + m[1][1] * v[1] + m[2][1] * v[2],
        m[0][2] * v[0] + m[1][2] * v[1] + m[2][2] * v[2],
    ]
}

fn pivot(p: &PartPose) -> [f32; 3] {
    [p.tx as f32, p.ty as f32, p.tz as f32]
}

fn vsub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn vlen(a: [f32; 3]) -> f64 {
    ((a[0] * a[0] + a[1] * a[1] + a[2] * a[2]) as f64).sqrt()
}

/// Sign-extend a 12-bit field (the stream's translation encoding).
fn sx12(v: u16) -> i16 {
    if v & 0x800 != 0 {
        (v | 0xf000) as i16
    } else {
        v as i16
    }
}

/// Unpack one nine-byte part record - the `FUN_8004998C` bit layout.
fn unpack_part(b: &[u8]) -> PartPose {
    let f = |lo: usize, hi_nib: usize, high: bool| -> u16 {
        let h = b[hi_nib] as u16;
        b[lo] as u16
            | if high {
                (h & 0xf0) << 4
            } else {
                (h & 0x0f) << 8
            }
    };
    PartPose {
        tx: sx12(f(0, 2, false)),
        ty: sx12(f(1, 2, true)),
        tz: sx12(f(3, 5, false)),
        rx: f(4, 5, true) & 0xfff,
        ry: f(6, 8, false) & 0xfff,
        rz: f(7, 8, true) & 0xfff,
    }
}

fn decode_stream(bytes: &[u8], parts: usize, frames: usize) -> Vec<Vec<PartPose>> {
    (0..frames)
        .map(|f| {
            (0..parts)
                .map(|p| unpack_part(&bytes[(f * parts + p) * 9..(f * parts + p) * 9 + 9]))
                .collect()
        })
        .collect()
}

/// Worst FK gap per edge, over every frame: `|T_child - (T_parent +
/// R_parent * rest_local)|`, with `rest_local` the child's attachment in
/// the parent's frame taken from `rest` - the pose the part GEOMETRY was
/// authored against, which is what decides whether a joint looks open.
///
/// `rest` is the stream's own frame 0 for a retail stream, and the HOST's
/// retail rest for a rebuilt one: the playerize bake seats every part at
/// the host's rest pivots, so that is the spacing the swapped model's
/// bones actually have. Measuring a rebuilt stream against its own frame
/// 0 instead is blind to exactly the defect this file exists for - a
/// constant per-part offset present in every frame cancels out of it.
///
/// Returns `(worst gap, worst frame)` per edge plus the stream-wide worst.
fn edge_gaps(
    frames: &[Vec<PartPose>],
    chan: &[u8; 15],
    rest: &[PartPose],
) -> (Vec<(f64, usize)>, f64) {
    let mut out = Vec::with_capacity(EDGES.len());
    let mut worst = 0.0f64;
    for &(p, c) in &EDGES {
        let (pch, cch) = (chan[p] as usize, chan[c] as usize);
        let rest_local = apply_t(
            &rot_matrix(&rest[pch]),
            vsub(pivot(&rest[cch]), pivot(&rest[pch])),
        );
        let mut m = (0.0f64, 0usize);
        for (fi, fr) in frames.iter().enumerate() {
            let e = apply(&rot_matrix(&fr[pch]), rest_local);
            let t = pivot(&fr[pch]);
            let d = vlen(vsub(
                pivot(&fr[cch]),
                [t[0] + e[0], t[1] + e[1], t[2] + e[2]],
            ));
            if d > m.0 {
                m = (d, fi);
            }
        }
        worst = worst.max(m.0);
        out.push(m);
    }
    (out, worst)
}

fn print_table(label: &str, gaps: &[(f64, usize)]) {
    let mut row = String::new();
    for (i, &(g, f)) in gaps.iter().enumerate() {
        let (p, c) = EDGES[i];
        row.push_str(&format!(
            "  {:>6}->{:<6} {:7.2} @f{}\n",
            PART_NAMES[p], PART_NAMES[c], g, f
        ));
    }
    eprintln!("{label}\n{row}");
}

struct Host {
    file: Vec<u8>,
    rig: &'static PlayerRig,
    who: &'static str,
}

fn hosts(dir: &std::path::Path) -> Vec<Host> {
    [
        ("0863_edstati3.BIN", &party_swap::RIG_VAHN_GALA, "Vahn"),
        ("0864_edstati3.BIN", &party_swap::RIG_NOA, "Noa"),
        ("0865_battle_data.BIN", &party_swap::RIG_VAHN_GALA, "Gala"),
    ]
    .into_iter()
    .map(|(f, rig, who)| Host {
        file: std::fs::read(dir.join(f)).expect("read player file"),
        rig,
        who,
    })
    .collect()
}

const SIBLINGS: [(u16, &str); 3] = [(162, "Gi"), (163, "Che"), (164, "Lu")];

/// What each candidate whole-body anchor costs, on the un-anchored
/// retarget: the per-part frame-0 offsets from the host's rest (are they
/// one rigid shift or a scatter?), then, for the torso / head / foot-level
/// anchors, where the anchored body's feet and pelvis end up against the
/// host rest the rest of the character's clips start from.
///
/// GTE space is y-DOWN, so the feet are the parts at the LARGEST y and a
/// positive `dy` error means the body sinks into the floor.
fn anchor_report(
    name: &str,
    who: &str,
    raw: &[Vec<PartPose>],
    host_frames: &[Vec<PartPose>],
    chan_rig: &PlayerRig,
) {
    let chan = &chan_rig.channel_for_canonical;
    let host_rest = &host_frames[0];
    let hp = |c: usize| pivot(&host_rest[chan[c] as usize]);
    let rp = |c: usize| pivot(&raw[0][chan[c] as usize]);
    let mut line = String::new();
    for (c, part) in PART_NAMES.iter().enumerate() {
        let d = vsub(hp(c), rp(c));
        line.push_str(&format!(
            "  {part:>6} d=({:6.1},{:6.1},{:6.1}) |d|={:6.1}\n",
            d[0],
            d[1],
            d[2],
            vlen(d)
        ));
    }
    eprintln!("   {name} -> {who}: frame-0 offset from host rest, per part\n{line}");

    // Ground contact = the LOWER (larger-y) of the two ankle pivots; the
    // support point = their midpoint in x/z.
    let floor = |f: &dyn Fn(usize) -> [f32; 3]| f(11)[1].max(f(14)[1]);
    let stand = |f: &dyn Fn(usize) -> [f32; 3]| {
        let (a, b) = (f(11), f(14));
        [(a[0] + b[0]) / 2.0, floor(f), (a[2] + b[2]) / 2.0]
    };
    let span = {
        let ys: Vec<f32> = (0..15).map(|c| hp(c)[1]).collect();
        ys.iter().copied().fold(f32::MIN, f32::max) - ys.iter().copied().fold(f32::MAX, f32::min)
    };
    let torso_d = vsub(hp(1), rp(1));
    let head_d = vsub(hp(0), rp(0));
    let stand_d = vsub(stand(&hp), stand(&rp));
    let mixed_d = [torso_d[0], stand_d[1], torso_d[2]];
    let shipped = winpose::idle_anchor(raw, host_frames, chan_rig);
    eprintln!("   host pivot span (crown ankle) {span:.0} units");
    for (label, d) in [
        ("torso", torso_d),
        ("head", head_d),
        ("torso-xz + floor-y", mixed_d),
        ("support point", stand_d),
        (
            "SHIPPED",
            [shipped[0] as f32, shipped[1] as f32, shipped[2] as f32],
        ),
    ] {
        let at = |c: usize| {
            let p = rp(c);
            [p[0] + d[0], p[1] + d[1], p[2] + d[2]]
        };
        let sunk = at(11)[1].max(at(14)[1]) - floor(&hp);
        // The same, taken over the WHOLE cycle on both sides: how much
        // deeper than retail's own deepest contact this idle ever plants.
        let deepest = |fr: &[Vec<PartPose>]| {
            fr.iter()
                .map(|f| f[chan[11] as usize].ty.max(f[chan[14] as usize].ty) as f32)
                .fold(f32::MIN, f32::max)
        };
        let cycle = deepest(raw) + d[1] - deepest(host_frames);
        let foot_xz = {
            let (a, b) = (stand(&hp), stand(&rp));
            vlen([b[0] + d[0] - a[0], 0.0, b[2] + d[2] - a[2]])
        };
        eprintln!(
            "   anchor {label:>18}: d=({:6.1},{:6.1},{:6.1}) floor dy f0 {sunk:+6.1} \
             cycle {cycle:+6.1} stance dxz {foot_xz:5.1} | err torso {:5.1} pelvis {:5.1} \
             head {:5.1}",
            d[0],
            d[1],
            d[2],
            vlen(vsub(at(1), hp(1))),
            vlen(vsub(at(2), hp(2))),
            vlen(vsub(at(0), hp(0))),
        );
    }
}

/// The defect the probe exists for: the rebuilt idle must keep every
/// joint as closed as the retail streams it is built from.
#[test]
fn the_rebuilt_idle_keeps_its_joints_closed() {
    let Some(dir) = prot_dir() else {
        eprintln!("[skip] extracted/PROT or LEGAIA_DISC_BIN missing");
        return;
    };
    let archive = std::fs::read(dir.join("0867_battle_data.BIN")).expect("read archive");
    let identity: [u8; 15] = std::array::from_fn(|i| i as u8);
    // Collected so one torn pair does not hide the other eight.
    let mut failures: Vec<String> = Vec::new();

    for host in hosts(&dir) {
        let retail = battle_char_assembly::idle_battle_animation(&host.file)
            .expect("host idle")
            .expect("host idle populated");
        let host_rest = retail.frames[0].clone();
        let (rgaps, rworst) =
            edge_gaps(&retail.frames, &host.rig.channel_for_canonical, &host_rest);
        eprintln!(
            "== {} retail idle: {} parts x {} frames, worst FK gap {:.2}",
            host.who, retail.part_count, retail.frame_count, rworst
        );
        print_table("   retail host edges", &rgaps);

        for (id, name) in SIBLINGS {
            let src = monster_archive::idle_animation(&archive, id)
                .expect("sibling idle")
                .expect("sibling idle populated");
            let (sgaps, sworst) = edge_gaps(&src.frames, &identity, &src.frames[0]);
            eprintln!(
                "-- {name} enemy idle: {} parts x {} frames, worst FK gap {:.2}",
                src.part_count, src.frame_count, sworst
            );
            print_table("   sibling edges", &sgaps);

            let built = winpose::rebuild_idle_stream(&host.file, host.rig, &archive, id)
                .unwrap_or_else(|e| panic!("{name} -> {}: rebuild: {e:#}", host.who));
            let rows = decode_stream(&built.bytes, retail.part_count, built.frames);
            assert_eq!(rows.len(), retail.frame_count, "rebuilt frame count");

            // Competing hypothesis: a canonical part left at
            // `PartPose::default()` (the world origin) would read as
            // "detached" too. Every channel the rig names must be inside
            // the stream and must carry a real pose.
            let named: Vec<(usize, &str)> = PART_NAMES
                .iter()
                .enumerate()
                .map(|(c, n)| (host.rig.channel_for_canonical[c] as usize, *n))
                .chain(host.rig.hair_channel.map(|h| (h as usize, "hair")))
                .collect();
            for (ch, part) in named {
                assert!(
                    ch < retail.part_count,
                    "{name} -> {}: {part} maps to channel {ch} outside the {}-part \
                     stream - retarget_clip would leave it at the origin",
                    host.who,
                    retail.part_count
                );
                assert!(
                    rows[0][ch] != PartPose::default(),
                    "{name} -> {}: {part} (channel {ch}) is the default pose (origin)",
                    host.who
                );
            }

            // The un-anchored retarget, for the anchor arithmetic below:
            // FK straight off the host rig, before any whole-body shift.
            let raw = winpose::retarget_clip(
                &src,
                host.rig,
                &host.file,
                &archive,
                id,
                retail.part_count,
                retail.frame_count,
            )
            .expect("un-anchored retarget");
            anchor_report(name, host.who, &raw, &retail.frames, host.rig);

            // Two references, because the rebuilt skeleton has two kinds
            // of joint. Chain and carried edges hang off the HOST's rest
            // spacing (`retarget_clip` walks the baked rig's own bone
            // vectors), so the host rest is their ground truth. The four
            // SOCKET edges do not: a limb chain's root is placed on the
            // baked socket - where the sibling's own shoulder / hip
            // landed once its pelvis and torso were baked onto the host -
            // so its offset from the host's rest is a deliberate constant
            // carrying the sibling's build. Those are held to constancy
            // instead, against the stream's own frame 0.
            let (abs_gaps, _) = edge_gaps(&rows, &host.rig.channel_for_canonical, &host_rest);
            let (rel_gaps, _) = edge_gaps(&rows, &host.rig.channel_for_canonical, &rows[0]);
            // Worst over the ten NON-socket edges of each table.
            let worst_anchored = |gaps: &[(f64, usize)]| -> f64 {
                EDGES
                    .iter()
                    .zip(gaps)
                    .filter(|(e, _)| !SOCKETS.contains(e))
                    .map(|(_, g)| g.0)
                    .fold(0.0, f64::max)
            };
            let (abs_worst, rel_worst) = (worst_anchored(&abs_gaps), worst_anchored(&rel_gaps));
            eprintln!(
                "** {name} -> {}: rebuilt idle anchored edges - worst gap from the host \
                 rest {abs_worst:.2}, worst drift across its own frames {rel_worst:.2} \
                 (retail host {rworst:.2}, sibling {sworst:.2})",
                host.who
            );
            print_table("   rebuilt edges vs host rest", &abs_gaps);
            print_table("   rebuilt edges vs own frame 0", &rel_gaps);

            // The rebuilt stream is FK-derived from the host rig, so its
            // joints should be TIGHTER than either hand-authored control,
            // never looser. Allow the controls' own worst plus a rounding
            // margin (the stream stores integers).
            let budget = rworst.max(sworst) + 4.0;
            let fail = |what: &str, got: f64| {
                format!(
                    "{name} -> {}: rebuilt idle {what} is {got:.2} units against a \
                     {budget:.2}-unit retail budget - the body is coming apart",
                    host.who
                )
            };
            if abs_worst > budget {
                failures.push(fail("joint gap from the host rest", abs_worst));
            }
            if rel_worst > budget {
                failures.push(fail("joint drift across its own frames", rel_worst));
            }

            // The whole point of the re-anchor: it is a RIGID whole-body
            // translation. Applied to one channel it does not move the
            // character at all, it shears that part off the body - which
            // no gap measurement can see on a socket edge and which the
            // frame-to-frame measurement above is blind to by
            // construction (the shear is the same in every frame).
            let mut shifts: Vec<[i16; 3]> = Vec::new();
            for (f, (r, w)) in rows.iter().zip(&raw).enumerate() {
                for (ch, (a, b)) in r.iter().zip(w).enumerate() {
                    let d = [a.tx - b.tx, a.ty - b.ty, a.tz - b.tz];
                    if !shifts.contains(&d) {
                        shifts.push(d);
                        if shifts.len() > 1 {
                            failures.push(format!(
                                "{name} -> {}: frame {f} channel {ch} moves by {d:?} \
                                 where the rest of the stream moves by {:?} - the \
                                 re-anchor is not a rigid whole-body translation",
                                host.who, shifts[0]
                            ));
                        }
                    }
                }
            }

            // Headroom in the 12-bit packed translation field: a pose
            // pushed past +-2048 wraps (the decode sign-extends, so a
            // wrap cannot be seen after the fact - it shows up only as a
            // torn joint above). Reported so a shrinking margin is
            // visible before it bites.
            let peak = rows
                .iter()
                .flatten()
                .flat_map(|p| [p.tx, p.ty, p.tz])
                .map(|v| v.unsigned_abs())
                .max()
                .unwrap_or(0);
            eprintln!("   peak |coordinate| {peak} of the 2047 the field holds");
        }
    }
    assert!(
        failures.is_empty(),
        "{} of 9 sibling/host pairs tear:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
