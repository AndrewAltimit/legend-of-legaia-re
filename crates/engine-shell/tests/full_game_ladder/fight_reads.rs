//! Guards and boss reads: what the fighter reads off the battle before
//! choosing - heal thresholds, a boss's big round, a finisher race, evil
//! magic, a failing run, a solo duel.

use super::*;

/// Percent of max HP below which the fighter heals a member.
pub(super) const HEAL_BELOW_PCT: u32 = 45;
/// [`HEAL_BELOW_PCT`] against a boss ([`boss_fight`]).
pub(super) const BOSS_HEAL_BELOW_PCT: u32 = 60;

/// A fight that forbids running against a foe of eight times the party's
/// largest HP or more (`dohaty`'s 17200 to a 1256-HP Gala).
pub(super) fn boss_fight(w: &legaia_engine_core::world::World) -> bool {
    if !w.battle.no_escape {
        return false;
    }
    let n = w.party.party_count.clamp(1, 3) as usize;
    let ours = (0..n)
        .map(|i| u32::from(w.actors[i].battle.max_hp))
        .max()
        .unwrap_or(1)
        .max(1);
    (n..w.actors.len()).any(|i| {
        let a = &w.actors[i].battle;
        a.hp > 0 && u32::from(a.max_hp) >= ours * 8
    })
}

/// The heal threshold for this fight. Against a boss a member tops up below
/// three fifths: its party-wide hit is unseen until it lands, and the
/// `dohaty` fight's first one (832 / 913 / 671) dropped two members sitting
/// at 54% and 53% of their HP, above the ordinary threshold.
pub(super) fn heal_below_pct(w: &legaia_engine_core::world::World) -> u32 {
    if boss_fight(w) {
        BOSS_HEAL_BELOW_PCT
    } else {
        HEAL_BELOW_PCT
    }
}

