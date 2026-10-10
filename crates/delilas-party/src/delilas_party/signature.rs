//! Signature arts: the host-art table, staged clip chains, authored cast rows and fanfares.
//! Split out of `delilas_party.rs`.

use super::*;

/// The sibling's signature special, as the disc spells it. All three
/// are 13 characters, which is what lets the rename be an in-place
/// same-length write over a host art of the same width.
pub(super) fn signature_name(sibling: Sibling) -> &'static [u8] {
    match sibling {
        Sibling::Gi => b"Blazing Slash",
        Sibling::Che => b"Megaton Press",
        Sibling::Lu => b"Plasma Strike",
    }
}

/// The host Hyper art a hero slot gives up to carry the sibling's
/// signature special.
pub(super) struct HostArt {
    /// Retail name - must be the same byte length as [`signature_name`],
    /// and the rename hits every occurrence in SCUS.
    pub(super) retail_name: &'static [u8],
    /// Index among the character's non-Miracle arts.
    pub(super) index: u8,
    /// The art's action constant - the key its fanfare cue is selected
    /// by ([`legaia_art::hyper_fanfare::HYPER_FANFARES`]).
    pub(super) action_constant: u8,
    /// The replacement 5-input combo, checked unique against the
    /// character's other arts before anything is written.
    pub(super) combo: [legaia_art::queue::Command; 5],
    /// A better-choreographed camera arm to take, when the host art's
    /// own has nothing to re-time.
    ///
    /// Taken as a **swap**, not a retarget: every arm in the dispatcher
    /// is already live in some character's table, so pointing this art
    /// at another one would alias an arm a second art still uses, and
    /// [`retime_camera_arm`] would then follow the alias and mistune
    /// that art too. The slots that held the borrowed arm therefore take
    /// this art's own in exchange - no art loses its camera, both
    /// cameras are still retail, and the borrowed arm ends up reachable
    /// from exactly one slot, which is what makes re-timing it safe.
    ///
    /// `None` where the host art's own arm is already cursor-gated and
    /// so has choreography worth re-timing - see [`host_art`].
    pub(super) camera_swap: Option<u32>,
}

/// Which art each hero slot (0 Vahn / 1 Noa / 2 Gala) gives up: each
/// character's 50-AP Hyper.
///
/// They are the only three that clear every gate at once. The combo has
/// to be the same LENGTH as the one it replaces (`player_edits` drops a
/// mismatched edit and `glyph_patches` zips slot-for-slot), which by
/// itself rules out every 3- and 4-input Hyper. Of what is left, Noa's
/// Hurricane Kick is disqualified twice over - three bank records carry
/// its combo, and its ME stream is shared with a Super Art - and the
/// remaining candidates share a combo STRING across characters
/// (`0x80014198` is both Vahn's Tornado Flame and Gala's Thunder Punch,
/// so rewriting one rewrites the other's menu glyphs).
///
/// The combo must be free of every other art of that character **as a
/// substring at any offset**, which is a much stronger condition than
/// not being equal to one. The retail matcher (`FUN_801EED1C`, the
/// arrow-to-art normalisation) walks the scan START index DOWNWARD from
/// 15 and takes the first art that matches anywhere, so **a match
/// starting later in the input beats a longer match starting earlier**,
/// regardless of art length or bank order. An ordinary art also does not
/// consume its run - only its last direction becomes the action
/// constant, the leading N-1 stay in the queue and remain matchable.
///
/// `L R L R D` was the first choice and it fails on Gala: his Battering
/// Ram is `L R D`, which sits at offset 2, so it matches three passes
/// before the host art gets a look, does not consume, and leaves `L R`
/// for Back Punch - two arts fire and the signature never does. Vahn
/// survived the same mistake only by luck: his collision (`L R L`, Hyper
/// Elbow) is at offset 0, and the host being a Hyper consumes all five
/// inputs before the ordinary row is reached.
///
/// `L R U U D` occurs at no offset in any bank record of any of the
/// three (405 of the 1024 five-input combos are substring-free on all
/// three; the alternates `L R R R D`, `U R U R D`, `L D U R D` are
/// equally clean).
pub(super) fn host_art(slot: usize) -> Option<HostArt> {
    use legaia_art::queue::Command::{Down, Left, Right, Up};
    let combo = [Left, Right, Up, Up, Down];
    match slot {
        0 => Some(HostArt {
            retail_name: b"Burning Flare",
            index: 1,
            action_constant: 0x1C,
            combo,
            // Burning Flare's own arm (`0x801D7650`) reads no cursor at
            // all: one static framing for the entire swing. That is why
            // this slot reads flat, and it is also why there is nothing
            // here to re-time. It is not unique in that - three of Noa's
            // arms are flat too - but it is the only flat one any of the
            // three host arts dispatches to. Take Tornado Flame's arm instead (two cursor
            // bands, gate at keyframe 14, three ramp folds) and hand
            // Tornado Flame - and the Miracle finisher that shares it -
            // the static one in exchange.
            camera_swap: Some(0x801D_74A8),
        }),
        1 => Some(HostArt {
            retail_name: b"Vulture Blade",
            index: 4,
            action_constant: 0x1F,
            combo,
            // Vulture Blade's arm is already the second-richest in
            // Noa's table (two bands, gate at keyframe 14, five ramp
            // folds, no side effects) and is hers alone, so it is
            // re-timed in place.
            camera_swap: None,
        }),
        2 => Some(HostArt {
            retail_name: b"Explosive Fist",
            index: 1,
            action_constant: 0x1C,
            combo,
            // Explosive Fist's arm is the most band-rich in the entire
            // dispatcher: four framings across the swing at keyframes
            // 4/7/10, no side effects, and uniquely it reads no table
            // row and never touches `ctx+0x26D`, so it is immune to the
            // per-turn column coin-flip. Nothing to upgrade to, and
            // hers alone, so it is re-timed in place.
            camera_swap: None,
        }),
        _ => None,
    }
}

