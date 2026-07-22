//! Battle SFX-cue router: cue id -> the 4-slot pending ring / XA voice clip.
//!
//! PORT: FUN_8004fe5c
//!
//! `FUN_8004FE5C(id, category)` is the battle overlay's one funnel for
//! "play this sound now": every battle cue - menu blips, hit impacts,
//! elemental cast tones, arts shouts - passes through it. It either
//! appends a resolved cue id to the 4-slot pending ring at `_DAT_8007B6D8`
//! (counter = byte `+9` of the `gp[+0xA0C]` battle context; drained by
//! `FUN_80016B6C` against the SFX descriptor table - see
//! `docs/formats/sfx-table.md`) or, for ids `>= 0x100` fired by a party
//! attacker, starts a CD-XA **voice clip** (the arts shout) via
//! `FUN_8003D53C`.
//!
//! Ported from the disassembly (`see ghidra/scripts/funcs/8004fe5c.txt`)
//! cross-checked block-for-block against the static-recomp rendering of
//! `func_8004FE5C` (708 bytes / 33 blocks). The id space routes as:
//!
//! | id | category | action |
//! |---|---|---|
//! | `>= 0x100` | `< 3` (party) | XA clip: `clip = (id-0x100) >> 3` (remap 1/3/5 → 26/27/28), `channel = (id-0x100) & 7`, `duration = (tbl[id-0x100]*60 + 99)/100` sectors (per-clip u16 table at `0x800788B8`). Gated on the tutorial byte (`ctx+0x276 == 0`) and CD-read idle (`FUN_8003DE7C(1) == 0`); snapshots the vsync counter to `gp+0x9F0`. |
//! | `< 0x48` | `>= 3` and `0x1B <= id` | element-tinted: byte `+4` of the runtime-bank descriptor of `id + 0x281` gets the attacker's element byte, ring gets `id + 0x281` |
//! | `< 0x48` | otherwise | ring gets `id - 1` (the plain static-table cue) |
//! | `0x48..0x64` | any | ring gets `id` unchanged |
//! | `>= 0x64` | any (incl. `>= 0x100` from a non-party `category >= 3`) | element-tinted: descriptor of `id + 0x19C` gets the element byte (literal `2` when `category < 3` and `id >= 0xA7`), ring gets `id + 0x19C` |
//!
//! Both tinted legs write `runtime_bank + enqueued_ring_id*8 - 0x1000 + 4`
//! (`_DAT_8007B990 + id*8 + 0x40C` / `- 0x31C` in the raw address math -
//! the same byte once you subtract the runtime-bank id base `0x200`,
//! see [`RUNTIME_BANK_ID_BASE`]).
//!
//! **Dedupe**: a cue whose resolved ring id the last-played word
//! `DAT_8007B724` already holds is dropped (`id - 1` for the low leg,
//! `id + 0x19C` for the high leg; the XA leg never dedupes).
//! **Wrap**: after any append, a counter `> 3` resets to 0 - the ring
//! really is 4 slots.
//!
//! The element byte itself comes from the attacker's battle actor:
//! `*(actor_table[category] + 0x22C) + 0x80` with the actor-pointer table
//! at `0x801C9370` - the caller supplies it through [`SfxCueSources`],
//! keeping this module free of battle-actor layout.

/// Runtime-bank cue ids start here; ids below are static-table cues
/// (`docs/formats/sfx-table.md`).
pub const RUNTIME_BANK_ID_BASE: u16 = 0x200;

/// XA arts-voice clip request (the `FUN_8003D53C` call, decoded).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct XaVoiceClip {
    /// Clip-table index (`0x801C6ED8` table).
    pub clip: u32,
    /// XA channel within the clip file.
    pub channel: u32,
    /// Play length in sectors: `(raw*60 + 99) / 100`.
    pub duration_sectors: u32,
}

/// What one cue routed to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SfxCueOutcome {
    /// Ring id appended this call (`None` = dropped or XA leg).
    pub enqueued: Option<u16>,
    /// Element byte written into the runtime-bank descriptor (`+4`) of the
    /// enqueued id.
    pub element_write: Option<u8>,
    /// XA voice clip started this call.
    pub xa: Option<XaVoiceClip>,
}

/// Host-supplied inputs the router reads.
pub struct SfxCueSources<'a> {
    /// Attacker element byte per category slot
    /// (`*(0x801C9370[cat] + 0x22C) + 0x80`).
    pub element_of: &'a dyn Fn(u8) -> u8,
    /// Per-clip raw duration (`u16` table at `0x800788B8`, index
    /// `id - 0x100`).
    pub xa_duration_raw: &'a dyn Fn(u32) -> u16,
    /// Battle-tutorial byte (`ctx+0x276`): non-zero suppresses voice clips.
    pub tutorial_active: bool,
    /// `FUN_8003DE7C(1) != 0`: a CD read is in flight, voice clip skipped.
    pub cd_read_busy: bool,
}

