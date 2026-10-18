//! Battle **equipment mesh-variant swap** - the per-frame pass that decides
//! which TMD object sits in each of a party character's two attach-bone
//! animation channels - and the per-channel model table it writes.
//!
//! PORT: FUN_8004ccd4
//!
//! Retail writes object pointers into the render node's per-channel model
//! table (`*(node + 0x44) + 4 + channel*4`), the array the battle draw pass
//! `FUN_80048A08` hands to the TMD renderer once per animation channel, for as
//! many channels as the playing stream has parts. [`BattleModelTable`] is that
//! table for one party seat, as object indices into the assembled battle TMD:
//! the identity at registration, re-written by [`resolve_attach_swap`] every
//! anim tick, and read back as the set of objects the draw pass draws
//! ([`BattleModelTable::drawn_objects`]). The world keeps one per party seat
//! (`Actor::battle_model_table` in `legaia-engine-core`) and both play hosts
//! draw only the objects it names.
//!
//! REF: FUN_80048a08 - the battle draw pass that reads the table this fills.
//! REF: FUN_8004c7b4 - the facial animator, called immediately before this from
//! the same render-node tick under the same guards.
//! REF: FUN_80049348 - the arts after-image renderer, which re-runs this pass
//! per ghost with that ghost's historical cursor + entry.
//! REF: FUN_800513f0 - the registration pass that snapshots the pair table
//! ([`AttachPairs::from_assembly`]) at battle load.
//! REF: FUN_80053898 - the assembly post-pass that sorts the variants to the
//! end of the object table and records the pinned ordinal.
//!
//! # The shape of the decision
//!
//! Each party slot owns **two** attach pairs. A pair is
//! `(default_object, variant_object)` plus the animation `channel` the chosen
//! object is installed into - all three snapshotted at battle load into the
//! battle context (`ctx + 0x1030` / `+0x1034` / `+0x23A`) by
//! `FUN_800513F0`. The pair whose 1-based ordinal matches `ctx + 0x240 + slot`
//! is the **pinned** pair.
//!
//! Per frame the pass runs one of two paths:
//!
//! * **Extra-channel escape.** When the playing stream's part count differs
//!   from the idle stream's, the clip is one of the surplus-object swings; the
//!   pass force-installs the *pinned* pair's variant and returns, touching no
//!   other channel.
//! * **Window test.** Otherwise, for each of the two pairs: the pinned pair
//!   never tests and always takes its default. The other pair tests the
//!   playing entry's two `[start, end]` byte windows at `entry + 0xA4`
//!   (`start <= frame <= end`, `end != 0`); a hit installs the variant, no hit
//!   installs the default.
//!
//! Retail's window test lives in the *inner* loop and can fire twice, storing
//! the same value twice - harmless, and modelled here as a single write with a
//! hit count so the redundancy is visible rather than smoothed away.
//!
//! Full byte layout: `docs/formats/battle-data-pack.md`
//! § "Equipment-variant track". Source: `ghidra/scripts/funcs/8004ccd4.txt`.

/// Number of attach pairs per party slot.
pub const ATTACH_PAIRS: usize = 2;
/// Number of `[start, end]` frame windows each pair tests.
pub const WINDOWS_PER_PAIR: usize = 2;

/// One attach pair as `FUN_800513F0` snapshots it into the battle context.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AttachPair {
    /// Animation channel the chosen object is installed into
    /// (`ctx + 0x23A + slot*2 + pair`).
    pub channel: u8,
    /// The bone's own object - the out-of-window choice
    /// (`ctx + 0x1030 + slot*0x10 + pair*8`).
    pub default_object: u32,
    /// The surplus `0xFF` equipment object - the in-window choice
    /// (`ctx + 0x1034 + ...`).
    pub variant_object: u32,
}

/// The per-slot snapshot the pass reads.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AttachPairs {
    /// The two pairs, in ordinal order.
    pub pairs: [AttachPair; ATTACH_PAIRS],
    /// `ctx + 0x240 + slot` - the **1-based** ordinal of the pinned pair. `0`
    /// (or anything outside `1..=2`) pins nothing, so both pairs test their
    /// windows.
    pub pinned_ordinal: u8,
}

