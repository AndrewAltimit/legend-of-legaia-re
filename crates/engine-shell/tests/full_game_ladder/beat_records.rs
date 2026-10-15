//! Reading what an actor record's script would do before playing it: the
//! latches a beat would overreach, staged fights, minigame entries and
//! entry-script hand-offs.

use super::*;

/// Does the next anchor carry `flag`? `true` when there is no anchor to ask.
pub(super) fn next_anchor_has(flag: u16) -> bool {
    NEXT_ANCHOR_FLAGS.with(|a| a.borrow().as_ref().is_none_or(|f| f.contains(&flag)))
}

/// The partition-1 records of the loaded scene that **poll a box** of the
/// player's tiles (a player-channel `BBoxTest` among their first few
/// instructions, `CD F8 x0 z0 x1 z1`) and so run while the player stands
/// in it, with the box's tiles - those worth standing in: the record
/// cleanly raises a still-clear system flag the next anchor carries or a
/// partition-2 record of the scene tests. `doman` P1[18], Dr. Usha's
/// intercom on (40..41, 81), asks for the password (`0x36E`) and calls the
/// lift (`0x371`), which the lift band P2[7] waits on.
pub(super) fn box_poll_beats(
    session: &NativeSession,
    mf: &legaia_scene_formats::man_section::ManFile,
    man: &[u8],
) -> Vec<(u8, Vec<(u8, u8)>)> {
    use legaia_engine_field::man_field_scripts::{
        FlagBank, partition_record_span, walk_partition_gflag_sites,
    };
    use legaia_scene_formats::field_disasm::{InsnInfo, LinearWalker};
    let w = &session.host.world;
    let tested2: BTreeSet<u16> = walk_partition_gflag_sites(mf, man, 2)
        .iter()
        .filter(|s| s.bank == FlagBank::System && s.kind == FlagKind::Test && s.clean)
        .map(|s| s.flag)
        .collect();
    let sets1 = walk_partition_gflag_sites(mf, man, 1);
    // A flag the scene lowers again is a ride's, not the story's: `doman`'s
    // intercom raises `0x371` to call the lift, P2[3] sends it back up. The
    // box is stood in for one only while a story talk waits in a walk
    // component the ride reaches - Dr. Usha's lab, once `0x36F` has given
    // her talk something to say.
    let lowered: BTreeSet<u16> = (0..3)
        .flat_map(|p| walk_partition_gflag_sites(mf, man, p))
        .filter(|s| s.bank == FlagBank::System && s.kind == FlagKind::Clear && s.clean)
        .map(|s| s.flag)
        .collect();
    let n1 = mf.partitions.get(1).map_or(0, Vec::len);
    let mut out = Vec::new();
    for rec in 1..n1 {
        let Some((start, pc0, len)) = partition_record_span(mf, man, 1, rec) else {
            continue;
        };
        let bx = LinearWalker::new(&man[start..start + len], pc0)
            .take(4)
            .map_while(Result::ok)
            .find_map(|i| match i.info {
                InsnInfo::BBoxTest {
                    x_min,
                    z_min,
                    x_max,
                    z_max,
                    ..
                } if i.extended == Some(0xF8) => Some((x_min, z_min, x_max, z_max)),
                _ => None,
            });
        let Some((x0, z0, x1, z1)) = bx else { continue };
        let worth = sets1.iter().any(|s| {
            s.record == rec
                && s.bank == FlagBank::System
                && s.kind == FlagKind::Set
                && s.clean
                && !s.text_alias
                && !s.debug_menu
                && !w.system_flag_test(s.flag)
                && (next_anchor_has(s.flag)
                    || (tested2.contains(&s.flag)
                        && (!lowered.contains(&s.flag) || !far_talk_tiles(session).is_empty())))
        });
        if !worth || x1 < x0 || z1 < z0 || usize::from(x1 - x0) * usize::from(z1 - z0) > 16 {
            continue;
        }
        let tiles = (z0..=z1)
            .flat_map(|z| (x0..=x1).map(move |x| (x, z)))
            .collect();
        out.push((rec as u8, tiles));
    }
    out
}

