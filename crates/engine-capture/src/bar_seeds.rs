//! [`BarSeed`]: the HP / MP / status bars a capture shows, applied to the
//! engine's combatants, with their environment hand-off.

/// [`BarSeed::slot`] values from this base name an **empty retail pool
/// seat** (`slot - EMPTY_SEAT_SLOT_BASE`) rather than an engine actor: only
/// the ground pair is read, and it stands the seat's struct
/// (`World::seat_empty_battle_seat`). Engine slots never reach it.
pub const EMPTY_SEAT_SLOT_BASE: u8 = 0x80;

/// One combatant's mid-fight bars as the capture read them: engine actor
/// slot, HP, MP - and, where the seed places it, the live ground pair
/// `+0x34` / `+0x38` (`RetailBattle::seeded_ground`). A slot from
/// [`EMPTY_SEAT_SLOT_BASE`] carries an empty seat's ground instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BarSeed {
    pub slot: u8,
    pub hp: u16,
    pub mp: u16,
    /// A party seat's maximum `+0x14E` where the capture's own word stands
    /// off its character record's: a probe raised the seat's pool
    /// (`LEGAIA_TOUGH=2`), which the fight reads and the HUD shows but the
    /// record never saw. `None` keeps the maximum the record gave the engine.
    pub hp_max: Option<u16>,
    /// A party seat's maximum MP `+0x152` where the capture's word stands off
    /// its character record's: a battle-local write the replay never ran
    /// (Lapis Wave zeroes it). `None` keeps the maximum the record gave.
    pub mp_max: Option<u16>,
    pub ground: Option<[i16; 2]>,
    /// The heading `+0x46` that goes with [`Self::ground`].
    pub facing: Option<u16>,
    /// A body seeded inside (or past) its defeat fade: the colour lanes it
    /// holds (`RetailBattle::defeat_lanes`).
    pub defeat_lanes: Option<u32>,
    /// The ailments the capture's status word `+0x16E` carries
    /// ([`seedable_status_bits`]); `0` for none.
    pub status: u16,
    /// The initiative key `+0x16C` the seed stands this combatant on for the
    /// first round it plays, or `None` to keep the engine's own roll.
    ///
    /// `Some(0)` is a key the capture holds spent: the combatant had already
    /// dispatched in the round the capture was taken in (`sh zero,0x16c(s3)`
    /// at `0x801E2CDC`, the action SM's `0x0C`), so it sits out that round.
    /// A live key orders the replayed round as the capture's own remainder:
    /// the pick `FUN_801DABA4` takes the highest key first.
    pub init_key: Option<u16>,
    /// The monster's self-heal latch `DAT_801C8FE0[index + 4]` stands raised
    /// for the replay's pick (`RetailBattle::heal_gate_passed`).
    pub heal_latch: bool,
    /// The Spirit gauge `+0x170` of a body whose size follows it (Dohati's,
    /// `World::tick_dohati_growth`); `None` keeps the engine's.
    pub gauge: Option<u16>,
}

/// The status-word bits a seed carries over: the ailments the engine tracks
/// as [`legaia_engine_battle_vm::status_effects::StatusKind`]s. The AI-delegation
/// group `0x380` is left out - it is Rage's equipment passive or a charm, not
/// an ailment the tracker models as one.
pub fn seedable_status_bits(word: u16) -> u16 {
    use legaia_engine_battle_vm::status_effects::display_flags as f;
    word & (f::AILMENT_MASK & !f::CONFUSE_MASK)
}

/// Raise the ailments `word` carries on engine slot `slot`.
pub(crate) fn seed_status(world: &mut legaia_engine_core::world::World, slot: u8, word: u16) {
    use legaia_engine_battle_vm::status_effects::{StatusKind, display_flags as f};
    let tracker = &mut world.battle.status_effects;
    for (bit, kind) in [
        (f::VENOM, StatusKind::Venom),
        (f::TOXIC, StatusKind::Toxic),
        (f::STONE, StatusKind::Stone),
        (f::NUMB, StatusKind::Numb),
        (f::SLEEP, StatusKind::Sleep),
        (f::CURSE, StatusKind::Curse),
    ] {
        if word & bit != 0 {
            tracker.apply(slot, kind);
        }
    }
    for (limb, bit) in f::ROT_LIMBS.iter().enumerate() {
        if word & bit != 0 {
            tracker.apply(slot, StatusKind::Rot);
            tracker.set_rot_limb(slot, limb as u8);
        }
    }
}

