use super::*;

fn c(a: usize, b: usize) -> Claim {
    Claim::new(a, b, OWNER_RECORD, "t")
}

#[test]
fn lui_pair_address_reads_the_first_access_through_the_register() {
    // lui v0,0x801e ; lw v1,-0x7000(a0) (other reg) ; addiu s4,v0,-0x7acc
    let b = words(&[0x3C02_801E, 0x8C83_9000, 0x2454_8534]);
    assert_eq!(lui_pair_address(&b, 0), Some(0x801D_8534));
    // lui at,0x801d ; addu at,at,s3 ; lh v1,-0x770(at): the displacement
    // completes the pair.
    let b = words(&[0x3C01_801D, 0x0033_0821, 0x8423_F890]);
    assert_eq!(lui_pair_address(&b, 0), Some(0x801C_F890));
    // Not a lui.
    assert_eq!(lui_pair_address(&words(&[0x2402_0001]), 0), None);
}

#[test]
fn imm_before_reads_an_li_through_copies_and_stops_at_a_call() {
    // li a0,0xb ; move a1,s0 ; jal ... ; nop  (walk from the delay slot)
    let b = words(&[0x2404_000B, 0x0200_2821, 0x0C00_724F, 0x0000_0000]);
    assert_eq!(imm_before(&b, 0x801C_E818, 12, 4, 8), Some(0xB));
    // li s1,5 ; move a0,s1 ; jal ; nop - followed through the copy.
    let b = words(&[0x2411_0005, 0x0220_2021, 0x0C00_724F, 0x0000_0000]);
    assert_eq!(imm_before(&b, 0x801C_E818, 12, 4, 8), Some(5));
    // li a0,3 ; jal other ; nop ; jal target ; nop - a call in between
    // names nothing.
    let b = words(&[0x2404_0003, 0x0C00_1000, 0, 0x0C00_724F, 0]);
    assert_eq!(imm_before(&b, 0x801C_E818, 16, 4, 12), None);
}

#[test]
fn the_dead_vlc_table_is_claimed_only_when_every_word_is_an_entry() {
    let base = 0x801C_E818u32;
    let start = (STR_DEAD_VLC_VA - base) as usize;
    let end = (STR_DEAD_VLC_END_VA - base) as usize;
    let mut buf = vec![0u8; end + 16];
    for (i, w) in buf[start..end]
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .enumerate()
    {
        let v: u32 = if i % 3 == 0 { 0 } else { 0x1000_0401 };
        w.copy_from_slice(&v.to_le_bytes());
    }
    let mut sink = Sink::new();
    claim_str_dead_vlc_table(&buf, &mut sink, base);
    assert_eq!(sink.claims.len(), 1);
    assert_eq!((sink.claims[0].start, sink.claims[0].end), (start, end));
    // One word with bits 16..26 set is not a run / level entry.
    buf[start + 8..start + 12].copy_from_slice(&0x1001_0401u32.to_le_bytes());
    let mut sink = Sink::new();
    claim_str_dead_vlc_table(&buf, &mut sink, base);
    assert!(sink.claims.is_empty());
}

#[test]
fn merge_orders_overlaps_and_clamps() {
    let claims = vec![c(10, 20), c(0, 5), c(15, 30), c(40, 44), c(90, 200)];
    assert_eq!(
        merge_ranges(&claims, 100),
        vec![(0, 5), (10, 30), (40, 44), (90, 100)]
    );
}

#[test]
fn merge_drops_empty_and_out_of_range() {
    let claims = vec![c(5, 5), c(200, 300), c(7, 9)];
    assert_eq!(merge_ranges(&claims, 100), vec![(7, 9)]);
}

#[test]
fn merge_joins_touching_ranges() {
    // Adjacent claims are one covered run, not two.
    assert_eq!(merge_ranges(&[c(0, 8), c(8, 16)], 16), vec![(0, 16)]);
}

