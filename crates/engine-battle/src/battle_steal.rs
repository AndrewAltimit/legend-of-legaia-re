//! The **death spoils** a slain monster hands over: the Evil God Icon's
//! steal attack and the return of whatever a thief had stolen.
//!
//! Both live in one arm of the battle anim commit `FUN_8004AD80`
//! (`SCUS_942.54`, `0x8004B29C..0x8004B65C`), which runs when a monster seat
//! (`>= 3`) commits the end of its knockdown clip (the installed entry's tag
//! byte is `4`, `0x8004B0A4`) with live HP `+0x14C == 0` - the death fall, not
//! the hit. The arm forks on the monster's cell of the **stolen band** at
//! `0x801C8FE0`:
//!
//! * **a thief** (`band[seat - 3] != 0`, the id PROT 0941's Steal stashed
//!   there) gives it back: `FUN_800421D4(item, 1)` adds it to the bag, the
//!   caption reads *took* when `band[seat + 5]` is non-zero (the loot came
//!   off another monster) and *recovered* otherwise, and the cell is zeroed.
//!   When the last message raised is already `0x5B` (`ctx[+0x18]`), the whole
//!   line is swapped for the *every stolen item is back* caption instead.
//! * **anyone else** runs the steal attack, once per strike chain: with the
//!   acting seat `ctx[+0x13]` in the party and the latch `ctx[+0x27]` clear,
//!   the latch is set **before any other test**, so the first monster an
//!   action fells spends that action's one attempt whether or not the killer
//!   could steal. Then, outside a special battle (`_DAT_8007BAC0 == 0`), with
//!   the killer's action category `+0x1DE == 3` (Attack) and its record's
//!   ability word `+0xF4` bit `0x10000` (passive `0x10`, Steal Attack - the
//!   Evil God Icon), it rolls `rand() % 100 < chance * mult` against the
//!   static steal table (`0x80077828 + id * 2`, `[chance, item]`); `mult` is
//!   `2` when any living member's `+0xF8` carries bit `0x20000` (passive
//!   `0x31`, Items Up - the Bronze Book) and `1` otherwise. A hit whose item
//!   the bag already holds `99` of (`FUN_80042F4C`) is dropped silently;
//!   anything else raises the caption and adds the item.
//!
//! The `ctx[+0x27]` latch is re-armed by the battle-action state machine
//! `FUN_801E295C` (PROT 0898): the arm that moves an actor's strike chain
//! (`0x1E`) to recovery (`0x1F`) at `0x801E3A7C` clears it in its jump's
//! delay slot, `sb zero, 0x16(s5)` at `0x801E3A84` with `s5 = ctx + 0x11`.
//! That runs at the end of every chain, monsters' included, so each attacking
//! action gets one roll (capture: the latch goes `1 -> 0` at the chain's end
//! after a kill set it). A byte scan for `0x27(reg)` cannot see the store,
//! whose displacement rides a base already offset by `0x11`; the port clears
//! [`StealBand::attempted`] on the same edge in `World::step_battle`.
//!
//! The caption is composed the retail way - template copy
//! (`FUN_8003CA78`), the `{0xC2, item}` item-name token appended by
//! `FUN_8003CB54` ([`legaia_engine_vm::battle_helpers::mes_append_escape`]),
//! the two-byte tail appended by `FUN_8003CAC4` - from the templates pinned in
//! [`legaia_asset::battle_ui_strings`] and read off the user's own
//! executable, and shown in HUD element `0x5B`: placement record 91
//! (`0x80077498`, payload pointer `+0x14` = `0x800774AC`), the full-width
//! bar that slides from `(16, 236)` up to the active-actor bar's seat
//! `(16, 194)`.

use legaia_asset::battle_ui_strings::{BattleUiLabel, BattleUiStrings};

/// HUD element id the caption is raised on (`li a0,0x5b` before the
/// `jal 0x801D8DE8` at `0x8004B37C` / `0x8004B628`), and the message id the
/// arm writes into `ctx[+0x18]`.
pub const STEAL_CAPTION_ELEMENT: u8 = 0x5B;

/// Monster seats the stolen band covers. Retail's band is two runs of
/// four-byte cells, `[seat - 3]` (the item) and `[seat + 5]` (the
/// took-from-a-monster counter), so its seat space is eight wide.
pub const BAND_SEATS: usize = 8;

/// Action category `+0x1DE` the steal attack requires - Attack.
pub const ATTACK_CATEGORY: u8 = 3;

