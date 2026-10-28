//! The retail arts tokenizer - the queue-builder's normalisation pass that turns
//! the player's flat arrow string into the `… 0x19 <art> …` action queue - and
//! the derivation it enables: the **physical input** of a Super Art.
//!
//! ## What retail does (`FUN_801EED1C`, battle overlay, `0x801EF2EC..0x801EF858`)
//!
//! Read off the disassembly (`overlay_battle_action_801eed1c.txt`), not the C:
//!
//! ```text
//! for start in 15 ..= 0:                        ; 801ef848: s8 counts DOWN
//!   for each art record in grid order:          ; 801ef2ec/801ef318: s3 = 0xB..
//!     pos = start; matched = 0
//!     while pos < 16 and queue[pos] != 0:       ; 801ef3ac / 801ef7bc
//!       if queue[pos] - 0xB == art.cmd[matched]:  ; 801ef3e8: arrows 0x0C..0x0F vs bytes 1..4
//!         matched += 1
//!         if matched == art.len:                ; 801ef408: cmd[matched] == 0
//!           queue[pos] = 0x19 (0x1A if newly learned)  ; 801ef6f8: over the LAST arrow
//!           shift queue[pos+1 ..= 14] up one    ; 801ef708..801ef750 (queue[15] falls off)
//!           queue[pos+1] = art constant         ; 801ef7a0
//!           matched = 0; pos = start            ; 801ef780
//!       else:
//!         pos -= matched; matched = 0           ; 801ef7ac: rewind
//!       pos += 1                                ; 801ef7b4
//! ```
//!
//! Two consequences the earlier "compacting" reading missed, both of which the
//! in-the-wild Tri-Somersault capture (`0F 0E 19 27 0F 19 1F 0E 1A 2B 2B 2B`,
//! [`super-art-queue-capture.md`](../../../docs/tooling/super-art-queue-capture.md))
//! confirms byte-exact:
//!
//! - **The leading arrows of a matched art stay in the queue.** Only the last
//!   arrow is overwritten (by the `0x19` starter) and the constant is inserted
//!   after it. `↑↓↑` alone becomes `0F 0E 19 27` - the capture's leading `0F 0E`
//!   are Somersault's own first two arrows, not "residual input".
//! - **Arts overlap.** The walk is tail-first (`start` descends) and restarts at
//!   `start + 1` after every match, so an arrow can be part of two arts. The
//!   "connector" directions in a Super's `find` pattern are therefore not typed
//!   between the arts: `19 27 0F 19 1F 0E 19 27` is what `↑↓↑↑↑↓↑` (seven
//!   arrows) tokenizes to - Somersault, Cyclone and Somersault sharing arrows -
//!   which is exactly the input the walkthroughs print for Tri-Somersault.
//!
//! This is the retail **normal-art** path (`t5 == 0`: art ordinal `>= 4`,
//! i.e. everything but the Miracle Art and the three Hyper Arts, whose matches
//! take a different arm and, with the slot's `+0x25F` marker clear, write
//! nothing; with it set, [`tokenize_bank`] ports that arm too). Every Super
//! Art chain is normal arts only, so
//! [`derive_super_input`] needs no more than this. The AP-payment and
//! learn-on-use side effects (`FUN_801EFBFC`, `+0x170`) are not modelled - they
//! decide `0x19` vs `0x1A` and whether the walk aborts, not where the tokens
//! land.
//!
//! PORT: FUN_801EED1C (the art-normalisation loop `0x801EF2EC..0x801EF858`,
//! normal-art arm).

use crate::queue::{ActionConstant, Command};

/// Queue length the builder normalises over (`actor[+0x1DF..+0x1EF]`).
pub const QUEUE_LEN: usize = 16;
/// The starter written over a known art's last arrow (`FUN_801EFBFC` returned
/// `1`); a newly learned art gets `0x1A` instead.
pub const STARTER: u8 = 0x19;

