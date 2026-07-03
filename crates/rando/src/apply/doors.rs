//! Scene-transition (door) + intra-town (house) door randomization.

use super::*;

/// The CDNAME scene label for a PROT entry, read from the disc's `CDNAME.TXT`
/// (or `"?"` when the map is unavailable). Returns the parsed map so callers can
/// label many entries without re-reading.
pub fn cdname_map(patcher: &DiscPatcher) -> std::collections::BTreeMap<u32, String> {
    patcher
        .read_named_file("CDNAME.TXT")
        .and_then(|b| String::from_utf8(b).ok())
        .and_then(|t| legaia_prot::cdname::parse_str(&t).ok())
        .unwrap_or_default()
}

/// One scene-transition ("door / exit") site: where it lives and where it goes.
/// This is the audit surface the door randomizer reassigns. `home_scene` is the
/// CDNAME label of the scene the door is in; `dest_scene` is the inline
/// destination name the `0x3F` op carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoorSite {
    /// PROT entry index of the scene bundle holding this door.
    pub entry_idx: usize,
    /// CDNAME label of the scene the door lives in (e.g. `"town01"`).
    pub home_scene: String,
    /// Byte offset of the `0x3F` op within the scene's decompressed MAN.
    pub op_pc: usize,
    /// Partition of the carrying record (almost always 2).
    pub partition: usize,
    /// Destination-scene `i16` index the op carries.
    pub index: i16,
    /// Destination CDNAME scene label (e.g. `"map01"`).
    pub dest_scene: String,
    /// Destination entry-tile X / Z bytes.
    pub entry_x: u8,
    pub entry_z: u8,
    /// Facing/depth selector byte.
    pub dir: u8,
}

/// Read every scene-transition door on the disc (the randomizable population),
/// in PROT-entry then op-offset order. Purely read-only; decodes each scene MAN
/// once via [`SceneDoors::locate`] and labels the home scene from `CDNAME.TXT`.
pub fn current_doors(patcher: &DiscPatcher) -> Result<Vec<DoorSite>> {
    let cd = cdname_map(patcher);
    let mut out = Vec::new();
    for idx in 0..patcher.entry_count() {
        let entry = patcher
            .read_entry(idx)
            .with_context(|| format!("read PROT entry {idx}"))?;
        let Some(doors) = SceneDoors::locate(&entry, idx) else {
            continue;
        };
        let home = legaia_prot::cdname::block_for(&cd, idx as u32)
            .unwrap_or("?")
            .to_string();
        for s in &doors.sites {
            out.push(DoorSite {
                entry_idx: idx,
                home_scene: home.clone(),
                op_pc: s.op_pc,
                partition: s.partition,
                index: s.index,
                dest_scene: s.name.clone(),
                entry_x: s.entry_x,
                entry_z: s.entry_z,
                dir: s.dir,
            });
        }
    }
    Ok(out)
}

/// A complete scene-transition destination descriptor (everything a `0x3F` op
/// carries). The atomic unit the door randomizer moves between sites: moving the
/// whole descriptor keeps the destination scene, its entry tile, and facing
/// internally consistent.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Dest {
    index: i16,
    name: Vec<u8>,
    entry_x: u8,
    entry_z: u8,
    dir: u8,
}

/// How scene-transition doors are reconnected.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DoorCoupling {
    /// Bidirectional: re-pair doors into two-way connections so walking through
    /// a door and turning around returns you the way you came. Doors that have
    /// no reverse partner (dead-end / one-way story warps) fall back to the
    /// decoupled assignment and are counted in [`DoorApplyReport::unpaired`].
    Coupled,
    /// One-way: every door's destination is reassigned independently, so going
    /// back through the destination's own doors is not guaranteed to return you.
    Decoupled,
}

/// Outcome of randomizing scene-transition doors.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DoorApplyReport {
    /// Scene bundles whose MAN was rewritten + written back.
    pub scenes_changed: usize,
    /// Total door sites whose destination changed.
    pub sites_changed: usize,
    /// Total door sites found (the randomizable population).
    pub sites_total: usize,
    /// Coupled mode only: door sites with no reverse partner (dead-end / one-way
    /// story warps, or doors orphaned by an unequal-direction connection), left
    /// at their original destination because they can't be made two-way.
    pub unpaired: usize,
    /// Coupled mode only: matched connections left at their original
    /// destination because one endpoint's scene couldn't be rebuilt (e.g. an
    /// overflowing overworld hub). Both ends are reverted so the connection
    /// stays genuinely two-way rather than half-applied (a one-way warp).
    pub coupled_kept_original: usize,
    /// Scene PROT-entry indices whose rebuilt MAN overflowed the compressed
    /// footprint or failed validation, so the scene kept its original doors.
    pub skipped: Vec<usize>,
}

