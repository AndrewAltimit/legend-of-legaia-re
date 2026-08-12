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
    // The field forms must come from the siblings' own NPC meshes at
    // full detail - no battle-model fallback, no decimation ladder, no
    // head-texture downscale.
    for note in &report.notes {
        assert!(
            !note.contains("NPC-mesh source unavailable")
                && !note.contains("detail reduced")
                && !note.contains("resolution"),
            "field quality regression: {note}"
        );
    }
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

    // Battle voices, both directions: program 7 (Vahn) tone 0 now decodes
    // to RETAIL Gi's grunt, and the patched monster.snd Gi bank tone 0
    // decodes to RETAIL Vahn's grunt (the samples are byte-copies, so the
    // decoded prefix over the copied span must match exactly).
    {
        use legaia_patcher::delilas_voice::{BATTLE_BANK_ENTRY, MONSTER_SND_ENTRY};
        let retail = DiscPatcher::open(original.clone()).expect("open retail");
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
        let body = |buf: &[u8], off: usize, prog: usize, tone: usize| -> Vec<u8> {
            let vab = legaia_vab::parse(buf, off).expect("VAB parses");
            let span = vab.vag_samples[tone_vag(&vab, prog, tone) - 1];
            buf[span.byte_offset..span.byte_offset + span.size].to_vec()
        };
        let gi_off = |snd: &[u8]| -> usize {
            u32::from_le_bytes(snd[8 + 161 * 4..12 + 161 * 4].try_into().unwrap()) as usize * 0x800
                + 4
        };

        let bank = reopened.read_entry(BATTLE_BANK_ENTRY).expect("bank");
        let bank_off = *legaia_vab::find_vabs(&bank).first().expect("bank VAB");
        let retail_bank = retail.read_entry(BATTLE_BANK_ENTRY).expect("retail bank");
        let snd = reopened.read_entry(MONSTER_SND_ENTRY).expect("monster.snd");
        let retail_snd = retail.read_entry(MONSTER_SND_ENTRY).expect("retail snd");

        let patched_party = body(&bank, bank_off, 7, 0);
        let retail_gi = body(&retail_snd, gi_off(&retail_snd), 62, 0);
        let n = patched_party.len().min(retail_gi.len());
        assert!(n > 500, "spliced sample too short ({n} bytes)");
        assert_eq!(
            &patched_party[..n / 16 * 16],
            &retail_gi[..n / 16 * 16],
            "program 7 tone 0 does not carry Gi's sample"
        );

        let patched_gi = body(&snd, gi_off(&snd), 62, 0);
        let retail_party = body(&retail_bank, bank_off, 7, 0);
        let n = patched_gi.len().min(retail_party.len()) / 16 * 16;
        assert!(n > 500, "mirrored sample too short ({n} bytes)");
        // The mirror truncates into the smaller sibling slot; the copied
        // prefix (minus the re-flagged final block) matches byte-exact.
        assert_eq!(
            &patched_gi[..n - 16],
            &retail_party[..n - 16],
            "Gi's duel bank does not carry Vahn's sample"
        );

        // Sharing rules: retail's silent placeholder tone (prog 7 tone 2,
        // vol 0) stays silent, and the SHARED vag-1 body it points at
        // (also a battle SFX + other programs' placeholders) stays
        // byte-identical to retail.
        let vab = legaia_vab::parse(&bank, bank_off).expect("patched bank");
        let retail_vab = legaia_vab::parse(&retail_bank, bank_off).expect("retail bank");
        let page7 = vab
            .programs
            .iter()
            .enumerate()
            .filter(|(_, p)| p.tones > 0)
            .position(|(i, _)| i == 7)
            .expect("program 7 page");
        assert_eq!(
            vab.tones[page7][2].vol, 0,
            "silent placeholder tone woke up"
        );
        let (r1, p1) = (retail_vab.vag_samples[0], vab.vag_samples[0]);
        assert_eq!(
            &retail_bank[r1.byte_offset..r1.byte_offset + r1.size],
            &bank[p1.byte_offset..p1.byte_offset + p1.size],
            "shared vag 1 body was overwritten"
        );
    }

    // The container header words the battle scene loader registers as
    // battle-VDF pointers at every battle load (FUN_800520F0 state 0xc:
    // meta[0], meta[1], type<<24|size0, offset0) must stay byte-exact -
    // meta[1] is the offset of the VDF tail past the LZS payload, and a
    // recomputed value points the effect system at garbage (battle-load
    // hang). PROT 0874 is a dual-consumer entry: field player pack AND
    // battle VDF carrier.
    {
        let retail = DiscPatcher::open(original.clone()).expect("open retail");
        let r = retail.read_entry(874).expect("retail 874");
        let p = reopened.read_entry(874).expect("patched 874");
        assert_eq!(
            &r[..16],
            &p[..16],
            "PROT 0874 registered header words changed"
        );
    }

    // The arts XA shout banks are muted: every Form 2 sector's ADPCM
    // payload is zero while the subheaders (channel routing) survive.
    {
        let (lba, size) =
            legaia_iso::iso9660::find_path_in_image(&patched, "XA/XA2.XA").expect("XA2.XA");
        let sectors = (size as usize).div_ceil(2048);
        let mut audio = 0usize;
        for i in 0..sectors {
            let base = (lba as usize + i) * 2352;
            let sector = &patched[base..base + 2352];
            if sector[0x12] & 0x20 == 0 {
                continue; // not Form 2
            }
            assert!(
                sector[0x18..0x92C].iter().all(|&b| b == 0),
                "XA2.XA sector {i} still carries audio"
            );
            assert_ne!(&sector[0x10..0x18], &[0u8; 8], "subheader wiped");
            audio += 1;
        }
        assert!(audio > 100, "XA2.XA had only {audio} Form 2 sectors");
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