/// [`talk_box_gate`]'s answer: the flag, the box's tiles and the member
/// whose own box test sits beside the leader's.
pub(super) type TalkBox = (u16, Vec<(i16, i16)>, Option<u8>);

/// A talk whose story branch waits on a flag the scene's entry script
/// (P1[0]) raises for a party member standing in a box: the flag, and the
/// tiles of the last player-channel `BBoxTest` ahead of the raise, and the
/// party member (`0x30 + n` channel) the script tests on the same box, if
/// any. `nilboa` P1[13] / P1[14], the Three Tunnels' boulders, push
/// (`0x457`) only while `0x4F2` is up, which the entry script raises while
/// the leader or member `0x31` stands in (53..55, 62..64) - not where the
/// boulders are talked to from. `None` when the talk tests no such flag, or
/// it is up.
pub(super) fn talk_box_gate(
    session: &NativeSession,
    mf: &legaia_scene_formats::man_section::ManFile,
    man: &[u8],
    slot: u8,
) -> Option<TalkBox> {
    use legaia_engine_field::man_field_scripts::{
        FlagBank, partition_record_span, walk_partition_gflag_sites,
    };
    use legaia_scene_formats::field_disasm::{InsnInfo, LinearWalker};
    let w = &session.host.world;
    let sites = walk_partition_gflag_sites(mf, man, 1);
    let raised: Vec<&legaia_engine_field::man_field_scripts::GFlagSite> = sites
        .iter()
        .filter(|s| {
            s.record == 0 && s.bank == FlagBank::System && s.kind == FlagKind::Set && s.clean
        })
        .collect();
    let (start, pc0, len) = partition_record_span(mf, man, 1, 0)?;
    let entry: Vec<_> = LinearWalker::new(&man[start..start + len], pc0)
        .map_while(Result::ok)
        .collect();
    sites
        .iter()
        .filter(|s| {
            s.record == usize::from(slot)
                && s.bank == FlagBank::System
                && s.kind == FlagKind::Test
                && s.clean
                && !w.system_flag_test(s.flag)
        })
        .find_map(|t| {
            let set = raised.iter().find(|r| r.flag == t.flag)?;
            let at = set.abs_pc.checked_sub(start)?;
            let bx = entry
                .iter()
                .rev()
                .filter(|i| i.pc < at && i.extended == Some(0xF8))
                .find_map(|i| match i.info {
                    InsnInfo::BBoxTest {
                        x_min,
                        z_min,
                        x_max,
                        z_max,
                        ..
                    } => Some((x_min, z_min, x_max, z_max)),
                    _ => None,
                })?;
            let (x0, z0, x1, z1) = bx;
            if x1 < x0 || z1 < z0 || usize::from(x1 - x0) * usize::from(z1 - z0) > 16 {
                return None;
            }
            let tiles = (z0..=z1)
                .flat_map(|z| (x0..=x1).map(move |x| (i16::from(x), i16::from(z))))
                .collect();
            // The member whose own box test sits beside the leader's: the
            // one that holds the flag up once control has moved on.
            let holder = entry.iter().find_map(|i| match (i.extended, &i.info) {
                (
                    Some(ch @ 0x30..=0x32),
                    InsnInfo::BBoxTest {
                        x_min,
                        z_min,
                        x_max,
                        z_max,
                        ..
                    },
                ) if (*x_min, *z_min, *x_max, *z_max) == bx => Some(ch - 0x30),
                _ => None,
            });
            Some((t.flag, tiles, holder))
        })
}

