//! Parts D, E and E2: the five doors, which decoder sees each, and how those scenes exit.

use super::*;

// ---------------------------------------------------------------------------
// Part D: which decoder can see which of the five's doors
// ---------------------------------------------------------------------------

/// Where the five late chapter-1 rooms' doors sit relative to the decoders,
/// which is a statement about the decoders and the bytes and **not** about
/// whether a player can leave (Part E answers that, and the answer is yes for
/// four of the five).
///
/// Three decoders read the same MAN and they disagree:
///
/// - [`legaia_scene_formats::man_edit::scene_change_sites`] - a **clean** per-partition
///   fall-through walk from each record's true `pc0`, stopping at the first
///   desync. This is what a door *op* looks like.
/// - [`legaia_scene_formats::man_edit::partition1_destinations`] - the **recovering**
///   walk that also reaches the destination-*table* blob some controllers
///   append past their last partition-1 record. This is what a door *entry*
///   looks like.
/// - The ladder's own exit-site join (`.MAP` / `.PCH` gate-1 trigger ->
///   partition-2 record -> `0x3F`), which is what a door the player can *walk
///   onto* looks like.
///
/// Ordinary chapter-1 interiors have all three: `town01`, `keikoku`, `jouina`,
/// `jouinb` all carry partition-2 `0x3F` ops the clean walk sees, and so does
/// `uru` - its `MAP03` exit record `P2[42]` is 41 bytes of pure choreography
/// (flag test, `B1 F8 13`, white fade, the `0x3F`, the park pair) with no text
/// to desync on.
///
/// `urudre1` / `urudre2` / `urudre3` have both, and they agree: each exit
/// sits `0x2DC` / `0x124C` / `0x2034` bytes into a partition-2 record, past
/// kilobytes of inline `0x1F` text pages, which the clean walk once could not
/// cross (a bare `0x1F` read as an unknown opcode and ended the walk at the
/// record's first line), so for a while their destinations came from the
/// destination-table pass alone and those three BFS edges rested on a weaker
/// footing. With text segments decoded as strides of the stream the clean
/// walk reaches the same `0x3F` the table names, and Part E walks onto two
/// of the doors.
///
/// `jouine` is a fourth shape: no decoder finds a named scene change, because
/// there is none. Its exit is the FMV hand-off op `4C E2 08` at MAN `0x03E90`,
/// and control returns through
/// [`legaia_engine_system::movie::cutscene::fmv_post_play_handoff`], not through the
/// field VM's `0x3F` arm.
#[test]
fn part_d_which_decoder_sees_each_of_the_five_doors() {
    let Some(host) = open_host() else {
        return;
    };
    /// `(clean-walk sites as (partition, name), destination-table names)`.
    fn decoders(host: &SceneHost, name: &str) -> (Vec<(usize, String)>, Vec<String>) {
        let (mf, man) = scene_man(&host.index, name).unwrap_or_else(|| panic!("{name}: no MAN"));
        let clean = legaia_scene_formats::man_edit::scene_change_sites(&man)
            .into_iter()
            .map(|s| (s.partition, s.name))
            .collect();
        let table = legaia_scene_formats::man_edit::partition1_destinations(&mf, &man)
            .into_iter()
            .map(|d| d.scene_name)
            .collect();
        (clean, table)
    }

    for name in [
        "uru", "urudre1", "urudre2", "urudre3", "jouine", "town01", "keikoku", "jouina", "jouinb",
    ] {
        let (clean, table) = decoders(&host, name);
        eprintln!("[doors] {name:<8} clean-walk ops {clean:?} | destination table {table:?}");
    }

    // The three dream rooms: the clean walk crosses the text pages and finds
    // the partition-2 `0x3F`, and the destination table names the same
    // scene - two decoders, one answer.
    for (name, dest) in [("urudre1", "uru"), ("urudre2", "map01"), ("urudre3", "uru")] {
        let (clean, table) = decoders(&host, name);
        assert_eq!(
            clean,
            vec![(2, dest.to_string())],
            "{name}'s clean-walk door is the partition-2 op naming {dest}"
        );
        assert_eq!(
            table,
            vec![dest.to_string()],
            "{name}'s destination table names the same scene"
        );
    }

    // The contrast that makes the above discriminating rather than a property
    // of the decoder: ordinary interiors DO carry partition-2 ops the clean
    // walk reaches, and it is those the join reads. `uru` is in this set - its
    // door record is short enough that the clean walk gets to the tail.
    for name in ["town01", "keikoku", "jouina", "jouinb", "uru"] {
        let (clean, _) = decoders(&host, name);
        assert!(
            clean.iter().any(|(p, _)| *p == 2),
            "{name}'s doors are partition-2 records; got {clean:?}"
        );
    }
    let (uru_clean, _) = decoders(&host, "uru");
    assert!(
        uru_clean.iter().any(|(p, n)| *p == 2 && n == "map03"),
        "uru's decoded partition-2 door is the MAP03 exit; got {uru_clean:?}"
    );

    // `jouine`: neither decoder sees anything, because its exit is not a named
    // scene change. The MAN does carry the FMV trigger the exit really is.
    let (clean, table) = decoders(&host, "jouine");
    assert!(
        clean.is_empty() && table.is_empty(),
        "jouine carries no named-scene-change at all; got {clean:?} / {table:?}"
    );
    let (mf, man) = scene_man(&host.index, "jouine").expect("jouine MAN");
    let fmvs = legaia_engine_field::man_field_scripts::scene_fmv_triggers(&mf, &man);
    assert!(
        fmvs.iter().any(|t| t.fmv_id == 8),
        "jouine's exit is the `4C E2 08` FMV trigger; decoded triggers {fmvs:?}"
    );
    eprintln!(
        "[ok] Part D: uru's and urudre1/2/3's doors are decoded P2 ops the \
         destination table agrees with; jouine's is an FMV trigger, not a 0x3F"
    );
}

