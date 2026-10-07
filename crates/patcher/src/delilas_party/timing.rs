//! Frame retiming: clip contacts, retimed hit frames and effect-script edits.
//! Split out of `delilas_party.rs`.

use super::*;

/// Map a frame index from the host stream's timeline onto the rebuilt
/// one, never past the last frame.
pub(super) fn rescale_frame(f: u8, from: usize, to: usize) -> u8 {
    if from == 0 || to == 0 {
        return f;
    }
    let scaled = (f as usize * to).div_ceil(from);
    scaled.min(to - 1) as u8
}

/// One connect in the **rebuilt stream's** frame space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Contact {
    /// The value to write into a hit slot / effect gate for this connect.
    pub(super) frame: usize,
    /// How hard the stage decelerates at this connect, in that stage's own
    /// units. `f64::INFINITY` when the stage's beats are authored on disc,
    /// so an authored stage can never be filtered out by a measured one.
    pub(super) force: f64,
}

/// The frame a clip's whole body stops moving on, and how hard it stops.
///
/// A strike is the frame the body's translation speed falls fastest: the
/// wind-up accelerates, the connect arrests. Speed is the mean per-part
/// translation delta over consecutive keyframes, so it reads the whole body
/// rather than one bone that may or may not be the weapon.
///
/// The returned frame is already a **hit value**, not a visual frame: a hit
/// fires on the first tick where `frame >= value - 1` (`FUN_801EC3E4`
/// `0x801ec468`-`0x801ec478`), so landing damage on the connect means
/// writing `connect + 1`.
///
/// `None` when the clip is too short to have a speed profile or never
/// decelerates at all (a pure approach).
pub(super) fn principal_impact(
    clip: &legaia_asset::monster_archive::MonsterAnimation,
) -> Option<Contact> {
    if clip.part_count == 0 || clip.frame_count < 4 {
        return None;
    }
    let speed: Vec<f64> = (1..clip.frame_count)
        .map(|i| {
            let (a, b) = (&clip.frames[i - 1], &clip.frames[i]);
            let sum: f64 = (0..clip.part_count)
                .map(|p| {
                    let (u, v) = (a[p], b[p]);
                    let dx = f64::from(v.tx) - f64::from(u.tx);
                    let dy = f64::from(v.ty) - f64::from(u.ty);
                    let dz = f64::from(v.tz) - f64::from(u.tz);
                    (dx * dx + dy * dy + dz * dz).sqrt()
                })
                .sum();
            sum / clip.part_count as f64
        })
        .collect();
    let mut best: Option<(usize, f64)> = None;
    for i in 1..speed.len() {
        let drop = speed[i - 1] - speed[i];
        if drop > 0.0 && best.is_none_or(|(_, f)| drop > f) {
            best = Some((i, drop));
        }
    }
    let (i, force) = best?;
    // `speed[j]` spans keyframes `j..j+1`, so a fall from `speed[i-1]` to
    // `speed[i]` means the body arrived on keyframe `i` - that is the
    // connect - and the `+ 1` is the firing rule above.
    Some(Contact {
        frame: i + 1,
        force,
    })
}