/// The monster-archive animation **entry index** carrying each sibling's
/// signature choreography.
///
/// Entry index, not action tag, and not a heuristic. The tag space does
/// not separate a special from an ordinary castable - Gi's and Che's
/// signature clips are both tagged `0x23`, and the old
/// `max_by_key(frame_count)` over the `0x0C..=0x1F` band therefore could
/// not reach either of them (it returned a generic castable for Gi, and
/// only found Che's by landing on a byte-identical duplicate).
///
/// A signature move is a **chain**, not a clip. The enemy-side modules
/// stage several archive entries in sequence - `delilas_dome` records
/// Lu's action `0x7B` as `14 -> 12 -> 13` and Che's `0x7A` as
/// `10 -> 11` - and shipping only the last stage is why the reskinned
/// arts showed the payoff swing with no wind-up: Megaton Press skipped
/// the lift, Blazing Slash skipped two thirds of itself.
///
/// Gi's chain is the one no static evidence pinned. `10 -> 11 -> 12` was
/// inferred from clip shape (an 11-frame crouch, a 30-frame leap with
/// the largest torso rise in his archive, a 23-frame slash), and a
/// player watching Blazing Slash independently reported "3 different
/// mini animations", which is the count that inference predicts.
pub(super) fn signature_clip_chain(sibling: Sibling) -> &'static [usize] {
    match sibling {
        Sibling::Gi => &[10, 11, 12],
        Sibling::Che => &[10, 11],
        Sibling::Lu => &[14, 12, 13],
    }
}

/// One sibling's staged-caster chain: which archive clips are authored
/// into the player file, in module walk order, and how the head table
/// binds them at rest.
pub(super) struct StagedChain {
    /// Archive entry index of each clip, module walk order (opener first).
    pub(super) clips: &'static [usize],
    /// Per-clip hard keyframe floor (`cast_stage::stage_ladder`).
    pub(super) floors: &'static [usize],
    /// Per-clip: carry the SOURCE entry's authored loop window across
    /// (`cast_stage::build_entry` rescales it).
    pub(super) windows: &'static [bool],
    /// Per-clip: host at the SOURCE's exact frames + rate instead of
    /// the duration-true rate-1 re-timing
    /// (`cast_stage::stage_ladder`). Required where a module cursor
    /// gate rides the stage - the anim cursor climbs `2 * rate`
    /// sixteenths a tick, so re-timing a gated clip to rate 1 halves
    /// the climb and slides every absolute-cursor test off retail's
    /// tick schedule.
    pub(super) identity: &'static [bool],
    /// Per-clip id/tag byte = the table row the clip is reached through.
    pub(super) row_ids: &'static [u8],
    /// At-rest head-table bindings `(row, chain index)`.
    pub(super) binding: &'static [(usize, usize)],
}