/// Build a random involution over the door sites and assign each its new
/// destination so the connection is **bidirectional**: for matched sites `A` and
/// `B`, `A` is sent to where `B` is reached from (`dest(partner_orig(B))`) and
/// vice versa, so walking `A → B`'s doorstep lands you on `B`, whose new
/// destination is `A`'s doorstep. `partner_orig(X)` is the reverse door (same
/// scene-pair, opposite direction); sites without one - or the odd site left
/// over by an odd matching - are **left at their original destination**
/// (untouched) and returned as the `unpaired` count: a door with no clean reverse
/// can't be made two-way, so coupled mode leaves it vanilla rather than giving it
/// a one-way reassignment. `homes[i]` is the CDNAME label of site `i`'s home
/// scene.
///
/// `plan_doors_coupled`'s return: `(dest_new, unpaired_count, matched_pairs,
/// original_partner)`. See its doc for what each element means.
type CoupledPlan = (Vec<Dest>, usize, Vec<(usize, usize)>, Vec<Option<usize>>);

/// Also returns the matched `(a, b)` pairs (the new involution) and the original
/// `partner` array (each site's reverse door, if any). [`randomize_doors`] needs
/// both: a connection is only truly bidirectional if every site it touches gets
/// written, and a site's edit depends on **both** its matched partner and its
/// original reverse. When a scene can't be rebuilt (e.g. an overflowing overworld
/// hub), the revert must propagate transitively along both involutions, or a
/// one-way warp masquerading as coupled survives.
fn plan_doors_coupled(origs: &[Dest], homes: &[String], rng: &mut SplitMix64) -> CoupledPlan {
    use std::collections::HashMap;
    let n = origs.len();
    let name_of = |d: &Dest| String::from_utf8_lossy(&d.name).into_owned();

    // Group site indices by (home_scene, dest_scene) - both directions of a
    // connection live in mirror groups (a,b) / (b,a).
    let mut groups: HashMap<(String, String), Vec<usize>> = HashMap::new();
    for (i, d) in origs.iter().enumerate() {
        groups
            .entry((homes[i].clone(), name_of(d)))
            .or_default()
            .push(i);
    }
    let mut partner = vec![None; n];
    let mut done: std::collections::HashSet<(String, String)> = std::collections::HashSet::new();
    let keys: Vec<(String, String)> = groups.keys().cloned().collect();
    for key in keys {
        if done.contains(&key) {
            continue;
        }
        let (h, d) = key.clone();
        if h == d {
            // Self-scene connection: pair consecutive sites within the group.
            let g = &groups[&key];
            let mut it = g.iter();
            while let (Some(&a), Some(&b)) = (it.next(), it.next()) {
                partner[a] = Some(b);
                partner[b] = Some(a);
            }
            done.insert(key);
            continue;
        }
        let rev = (d.clone(), h.clone());
        done.insert(key.clone());
        done.insert(rev.clone());
        if let (Some(g1), Some(g2)) = (groups.get(&key), groups.get(&rev)) {
            // Only couple a connection whose two directions have the SAME number
            // of doors. If they differ, pairing `min(len)` of them would leave
            // the majority direction's excess doors at their original
            // destination while their lone reverse gets matched away - producing
            // a dangling one-way edge (`HA→HB` survives, `HB→HA` vanishes). The
            // safe choice is to leave the whole unbalanced connection static so
            // both directions stay intact (it's reported in `unpaired`).
            if g1.len() == g2.len() {
                for i in 0..g1.len() {
                    partner[g1[i]] = Some(g2[i]);
                    partner[g2[i]] = Some(g1[i]);
                }
            }
        }
    }

    let mut dest_new = origs.to_vec();

    // Match partnered doors into the new involution, constrained to
    // **length-preserving** swaps. When `a` matches `b`, `a` receives the
    // descriptor `origs[partner[b]]` (its name is `b`'s home-scene label) and
    // vice versa. The rewrite keeps the MAN's decompressed size unchanged - so no
    // scene grows and none can overflow on recompress - exactly when the name
    // lengths line up: `len(home[b]) == len(dest[a])` and `len(home[a]) ==
    // len(dest[b])`. Bucketing each door by `key = (len(home), len(dest))` and
    // matching a `(p, q)` bucket against the mirror `(q, p)` bucket guarantees
    // both. This is what lets coupled mode randomize the overworld hubs (which
    // can't be grown in place) while staying two-way; the variable-length
    // relocation path is reserved for decoupled mode.
    let key = |i: usize| (homes[i].len(), origs[i].name.len());
    let mut buckets: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
    for (i, p) in partner.iter().enumerate() {
        if p.is_some() {
            buckets.entry(key(i)).or_default().push(i);
        }
    }
    let mut matched_pairs: Vec<(usize, usize)> = Vec::new();
    let mut handled: std::collections::HashSet<(usize, usize)> = std::collections::HashSet::new();
    let mut bkeys: Vec<(usize, usize)> = buckets.keys().copied().collect();
    bkeys.sort_unstable(); // deterministic iteration order
    for k in bkeys {
        if handled.contains(&k) {
            continue;
        }
        let (p, q) = k;
        if p == q {
            handled.insert(k);
            let mut g = buckets[&k].clone();
            rng.shuffle(&mut g);
            let mut i = 0;
            while i + 1 < g.len() {
                matched_pairs.push((g[i], g[i + 1]));
                i += 2;
            }
        } else {
            let rk = (q, p);
            handled.insert(k);
            handled.insert(rk);
            let mut ga = buckets.get(&k).cloned().unwrap_or_default();
            let mut gb = buckets.get(&rk).cloned().unwrap_or_default();
            rng.shuffle(&mut ga);
            rng.shuffle(&mut gb);
            for i in 0..ga.len().min(gb.len()) {
                matched_pairs.push((ga[i], gb[i]));
            }
        }
    }
    for &(a, b) in &matched_pairs {
        dest_new[a] = origs[partner[b].unwrap()].clone();
        dest_new[b] = origs[partner[a].unwrap()].clone();
    }

    // Unpaired: every door not placed in a matched pair - no reverse partner
    // (dead-end / one-way story warp, or a door orphaned by an unequal-direction
    // connection) or no length-compatible partner to swap with. These keep their
    // ORIGINAL destination (coupled mode never gives a door a one-way
    // reassignment), so `dest_new` already holds the right bytes - only the count
    // is reported.
    let matched: std::collections::HashSet<usize> =
        matched_pairs.iter().flat_map(|&(a, b)| [a, b]).collect();
    let unpaired = (0..n).filter(|i| !matched.contains(i)).count();
    (dest_new, unpaired, matched_pairs, partner)
}