/// Ability word 0 (`+0xF4`) bit of passive `0x10`, Steal Attack.
pub const STEAL_ATTACK_BIT: u32 = 0x0001_0000;
/// Ability word 1 (`+0xF8`) bit of passive `0x31`, Items Up.
pub const ITEMS_UP_BIT: u32 = 0x0002_0000;

/// The held count at which a stolen item is dropped instead of granted
/// (`li v1,0x63` against `FUN_80042F4C` at `0x8004B5CC`).
pub const HELD_CAP: u8 = 99;

/// The battle-scoped steal state: retail's stolen band plus the one-attempt
/// latch `ctx[+0x27]`. Zeroed at battle load; the latch is also cleared at
/// the end of every strike chain.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StealBand {
    /// `0x801C8FE0 + m*4` - the item a thief in monster seat `m` holds.
    pub stolen: [u8; BAND_SEATS],
    /// `0x801C8FE0 + (m + 8)*4` - non-zero when that loot came off a monster.
    pub took: [u32; BAND_SEATS],
    /// `ctx[+0x27]` - the current strike chain's one steal attempt has been
    /// spent.
    pub attempted: bool,
    /// Port bookkeeping: the death commit has run for monster seat `m`.
    /// Retail's arm runs once because the commit it lives in runs once per
    /// installed clip; the port's end-of-knockdown test is level-triggered.
    pub resolved: [bool; BAND_SEATS],
}

impl StealBand {
    /// Re-arm the latch on the strike chain's exit - `0x1E -> 0x1F`, the arm
    /// of `FUN_801E295C` whose delay slot clears `ctx[+0x27]` (`0x801E3A84`).
    ///
    /// REF: FUN_801E295C
    pub fn on_action_transition(&mut self, from: u8, to: u8) {
        use legaia_engine_vm::battle_action::ActionState;
        if from == ActionState::AttackChain.as_byte() && to == ActionState::AttackRecovery.as_byte()
        {
            self.attempted = false;
        }
    }
}

/// Everything the steal-attack leg reads besides the band and the rand.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StealAttackInputs {
    /// The acting seat `ctx[+0x13]` is a party seat.
    pub acting_party: bool,
    /// `_DAT_8007BAC0 != 0` - a special (arena) battle.
    pub special_battle: bool,
    /// The acting seat's `+0x1DE == 3`.
    pub killer_attack: bool,
    /// The acting member's `+0xF4 & 0x10000`.
    pub killer_steals: bool,
    /// Any living member's `+0xF8 & 0x20000`.
    pub items_up: bool,
    /// The slain monster's steal-table row, `None` with no table installed.
    pub entry: Option<legaia_asset::steal_table::StealEntry>,
    /// `ctx[+0x18] == 0x5B` - the last message raised is this caption.
    pub caption_up: bool,
}

/// Which caption a death raised, and the item it names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeathSpoils {
    /// The steal attack landed.
    Stole(u8),
    /// A thief's loot, taken off another monster, is handed over.
    Took(u8),
    /// A thief's loot, taken off the party, is handed back.
    Recovered(u8),
    /// A thief's loot is handed back while the caption was already up.
    RecoveredAll(u8),
}

impl DeathSpoils {
    /// The item the bag gains.
    pub fn item(self) -> u8 {
        match self {
            Self::Stole(i) | Self::Took(i) | Self::Recovered(i) | Self::RecoveredAll(i) => i,
        }
    }
}

/// Resolve monster seat `m`'s death commit against the band.
///
/// `roll` is drawn only on the frame retail calls `rand()` - past every gate
/// of the steal-attack leg - so the shared cursor advances exactly as often.
/// `held` answers `FUN_80042F4C`. Returns the caption to raise, which also
/// names the item the caller adds to the bag.
///
// PORT: FUN_8004AD80 (`0x8004B29C..0x8004B65C`: the death-spoils arm - thief
// return and the once-per-chain steal attack)
pub fn resolve_death_spoils(
    band: &mut StealBand,
    m: usize,
    inputs: &StealAttackInputs,
    roll: impl FnOnce() -> u32,
    held: impl Fn(u8) -> u8,
) -> Option<DeathSpoils> {
    if m >= BAND_SEATS {
        return None;
    }
    let loot = band.stolen[m];
    if loot != 0 {
        band.stolen[m] = 0;
        return Some(if inputs.caption_up {
            DeathSpoils::RecoveredAll(loot)
        } else if band.took[m] != 0 {
            DeathSpoils::Took(loot)
        } else {
            DeathSpoils::Recovered(loot)
        });
    }
    if !inputs.acting_party || band.attempted {
        return None;
    }
    band.attempted = true;
    if inputs.special_battle || !inputs.killer_attack || !inputs.killer_steals {
        return None;
    }
    let entry = inputs.entry?;
    let mult = if inputs.items_up { 2 } else { 1 };
    let r = roll() % 100;
    if r >= u32::from(entry.chance_pct) * mult || entry.item_id == 0 {
        return None;
    }
    (held(entry.item_id) != HELD_CAP).then_some(DeathSpoils::Stole(entry.item_id))
}

