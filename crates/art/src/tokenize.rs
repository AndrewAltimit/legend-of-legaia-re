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
//! nothing). Every Super Art chain is normal arts only, so
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
}