/// The partition-2 records of the scene whose bodies test (cleanly) one of
/// `flags`: the bands a just-raised flag arms (`doman` P2[7], the lift,
/// loops on `0x371` until the intercom raises it).
pub(super) fn bands_testing(
    mf: &legaia_scene_formats::man_section::ManFile,
    man: &[u8],
    flags: &BTreeSet<u16>,
) -> BTreeSet<u8> {
    use legaia_engine_field::man_field_scripts::{FlagBank, walk_partition_gflag_sites};
    walk_partition_gflag_sites(mf, man, 2)
        .iter()
        .filter(|s| {
            s.bank == FlagBank::System
                && s.kind == FlagKind::Test
                && s.clean
                && flags.contains(&s.flag)
        })
        .filter_map(|s| u8::try_from(s.record).ok())
        .collect()
}

/// The records of `partition` that SET (cleanly, outside a debug picker) a
/// **latch** the next anchor does not carry - a flag some partition-2
/// record of this scene lists in its C1 gate, so setting it shuts that
/// record. The retail run had not played such a beat by the next milestone,
/// and playing it overreaches: `conc2` P2[12] latches `0x3E1`, the C1 gate
/// of the `juui1` hand-off P2[20] the story takes. Flags no gate reads are
/// left alone - a long cutscene sets and clears many a scratch flag the
/// anchor never shows.
pub(super) fn overreaching_records(
    session: &NativeSession,
    mf: &legaia_scene_formats::man_section::ManFile,
    man: &[u8],
    partition: usize,
) -> BTreeSet<usize> {
    use legaia_engine_field::man_field_scripts::{
        FlagBank, partition2_record_gates, walk_partition_gflag_sites,
    };
    let n2 = mf.partitions.get(2).map_or(0, Vec::len);
    let latches: BTreeSet<u16> = (0..n2)
        .filter_map(|r| partition2_record_gates(mf, man, r))
        .flat_map(|(c1, _)| c1)
        .collect();
    let latching = |part: usize| -> BTreeSet<usize> {
        let sites = walk_partition_gflag_sites(mf, man, part);
        // A latch the record lowers again itself is a hold for its own
        // span, not a latch: `nilboa` P2[20] raises `0x00F` (the C1 gate of
        // the stepping-stone hops P2[8] / P2[9]) for the jump onto the
        // challenge ledge and clears it on landing, before it spawns the
        // Delilas family's challenge P2[21].
        // Only a record that carries the party along a chain of arcs (two
        // or more op `0x43` arcs on the `0xF8` channel: `nilboa`'s stepping
        // stones to the Koru ledge) is exempted. A stair band (`dohaty` P2[3..8]) holds `0x00F` the same
        // way around its walk legs, and a single-arc ledge (`jouind`
        // P2[0] / P2[6..9]) around one drop; both play nothing, and walking
        // one strands the hand on the far side.
        let base =
            mf.partitions.first().map_or(0, Vec::len) + mf.partitions.get(1).map_or(0, Vec::len);
        let carries = |rec: usize| {
            use legaia_engine_field::man_field_scripts::partition_record_span;
            use legaia_scene_formats::field_disasm::{ActorCtrlKind, InsnInfo, LinearWalker};
            partition_record_span(mf, man, part, rec).is_some_and(|(start, pc0, len)| {
                LinearWalker::new(&man[start..start + len], pc0)
                    .flatten()
                    .filter(|i| {
                        i.extended == Some(0xF8)
                            && matches!(
                                i.info,
                                InsnInfo::ActorCtrl {
                                    kind: ActorCtrlKind::ArcJump { .. },
                                    ..
                                }
                            )
                    })
                    .count()
                    >= 2
            })
        };
        let lowered: BTreeSet<(usize, u16)> = sites
            .iter()
            .filter(|s| s.bank == FlagBank::System && s.kind == FlagKind::Clear)
            .filter(|s| carries(s.record))
            .map(|s| (s.record, s.flag))
            .collect();
        // A record that also raises a still-clear flag the next anchor
        // carries is the beat retail played, and its latch sits on another
        // branch: `concnow` P2[15], the third vision, raises `0x3EF` on the
        // visit the story takes and jumps past the `0x3D2` arm a later
        // visit plays.
        let w = &session.host.world;
        // A decode-coherent site counts even where its two bytes also read
        // as text (`nilboa` P2[25]'s `55 6D`, "Um"): the record's dialogue
        // is `0x1F`-framed, so the walk is past it.
        let wanted_by = |part: usize| -> BTreeSet<usize> {
            walk_partition_gflag_sites(mf, man, part)
                .iter()
                .filter(|s| {
                    s.bank == FlagBank::System
                        && s.kind == FlagKind::Set
                        && s.clean
                        && !s.debug_menu
                        && !w.system_flag_test(s.flag)
                        && NEXT_ANCHOR_FLAGS
                            .with(|a| a.borrow().as_ref().is_some_and(|f| f.contains(&s.flag)))
                })
                .map(|s| s.record)
                .collect()
        };
        let wanted_here = wanted_by(part);
        let wanted2 = wanted_by(2);
        // The chain counts: what the record spawns (three levels), and the
        // sequel the entry script spawns on a flag one of them raises
        // before a fight (`nilboa` P2[20] spawns the challenge P2[21],
        // whose `0x47A` has P1[0] spawn P2[25], which raises the `0x56D`
        // the next anchor carries).
        let set_by = |part: usize, rec: usize| -> BTreeSet<u16> {
            walk_partition_gflag_sites(mf, man, part)
                .iter()
                .filter(|s| {
                    s.record == rec && s.bank == FlagBank::System && s.kind == FlagKind::Set
                })
                .map(|s| s.flag)
                .collect()
        };
        let played: BTreeSet<usize> = sites
            .iter()
            .map(|s| s.record)
            .collect::<BTreeSet<usize>>()
            .into_iter()
            .filter(|&rec| {
                if wanted_here.contains(&rec) {
                    return true;
                }
                let spawned = spawned_p2(mf, man, part, rec, base, 3);
                let mut chain: Vec<(usize, usize)> = vec![(part, rec)];
                chain.extend(spawned.iter().map(|&r| (2, r)));
                spawned.iter().any(|r| wanted2.contains(r))
                    || chain.iter().any(|&(p, r)| {
                        entry_spawns_on(mf, man, &set_by(p, r))
                            .iter()
                            .any(|q| wanted2.contains(q))
                    })
            })
            .collect();
        sites
            .into_iter()
            .filter(|s| !played.contains(&s.record))
            .filter(|s| !lowered.contains(&(s.record, s.flag)))
            // Nor is a flag the record hands to the scene it leaves for,
            // whose entry script consumes it: `concnow` P2[18], the way into
            // Conkram's past, raises `0x423` and changes scene to `conc2`,
            // whose P1[0] tests and clears it.
            .filter(|s| !handoff_cleared(session, mf, man, part, s.record).contains(&s.flag))
            .filter(|s| {
                s.bank == FlagBank::System
                    && s.kind == FlagKind::Set
                    && s.clean
                    && !s.text_alias
                    && !s.debug_menu
                    && latches.contains(&s.flag)
                    && !next_anchor_has(s.flag)
            })
            .inspect(|s| {
                if std::env::var_os("LEGAIA_FGL_TRACE").is_some() {
                    eprintln!(
                        "    [latch] P{part}[{}] sets latch {:#05X}",
                        s.record, s.flag
                    );
                }
            })
            .map(|s| s.record)
            .collect()
    };
    let mut out = latching(partition);
    // A record also overreaches through what it spawns: `conc2` P2[11], the
    // King's audience, spawns that P2[12] as its epilogue.
    let latching2 = if partition == 2 {
        out.clone()
    } else {
        latching(2)
    };
    let n0 = mf.partitions.first().map_or(0, Vec::len);
    let n1 = mf.partitions.get(1).map_or(0, Vec::len);
    let n_part = mf.partitions.get(partition).map_or(0, Vec::len);
    for r in 0..n_part {
        if spawned_p2(mf, man, partition, r, n0 + n1, 3)
            .iter()
            .any(|s| latching2.contains(s))
        {
            out.insert(r);
        }
    }
    out
}