#[test]
fn complement_is_the_uncovered_runs() {
    let merged = vec![(0, 5), (10, 30), (90, 100)];
    assert_eq!(complement(&merged, 100), vec![(5, 10), (30, 90)]);
    assert_eq!(complement(&[], 8), vec![(0, 8)]);
    assert_eq!(complement(&[(0, 8)], 8), Vec::<(usize, usize)>::new());
}

#[test]
fn complement_covers_the_tail() {
    assert_eq!(complement(&[(0, 4)], 16), vec![(4, 16)]);
}

#[test]
fn residue_zero_pad() {
    assert_eq!(classify_residue(&[0u8; 4096]), ResidueShape::ZeroPad);
}

#[test]
fn residue_alignment_is_short_and_nonzero() {
    assert_eq!(classify_residue(&[0, 0, 1]), ResidueShape::Alignment);
    // ...but a short all-zero run is padding, not alignment.
    assert_eq!(classify_residue(&[0, 0, 0]), ResidueShape::ZeroPad);
}

#[test]
fn residue_repeated_fill_catches_multi_byte_periods() {
    let run: Vec<u8> = b"bX".iter().cycle().take(1024).copied().collect();
    assert_eq!(classify_residue(&run), ResidueShape::RepeatedFill);
    assert_eq!(classify_residue(&[0xAAu8; 512]), ResidueShape::RepeatedFill);
}

#[test]
fn residue_ascii_text() {
    let run: Vec<u8> = b"Display Off\0Gradual\0Immediate\0Turns Left:\0"
        .iter()
        .cycle()
        .take(600)
        .copied()
        .collect();
    // A repeated string pool would read as RepeatedFill; break the period.
    let mut run = run;
    run.push(b'z');
    assert_eq!(classify_residue(&run), ResidueShape::AsciiText);
}

#[test]
fn residue_pointer_dense_beats_plausible_mips() {
    // `0x801C0000` words have primary opcode 0x20 (a plausible `lb`), so a
    // pointer table would read as code if the order were reversed.
    let mut run = Vec::new();
    for i in 0..64u32 {
        run.extend_from_slice(&(0x801C_0000 + i * 4).to_le_bytes());
    }
    assert_eq!(classify_residue(&run), ResidueShape::PointerDense);
}

#[test]
fn residue_plausible_mips() {
    // A run of real prologue / store instructions.
    let words = [
        0x27BD_FFE8u32,
        0xAFBF_0014,
        0xAFB0_0010,
        0x0C00_1234,
        0x8FBF_0014,
        0x8FB0_0010,
        0x27BD_0018,
        0x03E0_0008,
    ];
    let mut run = Vec::new();
    for _ in 0..8 {
        for w in words {
            run.extend_from_slice(&w.to_le_bytes());
        }
    }
    assert_eq!(classify_residue(&run), ResidueShape::PlausibleMips);
}

#[test]
fn residue_bgr555_beats_plausible_mips() {
    // A 15-bit colour page: every halfword under 0x8000, wide spread. A
    // BGR555 pair decodes to a word in the low opcode range, so without
    // this test the run would read as un-dumped code.
    let mut run = Vec::new();
    let mut x = 12345u32;
    for _ in 0..4096 {
        x = x.wrapping_mul(1664525).wrapping_add(1013904223);
        run.extend_from_slice(&(((x >> 9) as u16) & 0x7FFF).to_le_bytes());
    }
    assert_eq!(classify_residue(&run), ResidueShape::Bgr555);

    // Real MIPS is not stolen by it: every load/store word puts a halfword
    // at or above 0x8000.
    let words = [
        0x27BD_FFE8u32,
        0xAFBF_0014,
        0xAFB0_0010,
        0x0C00_1234,
        0x8FBF_0014,
        0x8FB0_0010,
        0x27BD_0018,
        0x03E0_0008,
    ];
    let mut code = Vec::new();
    for _ in 0..8 {
        for w in words {
            code.extend_from_slice(&w.to_le_bytes());
        }
    }
    assert_eq!(classify_residue(&code), ResidueShape::PlausibleMips);
}

