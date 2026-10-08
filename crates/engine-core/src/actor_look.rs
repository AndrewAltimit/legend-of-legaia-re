//! The per-object **look rotation** a field script gives an animated actor:
//! field-VM op `4C 45 <object> <x> <y> <z> <ticks>`.
//!
//! Retail keeps it in the actor's side buffer (`actor[+0x44]`): the object
//! index at `+0x9A` (the allocator seeds `-1`, `FUN_801D7518`) and three
//! angles at `+0x94` / `+0x96` / `+0x98`. The op stores the object byte at
//! once (`sh v0,0x9a(v1)`, `0x801E12C8` / `0x801E1304` in PROT 0897) and
//! each angle either at once (`ticks == 0`, `0x801E12CC..0x801E12F4`) or as
//! one `FUN_8003C5F0` ramp from the live value per angle that changes
//! (`0x801E1308..0x801E138C`).
//!
//! The animated renderer `FUN_8001B964` reads it per object: on the object
//! whose index equals `+0x9A` it turns the GTE matrix by `RotZ(+0x98)`,
//! `RotY(+0x96)`, `RotX(+0x94)` (`0x8001BB40..0x8001BB88`) before the
//! keyframe's own `RotZ(+0xC)`, `RotY(+0xA)`, `RotX(+0x8)`
//! (`0x8001BB8C..0x8001BBE8`). Each helper post-multiplies (`FUN_8004638C`
//! rebuilds the matrix's columns as `M * column(Rz)`), so the object draws
//! with `R_look * R_key` about the keyframe's own pivot - the keyframe
//! translation is already in `TR`. Object `0` of a field character rig is
//! its head: a cutscene uses the op to tilt or turn a head toward whoever
//! speaks (`cort_evolved_pre_battle` holds Vahn's at `X = -412`, looking up
//! at the flesh wall; the casino counter clerk turns hers by `Y = -512`).
//! The static bracket `FUN_8001ADA4` reads none of it, so only a posed actor
//! turns.
//!
//! [`apply_look`] folds the rotation into a pose's bone list - the shape
//! both hosts already pose from - so neither needs a second matrix path.

use legaia_engine_vm::ambient_motion::{Ramp, RampKind, RampScheduler};

/// One actor's side-buffer look state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ActorLook {
    /// `+0x9A`: the object the rotation turns (`-1` = none).
    pub object: i16,
    /// `+0x94` / `+0x96` / `+0x98`: the X / Y / Z angles, `4096` a turn.
    pub angles: [i16; 3],
}

impl ActorLook {
    /// The allocator's seed: no object, zero angles.
    pub const NONE: Self = Self {
        object: -1,
        angles: [0; 3],
    };

    /// `true` when the renderer turns an object: `+0x9A` names one and an
    /// angle is non-zero.
    pub fn turns(&self) -> bool {
        self.object >= 0 && self.angles != [0; 3]
    }
}

/// Which actor a look belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LookKey {
    /// The player actor (`CC F8 45 ..`, or the op run on the player's own
    /// stand-in context).
    Player,
    /// A placed NPC, by placement slot.
    Npc(u8),
}

impl LookKey {
    fn owner(self) -> u32 {
        match self {
            Self::Player => 0x100,
            Self::Npc(s) => u32::from(s),
        }
    }

    fn from_owner(owner: u32) -> Self {
        if owner == 0x100 {
            Self::Player
        } else {
            Self::Npc(owner as u8)
        }
    }
}

/// Every actor's look plus the ramps driving them - the port of the side
/// buffer's three angles and the `FUN_8003C5F0` jobs the op schedules on
/// them.
#[derive(Debug, Clone, Default)]
pub struct ActorLooks {
    looks: std::collections::BTreeMap<LookKey, ActorLook>,
    ramps: Option<RampScheduler>,
}

/// The ramp destination tags: retail's side-buffer offsets.
const DEST: [u32; 3] = [0x94, 0x96, 0x98];

