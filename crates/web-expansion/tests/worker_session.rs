//! The page's validation worker runs a second session built from the
//! first one's encodings (`take_disc_parts`, `session_files`): it must
//! validate exactly as the first does, disc half included.

use std::path::Path;

use legaia_expansion_format::read_dir_files;
use legaia_web_expansion::session::Core;

fn rebirth(core: &mut Core) -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/rebirth");
    let loaded = core.load_files(read_dir_files(&root).unwrap());
    loaded["pack"].to_string()
}

#[test]
fn session_files_carry_validation_to_a_second_session() {
    let mut main = Core::new();
    let json = rebirth(&mut main);
    // An edit that changes one file and breaks a reference.
    let mut pack: serde_json::Value = serde_json::from_str(&json).unwrap();
    pack["battle"]["monsters"]["ironhide_ogre"]["hp"] = serde_json::json!(5);
    let edited = pack.to_string();

    let mut worker = Core::new();
    worker.set_session_files(&main.session_files()).unwrap();
    assert_eq!(worker.validate(&json), main.validate(&json));
    assert_eq!(worker.validate(&edited), main.validate(&edited));
    // The worker exports the same tree, comments and all.
    assert_eq!(
        worker.pack_files(&json).unwrap(),
        main.pack_files(&json).unwrap()
    );
    // Truncated data is refused, not half-applied.
    let data = main.session_files();
    assert!(
        Core::new()
            .set_session_files(&data[..data.len() - 3])
            .is_err()
    );
}

#[test]
fn disc_parts_open_the_same_catalog() {
    let Some(path) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("LEGAIA_DISC_BIN unset; skipping");
        return;
    };
    let Ok(image) = std::fs::read(&path) else {
        eprintln!("LEGAIA_DISC_BIN unreadable; skipping");
        return;
    };
    let mut main = Core::new();
    main.open_disc_owned(image, true).unwrap();
    let parts = main.take_disc_parts().expect("parts kept");
    assert!(main.take_disc_parts().is_none(), "handed over once");
    let mut worker = Core::new();
    worker.open_disc_parts(&parts).unwrap();
    assert_eq!(worker.catalog_json(), main.catalog_json());

    let json = rebirth(&mut main);
    worker.set_session_files(&main.session_files()).unwrap();
    // A coordinate the disc refuses shows up in both.
    let mut pack: serde_json::Value = serde_json::from_str(&json).unwrap();
    pack["battle"]["monsters"]["ironhide_ogre"]["base"] =
        serde_json::json!({ "kind": "monster", "id": 250 });
    let edited = pack.to_string();
    for j in [&json, &edited] {
        assert_eq!(worker.validate(j), main.validate(j));
    }
    assert_ne!(main.validate(&edited), main.validate(&json));
}

#[test]
fn an_unchecked_load_is_the_checked_one_without_its_issues() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/rebirth");
    let files = read_dir_files(&root).unwrap();
    let mut checked = Core::new();
    for (p, b) in &files {
        checked.add_file(p, b.clone());
    }
    let a = checked.finish_files();
    let mut quick = Core::new();
    for (p, b) in &files {
        quick.add_file(p, b.clone());
    }
    let b = quick.finish_files_unchecked();
    assert_eq!(a["pack"], b["pack"]);
    assert_eq!(a["files"], b["files"]);
    assert_eq!(b["issues"], serde_json::json!([]));
    // The session it leaves validates (and exports) as the checked one.
    let json = a["pack"].to_string();
    assert_eq!(quick.validate(&json), checked.validate(&json));
    assert_eq!(quick.session_files(), checked.session_files());

    // A tree that does not load still says why.
    let mut broken = Core::new();
    broken.add_file("pack.toml", b"not = [toml".to_vec());
    let r = broken.finish_files_unchecked();
    assert!(r["pack"].is_null());
    assert!(!r["issues"].as_array().unwrap().is_empty());
}