#[test]
fn residue_bgr555_does_not_steal_a_small_value_table() {
    // Narrow spread: a sparse table, not a colour page.
    let mut low = Vec::new();
    for i in 0..2048u16 {
        low.extend_from_slice(&(i % 7).to_le_bytes());
    }
    assert_eq!(classify_residue(&low), ResidueShape::LowEntropy);
}

#[test]
fn residue_low_and_high_entropy() {
    // Sparse 16-bit vectors: few distinct byte values.
    let mut low = Vec::new();
    for i in 0..512u16 {
        low.extend_from_slice(&(i % 7).to_le_bytes());
    }
    assert_eq!(classify_residue(&low), ResidueShape::LowEntropy);

    // A deterministic full-spectrum permutation stands in for compressed
    // bytes: every value appears equally often, so H = 8.0.
    let mut hi = Vec::new();
    let mut x = 1u32;
    for _ in 0..64 {
        for b in 0..=255u8 {
            x = x.wrapping_mul(1664525).wrapping_add(1013904223);
            hi.push(b ^ (x >> 24) as u8);
        }
    }
    assert_eq!(classify_residue(&hi), ResidueShape::HighEntropy);
}

#[test]
fn entropy_bounds() {
    assert!(entropy_bits(&[0u8; 256]) < 0.001);
    let all: Vec<u8> = (0..=255u8).collect();
    assert!((entropy_bits(&all) - 8.0).abs() < 0.001);
}

#[test]
fn dump_header_accepts_every_spelling() {
    let a = "== tmd_render 8002735c (entry=8002735c) ==\nsize=2320 bytes, 580 instructions\n\n--- DISASSEMBLY ---\n8002735c  addiu sp,sp,-0x158\n80027360  lui v0,0x8008\n";
    let d = parse_dump_header(a, "8002735c").expect("bare VA");
    assert_eq!(d.entry_va, 0x8002_735C);
    assert_eq!(d.bytes, 2320);
    assert_eq!(d.label, None);
    assert_eq!(d.head_insns[0], "addiu sp,sp,-0x158");

    let b = "== FUN_801cf650 0x801CF650 (entry=0x801cf650, label=equip) [menu] ==\nsize=64 bytes\n";
    let d = parse_dump_header(b, "overlay_menu_801cf650").expect("0x spelling");
    assert_eq!(d.entry_va, 0x801C_F650);
    assert_eq!(d.bytes, 64);
    assert_eq!(d.label.as_deref(), Some("menu"));

    let c = "-- FUN_80012345 (entry 80012345) --\nmin=80012345 max=80012351\n";
    let d = parse_dump_header(c, "80012345").expect("min/max spelling");
    assert_eq!(d.bytes, 0x10);
}

#[test]
fn dump_header_rejects_recorded_answers() {
    let t = "== citation pointer 801cf650 -> FUN_801cf600 ==\nsize=4 bytes\n";
    assert!(parse_dump_header(t, "overlay_menu_801cf650").is_none());
}

#[test]
fn insn_encoder_round_trips_the_common_first_instructions() {
    assert_eq!(encode_insn("nop"), Some(0));
    assert_eq!(encode_insn("addiu sp,sp,-0x18"), Some(0x27BD_FFE8));
    assert_eq!(encode_insn("sw ra,0x14(sp)"), Some(0xAFBF_0014));
    assert_eq!(encode_insn("lui v0,0x8008"), Some(0x3C02_8008));
    assert_eq!(encode_insn("li v0,0x1"), Some(0x2402_0001));
    assert_eq!(encode_insn("jal 0x80012340"), Some(0x0C00_48D0));
    assert_eq!(encode_insn("jr ra"), Some(0x03E0_0008));
    // Unknown mnemonics are unverifiable, never a mismatch.
    assert_eq!(encode_insn("mtc2 v0,$12"), None);
}

