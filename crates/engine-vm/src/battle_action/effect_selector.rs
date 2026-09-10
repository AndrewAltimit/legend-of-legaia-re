//! The effect applier's **selector dispatch** - the jump table
//! `FUN_800402F4` indexes with its first argument.
//!
//! `docs/subsystems/battle-formulas.md` describes the applier as
//! `switch(selector) { case 0..0x83 }`, and a standing item on that page read
//! the cases above `0x0F` as arms that "handle stat-up animations,
//! status-clear, queue-end markers, and the multi-target item slot" and were
//! left un-decoded. **The table refutes that.** It is 132 words at `0x80014FA0`
//! (`sltiu v1, 0x84` at `0x80040448`, `jr v0` at `0x80040468`) and it holds
//! only **sixteen** distinct targets: selectors `0x00..=0x0E`, selector
//! `0x82`, and one shared default that 116 of the 132 slots point at. That
//! default is `0x800421A8`, the function's own epilogue - the `lw ra` of the
//! register restore - so a selector in `0x0F..=0x81` or `0x83` restores the
//! frame and returns without touching a single actor field.
//!
//! Selector `0x82` is not an arm either in any real sense: its slot points at
//! `0x800421A0`, two instructions above the epilogue, which is one
//! `jal 0x80046870` - the brightness ramp already ported as
//! [`crate::battle_helpers::advance_gauge`].
//!
//! So the applier's whole behavioural surface is selectors `0x00..=0x0E`, and
//! `0x0B` / `0x0C` / `0x0D` share one arm - fourteen bodies over fifteen
//! selectors. Everything above is dead index space, which is what the class
//! bytes from the spell table's routing band (`0x14` plain cast, `0x32`
//! summon, `0x63` capture) fall into: they are inside the bound and they
//! dispatch to the epilogue.
//!
//! Provenance: the table is read out of `extracted/SCUS_942.54` at
//! `0x800 + 0x80014FA0 - 0x80010000`; the bound and the indexed jump are in
//! `ghidra/scripts/funcs/800402f4.txt` at `0x80040444..0x8004046C`.

/// Selector values the applier's `sltiu` admits: `0x00..=0x83`.
pub const EFFECT_SELECTOR_BOUND: u16 = 0x84;

/// Where the 116 dead slots point - `FUN_800402F4`'s epilogue.
pub const EFFECT_SELECTOR_DEFAULT_ARM: u32 = 0x8004_21A8;

/// The applier's brightness-ramp selector, whose whole arm is one
/// `jal 0x80046870`.
pub const EFFECT_SELECTOR_GAUGE_RAMP: u8 = 0x82;

/// Every selector the jump table at `0x80014FA0` sends anywhere but the
/// epilogue, with the arm VA it sends it to.
///
/// Read straight out of the table, in index order. `0x0B`, `0x0C` and `0x0D`
/// share `0x80041FB4`; every other live selector has its own body.
pub const EFFECT_SELECTOR_ARMS: [(u8, u32); 16] = [
    (0x00, 0x8004_0470),
    (0x01, 0x8004_0908),
    (0x02, 0x8004_0D94),
    (0x03, 0x8004_0E64),
    (0x04, 0x8004_0F14),
    (0x05, 0x8004_10D4),
    (0x06, 0x8004_112C),
    (0x07, 0x8004_1464),
    (0x08, 0x8004_1BB0),
    (0x09, 0x8004_1C70),
    (0x0A, 0x8004_1E64),
    (0x0B, 0x8004_1FB4),
    (0x0C, 0x8004_1FB4),
    (0x0D, 0x8004_1FB4),
    (0x0E, 0x8004_209C),
    (EFFECT_SELECTOR_GAUGE_RAMP, 0x8004_21A0),
];

/// The arm `FUN_800402F4` reaches for `selector`, or `None` when the slot is
/// the shared epilogue (or the selector is past the `sltiu` bound, which
/// retail treats identically - `beq v0, zero` at `0x8004044C` jumps to the
/// same `0x800421A8`).
///
/// PORT: FUN_800402F4 (the selector jump table at `0x80014FA0` and its `sltiu 0x84` bound)
pub fn effect_selector_arm(selector: u8) -> Option<u32> {
    EFFECT_SELECTOR_ARMS
        .iter()
        .find(|(s, _)| *s == selector)
        .map(|(_, arm)| *arm)
}

