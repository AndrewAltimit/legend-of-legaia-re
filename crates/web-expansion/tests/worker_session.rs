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