/// One `[start, end]` frame window from the playing entry's `+0xA4` track.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AttachWindow {
    /// Inclusive first frame.
    pub start: u8,
    /// Inclusive last frame. `0` disables the window regardless of `start`.
    pub end: u8,
}

impl AttachWindow {
    /// Retail's activity rule: `start <= frame && frame <= end && end != 0`.
    pub fn active(self, frame: i16) -> bool {
        self.end != 0 && frame >= i16::from(self.start) && frame <= i16::from(self.end)
    }
}

/// Which object a pair resolved to, and why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachChoice {
    /// The pinned pair, or a pair whose windows all missed.
    Default,
    /// A window hit. `hits` is how many of the pair's two windows matched -
    /// retail stores the variant once per hit, so `2` means a duplicated store.
    Variant {
        /// Matching window count (`1` or `2`).
        hits: u8,
    },
    /// The extra-channel escape force-installed the variant.
    Escape,
}

/// One store the pass makes into the render node's per-channel model table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttachWrite {
    /// Pair ordinal (0-based) this store came from.
    pub pair: usize,
    /// Animation channel index - the table slot written.
    pub channel: u8,
    /// Object handle stored.
    pub object: u32,
    /// Why that object.
    pub choice: AttachChoice,
}

/// The whole pass's output. At most one write per pair, and exactly one on the
/// escape path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AttachSwap {
    /// Stores in retail order.
    pub writes: Vec<AttachWrite>,
    /// `true` when the extra-channel escape fired.
    pub escaped: bool,
}

impl AttachSwap {
    /// Apply the stores to a per-channel model table, the way retail's
    /// `*(node + 0x44) + 4` array is written. Out-of-range channels are
    /// dropped rather than clobbering neighbouring memory the way retail's raw
    /// pointer arithmetic would.
    pub fn apply(&self, table: &mut [u32]) {
        for w in &self.writes {
            if let Some(slot) = table.get_mut(w.channel as usize) {
                *slot = w.object;
            }
        }
    }
}

impl AttachPairs {
    /// The battle-load snapshot `FUN_800513F0` takes for a party slot, from
    /// the assembled blob's tag tables (`0x80051614..0x800516BC`): with
    /// `nobj` objects, pair `k`'s channel is the attach-bone side-table byte
    /// `blob[nobj + k]`, its default the model-table entry at that channel
    /// (the bone's own object), its variant the entry at `nobj - 2 + k` (the
    /// `0xFF` surplus the post-pass sorted last).
    ///
    /// `attach_bones` is that side table (`AssembledCharacter::attach_bones`
    /// in `legaia-actor-formats`) and `pinned_ordinal` the post-pass's ctx
    /// `+0x240` byte. Object handles are object indices. `None` when the
    /// assembly carries fewer than two variants: retail reads the two
    /// side-table bytes unconditionally, and with one missing it would
    /// install a stale byte's object - the engine declines instead and draws
    /// the identity table.
    pub fn from_assembly(nobj: usize, attach_bones: &[u8], pinned_ordinal: u8) -> Option<Self> {
        if attach_bones.len() < ATTACH_PAIRS || nobj < ATTACH_PAIRS {
            return None;
        }
        let pairs = std::array::from_fn(|k| AttachPair {
            channel: attach_bones[k],
            default_object: u32::from(attach_bones[k]),
            variant_object: (nobj - ATTACH_PAIRS + k) as u32,
        });
        Some(Self {
            pairs,
            pinned_ordinal,
        })
    }
}

/// Split an entry's eight `+0xA4` bytes into the four windows
/// [`resolve_attach_swap`] tests, in `[pair0_w0, pair0_w1, pair1_w0,
/// pair1_w1]` order.
pub fn windows_from_entry(bytes: &[u8; 8]) -> [AttachWindow; ATTACH_PAIRS * WINDOWS_PER_PAIR] {
    std::array::from_fn(|w| AttachWindow {
        start: bytes[w * 2],
        end: bytes[w * 2 + 1],
    })
}