/// The FULL retail chain per sibling - the un-folded module walk, where
/// the module-side stage caves ([`crate::delilas_cast`]) repoint row
/// `0x0A` (and, for 958, row `0x0B`) at each mid-stage:
///
/// * Gi (module 958, walk `10,11,12,10,11,13`): crouch wind-up, leap,
///   mid slash, second crouch+leap pass, finale. Rows at rest:
///   `0x0A` -> crouch (10), `0x0B` -> leap (11); the caves swap `0x0A`
///   to 12/back-to-10 and `0x0B` to 13 for the finale (reset by the s2
///   cave next cast).
/// * Lu (module 960, walk `10,14,12,13,15`): raise, charge, channel,
///   strike, closing flourish. Rows at rest: `0x0A` -> raise (10),
///   `0x0B` -> flourish (15, the burst stage); the caves walk `0x0A`
///   through 14/12/13. The strike (13) hosts IDENTITY: module 0960's
///   mp5 confirm (cursor `0x90`) and damage tick (cursor `0x160`, 28
///   ticks after the module releases the authored `[15, 15]` park at
///   file `+0x1638`) both ride the strike stage as absolute-cursor
///   tests, so the hosted clip must keep the source's own 39-frame
///   rate-2 schedule and its park window.
/// * Che (module 959): the retail walk IS two stages - chain == fold.
pub(super) fn staged_chain_full(sibling: Sibling) -> StagedChain {
    use crate::party_swap::enemy_anim::{PAYOFF_FLOOR_FRAMES, RETAIL_STAGED_FLOOR as RF};
    match sibling {
        Sibling::Gi => StagedChain {
            clips: &[10, 11, 12, 13],
            floors: &[RF, RF, RF, RF],
            // Retail windows: crouch holds [9, 10], leap holds [8, 9],
            // the slash authors none (the retail flurry replays), the
            // finale parks on its last frame.
            windows: &[true, true, true, true],
            identity: &[false, false, false, false],
            row_ids: &[0x0A, 0x0B, 0x0A, 0x0B],
            binding: &[(0x0A, 0), (0x0B, 1)],
        },
        Sibling::Che => StagedChain {
            clips: &[10, 11],
            floors: &[RF, RF],
            // Che's lift/smash author no windows - carrying them is a
            // no-op, kept uniform.
            windows: &[true, true],
            identity: &[false, false],
            row_ids: &[0x0A, 0x0B],
            binding: &[(0x0A, 0), (0x0B, 1)],
        },
        Sibling::Lu => StagedChain {
            clips: &[10, 14, 12, 13, 15],
            floors: &[RF, RF, RF, PAYOFF_FLOOR_FRAMES, RF],
            // The strike (13) hosts IDENTITY (39f rate 2) with its
            // authored [15, 15] park intact: module 0960 confirms mp5
            // at cursor 0x90 (strike frame 9), RELEASES the park
            // itself (file +0x1638 clears the caster's +0x176/+0x21B
            // hold budget) and fires the damage a fixed 28 ticks
            // later at cursor 0x160 (frame 22) - the burst lands ON
            // the thrust only when the hosted clip reproduces the
            // source's cursor schedule tick for tick. The rate-1
            // re-timing (a windowless 23f host) halved the cursor
            // climb: mp5 confirmed 36 ticks late, the park never
            // held, and the burst decoupled from the release - the
            // audible desync against the module-fired cast bed.
            windows: &[true, true, true, true, true],
            identity: &[false, false, false, true, false],
            row_ids: &[0x0A, 0x0A, 0x0A, 0x0A, 0x0B],
            binding: &[(0x0A, 0), (0x0B, 4)],
        },
    }
}