/// The 4-slot pending-cue ring (`_DAT_8007B6D8` + context counter byte).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SfxCueRing {
    /// Pending cue ids (`_DAT_8007B6D8`, u16 x 4).
    pub slots: [u16; 4],
    /// Write counter (`ctx+9`); wraps to 0 after exceeding 3.
    pub counter: u8,
    /// Last-played dedupe word (`DAT_8007B724`, maintained by the drainer).
    pub last_played: u32,
}

impl SfxCueRing {
    fn push(&mut self, id: u16) {
        // Retail indexes `counter * 2` bytes into the ring without masking;
        // the wrap below keeps it in range before the next call.
        self.slots[usize::from(self.counter) & 3] = id;
        self.counter = self.counter.wrapping_add(1);
    }

    fn wrap(&mut self) {
        if self.counter > 3 {
            self.counter = 0;
        }
    }
}

/// Route one battle cue. Mirrors `FUN_8004FE5C(id, category)`.
pub fn route_sfx_cue(
    ring: &mut SfxCueRing,
    id: u32,
    category: u8,
    src: &SfxCueSources,
) -> SfxCueOutcome {
    let mut out = SfxCueOutcome::default();
    if id >= 0x100 && category < 3 {
        // Arts-voice XA clip leg. No ring write, no wrap.
        if !src.tutorial_active && !src.cd_read_busy {
            let v = id - 0x100;
            let clip = match v >> 3 {
                1 => 26,
                3 => 27,
                5 => 28,
                c => c,
            };
            let raw = u32::from((src.xa_duration_raw)(v));
            out.xa = Some(XaVoiceClip {
                clip,
                channel: v & 7,
                duration_sectors: (raw * 60).div_ceil(100),
            });
        }
        return out;
    }
    if id < 0x48 {
        // Low static-table leg.
        if ring.last_played == id.wrapping_sub(1) {
            ring.wrap();
            return out;
        }
        if category >= 3 && id >= 0x1B {
            // Element-tinted runtime-bank cue (id < 0x48 < 0x64 always
            // satisfies the `< 100` arm of the retail test).
            let rid = (id + 0x281) as u16;
            out.element_write = Some((src.element_of)(category));
            ring.push(rid);
            out.enqueued = Some(rid);
        } else {
            let rid = id.wrapping_sub(1) as u16;
            ring.push(rid);
            out.enqueued = Some(rid);
        }
    } else {
        // High leg (0x48.. plus the >= 0x100 non-party fall-through).
        if ring.last_played == id + 0x19C {
            ring.wrap();
            return out;
        }
        if id < 0x64 {
            ring.push(id as u16);
            out.enqueued = Some(id as u16);
        } else {
            let elem = if category < 3 && id >= 0xA7 {
                2
            } else {
                (src.element_of)(category)
            };
            let rid = (id + 0x19C) as u16;
            out.element_write = Some(elem);
            ring.push(rid);
            out.enqueued = Some(rid);
        }
    }
    ring.wrap();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn srcs() -> SfxCueSources<'static> {
        SfxCueSources {
            element_of: &|cat| 0x10 + cat,
            xa_duration_raw: &|v| (v as u16) * 10 + 5,
            tutorial_active: false,
            cd_read_busy: false,
        }
    }

    #[test]
    fn low_ids_enqueue_id_minus_one() {
        let mut r = SfxCueRing::default();
        let o = route_sfx_cue(&mut r, 0x21, 0, &srcs());
        assert_eq!(o.enqueued, Some(0x20));
        assert_eq!(o.element_write, None);
        assert_eq!(r.slots[0], 0x20);
        assert_eq!(r.counter, 1);
    }

    #[test]
    fn low_ids_from_non_party_get_element_tint() {
        let mut r = SfxCueRing::default();
        // cat >= 3 and 0x1B <= id < 0x48: runtime-bank tinted cue.
        let o = route_sfx_cue(&mut r, 0x1B, 3, &srcs());
        assert_eq!(o.enqueued, Some(0x1B + 0x281));
        assert_eq!(o.element_write, Some(0x13));
        // Below 0x1B stays plain even for cat >= 3.
        let o = route_sfx_cue(&mut r, 0x1A, 3, &srcs());
        assert_eq!(o.enqueued, Some(0x19));
        assert_eq!(o.element_write, None);
    }

    #[test]
    fn mid_band_enqueues_raw_id() {
        let mut r = SfxCueRing::default();
        let o = route_sfx_cue(&mut r, 0x48, 0, &srcs());
        assert_eq!(o.enqueued, Some(0x48));
        let o = route_sfx_cue(&mut r, 0x63, 5, &srcs());
        assert_eq!(o.enqueued, Some(0x63));
        assert_eq!(o.element_write, None);
    }

    #[test]
    fn high_band_tints_and_offsets() {
        let mut r = SfxCueRing::default();
        // Party attacker, id < 0xA7: element from the actor table.
        let o = route_sfx_cue(&mut r, 0x64, 1, &srcs());
        assert_eq!(o.enqueued, Some(0x64 + 0x19C));
        assert_eq!(o.element_write, Some(0x11));
        // Party attacker, id >= 0xA7: literal element 2.
        let o = route_sfx_cue(&mut r, 0xA7, 1, &srcs());
        assert_eq!(o.element_write, Some(2));
        // Non-party attacker keeps the table element even at high ids.
        let o = route_sfx_cue(&mut r, 0xA7, 4, &srcs());
        assert_eq!(o.element_write, Some(0x14));
    }

    #[test]
    fn party_high_ids_start_xa_voice_clip() {
        let mut r = SfxCueRing::default();
        // id 0x100 + 8 -> clip (8>>3)=1 remapped to 26, channel 0.
        let o = route_sfx_cue(&mut r, 0x108, 0, &srcs());
        let xa = o.xa.unwrap();
        assert_eq!(xa.clip, 26);
        assert_eq!(xa.channel, 0);
        // raw = 8*10+5 = 85 -> (85*60+99)/100 = 51 sectors.
        assert_eq!(xa.duration_sectors, 51);
        assert_eq!(o.enqueued, None);
        assert_eq!(r.counter, 0, "XA leg never touches the ring");
        // Remaps 3 -> 27, 5 -> 28; others pass through.
        assert_eq!(
            route_sfx_cue(&mut r, 0x118, 0, &srcs()).xa.unwrap().clip,
            27
        );
        assert_eq!(
            route_sfx_cue(&mut r, 0x128, 0, &srcs()).xa.unwrap().clip,
            28
        );
        assert_eq!(route_sfx_cue(&mut r, 0x110, 0, &srcs()).xa.unwrap().clip, 2);
        // Channel = low 3 bits.
        assert_eq!(
            route_sfx_cue(&mut r, 0x10D, 2, &srcs()).xa.unwrap().channel,
            5
        );
    }

    #[test]
    fn xa_leg_gates_on_tutorial_and_cd_busy() {
        let mut r = SfxCueRing::default();
        let mut s = srcs();
        s.tutorial_active = true;
        assert_eq!(route_sfx_cue(&mut r, 0x108, 0, &s).xa, None);
        let mut s = srcs();
        s.cd_read_busy = true;
        assert_eq!(route_sfx_cue(&mut r, 0x108, 0, &s).xa, None);
    }

    #[test]
    fn non_party_high_ids_fall_into_the_ring_leg() {
        let mut r = SfxCueRing::default();
        // cat >= 3 with id >= 0x100: no XA - element-tinted ring cue.
        let o = route_sfx_cue(&mut r, 0x100, 3, &srcs());
        assert_eq!(o.xa, None);
        assert_eq!(o.enqueued, Some(0x100 + 0x19C));
        assert_eq!(o.element_write, Some(0x13));
    }

    #[test]
    fn dedupe_against_last_played() {
        let mut r = SfxCueRing {
            last_played: 0x20,
            ..Default::default()
        };
        // Low leg dedupes on id-1.
        assert_eq!(route_sfx_cue(&mut r, 0x21, 0, &srcs()).enqueued, None);
        // High leg dedupes on id+0x19C.
        r.last_played = 0x64 + 0x19C;
        assert_eq!(route_sfx_cue(&mut r, 0x64, 0, &srcs()).enqueued, None);
        // Different id passes.
        assert!(route_sfx_cue(&mut r, 0x65, 3, &srcs()).enqueued.is_some());
    }

    #[test]
    fn counter_wraps_after_four_appends() {
        let mut r = SfxCueRing::default();
        for i in 0..4 {
            route_sfx_cue(&mut r, 0x30 + i, 0, &srcs());
        }
        assert_eq!(r.counter, 0, "counter > 3 resets to 0");
        assert_eq!(r.slots, [0x2F, 0x30, 0x31, 0x32]);
    }
}
