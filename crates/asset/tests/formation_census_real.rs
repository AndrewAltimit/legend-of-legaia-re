//! Disc-gated: the formation census over the real `PROT.DAT` reads every
//! scene MAN's encounter rows and classifies the roster the way the entity
//! SM does - a boss sits in a row whose header byte is non-zero, a
//! random-encounter native sits only in rows a `rate > 0` region rolls.
//! Skips and passes when `LEGAIA_DISC_BIN` / `extracted/` is absent.

use std::path::PathBuf;

use legaia_asset::formation_census::FormationCensus;
use legaia_prot::archive::Archive;

fn extracted_prot() -> Option<PathBuf> {
    for base in ["extracted", "../../extracted"] {
        let prot = PathBuf::from(base).join("PROT.DAT");
        if prot.is_file() {
            return Some(prot);
        }
    }
    None
}

fn census() -> Option<FormationCensus> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    let prot = extracted_prot()?;
    let mut archive = Archive::open(&prot).ok()?;
    let metas = archive.entries.clone();
    let mut entries: Vec<(usize, Vec<u8>)> = Vec::with_capacity(metas.len());
    let mut buf = Vec::new();
    for (i, meta) in metas.iter().enumerate() {
        buf.clear();
        if archive.read_entry(meta, &mut buf).is_ok() {
            entries.push((i, buf.clone()));
        }
    }
    Some(FormationCensus::from_entries(
        entries.iter().map(|(i, b)| (*i, b.as_slice())),
    ))
}

#[test]
fn census_separates_bosses_from_random_natives() {
    let Some(c) = census() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or extracted/PROT.DAT missing (disc-gated)");
        return;
    };
    // Every scene MAN with an encounter section contributes; the disc has
    // dozens of them and hundreds of rows.
    assert!(c.scenes >= 60, "scenes: {}", c.scenes);
    assert!(c.rows >= 300, "rows: {}", c.rows);

    // Gobu Gobu (id 4): a Rim Elm / world-map native - random rows only.
    let gobu = c.rows_for(4);
    assert!(gobu.random > 0, "{gobu:?}");
    assert_eq!(gobu.flagged, 0, "{gobu:?}");
    assert!(!gobu.default_scripted());

    // Caruban (id 73): rikuroa rows 16/17 carry the non-zero header
    // (`docs/formats/encounter.md`), so it is met scripted.
    let caruban = c.rows_for(73);
    assert!(caruban.flagged > 0, "{caruban:?}");
    assert!(caruban.met_scripted());

    // Gaza (ids 165/166): the boss captures carry `ctx[+0x287] == 4`, so the
    // rows that name him must be flagged, never region-rollable.
    for id in [165u8, 166] {
        let g = c.rows_for(id);
        assert_eq!(g.random, 0, "Gaza {id}: {g:?}");
        assert!(g.met_scripted(), "Gaza {id}: {g:?}");
    }

    // No row ever names a random-rollable formation with a non-zero header:
    // the two classes are disjoint on the retail disc.
    for (id, r) in &c.monsters {
        assert!(r.random <= r.clear, "id {id}: {r:?}");
    }
}
