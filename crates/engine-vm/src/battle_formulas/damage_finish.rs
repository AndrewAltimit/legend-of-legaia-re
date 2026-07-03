//! Post-roll damage finisher (`FUN_801ddb30`) + spirit-gauge fill.
//! Split out of `battle_formulas.rs`.

// ---------------------------------------------------------------------------
// FUN_801ddb30 - damage finisher (post-roll finalisation)
// ---------------------------------------------------------------------------
//
// The shared finisher `FUN_801ddb30` (`overlay_battle_action_801ddb30.txt`) takes
// the pre-finisher damage produced by the roll + scale stages above and turns it
// into the final HP loss + the defender's spirit-gauge fill. It works on
// `over = *param_3 - *param_4` (the damage *above* the base `*param_4`); every
// stage rewrites `over` in place. The closed-form arithmetic splits cleanly into
// two pure kernels:
//
//   * [`damage_finish`] - equipment elemental-resistance halving (one element
//     bit per attacker element; the absorb-bit `0x10` gate routes to a 3/4 scale
//     instead), the defender-guard halve (`actor+0x1de == 4`), the no-damage
//     `rand()%9 + 8` floor, the summon power-percent scale (`attacker_slot == 7`),
//     and the `9999` cap. Returns the final `over` (HP loss).
//   * [`spirit_gauge_fill`] - the defender's spirit-gauge accrual from the same
//     `over`, plus the two "spirit gain up" equipment bits, clamped to 100.
//
// The finisher's remaining tail is genuinely coupled to live battle state and is
// **not** reproduced here: the damage-popup accumulator (`_DAT_8007bd14`), the
// `DAT_801f6980` AI revenge / counter-aggro table, the MP-drain + the
// per-element stat-debuff `switch` keyed on the attacker's element
// (`DAT_801c9358+0x1d`), and the `+0x16e` "nullify" status that zeroes the hit
// after the spirit accrual. The action SM applies those; see the REF in the
// module header.

/// The defender's equipment-derived elemental-resistance + spirit flags, read by
/// [`damage_finish`] / [`spirit_gauge_fill`] from the live character record's two
/// words at `+0xF4` (`lo`) and `+0xF8` (`hi`) (runtime `0x800847FC`/`0x80084800`
/// for member 0, `0x414` stride). Only a party defender (slot `< 3`) carries
/// these; enemy defenders pass [`Default`] (no resistance).
///
/// The two words are the first half of the accessory-passive **ability
/// bitfield** (record `+0xF4..+0x103`, aggregator `FUN_80042558`), and every
/// flag here is simply passive index `0x1D + element` read through the word
/// boundary: the elemental-guard passives are contiguous at `0x1D..=0x23`
/// (Earth, Water, Fire, Wind, Thunder, Light, Dark - the element-id order),
/// All Guard is `0x24`, and the "spirit gain up" pair is AP Boost 1/2 at
/// `0x28`/`0x29`. So the bit layout (mirroring the disassembly's per-element
/// `if` ladder in `FUN_801ddb30`):
///
/// | element | guard passive | bit | word |
/// |---|---|---|---|
/// | 0 Earth | `0x1D` | `0x20000000` | `lo` (`+0xF4`) |
/// | 1 Water | `0x1E` | `0x40000000` | `lo` |
/// | 2 Fire | `0x1F` | `0x80000000` | `lo` |
/// | 3 Wind | `0x20` | `0x1` | `hi` (`+0xF8`) |
/// | 4 Thunder | `0x21` | `0x2` | `hi` |
/// | 5 Light | `0x22` | `0x4` | `hi` |
/// | 6 Dark | `0x23` | `0x8` | `hi` |
///
/// `hi & 0x10` is the All-Guard gate (passive `0x24`, Rainbow Jewel);
/// `hi & 0x100` / `hi & 0x200` are AP Boost 1/2 (`0x28`/`0x29`), the two
/// spirit-gain-up flags (see [`spirit_gauge_fill`]).
#[derive(Debug, Clone, Copy, Default)]
pub struct DefenderResist {
    /// Record word `+0xF4` (ability-bitfield word 0, passive indices
    /// `0x00..=0x1F`): elements 0..=2 in the top three bits.
    pub lo: u32,
    /// Record word `+0xF8` (ability-bitfield word 1, passive indices
    /// `0x20..=0x3F`): elements 3..=6 in the low nibble, the All-Guard gate
    /// (`0x10`) and the spirit-gain-up bits (`0x100`/`0x200`).
    pub hi: u32,
}