/// One party seat's per-channel model table (`*(node + 0x44) + 4`) plus the
/// battle-load snapshot that rewrites it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BattleModelTable {
    /// Object index per channel. Starts as the identity - registration lays
    /// the TMD's objects out in order - and keeps whatever the last pass
    /// stored: the extra-channel escape rewrites only the pinned pair's
    /// channel, so the other pair's channel holds its previous object.
    pub table: Vec<u32>,
    /// The two attach pairs and the pinned ordinal.
    pub pairs: AttachPairs,
    /// Part count of the seat's idle stream (`**(ctx_slot) + 0xAC`) - the
    /// escape's comparand.
    pub idle_parts: u8,
}

impl BattleModelTable {
    /// The registration-time table for an `nobj`-object mesh.
    pub fn new(nobj: usize, pairs: AttachPairs, idle_parts: u8) -> Self {
        Self {
            table: (0..nobj as u32).collect(),
            pairs,
            idle_parts,
        }
    }

    /// Run `FUN_8004CCD4` for one anim tick and store its writes.
    ///
    /// * `windows` - the playing entry's `+0xA4` bytes.
    /// * `frame` - the playing clip's whole-keyframe cursor (`node[+0x68]`
    ///   sign-extended and `>> 4`).
    /// * `playing_parts` - the playing stream's own part count.
    pub fn step(&mut self, windows: &[u8; 8], frame: i16, playing_parts: u8) -> AttachSwap {
        let swap = resolve_attach_swap(
            &self.pairs,
            &windows_from_entry(windows),
            frame,
            playing_parts,
            self.idle_parts,
        );
        swap.apply(&mut self.table);
        swap
    }

    /// Which objects the draw pass `FUN_80048A08` draws this frame: the
    /// table's entries for channels `0..playing_parts`
    /// (`while (c < **(entry + 0x88))`). Indexed by object.
    pub fn drawn_objects(&self, playing_parts: usize) -> Vec<bool> {
        let mut drawn = vec![false; self.table.len()];
        for &o in self.table.iter().take(playing_parts) {
            if let Some(d) = drawn.get_mut(o as usize) {
                *d = true;
            }
        }
        drawn
    }
}

/// Resolve one party slot's attach channels for one animation frame.
///
/// * `pairs` - the battle-load snapshot for this slot.
/// * `windows` - the playing entry's four `+0xA4` window bytes, as two windows
///   per pair in `[pair0_w0, pair0_w1, pair1_w0, pair1_w1]` order.
/// * `frame` - the clip cursor (`(node[+0x68] << 16) >> 20`, already
///   sign-extended and `>> 4`ed by the caller).
/// * `playing_parts` / `idle_parts` - first byte of the playing stream
///   (`*(entry + 0x88)`) and of the idle stream (`**(ctx_slot) + 0xAC`). A
///   mismatch takes the escape.
pub fn resolve_attach_swap(
    pairs: &AttachPairs,
    windows: &[AttachWindow; ATTACH_PAIRS * WINDOWS_PER_PAIR],
    frame: i16,
    playing_parts: u8,
    idle_parts: u8,
) -> AttachSwap {
    let mut out = AttachSwap::default();

    if playing_parts != idle_parts {
        // Extra-channel escape: the pinned pair's variant, unconditionally.
        // Retail indexes `pair = pinned_ordinal - 1` with no bound check; a
        // zero ordinal underflows there, so the engine simply declines.
        out.escaped = true;
        let Some(pair) = (pairs.pinned_ordinal as usize).checked_sub(1) else {
            return out;
        };
        if let Some(p) = pairs.pairs.get(pair) {
            out.writes.push(AttachWrite {
                pair,
                channel: p.channel,
                object: p.variant_object,
                choice: AttachChoice::Escape,
            });
        }
        return out;
    }

    for (pair, p) in pairs.pairs.iter().enumerate() {
        // The pinned pair skips the window test entirely.
        let pinned = pairs.pinned_ordinal as usize == pair + 1;
        let hits = if pinned {
            0
        } else {
            windows[pair * WINDOWS_PER_PAIR..][..WINDOWS_PER_PAIR]
                .iter()
                .filter(|w| w.active(frame))
                .count() as u8
        };
        let (object, choice) = if hits == 0 {
            (p.default_object, AttachChoice::Default)
        } else {
            (p.variant_object, AttachChoice::Variant { hits })
        };
        out.writes.push(AttachWrite {
            pair,
            channel: p.channel,
            object,
            choice,
        });
    }
    out
}