/// Whether the applier does **anything** for `selector`.
///
/// The one live selector this answers `true` for that changes no actor field
/// is [`EFFECT_SELECTOR_GAUGE_RAMP`]; every other `true` is a real arm.
pub fn effect_selector_dispatches(selector: u8) -> bool {
    effect_selector_arm(selector).is_some()
}

// ---------------------------------------------------------------------------
// The three live arms above the documented `0x00..=0x07` band
// ---------------------------------------------------------------------------

/// Selector `8`'s status mask - it clears the bottom **two** bits only
/// (`andi v0, v0, 0xfffc` at `0x80041C04` in battle and `0x80041C34` out of
/// it).
pub const STATUS_CLEAR_MASK: u16 = 0xFFFC;

/// Selector `8` - status clear.
///
/// `hp` is the target's current HP through the applier's own pointer array
/// (`local_b0[slot]`, which is `actor[+0x14C]` in battle and character record
/// `+0x106` outside it): **a target at zero HP is skipped**
/// (`beq v0, zero, 0x80041C3C` at `0x80041BCC`), and only then does the
/// clear happen. The word cleared is `actor[+0x16E]` in battle and character
/// record `+0x12E` (`0x80084140 + slot*0x414 + 0x6F6`) out of it.
///
/// Returns the new status word, or `None` when the target is dead and retail
/// writes nothing. The cue-group placement that follows
/// ([`crate::battle_cue_group::cue_group_for`] class `8`) is battle-only and
/// is the caller's, exactly as it is for every other arm.
///
/// PORT: FUN_800402F4 (selector `8`, `0x80041BB0..0x80041C3C`)
pub fn selector_status_clear(hp: u16, status: u16) -> Option<u16> {
    if hp == 0 {
        return None;
    }
    Some(status & STATUS_CLEAR_MASK)
}

/// Lowest selector of the three that insert into a party member's displayed
/// skill list; the slot is `selector - 0x0B`
/// (`addiu v1, v1, -0xb` at `0x80041FC0`).
pub const SELECTOR_SKILL_INSERT_BASE: u8 = 0x0B;

/// How many ids the on-record list holds - the gap from `+0x186` to the
/// equipment field at `+0x196` (`docs/formats/save-record.md`).
pub const DISPLAYED_SKILL_CAP: usize = 16;

/// Selectors `0x0B` / `0x0C` / `0x0D` - insert one id into party slot
/// `selector - 0x0B`'s displayed skill list.
///
/// **This is an ordered insert, not a head insert.** The shift loop at
/// `0x80041FFC..0x8004202C` walks *down* from the current count and moves an
/// entry up only while `sltu a3, v1` holds - that is, while the id being
/// inserted is **less than** the entry it is looking at - then drops the new
/// id at the position it stopped on (`sb s6, 0x74e(a0)` at `0x80042064`) and
/// bumps the count. The list is therefore kept sorted **ascending by id**.
///
/// `docs/formats/save-record.md` and `docs/subsystems/level-up.md` both read
/// this as a head insert, from a capture where a list holding `0x0C` received
/// `0x03`. An ordered insert puts `0x03` at position `0` too, so that sample
/// cannot tell the two apart - the loop can.
///
/// Retail's write is unbounded; the port stops at [`DISPLAYED_SKILL_CAP`],
/// which is where the record's own field ends. Returns the position the id
/// landed at, or `None` when the list is already full.
///
/// The out-of-battle leg also calls `FUN_80035C00(slot, id)` - the "learned"
/// notification - and the in-battle leg does not (`beq v1, v0, epilogue` on
/// the mode word at `0x80042084`). That call is the caller's.
///
/// PORT: FUN_800402F4 (selectors `0x0B`..`0x0D`, `0x80041FB4..0x80042098`)
pub fn selector_insert_displayed_skill(list: &mut [u8], count: &mut u8, id: u8) -> Option<usize> {
    let cap = list.len().min(DISPLAYED_SKILL_CAP);
    let mut at = usize::from(*count).min(cap);
    if at >= cap {
        return None;
    }
    while at > 0 && id < list[at - 1] {
        list[at] = list[at - 1];
        at -= 1;
    }
    list[at] = id;
    *count = count.saturating_add(1);
    Some(at)
}

/// The cap selector `0x0E` discharges at - `sltiu v0, s1, 0x2710` at
/// `0x800420B4`, so at most `9999` is spent in one hit.
pub const POINT_CARD_CAP: u32 = 0x270F;

