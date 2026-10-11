//! The arts-voice and move modes: shout banks, the hybrid and Delilas kits,
//! labels under a rearranged mapping.

use super::load_disc;
use legaia_art::queue::Character;
use legaia_delilas_party::delilas_party::{
    CastRoutePolicy, DelilasMoveMode as MoveMode, PartyMapping, Sibling, apply_delilas_party,
};
use legaia_delilas_party::delilas_voice_fx::ArtsVoiceMode;
use legaia_disc_patch::disc::DiscPatcher;

/// The three `--delilas-arts-voice` modes against the retail shout
/// banks: `original` leaves XA2 byte-identical, `removed` leaves every
/// channel silent, `adjusted` (default) writes processed audio that is
/// voiced, differs from retail, and is byte-deterministic.
#[test]
pub(super) fn arts_voice_modes_shape_the_shout_banks() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let retail = DiscPatcher::open(original.clone()).expect("open retail");
    let mapping = PartyMapping::default();

    // pick a retail-voiced channel of Vahn's shout bank as the probe
    let chans = retail.xa_channels("XA/XA2.XA").expect("xa2 channels");
    let probe = *chans
        .iter()
        .find(|&&c| {
            legaia_delilas_party::delilas_xa_voice::read_xa_channel_pcm(&retail, "XA/XA2.XA", c)
                .map(|(pcm, _)| pcm.iter().any(|&s| s.unsigned_abs() > 1000))
                .unwrap_or(false)
        })
        .expect("XA2 has a voiced channel");
    let (retail_pcm, _) =
        legaia_delilas_party::delilas_xa_voice::read_xa_channel_pcm(&retail, "XA/XA2.XA", probe)
            .expect("read");

    let run = |mode: ArtsVoiceMode| -> DiscPatcher {
        let mut p = DiscPatcher::open(original.clone()).expect("open scratch");
        let rep = apply_delilas_party(
            &mut p,
            &mapping,
            mode,
            MoveMode::default(),
            CastRoutePolicy::Install,
        )
        .expect("apply");
        assert!(rep.changed, "{mode}: apply reported no change");
        p
    };

    // original: the shout bank survives byte-identical
    let p_orig = run(ArtsVoiceMode::Original);
    let (pcm, _) =
        legaia_delilas_party::delilas_xa_voice::read_xa_channel_pcm(&p_orig, "XA/XA2.XA", probe)
            .expect("read");
    assert_eq!(pcm, retail_pcm, "original mode must not touch XA2");

    // removed: every channel decodes to silence
    let p_rm = run(ArtsVoiceMode::Removed);
    for &c in &chans {
        if let Ok((pcm, _)) =
            legaia_delilas_party::delilas_xa_voice::read_xa_channel_pcm(&p_rm, "XA/XA2.XA", c)
        {
            assert!(
                pcm.iter().all(|&s| s.unsigned_abs() <= 24),
                "removed mode left channel {c} audible"
            );
        }
    }

    // adjusted: voiced, different from retail, deterministic
    let p_adj = run(ArtsVoiceMode::Adjusted);
    let (adj, _) =
        legaia_delilas_party::delilas_xa_voice::read_xa_channel_pcm(&p_adj, "XA/XA2.XA", probe)
            .expect("read");
    assert!(
        adj.iter().any(|&s| s.unsigned_abs() > 1000),
        "adjusted mode silenced the probe channel"
    );
    assert_ne!(adj, retail_pcm, "adjusted mode left retail audio untouched");
    let p_adj2 = run(ArtsVoiceMode::Adjusted);
    let (adj2, _) =
        legaia_delilas_party::delilas_xa_voice::read_xa_channel_pcm(&p_adj2, "XA/XA2.XA", probe)
            .expect("read");
    assert_eq!(adj, adj2, "adjusted mode must be byte-deterministic");

    // The Super / Hyper / Miracle FANFARE bank. A Hyper Art fires no
    // shout from the pool above, so this cue bed is its only audio -
    // and the bank once took a quarter-second grunt over a 3-7 second
    // channel, which is silence for 90%+ of every cue. Coverage, not
    // mere "differs from retail", is what catches that: a lossy
    // re-encode of a grunt also differs.
    let fan_chans = retail.xa_channels("XA/XA1.XA").expect("xa1 channels");
    for &c in fan_chans.iter().take(8) {
        let Ok((r, _)) =
            legaia_delilas_party::delilas_xa_voice::read_xa_channel_pcm(&retail, "XA/XA1.XA", c)
        else {
            continue;
        };
        if !r.iter().any(|&s| s.unsigned_abs() > 1000) {
            continue; // never voiced in retail
        }
        let (a, _) =
            legaia_delilas_party::delilas_xa_voice::read_xa_channel_pcm(&p_adj, "XA/XA1.XA", c)
                .expect("read patched fanfare");
        let audible = |p: &[i16]| p.iter().filter(|s| s.unsigned_abs() > 200).count();
        // The signature special's channel pair plus the GENERIC
        // Super-chain channel (`0x101` -> channel 1, fired by a special
        // performed at the end of a chain) are SILENCED in every mode:
        // the special's audio is the cast bed the module-open preempt
        // fires, and the preempt restarts whatever XA stream is live -
        // so any content here replays over the bed's own opening
        // (heard as "her intro repeats" once the bed remaster made the
        // head loud). Asserted both ways so neither half can rot: the
        // special's channels must be digitally silent, the rest of the
        // bank must keep its coverage.
        let sig = legaia_delilas_party::delilas_party::signature_fanfare_channels(0)
            .expect("Vahn's slot has a signature fanfare pair");
        let generic = legaia_art::hyper_fanfare::GENERIC_FANFARE_CHANNEL;
        let (o, _) =
            legaia_delilas_party::delilas_xa_voice::read_xa_channel_pcm(&p_orig, "XA/XA1.XA", c)
                .expect("read original fanfare");
        if c == sig.0 || c == sig.1 || c == generic {
            for (mode, pcm) in [("adjusted", &a), ("original", &o)] {
                assert!(
                    pcm.iter().all(|&s| s == 0),
                    "{mode} mode left the signature special's channel {c} audible - \
                     a pre-cast fanfare fire would replay the bed's head"
                );
            }
        } else {
            let (ra, aa) = (audible(&r), audible(&a));
            assert!(
                aa * 5 >= ra * 4,
                "adjusted fanfare channel {c} covers {aa} of retail's {ra} audible samples \
                 - a Hyper Art would play near-silence"
            );
            assert_eq!(
                o, r,
                "original mode must not touch the rest of the fanfare bank (channel {c})"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// `--delilas-moves`: the two move modes.
// ---------------------------------------------------------------------------

/// Bank row 11 - the Miracle Art's record, and the row the arts matcher
/// starts every scan at.
const MIRACLE_ROW: usize = 0x0B;

fn art_bank(
    patcher: &DiscPatcher,
    ch: Character,
) -> Vec<legaia_actor_formats::battle_char_assembly::ArtAnimRecord> {
    let entry = patcher
        .read_entry(legaia_arts_patch::arts::player_entry_index(ch))
        .expect("player file");
    let rec0 = legaia_actor_formats::battle_char_assembly::decode_record0(&entry).expect("record0");
    legaia_actor_formats::battle_char_assembly::art_animation_bank(&rec0).expect("art bank")
}

fn me_slot(patcher: &DiscPatcher, slot: usize) -> Vec<u8> {
    use legaia_delilas_party::delilas_party::READEF_ENTRY;
    let readef = patcher
        .read_entry_footprint(READEF_ENTRY)
        .expect("readef.DAT");
    let off = legaia_actor_formats::battle_char_assembly::art_me_slot(slot, false)
        * legaia_party_swap::winpose::READEF_SLOT;
    readef[off..off + legaia_party_swap::winpose::READEF_SLOT].to_vec()
}

fn run_mode(original: &[u8], moves: MoveMode) -> DiscPatcher {
    run_mapped(original, moves, PartyMapping::default()).0
}

pub(super) fn run_mapped(
    original: &[u8],
    moves: MoveMode,
    mapping: PartyMapping,
) -> (DiscPatcher, Vec<String>) {
    let mut p = DiscPatcher::open(original.to_vec()).expect("open");
    let rep = apply_delilas_party(
        &mut p,
        &mapping,
        ArtsVoiceMode::default(),
        moves,
        CastRoutePolicy::Install,
    )
    .expect("apply");
    assert!(rep.changed);
    (p, rep.notes)
}

/// The character slots in `(party slot, character)` order.
pub(super) const SLOTS: [(usize, Character); 3] = [
    (0, Character::Vahn),
    (1, Character::Noa),
    (2, Character::Gala),
];

/// Hybrid must leave every coordinate the Delilas pass owns exactly where
/// retail put it - the whole point of the default being a no-change.
///
/// Proved by contrast against the *retail* disc rather than by a stored
/// hash: for every art record other than the one the signature reskin
/// claims, the stream index, the impact class, the inline name and the
/// combo must all still read retail, and every stream body of the art
/// archive but the reskinned one must be byte-identical.
#[test]
fn hybrid_leaves_the_whole_kit_where_retail_put_it() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let retail = DiscPatcher::open(original.clone()).expect("open");
    let hybrid = run_mode(&original, MoveMode::Hybrid);

    for (slot, ch) in SLOTS {
        let before = art_bank(&retail, ch);
        let after = art_bank(&hybrid, ch);
        assert_eq!(before.len(), after.len(), "{ch:?}: bank length");
        // Exactly one record may differ, and only in the fields the
        // signature reskin declares.
        let changed: Vec<usize> = (0..before.len())
            .filter(|&i| {
                before[i].stream_source != after[i].stream_source
                    || before[i].name != after[i].name
                    || before[i].combo != after[i].combo
            })
            .collect();
        assert_eq!(
            changed.len(),
            1,
            "{ch:?}: hybrid moved {} art record(s); only the signature host may move",
            changed.len()
        );
        let host = changed[0];
        assert_ne!(host, MIRACLE_ROW, "{ch:?}: the Miracle row must not move");
        // The signature keeps the stream it already read - hybrid never
        // repoints.
        assert_eq!(
            before[host].stream_source, after[host].stream_source,
            "{ch:?}: hybrid must not repoint any art stream"
        );
        for i in 0..before.len() {
            if i == host {
                continue;
            }
            assert_eq!(
                before[i].impact_class, after[i].impact_class,
                "{ch:?} row {i}: hybrid must not clear an impact class"
            );
            assert_eq!(
                before[i].rate, after[i].rate,
                "{ch:?} row {i}: hybrid must not re-time an art"
            );
        }

        // The stream archive: same entry count, and every body but the
        // reskinned one byte-identical.
        let a = me_slot(&retail, slot);
        let b = me_slot(&hybrid, slot);
        let ar = legaia_actor_formats::me_archive::parse(&a).expect("retail ME");
        let br = legaia_actor_formats::me_archive::parse(&b).expect("hybrid ME");
        assert_eq!(
            ar.len(),
            br.len(),
            "{ch:?}: hybrid must not resize the archive"
        );
        let sig = after[host].stream_source as usize;
        for i in 0..ar.len() {
            if i == sig {
                continue;
            }
            assert_eq!(
                ar.raw_body(i),
                br.raw_body(i),
                "{ch:?}: hybrid rewrote art stream {i}"
            );
        }
    }
}

/// Delilas mode: every art plays a sibling clip, the Super and Miracle
/// components keep working combos, and what it hides is provably
/// unreachable rather than merely absent.
#[test]
fn delilas_mode_reanimates_the_kit_and_keeps_the_supers_reachable() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let retail = DiscPatcher::open(original.clone()).expect("open");
    let del = run_mode(&original, MoveMode::Delilas);
    let mapping = PartyMapping::default();
    let siblings = [mapping.vahn, mapping.noa, mapping.gala];

    for (slot, ch) in SLOTS {
        let before = art_bank(&retail, ch);
        let after = art_bank(&del, ch);

        // Baseline contrast: retail spreads its arts over many streams,
        // and the rebuilt archive is a handful.
        let retail_streams: std::collections::BTreeSet<u8> = before
            .iter()
            .filter(|r| !r.uses_base_archive())
            .map(|r| r.stream_source)
            .collect();
        let new_streams: std::collections::BTreeSet<u8> = after
            .iter()
            .filter(|r| !r.uses_base_archive())
            .map(|r| r.stream_source)
            .collect();
        assert!(
            retail_streams.len() > new_streams.len(),
            "{ch:?}: retail read {} streams, the rebuild reads {} - the pass did nothing",
            retail_streams.len(),
            new_streams.len()
        );

        // The archive really is the sibling's: entry count equals what
        // the record set points at, and it shrank from retail's.
        let retail_slot = me_slot(&retail, slot);
        let a = legaia_actor_formats::me_archive::parse(&retail_slot).expect("retail ME");
        let slot_bytes = me_slot(&del, slot);
        let b = legaia_actor_formats::me_archive::parse(&slot_bytes).expect("delilas ME");
        assert!(
            b.len() < a.len(),
            "{ch:?}: the rebuilt archive still carries {} entries",
            b.len()
        );
        for &s in &new_streams {
            assert!(
                (s as usize) < b.len(),
                "{ch:?}: an art record points at stream {s}, past the {}-entry archive",
                b.len()
            );
        }
        // Every emitted stream decodes, and at the rig's own part count.
        let parts = a.entry(0).expect("retail entry 0")[0];
        for i in 0..b.len() {
            let d = b
                .entry(i)
                .unwrap_or_else(|e| panic!("{ch:?} stream {i}: {e:#}"));
            assert_eq!(d[0], parts, "{ch:?} stream {i}: part count");
            assert!(d[1] > 0, "{ch:?} stream {i}: empty");
        }

        // Every Super Art's component arts still carry a working combo,
        // and the Miracle's row still carries its nine-input one.
        let miracle = &after[MIRACLE_ROW];
        assert_eq!(
            miracle.combo, before[MIRACLE_ROW].combo,
            "{ch:?}: the Miracle Art's combo must survive - it is the only \
             route to the wholesale queue overwrite"
        );
        for sup in legaia_art::super_art::SUPER_ARTS
            .iter()
            .filter(|s| s.character == ch)
        {
            for art in sup.art_sequence() {
                let row = art as usize - 0x10;
                assert!(
                    after[row].combo.len() >= 2,
                    "{ch:?}: {} needs art {art:#04X} (row {row}), whose combo was blanked",
                    sup.name
                );
                assert_eq!(
                    after[row].combo, before[row].combo,
                    "{ch:?}: {}'s component art {art:#04X} changed combo",
                    sup.name
                );
            }
        }

        // Everything blanked is a real art that was blanked whole, and
        // nothing retail could still reach: a blanked row's id must sit
        // above the innate cap, so `FUN_801EFBFC` can only have added it
        // through a performance the blank makes impossible.
        let overlay = del.read_entry(898).expect("battle overlay");
        let cap = overlay[(0x801F_686C - 0x801C_E818) + slot] as usize;
        let mut blanked = 0usize;
        for row in MIRACLE_ROW..after.len() {
            if before[row].combo.len() >= 2 && after[row].combo.is_empty() {
                blanked += 1;
                assert!(
                    row - MIRACLE_ROW > cap,
                    "{ch:?} row {row}: art id {} is at or below the innate cap {cap}, \
                     so a script can still grant it and it would list unusable",
                    row - MIRACLE_ROW
                );
            }
        }
        assert!(blanked > 0, "{ch:?}: nothing was hidden");

        // Every re-animated art dropped the host's impact class (the
        // element spark and the character-tinted afterimages both sit
        // outside the effect script, so nothing else removes them).
        for r in after
            .iter()
            .filter(|r| !r.uses_base_archive() && r.index >= MIRACLE_ROW)
        {
            assert_eq!(
                r.impact_class, 0,
                "{ch:?} row {}: still carries the host's impact class",
                r.index
            );
        }

        // The menu reads as the sibling's list: every performable art
        // other than the signature is named after its clip.
        let label_stem = siblings[slot].display_name();
        let scus =
            legaia_iso::iso9660::read_file_in_image(del.image(), "SCUS_942.54").expect("SCUS");
        let recs = legaia_art::arts_table::raw_records_from_scus(&scus).expect("arts table");
        let mut renamed = 0usize;
        for r in recs.iter().filter(|r| r.character == ch && !r.is_miracle) {
            let row = r.index as usize + MIRACLE_ROW;
            if after[row].combo.len() < 2 {
                continue; // hidden
            }
            let f = legaia_art::arts_table::name_field(&scus, r.record_file_offset).expect("name");
            let name = String::from_utf8_lossy(&scus[f.file_offset..f.file_offset + f.len]);
            if name.starts_with(label_stem) {
                renamed += 1;
            }
        }
        assert!(
            renamed >= 5,
            "{ch:?}: only {renamed} performable art(s) carry a {label_stem} name"
        );
    }

    // Footprint + sector integrity for everything the pass writes.
    let patched = del.into_image();
    assert_eq!(patched.len(), original.len(), "image length preserved");
    let reopened = DiscPatcher::open(patched.clone()).expect("re-open the patched image");
    for entry in [863usize, 864, 865, 894] {
        let lba = reopened.entry_disc_lba(entry).unwrap() as usize;
        let sectors = (reopened.entry_footprint(entry).unwrap() as usize)
            .div_ceil(legaia_iso::raw::USER_DATA_SIZE);
        for s in 0..sectors {
            let sb = (lba + s) * legaia_iso::raw::SECTOR_SIZE;
            assert!(
                legaia_iso::write::mode2_form1_sector_is_valid(
                    &patched[sb..sb + legaia_iso::raw::SECTOR_SIZE]
                ),
                "PROT {entry} sector {s} lost EDC/ECC under the Delilas moveset"
            );
        }
    }

    // Deterministic: the mode takes no seed, so a second run is byte-identical.
    let again = run_mode(&original, MoveMode::Delilas).into_image();
    assert!(
        again == patched,
        "the Delilas moveset must be byte-deterministic"
    );

    // And the two modes really do differ.
    let hybrid = run_mode(&original, MoveMode::Hybrid).into_image();
    assert!(
        hybrid != patched,
        "hybrid and delilas produced the same image"
    );
}

/// The menu labels are per-**sibling** but the name field they overwrite
/// is per-**slot**, and the mapping is a free permutation - so a label
/// sized against the slot a sibling usually lands in silently keeps the
/// retail name under a rearranged party. Proved on the rearrangement,
/// not on the default.
#[test]
fn delilas_labels_fit_every_slot_under_a_rearranged_mapping() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // Che onto Vahn's slot, whose tightest retained field is the
    // seven-byte "Cyclone".
    let mapping = PartyMapping {
        vahn: Sibling::Che,
        noa: Sibling::Gi,
        gala: Sibling::Lu,
    };
    let (_, notes) = run_mapped(&original, MoveMode::Delilas, mapping);
    let tight: Vec<&String> = notes.iter().filter(|n| n.contains("too tight")).collect();
    assert!(
        tight.is_empty(),
        "a menu label did not fit its slot's name field: {tight:?}"
    );
    assert!(
        notes.iter().any(|n| n.contains("art names:")),
        "the rename pass did not run at all"
    );
}

/// The mode string parses both ways and round-trips through `Display`,
/// which is what the CLI flag and the browser dropdown both hand it.
#[test]
fn move_mode_parses_both_ways() {
    assert_eq!("hybrid".parse::<MoveMode>().unwrap(), MoveMode::Hybrid);
    assert_eq!("DELILAS".parse::<MoveMode>().unwrap(), MoveMode::Delilas);
    assert_eq!(MoveMode::default(), MoveMode::Hybrid);
    for m in [MoveMode::Hybrid, MoveMode::Delilas] {
        assert_eq!(m.to_string().parse::<MoveMode>().unwrap(), m);
    }
    assert!("purist".parse::<MoveMode>().is_err());
}