// ---------------------------------------------------------------------------
// Part E: do the five actually let a player out?
// ---------------------------------------------------------------------------

/// The exit rung's join is a decoder, and a decoder that finds nothing says
/// nothing about whether a player standing in one of these rooms can leave.
/// So step onto **every** gate-1 trigger tile each of the five carries - the
/// `.MAP` table and the `.PCH` sidecar both, in the order the runtime lookup
/// searches them - one fresh visit per tile, and report what fires.
///
/// This is the same walk-on dispatch a player's own tile crossing runs
/// (`FUN_801D1EC4` -> `FUN_801D5630` -> `FUN_8003BDE0`); only the choice of
/// tile is synthetic, and the pad pulse that pages the record's conversation
/// stands in for a player pressing Confirm.
///
/// All five leave, each to the destination the disc names:
///
/// | scene | band | record | tail | lands in |
/// |---|---|---|---|---|
/// | `uru` | `(36..39, 5)` (`.PCH` rows 23..26) | `P2[42]` | `0x3F` | `map03` |
/// | `urudre1` | `(35..37, 22..24)` | `P2[2]` | `0x3F` | `uru` |
/// | `urudre2` | gate-1 band -> `P2[9]` | `0x3F` | `map01` |
/// | `urudre3` | `(51, 90)` | `P2[0]` | `0x3F` | `uru` |
/// | `jouine` | `(17, 17..19)` (`.PCH` rows 3..5) | `P2[16]` | `4C E2 08` | `town0e` (FMV 8) |
///
/// Three things had to change for that to be measurable, and each was a cap of
/// this test rather than a property of the disc: the tile sweep stopped at 48
/// deduplicated tiles while `uru`'s exit band sits at positions 63..66 of its
/// own 118; the post-step budget was 24 ticks while `urudre1`'s record alone
/// waits 240 + 60 + 60 frames before its `0x3F`; and `jouine`'s tail is not a
/// `0x3F` at all, so watching only for `SceneEntered` could never see it.
///
/// `urudre2` joins them through `P2[9]`'s tail `0x3F` -> `map01`; the row
/// below records the op-`0x45` misreading that used to keep it in.
#[test]
fn part_e_the_five_leave_through_their_pch_bands() {
    let Some(mut host) = open_host() else {
        return;
    };
    let base = FlagBaseline::snapshot(&host);

    /// Every gate-1 trigger tile of `name`, deduplicated, `.MAP` rows then
    /// `.PCH` rows - the order [`Scene::field_tile_triggers`] returns and the
    /// order retail's per-tile lookup searches.
    fn walk_on_tiles(host: &SceneHost, name: &str) -> Vec<(u8, u8)> {
        let Ok(scene) = Scene::load(&host.index, name) else {
            return Vec::new();
        };
        let Ok((primary, fallback)) = scene.field_tile_triggers(&host.index) else {
            return Vec::new();
        };
        let mut seen: BTreeSet<(u8, u8)> = BTreeSet::new();
        primary
            .into_iter()
            .chain(fallback)
            .filter(|t| t.gate == 1)
            .filter_map(|t| {
                seen.insert((t.tile_x, t.tile_z))
                    .then_some((t.tile_x, t.tile_z))
            })
            .take(MAX_TILES_SWEPT)
            .collect()
    }

    /// `(scene, expected destination)`. `None` = this scene does not leave in
    /// the port as it stands, and the row says why.
    const EXPECTED: [(&str, Option<&str>); 5] = [
        // `.PCH` rows 23..26, band (36..39, 5) -> P2[42] -> `0x3F` MAP03.
        ("uru", Some("map03")),
        // band (35..37, 22..24) -> P2[2] -> `0x3F` uru, behind 360 frames of
        // authored WaitFrames.
        ("urudre1", Some("uru")),
        // `P2[9]`, a 4703-byte King Nebular dream whose `0x3F` -> `map01` is
        // its very tail (body `0x124C`). What used to stop the port ~0x670
        // bytes short of that tail was the field VM's op-`0x45` sub-`0xC0`
        // CAMERA arm: the record's `45 C0 00 00` at body `0x0BDB` was read as
        // an absolute jump to the operand `s16`, so a trigger of zero sent the
        // PC back to byte 0 and the conversation replayed forever. Retail's arm
        // is a four-byte fall-through (`overlay_0897` `0x801DF210`, exit
        // `addiu s8, s8, 4`) and the `s16` is the apply trigger.
        ("urudre2", Some("map01")),
        // band (51, 90) -> P2[0] -> `0x3F` uru; ~14.9k ticks of record.
        ("urudre3", Some("uru")),
        // band (17, 17..19), `.PCH` rows 3..5 -> P2[16] -> `4C E2 08` -> FMV 8
        // -> `fmv_post_play_handoff(8)` -> town0e, door 0x2E5.
        ("jouine", Some("town0e")),
    ];

    let mut fired_by_scene: BTreeMap<&str, BTreeMap<String, usize>> = BTreeMap::new();
    let mut tried_total = 0usize;
    for (name, expected) in EXPECTED {
        let tiles = walk_on_tiles(&host, name);
        let fired = fired_by_scene.entry(name).or_default();
        // Re-enter only when the previous tile changed the scene or left a
        // record running. A scene load is by far the expensive step here and a
        // tile that fires nothing leaves the world unchanged but for the
        // player's position, so one entry covers a whole run of quiet tiles.
        let mut need_entry = true;
        for (tx, tz) in &tiles {
            if need_entry {
                if !enter(&mut host, name, &base) || !matches!(settle(&mut host), Settle::Released)
                {
                    continue;
                }
                need_entry = false;
            }
            tried_total += 1;
            step_onto_tile(&mut host, *tx, *tz);
            if let Some(hit) = run_to_transition(&mut host, DEEP_EXIT_TICKS) {
                let label = match &hit {
                    Fired::Scene(s) => s.clone(),
                    Fired::Fmv { id, scene } => {
                        format!("fmv{id}->{}", scene.as_deref().unwrap_or("(no hand-off)"))
                    }
                };
                *fired.entry(label).or_default() += 1;
                need_entry = true;
                // The destination is what the row claims; one hit per scene is
                // the claim, so stop sweeping this scene's remaining tiles.
                if hit.scene() == expected {
                    break;
                }
                continue;
            }
            // A tile that spawned a record without warping has to drain before
            // the next tile is a clean test.
            if !matches!(settle(&mut host), Settle::Released) {
                need_entry = true;
            }
        }
        match expected {
            Some(dest) => eprintln!(
                "[exit]   {name:<8} {} walk-on tile(s) -> {fired:?} (expected {dest})",
                tiles.len()
            ),
            None => eprintln!(
                "[stuck]  {name:<8} {} walk-on tile(s) -> {fired:?} (no exit expected in-port)",
                tiles.len()
            ),
        }
    }

    // The sweep short-circuits a scene as soon as its expected exit fires, so
    // this is a floor on the work actually done rather than the tile count -
    // and now that every one of the five leaves, the count is the sum of five
    // prefixes rather than of one 186-tile scan that found nothing. Each scene
    // must still have stepped onto at least one tile for its row to mean
    // anything, which is the second assert.
    assert!(
        tried_total >= 40,
        "the sweep must actually step onto tiles to mean anything; tried {tried_total}"
    );
    for (name, _) in EXPECTED {
        assert!(
            !walk_on_tiles(&host, name).is_empty(),
            "{name} must carry at least one gate-1 walk-on tile to step onto"
        );
    }
    // The four that leave, and where to. This is what makes the part
    // non-vacuous in the regression direction: if `Scene::field_tile_triggers`
    // stopped reading the `.PCH` fallback half, `uru` / `urudre1` / `urudre3` /
    // `jouine` would each have zero gate-1 bands left to step onto and every
    // one of these would fail.
    for (name, expected) in EXPECTED {
        let fired = &fired_by_scene[name];
        match expected {
            Some(dest) => {
                let want = if name == "jouine" {
                    format!("fmv8->{dest}")
                } else {
                    dest.to_string()
                };
                assert!(
                    fired.contains_key(&want),
                    "{name} must leave for {want} through a gate-1 walk-on band; \
                     what fired: {fired:?}"
                );
            }
            None => assert!(
                fired.is_empty(),
                "{name} now leaves ({fired:?}) - the port gap named on its EXPECTED \
                 row has closed; drop the row's NOT EXITABLE HEADLESS note and give \
                 it its destination"
            ),
        }
    }
    eprintln!(
        "[ok] Part E: all five leave through a `.PCH`-carried gate-1 band \
         ({tried_total} tiles stepped)"
    );
}