/// The FOLDED two-clip fallback (the pre-cave shape): the module's
/// staged walk stays folded onto rows `0x0A`/`0x0B`
/// (`MODULE_95x_STAGE_REMAP_EDITS`), so a sibling with a longer retail
/// chain contributes its first stage and its payoff - Gi the crouch
/// wind-up + leap, Lu the charge + strike (the fold's damage build-up
/// rides the restaged wind-up row, hence Lu's
/// [`enemy_anim::PAYOFF_FLOOR_FRAMES`] floor on the charge).
pub(super) fn staged_chain_folded(sibling: Sibling) -> StagedChain {
    use crate::party_swap::enemy_anim::{PAYOFF_FLOOR_FRAMES, RETAIL_STAGED_FLOOR as RF};
    match sibling {
        Sibling::Gi | Sibling::Che => StagedChain {
            clips: &[10, 11],
            floors: &[RF, RF],
            windows: &[true, true],
            identity: &[false, false],
            row_ids: &[0x0A, 0x0B],
            binding: &[(0x0A, 0), (0x0B, 1)],
        },
        Sibling::Lu => StagedChain {
            clips: &[14, 13],
            // The fold's damage build-up rides the restaged charge (row
            // 0x0A carries the PAYOFF floor), so BOTH slots drop their
            // windows here: a hold would park the cursor short of the
            // keyframe-22 damage gate.
            floors: &[PAYOFF_FLOOR_FRAMES, RF],
            windows: &[false, false],
            identity: &[false, false],
            row_ids: &[0x0A, 0x0B],
            binding: &[(0x0A, 0), (0x0B, 1)],
        },
    }
}

/// PROT entry of Terra's player battle file (`data\battle\PLAYER4`).
pub(super) const TERRA_PLAYER_ENTRY: usize = 866;

/// What [`author_staged_cast_rows`] landed: the notes, plus the authored
/// entries' decoded-image offsets for each module that hosts a chain
/// LONGER than the folded pair (what the module-side stage caves need).
/// `None` = that sibling shipped the folded two-row shape.
pub(super) struct AuthoredCastRows {
    pub(super) notes: Vec<String>,
    /// Gi / module 958: `[crouch, leap, slash, finale]` offsets.
    pub(super) gi_unfold: Option<Vec<usize>>,
    /// Lu / module 960: `[raise, charge, channel, strike, flourish]`.
    pub(super) lu_unfold: Option<Vec<usize>>,
}