#[test]
fn attribution_uses_the_bytes_not_the_address() {
    let dump = DumpExtent {
        entry_va: 0x801C_E818,
        bytes: 16,
        label: Some("menu".into()),
        head_insns: vec!["addiu sp,sp,-0x18".into(), "sw ra,0x14(sp)".into()],
    };
    let mut img = Vec::new();
    img.extend_from_slice(&0x27BD_FFE8u32.to_le_bytes());
    img.extend_from_slice(&0xAFBF_0014u32.to_le_bytes());
    assert_eq!(attribute(&dump, &img, 0x801C_E818), Attribution::Confirmed);

    // An aliased sibling at the same VA holds different bytes.
    let mut other = Vec::new();
    other.extend_from_slice(&0x3C02_8008u32.to_le_bytes());
    other.extend_from_slice(&0xAFBF_0014u32.to_le_bytes());
    assert_eq!(attribute(&dump, &other, 0x801C_E818), Attribution::Refuted);

    // Nothing encodable: the bytes say nothing either way.
    let quiet = DumpExtent {
        head_insns: vec!["mtc2 v0,$12".into()],
        ..dump.clone()
    };
    assert_eq!(
        attribute(&quiet, &img, 0x801C_E818),
        Attribution::Unverifiable
    );

    // A `nop`-headed dump agrees with zero fill in every image at every
    // base, so agreement carries no information. The corpus has such
    // dumps, taken over a sibling image's own zero region, and one of them
    // used to confirm a 20060-byte extent inside a 131172-byte hole.
    let nops = DumpExtent {
        head_insns: vec!["nop".into(), "nop".into(), "nop".into()],
        ..dump.clone()
    };
    assert_eq!(
        attribute(&nops, &[0u8; 0x40], 0x801C_E818),
        Attribution::Unverifiable,
        "matching only zero words is not a confirmation"
    );
    // A real instruction beside the zeros still decides it.
    let mixed = DumpExtent {
        head_insns: vec!["nop".into(), "addiu sp,sp,-0x18".into()],
        ..dump.clone()
    };
    let mut with_code = vec![0u8; 4];
    with_code.extend_from_slice(&0x27BD_FFE8u32.to_le_bytes());
    assert_eq!(
        attribute(&mixed, &with_code, 0x801C_E818),
        Attribution::Confirmed
    );
    // And a mismatch is still a refutation, zero word or not.
    assert_eq!(
        attribute(&mixed, &[0u8; 0x40], 0x801C_E818),
        Attribution::Refuted
    );
}

#[test]
fn scan_claims_do_not_inflate_structural() {
    // A buffer with one TIM inside a run no structural walker claims.
    let mut buf = vec![0u8; 0x400];
    // A minimal 16bpp TIM: magic, flags=2 (no CLUT), then a 4x4 image block.
    buf[0x100..0x104].copy_from_slice(&0x0000_0010u32.to_le_bytes());
    buf[0x104..0x108].copy_from_slice(&0x0000_0002u32.to_le_bytes());
    let img_len = 12 + 4 * 4 * 2;
    buf[0x108..0x10C].copy_from_slice(&(img_len as u32).to_le_bytes());
    buf[0x10C..0x110].copy_from_slice(&0u32.to_le_bytes()); // fb x/y
    buf[0x110..0x112].copy_from_slice(&4u16.to_le_bytes()); // w halfwords
    buf[0x112..0x114].copy_from_slice(&4u16.to_le_bytes()); // h
    for (i, b) in buf.iter_mut().skip(0x114).take(32).enumerate() {
        *b = (i as u8) | 0x40;
    }
    let opts = AccountOptions {
        label: "synthetic".into(),
        rescan: true,
        ..Default::default()
    };
    let acc = account(&buf, &opts);
    assert_eq!(acc.structural, 0, "no structural walker fired");
    if acc.accounted > 0 {
        assert!(
            acc.by_owner.iter().any(|o| o.owner == OWNER_SCAN),
            "any claim here must be tagged as a scan"
        );
    }
}

#[test]
fn prot_index_parses_only_a_four_digit_prefix() {
    assert_eq!(prot_index_from_name("0867_battle_data.BIN"), Some(867));
    assert_eq!(prot_index_from_name("1221_other5.BIN"), Some(1221));
    assert_eq!(prot_index_from_name("battle_data.BIN"), None);
    assert_eq!(prot_index_from_name("867_x.BIN"), None);
}