thread_local! {
    /// The system flags each scene's entry script (partition 1 record 0)
    /// clears, by scene name.
    pub(super) static ENTRY_CLEARS: std::cell::RefCell<HashMap<String, BTreeSet<u16>>> =
        Default::default();
}

/// The flags record `(part, rec)` raises for the scene it changes to: those
/// the entry scripts of its `0x3F` destinations (other than this scene)
/// clear.
pub(super) fn handoff_cleared(
    session: &NativeSession,
    mf: &legaia_scene_formats::man_section::ManFile,
    man: &[u8],
    part: usize,
    rec: usize,
) -> BTreeSet<u16> {
    use legaia_engine_field::man_field_scripts::{
        FlagBank, partition_record_span, walk_partition_gflag_sites,
    };
    use legaia_scene_formats::field_disasm::{InsnInfo, LinearWalker, scene_change_name};
    let Some((start, pc0, len)) = partition_record_span(mf, man, part, rec) else {
        return BTreeSet::new();
    };
    let body = &man[start..start + len];
    let here = scene_name(session);
    let dests: BTreeSet<String> = LinearWalker::new(body, pc0)
        .flatten()
        .filter(|i| matches!(i.info, InsnInfo::SceneChange { .. }))
        .filter_map(|i| scene_change_name(body, &i))
        .map(|d| d.to_ascii_lowercase())
        .filter(|d| *d != here)
        .collect();
    let index = &session.host.index;
    dests
        .iter()
        .flat_map(|d| {
            ENTRY_CLEARS.with(|c| {
                c.borrow_mut()
                    .entry(d.clone())
                    .or_insert_with(|| {
                        let Some(dman) = Scene::load(index, d)
                            .ok()
                            .and_then(|sc| sc.field_man_payload(index).ok().flatten())
                        else {
                            return BTreeSet::new();
                        };
                        let Ok(dmf) = legaia_scene_formats::man_section::parse(&dman) else {
                            return BTreeSet::new();
                        };
                        walk_partition_gflag_sites(&dmf, &dman, 1)
                            .iter()
                            .filter(|s| {
                                s.record == 0
                                    && s.bank == FlagBank::System
                                    && s.kind == FlagKind::Clear
                            })
                            .map(|s| s.flag)
                            .collect()
                    })
                    .clone()
            })
        })
        .collect()
}