/// Author the signature caster rows for every routed slot, and re-home
/// the Block reaction across every player file.
///
/// What lands, all-or-nothing (every record[0] splice is computed before
/// the first byte is written, so a failure leaves the disc untouched):
///
/// 1. Each mapped slot's record[0] gets real staged rows: the sibling's
///    FULL retail chain when the LZS budget takes it, the folded
///    wind-up + payoff pair otherwise ([`staged_chain_full`] /
///    [`staged_chain_folded`]), hosted below the loader's sub-record
///    scratch ([`party_swap::cast_stage::build_staged_cast_rows`]), with
///    the retail Block entry re-homed byte-unmoved onto placeholder row
///    `0x06`.
/// 2. Files hosting no rows (Terra always) get the same one-word
///    row-`0x06` -> Block re-home.
/// 3. The party-init Block-reaction literal flips `0x0B` -> `0x06`
///    ([`crate::delilas_cast::relocate_block_reaction`]) so every slot's
///    guard keeps its retail clip while the cast modules own row `0x0B`.
pub(super) fn author_staged_cast_rows(
    patcher: &mut DiscPatcher,
    mapping: &PartyMapping,
    retail_players: &[Vec<u8>],
    archive: &[u8],
    options: &DelilasPartyOptions,
) -> Result<AuthoredCastRows> {
    use legaia_asset::battle_char_assembly;
    use party_swap::cast_stage;

    // Expand region writes into the offset-edit form
    // `patch_player_record0_full` consumes, dropping bytes that already
    // hold the target value (so an already-applied file reads as "no
    // change" instead of mistaking the encoder's `None` for an
    // overflow). The three outcomes are kept apart: an overflow is a
    // pose-ladder signal, not an error.
    enum Plan {
        NoChange,
        Fit(usize, Vec<u8>),
        Overflow,
    }
    let plan_splice = |entry: &[u8], writes: &[(usize, Vec<u8>)], label: &str| -> Result<Plan> {
        let decoded = battle_char_assembly::decode_record0(entry)
            .with_context(|| format!("decode {label} record0"))?;
        let edits: Vec<(usize, u8)> = writes
            .iter()
            .flat_map(|(off, bytes)| bytes.iter().enumerate().map(move |(i, &b)| (off + i, b)))
            .filter(|&(off, b)| decoded.get(off).copied() != Some(b))
            .collect();
        if edits.is_empty() {
            return Ok(Plan::NoChange);
        }
        Ok(
            match crate::arts::patch_player_record0_full(entry, &[], &edits) {
                Some((off, bytes)) => Plan::Fit(off, bytes),
                None => Plan::Overflow,
            },
        )
    };

    // Every write is planned before the first byte lands, so a failure
    // leaves the disc untouched. `(PROT entry, file offset, bytes)`.
    let mut commits: Vec<(usize, usize, Vec<u8>)> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    let mut row_hosts: Vec<usize> = Vec::new();

    let mut unfold: std::collections::HashMap<u8, Vec<usize>> = std::collections::HashMap::new();
    for (host_entry, rig, slot, who, sibling) in mapping.pairs() {
        let source_id = sibling.monster_id();
        let clips = monster_archive::animations(archive, source_id)
            .with_context(|| format!("read monster {source_id} animations"))?
            .unwrap_or_default();
        let full = staged_chain_full(sibling);
        let folded = staged_chain_folded(sibling);
        let pick = |spec: &StagedChain| -> Result<Vec<&monster_archive::MonsterAnimation>> {
            let picked: Vec<&monster_archive::MonsterAnimation> =
                spec.clips.iter().filter_map(|&i| clips.get(i)).collect();
            if picked.len() != spec.clips.len() {
                bail!(
                    "monster {source_id} carries {} of the {} staged clips",
                    picked.len(),
                    spec.clips.len()
                );
            }
            Ok(picked)
        };

        // Reclaim the descriptor-table slack up front (free compressed-
        // stream footprint, transparent to the loader - see
        // `cast_stage::push_up_desc_table`), then walk the pose ladder
        // against the real LZS budget.
        let mut live = patcher
            .read_entry(host_entry)
            .with_context(|| format!("read {who} player file"))?;
        if let Some(writes) = cast_stage::push_up_desc_table(&live)
            .with_context(|| format!("re-lay {who}'s descriptor table"))?
        {
            for (off, bytes) in &writes {
                live[*off..*off + bytes.len()].copy_from_slice(bytes);
                commits.push((host_entry, *off, bytes.clone()));
            }
        }
        // The staged rows GROW the decoded record[0] (inserted below
        // `clut_a_off` - everything from that offset on is the loader's
        // sub-record decode scratch, so rows parked any higher are
        // destroyed during battle load). The commit is therefore a full
        // stream replacement plus the three shifted header words, not a
        // same-size byte splice.
        let decoded_live = battle_char_assembly::decode_record0(&live)
            .with_context(|| format!("decode {who} record0"))?;
        let (clut_a_live, _) = cast_stage::record0_clut_offsets(&live)
            .with_context(|| format!("read {who} record0 header"))?;
        let region = crate::arts::record0_lzs_region(&live)
            .ok_or_else(|| anyhow::anyhow!("{who}'s record0 LZS region not found"))?;
        match cast_stage::staged_state(&decoded_live, clut_a_live)? {
            cast_stage::StagedState::Applied => {
                // Recover the authored layout so the module-side caves can
                // re-derive their offsets on an idempotent re-run. A file
                // authored with neither the full nor the folded chain is
                // an older build's layout - only a clean image re-patches.
                match cast_stage::recover_entry_offsets(
                    &decoded_live,
                    clut_a_live,
                    full.clips.len(),
                ) {
                    Ok(offs) => {
                        unfold.insert(sibling.monster_id() as u8, offs);
                        notes.push(format!(
                            "cast route: {who} caster rows already present (full chain)"
                        ));
                    }
                    Err(_) => {
                        cast_stage::recover_entry_offsets(
                            &decoded_live,
                            clut_a_live,
                            folded.clips.len(),
                        )
                        .with_context(|| {
                            format!(
                                "{who}'s staged rows match neither the full nor the folded \
                                 chain; patch a clean retail image instead"
                            )
                        })?;
                        notes.push(format!(
                            "cast route: {who} caster rows already present (folded chain)"
                        ));
                    }
                }
            }
            cast_stage::StagedState::Stale => bail!(
                "{who}'s player file carries the superseded payload-reuse staged-row \
                 layout (its rows are destroyed at battle load); patch a clean retail \
                 image instead"
            ),
            cast_stage::StagedState::Absent => {
                // The source clips' authored loop windows, index-aligned
                // with `animations()` (same walk, same skip rules).
                let src_windows = monster_archive::animation_loop_windows(archive, source_id)
                    .with_context(|| format!("read monster {source_id} loop windows"))?
                    .unwrap_or_default();
                // Source sound-cue tracks, index-aligned like the windows:
                // the punch-volley impacts the retail cast fires through
                // `FUN_800508DC` (zeroing them is what made a player-cast
                // flurry silent).
                let src_cues = monster_archive::animation_cue_tracks(archive, source_id)
                    .with_context(|| format!("read monster {source_id} cue tracks"))?
                    .unwrap_or_default();
                let build = |spec: &StagedChain| -> Result<(cast_stage::StagedCastRows, Vec<u8>)> {
                    let chain = pick(spec)?;
                    let windows: Vec<Option<monster_archive::ActionLoopWindow>> = spec
                        .clips
                        .iter()
                        .zip(spec.windows)
                        .map(|(&i, &keep)| {
                            if keep {
                                src_windows.get(i).copied().flatten()
                            } else {
                                None
                            }
                        })
                        .collect();
                    let cue_tracks: Vec<monster_archive::ActionCueTrack> = spec
                        .clips
                        .iter()
                        .map(|&i| src_cues.get(i).cloned().unwrap_or_default())
                        .collect();
                    let mut packed: Option<Vec<u8>> = None;
                    let built = cast_stage::build_staged_cast_rows(
                        &live,
                        &retail_players[slot],
                        rig,
                        archive,
                        source_id,
                        &chain,
                        spec.floors,
                        &windows,
                        &cue_tracks,
                        spec.identity,
                        spec.row_ids,
                        spec.binding,
                        party_swap::playerize::kept_welded_hand(
                            source_id,
                            options.keep_che_hammer && sibling == Sibling::Che,
                        ),
                        |decoded_new| {
                            let mut c = legaia_lzs::compress(decoded_new);
                            if c.len() > region.avail {
                                c = legaia_lzs::compress_optimal(decoded_new);
                            }
                            if c.len() > region.avail {
                                return Ok(false);
                            }
                            packed = Some(c);
                            Ok(true)
                        },
                    )?;
                    let packed =
                        packed.ok_or_else(|| anyhow::anyhow!("fits oracle accepted no stream"))?;
                    Ok((built, packed))
                };
                // The full retail chain first; the folded two-clip shape
                // is the budget fallback (and identical for Che).
                let (built, packed, is_full) = match build(&full) {
                    Ok((b, p)) => (b, p, true),
                    Err(full_err) => {
                        if full.clips.len() == folded.clips.len() {
                            return Err(
                                full_err.context(format!("author {who}'s staged cast rows"))
                            );
                        }
                        notes.push(format!(
                            "cast route: {who} full chain does not fit ({full_err:#}); \
                             folded two-clip chain kept"
                        ));
                        let (b, p) = build(&folded)
                            .with_context(|| format!("author {who}'s staged cast rows"))?;
                        (b, p, false)
                    }
                };
                if is_full && full.clips.len() > folded.clips.len() {
                    unfold.insert(sibling.monster_id() as u8, built.entry_offsets.clone());
                }
                let (ca, cb, bud) = built.header;
                let mut hdr = Vec::with_capacity(12);
                hdr.extend_from_slice(&ca.to_le_bytes());
                hdr.extend_from_slice(&cb.to_le_bytes());
                hdr.extend_from_slice(&bud.to_le_bytes());
                // Header words +0x04/+0x08/+0x0C sit right before the LZS
                // stream at header +0x10.
                commits.push((host_entry, region.lzs_off - 0x10 + 4, hdr));
                commits.push((host_entry, region.lzs_off, packed));
                let stages: Vec<String> = built
                    .frames
                    .iter()
                    .zip(&built.source_frames)
                    .zip(built.rates.iter().zip(&built.holds))
                    .map(|((f, sf), (r, h))| {
                        let hold = if *h > 1 {
                            format!(" hold x{h}")
                        } else {
                            String::new()
                        };
                        format!("{f}f (of {sf}) rate {r}{hold}")
                    })
                    .collect();
                notes.push(format!(
                    "cast route: {who} caster rows inserted (+{:#x} decoded bytes, {} stage \
                     clips) - {}",
                    built.delta,
                    built.frames.len(),
                    stages.join(", "),
                ));
            }
        }
        row_hosts.push(host_entry);
    }

    // Row-6 Block re-home in every file that hosts no staged rows
    // (Terra always: the party-init literal is shared by all four
    // slots). The one-word edit is compression-neutral in practice, but
    // a file already at its ceiling gets the same descriptor-table
    // reclaim as a fallback.
    for (other_entry, other_who) in mapping
        .pairs()
        .into_iter()
        .map(|(e, _, _, w, _)| (e, w))
        .chain([(TERRA_PLAYER_ENTRY, "Terra")])
        .filter(|&(e, _)| !row_hosts.contains(&e))
    {
        let mut f = patcher
            .read_entry(other_entry)
            .with_context(|| format!("read {other_who} player file"))?;
        let write = cast_stage::relocate_block_row(&f)
            .with_context(|| format!("re-home {other_who}'s Block row"))?;
        let mut plan = plan_splice(&f, std::slice::from_ref(&write), other_who)?;
        if matches!(plan, Plan::Overflow)
            && let Some(writes) = cast_stage::push_up_desc_table(&f)
                .with_context(|| format!("re-lay {other_who}'s descriptor table"))?
        {
            for (off, bytes) in &writes {
                f[*off..*off + bytes.len()].copy_from_slice(bytes);
                commits.push((other_entry, *off, bytes.clone()));
            }
            plan = plan_splice(&f, std::slice::from_ref(&write), other_who)?;
        }
        match plan {
            Plan::NoChange => {}
            Plan::Fit(off, bytes) => commits.push((other_entry, off, bytes)),
            Plan::Overflow => bail!(
                "{other_who}'s record0 will not fit its LZS footprint with the Block \
                 row re-homed"
            ),
        }
    }

    // Everything fits - commit, then the SCUS literal.
    for (entry, off, bytes) in &commits {
        patcher
            .patch_prot_entry(*entry, *off as u64, bytes)
            .with_context(|| format!("write staged-row bytes into PROT {entry}"))?;
    }
    crate::delilas_cast::relocate_block_reaction(patcher)
        .context("re-home the party Block reaction")?;

    notes.push("cast route: Block re-homed to row 0x06 on all four files".to_string());
    Ok(AuthoredCastRows {
        notes,
        gi_unfold: unfold.remove(&(Sibling::Gi.monster_id() as u8)),
        lu_unfold: unfold.remove(&(Sibling::Lu.monster_id() as u8)),
    })
}