/// The sibling question, asked without the `.MAP` / `.PCH` join: do those
/// scenes' **own record bodies** reach an exit when the field VM runs them?
///
/// Part E drives the walk-on dispatch, so a failure there could be the
/// dispatch, the trigger tables, or the record. This loads each partition-1 and
/// partition-2 record straight into the VM - the way `minigame_replay` arms a
/// venue door - and reports which reach a scene change or an FMV trigger. What
/// it can prove is that the bytes contain a reachable exit; it cannot prove a
/// player gets there, which is Part E's job.
///
/// All five carry one. `jouine`'s is the deepest: its exit record is `P2[16]`
/// of 18 and needs the deep budget, which is why
/// [`DEEP_RERUNS_PER_SCENE`] has to cover a whole partition rather than a
/// sample of it.
#[test]
fn part_e2_do_those_scenes_carry_a_record_that_exits_at_all() {
    let Some(mut host) = open_host() else {
        return;
    };
    let base = FlagBaseline::snapshot(&host);
    let mut exiting: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
    let mut ran_total = 0usize;

    for name in ["uru", "urudre1", "urudre2", "urudre3", "jouine"] {
        let Some((mf, man)) = scene_man(&host.index, name) else {
            continue;
        };
        let mut need_entry = true;
        let mut deep_reruns = 0usize;
        // Partition 2 first: door records live there, so the scenes that do
        // carry an exit answer early instead of after forty partition-1
        // placement scripts.
        for partition in [2usize, 1] {
            let count = mf.header.partition_counts[partition].max(0) as usize;
            for record in 0..count.min(MAX_RECORDS_RUN) {
                if legaia_engine_field::man_field_scripts::partition_record_span(
                    &mf, &man, partition, record,
                )
                .is_none()
                {
                    continue;
                }
                // Partition-2 records - the door records this part exists to
                // find - get a FRESH scene entry each. Resetting the flag banks
                // and the contexts between them is not the same visit: sixteen
                // preceding records leave the scene's actor channels, camera
                // and NPC seats where they parked, and `jouine`'s exit record
                // (`P2[16]` of 18) terminates early in that state while it
                // reaches its `4C E2 08` tail from a clean entry. Partition 1
                // keeps the cheap shared-entry walk: those are placement
                // scripts, and a scene load per record turns this into a
                // ten-minute run.
                if partition == 2 {
                    need_entry = true;
                }
                if need_entry {
                    if !enter(&mut host, name, &base) {
                        continue;
                    }
                    need_entry = false;
                } else {
                    // Reset only the state a previous record could have left
                    // behind - the flag banks, its spawned contexts, its own
                    // timeline and any dialogue it opened. A scene load per
                    // record turns this into a quarter-hour run.
                    base.restore(&mut host);
                    host.world.field_vm.helper_contexts.clear();
                    host.world.cutscene.timeline = None;
                    host.world.dialog.current = None;
                    host.world.dialog.inline = None;
                }
                ran_total += 1;
                // Install the record the way the walk-on dispatch installs one
                // (retail `FUN_8003BDE0` -> a spawned context), not as a raw
                // field script: a door record's tail is reached through the
                // timeline's parks - authored waits, channel handshakes, its
                // own conversation - and a bare script load does not have them.
                // The story-flag gates are deliberately NOT checked here; the
                // question is whether the bytes carry a reachable exit.
                host.world
                    .install_cutscene_timeline_record(&mf, &man, partition, record, false);
                // Two passes, because the record lengths here span three orders
                // of magnitude. Every record gets the ordinary budget; only a
                // record still RUNNING when that expires - a boss cutscene, a
                // multi-screen conversation - earns the deep one, and at most
                // [`DEEP_RERUNS_PER_SCENE`] of them per scene. Giving every
                // record the deep budget turned this part into a quarter-hour
                // run for no extra finding: the records that never reach an
                // exit are the ones that go idle in a few hundred ticks.
                let mut hit = run_to_transition(&mut host, RECORD_RUN_TICKS);
                if hit.is_none() && !world_idle(&host) && deep_reruns < DEEP_RERUNS_PER_SCENE {
                    deep_reruns += 1;
                    hit = run_to_transition(&mut host, DEEP_EXIT_TICKS);
                }
                if let Some(hit) = hit {
                    let label = match &hit {
                        Fired::Scene(s) => s.clone(),
                        Fired::Fmv { id, scene } => {
                            format!("fmv{id}->{}", scene.as_deref().unwrap_or("(no hand-off)"))
                        }
                    };
                    exiting.entry(name).or_default().insert(label);
                    need_entry = true;
                }
            }
        }
        eprintln!(
            "[records] {name:<8} -> {:?}",
            exiting.get(name).cloned().unwrap_or_default()
        );
    }

    assert!(
        ran_total > 40,
        "the probe must actually execute records; ran {ran_total}"
    );
    for name in ["uru", "urudre1", "urudre2", "urudre3", "jouine"] {
        assert!(
            exiting.contains_key(name),
            "{name} must carry a record the field VM can run to an exit; \
             scenes that exited: {:?}",
            exiting.keys().collect::<Vec<_>>()
        );
    }
    eprintln!(
        "[ok] Part E2: executed {ran_total} record(s); scenes whose own records \
         reach an exit: {:?}",
        exiting.keys().collect::<Vec<_>>()
    );
}