/// Randomize scene-transition doors, one-way ([`DoorCoupling::Decoupled`]) or
/// bidirectional ([`DoorCoupling::Coupled`]). Each door's destination descriptor
/// (scene + entry tile + facing) is the atomic unit moved between sites.
///
/// - **Decoupled**: `Shuffle` permutes the existing destinations across all
///   doors (every scene stays reachable as some door's target); `Random` draws
///   each door's destination from the global pool.
/// - **Coupled**: re-pairs doors into two-way connections (the `mode` is treated
///   as the matching's randomness; doors with no reverse partner get the
///   decoupled fallback - see `plan_doors_coupled`).
///
/// Because the destination name is variable length, this uses the
/// [`crate::door::SceneDoors::rebuild`] relocation path (decompress → resize the
/// `0x3F` ops → fix the partition tables / section offset / intra-record jumps →
/// recompress → rewrite the descriptor size word). A scene whose rebuilt MAN
/// overflows its compressed footprint or fails validation keeps its original
/// doors and is recorded in `skipped`.
pub fn randomize_doors(
    patcher: &mut DiscPatcher,
    seed: u64,
    mode: DropMode,
    coupling: DoorCoupling,
) -> Result<DoorApplyReport> {
    use legaia_asset::man_edit::DestEdit;
    use legaia_asset::scene_asset_table::encode_size_word;

    let cd = cdname_map(patcher);

    // Pass 1: collect every scene's doors (decoded MAN held for pass 2).
    let mut scenes: Vec<SceneDoors> = Vec::new();
    for idx in 0..patcher.entry_count() {
        let entry = patcher
            .read_entry(idx)
            .with_context(|| format!("read PROT entry {idx}"))?;
        if let Some(d) = SceneDoors::locate(&entry, idx) {
            scenes.push(d);
        }
    }

    // Flatten to a global ordered list of original destinations + home labels.
    let mut origs: Vec<Dest> = Vec::new();
    let mut homes: Vec<String> = Vec::new();
    for s in &scenes {
        let home = legaia_prot::cdname::block_for(&cd, s.entry_idx as u32)
            .unwrap_or("?")
            .to_string();
        for site in &s.sites {
            origs.push(Dest {
                index: site.index,
                name: site.name.clone().into_bytes(),
                entry_x: site.entry_x,
                entry_z: site.entry_z,
                dir: site.dir,
            });
            homes.push(home.clone());
        }
    }

    let mut report = DoorApplyReport {
        sites_total: origs.len(),
        ..Default::default()
    };
    if origs.is_empty() {
        return Ok(report);
    }

    // Plan the new destination for each global site index. `match_of` is the new
    // (coupled) involution; `partner_of` is each site's original reverse door.
    let n = origs.len();
    let mut rng = SplitMix64::new(seed);
    let mut match_of: Vec<Option<usize>> = vec![None; n];
    let mut partner_of: Vec<Option<usize>> = vec![None; n];
    let new_descs: Vec<Dest> = match coupling {
        DoorCoupling::Coupled => {
            let (descs, unpaired, pairs, partner) = plan_doors_coupled(&origs, &homes, &mut rng);
            report.unpaired = unpaired;
            for (a, b) in pairs {
                match_of[a] = Some(b);
                match_of[b] = Some(a);
            }
            partner_of = partner;
            descs
        }
        DoorCoupling::Decoupled => match mode {
            DropMode::Shuffle => {
                let mut v = origs.clone();
                rng.shuffle(&mut v);
                v
            }
            DropMode::Random => (0..origs.len())
                .map(|_| origs[rng.below(origs.len())].clone())
                .collect(),
        },
    };

    // Map each global site index to its scene (for the coupled revert pass).
    let mut site_scene = vec![0usize; origs.len()];
    {
        let mut g = 0usize;
        for (si, s) in scenes.iter().enumerate() {
            for _ in 0..s.sites.len() {
                site_scene[g] = si;
                g += 1;
            }
        }
    }

    // Pass 2: rebuild each scene from the planned destinations and collect the
    // streams to write. `forced` is the set of global site indices pinned to
    // their ORIGINAL destination (a forced site contributes no edit).
    //
    // The coupled revert is a transitive closure. If a site can't be written -
    // its scene overflows - it keeps its original destination, which only stays a
    // valid two-way connection if its matched partner *and* its original reverse
    // also keep theirs. So a forced site forces both `match_of[X]` (the new
    // partner, which was sending players to X) and `partner_of[X]` (the original
    // reverse, which X's now-reverted destination points back at). Propagating
    // along both involutions reverts whole alternating cycles, never leaving a
    // dangling one-way edge. Forcing only removes edits (shrinks scenes), so the
    // skipped set only shrinks and the loop converges (decoupled mode has empty
    // involutions, so it runs once and reverts nothing).
    let mut forced: std::collections::HashSet<usize> = std::collections::HashSet::new();
    // Per scene: (scene index, rebuilt stream, new decompressed size, edit count).
    let mut streams: Vec<(usize, Vec<u8>, u32, usize)> = Vec::new();
    let mut skipped_scenes: std::collections::HashSet<usize> = std::collections::HashSet::new();
    loop {
        streams.clear();
        skipped_scenes.clear();
        let mut g = 0usize;
        for (si, scene) in scenes.iter().enumerate() {
            let base = g;
            g += scene.sites.len();
            let mut edits: Vec<DestEdit> = Vec::new();
            for (k, site) in scene.sites.iter().enumerate() {
                if forced.contains(&(base + k)) {
                    continue; // pinned to original - no edit
                }
                let d = &new_descs[base + k];
                let unchanged = d.index == site.index
                    && d.name == site.name.as_bytes()
                    && d.entry_x == site.entry_x
                    && d.entry_z == site.entry_z
                    && d.dir == site.dir;
                if unchanged {
                    continue;
                }
                edits.push(DestEdit {
                    op_pc: site.op_pc,
                    index: d.index,
                    name: d.name.clone(),
                    entry_x: d.entry_x,
                    entry_z: d.entry_z,
                    dir: d.dir,
                });
            }
            if edits.is_empty() {
                continue;
            }
            match scene.rebuild(&edits) {
                Some((stream, new_size)) => streams.push((si, stream, new_size, edits.len())),
                None => {
                    skipped_scenes.insert(si);
                }
            }
        }

        // Seed the closure with every coupled site in an overflowing scene, then
        // propagate along both involutions until no new site is forced.
        let mut stack: Vec<usize> = (0..n)
            .filter(|&i| {
                !forced.contains(&i)
                    && (match_of[i].is_some() || partner_of[i].is_some())
                    && skipped_scenes.contains(&site_scene[i])
            })
            .collect();
        let mut new_force = false;
        while let Some(x) = stack.pop() {
            if !forced.insert(x) {
                continue;
            }
            new_force = true;
            if let Some(y) = match_of[x]
                && !forced.contains(&y)
            {
                stack.push(y);
            }
            if let Some(p) = partner_of[x]
                && !forced.contains(&p)
            {
                stack.push(p);
            }
        }
        if !new_force {
            break;
        }
    }

    // Coupled sites reverted to original (their cycle touched an un-writable
    // scene), reported so the user knows what stayed vanilla to keep returns
    // honest.
    report.coupled_kept_original = (0..n)
        .filter(|i| forced.contains(i) && (match_of[*i].is_some() || partner_of[*i].is_some()))
        .count();

    // Write the converged set.
    for (si, stream, new_size, n_edits) in &streams {
        let scene = &scenes[*si];
        patcher
            .patch_prot_entry(
                scene.entry_idx,
                scene.man_descriptor_off as u64,
                &encode_size_word(0x03, *new_size).to_le_bytes(),
            )
            .with_context(|| format!("write scene {} MAN size word", scene.entry_idx))?;
        patcher
            .patch_prot_entry(scene.entry_idx, scene.man_offset as u64, stream)
            .with_context(|| format!("write scene {} MAN", scene.entry_idx))?;
        report.scenes_changed += 1;
        report.sites_changed += n_edits;
    }
    report.skipped = skipped_scenes
        .iter()
        .map(|&si| scenes[si].entry_idx)
        .collect();
    report.skipped.sort_unstable();
    Ok(report)
}