/// Did the retail run leave boss stager placement `p1_record` alone before
/// the next milestone? Of the system flags its own record cleanly SETs and
/// no record of the scene ever CLEARs - flags that, once set, would still
/// show - the next anchor carries none. `town0b` P1[36], a loss-allowed
/// fight (`50 00` then `3E FF 03`), raises `0x5C0` / `0x5C1` before its
/// fight and the Hunter's Spring anchor has both clear; `town01` P1[10],
/// Tetsu's spar, is played for the `0x22E` its record also raises.
pub(super) fn stager_overreaches(
    mf: &legaia_scene_formats::man_section::ManFile,
    man: &[u8],
    p1_record: usize,
) -> bool {
    record_overreaches(mf, man, 1, p1_record, false)
}

/// [`stager_overreaches`] for any record: of the system flags record
/// `(partition, record)` cleanly SETs and no record of the scene ever
/// CLEARs, the next anchor carries none. A walk-on band that stages a fight
/// is held to it as a stager is: `deene` P2[18] raises `0x6B5` and installs
/// formation 11 (a 64800 HP fight), and no anchor from `zora_castle` on
/// carries `0x6B5` (cleared again by the entry script) or the `0x6B6` of
/// P2[19], the sequel the entry script spawns on it after the fight.
pub(super) fn record_overreaches(
    mf: &legaia_scene_formats::man_section::ManFile,
    man: &[u8],
    partition: usize,
    record: usize,
    follow_fight: bool,
) -> bool {
    use legaia_engine_field::man_field_scripts::{FlagBank, walk_partition_gflag_sites};
    let sites: Vec<_> = (0..3)
        .flat_map(|p| walk_partition_gflag_sites(mf, man, p))
        .filter(|s| s.bank == FlagBank::System && s.clean && !s.text_alias && !s.debug_menu)
        .collect();
    let cleared: BTreeSet<u16> = sites
        .iter()
        .filter(|s| s.kind == FlagKind::Clear)
        .map(|s| s.flag)
        .collect();
    let mut records = vec![(partition, record)];
    if follow_fight {
        // The fight's sequel: the entry script's post-battle return spawns
        // the record a pending flag of this one names.
        let pending: BTreeSet<u16> = sites
            .iter()
            .filter(|s| s.partition == partition && s.record == record && s.kind == FlagKind::Set)
            .map(|s| s.flag)
            .collect();
        records.extend(
            entry_spawns_on(mf, man, &pending)
                .into_iter()
                .map(|r| (2, r)),
        );
    }
    let lasting: BTreeSet<u16> = sites
        .iter()
        .filter(|s| {
            records.contains(&(s.partition, s.record))
                && s.kind == FlagKind::Set
                && s.flag != 0
                && !cleared.contains(&s.flag)
        })
        .map(|s| s.flag)
        .collect();
    !lasting.is_empty() && !lasting.iter().any(|&f| next_anchor_has(f))
}