/// Every frame the sibling's chain actually connects on, in the rebuilt
/// stream's coordinates.
///
/// Authority order per stage: the stage's OWN authored beats when it is in
/// the damaging tag band (`0x0C..=0x1F` - the castable/attack actions, the
/// only ones retail gives `+0x10..+0x13` to) and carries any; otherwise its
/// measured [`principal_impact`]. Lu's strike stages are authored; Gi's and
/// Che's signature stages are tag `0x23`, whose beats are all zero because
/// as enemies their damage comes from the PROT 958 / 959 cast modules, so
/// theirs are measured.
///
/// A measured stage whose deceleration is under 40% of the chain's hardest
/// is dropped: that is a wind-up settling, not a connect. Authored stages
/// carry infinite force and so are never dropped.
///
/// The per-stage length expression is the rebuild's own
/// (`(frame_count * rate).div_ceil(stage.rate)`), and the front stages are
/// the ones the rebuild drops when the slot is tight, so only the last
/// [`RebuiltArtSlot::stages`](crate::party_swap::winpose::RebuiltArtSlot)
/// of the chain are walked.
pub(super) fn chain_contacts(
    chain: &[&legaia_asset::monster_archive::MonsterAnimation],
    rebuilt: &crate::party_swap::winpose::RebuiltArtSlot,
) -> Vec<Contact> {
    let kept = chain.len().saturating_sub(rebuilt.stages);
    let rate = rebuilt.rate.max(1) as usize;
    let last = rebuilt.frames.saturating_sub(1).max(1);

    // Pass 1: every stage's own beats, its own measured impact, and where
    // it sits in the concatenated stream. The measured impact is taken even
    // for a stage whose beats are authored, because it is the yardstick the
    // wind-up filter below is measured against.
    struct Stage<'a> {
        clip: &'a legaia_asset::monster_archive::MonsterAnimation,
        start: usize,
        len: usize,
        authored: Vec<u8>,
        measured: Option<Contact>,
    }
    let mut stages: Vec<Stage<'_>> = Vec::new();
    let mut start = 0usize;
    for stage in &chain[kept.min(chain.len())..] {
        let len = (stage.frame_count * rate)
            .div_ceil(stage.rate.max(1) as usize)
            .max(1);
        let authored: Vec<u8> = if (0x0C..=0x1F).contains(&stage.action_id) {
            (0..4)
                .filter_map(|i| stage.effect_script.get(0x10 + i).copied())
                .filter(|&f| f != 0)
                .collect()
        } else {
            Vec::new()
        };
        stages.push(Stage {
            clip: stage,
            start,
            len,
            authored,
            measured: principal_impact(stage),
        });
        start += len;
    }

    // Pass 2: a stage with no beats of its own only counts as a connect if
    // it stops the body hard against the chain's own hardest stop. That is
    // what tells Lu's opening lunge (a wind-up she authored no beat for)
    // from the two swings she did.
    let hardest = stages
        .iter()
        .filter_map(|s| s.measured.map(|c| c.force))
        .fold(0.0f64, f64::max);
    let mut out: Vec<Contact> = Vec::new();
    for s in &stages {
        let local: Vec<Contact> = if s.authored.is_empty() {
            s.measured
                .filter(|c| c.force >= 0.4 * hardest)
                .into_iter()
                .collect()
        } else {
            s.authored
                .iter()
                .map(|&f| Contact {
                    frame: f as usize,
                    force: f64::INFINITY,
                })
                .collect()
        };
        for c in local {
            let mapped = s.start + (c.frame * s.len).div_ceil(s.clip.frame_count.max(1));
            out.push(Contact {
                frame: mapped.clamp(1, last),
                force: c.force,
            });
        }
    }
    out.sort_by_key(|c| c.frame);
    out.dedup_by_key(|c| c.frame);
    out
}

/// Force a hit / gate list strictly ascending inside `1..=last`, keeping
/// its length. A tail that collides with the cap is pushed back down rather
/// than collapsed onto one frame.
pub(super) fn ascending_within(frames: &[usize], last: usize) -> Vec<u8> {
    let last = last.max(1);
    let mut out: Vec<usize> = Vec::with_capacity(frames.len());
    let mut prev = 0usize;
    for &f in frames {
        let v = f.max(prev + 1).clamp(1, last);
        out.push(v);
        prev = v;
    }
    for i in (0..out.len().saturating_sub(1)).rev() {
        if out[i] >= out[i + 1] {
            out[i] = out[i + 1].saturating_sub(1).max(1);
        }
    }
    out.into_iter().map(|v| v.min(0xFF) as u8).collect()
}