/// Outcome of randomizing intra-town (house / interior) doors.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct HouseDoorApplyReport {
    /// Scene bundles whose MAN was rewritten + written back.
    pub scenes_changed: usize,
    /// Total door-warp target tiles whose operand changed.
    pub sites_changed: usize,
    /// Total classified door-warp sites found (IN + OUT classes).
    pub sites_total: usize,
    /// Scene PROT-entry indices whose recompressed MAN overflowed (kept original).
    pub skipped: Vec<usize>,
}

/// Read every classified intra-town door warp on the disc (the house-door
/// population), in PROT-entry order: the cross-context player `MOVE_TO`s
/// (`0xA3 0xF8`) in named partition-0 door records (see
/// [`crate::house_door`]). NPC / prop / cutscene movement is excluded by
/// construction. Purely read-only audit surface.
pub fn current_house_doors(patcher: &DiscPatcher) -> Result<Vec<(usize, u8, u8)>> {
    let mut out = Vec::new();
    for idx in 0..patcher.entry_count() {
        let entry = patcher
            .read_entry(idx)
            .with_context(|| format!("read PROT entry {idx}"))?;
        let Some(sd) = SceneHouseDoors::locate(&entry, idx) else {
            continue;
        };
        for (xb, zb) in sd.current_targets() {
            out.push((idx, xb & 0x7F, zb & 0x7F));
        }
    }
    Ok(out)
}