impl DefenderResist {
    /// Build from the first two words of a character's accessory-passive
    /// ability bitfield (record `+0xF4` / `+0xF8`), the exact words retail's
    /// finisher indexes.
    pub fn from_ability_words(word0: u32, word1: u32) -> Self {
        Self {
            lo: word0,
            hi: word1,
        }
    }

    /// `true` if the defender resists `element` (0..=6) - the per-element bit set.
    pub(super) fn resists(&self, element: u8) -> bool {
        match element {
            0 => self.lo & 0x2000_0000 != 0,
            1 => self.lo & 0x4000_0000 != 0,
            2 => self.lo & 0x8000_0000 != 0,
            3 => self.hi & 0x1 != 0,
            4 => self.hi & 0x2 != 0,
            5 => self.hi & 0x4 != 0,
            6 => self.hi & 0x8 != 0,
            _ => false,
        }
    }
}

/// All inputs to [`damage_finish`] (the closed-form stages of `FUN_801ddb30`).
#[derive(Debug, Clone, Copy)]
pub struct DamageFinish {
    /// Pre-finisher damage above base (`attacker_roll - defender_roll`, the
    /// `over` the roll/scale stages produce - already saturated to `>= 0`).
    pub predamage: u32,
    /// Attacker actor slot (`param_1`); `7` is the summon body, `>= 3` an enemy.
    pub attacker_slot: u8,
    /// Defender actor slot (`param_2`); `< 3` a party member.
    pub defender_slot: u8,
    /// Attacker element (0..=6); `7` = non-elemental, which bypasses the absorb
    /// gate so the per-element halve ladder still runs.
    pub attacker_element: u8,
    /// The party defender's equipment resistance flags. Ignored for an enemy
    /// defender (`defender_slot >= 3`).
    pub defender_resist: DefenderResist,
    /// Defender is in the guard/defend state (`actor+0x1de == 4`) - halves `over`.
    pub defender_guarding: bool,
    /// The `_DAT_8007bd84` global, consulted only for an enemy defender
    /// (`defender_slot >= 3`): when set, the enemy takes half damage.
    pub enemy_defender_halve: bool,
    /// The `param_5` flag: when non-zero the party-defender resistance block is
    /// skipped entirely (the retail caller passes it for certain fixed hits).
    pub bypass_party_resist: bool,
    /// Summon power-percent (`attacker_slot == 7` only): `over` is scaled
    /// `over * pct / 100`. From the per-caster element table at `0x801F5468`.
    pub summon_power_pct: u8,
    /// One `rand()` draw (`0..=0x7FFF`), consumed **only** when `over` has been
    /// reduced to `0` by mitigation (the `rand()%9 + 8` floor). Pass any value
    /// when the caller knows mitigation can't zero the hit.
    pub floor_rand: u16,
}

/// Apply the closed-form finalisation stages of `FUN_801ddb30` to the
/// pre-finisher damage and return the final HP loss (`over`).
///
/// Order mirrors the disassembly exactly:
///
/// 1. **Party-defender elemental resistance** (`defender_slot < 3`, attacker is
///    an enemy `>= 3`, and `!bypass_party_resist`): if the defender's absorb bit
///    (`hi & 0x10`) is clear *or* the attacker is non-elemental (element 7), the
///    per-element halve ladder runs - `over >>= 1` when the defender resists the
///    attacker's element. Otherwise `over = over * 3 >> 2` (3/4).
/// 2. **Enemy-defender halve** (`defender_slot >= 3`): `over >>= 1` when
///    `enemy_defender_halve`.
/// 3. **Guard halve**: `over >>= 1` when the defender is guarding.
/// 4. **No-damage floor**: when `over == 0`, `over = rand()%9 + 8`.
/// 5. **Summon power scale** (`attacker_slot == 7`): `over = over * pct / 100`.
/// 6. **9999 cap**.
///
/// The multi-hit pointer bump (`if *param_3 == *param_4 param_3++`) and the
/// `+0x16e` nullify status are not part of this value - they are caller concerns
/// (see the module section comment).
pub fn damage_finish(i: &DamageFinish) -> u32 {
    damage_finish_lazy(i, || i.floor_rand)
}