/// Seed the capture's HP / MP onto the engine actors (the engine enters a
/// fight on full bars). Run on the first battle tick by the headless seed and
/// by the `play-window` image child alike: the monster AI's picks and every
/// kill read these bars, so a child on full bars plays a different fight
/// from the one its seed scored.
///
/// A seed that places ground also holds the action seed's formation
/// recentre (`BattleState::seed_recentre_held`): the captured ground
/// already carries it.
pub fn apply_bar_seeds(world: &mut legaia_engine_core::world::World, seeds: &[BarSeed]) {
    if seeds.iter().any(|s| s.ground.is_some()) {
        world.battle.seed_recentre_held = true;
    }
    // A round start that already picked did so on the engine's own keys.
    if seeds.iter().any(|s| s.init_key.is_some()) {
        world.forget_parked_turn();
    }
    for s in seeds {
        if let Some(pool) = s.slot.checked_sub(EMPTY_SEAT_SLOT_BASE) {
            if let Some([x, z]) = s.ground {
                world.seat_empty_battle_seat(pool, x, z);
            }
            continue;
        }
        let Some(a) = world.actors.get_mut(usize::from(s.slot)) else {
            continue;
        };
        if let Some(m) = s.hp_max {
            a.battle.max_hp = m;
        }
        if let Some(m) = s.mp_max {
            a.battle.mp_max = m;
        }
        a.battle.hp = s.hp;
        a.battle.liveness = a.battle.hp;
        if a.battle.hp_display.is_some() {
            a.battle.hp_display = Some(a.battle.hp);
        }
        a.battle.mp = s.mp;
        if let Some([x, z]) = s.ground {
            a.move_state.world_x = x;
            a.move_state.world_z = z;
            if a.battle.seat.is_some() {
                a.battle.seat = Some((x, z));
            }
        }
        if let Some(f) = s.facing {
            a.battle.facing_angle = f;
        }
        if let Some(lanes) = s.defeat_lanes
            && a.battle.hp == 0
        {
            a.battle.render_flag = legaia_engine_battle_vm::battle_formulas::STATE_DEFEAT_FADE;
            a.battle.render_color = lanes;
        }
        if let Some(k) = s.init_key {
            a.battle.init_key = k;
        }
        if let Some(g) = s.gauge {
            a.battle.spirit_gauge = g;
        }
        if s.heal_latch {
            let index = usize::from(s.slot).saturating_sub(usize::from(world.party.party_count));
            if let Some(l) = world.battle.monster_ai_state.dat.get_mut(index + 4) {
                *l = 1;
            }
        }
        // A party member seeded down kneels, as the capture's does: the
        // seat skips the fall that leads there.
        if s.hp == 0 && s.defeat_lanes.is_none() {
            world.seat_downed_kneel(usize::from(s.slot));
        }
        if s.status != 0 {
            seed_status(world, s.slot, s.status);
        }
    }
}