/// The victim fields selector `0x0E` reads before it stages a reaction.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PointCardVictim {
    /// `+0x14C` - current HP.
    pub hp: u16,
    /// `+0x1EF` - the flinch reaction.
    pub flinch_anim: u8,
    /// `+0x1F1` - the knockdown reaction.
    pub knockdown_anim: u8,
    /// `+0x1F2` - non-zero forces the knockdown leg.
    pub reaction_gate: u8,
    /// `+0x1DC` - the restage byte the arm ORs into.
    pub restage: u8,
}

/// What one selector-`0x0E` discharge resolved to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PointCardDischarge {
    /// How much came off the counter at `0x800845B4` - the clamped amount,
    /// which is also what `FUN_801F44A0` pops as the damage number.
    pub spent: u32,
    /// The counter after the subtract.
    pub remaining: u32,
    /// The victim's HP after the shape-A clamp.
    pub hp: u16,
    /// The clip staged into `+0x1DA`.
    pub staged_anim: u8,
    /// `+0x1DC` after the arm's two ORs.
    pub restage: u8,
}

/// Selector `0x0E` - the Point Card discharge.
///
/// The counter is a **word** at `0x800845B4` (`0x80084140 + 0x474`), not a
/// per-actor field: the arm spends `min(counter, 0x270F)` of it, writes the
/// remainder back, pops the amount as a damage number through
/// `FUN_801F44A0(amount, slot)`, then stages the victim's reaction and
/// applies the amount to `+0x14C` with the band's usual kill-capable clamp
/// (`sltu a0, s1` at `0x80042184`, so the HP floor is `0`).
///
/// The reaction pick is the same three-leg shape the cast band uses: a
/// non-zero `+0x1F2` forces `+0x1F1`; otherwise an amount **below** the
/// victim's HP takes `+0x1EF` and sets `+0x1DC` bit `4`; an amount at or
/// above it takes `+0x1F1`. Bit `1` is then set on every path.
///
/// Returns `None` when the counter is empty, which is retail's first test
/// (`beq v1, zero, epilogue` at `0x800420AC`).
///
/// PORT: FUN_800402F4 (selector `0x0E`, `0x8004209C..0x8004219C`)
pub fn selector_point_card(counter: u32, victim: PointCardVictim) -> Option<PointCardDischarge> {
    if counter == 0 {
        return None;
    }
    let spent = counter.min(POINT_CARD_CAP);
    let remaining = counter - spent;
    let mut restage = victim.restage;
    let staged_anim = if victim.reaction_gate != 0 {
        victim.knockdown_anim
    } else if spent < u32::from(victim.hp) {
        restage |= 4;
        victim.flinch_anim
    } else {
        victim.knockdown_anim
    };
    restage |= 1;
    let applied = spent.min(u32::from(victim.hp)) as u16;
    Some(PointCardDischarge {
        spent,
        remaining,
        hp: victim.hp - applied,
        staged_anim,
        restage,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_sixteen_of_the_hundred_and_thirty_two_slots_are_live() {
        assert_eq!(EFFECT_SELECTOR_ARMS.len(), 16);
        let live = (0..EFFECT_SELECTOR_BOUND)
            .filter(|s| effect_selector_dispatches(*s as u8))
            .count();
        assert_eq!(live, 16);
        // 116 dead slots inside the bound.
        assert_eq!(EFFECT_SELECTOR_BOUND as usize - live, 116);
    }

    #[test]
    fn the_live_span_is_zero_through_fourteen_plus_the_ramp() {
        for s in 0..=0x0Eu8 {
            assert!(effect_selector_dispatches(s), "selector {s:#04x}");
        }
        for s in 0x0Fu8..=0x81 {
            assert!(!effect_selector_dispatches(s), "selector {s:#04x}");
        }
        assert!(effect_selector_dispatches(EFFECT_SELECTOR_GAUGE_RAMP));
        assert!(!effect_selector_dispatches(0x83));
    }

    /// The three selectors that share one body, and the one that does not
    /// really have one.
    #[test]
    fn three_selectors_share_a_body_and_the_ramp_is_two_instructions() {
        let b = effect_selector_arm(0x0B).unwrap();
        assert_eq!(effect_selector_arm(0x0C), Some(b));
        assert_eq!(effect_selector_arm(0x0D), Some(b));
        assert_ne!(effect_selector_arm(0x0E), Some(b));
        assert_eq!(
            effect_selector_arm(EFFECT_SELECTOR_GAUGE_RAMP),
            Some(EFFECT_SELECTOR_DEFAULT_ARM - 8)
        );
    }

    /// The spell table's routing bytes are inside the bound and dispatch to
    /// nothing - which is why a cast never reaches the applier's arms.
    #[test]
    fn the_spell_routing_bytes_are_dead_selectors() {
        for routing in [0x14u8, 0x32, 0x63] {
            assert!(u16::from(routing) < EFFECT_SELECTOR_BOUND);
            assert!(!effect_selector_dispatches(routing));
        }
    }

    /// Selector 8 skips a dead target entirely and clears only two bits.
    #[test]
    fn status_clear_skips_a_dead_target_and_masks_two_bits() {
        assert_eq!(selector_status_clear(0, 0xFFFF), None);
        assert_eq!(selector_status_clear(1, 0xFFFF), Some(0xFFFC));
        assert_eq!(selector_status_clear(1, 0x0003), Some(0));
        // Every other bit survives.
        assert_eq!(selector_status_clear(10, 0x0F80 | 3), Some(0x0F80));
    }

    /// The skill insert is ordered, which a head insert is not.
    #[test]
    fn displayed_skill_insert_keeps_the_list_ascending() {
        let mut list = [0u8; DISPLAYED_SKILL_CAP];
        let mut count = 0u8;
        for id in [0x0C, 0x03, 0x07, 0x01, 0x0F] {
            selector_insert_displayed_skill(&mut list, &mut count, id);
        }
        assert_eq!(count, 5);
        assert_eq!(&list[..5], &[0x01, 0x03, 0x07, 0x0C, 0x0F]);
    }

    /// The one capture the head-insert reading came from is consistent with
    /// both, which is why it could not settle the question.
    #[test]
    fn the_capture_sample_cannot_tell_ordered_from_head_insert() {
        let mut list = [0u8; DISPLAYED_SKILL_CAP];
        list[0] = 0x0C;
        let mut count = 1u8;
        assert_eq!(
            selector_insert_displayed_skill(&mut list, &mut count, 0x03),
            Some(0)
        );
        assert_eq!(&list[..2], &[0x03, 0x0C]);
        // ...but an id above the head lands at the tail, where a head insert
        // would still put it at position 0.
        assert_eq!(
            selector_insert_displayed_skill(&mut list, &mut count, 0x40),
            Some(2)
        );
        assert_eq!(&list[..3], &[0x03, 0x0C, 0x40]);
    }

    #[test]
    fn displayed_skill_insert_stops_at_the_records_own_field() {
        let mut list = [0u8; DISPLAYED_SKILL_CAP];
        let mut count = DISPLAYED_SKILL_CAP as u8;
        assert_eq!(
            selector_insert_displayed_skill(&mut list, &mut count, 1),
            None
        );
        assert_eq!(count, DISPLAYED_SKILL_CAP as u8);
    }

    #[test]
    fn point_card_spends_clamped_and_stages_by_the_three_legs() {
        let victim = PointCardVictim {
            hp: 500,
            flinch_anim: 0x02,
            knockdown_anim: 0x0B,
            reaction_gate: 0,
            restage: 0,
        };
        assert_eq!(selector_point_card(0, victim), None);

        // Under HP: flinch, bit 4 and bit 1, HP drops by the amount.
        let d = selector_point_card(100, victim).unwrap();
        assert_eq!((d.spent, d.remaining, d.hp), (100, 0, 400));
        assert_eq!(d.staged_anim, 0x02);
        assert_eq!(d.restage, 5);

        // At or above HP: knockdown, no bit 4, HP floors at zero.
        let d = selector_point_card(500, victim).unwrap();
        assert_eq!((d.spent, d.hp, d.staged_anim, d.restage), (500, 0, 0x0B, 1));

        // The counter is clamped, and the remainder stays on the counter.
        let d = selector_point_card(0x3000, victim).unwrap();
        assert_eq!(d.spent, POINT_CARD_CAP);
        assert_eq!(d.remaining, 0x3000 - POINT_CARD_CAP);

        // A set `+0x1F2` forces the knockdown leg even below HP.
        let gated = PointCardVictim {
            reaction_gate: 1,
            ..victim
        };
        let d = selector_point_card(1, gated).unwrap();
        assert_eq!((d.staged_anim, d.restage), (0x0B, 1));
    }

    /// Selector `0x0F` past the bound behaves the same as one inside it.
    #[test]
    fn past_the_bound_is_the_same_answer() {
        assert_eq!(effect_selector_arm(0xFF), None);
        assert_eq!(effect_selector_arm(0x84), None);
    }
}