/// The battle TMD as the draw pass sees it this frame: a copy whose objects
/// outside `drawn` carry no primitives, so a mesh builder emits nothing for
/// them while every object keeps its index (and so its pose channel).
/// Objects past the end of `drawn` are kept.
pub fn mask_tmd(tmd: &legaia_tmd::Tmd, drawn: &[bool]) -> legaia_tmd::Tmd {
    let mut out = tmd.clone();
    for (o, &d) in out.objects.iter_mut().zip(drawn) {
        if !d {
            o.primitives_byte_size = 0;
            o.header.n_primitive = 0;
            o.claimed_n_primitive = 0;
        }
    }
    out
}

/// The objects `drawn` leaves out, as object indices - the browser page's
/// per-frame hide list.
pub fn hidden_objects(drawn: &[bool]) -> Vec<u32> {
    drawn
        .iter()
        .enumerate()
        .filter(|(_, d)| !**d)
        .map(|(o, _)| o as u32)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_assembly_reads_the_side_table_and_the_sorted_variants() {
        // 16 bones + one 0xFE extra + two 0xFF variants = 19 objects.
        let p = AttachPairs::from_assembly(19, &[9, 13], 2).unwrap();
        assert_eq!(p.pairs[0].channel, 9);
        assert_eq!(p.pairs[0].default_object, 9);
        assert_eq!(p.pairs[0].variant_object, 17);
        assert_eq!(p.pairs[1].channel, 13);
        assert_eq!(p.pairs[1].default_object, 13);
        assert_eq!(p.pairs[1].variant_object, 18);
        assert_eq!(p.pinned_ordinal, 2);
        assert!(AttachPairs::from_assembly(17, &[9], 0).is_none());
    }

    #[test]
    fn model_table_draws_the_streams_channels_and_swaps_in_the_variant() {
        let pairs = AttachPairs::from_assembly(19, &[9, 13], 0).unwrap();
        let mut t = BattleModelTable::new(19, pairs, 16);
        // Idle (16 parts, no windows): the bones only.
        t.step(&[0; 8], 0, 16);
        let d = t.drawn_objects(16);
        assert!(d[..16].iter().all(|&x| x));
        assert!(!d[16] && !d[17] && !d[18]);
        // A window over frame 5 on pair 0: channel 9 draws variant 17.
        t.step(&[3, 47, 0, 0, 0, 0, 0, 0], 5, 16);
        let d = t.drawn_objects(16);
        assert!(!d[9] && d[17]);
        assert_eq!(hidden_objects(&d), vec![9, 16, 18]);
        // Out of the window the default comes back.
        t.step(&[3, 47, 0, 0, 0, 0, 0, 0], 50, 16);
        assert!(t.drawn_objects(16)[9]);
    }

    #[test]
    fn escape_draws_the_extra_channel_and_keeps_the_other_pair() {
        let pairs = AttachPairs::from_assembly(19, &[9, 13], 1).unwrap();
        let mut t = BattleModelTable::new(19, pairs, 16);
        // Pair 1 in a window first, then a 17-part swing: the escape only
        // rewrites the pinned pair 0's channel; pair 1 keeps its variant.
        t.step(&[0, 0, 0, 0, 0, 9, 0, 0], 4, 16);
        assert_eq!(t.table[13], 18);
        t.step(&[0; 8], 4, 17);
        assert_eq!(t.table[9], 17);
        assert_eq!(t.table[13], 18);
        let d = t.drawn_objects(17);
        assert!(d[16], "the 0xFE extra draws on the surplus channel");
        assert!(d[17] && d[18] && !d[9] && !d[13]);
    }

    fn pairs(pinned: u8) -> AttachPairs {
        AttachPairs {
            pairs: [
                AttachPair {
                    channel: 4,
                    default_object: 0x1000,
                    variant_object: 0x2000,
                },
                AttachPair {
                    channel: 7,
                    default_object: 0x1100,
                    variant_object: 0x2100,
                },
            ],
            pinned_ordinal: pinned,
        }
    }

    const NO_WINDOWS: [AttachWindow; 4] = [AttachWindow { start: 0, end: 0 }; 4];

    #[test]
    fn all_zero_windows_reassert_the_defaults_every_frame() {
        // The retail census: Vahn / Gala / Terra carry all-zero windows.
        let s = resolve_attach_swap(&pairs(0), &NO_WINDOWS, 20, 16, 16);
        assert!(!s.escaped);
        assert_eq!(s.writes.len(), 2);
        for w in &s.writes {
            assert_eq!(w.choice, AttachChoice::Default);
        }
        let mut table = [0u32; 8];
        s.apply(&mut table);
        assert_eq!(table[4], 0x1000);
        assert_eq!(table[7], 0x1100);
    }

    #[test]
    fn a_window_hit_installs_the_variant() {
        let mut w = NO_WINDOWS;
        w[0] = AttachWindow { start: 3, end: 47 };
        let s = resolve_attach_swap(&pairs(2), &w, 20, 16, 16);
        assert_eq!(s.writes[0].choice, AttachChoice::Variant { hits: 1 });
        assert_eq!(s.writes[0].object, 0x2000);
        // Pair 1 is the pinned pair - never tests, always default.
        assert_eq!(s.writes[1].choice, AttachChoice::Default);
        assert_eq!(s.writes[1].object, 0x1100);
    }

    #[test]
    fn both_windows_of_a_pair_can_hit_and_retail_stores_twice() {
        let mut w = NO_WINDOWS;
        w[0] = AttachWindow { start: 0, end: 50 };
        w[1] = AttachWindow { start: 10, end: 30 };
        let s = resolve_attach_swap(&pairs(0), &w, 20, 16, 16);
        assert_eq!(s.writes[0].choice, AttachChoice::Variant { hits: 2 });
    }

    #[test]
    fn end_zero_disables_a_window_even_at_frame_zero() {
        let mut w = NO_WINDOWS;
        w[0] = AttachWindow { start: 0, end: 0 };
        let s = resolve_attach_swap(&pairs(0), &w, 0, 16, 16);
        assert_eq!(s.writes[0].choice, AttachChoice::Default);
    }

    #[test]
    fn window_bounds_are_inclusive_on_both_ends() {
        let win = AttachWindow { start: 3, end: 5 };
        assert!(!win.active(2));
        assert!(win.active(3));
        assert!(win.active(5));
        assert!(!win.active(6));
    }

    #[test]
    fn negative_frame_never_matches() {
        let win = AttachWindow { start: 0, end: 40 };
        assert!(!win.active(-1));
    }

    #[test]
    fn pinned_pair_ignores_its_own_windows() {
        let mut w = NO_WINDOWS;
        w[2] = AttachWindow { start: 0, end: 99 };
        w[3] = AttachWindow { start: 0, end: 99 };
        let s = resolve_attach_swap(&pairs(2), &w, 10, 16, 16);
        assert_eq!(s.writes[1].choice, AttachChoice::Default);
    }

    #[test]
    fn part_count_mismatch_escapes_to_the_pinned_variant_only() {
        // Noa's 0x1E weapon band: 17-part swing against 16 bones.
        let s = resolve_attach_swap(&pairs(2), &NO_WINDOWS, 10, 17, 16);
        assert!(s.escaped);
        assert_eq!(s.writes.len(), 1);
        assert_eq!(s.writes[0].pair, 1);
        assert_eq!(s.writes[0].channel, 7);
        assert_eq!(s.writes[0].object, 0x2100);
        assert_eq!(s.writes[0].choice, AttachChoice::Escape);
    }

    #[test]
    fn escape_with_no_pinned_pair_writes_nothing() {
        let s = resolve_attach_swap(&pairs(0), &NO_WINDOWS, 10, 17, 16);
        assert!(s.escaped);
        assert!(s.writes.is_empty());
    }

    #[test]
    fn apply_drops_out_of_range_channels() {
        let mut p = pairs(0);
        p.pairs[1].channel = 40;
        let s = resolve_attach_swap(&p, &NO_WINDOWS, 0, 16, 16);
        let mut table = [0u32; 8];
        s.apply(&mut table);
        assert_eq!(table[4], 0x1000);
        assert!(table.iter().all(|&v| v != 0x1100));
    }
}
