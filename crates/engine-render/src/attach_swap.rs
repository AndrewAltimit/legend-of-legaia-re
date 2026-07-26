//! Battle **equipment mesh-variant swap** - the per-frame pass that decides
//! which TMD object sits in each of a party character's two attach-bone
//! animation channels.
//!
//! PORT: FUN_8004ccd4
//!
//! NOT WIRED: the destination does not exist yet. Retail writes object pointers
//! into the render node's per-channel model table (`*(node + 0x44) + 4 +
//! channel*4`) - the array the battle draw pass hands to the TMD renderer once
//! per animation channel. The engine's battle render path draws an assembled
//! whole-character mesh (`legaia_asset::battle_char_assembly`) and has no
//! per-channel model table to write into, so there is no slice to pass. What
//! closes the gap is the battle draw path keeping the assembled pieces as
//! **channels** rather than one merged mesh; then [`resolve_attach_swap`] fills
//! the table verbatim.
//!
//! REF: FUN_80048a08 - the battle draw pass that reads the table this fills.
//! REF: FUN_8004c7b4 - the facial animator, called immediately before this from
//! the same render-node tick under the same guards.
//! REF: FUN_80049348 - the arts after-image renderer, which re-runs this pass
//! per ghost with that ghost's historical cursor + entry.
//! REF: FUN_800513f0 - the registration pass that snapshots the pair table
//! ([`AttachPairs`]) at battle load.
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
#[derive(Debug, Clone, Copy, Default)]
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

#[cfg(test)]
mod tests {
    use super::*;

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