/// The partition-2 records the scene's entry script (partition 1 record 0)
/// spawns right behind a test of one of `flags`: the post-battle hand-off a
/// pending flag set before `3E FF` arms (`deene` P1[0]: `76 B5`, `66 B5`,
/// `44 27`).
pub(super) fn entry_spawns_on(
    mf: &legaia_scene_formats::man_section::ManFile,
    man: &[u8],
    flags: &BTreeSet<u16>,
) -> BTreeSet<usize> {
    use legaia_engine_field::man_field_scripts::partition_record_span;
    use legaia_scene_formats::field_disasm::{FlagKind as Kind, InsnInfo, LinearWalker};
    let mut out = BTreeSet::new();
    let Some((start, pc0, len)) = partition_record_span(mf, man, 1, 0) else {
        return out;
    };
    let base = mf.partitions.first().map_or(0, Vec::len) + mf.partitions.get(1).map_or(0, Vec::len);
    let n2 = mf.partitions.get(2).map_or(0, Vec::len);
    let mut armed = 0usize;
    for insn in LinearWalker::new(&man[start..start + len], pc0).flatten() {
        match insn.info {
            InsnInfo::SystemFlag {
                kind: Kind::Test,
                idx,
                ..
            } => armed = if flags.contains(&idx) { 4 } else { 0 },
            InsnInfo::SpawnRecord { global_index } if armed > 0 => {
                if let Some(r) = usize::from(global_index).checked_sub(base)
                    && r < n2
                {
                    out.insert(r);
                }
                armed = 0;
            }
            _ => armed = armed.saturating_sub(1),
        }
    }
    out
}

/// The live walk-on tiles of the loaded field scene whose record stages a
/// fight ([`record_stages_fight`]).
pub(super) fn fight_band_tiles(session: &NativeSession) -> HashSet<(i32, i32)> {
    use legaia_engine_field::man_field_scripts::partition2_record_gates;
    let Some((mf, man, triggers)) = scene_man_and_triggers(session) else {
        return HashSet::new();
    };
    let w = &session.host.world;
    let mut fights: HashMap<u8, bool> = HashMap::new();
    triggers
        .iter()
        .filter(|t| t.gate == 1)
        .filter(|t| {
            *fights.entry(t.record).or_insert_with(|| {
                partition2_record_gates(&mf, &man, usize::from(t.record))
                    .is_none_or(|(c1, c2)| w.p2_record_gates_pass(&c1, &c2))
                    && record_stages_fight(&mf, &man, 2, usize::from(t.record))
            })
        })
        .map(|t| (i32::from(t.tile_x), i32::from(t.tile_z)))
        .collect()
}