impl ActorLooks {
    /// A scene load: every side buffer is fresh (`+0x9A = -1`) and the ramp
    /// pool is reset (`FUN_8003CDA8`).
    pub fn clear(&mut self) {
        self.looks.clear();
        self.ramps = None;
    }

    /// The live look of `key` (the allocator seed when the op never ran).
    pub fn get(&self, key: LookKey) -> ActorLook {
        self.looks.get(&key).copied().unwrap_or(ActorLook::NONE)
    }

    /// The look `key` draws with this frame, or `None` when it turns
    /// nothing.
    pub fn turning(&self, key: LookKey) -> Option<ActorLook> {
        Some(self.get(key)).filter(ActorLook::turns)
    }

    /// `4C 45` with `ticks == 0`: store the object and the three angles.
    pub fn write(&mut self, key: LookKey, object: u8, angles: [i16; 3]) {
        if let Some(r) = self.ramps.as_mut() {
            r.free_owner(key.owner());
        }
        self.looks.insert(
            key,
            ActorLook {
                object: i16::from(object),
                angles,
            },
        );
    }

    /// `4C 45` with `ticks != 0`: store the object now and ramp each angle
    /// that differs from its live value over `ticks` frames.
    pub fn ramp(&mut self, key: LookKey, object: u8, angles: [i16; 3], ticks: u16) {
        let live = self.get(key);
        self.looks.insert(
            key,
            ActorLook {
                object: i16::from(object),
                angles: live.angles,
            },
        );
        let pool = self.ramps.get_or_insert_with(RampScheduler::new);
        for (i, (&to, &from)) in angles.iter().zip(&live.angles).enumerate() {
            if to == from {
                continue;
            }
            let total = i32::from(ticks);
            pool.install(Ramp {
                dest: DEST[i],
                owner: key.owner(),
                start: i32::from(from),
                end: i32::from(to),
                total,
                remaining: total,
                kind: RampKind::U16,
            });
        }
    }

    /// One frame of the ramp pool at frame step `speed`
    /// (`FUN_80036D80`'s halfword store).
    pub fn tick(&mut self, speed: u8) {
        let Some(pool) = self.ramps.as_mut() else {
            return;
        };
        for w in pool.tick(speed) {
            let key = LookKey::from_owner(w.owner);
            let Some(axis) = DEST.iter().position(|&d| d == w.dest) else {
                continue;
            };
            let look = self.looks.entry(key).or_insert(ActorLook::NONE);
            look.angles[axis] = w.value as i16;
        }
        if pool.active() == 0 {
            self.ramps = None;
        }
    }
}

/// Fold `look` into a pose's bone list: the bone `look.object` takes the
/// rotation `R_look * R_key` (both `Rz * Ry * Rx` in retail's angle units),
/// re-expressed as the `Rz * Ry * Rx` Euler triple every posing kernel
/// reads. Its translation is untouched - the turn is about the keyframe's
/// own pivot. A look naming no bone of the pose changes nothing.
pub fn apply_look(bones: &mut [([i16; 3], [i16; 3])], look: ActorLook) {
    if !look.turns() {
        return;
    }
    let Some((_, rot)) = usize::try_from(look.object)
        .ok()
        .and_then(|i| bones.get_mut(i))
    else {
        return;
    };
    let m = mat_mul(&euler_zyx(look.angles), &euler_zyx(*rot));
    *rot = zyx_from_matrix(&m);
}

type Mat3 = [[f64; 3]; 3];

fn angle(a: i16) -> f64 {
    f64::from(a) * std::f64::consts::TAU / 4096.0
}

/// `Rz(z) * Ry(y) * Rx(x)` for retail's `[x, y, z]` angle triple, in the
/// rotation sense the posing kernels use (`legaia_tmd`'s `rot_zyx`).
fn euler_zyx(a: [i16; 3]) -> Mat3 {
    let (sx, cx) = angle(a[0]).sin_cos();
    let (sy, cy) = angle(a[1]).sin_cos();
    let (sz, cz) = angle(a[2]).sin_cos();
    let rx = [[1.0, 0.0, 0.0], [0.0, cx, -sx], [0.0, sx, cx]];
    let ry = [[cy, 0.0, sy], [0.0, 1.0, 0.0], [-sy, 0.0, cy]];
    let rz = [[cz, -sz, 0.0], [sz, cz, 0.0], [0.0, 0.0, 1.0]];
    mat_mul(&rz, &mat_mul(&ry, &rx))
}