/// One art the tokenizer can recognise: its action constant and its arrow
/// string, in the grid order retail walks (ascending action constant).
pub type ArtEntry<'a> = (ActionConstant, &'a [Command]);

/// Normalise a raw arrow string into the action queue retail's builder produces,
/// treating every art as already learned (`0x19` starters). Returns the 16-byte
/// queue; unused tail bytes are `0`. Inputs longer than [`QUEUE_LEN`] are
/// truncated, exactly as the 16-byte queue would be.
pub fn tokenize(arts: &[ArtEntry<'_>], input: &[Command]) -> [u8; QUEUE_LEN] {
    let mut q = [0u8; QUEUE_LEN];
    for (i, c) in input.iter().take(QUEUE_LEN).enumerate() {
        q[i] = c.as_action().as_byte();
    }
    for start in (0..QUEUE_LEN).rev() {
        for &(action, cmds) in arts {
            if cmds.is_empty() {
                continue;
            }
            let mut pos = start;
            let mut matched = 0usize;
            while pos < QUEUE_LEN && q[pos] != 0 {
                if q[pos].wrapping_sub(0xB) == cmds[matched].as_byte() {
                    matched += 1;
                    if matched == cmds.len() {
                        q[pos] = STARTER;
                        if pos < QUEUE_LEN - 2 {
                            for k in (pos + 1..QUEUE_LEN - 1).rev() {
                                q[k + 1] = q[k];
                            }
                        }
                        if pos + 1 < QUEUE_LEN {
                            q[pos + 1] = action.as_byte();
                        }
                        matched = 0;
                        pos = start;
                    }
                } else {
                    pos -= matched;
                    matched = 0;
                }
                pos += 1;
            }
        }
    }
    q
}

/// The input positions at which an art **ends** - the arrows the tokenizer wrote
/// the `0x19` starter over - in ascending order. With arts overlapping, this is
/// what marks the boundary of each sub-art inside a Super Art's input:
/// Tri-Somersault's `↑↓↑↑↑↓↑` ends arts at positions 2 (Somersault), 4
/// (Cyclone) and 6 (Somersault). The tokenizer is re-run with each queue slot
/// carrying the input index it came from, so inserted constants never shift the
/// bookkeeping.
pub fn art_ends(arts: &[ArtEntry<'_>], input: &[Command]) -> Vec<usize> {
    let mut q = [0u8; QUEUE_LEN];
    let mut origin = [usize::MAX; QUEUE_LEN];
    for (i, c) in input.iter().take(QUEUE_LEN).enumerate() {
        q[i] = c.as_action().as_byte();
        origin[i] = i;
    }
    let mut ends = Vec::new();
    for start in (0..QUEUE_LEN).rev() {
        for &(action, cmds) in arts {
            if cmds.is_empty() {
                continue;
            }
            let mut pos = start;
            let mut matched = 0usize;
            while pos < QUEUE_LEN && q[pos] != 0 {
                if q[pos].wrapping_sub(0xB) == cmds[matched].as_byte() {
                    matched += 1;
                    if matched == cmds.len() {
                        if origin[pos] != usize::MAX && !ends.contains(&origin[pos]) {
                            ends.push(origin[pos]);
                        }
                        q[pos] = STARTER;
                        origin[pos] = usize::MAX;
                        if pos < QUEUE_LEN - 2 {
                            for k in (pos + 1..QUEUE_LEN - 1).rev() {
                                q[k + 1] = q[k];
                                origin[k + 1] = origin[k];
                            }
                        }
                        if pos + 1 < QUEUE_LEN {
                            q[pos + 1] = action.as_byte();
                            origin[pos + 1] = usize::MAX;
                        }
                        matched = 0;
                        pos = start;
                    }
                } else {
                    pos -= matched;
                    matched = 0;
                }
                pos += 1;
            }
        }
    }
    ends.sort_unstable();
    ends
}

/// The queue's populated prefix (up to the first zero byte).
pub fn populated(q: &[u8; QUEUE_LEN]) -> &[u8] {
    let n = q.iter().position(|&b| b == 0).unwrap_or(QUEUE_LEN);
    &q[..n]
}

/// Longest input [`derive_super_input`] searches. Every retail Super is typed
/// in 7..=9 arrows; the search is exponential in this bound (`4^len` at worst),
/// so it stays well under the 16-byte queue.
pub const MAX_DERIVED_INPUT: usize = 12;

/// Derive the **shortest** arrow string whose tokenized queue ends with
/// `find` - a Super Art's physical input. Searches inputs over the directions
/// the chain arts use, shortest first, and returns `None` if nothing up to
/// [`MAX_DERIVED_INPUT`] arrows produces the pattern or if the shortest length
/// is ambiguous (two different strings of that length both work - which no
/// retail Super has).
///
/// `arts` is the character's catalog in grid order (see [`ArtEntry`]).
pub fn derive_super_input(arts: &[ArtEntry<'_>], find: &[u8]) -> Option<Vec<Command>> {
    // Which arts the pattern names, and the alphabet they are typed with.
    let mut alphabet: Vec<Command> = Vec::new();
    let mut min_len = 0usize;
    for w in find.windows(2) {
        if w[0] != STARTER {
            continue;
        }
        let (_, cmds) = arts.iter().find(|(a, _)| a.as_byte() == w[1]).copied()?;
        min_len = min_len.max(cmds.len());
        for &c in cmds {
            if !alphabet.contains(&c) {
                alphabet.push(c);
            }
        }
    }
    if alphabet.is_empty() || min_len == 0 {
        return None;
    }
    let mut buf: Vec<Command> = Vec::new();
    for len in min_len..=MAX_DERIVED_INPUT {
        let mut hits: Vec<Vec<Command>> = Vec::new();
        buf.clear();
        buf.resize(len, alphabet[0]);
        let mut idx = vec![0usize; len];
        loop {
            let q = tokenize(arts, &buf);
            let p = populated(&q);
            if p.len() >= find.len() && &p[p.len() - find.len()..] == find {
                hits.push(buf.clone());
                if hits.len() > 1 {
                    return None;
                }
            }
            // Odometer step over the alphabet.
            let mut k = len;
            loop {
                if k == 0 {
                    break;
                }
                k -= 1;
                idx[k] += 1;
                if idx[k] < alphabet.len() {
                    buf[k] = alphabet[idx[k]];
                    break;
                }
                idx[k] = 0;
                buf[k] = alphabet[0];
            }
            if idx.iter().all(|&i| i == 0) {
                break;
            }
        }
        if hits.len() == 1 {
            return hits.pop();
        }
    }
    None
}

/// One record of a character's art bank as the builder's walk reads it: the
/// record's `+0x00` combo and its `+0x0A..+0x0D` stream-source bytes. The
/// first of those is the record's own `"ME"` stream; a non-zero `+0x0B..`
/// chains the records after it into one art (Noa's Hurricane Kick, bank
/// record `0x1C` with `+0x0A..+0x0B = 04 05`, plays records `0x1D` and
/// `0x1E`).
#[derive(Debug, Clone, Copy, Default)]
pub struct BankArt<'a> {
    pub cmds: &'a [Command],
    pub sources: [u8; 4],
}

/// The starter the special-art arm writes, whatever the learn verdict
/// (`li v1,0x1a` at `0x801EF5C8`).
pub const SPECIAL_STARTER: u8 = 0x1A;

/// How many art groups at the head of the walk take the special arm: the
/// Miracle Art (ordinal `0`) and the three Hyper Arts (`sltiu a1,a0,0x4` at
/// `0x801EF330`).
pub const SPECIAL_ORDINALS: u8 = 4;

/// What [`tokenize_bank`] built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BankTokens {
    /// The 16-byte queue window.
    pub queue: [u8; QUEUE_LEN],
    /// The builder's per-token side array `0x801F6990` over the window: `1`
    /// on each normal art's starter, moved with every shift.
    pub marks: [u32; QUEUE_LEN],
    /// The bank constant (`0x1B + k`) of each Hyper Art the walk compacted,
    /// in walk order - the record `FUN_801EFBFC` was handed for it.
    pub hyper_matches: Vec<u8>,
    /// Whether the walk matched the Miracle record (ordinal `0`) with the
    /// marker armed - where retail copies the Miracle row over the window.
    pub miracle_matched: bool,
}

/// One owned bank row: a record's combo and its `+0x0A..+0x0D` chain bytes.
pub type BankRow = (Vec<Command>, [u8; 4]);

/// A character's bank in walk order out of `(constant, combo, chain bytes)`
/// records in any order: one row per constant from `0x1B` up to the highest
/// given, a missing constant an empty row (it matches nothing but still
/// counts as a group). Constants below `0x1B` are not walked
/// (`li s3,0xb` at `0x801EF2EC`) and are dropped.
pub fn bank_rows(records: impl IntoIterator<Item = (u8, Vec<Command>, [u8; 4])>) -> Vec<BankRow> {
    let mut rows: Vec<BankRow> = Vec::new();
    for (b, cmds, sources) in records {
        let Some(k) = (b as usize).checked_sub(0x1B) else {
            continue;
        };
        if rows.len() <= k {
            rows.resize(k + 1, (Vec::new(), [0; 4]));
        }
        rows[k] = (cmds, sources);
    }
    rows
}

/// [`BankArt`] views over owned [`BankRow`]s.
pub fn bank_view(rows: &[BankRow]) -> Vec<BankArt<'_>> {
    rows.iter()
        .map(|(cmds, sources)| BankArt {
            cmds: cmds.as_slice(),
            sources: *sources,
        })
        .collect()
}