/// Whether record `(part, rec)`, or a partition-2 record it spawns, installs
/// a scripted battle (op `0x3E` with `op0 == 0xFF` or `< 100`).
pub(super) fn record_stages_fight(
    mf: &legaia_scene_formats::man_section::ManFile,
    man: &[u8],
    part: usize,
    rec: usize,
) -> bool {
    use legaia_engine_field::man_field_scripts::partition_record_span;
    use legaia_scene_formats::field_disasm::{InsnInfo, LinearWalker};
    let fights = |part: usize, rec: usize| {
        partition_record_span(mf, man, part, rec).is_some_and(|(start, pc0, len)| {
            LinearWalker::new(&man[start..start + len], pc0)
                .flatten()
                .any(|i| matches!(i.info, InsnInfo::WarpOrInteract { is_warp: false, .. }))
        })
    };
    let n0 = mf.partitions.first().map_or(0, Vec::len);
    let n1 = mf.partitions.get(1).map_or(0, Vec::len);
    fights(part, rec)
        || spawned_p2(mf, man, part, rec, n0 + n1, 3)
            .into_iter()
            .any(|r| fights(2, r))
}

/// The formation rows record `(part, rec)` installs (op `0x3E` with
/// `op0 == 0xFF` or `< 100`), its own and those of the partition-2 records
/// it spawns (three levels).
pub(super) fn staged_formation_rows(
    mf: &legaia_scene_formats::man_section::ManFile,
    man: &[u8],
    part: usize,
    rec: usize,
) -> BTreeSet<u16> {
    use legaia_engine_field::man_field_scripts::partition_record_span;
    use legaia_scene_formats::field_disasm::{InsnInfo, LinearWalker};
    let rows = |part: usize, rec: usize| -> Vec<u16> {
        partition_record_span(mf, man, part, rec).map_or_else(Vec::new, |(start, pc0, len)| {
            LinearWalker::new(&man[start..start + len], pc0)
                .flatten()
                .filter_map(|i| match i.info {
                    InsnInfo::WarpOrInteract {
                        op1,
                        is_warp: false,
                        ..
                    } => Some(u16::from(op1)),
                    _ => None,
                })
                .collect()
        })
    };
    let n0 = mf.partitions.first().map_or(0, Vec::len);
    let n1 = mf.partitions.get(1).map_or(0, Vec::len);
    let mut out: BTreeSet<u16> = rows(part, rec).into_iter().collect();
    for r in spawned_p2(mf, man, part, rec, n0 + n1, 3) {
        out.extend(rows(2, r));
    }
    out
}

/// Whether record `(part, rec)` runs the mode-24 minigame door-warp
/// (op `0x3E` with `op0 >= 100`, a venue cabinet's entry).
pub(super) fn record_enters_minigame(
    mf: &legaia_scene_formats::man_section::ManFile,
    man: &[u8],
    part: usize,
    rec: usize,
) -> bool {
    use legaia_engine_field::man_field_scripts::partition_record_span;
    use legaia_scene_formats::field_disasm::{InsnInfo, LinearWalker};
    let Some((start, pc0, len)) = partition_record_span(mf, man, part, rec) else {
        return false;
    };
    LinearWalker::new(&man[start..start + len], pc0)
        .flatten()
        .any(|i| matches!(i.info, InsnInfo::WarpOrInteract { is_warp: true, .. }))
}