/// Re-time the art's hit events (`entry +0x10..0x13`) onto the frames the
/// sibling's chain actually connects on.
///
/// A proportional rescale across the WHOLE stream is wrong for a chain -
/// the host's hits were spaced against a single swing, so spreading them
/// over wind-up-plus-payoff drops most of them into the wind-up. But
/// anchoring them on the payoff stage's *start* is wrong too, and that is
/// what shipped: the payoff stage opens with its own approach, so the first
/// application landed 1.1-3.0 seconds after the body connected in all nine
/// sibling/host pairings. The anchor has to be the connect itself
/// ([`chain_contacts`]), not a stage boundary.
///
/// The number of non-zero slots is always the host's. `entry[+0x00 + i]`
/// (power) and `entry[+0x10 + i]` (frame) are parallel arrays walked by one
/// cursor (`actor[+0x1F4]`), so moving the count would re-pair the power
/// bytes, and a zero slot ends the walk outright (`0x801EC47C`) - which is
/// why nothing here may write `0`.
///
/// With more connects than hits the hits sample the connects evenly with
/// both endpoints included, so the first application lands on the first
/// connect and the last (biggest) power byte on the finisher. With fewer,
/// the extras ride the same impact on consecutive frames - retail's own
/// multi-hit idiom (Burning Flare is `11 12 13 14` on one swing).
pub(super) fn retimed_hit_frames(
    host: &legaia_asset::battle_char_assembly::ArtAnimRecord,
    rebuilt: &crate::party_swap::winpose::RebuiltArtSlot,
    contacts: &[Contact],
) -> (Vec<(usize, u8)>, String) {
    if rebuilt.frames == rebuilt.retail_frames && rebuilt.stages == 1 {
        return (Vec::new(), "unchanged (retail shape)".into());
    }
    let head = &host.effect_script;
    let k = (0..4)
        .filter_map(|i| head.get(0x10 + i).copied())
        .filter(|&f| f != 0)
        .count();
    if k == 0 {
        return (Vec::new(), "none scheduled".into());
    }
    let last = rebuilt.frames.saturating_sub(1).max(1);
    let (raw, why) = if contacts.is_empty() {
        // No measurable connect anywhere in the chain: keep the old
        // behaviour rather than scheduling on nothing - the host's rhythm,
        // compressed into the payoff stage.
        let start = rebuilt.payoff_start.min(last);
        let span = rebuilt.frames - start;
        let f = (0..4)
            .filter_map(|i| head.get(0x10 + i).copied())
            .filter(|&f| f != 0)
            .map(|h| start + rescale_frame(h, rebuilt.retail_frames, span) as usize)
            .collect::<Vec<_>>();
        (
            f,
            format!("no measurable connect - the host's {k} hit(s) re-spaced over the payoff"),
        )
    } else {
        let n = contacts.len();
        let mut f = Vec::with_capacity(k);
        if n >= k {
            for i in 0..k {
                let idx = if k == 1 { n - 1 } else { i * (n - 1) / (k - 1) };
                f.push(contacts[idx].frame);
            }
        } else {
            let (per, rem) = (k / n, k % n);
            for (i, c) in contacts.iter().enumerate() {
                for j in 0..per + usize::from(i < rem) {
                    f.push(c.frame + j);
                }
            }
        }
        let measured = contacts.iter().filter(|c| c.force.is_finite()).count();
        (
            f,
            format!(
                "{k} hit(s) on the chain's {n} connect(s) at {:?} ({} authored, {measured} measured)",
                contacts.iter().map(|c| c.frame).collect::<Vec<_>>(),
                n - measured,
            ),
        )
    };
    let frames = ascending_within(&raw, last);
    let edits = (0..4)
        .map(|i| {
            (
                host.entry_offset + 0x10 + i,
                frames.get(i).copied().unwrap_or(0),
            )
        })
        .collect();
    (edits, why)
}

/// Whether an effect-script record actually spawns something: id `0` is
/// an empty slot and `id & 0x7F == 0x7F` terminates the walk.
pub(super) fn fx_record_spawns(script: &[u8], i: usize) -> bool {
    script
        .get(FX_BASE + i * FX_RECORD + 1)
        .is_some_and(|&id| id != 0 && id & 0x7F != 0x7F)
}

/// How many of a script's eight records spawn.
pub(super) fn fx_live_count(script: &[u8]) -> usize {
    if script.len() < FX_BASE + FX_RECORDS * FX_RECORD {
        return 0;
    }
    (0..FX_RECORDS)
        .filter(|&i| fx_record_spawns(script, i))
        .count()
}