/// The constants the three Hyper-Art groups of `records` write into a queue,
/// paired with the group's own bank constant. A plain group writes its own
/// constant; a chained one writes the records after it
/// ([`BankArt::sources`]).
pub fn hyper_queue_constants(records: &[BankArt<'_>]) -> Vec<(u8, Vec<u8>)> {
    let mut out = Vec::new();
    let mut s3 = 0usize;
    let mut ordinal = 0u8;
    while s3 < records.len() && ordinal < SPECIAL_ORDINALS {
        let rec = records[s3];
        if ordinal > 0 {
            let base = 0x1B + s3 as u8;
            let consts = if rec.sources[1] == 0 {
                vec![base]
            } else {
                rec.sources
                    .iter()
                    .take_while(|&&b| b != 0)
                    .enumerate()
                    .map(|(i, _)| base + 1 + i as u8)
                    .collect()
            };
            out.push((base, consts));
        }
        s3 = group_end(records, s3);
        ordinal += 1;
    }
    out
}

/// The walk's step past the group at `s3` (`0x801EF7C8..0x801EF828`): each
/// non-zero `+0x0B..+0x0D` read along the chain moves one record on, and a
/// chain found skips one more.
fn group_end(records: &[BankArt<'_>], mut s3: usize) -> usize {
    let mut chained = 0usize;
    for a2 in 1..4 {
        if records.get(s3).is_some_and(|r| r.sources[a2] != 0) {
            s3 += 1;
            chained = 1;
        }
    }
    s3 + chained + 1
}

/// The builder's normalisation walk over a character's whole art bank,
/// **both** of its arms: `records[k]` is bank record `0x1B + k` (the walk
/// starts at `s3 = 0xB`, `0x801EF2EC`), and the first
/// [`SPECIAL_ORDINALS`] groups take the special arm.
///
/// - **Normal art** (ordinal `>= 4`): [`tokenize`]'s rewrite - starter over
///   the run's last arrow, constant inserted after it, leading arrows kept.
/// - **Hyper Art** (ordinals `1..=3`) with `special_armed` (the slot's
///   Ra-Seru marker `ctx[+0x25F + slot]`, `0x801EF4C8`): the run **collapses**
///   to `0x1A` and the art's constant(s), and the queue after the run moves
///   down behind them (`0x801EF5BC..0x801EF6CC`). Gala's Lightning Storm
///   `R R U L` becomes `1A 1D`.
/// - **Miracle** (ordinal `0`) with the marker armed: retail copies the
///   Miracle row over the window and leaves the run (`0x801EF4E8..`); that
///   copy is the caller's ([`BankTokens::miracle_matched`]), here the run
///   just ends.
/// - A special art with the marker **clear** writes nothing and rewinds
///   (`0x801EF6D4..0x801EF6DC`, matched count kept).
///
/// Ordinals count **groups**, not constants: Noa's chained Hurricane Kick
/// spans three records, so her Vulture Blade (`0x1F`) and Frost Breath
/// (`0x20`) are her second and third Hyper Arts.
///
/// A one-arrow full match ends the walk at this start
/// (`0x801EF420..0x801EF434`, `s3` forced to the count). Not modelled, as in
/// [`tokenize`]: the AP test that ends it the same way and the learn verdict
/// that drops the run (`FUN_801EFBFC`), which decide whether a run is taken,
/// not where its tokens land.
///
/// PORT: FUN_801EED1C (the art-normalisation loop `0x801EF2EC..0x801EF858`,
/// both arms)
pub fn tokenize_bank(
    records: &[BankArt<'_>],
    input: &[Command],
    special_armed: bool,
) -> BankTokens {
    let mut q = [0u8; QUEUE_LEN];
    let mut marks = [0u32; QUEUE_LEN];
    for (i, c) in input.iter().take(QUEUE_LEN).enumerate() {
        q[i] = c.as_action().as_byte();
    }
    let mut hyper_matches = Vec::new();
    let mut miracle_matched = false;
    let in_window = |p: isize| (0..QUEUE_LEN as isize).contains(&p);
    for start in (0..QUEUE_LEN).rev() {
        let mut s3 = 0usize;
        let mut ordinal = 0u8;
        'groups: while s3 < records.len() {
            let special = ordinal < SPECIAL_ORDINALS;
            let mut pos = start as isize;
            let mut matched = 0usize;
            // The record is re-read every pass (`0x801EF3BC..0x801EF3D8`), so
            // a chained match's `s3` step retargets the rest of this run.
            while in_window(pos) && q[pos as usize] != 0 {
                let Some(rec) = records.get(s3) else { break };
                let want = rec.cmds.get(matched).map_or(0, |c| c.as_byte());
                if q[pos as usize].wrapping_sub(0xB) != want || rec.cmds.is_empty() {
                    pos -= matched as isize;
                    matched = 0;
                    pos += 1;
                    continue;
                }
                matched += 1;
                if matched < rec.cmds.len() {
                    pos += 1;
                    continue;
                }
                if matched == 1 {
                    break 'groups;
                }
                let p = pos as usize;
                if special {
                    if !special_armed {
                        // `subu s0,s0,(s2 + 2)`, `s6` left at the length.
                        pos -= matched as isize + 2;
                        pos += 1;
                        continue;
                    }
                    if ordinal == 0 {
                        miracle_matched = true;
                        break;
                    }
                    hyper_matches.push(0x1B + s3 as u8);
                    let run_start = p + 1 - matched;
                    q[run_start] = SPECIAL_STARTER;
                    let written = if rec.sources[1] == 0 {
                        if run_start + 1 < QUEUE_LEN {
                            q[run_start + 1] = 0x1B + s3 as u8;
                        }
                        1
                    } else {
                        let mut n = 0usize;
                        while n < 4 && rec.sources[n] != 0 {
                            if run_start + 1 + n < QUEUE_LEN {
                                q[run_start + 1 + n] = 0x1C + (s3 + n) as u8;
                            }
                            n += 1;
                        }
                        s3 += n;
                        n
                    };
                    // `0x801EF648..0x801EF6C4`: the queue past the run moves
                    // down behind the constants, marks with it. The bytes
                    // the move vacates keep what they held.
                    let mut a3 = 1usize;
                    while p + a3 < QUEUE_LEN {
                        let dst = run_start + written + a3;
                        if dst < QUEUE_LEN {
                            q[dst] = q[p + a3];
                            marks[dst] = marks[p + a3];
                        }
                        a3 += 1;
                    }
                } else {
                    q[p] = STARTER;
                    if p < QUEUE_LEN - 2 {
                        for k in (p + 1..QUEUE_LEN - 1).rev() {
                            q[k + 1] = q[k];
                            marks[k + 1] = marks[k];
                        }
                    }
                    if p + 1 < QUEUE_LEN {
                        marks[p + 1] = marks[p];
                        q[p + 1] = 0x1B + s3 as u8;
                    }
                    marks[p] = 1;
                }
                matched = 0;
                pos = start as isize + 1;
            }
            s3 = group_end(records, s3);
            ordinal += 1;
        }
    }
    BankTokens {
        queue: q,
        marks,
        hyper_matches,
        miracle_matched,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queue::Command::{Down, Left, Right, Up};

    fn ac(b: u8) -> ActionConstant {
        ActionConstant::from_byte(b).unwrap()
    }

    // Vahn's normal arts in grid order (ids 4..14 -> constants 0x1F..0x29),
    // arrows as the SCUS arts-name table spells them.
    fn vahn() -> Vec<(ActionConstant, &'static [Command])> {
        vec![
            (ac(0x1F), &[Down, Up, Up, Up][..]),      // Cyclone
            (ac(0x20), &[Up, Up, Down, Down][..]),    // Hurricane
            (ac(0x21), &[Down, Up, Up, Left][..]),    // PK Combo
            (ac(0x22), &[Up, Down, Right, Left][..]), // Spin Combo
            (ac(0x23), &[Left, Right, Up, Left][..]), // Pyro Pummel
            (ac(0x24), &[Down, Down, Down, Up][..]),  // Cross-Kick
            (ac(0x25), &[Left, Left, Down][..]),      // Power Punch
            (ac(0x26), &[Up, Down, Left][..]),        // Slash Kick
            (ac(0x27), &[Up, Down, Up][..]),          // Somersault
            (ac(0x28), &[Down, Right, Up][..]),       // Charging Scorch
            (ac(0x29), &[Left, Right, Left][..]),     // Hyper Elbow
        ]
    }

    #[test]
    fn a_lone_art_keeps_its_leading_arrows() {
        let q = tokenize(&vahn(), &[Up, Down, Up]);
        assert_eq!(populated(&q), &[0x0F, 0x0E, 0x19, 0x27]);
    }

    #[test]
    fn tri_somersault_input_reproduces_the_capture() {
        // docs/tooling/super-art-queue-capture.md: the resident queue before
        // the Super applier's tail replace.
        let q = tokenize(&vahn(), &[Up, Down, Up, Up, Up, Down, Up]);
        assert_eq!(
            populated(&q),
            &[0x0F, 0x0E, 0x19, 0x27, 0x0F, 0x19, 0x1F, 0x0E, 0x19, 0x27]
        );
    }

    #[test]
    fn naive_concatenation_does_not_trigger_the_super() {
        // Somersault + Cyclone + Somersault typed back to back tokenizes to
        // four arts, not the Tri-Somersault tail.
        let q = tokenize(&vahn(), &[Up, Down, Up, Down, Up, Up, Up, Up, Down, Up]);
        let p = populated(&q);
        let find = [0x19, 0x27, 0x0F, 0x19, 0x1F, 0x0E, 0x19, 0x27];
        assert_ne!(&p[p.len() - find.len()..], &find[..]);
    }

    #[test]
    fn derives_every_vahn_super_input() {
        let arts = vahn();
        let cases: &[(&str, &[u8], &[Command])] = &[
            (
                "Tri-Somersault",
                &[0x19, 0x27, 0x0F, 0x19, 0x1F, 0x0E, 0x19, 0x27],
                &[Up, Down, Up, Up, Up, Down, Up],
            ),
            (
                "Maximum Blow",
                &[0x19, 0x28, 0x0E, 0x19, 0x26, 0x0C, 0x19, 0x25],
                &[Down, Right, Up, Down, Left, Left, Down],
            ),
            (
                "Rolling Combo",
                &[0x19, 0x22, 0x0C, 0x19, 0x25, 0x0F, 0x0F, 0x19, 0x21],
                &[Up, Down, Right, Left, Left, Down, Up, Up, Left],
            ),
        ];
        for (name, find, want) in cases {
            let got = derive_super_input(&arts, find).unwrap_or_else(|| panic!("{name}"));
            assert_eq!(&got[..], *want, "{name}");
        }
    }

    #[test]
    fn art_ends_mark_each_overlapping_arts_last_arrow() {
        let arts = vahn();
        assert_eq!(
            art_ends(&arts, &[Up, Down, Up, Up, Up, Down, Up]),
            vec![2, 4, 6]
        );
        // Rolling Combo: Spin Combo (0..3), Power Punch (3..5), PK Combo (5..8).
        assert_eq!(
            art_ends(&arts, &[Up, Down, Right, Left, Left, Down, Up, Up, Left]),
            vec![3, 5, 8]
        );
        assert_eq!(art_ends(&arts, &[Up, Down, Up]), vec![2]);
        assert!(art_ends(&arts, &[Left, Left]).is_empty());
    }

    #[test]
    fn a_pattern_no_art_produces_derives_nothing() {
        assert!(derive_super_input(&vahn(), &[0x19, 0x27, 0x0C, 0x0C, 0x0C, 0x19, 0x1F]).is_none());
        assert!(derive_super_input(&vahn(), &[0x19, 0x50]).is_none());
    }
    /// A bank built from `(combo, sources)` rows, record `0x1B` first.
    fn bank(rows: &[(&'static [Command], [u8; 4])]) -> Vec<BankArt<'static>> {
        rows.iter()
            .map(|&(cmds, sources)| BankArt { cmds, sources })
            .collect()
    }

    // Gala's bank head: Biron Rage, Explosive Fist, Lightning Storm, Thunder
    // Punch, then normal arts (the record `+0x00` combos; `sources[0]` is
    // each record's own stream).
    fn gala() -> Vec<BankArt<'static>> {
        bank(&[
            (
                &[Right, Right, Down, Up, Down, Up, Down, Left, Left],
                [0; 4],
            ),
            (&[Right, Right, Left, Left, Left], [0x0C, 0, 0, 0]),
            (&[Right, Right, Up, Left], [0x04, 0, 0, 0]),
            (&[Right, Right, Left], [0x0A, 0, 0, 0]),
            (&[Left, Up, Right, Down, Left], [0x01, 0, 0, 0]),
            (&[Up, Left, Down, Right, Left], [0x11, 0, 0, 0]),
        ])
    }

    #[test]
    fn a_hyper_art_collapses_to_its_starter_and_constant() {
        // `koru_pre_*`: Gala's committed Lightning Storm reads `1A 1D`.
        let t = tokenize_bank(&gala(), &[Right, Right, Up, Left], true);
        assert_eq!(populated(&t.queue), &[0x1A, 0x1D]);
        assert_eq!(t.hyper_matches, vec![0x1D]);
        assert_eq!(t.marks, [0; QUEUE_LEN], "the special arm marks nothing");
        // The queue past the run moves down behind it.
        let t = tokenize_bank(&gala(), &[Right, Right, Left, Down, Down], true);
        assert_eq!(populated(&t.queue), &[0x1A, 0x1E, 0x0E, 0x0E]);
    }

    #[test]
    fn a_hyper_art_writes_nothing_without_the_ra_seru() {
        let t = tokenize_bank(&gala(), &[Right, Right, Up, Left], false);
        assert_eq!(populated(&t.queue), &[0x0D, 0x0D, 0x0F, 0x0C]);
        assert!(t.hyper_matches.is_empty());
    }

    #[test]
    fn normal_arts_tokenize_as_before_beside_the_special_groups() {
        let t = tokenize_bank(&gala(), &[Left, Up, Right, Down, Left], true);
        assert_eq!(populated(&t.queue), &[0x0C, 0x0F, 0x0D, 0x0E, 0x19, 0x1F]);
        assert_eq!(t.marks[4], 1);
    }

    #[test]
    fn a_chained_group_counts_once_toward_the_special_ordinals() {
        // Noa: Miracle, the chained Hurricane Kick (records 0x1C..0x1E),
        // Vulture Blade, Frost Breath - the latter two are Hyper Arts.
        let hk: &[Command] = &[Left, Up, Up, Up, Up, Down, Right];
        let noa = bank(&[
            (&[Left, Up, Right, Down, Up, Left, Up, Down, Right], [0; 4]),
            (hk, [0x04, 0x05, 0, 0]),
            (hk, [0x04, 0, 0, 0]),
            (hk, [0x05, 0, 0, 0]),
            (&[Left, Left, Right, Left, Right], [0x0D, 0, 0, 0]),
            (&[Left, Left, Right, Right], [0x08, 0, 0, 0]),
            (&[Right, Right, Left, Up, Up, Up], [0x01, 0, 0, 0]),
        ]);
        let t = tokenize_bank(&noa, hk, true);
        assert_eq!(populated(&t.queue), &[0x1A, 0x1D, 0x1E]);
        assert_eq!(t.hyper_matches, vec![0x1C]);
        let t = tokenize_bank(&noa, &[Left, Left, Right, Left, Right], true);
        assert_eq!(populated(&t.queue), &[0x1A, 0x1F]);
        let t = tokenize_bank(&noa, &[Left, Left, Right, Right], true);
        assert_eq!(populated(&t.queue), &[0x1A, 0x20]);
        // The fifth group is the first normal art.
        let t = tokenize_bank(&noa, &[Right, Right, Left, Up, Up, Up], true);
        assert_eq!(
            populated(&t.queue),
            &[0x0D, 0x0D, 0x0C, 0x0F, 0x0F, 0x19, 0x21]
        );
        let consts = hyper_queue_constants(&noa);
        assert_eq!(
            consts,
            vec![
                (0x1C, vec![0x1D, 0x1E]),
                (0x1F, vec![0x1F]),
                (0x20, vec![0x20])
            ]
        );
    }

    #[test]
    fn the_bank_walk_agrees_with_the_normal_tokenizer() {
        // Vahn's normal arts behind four empty special groups.
        let normal = vahn();
        let mut rows: Vec<BankArt<'static>> = vec![BankArt::default(); 4];
        rows.extend(normal.iter().map(|&(_, cmds)| BankArt {
            cmds,
            sources: [1, 0, 0, 0],
        }));
        for input in [
            &[Up, Down, Up][..],
            &[Up, Down, Up, Up, Up, Down, Up],
            &[Down, Right, Up, Down, Left, Left, Down],
            &[Up, Down, Right, Left, Left, Down, Up, Up, Left],
        ] {
            assert_eq!(
                tokenize_bank(&rows, input, true).queue,
                tokenize(&normal, input)
            );
        }
    }
}