/// Compose a caption's MES bytes the way retail does: copy the head
/// (`FUN_8003CA78`), append the `{0xC2, item}` item-name token
/// (`FUN_8003CB54`), append the tail (`FUN_8003CAC4`). The
/// [`DeathSpoils::RecoveredAll`] line is the head alone.
///
/// `None` when the head is not on the image the strings were read from.
pub fn compose_caption(strings: &BattleUiStrings, spoils: DeathSpoils) -> Option<Vec<u8>> {
    let (head, tail) = match spoils {
        DeathSpoils::Stole(_) => (BattleUiLabel::StealStole, BattleUiLabel::StealStoleTail),
        DeathSpoils::Took(_) => (BattleUiLabel::StealTook, BattleUiLabel::StealTookTail),
        DeathSpoils::Recovered(_) => (
            BattleUiLabel::StealRecovered,
            BattleUiLabel::StealRecoveredTail,
        ),
        DeathSpoils::RecoveredAll(_) => {
            return strings
                .get(BattleUiLabel::StealRecoveredAll)
                .map(latin1_bytes);
        }
    };
    let mut buf = latin1_bytes(strings.get(head)?);
    // Room for the token + its terminator, the fixed-buffer contract
    // `mes_append_escape` documents.
    buf.extend_from_slice(&[0, 0, 0]);
    let end = legaia_engine_vm::battle_helpers::mes_append_escape(&mut buf, 0xC2, spoils.item());
    buf.truncate(end + 2);
    buf.extend(strings.get(tail).map(latin1_bytes).unwrap_or_default());
    Some(buf)
}

/// Render a composed caption for the font's ASCII layout: glyph bytes pass
/// through, a `0xC2` / `0xC4` item token becomes `item_name(id)`, every other
/// escape pair is dropped.
pub fn caption_text(bytes: &[u8], item_name: impl Fn(u8) -> Option<String>) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b < 0x1F {
            break;
        }
        if b & 0xF0 == 0xC0 {
            let arg = bytes.get(i + 1).copied().unwrap_or(0);
            if matches!(b, 0xC2 | 0xC4)
                && let Some(name) = item_name(arg)
            {
                out.push_str(&name);
            }
            i += 2;
            continue;
        }
        out.push(char::from(b));
        i += 1;
    }
    out
}

fn latin1_bytes(s: &str) -> Vec<u8> {
    s.chars().map(|c| c as u32 as u8).collect()
}