/// Give the art the SIBLING's own visual effects.
///
/// The host art's script is eight `[frame_gate, effect_id, x, y, z]`
/// records - for Vahn's Burning Flare, eight spawns of flame `0x96`
/// sweeping forward through the swing. That flame is why a reskinned art
/// still reads as the host's move however faithful the body animation
/// gets, so it has to go.
///
/// The staged special clips carry **no** script of their own: as an
/// enemy, a sibling's signature move gets its visuals from a per-spell
/// code module (PROT 960 for Lu's), which spawns from module-resident
/// parameter blocks the art record's one-byte id cannot name. What the
/// siblings do have is their ordinary casts, whose entries carry real
/// scripts in exactly this format - so the effects come from there,
/// re-timed onto the rebuilt stream.
///
/// With nothing to borrow, the host's script is suppressed instead by
/// deferring every gate to `0xFF`: the walker never advances its cursor
/// past a gate it has not reached, so nothing spawns and no terminator
/// arm runs. That leaves the art quiet but honest - the body still
/// swings and the hits still land, with no borrowed fire.
///
/// `burst` is the prototype id of a record transplanted out of the
/// sibling's own cast module ([`crate::delilas_effects`]) - the genuine
/// enemy-side spectacle rather than an approximation of it. It replaces
/// the spawning records' effect id while keeping the donor's offsets,
/// because both spawn paths apply the record's own XYZ. The gate must stay
/// non-zero: the walker terminates on `record[0] == 0`, not on the id.
///
/// The gates come from `contacts` - the same connect list the hit frames
/// are scheduled on ([`chain_contacts`]) - so the burst, the damage and the
/// body all run off ONE clock. Rescaling the donor's own gates instead
/// (which is what shipped) puts the burst on a third timeline: the donor is
/// an unrelated cast of the sibling's, and a whole-stream proportional
/// rescale of its beats lands nowhere near either the strike or the
/// contact. The walker's gate rule is the hit rule (`frame + 1 >= gate`,
/// `0x801decb4`), so a gate equal to a hit value fires on the same frame.
pub(super) fn effect_script_edits(
    host: &legaia_asset::battle_char_assembly::ArtAnimRecord,
    clip: &legaia_asset::monster_archive::MonsterAnimation,
    siblings_clips: &[legaia_asset::monster_archive::MonsterAnimation],
    rebuilt: &crate::party_swap::winpose::RebuiltArtSlot,
    burst: Option<u8>,
    contacts: &[Contact],
) -> (Vec<(usize, u8)>, String) {
    // The clip's own script first; failing that, the sibling's richest
    // other one (their casts spawn the projectile art they own).
    let donor = if fx_live_count(&clip.effect_script) > 0 {
        Some((clip, "its own"))
    } else {
        // Only the offensive band: the reaction tags (idle, walk, flinch,
        // knockdown, block - everything below 0x0C) carry scripts too,
        // and a knockdown's dust cloud is not what a signature move
        // should spawn.
        siblings_clips
            .iter()
            .filter(|a| a.action_id >= 0x0C && fx_live_count(&a.effect_script) > 0)
            .max_by_key(|a| fx_live_count(&a.effect_script))
            .map(|a| (a, "borrowed from the sibling's own casts"))
    };
    let Some((donor, origin)) = donor else {
        let edits = (0..FX_RECORDS)
            .map(|i| (host.entry_offset + FX_BASE + i * FX_RECORD, 0xFFu8))
            .collect();
        return (
            edits,
            "host art's flame suppressed (the sibling has no script to give)".into(),
        );
    };
    let src = &donor.effect_script;
    // Each spawning record takes the connect at the same ordinal, cycling
    // when the donor spawns more times than the chain connects. Falling
    // back to the donor's own rescaled beat keeps a chain with no
    // measurable connect firing something.
    let gates: Vec<u8> = if contacts.is_empty() {
        Vec::new()
    } else {
        ascending_within(
            &contacts.iter().map(|c| c.frame).collect::<Vec<_>>(),
            rebuilt.frames.saturating_sub(1).max(1),
        )
    };
    let mut spawned = 0usize;
    let mut edits = Vec::with_capacity(FX_RECORDS * FX_RECORD);
    for i in 0..FX_RECORDS {
        let rec = &src[FX_BASE + i * FX_RECORD..FX_BASE + (i + 1) * FX_RECORD];
        let dst = host.entry_offset + FX_BASE + i * FX_RECORD;
        // An empty or terminating record is copied verbatim - its gate is
        // what ends the walk.
        let gate = if fx_record_spawns(src, i) {
            let g = if gates.is_empty() {
                rescale_frame(rec[0], donor.frame_count, rebuilt.frames)
            } else {
                gates[spawned % gates.len()]
            };
            spawned += 1;
            g.max(1)
        } else {
            rec[0]
        };
        edits.push((dst, gate));
        for (k, &b) in rec.iter().enumerate().skip(1) {
            // A spawning record hands its slot to the transplanted burst;
            // its own gate and offsets stand, since both spawn paths read
            // the record's XYZ.
            let byte = match burst {
                Some(id) if k == 1 && fx_record_spawns(src, i) => id,
                _ => b,
            };
            edits.push((dst + k, byte));
        }
    }
    let live = fx_live_count(src);
    let when = if gates.is_empty() {
        "on the donor's own rescaled beats".to_string()
    } else {
        format!(
            "on the chain's connects {:?}",
            &gates[..gates.len().min(live)]
        )
    };
    let why = match burst {
        Some(id) => format!(
            "{live} spawn(s) of the sibling's own cast-module burst \
             (transplanted id {id}) {when}"
        ),
        None => {
            let ids: Vec<String> = (0..FX_RECORDS)
                .filter(|&i| fx_record_spawns(src, i))
                .map(|i| format!("0x{:02X}", src[FX_BASE + i * FX_RECORD + 1]))
                .collect();
            format!("{live} spawn(s) {origin} ({}) {when}", ids.join(", "))
        }
    };
    (edits, why)
}

/// A combo as the game prints it: `L R L R D`.
pub(super) fn combo_str(cmds: &[legaia_art::queue::Command]) -> String {
    use legaia_art::queue::Command;
    cmds.iter()
        .map(|c| match c {
            Command::Up => "U",
            Command::Down => "D",
            Command::Left => "L",
            Command::Right => "R",
        })
        .collect::<Vec<_>>()
        .join(" ")
}
