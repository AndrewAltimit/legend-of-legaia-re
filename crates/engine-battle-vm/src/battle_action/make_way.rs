//! The **make-way step**: bystanders stepping out of a melee's path.
//!
//! When an attack starts, state `0x14` gives every combatant that is neither
//! the attacker nor its target a heading at right angles to the attack line,
//! on the side of the line it stands on (`0x801E30CC..0x801E3198`), and seeds
//! the countdown `ctx[+0x6D6]` with the melee monster's size class
//! (`0x801E31A0..0x801E31E8`). The action SM's shared tail (`0x801E6814`,
//! which every state's pass ends on) then steps each of those combatants
//! along its heading while the countdown runs, and drains the countdown by
//! the frame step (`0x801E6824..0x801E6934`). The party members the attacker
//! walks past part to either side of its lane instead of standing in it.
//!
//! The step a pass takes is `(lut * frame_step) >> 9` per axis - `sin` into
//! the live `+0x34`, `cos` into the live `+0x38`, both 16-bit wrapping
//! (`lhu` / `addu` / `sh`) - so a whole countdown moves a bystander
//! `size_class * 8` units at most, whatever the frame step.
//!
//! The tail's attacker and target are the ones the pass opened with: retail
//! loads `ctx[+0x13]` and the attacker's `+0x1DD` in the prologue
//! (`0x801E2994..0x801E29C0`) and the tail tests them (`beq s0,v0` /
//! `beq s0,t2`). The tail walks retail's seven pool slots with no liveness
//! test, so a fallen monster is moved too; the port walks every seated
//! engine slot.
//!
//! PORT: FUN_801E295C (`0x801E30CC..0x801E31E8` state `0x14`'s make-way arm,
//! `0x801E6814..0x801E6934` the shared tail)

use super::*;

/// The shift the tail applies to the LUT product: `sra v1,t2,0x9`.
pub const MAKE_WAY_SHIFT: u32 = 9;

/// The make-way heading of a bystander at `other`, for an attack from
/// `attacker` at `target` (live pairs, `(x, z)`).
///
/// `heading = (bearing(target -> attacker) + 0x800) & 0xFFF` is the attack
/// heading. The bystander's own `(bearing(other -> attacker) + 0x800)` read
/// against it picks the side: within a half turn (`< 0x800`) it takes
/// `heading + 0x400`, else `heading - 0x400` (`0x801E314C..0x801E3188`).
pub fn make_way_angle(attacker: (i16, i16), target: (i16, i16), other: (i16, i16)) -> u16 {
    let heading = bearing_12bit_approx(target.1, target.0, attacker.1, attacker.0)
        .wrapping_add(0x800)
        & 0xFFF;
    let side =
        bearing_12bit_approx(other.1, other.0, attacker.1, attacker.0).wrapping_add(0x800) & 0xFFF;
    if side.wrapping_sub(heading) & 0xFFF < 0x800 {
        heading.wrapping_add(0x400) & 0xFFF
    } else {
        heading.wrapping_sub(0x400) & 0xFFF
    }
}

/// One tail step of a bystander at `pos` along `angle`: `(x + (sin * step >>
/// 9), z + (cos * step >> 9))`, each axis 16-bit wrapping.
pub fn make_way_offset(pos: (i16, i16), angle: u16, step: i16) -> (i16, i16) {
    let (sin, cos) = legaia_engine_system::trig::trig12(angle);
    let dx = (i32::from(sin) * i32::from(step)) >> MAKE_WAY_SHIFT;
    let dz = (i32::from(cos) * i32::from(step)) >> MAKE_WAY_SHIFT;
    (
        (i32::from(pos.0) + dx) as i16,
        (i32::from(pos.1) + dz) as i16,
    )
}

/// The engine slots the make-way walk visits: retail's seven pool slots
/// ([`BattleActionHost::in_retail_pool`]).
fn make_way_slots<H: BattleActionHost + ?Sized>(host: &H) -> impl Iterator<Item = u8> + '_ {
    (0..ACTOR_SLOTS as u8).filter(move |&s| host.in_retail_pool(s))
}

/// State `0x14`'s make-way arm: write each bystander's heading and seed the
/// countdown with the melee monster's size class - the target's when a party
/// member attacks, the attacker's own when a monster does (`sltiu v0,v0,0x3`
/// at `0x801E31A8` picks the record `0x801C9348[slot - 3]`).
pub(super) fn arm_make_way<H: BattleActionHost + ?Sized>(
    host: &H,
    ctx: &mut BattleActionCtx,
    attacker: u8,
    target: u8,
) {
    // A group code names no body to measure the line against (retail loads
    // the target pointer only for a table slot, `sltiu v0,t2,0x8` at
    // `0x801E29B8`).
    if usize::from(target) >= ACTOR_SLOTS {
        return;
    }
    let (Some(a), Some(t)) = (host.actor_position(attacker), host.actor_position(target)) else {
        return;
    };
    let others: Vec<u8> = make_way_slots(host)
        .filter(|&s| s != attacker && s != target)
        .collect();
    for s in others {
        if let Some(p) = host.actor_position(s) {
            ctx.dodge_angles[usize::from(s)] = make_way_angle(a, t, p);
        }
    }
    let melee_monster = if attacker < host.party_count() {
        target
    } else {
        attacker
    };
    ctx.dodge_countdown = i16::from(host.monster_size_class(melee_monster));
}

/// The SM tail's make-way step, run after every pass: while the countdown
/// is non-zero, step every seated slot other than `attacker` / `target`
/// along its heading, then drain the countdown by the frame step, floored at
/// zero (`bgez` / `sh zero` at `0x801E692C..0x801E6934`).
pub fn make_way_step<H: BattleActionHost + ?Sized>(
    host: &mut H,
    ctx: &mut BattleActionCtx,
    attacker: u8,
    target: u8,
) {
    if ctx.dodge_countdown == 0 {
        return;
    }
    let step = host.frame_dt();
    let slots: Vec<u8> = make_way_slots(host)
        .filter(|&s| s != attacker && s != target)
        .collect();
    for s in slots {
        if let Some(p) = host.actor_position(s) {
            let (x, z) = make_way_offset(p, ctx.dodge_angles[usize::from(s)], step);
            host.set_actor_position(s, x, z);
        }
    }
    ctx.dodge_countdown = ctx.dodge_countdown.wrapping_sub(step).max(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An attacker walking up the Z axis at a target: a bystander on its
    /// left (negative X) steps further left, one on its right steps right.
    #[test]
    fn bystanders_step_away_from_the_attack_line() {
        let attacker = (0, -800);
        let target = (0, 800);
        let left = make_way_angle(attacker, target, (-600, -775));
        let right = make_way_angle(attacker, target, (600, -775));
        let (lx, _) = make_way_offset((-600, -775), left, 64);
        let (rx, _) = make_way_offset((600, -775), right, 64);
        assert!(lx < -600, "left bystander moves left: {lx}");
        assert!(rx > 600, "right bystander moves right: {rx}");
    }

    /// A full countdown moves a bystander `size * 8` units along a heading
    /// at a whole quarter turn, whatever the frame step it drains by.
    #[test]
    fn a_countdown_moves_size_times_eight() {
        for step in [1i16, 2, 3] {
            let mut pos = (0i16, 0i16);
            let mut countdown = 36i16;
            while countdown != 0 {
                pos = make_way_offset(pos, 0x400, step);
                countdown = countdown.wrapping_sub(step).max(0);
            }
            assert!((pos.0 - 36 * 8).abs() <= 8, "step {step}: {pos:?}");
            assert_eq!(pos.1, 0);
        }
    }
}
