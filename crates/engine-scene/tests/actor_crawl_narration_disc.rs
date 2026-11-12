//! Disc-gated: a narration crawl opened on an actor's context.
//!
//! The field VM's `4C 80` arm (`0x801E1ECC..0x801E1FBC` in `FUN_801DE840`)
//! halt-acquires whichever context it runs on and allocates the roller child
//! over the pages that follow; it reads its target for nothing else. The
//! opening's crawls run on the cutscene context (`CC F8 80 N`); East Voz
//! Forest's Ozma hatching (`vozz` `P2[17]`) opens its crawl on actor `0x06`
//! (`CC 06 80 17`), one of whose pages carries a `C1 02` name escape, and
//! waits on that actor's halt bit (`B3 06 0A`) until the roller retires.
//! A retail recording of the beat holds the record on that test for the
//! whole crawl.
//!
//! The test pins the block's shape (target, page count, the escape, the
//! wait after it) and lists every crawl the disc carries on a target other
//! than `0xF8`, without baking any page text into the source. Skip-passes
//! without disc data / extracted assets (CLAUDE.md convention).

use legaia_engine_field::man_field_scripts::partition_record_span;
use legaia_engine_scene::man_census::scene_man_carriers;
use legaia_engine_scene::scene::{ProtIndex, Scene};
use legaia_scene_formats::cutscene_text::{NarrationKind, parse_narration};
use std::path::PathBuf;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

#[test]
fn vozz_ozma_crawl_runs_on_actor_six() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return;
    };
    let index = ProtIndex::open_extracted(&extracted).expect("open ProtIndex");
    let scene = Scene::load(&index, "vozz").expect("load vozz");
    let man = scene
        .field_man_payload(&index)
        .expect("man payload fetch")
        .expect("vozz has a MAN payload");
    let man_file = legaia_scene_formats::man_section::parse(&man).expect("man parse");
    let (start, _pc0, len) = partition_record_span(&man_file, &man, 2, 17).expect("P2[17] span");
    let body = &man[start..start + len];

    let crawls: Vec<_> = parse_narration(body)
        .into_iter()
        .filter(|b| b.kind == NarrationKind::Crawl)
        .collect();
    assert_eq!(crawls.len(), 1, "P2[17] carries one crawl");
    let b = &crawls[0];
    assert_eq!(b.target, 0x06, "the crawl runs on actor 0x06");
    assert_eq!(b.declared_pages, 23);
    assert!(b.count_matches(), "every page frames, the escape page too");
    assert!(
        b.pages.iter().any(|p| p.raw.starts_with(&[0xC1, 0x02])),
        "one page opens on the C1 02 name escape"
    );
    // The record waits for the roller on the crawl target's halt bit.
    let (_, end) = b.byte_span();
    assert!(
        body[end..].windows(3).any(|w| w == [0xB3, 0x06, 0x0A]),
        "B3 06 0A follows the crawl"
    );

    // Every crawl the disc opens on another context than 0xF8.
    let mut others = Vec::new();
    for name in index.cdname_scene_names() {
        let Ok(scene) = Scene::load(&index, &name) else {
            continue;
        };
        for carrier in scene_man_carriers(&index, &scene) {
            let man = &carrier.payload;
            let Ok(mf) = legaia_scene_formats::man_section::parse(man) else {
                continue;
            };
            let n2 = mf.header.partition_counts[2].max(0) as usize;
            for r in 0..n2 {
                let Some((s, _, l)) = partition_record_span(&mf, man, 2, r) else {
                    continue;
                };
                let Some(body) = man.get(s..s + l) else {
                    continue;
                };
                for b in parse_narration(body) {
                    if b.kind == NarrationKind::Crawl && b.target != 0xF8 {
                        assert!(b.count_matches(), "{name} P2[{r}] crawl frames cleanly");
                        others.push(format!("{name} P2[{r}] target 0x{:02X}", b.target));
                    }
                }
            }
        }
    }
    others.sort();
    others.dedup();
    eprintln!("crawls on a target other than 0xF8: {others:?}");
    assert!(others.iter().any(|s| s.starts_with("vozz P2[17]")));
}
