//! A captured cast's live parts, handed to the image child as
//! `LEGAIA_SEAT_CAST_PARTS` and seated over the replay's own parts of the same
//! records on the capture frame
//! ([`SummonScene::install_part_snapshot`](legaia_engine_effects::summon::SummonScene::install_part_snapshot)).
//!
//! Format: one part per `;`-separated entry, `tag:off=value,off=value,...`
//! with every number hex.

use legaia_engine_effects::summon::CastPartSeed;

/// `seeds` as `LEGAIA_SEAT_CAST_PARTS`.
pub fn cast_parts_env(seeds: &[CastPartSeed]) -> String {
    seeds
        .iter()
        .map(|s| {
            let pokes: Vec<String> = s
                .pokes
                .iter()
                .map(|(o, v)| format!("{o:x}={v:x}"))
                .collect();
            format!("{:x}:{}", s.tag, pokes.join(","))
        })
        .collect::<Vec<_>>()
        .join(";")
}

/// Inverse of [`cast_parts_env`]; malformed entries and pairs are dropped.
pub fn cast_parts_from_env(s: &str) -> Vec<CastPartSeed> {
    s.split(';')
        .filter_map(|e| {
            let (tag, rest) = e.split_once(':')?;
            let tag = u32::from_str_radix(tag.trim(), 16).ok()?;
            let pokes = rest
                .split(',')
                .filter_map(|p| {
                    let (o, v) = p.split_once('=')?;
                    Some((
                        u16::from_str_radix(o.trim(), 16).ok()?,
                        u16::from_str_radix(v.trim(), 16).ok()?,
                    ))
                })
                .collect();
            Some(CastPartSeed { tag, pokes })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cast_parts_round_trip_through_the_environment() {
        let seeds = vec![
            CastPartSeed {
                tag: 0x801F_876C,
                pokes: vec![(0x28, 0x2CC), (0xB6, 0x80), (0xC2, 0xFF00)],
            },
            CastPartSeed {
                tag: 0x801F_86B8,
                pokes: vec![(0x14, 0)],
            },
        ];
        assert_eq!(cast_parts_from_env(&cast_parts_env(&seeds)), seeds);
        assert!(cast_parts_from_env("zz:1=2").is_empty());
    }
}