/// Whether talk record `P1[rec]` is a **hand-off**: it sets a system flag
/// the live state does not carry and changes scene, and the destination's
/// scene-entry script (its `P1[0]`) tests that flag. Such a talk is a story
/// beat whose effect lands in another scene, so no flag it writes itself is
/// one the next anchor shows: `suimon`'s Water Gate Controller (`P1[4]`)
/// sets `0x2C6` and enters `map01` at `(0, 0)`; `map01`'s `P1[0]` sees
/// `0x2C6` and spawns the drain cutscene `P2[15]`, which trades it for
/// `0x2C7` and returns the party to the drained chamber, where `suimon`'s
/// `P1[0]` trades that for `0x27B` - the flag that joins the two chambers.
pub(super) fn hands_off_to_entry_script(
    session: &NativeSession,
    mf: &legaia_scene_formats::man_section::ManFile,
    man: &[u8],
    rec: usize,
) -> bool {
    use legaia_engine_field::man_field_scripts::{
        FlagBank, partition_record_span, walk_partition_gflag_sites,
    };
    use legaia_scene_formats::field_disasm::{InsnInfo, LinearWalker, scene_change_name};
    let w = &session.host.world;
    let sets: BTreeSet<u16> = walk_partition_gflag_sites(mf, man, 1)
        .iter()
        .filter(|s| {
            s.record == rec
                && s.bank == FlagBank::System
                && s.kind == FlagKind::Set
                && s.clean
                && !s.text_alias
                && !s.debug_menu
                && !w.system_flag_test(s.flag)
        })
        .map(|s| s.flag)
        .collect();
    if sets.is_empty() {
        return false;
    }
    let Some((start, pc0, len)) = partition_record_span(mf, man, 1, rec) else {
        return false;
    };
    let body = &man[start..start + len];
    let here = scene_name(session);
    let dests: BTreeSet<String> = LinearWalker::new(body, pc0)
        .flatten()
        .filter(|i| matches!(i.info, InsnInfo::SceneChange { .. }))
        .filter_map(|i| scene_change_name(body, &i))
        .filter(|d| *d != here)
        .collect();
    let index = &session.host.index;
    dests.iter().any(|d| {
        let Some(dman) = Scene::load(index, d)
            .ok()
            .and_then(|sc| sc.field_man_payload(index).ok().flatten())
        else {
            return false;
        };
        let Ok(dmf) = legaia_scene_formats::man_section::parse(&dman) else {
            return false;
        };
        walk_partition_gflag_sites(&dmf, &dman, 1).iter().any(|s| {
            s.record == 0
                && s.bank == FlagBank::System
                && s.kind == FlagKind::Test
                && sets.contains(&s.flag)
        })
    })
}

/// The partition-2 records `(part, rec)` spawns through op `0x44`
/// (`SPAWN_RECORD`, global index `base + r`), followed `depth` levels.
pub(super) fn spawned_p2(
    mf: &legaia_scene_formats::man_section::ManFile,
    man: &[u8],
    part: usize,
    rec: usize,
    base: usize,
    depth: usize,
) -> BTreeSet<usize> {
    use legaia_engine_field::man_field_scripts::partition_record_span;
    use legaia_scene_formats::field_disasm::{InsnInfo, LinearWalker};
    let mut out = BTreeSet::new();
    let Some((start, pc0, len)) = partition_record_span(mf, man, part, rec) else {
        return out;
    };
    let body = &man[start..start + len];
    for insn in LinearWalker::new(body, pc0).flatten() {
        if let InsnInfo::SpawnRecord { global_index } = insn.info
            && let Some(r2) = usize::from(global_index).checked_sub(base)
            && r2 < mf.partitions.get(2).map_or(0, Vec::len)
            && out.insert(r2)
            && depth > 1
        {
            out.extend(spawned_p2(mf, man, 2, r2, base, depth - 1));
        }
    }
    out
}