/// The caption on screen: its text and the acting seat it was raised under.
/// The port closes it when a different seat takes the action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StealCaption {
    /// Display text, item name substituted.
    pub text: String,
    /// `battle_ctx.active_actor` when it was raised.
    pub owner: u8,
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_asset::steal_table::StealEntry;

    fn inputs(chance: u8, item: u8) -> StealAttackInputs {
        StealAttackInputs {
            acting_party: true,
            killer_attack: true,
            killer_steals: true,
            entry: Some(StealEntry {
                chance_pct: chance,
                item_id: item,
            }),
            ..Default::default()
        }
    }

    #[test]
    fn the_first_party_kill_spends_the_attempt_even_without_the_icon() {
        let mut band = StealBand::default();
        let mut i = inputs(100, 0x8A);
        i.killer_steals = false;
        assert_eq!(resolve_death_spoils(&mut band, 0, &i, || 0, |_| 0), None);
        assert!(band.attempted);
        // The icon wearer's later kill gets nothing: the latch is spent.
        i.killer_steals = true;
        assert_eq!(resolve_death_spoils(&mut band, 1, &i, || 0, |_| 0), None);
    }

    #[test]
    fn a_monster_kill_does_not_spend_the_attempt() {
        let mut band = StealBand::default();
        let mut i = inputs(100, 0x8A);
        i.acting_party = false;
        assert_eq!(resolve_death_spoils(&mut band, 0, &i, || 0, |_| 0), None);
        assert!(!band.attempted);
    }

    #[test]
    fn the_roll_is_chance_times_the_items_up_multiplier() {
        let mut band = StealBand::default();
        let i = inputs(30, 0x8A);
        assert_eq!(resolve_death_spoils(&mut band, 0, &i, || 45, |_| 0), None);
        let mut band = StealBand::default();
        let mut i = inputs(30, 0x8A);
        i.items_up = true;
        assert_eq!(
            resolve_death_spoils(&mut band, 0, &i, || 45, |_| 0),
            Some(DeathSpoils::Stole(0x8A))
        );
    }

    #[test]
    fn a_full_stack_drops_the_steal_and_a_special_battle_never_rolls() {
        let mut band = StealBand::default();
        assert_eq!(
            resolve_death_spoils(&mut band, 0, &inputs(100, 0x8A), || 0, |_| 99),
            None
        );
        let mut band = StealBand::default();
        let mut i = inputs(100, 0x8A);
        i.special_battle = true;
        let mut rolled = false;
        let got = resolve_death_spoils(
            &mut band,
            0,
            &i,
            || {
                rolled = true;
                0
            },
            |_| 0,
        );
        assert_eq!(got, None);
        assert!(!rolled, "no rand() behind a failed gate");
        assert!(band.attempted);
    }

    #[test]
    fn a_thief_returns_its_loot_and_skips_the_steal_attack() {
        let mut band = StealBand::default();
        band.stolen[1] = 0x10;
        let got = resolve_death_spoils(&mut band, 1, &inputs(100, 0x8A), || 0, |_| 0);
        assert_eq!(got, Some(DeathSpoils::Recovered(0x10)));
        assert_eq!(band.stolen[1], 0);
        assert!(!band.attempted, "the return arm never touches the latch");
        band.stolen[2] = 0x11;
        band.took[2] = 1;
        let got = resolve_death_spoils(&mut band, 2, &inputs(0, 0), || 0, |_| 0);
        assert_eq!(got, Some(DeathSpoils::Took(0x11)));
        band.stolen[3] = 0x12;
        let mut i = inputs(0, 0);
        i.caption_up = true;
        let got = resolve_death_spoils(&mut band, 3, &i, || 0, |_| 0);
        assert_eq!(got, Some(DeathSpoils::RecoveredAll(0x12)));
    }

    #[test]
    fn the_caption_is_head_token_tail() {
        let mut scus = vec![0u8; 0x800 + 0x8000];
        scus[..8].copy_from_slice(b"PS-X EXE");
        let t_addr: u32 = 0x8007_7000;
        scus[0x18..0x1C].copy_from_slice(&t_addr.to_le_bytes());
        scus[0x1C..0x20].copy_from_slice(&0x8000u32.to_le_bytes());
        let put = |b: &mut Vec<u8>, va: u32, s: &[u8]| {
            let o = (va - t_addr) as usize + 0x800;
            b[o..o + s.len()].copy_from_slice(s);
            b[o + s.len()] = 0;
        };
        put(
            &mut scus,
            legaia_asset::battle_ui_strings::SCUS_STEAL_STOLE,
            b"HEAD <",
        );
        put(
            &mut scus,
            legaia_asset::battle_ui_strings::SCUS_STEAL_STOLE_TAIL,
            b">.",
        );
        let strings = BattleUiStrings::from_scus(&scus);
        let bytes = compose_caption(&strings, DeathSpoils::Stole(0x8A)).expect("composed");
        assert_eq!(bytes, b"HEAD <\xC2\x8A>.".to_vec());
        let text = caption_text(&bytes, |id| (id == 0x8A).then(|| "Thing".to_string()));
        assert_eq!(text, "HEAD <Thing>.");
    }
    #[test]
    fn the_strike_chain_exit_rearms_the_latch() {
        use legaia_engine_vm::battle_action::ActionState;
        let mut band = StealBand {
            attempted: true,
            ..StealBand::default()
        };
        band.on_action_transition(
            ActionState::AttackStrike.as_byte(),
            ActionState::AttackChain.as_byte(),
        );
        assert!(band.attempted, "only the chain's exit clears it");
        band.on_action_transition(
            ActionState::AttackChain.as_byte(),
            ActionState::AttackRecovery.as_byte(),
        );
        assert!(!band.attempted);
    }
}