/// As [`damage_finish`], but the stage-4 floor `rand()` is produced lazily by
/// `floor_rand`, invoked **only when mitigation has reduced `over` to zero** -
/// the single point retail's `FUN_801ddb30` draws RNG. A caller pulling from a
/// shared RNG cursor advances it by zero or one draw, exactly as retail does;
/// [`DamageFinish::floor_rand`] is ignored on this path.
pub fn damage_finish_lazy(i: &DamageFinish, floor_rand: impl FnOnce() -> u16) -> u32 {
    let mut over = i.predamage;

    // Stage 1: party-defender elemental resistance.
    if (i.defender_slot as u32) < 3 {
        if i.attacker_slot >= 3 && !i.bypass_party_resist {
            let absorb_gate = i.defender_resist.hi & 0x10 != 0;
            if !absorb_gate || i.attacker_element == 7 {
                if i.attacker_element <= 6 && i.defender_resist.resists(i.attacker_element) {
                    over >>= 1;
                }
            } else {
                // Absorb bit set + elemental attacker: 3/4 scale.
                over = (over * 3) >> 2;
            }
        }
    } else if i.enemy_defender_halve {
        // Stage 2: enemy-defender global halve.
        over >>= 1;
    }

    // Stage 3: defender guard halve.
    if i.defender_guarding {
        over >>= 1;
    }

    // Stage 4: no-damage floor (the only RNG draw the finisher consumes).
    if over == 0 {
        over = (floor_rand() as u32) % 9 + 8;
    }

    // Stage 5: summon power-percent scale.
    if i.attacker_slot == 7 {
        over = over.saturating_mul(i.summon_power_pct as u32) / 100;
    }

    // Stage 6: 9999 cap.
    if over > 9999 {
        over = 9999;
    }
    over
}

/// The defender's spirit-gauge fill from a finished hit, `FUN_801ddb30`'s spirit
/// stage. Mirrors the disassembly:
///
/// ```text
/// pct = max(1, over * 100 / defender_maxhp)
/// if defender_is_party:
///     if (resist.hi & 0x200): spirit += pct >> 2     // "spirit gain up" ×1
///     if (resist.hi & 0x100): spirit += pct / 10     // "spirit gain up" ×2
/// spirit = min(100, spirit + pct)
/// ```
///
/// `over` is the **pre-nullify** damage (spirit still accrues when a `+0x16e`
/// nullify status later zeroes the HP loss). `defender_maxhp` is `actor+0x14e`;
/// retail `trap`s on a zero max-HP - the kernel instead returns the gauge
/// unchanged (the caller guarantees a living defender). Returns the new gauge
/// value (already clamped to `100`).
///
/// The live battle loop drives this on the defender of every damaging hit
/// (physical and magic) into [`BattleActor::spirit_gauge`] (`actor+0x170`); see
/// `World::accrue_spirit_gauge`. The engine passes [`DefenderResist::default`]
/// (the per-character resist/spirit-gain-up words aren't modelled yet), so only
/// the unconditional base `pct` term contributes today.
pub fn spirit_gauge_fill(
    over: u32,
    defender_maxhp: u16,
    current_spirit: u16,
    resist: DefenderResist,
    defender_is_party: bool,
) -> u16 {
    if defender_maxhp == 0 {
        return current_spirit.min(100);
    }
    let pct = (over * 100) / defender_maxhp as u32;
    let pct = if pct == 0 { 1 } else { pct };
    let mut spirit = current_spirit as u32;
    if defender_is_party {
        if resist.hi & 0x200 != 0 {
            spirit += pct >> 2;
        }
        if resist.hi & 0x100 != 0 {
            spirit += pct / 10;
        }
    }
    spirit += pct;
    spirit.min(100) as u16
}
