//! Disc oracle for `--delilas-party`: apply on a scratch copy, re-decode
//! everything the swap claims to change, prove idempotence + determinism.
//! Skips (and passes) when `LEGAIA_DISC_BIN` is unset.

use legaia_patcher::delilas_party::{PartyMapping, Sibling, apply_delilas_party};
use legaia_patcher::disc::{DiscPatcher, MONSTER_ARCHIVE_ENTRY};

fn load_disc() -> Option<Vec<u8>> {
    let path = std::env::var("LEGAIA_DISC_BIN").ok()?;
    if path.is_empty() {
        return None;
    }
    std::fs::read(path).ok()
}

fn block_name(patcher: &DiscPatcher, id: u16) -> String {
    let archive = patcher
        .read_entry_footprint(MONSTER_ARCHIVE_ENTRY)
        .expect("archive");
    legaia_asset::monster_archive::record(&archive, id)
        .expect("record")
        .expect("populated")
        .name
}

#[test]
fn default_mapping_swaps_models_names_and_is_idempotent() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    let mut patcher = DiscPatcher::open(original.clone()).expect("open disc");

    // Baseline: retail block names.
    assert_eq!(block_name(&patcher, 162), "Gi Delilas");
    assert_eq!(block_name(&patcher, 163), "Che Delilas");
    assert_eq!(block_name(&patcher, 164), "Lu Delilas");

    let mapping = PartyMapping::default();
    let report = apply_delilas_party(&mut patcher, &mapping).expect("apply");
    assert!(report.changed);
    let patched = patcher.into_image();
    assert_eq!(patched.len(), original.len(), "image length preserved");

    // Re-open validates EDC/ECC on every touched sector.
    let reopened = DiscPatcher::open(patched.clone()).expect("re-open patched");

    // Default mapping: Gi<->Vahn, Lu<->Noa, Che<->Gala.
    assert_eq!(block_name(&reopened, 162), "Vahn");
    assert_eq!(block_name(&reopened, 164), "Noa");
    assert_eq!(block_name(&reopened, 163), "Gala");

    // The swapped blocks still decode as 15-part meshes and the retail
    // animation streams still cover them.
    let archive = reopened
        .read_entry_footprint(MONSTER_ARCHIVE_ENTRY)
        .expect("archive");
    for id in [162u16, 163, 164] {
        let mesh = legaia_asset::monster_archive::mesh(&archive, id)
            .expect("mesh")
            .expect("populated");
        let tmd = legaia_tmd::parse(mesh.tmd_bytes()).expect("swapped TMD");
        assert_eq!(tmd.objects.len(), 15, "monster {id} part count");
        let anims = legaia_asset::monster_archive::animations(&archive, id)
            .expect("anims")
            .expect("populated");
        assert!(anims.iter().all(|a| a.part_count == 15));
    }

    // The rebuilt player files re-assemble through the retail chain.
    for entry in [863usize, 864, 865] {
        let file = reopened.read_entry_footprint(entry).expect("player file");
        let pack = legaia_asset::battle_data_pack::parse(&file).expect("pack reparse");
        legaia_asset::battle_char_assembly::assemble_character(&file, &pack, &[0; 5])
            .expect("assemble");
        legaia_asset::battle_char_assembly::battle_animations(&file).expect("anims");
    }

    // New-game template names follow the mapping.
    let scus = reopened
        .read_named_file("SCUS_942.54")
        .expect("SCUS present");
    let tmpl = legaia_asset::new_game::party_template_file_offset(&scus).expect("template");
    let name_at = |slot: usize| -> String {
        let off = tmpl + slot * legaia_asset::new_game::RECORD_STRIDE + 16;
        let field = &scus[off..off + legaia_asset::new_game::NAME_LEN];
        let len = field.iter().position(|&b| b == 0).unwrap_or(field.len());
        String::from_utf8_lossy(&field[..len]).into_owned()
    };
    assert_eq!(name_at(0), "Gi");
    assert_eq!(name_at(1), "Lu");
    assert_eq!(name_at(2), "Che");

    // Battle voices: the party's voice program now carries the mapped
    // sibling's samples - program 7 (Vahn) tone 0 decodes to the same
    // PCM as Gi's monster.snd program tone 0.
    {
        use legaia_patcher::delilas_voice::{BATTLE_BANK_ENTRY, MONSTER_SND_ENTRY};
        let bank = reopened.read_entry(BATTLE_BANK_ENTRY).expect("bank");
        let bank_off = *legaia_vab::find_vabs(&bank).first().expect("bank VAB");
        let bank_vab = legaia_vab::parse(&bank, bank_off).expect("bank parses after splice");
        let snd = reopened
            .read_entry_footprint(MONSTER_SND_ENTRY)
            .expect("monster.snd");
        let gi_sec =
            u32::from_le_bytes(snd[8 + 161 * 4..12 + 161 * 4].try_into().unwrap()) as usize * 0x800;
        let gi_vab = legaia_vab::parse(&snd, gi_sec + 4).expect("Gi VAB");
        let tone_vag = |vab: &legaia_vab::VabReport, prog: usize, tone: usize| -> usize {
            let page = vab
                .programs
                .iter()
                .enumerate()
                .filter(|(_, p)| p.tones > 0)
                .position(|(i, _)| i == prog)
                .expect("program populated");
            vab.tones[page][tone].vag as usize
        };
        let bank_span = bank_vab.vag_samples[tone_vag(&bank_vab, 7, 0) - 1];
        let gi_span = gi_vab.vag_samples[tone_vag(&gi_vab, 62, 0) - 1];
        let bank_pcm = legaia_vab::decode_vag_aligned(
            &bank[bank_span.byte_offset..bank_span.byte_offset + bank_span.size],
        )
        .expect("patched VAG decodes");
        let gi_pcm = legaia_vab::decode_vag_aligned(
            &snd[gi_span.byte_offset..gi_span.byte_offset + gi_span.size],
        )
        .expect("Gi VAG decodes");
        let n = bank_pcm.len().min(gi_pcm.len());
        assert!(n > 1000, "spliced sample too short ({n} samples)");
        assert_eq!(
            &bank_pcm[..n.min(2000)],
            &gi_pcm[..n.min(2000)],
            "program 7 tone 0 does not carry Gi's sample"
        );
    }

    // Idempotence: a second apply is a no-op and changes no bytes.
    let mut second = DiscPatcher::open(patched.clone()).expect("open patched");
    let report2 = apply_delilas_party(&mut second, &mapping).expect("re-apply");
    assert!(!report2.changed, "second apply must be a no-op");
    assert_eq!(second.into_image(), patched, "re-apply changed bytes");

    // Determinism: a fresh run over the retail image is byte-identical.
    let mut third = DiscPatcher::open(original).expect("open again");
    apply_delilas_party(&mut third, &mapping).expect("apply again");
    assert_eq!(third.into_image(), patched, "apply is deterministic");
}

#[test]
fn custom_mapping_rearranges_the_assignment() {
    let Some(original) = load_disc() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return;
    };
    // "Lu can replace Vahn": lu,gi,che.
    let mapping = PartyMapping::parse("lu,gi,che").expect("parse mapping");
    assert_eq!(mapping.vahn, Sibling::Lu);
    let mut patcher = DiscPatcher::open(original).expect("open disc");
    let report = apply_delilas_party(&mut patcher, &mapping).expect("apply");
    assert!(report.changed);
    let reopened = DiscPatcher::open(patcher.into_image()).expect("re-open");
    // Lu's block now depicts Vahn, Gi's depicts Noa, Che's depicts Gala.
    assert_eq!(block_name(&reopened, 164), "Vahn");
    assert_eq!(block_name(&reopened, 162), "Noa");
    assert_eq!(block_name(&reopened, 163), "Gala");
}

#[test]
fn mapping_parser_rejects_non_permutations() {
    assert!(PartyMapping::parse("gi,gi,che").is_err());
    assert!(PartyMapping::parse("gi,lu").is_err());
    assert!(PartyMapping::parse("gi,lu,songi").is_err());
    assert!(PartyMapping::parse("che,gi,lu").is_ok());
}