/// Randomize intra-town (house / interior) doors by a **per-scene,
/// class-preserving shuffle** of the player door-warp target tiles: `IN`-class
/// (interior landing) targets permute among `IN` sites, `OUT`-class (exterior
/// doorstep) targets among `OUT` sites (see [`crate::house_door`] - exiting
/// any interior always lands back outside, so no softlock is constructible).
/// Each scene's MAN is recompressed (same-size operand edits) and written back
/// when it fits; a scene that overflows keeps its original tiles. Only
/// [`DropMode::Shuffle`] is meaningful (a random draw would place the player
/// off-map), so a non-shuffle mode is a no-op.
pub fn randomize_house_doors(
    patcher: &mut DiscPatcher,
    seed: u64,
    mode: DropMode,
) -> Result<HouseDoorApplyReport> {
    let mut report = HouseDoorApplyReport::default();
    if !crate::house_door::supported_mode(mode) {
        return Ok(report);
    }
    for idx in 0..patcher.entry_count() {
        let entry = patcher
            .read_entry(idx)
            .with_context(|| format!("read PROT entry {idx}"))?;
        let Some(mut scene) = SceneHouseDoors::locate(&entry, idx) else {
            continue;
        };
        report.sites_total += scene.sites.len();
        let changed = scene.shuffle(seed);
        if changed == 0 {
            continue;
        }
        match scene.repack() {
            Some(stream) => {
                patcher
                    .patch_prot_entry(idx, scene.man_offset as u64, &stream)
                    .with_context(|| format!("write scene {idx} MAN"))?;
                report.scenes_changed += 1;
                report.sites_changed += changed;
            }
            None => report.skipped.push(idx),
        }
    }
    Ok(report)
}

