//! Run / escape roll (`FUN_801E791C`). Split out of `battle_formulas.rs`.

// ---------------------------------------------------------------------------
// Run / escape roll (FUN_801E791C)
// ---------------------------------------------------------------------------
//
// The routine battle-action state 0x64 calls to decide a retail flee. It
// writes the outcome into `_DAT_8007726C` - the battle-message source pointer
// states 0x64/0x65 test: `ctx + 0x159` ("escaped" text) on success,
// `ctx + 0x189` ("couldn't escape" text) on failure.
//
//   party_score = Σ_party  (SPD*3)>>1 + (maxHP - curHP)>>4     (actor +0x164 / +0x14E / +0x14C)
//   enemy_score = Σ_enemy   SPD      + (maxHP - curHP)>>5
//   roll_p = rand() % party_score;  roll_e = rand() % enemy_score
//   if Escape Boost (ability bit 52):  roll_p += roll_p >> 1
//   if Great Escape (bit 55) or ctx[+0x291] == 2 or forced:  roll_p = roll_e
//   fail  iff  !forced && (roll_p < roll_e || ctx[+0x287] != 0)
//
// Both sides run faster the more hurt they are (missing HP raises the score),
// and the party's SPD is weighted 1.5x against the enemies' 1x. The accessory
// bits are the +0xF8 ability word (passives 0x34 "Escape Boost" / Chicken
// Heart and 0x37 "Great Escape" / Chicken King - the assured-escape bit wins
// the compare exactly but still loses to the no-escape battle flag
// `ctx+0x287`, which is why Chicken King is "assured escape (non-boss)").
// `forced` is the battle flag `_DAT_8007bac0 & 0x100`: it bypasses even the
// no-escape flag and skips the "No. of Escapes" Records counter
// (`_DAT_800846A8`) the normal success path increments.

/// One combatant folded into an escape-roll side score (`FUN_801E791C`).
#[derive(Clone, Copy, Debug, Default)]
pub struct EscapeActor {
    /// Live SPD stat (actor `+0x164`).
    pub speed: u16,
    /// Current HP (actor `+0x14C`).
    pub hp: u16,
    /// Max HP (actor `+0x14E`).
    pub max_hp: u16,
}

/// Party-side flags folded into the escape decision (`FUN_801E791C`).
#[derive(Clone, Copy, Debug, Default)]
pub struct EscapeFlags {
    /// Ability bit 52 (passive `0x34`, Chicken Heart): party roll * 1.5.
    /// Retail ORs the bit over the *living* party members' `+0xF8` words.
    pub escape_boost: bool,
    /// Ability bit 55 (passive `0x37`, Chicken King) - or the `ctx+0x291 == 2`
    /// battle-type byte: the party roll is set equal to the enemy roll, so
    /// the compare can't fail (assured escape) but `no_escape` still blocks.
    pub assured: bool,
    /// `ctx+0x287` - the scripted "can't run from this battle" flag.
    pub no_escape: bool,
    /// `_DAT_8007bac0 & 0x100` - forced flee: succeeds unconditionally
    /// (bypasses even `no_escape`) and skips the flee counter.
    pub forced: bool,
}

impl EscapeFlags {
    /// Bit 20 of the second ability word (`record+0xF8`) = passive index
    /// `0x34` (bit 52 of the 64-bit field) - Escape Boost.
    pub const ESCAPE_BOOST_WORD1: u32 = 0x0010_0000;
    /// Bit 23 of the second ability word = passive index `0x37` (bit 55) -
    /// Great Escape.
    pub const GREAT_ESCAPE_WORD1: u32 = 0x0080_0000;

    /// Fold one living party member's second ability word (`record+0xF8`)
    /// into the flags, the per-slot OR of `FUN_801E791C`'s party loop.
    pub fn fold_ability_word1(&mut self, word1: u32) {
        self.escape_boost |= word1 & Self::ESCAPE_BOOST_WORD1 != 0;
        self.assured |= word1 & Self::GREAT_ESCAPE_WORD1 != 0;
    }
}

/// The party side of the escape compare (`FUN_801E791C` first loop): each
/// party slot contributes `(SPD*3)>>1 + (maxHP - curHP)>>4`. Retail iterates
/// every party slot (downed members included - a downed member still
/// contributes its full missing HP).
pub fn escape_party_score(party: &[EscapeActor]) -> u32 {
    party
        .iter()
        .map(|a| ((a.speed as u32 * 3) >> 1) + ((a.max_hp.saturating_sub(a.hp) as u32) >> 4))
        .sum()
}

/// The enemy side of the escape compare (`FUN_801E791C` second loop): each
/// enemy slot contributes `SPD + (maxHP - curHP)>>5`.
pub fn escape_enemy_score(enemies: &[EscapeActor]) -> u32 {
    enemies
        .iter()
        .map(|a| a.speed as u32 + ((a.max_hp.saturating_sub(a.hp) as u32) >> 5))
        .sum()
}

/// The escape decision of `FUN_801E791C`: `true` = the party gets away.
///
/// `rand` is the routine's two 15-bit PsyQ rand draws in call order (first
/// modulo the party score, second modulo the enemy score). Retail traps on a
/// zero score (`break 0x1C00` on the div); the engine saturates both scores
/// at 1 instead - a zero score cannot occur in a live battle (every living
/// actor has nonzero SPD).
///
/// PORT: FUN_801E791C (roll + compare; the success-side staging - actor
/// scatter toward camera, live-HP writeback to the character records with
/// the downed-member 1-HP floor, flee-counter bump - stays with the callers
/// in `battle_action` / engine-core.)
pub fn escape_roll(party_score: u32, enemy_score: u32, flags: EscapeFlags, rand: [u16; 2]) -> bool {
    let mut roll_p = rand[0] as u32 % party_score.max(1);
    let roll_e = rand[1] as u32 % enemy_score.max(1);
    if flags.escape_boost {
        roll_p += roll_p >> 1;
    }
    if flags.assured || flags.forced {
        roll_p = roll_e;
    }
    if flags.forced {
        return true;
    }
    !(roll_p < roll_e || flags.no_escape)
}