/// The fanfare row the slot's host art fires through. The cue is a coin
/// flip between a PAIR of channels of the character's own fanfare bank
/// (`XA1`/`XA3`/`XA5`), and which pair is per-art - so the sibling's
/// special soundtrack has to be written to that art's pair, not to a
/// fixed one.
pub(super) fn signature_fanfare(slot: usize) -> Option<legaia_art::hyper_fanfare::HyperFanfare> {
    let art = host_art(slot)?;
    legaia_art::hyper_fanfare::HYPER_FANFARES
        .iter()
        .find(|f| f.cslot as usize == slot && f.action_constant == art.action_constant)
        .copied()
}

/// The two fanfare-bank channels the slot's signature art plays through.
pub fn signature_fanfare_channels(slot: usize) -> Option<(u8, u8)> {
    signature_fanfare(slot).map(|f| f.channel_pair())
}

/// The [`legaia_art::queue::Character`] a hero slot names.
pub(super) fn slot_character(slot: usize) -> legaia_art::queue::Character {
    use legaia_art::queue::Character;
    match slot {
        0 => Character::Vahn,
        1 => Character::Noa,
        _ => Character::Gala,
    }
}

/// Everything one hero slot's signature-art reskin reads.
#[derive(Clone, Copy)]
pub(super) struct SignatureCtx<'a> {
    /// 0 Vahn / 1 Noa / 2 Gala.
    pub(super) slot: usize,
    /// The sibling mapped onto that slot.
    pub(super) sibling: Sibling,
    pub(super) rig: &'a party_swap::PlayerRig,
    /// The hero's RETAIL player file, captured before the model loop.
    pub(super) retail_player: &'a [u8],
    pub(super) archive: &'a [u8],
    /// Canonical hand whose kept welded weapon plays the sibling's own
    /// wrist relation ([`party_swap::kept_welded_hand`]).
    pub(super) natural_wrist_hand: Option<usize>,
}