/// `slot:hp:mp[:x:z[:facing]][:d<lanes>][:s<status>][:k[<key>]][:m<max>][:g<gauge>][:v],...` for
/// `LEGAIA_BATTLE_BARS`.
pub fn bar_seeds_to_env(seeds: &[BarSeed]) -> String {
    seeds
        .iter()
        .map(|s| {
            let head = match (s.ground, s.facing) {
                (Some([x, z]), Some(f)) => format!("{}:{}:{}:{x}:{z}:{f}", s.slot, s.hp, s.mp),
                (Some([x, z]), None) => format!("{}:{}:{}:{x}:{z}", s.slot, s.hp, s.mp),
                _ => format!("{}:{}:{}", s.slot, s.hp, s.mp),
            };
            let head = match s.defeat_lanes {
                Some(l) => format!("{head}:d{l:x}"),
                None => head,
            };
            let head = match s.status {
                0 => head,
                w => format!("{head}:s{w:x}"),
            };
            let head = match s.init_key {
                Some(0) => format!("{head}:k"),
                Some(k) => format!("{head}:k{k}"),
                None => head,
            };
            let head = match s.hp_max {
                Some(m) => format!("{head}:m{m}"),
                None => head,
            };
            let head = match s.mp_max {
                Some(m) => format!("{head}:n{m}"),
                None => head,
            };
            let head = match s.gauge {
                Some(g) => format!("{head}:g{g}"),
                None => head,
            };
            if s.heal_latch {
                format!("{head}:v")
            } else {
                head
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// The inverse of [`bar_seeds_to_env`]; malformed entries are skipped.
pub fn bar_seeds_from_env(v: &str) -> Vec<BarSeed> {
    v.split(',')
        .filter_map(|e| {
            let e = e.trim();
            let (e, heal_latch) = match e.strip_suffix(":v") {
                Some(head) => (head, true),
                None => (e, false),
            };
            let (e, gauge) = match e.rsplit_once(":g") {
                Some((head, g)) => (head, Some(g.parse().ok()?)),
                None => (e, None),
            };
            let (e, mp_max) = match e.rsplit_once(":n") {
                Some((head, m)) => (head, Some(m.parse().ok()?)),
                None => (e, None),
            };
            let (e, hp_max) = match e.rsplit_once(":m") {
                Some((head, m)) => (head, Some(m.parse().ok()?)),
                None => (e, None),
            };
            let (e, init_key) = match e.rsplit_once(":k") {
                Some((head, "")) => (head, Some(0)),
                Some((head, k)) => (head, Some(k.parse().ok()?)),
                None => (e, None),
            };
            let (e, status) = match e.rsplit_once(":s") {
                Some((head, w)) => (head, u16::from_str_radix(w, 16).ok()?),
                None => (e, 0),
            };
            let (e, defeat_lanes) = match e.rsplit_once(":d") {
                Some((head, l)) => (head, Some(u32::from_str_radix(l, 16).ok()?)),
                None => (e.trim(), None),
            };
            let mut it = e.split(':');
            let slot = it.next()?.parse().ok()?;
            let hp = it.next()?.parse().ok()?;
            let mp = it.next()?.parse().ok()?;
            let ground = match (it.next(), it.next()) {
                (Some(x), Some(z)) => Some([x.parse().ok()?, z.parse().ok()?]),
                _ => None,
            };
            let facing = match it.next() {
                Some(f) => Some(f.parse().ok()?),
                None => None,
            };
            Some(BarSeed {
                slot,
                hp,
                mp,
                hp_max,
                mp_max,
                ground,
                facing,
                defeat_lanes,
                status,
                init_key,
                heal_latch,
                gauge,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests;

/// Arm the re-seat of a driven action capture
/// ([`legaia_engine_battle_loop::battle_state::BattleState::capture_reseat`]): each time
/// engine slot `seat`'s action seeds, every combatant whose seed places
/// ground stands back on it. Both the headless seed and the image child
/// call it beside [`apply_bar_seeds`].
///
/// `approach_yaw` is a monster-melee capture's arrival
/// ([`legaia_engine_battle_loop::battle_state::BattleState::capture_approach_yaw`]): the
/// attacker's walk onto its target then starts where retail's did.
pub fn arm_capture_reseat(
    world: &mut legaia_engine_core::world::World,
    seat: u8,
    seeds: &[BarSeed],
    approach_yaw: Option<legaia_engine_battle_loop::battle_state::CaptureApproach>,
) {
    let ground: Vec<(u8, i16, i16)> = seeds
        .iter()
        .filter(|s| s.slot < EMPTY_SEAT_SLOT_BASE)
        .filter_map(|s| s.ground.map(|[x, z]| (s.slot, x, z)))
        .collect();
    if !ground.is_empty() {
        world.battle.capture_reseat = Some((seat, ground));
        world.battle.capture_approach_yaw = approach_yaw;
        world.battle.capture_reseat_bars = seeds
            .iter()
            .filter(|s| s.slot < EMPTY_SEAT_SLOT_BASE)
            .map(
                |s| legaia_engine_battle_loop::battle_state::CaptureReseatBars {
                    slot: s.slot,
                    hp: s.hp,
                    mp: s.mp,
                    status: seedable_status_bits(s.status),
                    gauge: s.gauge,
                },
            )
            .collect();
    }
}
