use super::super::import::Key;
use super::super::markup::Target;
use super::super::ui;
use super::report::Outcome;
use super::*;

#[test]
fn targets_follow_the_key_shape() {
    assert_eq!(target_for_key("man:10:0x40"), Target::Segment);
    assert_eq!(target_for_key("raw:10:0x40"), Target::Segment);
    assert_eq!(target_for_key("scus:str:0x80012260"), Target::CString);
    assert_eq!(target_for_key("ui:899:0x801ce81c"), Target::CString);
    assert_eq!(target_for_key("mon:3"), Target::CString);
}

#[test]
fn encoded_len_counts_tokens_as_their_bytes() {
    assert_eq!(encoded_len("Abc", Target::CString), Ok(3));
    assert_eq!(encoded_len("{c2:79}x", Target::Segment), Ok(3));
    let err = encoded_len("\u{00e9}t\u{00e9}", Target::Segment).unwrap_err();
    assert_eq!(err.len(), 2, "every bad character reported at once");
}

#[test]
fn outcomes_serialize_as_their_names() {
    for o in [
        Outcome::InPlace,
        Outcome::NoFreeRun,
        Outcome::AlreadyApplied,
    ] {
        let json = serde_json::to_string(&o).unwrap();
        assert_eq!(json, format!("\"{}\"", o.as_str()));
    }
    assert!(Outcome::Moved.lands() && !Outcome::RolledBack.lands());
}

#[test]
fn pool_index_finds_overlay_and_scus_windows() {
    let first = &ui::UI_STRING_POOLS[0];
    assert_eq!(
        pool_index(&Key::Ui {
            prot: first.prot_index,
            va: first.va_start
        }),
        Some(0)
    );
    let s0 = &ui::SCUS_STRING_POOLS[0];
    assert_eq!(
        pool_index(&Key::ScusStr { va: s0.va_start }),
        Some(ui::UI_STRING_POOLS.len())
    );
    assert_eq!(pool_index(&Key::ScusStr { va: 0x8000_0000 }), None);
}