#[cfg(test)]
mod door_plan_tests {
    use super::*;

    fn dest(name: &str, ex: u8) -> Dest {
        Dest {
            index: 0,
            name: name.as_bytes().to_vec(),
            entry_x: ex,
            entry_z: 0,
            dir: 0,
        }
    }

    /// Sorted multiset of (name, entry_x) - the descriptor identity for the
    /// permutation check.
    fn multiset(v: &[Dest]) -> Vec<(Vec<u8>, u8)> {
        let mut m: Vec<_> = v.iter().map(|d| (d.name.clone(), d.entry_x)).collect();
        m.sort();
        m
    }

    #[test]
    fn coupled_preserves_multiset_and_is_deterministic_when_all_paired() {
        // Two clean connections: town<->map and cave<->map.
        //   site 0: home town -> dest map (entry 0x10)   [A]
        //   site 1: home map  -> dest town (entry 0x20)  [B = reverse of A]
        //   site 2: home cave -> dest map (entry 0x30)   [C]
        //   site 3: home map  -> dest cave (entry 0x40)  [D = reverse of C]
        let origs = vec![
            dest("map", 0x10),
            dest("town", 0x20),
            dest("map", 0x30),
            dest("cave", 0x40),
        ];
        let homes = vec![
            "town".to_string(),
            "map".to_string(),
            "cave".to_string(),
            "map".to_string(),
        ];
        let mut rng = SplitMix64::new(0xC0FFEE);
        let (out, unpaired, _pairs, _partner) = plan_doors_coupled(&origs, &homes, &mut rng);
        assert_eq!(unpaired, 0, "all four sites have a reverse partner");
        // Coupling only moves existing descriptors -> multiset preserved.
        assert_eq!(multiset(&out), multiset(&origs));
        // Deterministic for a fixed seed.
        let mut rng2 = SplitMix64::new(0xC0FFEE);
        let (out2, _, _, _) = plan_doors_coupled(&origs, &homes, &mut rng2);
        assert_eq!(out, out2);
        // Scene-level edge multiset stays symmetric: for the new graph, every
        // (home -> dest) edge has a matching (dest -> home) edge.
        let mut edges: Vec<(String, String)> = out
            .iter()
            .zip(&homes)
            .map(|(d, h)| (h.clone(), String::from_utf8_lossy(&d.name).into_owned()))
            .collect();
        edges.sort();
        for (a, b) in &edges {
            assert!(
                edges.iter().any(|(x, y)| x == b && y == a),
                "edge {a}->{b} has no reverse"
            );
        }
    }

    #[test]
    fn coupled_counts_a_dead_end_site_as_unpaired() {
        // A one-way story warp: site 0 home A -> dest B, but no B -> A door.
        // Plus a clean pair (1<->2) so something is matchable.
        let origs = vec![dest("b", 1), dest("y", 2), dest("x", 3)];
        let homes = vec!["a".to_string(), "x".to_string(), "y".to_string()];
        let mut rng = SplitMix64::new(7);
        let (_out, unpaired, _pairs, _partner) = plan_doors_coupled(&origs, &homes, &mut rng);
        // site 0 (a->b) has no reverse; sites 1 (x->y) and 2 (y->x) pair.
        assert_eq!(unpaired, 1);
    }
}
