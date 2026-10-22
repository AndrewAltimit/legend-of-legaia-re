//! A captured state's live effect-pool children, handed to the image child as
//! `LEGAIA_SEAT_EFFECT_CHILDREN` and stood in the engine's pool on the
//! capture frame
//! ([`World::install_effect_child_snapshot`](legaia_engine_core::world::World::install_effect_child_snapshot)).
//!
//! Format: one child per `;`-separated entry,
//! `anim:count:cursor:wait:mirror:x:y:z:vx:vy:vz`, every number decimal.

use legaia_engine_core::world::EffectChildSeed;

/// `seeds` as `LEGAIA_SEAT_EFFECT_CHILDREN`.
pub fn effect_children_env(seeds: &[EffectChildSeed]) -> String {
    seeds
        .iter()
        .map(|c| {
            format!(
                "{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}",
                c.anim_id,
                c.frame_count,
                c.frame_cursor,
                c.wait,
                c.mirror,
                c.pos[0],
                c.pos[1],
                c.pos[2],
                c.velocity[0],
                c.velocity[1],
                c.velocity[2]
            )
        })
        .collect::<Vec<_>>()
        .join(";")
}

/// Inverse of [`effect_children_env`]; malformed entries are dropped.
pub fn effect_children_from_env(s: &str) -> Vec<EffectChildSeed> {
    s.split(';')
        .filter_map(|e| {
            let f: Vec<&str> = e.trim().split(':').collect();
            if f.len() != 11 {
                return None;
            }
            let i = |k: usize| f[k].parse::<i64>().ok();
            Some(EffectChildSeed {
                anim_id: u16::try_from(i(0)?).ok()?,
                frame_count: u8::try_from(i(1)?).ok()?,
                frame_cursor: u8::try_from(i(2)?).ok()?,
                wait: u8::try_from(i(3)?).ok()?,
                mirror: u8::try_from(i(4)?).ok()?,
                pos: [
                    i32::try_from(i(5)?).ok()?,
                    i32::try_from(i(6)?).ok()?,
                    i32::try_from(i(7)?).ok()?,
                ],
                velocity: [
                    i16::try_from(i(8)?).ok()?,
                    i16::try_from(i(9)?).ok()?,
                    i16::try_from(i(10)?).ok()?,
                ],
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effect_children_round_trip_through_the_environment() {
        let seeds = vec![
            EffectChildSeed {
                anim_id: 6,
                frame_count: 8,
                frame_cursor: 3,
                wait: 8,
                mirror: 3,
                pos: [85 << 8, -212 << 8, -1193 << 8],
                velocity: [0, -4, 0],
            },
            EffectChildSeed {
                anim_id: 1,
                frame_count: 16,
                ..Default::default()
            },
        ];
        assert_eq!(
            effect_children_from_env(&effect_children_env(&seeds)),
            seeds
        );
        assert!(effect_children_from_env("1:2:3").is_empty());
        assert!(effect_children_from_env("").is_empty());
    }
}