fn mat_mul(a: &Mat3, b: &Mat3) -> Mat3 {
    let mut m = [[0.0; 3]; 3];
    for (i, row) in m.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            *cell = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    m
}

/// The `[x, y, z]` triple whose `Rz * Ry * Rx` is `m`, in retail angle
/// units (the `y = +-90` degree gimbal case keeps `x = 0`).
fn zyx_from_matrix(m: &Mat3) -> [i16; 3] {
    let to_units = |r: f64| -> i16 {
        let u = (r * 4096.0 / std::f64::consts::TAU).round() as i32;
        u.rem_euclid(4096) as i16
    };
    let sy = (-m[2][0]).clamp(-1.0, 1.0);
    let y = sy.asin();
    let (x, z) = if sy.abs() < 1.0 - 1e-9 {
        (m[2][1].atan2(m[2][2]), m[1][0].atan2(m[0][0]))
    } else {
        (0.0, (-m[0][1]).atan2(m[1][1]))
    };
    [to_units(x), to_units(y), to_units(z)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [i16; 3], b: [i16; 3]) -> bool {
        a.iter().zip(&b).all(|(&p, &q)| {
            (i32::from(p) - i32::from(q))
                .rem_euclid(4096)
                .min((i32::from(q) - i32::from(p)).rem_euclid(4096))
                <= 1
        })
    }

    /// A single-axis look on an unrotated key is that angle; on another
    /// object it changes nothing.
    #[test]
    fn a_look_turns_its_object_only() {
        let mut bones = vec![([0, 0, 0], [0, 0, 0]), ([10, 20, 30], [0, 100, 0])];
        apply_look(
            &mut bones,
            ActorLook {
                object: 0,
                angles: [-412, 0, 0],
            },
        );
        assert!(close(bones[0].1, [4096 - 412, 0, 0]));
        assert_eq!(bones[1], ([10, 20, 30], [0, 100, 0]));
    }

    /// The fold is the matrix product `R_look * R_key`: a yaw look over a
    /// yawed key adds the yaws, and the round trip of a general triple
    /// reproduces the same rotation.
    #[test]
    fn a_look_composes_in_front_of_the_key() {
        let mut bones = vec![([0, 0, 0], [0, 300, 0])];
        apply_look(
            &mut bones,
            ActorLook {
                object: 0,
                angles: [0, -512, 0],
            },
        );
        assert!(close(bones[0].1, [0, 4096 - 212, 0]), "{:?}", bones[0].1);
        let key = [100, -200, 350];
        let look = [-120, 200, 40];
        let mut b = vec![([0, 0, 0], key)];
        apply_look(
            &mut b,
            ActorLook {
                object: 0,
                angles: look,
            },
        );
        let want = mat_mul(&euler_zyx(look), &euler_zyx(key));
        let got = euler_zyx(b[0].1);
        for i in 0..3 {
            for j in 0..3 {
                assert!((want[i][j] - got[i][j]).abs() < 3e-3);
            }
        }
    }

    /// The ramp path stores the object at once, ramps only the angles that
    /// change, and lands on the target.
    #[test]
    fn ramps_land_on_their_targets() {
        let mut l = ActorLooks::default();
        l.write(LookKey::Npc(3), 0, [0, 200, 0]);
        l.ramp(LookKey::Npc(3), 0, [-120, 200, 0], 10);
        assert_eq!(l.get(LookKey::Npc(3)).angles, [0, 200, 0]);
        l.tick(5);
        let mid = l.get(LookKey::Npc(3)).angles;
        assert!(mid[0] < 0 && mid[0] > -120 && mid[1] == 200);
        l.tick(5);
        assert_eq!(l.get(LookKey::Npc(3)).angles, [-120, 200, 0]);
        assert_eq!(l.turning(LookKey::Player), None);
    }
}