#[test]
fn seq_extent_walks_to_end_of_track() {
    // Legaia meta encoding: no MIDI length byte after `FF 51` / `FF 2F`.
    let mut s = Vec::new();
    s.extend_from_slice(b"pQES");
    s.extend_from_slice(&[0, 0, 0, 1]); // version u32 BE
    s.extend_from_slice(&480u16.to_be_bytes()); // ppqn
    s.extend_from_slice(&[0x07, 0xA1, 0x20]); // tempo (3 bytes)
    s.extend_from_slice(&[4, 2]); // time signature
    assert_eq!(s.len(), 0x0F);
    s.extend_from_slice(&[0x00, 0x90, 0x3C, 0x40]); // delta, note on
    s.extend_from_slice(&[0x60, 0x3C, 0x00]); // delta, running status
    s.extend_from_slice(&[0x00, 0xFF, 0x2F]); // delta, end of track
    let n = seq_extent(&s, 0).expect("walks to end-of-track");
    assert_eq!(n, s.len());
    // A truncated stream never claims bytes it did not reach.
    assert_eq!(seq_extent(&s[..s.len() - 1], 0), None);
}

#[test]
fn account_of_all_zeros_claims_nothing_and_names_the_shape() {
    let buf = vec![0u8; 0x2000];
    let acc = account(&buf, &AccountOptions::default());
    assert_eq!(acc.accounted, 0);
    assert_eq!(acc.residue_bytes, buf.len());
    assert_eq!(acc.by_shape[0].shape, "zero_pad");
}

fn words(ws: &[u32]) -> Vec<u8> {
    ws.iter().flat_map(|w| w.to_le_bytes()).collect()
}

#[test]
fn lui_forms_reads_the_indexed_array_base() {
    // 0970 at 0x801D055C: lui at,0x801d; addu at,at,a3; lw v1,0xd9c(at)
    let img = words(&[0x3C01_801D, 0x0027_0821, 0x8C23_0D9C]);
    let f = lui_forms(&img, 0x801D_055C);
    assert_eq!(f.len(), 1);
    assert_eq!(f[0].target, 0x801D_0D9C);
    assert_eq!(f[0].index, Some(7));
    assert!(formed_addresses(&img, 0x801D_055C).is_empty());
    assert!(accessed_addresses(&img, 0x801D_055C).is_empty());
    assert_eq!(
        indexed_addresses(&img, 0x801D_055C),
        vec![(0x801D_0564, 0x801D_0D9C, 7)]
    );
}

#[test]
fn lui_forms_follows_a_copy_and_stops_at_a_redefinition() {
    // lui v0,0x8008; move v1,v0; lw a0,0x10(v1)  -> 0x80080010
    let copy = words(&[0x3C02_8008, 0x0040_1821, 0x8C64_0010]);
    assert_eq!(formed_addresses(&copy, 0), vec![(8, 0x8008_0010)]);
    // lui v0,0x8008; lw v0,0x46d0(v0); lbu v1,0x1df(v0) - the second
    // load's base is the pointer the first returned, not the high half.
    let reload = words(&[0x3C02_8008, 0x8C42_46D0, 0x9043_01DF]);
    assert_eq!(formed_addresses(&reload, 0), vec![(4, 0x8008_46D0)]);
}