thread_local! {
    /// The largest HP loss one member took between two of the party's
    /// command windows this battle. A member is healed while it could not
    /// survive another such stretch - the threshold a player reads off the
    /// last bad round, not a fixed fraction. A round, not a hit: a fast foe
    /// acts twice before the party's next input (Lu Delilas's swing then her
    /// Plasma Strike), and a cast lands its flurry and its burst as separate
    /// HP writes.
    pub(super) static BIGGEST_HIT: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    /// Per member, the HP lost since the party's last command window.
    pub(super) static ROUND_LOSS: std::cell::Cell<[u32; 3]> = const { std::cell::Cell::new([0; 3]) };
    /// Per acting slot, the ally its last committed item was aimed at, so a
    /// later member of the same round counts that heal as already coming.
    pub(super) static ITEM_TARGET: std::cell::RefCell<[Option<u8>; 3]> = const { std::cell::RefCell::new([None; 3]) };
    /// The party's total HP loss in each finished round of this battle, in
    /// order - the record [`big_round_due`] reads a foe's cadence off.
    pub(super) static ROUND_HISTORY: std::cell::RefCell<Vec<u32>> = const { std::cell::RefCell::new(Vec::new()) };
    /// The largest per-member loss of a finished round in which two or more
    /// members lost HP: the size of the foe's party-wide hit (Van Saryu's
    /// Earthquake). See [`wanted_item`]'s backup heal.
    pub(super) static AOE_HIT: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    /// Whether a foe has shown a charge latch this battle ([`foe_charged`]).
    pub(super) static CHARGE_SEEN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Per member, the round ([`ROUND_HISTORY`] length) in which
    /// [`wants_aoe_guard`] last took the stance.
    pub(super) static AOE_GUARDED: std::cell::Cell<[Option<usize>; 3]> =
        const { std::cell::Cell::new([None; 3]) };
    /// Per (member, action kind), the least one such action took off the
    /// foes this battle - what the member can count on its next one dealing
    /// ([`finishing_damage`]). The kind is a spell id, or [`ART_KIND`].
    pub(super) static DEALT: std::cell::RefCell<HashMap<(u8, u16), u32>> = Default::default();
    /// Finished rounds in a row in which two or more members lost HP: a foe
    /// whose party-wide hit has become its every turn ([`spell_heal`]'s
    /// cover).
    pub(super) static AOE_STREAK: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// The [`DEALT`] kind of an Arts string or a plain strike.
pub(super) const ART_KIND: u16 = 0x100;

/// The [`DEALT`] kind of a committed action, `None` for one that deals no
/// damage of the member's own.
pub(super) fn dealt_kind(p: &legaia_engine_core::battle_round::PendingPartyAction) -> Option<u16> {
    use legaia_engine_core::battle_round::PendingPartyAction;
    match p {
        PendingPartyAction::Spell { spell_id, .. } => Some(u16::from(*spell_id)),
        PendingPartyAction::Art { .. } | PendingPartyAction::Attack { .. } => Some(ART_KIND),
        _ => None,
    }
}

/// The HP of the lone foe still standing in a fight that forbids running,
/// `None` while two or more stand.
fn last_foe_hp(w: &legaia_engine_core::world::World) -> Option<u32> {
    if !w.battle.no_escape {
        return None;
    }
    let n = w.party.party_count.clamp(1, 3) as usize;
    let mut living = w
        .actors
        .iter()
        .skip(n)
        .filter(|a| a.active && a.battle.hp > 0);
    let foe = living.next()?;
    living.next().is_none().then_some(u32::from(foe.battle.hp))
}

/// The damage `actor` can count on from its next attack, and whether it is a
/// spell: of the kinds it has used on the foes this battle and can use now
/// (a spell it still has the MP for, or an Arts string), the one whose
/// smallest hit so far is largest.
pub(super) fn finishing_damage(
    w: &legaia_engine_core::world::World,
    actor: u8,
) -> Option<(u32, bool)> {
    let mp = u32::from(w.actors.get(usize::from(actor))?.battle.mp);
    DEALT.with(|d| {
        d.borrow()
            .iter()
            .filter(|((a, _), _)| *a == actor)
            .filter(|((_, kind), _)| {
                *kind == ART_KIND
                    || w.tables
                        .spell_catalog
                        .get(*kind as u8)
                        .is_some_and(|s| mp >= u32::from(s.mp_cost))
            })
            .map(|((_, kind), &dmg)| (dmg, *kind != ART_KIND))
            .max()
    })
}

/// The finishing blow: in a fight that forbids running, one foe is left and
/// `actor`'s next attack takes off at least what it still holds. A player
/// swings then, whatever else the party wants: a heal or a revive only gives
/// the foe another turn (Songi held at 366 HP while a lone Gala revived Vahn
/// into its next strike, round after round).
pub(super) fn finishing_blow(w: &legaia_engine_core::world::World, actor: u8) -> bool {
    match (last_foe_hp(w), finishing_damage(w, actor)) {
        (Some(hp), Some((dmg, _))) => dmg >= hp,
        _ => false,
    }
}

/// Whether `actor`'s turn goes on damage alone: the [`finishing_blow`].
pub(super) fn going_for_the_kill(w: &legaia_engine_core::world::World, actor: u8) -> bool {
    finishing_blow(w, actor)
}

/// Whether the coming round is the foe's heavy one, read off its cadence the
/// way a player does: a boss that has so far hit hard only every other round
/// (Rogue: Element Change, then a party-wide Wind / Thunder / Flame, from the
/// round counter's parity in its picker arm) and whose last round was the
/// quiet one is about to hit again. The last four rounds must read heavy,
/// quiet, heavy, quiet, with no two heavy rounds back to back anywhere: a foe
/// on a longer cycle (a lone Vahn's `nilboa` fight hits hard every third
/// round) is not this pattern, and guarding its quiet rounds only costs the
/// party its attacks.
pub(super) fn big_round_due() -> bool {
    ROUND_HISTORY.with(|h| {
        let h = h.borrow();
        let peak = h.iter().copied().max().unwrap_or(0);
        if peak == 0 || h.len() < 4 {
            return false;
        }
        // A third of the peak, not half: a heavy round the party guarded
        // through lands halved and still has to read as heavy.
        let big = |x: u32| x * 3 >= peak;
        let back_to_back = h.windows(2).any(|w| big(w[0]) && big(w[1]));
        let last: Vec<bool> = h[h.len() - 4..].iter().map(|&x| big(x)).collect();
        !back_to_back && last == [true, false, true, false]
    })
}

/// Whether `actor` should take the Spirit stance this round: the foe's heavy
/// round is due ([`big_round_due`]), its hit is a large share of the
/// member's HP, and the guard's halving is what lets the member live through
/// it. Spirit is up from the commit for the whole round
/// (`World::battle.guarding`), so it covers a foe that acts first.
pub(super) fn wants_guard(w: &legaia_engine_core::world::World, actor: u8) -> bool {
    let Some(a) = w.actors.get(usize::from(actor)) else {
        return false;
    };
    // The HP the member will hold once the round's committed heals land.
    let Some(&hp) = projected_hp(w).get(usize::from(actor)) else {
        return false;
    };
    let max = u32::from(a.battle.max_hp);
    let learned = learned_round_due(w);
    let threat = BIGGEST_HIT
        .with(std::cell::Cell::get)
        .max(learned.unwrap_or(0));
    // A foe winding up shows it: a capture-class charge body (Xain's Bull
    // Charge, PROT 0953 arm 0) sets its caster's ability latch
    // (`0x801C8FE0 + (seat - 3 + 1) * 4`) and deals nothing, and the next
    // cast through the same body is the party-wide punch. A player who sees
    // the charge guards the turn it lands, before any heavy round is on
    // record.
    let charged = foe_charged(w);
    // The charge round itself is quiet; a latch the foe holds while it
    // keeps hitting (an AI ability cooldown such as ids `0x97` / `0x98`
    // re-arm every turn) is not a wind-up, and guarding through it starves
    // the fight (`nilboa`'s lone-Vahn F30).
    let quiet_last = ROUND_HISTORY.with(|h| h.borrow().last() == Some(&0));
    if hp > 0 && charged && quiet_last && u32::from(a.battle.hp) * 4 >= max {
        return true;
    }
    hp > 0
        && threat * 5 >= max * 2
        && hp > threat / 2 + threat / 8
        && (big_round_due() || learned.is_some())
}

/// Koru (`nilboa` P2[20], monster `0xB6`): its pick arm casts by the
/// battle-mode counter - `0xA2`, `0xA3`, `0xA4`, `0xA5` on modes `0..=3`,
/// then Dead End Crisis (`0xA1`) on every mode after (`0x801EB568..`,
/// `monster_ai::decide`), and in this fight that body writes a flat 9999 to
/// every party seat (PROT 0961 arm 3, `0x801F7398`). The fight is a race:
/// a heal spent before the fourth round is damage the party never deals,
/// and nothing a member could be healed to survives the finisher.
pub(super) const KORU_MONSTER_ID: u16 = 0xB6;

/// Whether this fight is a race against a timed finisher
/// ([`KORU_MONSTER_ID`]): every turn goes to damage.
pub(super) fn racing_a_finisher(w: &legaia_engine_core::world::World) -> bool {
    boss_monster_id(w) == Some(KORU_MONSTER_ID)
}

/// The race's last round: the battle-mode counter has reached the value
/// Koru's pick arm answers with the finisher, so whatever the party does not
/// deal before it acts is never dealt - no heal, no buff, only damage.
pub(super) fn finisher_round(w: &legaia_engine_core::world::World) -> bool {
    racing_a_finisher(w) && w.battle.monster_ai_state.mode_flags >= 4
}

/// In a race, the one heal worth a turn: `actor` cannot throw a Miracle Art
/// (its turn is the party's smallest damage) and a member that can would not
/// live through the foe's biggest single-round loss so far. A member lost before
/// the finisher costs every round it would have hit for; a heal from the
/// summoner costs one cast.
pub(super) fn race_heal(w: &legaia_engine_core::world::World, actor: u8) -> bool {
    if miracle_ready(w, actor) || finisher_round(w) {
        return false;
    }
    let threat = BIGGEST_HIT.with(std::cell::Cell::get);
    threat > 0
        && projected_hp(w)
            .iter()
            .enumerate()
            .any(|(i, &h)| h > 0 && h <= threat && miracle_ready(w, i as u8))
}

thread_local! {
    /// What a lost attempt taught about the boss that wiped it: its monster
    /// id, the parity of the battle's stretch index its party-wide hits came
    /// on, and the per-member size of that hit. A player who reloads after
    /// Rogue (`rugi` P2[38]) wiped the party knows its Wind / Thunder /
    /// Flame comes every other round from the second, and guards the first
    /// one too - the cadence arm ([`big_round_due`]) needs four rounds of
    /// history, and a first hit that drops two members leaves a party that
    /// revives one at a quarter HP per quiet round into the next hit.
    /// Kept across the attempts of one segment; cleared when it starts.
    pub(super) static LEARNED_CADENCE: std::cell::Cell<Option<(u16, usize, u32)>> =
        const { std::cell::Cell::new(None) };
}

/// The boss in this fight, if any: the first monster seat's id in a fight
/// that forbids running.
pub(super) fn boss_monster_id(w: &legaia_engine_core::world::World) -> Option<u16> {
    if !w.battle.no_escape {
        return None;
    }
    w.actors.iter().find_map(|a| a.battle_monster_id)
}

/// The learned party-wide hit ([`LEARNED_CADENCE`]) when this fight is the
/// boss it was learned on and the coming stretch is one of its heavy ones.
pub(super) fn learned_round_due(w: &legaia_engine_core::world::World) -> Option<u32> {
    let (id, parity, aoe) = LEARNED_CADENCE.with(std::cell::Cell::get)?;
    if boss_monster_id(w) != Some(id) {
        return None;
    }
    let next = ROUND_HISTORY.with(|h| h.borrow().len());
    (next % 2 == parity).then_some(aoe)
}

/// Read a lost battle's round history for a strict every-other-round
/// party-wide hitter: two heavy stretches or more, all on one parity, never
/// two in a row. Returns the parity.
pub(super) fn alternating_heavy_parity(history: &[u32]) -> Option<usize> {
    let peak = history.iter().copied().max().unwrap_or(0);
    if peak == 0 {
        return None;
    }
    let heavy: Vec<usize> = history
        .iter()
        .enumerate()
        .filter(|&(_, &x)| x * 3 >= peak)
        .map(|(i, _)| i)
        .collect();
    let parity = *heavy.first()? % 2;
    (heavy.len() >= 2
        && heavy.iter().all(|i| i % 2 == parity)
        && heavy.windows(2).all(|p| p[1] - p[0] == 2))
    .then_some(parity)
}

/// Whether `actor` should take the Spirit stance against a party-wide hitter
/// (Van Saryu's Earthquake): in a fight that forbids running, a member whose
/// HP the foe's party-wide hit ([`AOE_HIT`]) would take to zero, but whom the
/// stance's halving lets live through it, guards rather than attacks. A
/// member below even the halved hit is the heal arm's; the guard is for the
/// band between, where it turns a death into a scratch whichever way the
/// round's turn order falls.
pub(super) fn wants_aoe_guard(w: &legaia_engine_core::world::World, actor: u8) -> bool {
    // A foe that telegraphs its party-wide hit (a charge latch) is guarded on
    // the tell ([`wants_guard`]); guarding on HP alone against it spends the
    // rounds between charges in the stance, and Xain outlasted the battle
    // cap that way with every member alive.
    if !w.battle.no_escape || CHARGE_SEEN.with(std::cell::Cell::get) {
        return false;
    }
    let aoe = AOE_HIT.with(std::cell::Cell::get);
    let Some(&hp) = projected_hp(w).get(usize::from(actor)) else {
        return false;
    };
    // Only against a hit that takes two fifths of the member or more (an
    // Earthquake is over half of a level-20 Vahn). Against lighter
    // party-wide hits the stance only costs the swings: Rogue (`rugi`
    // P2[38]) and the `chitei2` P2[13] fight both ran out the battle cap
    // with the party healthy.
    let max = u32::from(w.actors[usize::from(actor)].battle.max_hp);
    // Nor on the round after a heavy one from a foe that has never hit hard
    // twice running: Rogue's party-wide hit comes every other round, the
    // round after it is quiet, and guarding through it as well (every member
    // sits inside one unguarded hit) left nobody swinging - the cadence arm
    // ([`wants_guard`]) covers its heavy rounds.
    let quiet_next = ROUND_HISTORY.with(|h| {
        let h = h.borrow();
        let peak = h.iter().copied().max().unwrap_or(0);
        let big = |x: u32| peak > 0 && x * 3 >= peak;
        h.last().is_some_and(|&l| big(l)) && !h.windows(2).any(|p| big(p[0]) && big(p[1]))
    });
    // And never at nine tenths of full or above: a hit that drops even a
    // healthy member is the heal arm's to answer after it lands, and guarding
    // against it at full strength guards every round (`chitei2` P2[13]: all
    // three in the stance at ~1600/1700 HP until the battle cap).
    // Nor round after round against a foe whose party-wide hit is not every
    // round: a stance the last round spent with no such hit landing is not
    // taken again on the same read. Gaza (`korb3` P2[2]) lands its party-wide
    // hit every third round or so, and all three members held the stance
    // three rounds running at half their HP, healing nothing and dealing
    // nothing, until its single hits had worn them under the band.
    let round = ROUND_HISTORY.with(|h| h.borrow().len());
    let last_loss = ROUND_HISTORY.with(|h| h.borrow().last().copied().unwrap_or(0));
    let guarded_last = AOE_GUARDED.with(|g| {
        g.get()
            .get(usize::from(actor))
            .copied()
            .flatten()
            .is_some_and(|r| r + 1 == round)
    });
    if guarded_last && last_loss < aoe {
        return false;
    }
    let want = aoe > 0
        && !quiet_next
        && aoe * 5 >= max * 2
        && hp * 10 < max * 9
        && hp > aoe * 5 / 8
        && hp <= aoe + aoe / 8;
    if want {
        AOE_GUARDED.with(|g| {
            let mut v = g.get();
            if let Some(s) = v.get_mut(usize::from(actor)) {
                *s = Some(round);
            }
            g.set(v);
        });
    }
    want
}

/// [`wants_aoe_guard`], taken ahead of a heal: the member is the one in
/// danger and no ally is worse off (down, or below even the halved hit). The
/// stance is up from the commit, so it covers a foe that acts first; a heal
/// on itself lands only on the member's own turn, which the round's drawn
/// order may put behind the Earthquake that drops it.
pub(super) fn guard_before_heal(w: &legaia_engine_core::world::World, actor: u8) -> bool {
    if !wants_aoe_guard(w, actor) {
        return false;
    }
    let aoe = AOE_HIT.with(std::cell::Cell::get);
    projected_hp(w)
        .iter()
        .enumerate()
        .all(|(i, &h)| i == usize::from(actor) || h > aoe * 5 / 8)
}

/// Whether the coming round may bring Evil Seru Magic (`0xAD`), the
/// party-wide flurry and burst of monster `0xB4` (the `chitei2` P2[13]
/// fight): its pick arm (`FUN_801E9FD4`, ported as `monster_ai::decide`)
/// rolls the cast one time in three, but only once its Mystic Shield is
/// down (`_DAT_8007BD84` clear - the damage halve a player sees lift), on
/// the odd values of the per-round battle-mode counter, and while it holds
/// the 255 MP the cast spends. A lv30 party takes about 1900 a member from
/// it unguarded - more than any member holds - and about half that in the
/// Spirit stance, so a player who has watched the shield drop guards the
/// alternate rounds the cast can come on and acts on the others.
pub(super) fn evil_magic_due(w: &legaia_engine_core::world::World) -> bool {
    const EVIL_MAGIC_CASTER: u16 = 0xB4;
    const EVIL_MAGIC_MP: u16 = 0xFF;
    let ai = &w.battle.monster_ai_state;
    if ai.flag_bd84 != 0 || ai.mode_flags & 1 == 0 {
        return false;
    }
    let n = w.party.party_count.clamp(1, 3) as usize;
    (n..w.actors.len()).any(|i| {
        let a = &w.actors[i];
        a.battle.hp > 0
            && a.battle_monster_id == Some(EVIL_MAGIC_CASTER)
            && a.battle.mp >= EVIL_MAGIC_MP
            && u32::from(a.battle.hp) <= u32::from(a.battle.max_hp) / 2
    })
}

/// [`evil_magic_due`] for one member: it guards when the stance lets it
/// live through the cast (about half of what it does unguarded), and is left
/// to the heal arm when even the guarded hit would drop it.
pub(super) fn wants_evil_magic_guard(w: &legaia_engine_core::world::World, actor: u8) -> bool {
    if !evil_magic_due(w) {
        return false;
    }
    let Some(a) = w.actors.get(usize::from(actor)) else {
        return false;
    };
    let hp = projected_hp(w)
        .get(usize::from(actor))
        .copied()
        .unwrap_or(0);
    hp > 0 && hp * 100 >= u32::from(a.battle.max_hp) * 55
}

/// Whether the foes' last round took half or more of the HP the party
/// still holds: healing cannot keep pace with that, and fleeing can.
/// Whether the hand has fled this fight twice and is still in it, against
/// foes whose combined max HP the party's own outweighs by no more than four
/// to one - a fight it can win by fighting. Every finished round of a fled
/// fight is a failed run, since a run that lands ends the battle.
pub(super) fn runs_failing(w: &legaia_engine_core::world::World) -> bool {
    let rounds = ROUND_HISTORY.with(|h| h.borrow().len());
    if rounds < 3 {
        return false;
    }
    let n = w.party.party_count.clamp(1, 3) as usize;
    let ours: u32 = (0..n).map(|i| u32::from(w.actors[i].battle.max_hp)).sum();
    let theirs: u32 = (n..w.actors.len())
        .filter(|&i| w.actors[i].battle.hp > 0)
        .map(|i| u32::from(w.actors[i].battle.max_hp))
        .sum();
    theirs <= ours * 4
}

pub(super) fn losing_race(w: &legaia_engine_core::world::World) -> bool {
    let last = ROUND_HISTORY.with(|h| h.borrow().last().copied().unwrap_or(0));
    last > 0 && last * 2 >= party_hp_key(w)
}

/// Whether a living foe holds its ability latch - the wind-up of a charge
/// body (Xain's Bull Charge) before the party-wide hit it sets up. See
/// [`wants_guard`].
pub(super) fn foe_charged(w: &legaia_engine_core::world::World) -> bool {
    let n = w.party.party_count.clamp(1, 3) as usize;
    (n..w.actors.len()).any(|i| {
        w.actors[i].battle.hp > 0
            && w.battle
                .monster_ai_state
                .dat
                .get(i - n + 4)
                .is_some_and(|&v| v != 0)
    })
}

/// A **solo duel**: one party member against a no-escape fight whose foe
/// outweighs it several times over (the Nivora duels - `nilboa`'s Che
/// Delilas against Gala, 12000 HP to 1446). Plain swings barely scratch such
/// a foe (the underdog floor) and its three real swings a round take half the
/// member's HP, so the fight is won the way a player wins it: Arts at a full
/// gauge, Spirit between them.
pub(super) fn solo_duel(w: &legaia_engine_core::world::World) -> bool {
    let n = w.party.party_count.clamp(1, 3) as usize;
    if n != 1 || !w.battle.no_escape {
        return false;
    }
    let ours = u32::from(w.actors[0].battle.max_hp).max(1);
    (n..w.actors.len()).any(|i| {
        let a = &w.actors[i].battle;
        a.hp > 0 && u32::from(a.max_hp) >= ours * 4
    })
}

/// The duel's Spirit turn. The fighter alternates: a Spirit round, then an
/// Arts round on the gauge it extended - the stance triples the member's
/// guard for the foe's swings and halves its specials, so the rounds the foe
/// spends against it cost little, and a Delilas special (Che's Megaton
/// Press, Lu's Plasma Strike) lands on a guarded member far more often than
/// not. Below five eighths of its HP the member holds the stance round after
/// round until the heal arm ahead of this one has topped it up.
pub(super) fn duel_wants_spirit(w: &legaia_engine_core::world::World, actor: u8) -> bool {
    if !solo_duel(w) {
        return false;
    }
    let a = &w.actors[usize::from(actor)].battle;
    // Held round after round below five eighths, or while one of the foe's
    // heaviest rounds so far (a Delilas special lands most of a member's HP
    // at once) would drop the member unguarded.
    let threat = BIGGEST_HIT.with(std::cell::Cell::get);
    // A Delilas round varies by its swing count: Che's took 716 one round
    // and 903 two rounds later, so an Arts round waits while the member sits
    // within three twentieths again of the worst unguarded round seen, and
    // below five eighths of its HP (a 900-HP Gala at 62% swung into 903).
    // Half again starved the fight: Gala guarded and healed to the cap.
    // Held only while the bag can still top the member up: with no
    // restorative left the stance only waits for the foe to finish it.
    let low = bag_can_heal(w)
        && (u32::from(a.hp) * 8 < u32::from(a.max_hp) * 5 || u32::from(a.hp) * 20 <= threat * 23);
    // The choice is latched per foe round: the fighter re-reads the menu on
    // every pad poll until the press lands, and a choice that flipped under
    // it would press something else.
    let round = ROUND_HISTORY.with(|h| h.borrow().len());
    match DUEL_SPIRIT_ROUND.with(std::cell::Cell::get) {
        Some(r) if r == round => return true,
        Some(r) if r + 1 == round && !low => return false,
        _ => {}
    }
    DUEL_SPIRIT_ROUND.with(|d| d.set(Some(round)));
    true
}

/// Does `actor` know a healing spell?
pub(super) fn knows_heal_spell(w: &legaia_engine_core::world::World, actor: u8) -> bool {
    use legaia_engine_battle::spells::SpellEffect;
    let Some(m) = w
        .party
        .roster
        .members
        .get(w.party_roster_slot(usize::from(actor)))
    else {
        return false;
    };
    let list = m.spell_list();
    list.ids[..(list.count as usize).min(list.ids.len())]
        .iter()
        .any(|&id| {
            w.tables.spell_catalog.get(id).is_some_and(|d| {
                matches!(
                    d.effect,
                    SpellEffect::Heal { .. } | SpellEffect::HealAll { .. }
                )
            })
        })
}

/// Does the bag hold an HP restorative usable in battle?
pub(super) fn bag_can_heal(w: &legaia_engine_core::world::World) -> bool {
    use legaia_engine_menus::items::ItemEffect;
    w.party.inventory.iter().any(|(&id, &c)| {
        c > 0
            && w.tables.item_catalog.get(id).is_some_and(|e| {
                e.usable_in_battle
                    && matches!(e.effect, ItemEffect::Heal { .. } | ItemEffect::HealAll)
            })
    })
}

/// Is the pending action a party-wide heal spell?
pub(super) fn is_party_heal_spell(
    w: &legaia_engine_core::world::World,
    p: &Option<legaia_engine_core::battle_round::PendingPartyAction>,
) -> bool {
    use legaia_engine_battle::spells::SpellEffect;
    matches!(p, Some(legaia_engine_core::battle_round::PendingPartyAction::Spell { spell_id, .. })
    if w.tables.spell_catalog.get(*spell_id).is_some_and(|d| {
        matches!(d.effect, SpellEffect::HealAll { .. })
    }))
}

/// Can party slot `i` take a turn: standing, and under no status that
/// blocks the turn (Numb, Sleep, Stone, Faint: the `0xF84` group the
/// selectable scans skip). A Stoned member keeps its HP for the rest of the
/// battle and is counted as down by the `0x5A` wipe scan
/// (`docs/subsystems/battle-round-loop.md`), so it neither acts nor needs
/// a heal.
pub(super) fn can_act(w: &legaia_engine_core::world::World, i: usize) -> bool {
    w.actors.get(i).is_some_and(|a| a.battle.hp > 0)
        && !w
            .battle
            .status_effects
            .statuses(i as u8)
            .iter()
            .any(|s| s.kind.blocks_actions())
}

/// Is party slot `i` Stoned: invulnerable, out of the fight, and down to the
/// wipe scan.
pub(super) fn stoned(w: &legaia_engine_core::world::World, i: usize) -> bool {
    w.battle.status_effects.has(
        i as u8,
        legaia_engine_battle_vm::status_effects::StatusKind::Stone,
    )
}

/// The healing spell worth casting now in a fight that forbids running: the
/// one `actor` knows and can pay for that restores the most.
///
/// - In a **solo duel** it is cast once the bag has no restorative left,
///   while the member is under the duel's heal line (three fifths of its
///   HP). The three Nivora duels are fought out of one bag, and each
///   duellist knows Spoon (45 MP): a member that ran the bag dry held the
///   Spirit stance with a full MP pool until the foe finished it.
/// - In a **party fight** the group heal is cast for any member under the
///   line once the bag is dry (Gaza's second fight, `korb3`, ran it dry with
///   every member's Spoon unspent), and ahead of the bag whenever two members
///   are under it: one Spoon restores ~1150 on each of three members, where an
///   item's turn restores one. The line there is also the foe's party-wide
///   hit with a quarter's margin: Zora's (`taiku` P2[19]) is ~1150 a member,
///   two thirds of a 1710-HP Noa.
pub(super) fn spell_heal(w: &legaia_engine_core::world::World, actor: u8) -> Option<u8> {
    use legaia_engine_battle::spells::SpellEffect;
    if !w.battle.no_escape {
        return None;
    }
    let duel = solo_duel(w);
    if duel && bag_can_heal(w) {
        return None;
    }
    let a = &w.actors.get(usize::from(actor))?.battle;
    if a.hp == 0 {
        return None;
    }
    let n = w.party.party_count.clamp(1, 3) as usize;
    let aoe = AOE_HIT.with(std::cell::Cell::get);
    // One group heal a round covers the party: a heal the round's earlier
    // members committed counts as landed ([`projected_hp`]). The exception
    // is the backup the item arm also commits ([`wanted_item`]): while a
    // member stands within one party-wide hit of death, the round's drawn
    // order may put the first caster behind the foe, so a second member
    // casts on the HP as it stands. A third never does.
    let queued = w
        .battle
        .round_flow
        .pending
        .iter()
        .take(n)
        .filter(|p| is_party_heal_spell(w, p))
        .count();
    let raw: Vec<u32> = (0..n).map(|i| u32::from(w.actors[i].battle.hp)).collect();
    let brink = aoe > 0 && raw.iter().any(|&h| h > 0 && h <= aoe + aoe / 8);
    if queued >= 2 {
        return None;
    }
    // The backup is a third member's to spare: with two standing, a second
    // group heal leaves nobody swinging, and against a barrage that is every
    // round (`noaru`'s Songi: Noa and Gala cast Spoon side by side for nine
    // rounds with Vahn down and its HP untouched, until the battle stalled).
    let standing = (0..n).filter(|&i| can_act(w, i)).count();
    if queued == 1 && standing < 3 {
        return None;
    }
    let hp = if queued == 1 && brink {
        raw
    } else {
        projected_hp(w)
    };
    let low = |i: usize| {
        let max = u32::from(w.actors[i].battle.max_hp);
        let hit = if duel {
            0
        } else {
            (aoe + aoe / 4).min(max * 95 / 100)
        };
        hp[i] > 0 && !stoned(w, i) && (hp[i] * 5 < max * 3 || hp[i] <= hit)
    };
    if duel {
        if !low(usize::from(actor)) {
            return None;
        }
    } else {
        let lows = (0..n).filter(|&i| low(i)).count();
        // Cover against a foe whose party-wide hit has landed two rounds
        // running (Songi's Genocidal Cannon below half HP, ~1000 a guarded
        // member, every turn while its MP lasts): one group heal is queued
        // every round while some member could not take two more of it, full
        // HP or not. The foe usually acts ahead of the party, so a heal cast
        // only once members read low lands after the hit that took them there
        // and leaves the party on the line for the next round's opening hit;
        // the hand healed one round and swung with all three the next, and
        // the hit that opened the round after dropped two members at once.
        let cover = AOE_STREAK.with(std::cell::Cell::get) >= 2
            && queued == 0
            && (0..n).any(|i| hp[i] > 0 && hp[i] <= 2 * aoe + aoe / 8);
        if !cover && (lows == 0 || lows < 2 && bag_can_heal(w)) {
            return None;
        }
        // The group heal is the turn of the member whose swing deals least:
        // one still to commit this round who can pay for a group heal and
        // whose actions have taken less off the foes ([`DEALT`]) casts it,
        // and this member swings. A boss's race is lost on the round the
        // heavy hitter heals (`nilboa` P2[20]: Vahn's Spoon in place of a
        // ~2500 Art let the foe reach the party-wide blow it opens its fifth
        // round with, where Gala's ~1200 spell was the cheaper turn to give).
        if queued == 0 {
            let best = |i: usize| {
                DEALT.with(|d| {
                    d.borrow()
                        .iter()
                        .filter(|((m, _), _)| usize::from(*m) == i)
                        .map(|(_, &v)| v)
                        .max()
                        .unwrap_or(0)
                })
            };
            let mine = best(usize::from(actor));
            let weaker_healer = (0..n).any(|j| {
                // A member that cannot take its turn casts nothing: Gala
                // Stoned by Zora's Glare was the "weaker healer" both other
                // members left the Spoon to, and Darkness Typhoon wiped
                // them at 791 and 717 HP with 99 MP in Vahn's pool.
                j != usize::from(actor)
                    && can_act(w, j)
                    && w.battle
                        .round_flow
                        .pending
                        .get(j)
                        .is_some_and(Option::is_none)
                    && best(j) < mine
                    && can_cast_group_heal(w, j as u8)
            });
            if weaker_healer {
                return None;
            }
        }
    }
    let m = w
        .party
        .roster
        .members
        .get(w.party_roster_slot(usize::from(actor)))?;
    let list = m.spell_list();
    list.ids[..(list.count as usize).min(list.ids.len())]
        .iter()
        .filter_map(|&id| {
            let d = w.tables.spell_catalog.get(id)?;
            let amount = match d.effect {
                SpellEffect::HealAll { amount } => amount,
                // A single heal's target is the duellist itself.
                SpellEffect::Heal { amount } if duel => amount,
                _ => return None,
            };
            (u32::from(d.mp_cost) <= u32::from(a.mp)).then_some((amount, id))
        })
        .max()
        .map(|(_, id)| id)
}

/// Whether `actor` knows a group heal it can pay for now.
fn can_cast_group_heal(w: &legaia_engine_core::world::World, actor: u8) -> bool {
    use legaia_engine_battle::spells::SpellEffect;
    let Some(a) = w.actors.get(usize::from(actor)) else {
        return false;
    };
    let Some(m) = w
        .party
        .roster
        .members
        .get(w.party_roster_slot(usize::from(actor)))
    else {
        return false;
    };
    let list = m.spell_list();
    list.ids[..(list.count as usize).min(list.ids.len())]
        .iter()
        .filter_map(|&id| w.tables.spell_catalog.get(id))
        .any(|d| {
            matches!(d.effect, SpellEffect::HealAll { .. })
                && u32::from(d.mp_cost) <= u32::from(a.battle.mp)
        })
}