#[test]
fn lui_forms_keeps_a_delay_slot_and_drops_past_a_call_or_return() {
    // lui a1,0x801e; jal f; addiu a1,a1,-0x5810  -> formed in the slot
    let slot = words(&[0x3C05_801E, 0x0C00_0000, 0x24A5_A7F0]);
    assert_eq!(formed_addresses(&slot, 0), vec![(8, 0x801D_A7F0)]);
    // lui v0,0x8008; jal f; nop; lbu a2,2(v0): v0 is the callee's.
    let call = words(&[0x3C02_8008, 0x0C00_0000, 0, 0x9046_0002]);
    assert!(formed_addresses(&call, 0).is_empty());
    // lui s0,0x8008; jal f; nop; lbu a2,2(s0): s0 survives the call.
    let saved = words(&[0x3C10_8008, 0x0C00_0000, 0, 0x9206_0002]);
    assert_eq!(formed_addresses(&saved, 0), vec![(12, 0x8008_0002)]);
    // lui v0,0x8008; jr ra; nop; lw v1,4(v0): another routine.
    let ret = words(&[0x3C02_8008, 0x03E0_0008, 0, 0x8C43_0004]);
    assert!(formed_addresses(&ret, 0).is_empty());
}

#[test]
fn lui_forms_follows_a_branch_out_of_a_delay_slot() {
    // 0976 at 0x801D4750: bnez v0,L; lui v0,0x8008 (delay slot);
    // addiu v0,zero,1; L: lw a1,-0x4778(v0). Only the taken path pairs.
    let img = words(&[0x1440_0002, 0x3C02_8008, 0x2402_0001, 0x8C45_B888]);
    assert_eq!(formed_addresses(&img, 0), vec![(12, 0x8007_B888)]);
}

#[test]
fn a_window_of_zero_writes_is_data_not_code() {
    // The Baka Fighter image's `FUN_801daa50` label: seven `nop`s, then
    // `mfhi zero` and two more words that write `$zero`.
    let data = words(&[0, 0, 0, 0, 0, 0, 0, 0x0000_0010, 0x2400_0001, 0x8C00_0004]);
    assert!(no_instruction_signature(&data));
    // A real prologue, a `mult` (rd field zero, but it writes hi/lo) and a
    // GTE load into GTE register 0 are code.
    let code = words(&[
        0x27BD_FFE8,
        0xAFBF_0010,
        0x0085_0018,
        0xC880_0000,
        0x03E0_0008,
    ]);
    assert!(!no_instruction_signature(&code));
}

#[test]
fn loop_bounded_arrays_reads_count_stride_and_base_off_the_loop() {
    // PROT 0976 at 0x801D55A8: seventeen words at 0x801DB8B8, walked by
    // s7 = 0..0x10 with `sltiu v0,s7,0x11` as the latch.
    let img = words(&[
        0x2417_0000, // addiu s7,zero,0
        0x0017_1080, // L: sll v0,s7,2
        0x3C03_801E, // lui v1,0x801e
        0x2463_B8B8, // addiu v1,v1,-0x4748
        0x0043_1021, // addu v0,v0,v1
        0x8C50_0000, // lw s0,0(v0)
        0x26F7_0001, // addiu s7,s7,1
        0x2EE2_0011, // sltiu v0,s7,0x11
        0x1440_FFF8, // bnez v0,L
        0x0000_0000,
    ]);
    let a = loop_bounded_arrays(&img, 0);
    assert_eq!(a.len(), 1);
    assert_eq!((a[0].base, a[0].count, a[0].stride), (0x801D_B8B8, 17, 4));
    assert_eq!((a[0].form_site, a[0].bound_site), (12, 28));
    // The same loop counting from a runtime start pins nothing.
    let mut runtime = img.clone();
    runtime[..4].copy_from_slice(&0x0280_B821u32.to_le_bytes()); // move s7,s4
    assert!(loop_bounded_arrays(&runtime, 0).is_empty());
}

#[test]
fn a_dialog_token_argument_is_not_a_terminator() {
    // `0xC1 0x00` names the lead party member; the string runs on to the
    // real NUL.
    let mut s = b"@".to_vec();
    s.extend_from_slice(&[0xC1, 0x00]);
    s.extend_from_slice(b" will equip\0");
    assert_eq!(cstring_end(&s, 0), Some(s.len()));
    // A plain string still stops at its first NUL.
    assert_eq!(cstring_end(b"Load\0Save\0", 0), Some(5));
    // A token whose argument byte is past the buffer names nothing.
    assert_eq!(cstring_end(&[b'A', 0xCF], 0), None);
}
