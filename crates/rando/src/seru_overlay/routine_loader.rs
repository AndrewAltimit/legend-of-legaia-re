//! Overlay loader / warp / draw-side builders: the sentinel + draw overlays,
//! the CD-loader stubs, the mode-24 warp triggers + redirect, and the small
//! detour-word / sizing helpers.

use super::*;

/// Assemble the slice overlay: write [`SENTINEL`] to [`SENTINEL_ADDR`], return.
/// Position-independent (absolute store + `jr ra`), so it executes at any load
/// address. 6 instructions / 24 bytes.
pub fn assemble_sentinel_overlay() -> Vec<u32> {
    vec![
        lui(V0, imm_hi(SENTINEL)),     // v0 = SENTINEL hi
        ori(V0, V0, imm_lo(SENTINEL)), // v0 |= SENTINEL lo
        lui(V1, hi(SENTINEL_ADDR)),    // v1 = &SENTINEL_ADDR hi
        sw(V0, V1, lo(SENTINEL_ADDR)), // *SENTINEL_ADDR = v0
        jr(RA),                        // return to the stub
        nop(),                         // (branch delay)
    ]
}

/// Assemble the loader stub for an overlay at disc `lba` spanning `sectors`
/// sectors, loaded to [`DEST`] and called. `displaced` are the two hook
/// instructions to replay; `return_va` is where to jump back. Lives at
/// [`STUB_VA`]. 18 instructions / 72 bytes (fits the gap free window).
///
/// After the CD read it calls the BIOS `FlushCache` (A-func [`FLUSH_CACHE_FN`]
/// via the [`BIOS_DISPATCH_A`] dispatcher) **before** executing the loaded code:
/// the PSX I-cache is not DMA-coherent, so freshly streamed code can otherwise
/// run stale on hardware.
pub fn assemble_loader_stub(
    lba: u32,
    sectors: u16,
    displaced: [u32; 2],
    return_va: u32,
) -> Vec<u32> {
    vec![
        addiu(A0, ZERO, sectors),  // 0:  a0 = sector_count
        lui(A1, imm_hi(lba)),      // 1:  \ a1 = lba
        ori(A1, A1, imm_lo(lba)),  // 2:  /
        lui(A2, imm_hi(DEST)),     // 3:  \ a2 = dest
        ori(A2, A2, imm_lo(DEST)), // 4:  /
        jal(LOADER_FN),            // 5:  FUN_8005E4D4(sectors, lba, dest)
        nop(),                     // 6:  (delay)
        // FlushCache() so the just-loaded code isn't executed from a stale
        // I-cache line (PSX I-cache is not DMA-coherent).
        addiu(T2, ZERO, BIOS_DISPATCH_A), // 7:  t2 = 0xA0 (A-table dispatcher)
        jalr(T2),                         // 8:  call it (returns to us)
        addiu(T1, ZERO, FLUSH_CACHE_FN),  // 9:  (delay) t1 = 0x44 = FlushCache
        lui(T0, imm_hi(DEST)),            // 10: \ t0 = dest
        ori(T0, T0, imm_lo(DEST)),        // 11: /
        jalr(T0),                         // 12: call the loaded overlay
        nop(),                            // 13: (delay)
        displaced[0],                     // 14: replay hook instr 0
        displaced[1],                     // 15: replay hook instr 1
        j(return_va),                     // 16: back to the hook join
        nop(),                            // 17: (delay)
    ]
}

/// Assemble the **shop-gated** loader stub for the op-0x49 arm-edge detour. It
/// gates on the sub-op (`*s6 == 0` = a merchant; skips name-entry / inn / save),
/// loads + FlushCaches + runs the overlay, then replays the two displaced
/// instructions ([`SHOP_DISPLACED`]) and jumps back to [`SHOP_RETURN_VA`]. `s6`
/// (the field VM's live operand pointer) and `s0`/`s1` are preserved by the
/// callees, so the dispatcher continues correctly. Lives at [`STUB_VA`].
pub fn assemble_shop_loader_stub(lba: u32, sectors: u16) -> Vec<u32> {
    assemble_shop_loader_stub_gated(lba, sectors, true)
}

/// As [`assemble_shop_loader_stub`], but `gated` selects whether the sub-op gate
/// is live. With `gated = false` the gate branch is neutered (`bne zero,zero` is
/// never taken) so the overlay loads on **every** op-`0x49` arm (shop / inn /
/// save / name-entry alike) - a diagnostic build that proves whether the detour
/// fires at all, independent of the sub-op value. The word layout is identical
/// either way (only the branch's `rs` register changes), so the sidecar + the
/// disc oracle stay consistent.
pub fn assemble_shop_loader_stub_gated(lba: u32, sectors: u16, gated: bool) -> Vec<u32> {
    // Indices: the load block is 3..16, the replay block starts at 17.
    const REPLAY: usize = 17;
    let skip_off = (REPLAY as i32 - (1 + 1)) as i16; // bne at idx 1 -> REPLAY
    let gate_rs = if gated { T3 } else { ZERO }; // bne zero,zero is never taken
    vec![
        lbu(T3, S6, 0),                   // 0:  t3 = *operand (op-0x49 sub-op)
        bne(gate_rs, ZERO, skip_off),     // 1:  if sub-op != 0 (not a shop) -> replay
        nop(),                            // 2:  (delay)
        addiu(A0, ZERO, sectors),         // 3:  a0 = sector_count
        lui(A1, imm_hi(lba)),             // 4:  \ a1 = lba
        ori(A1, A1, imm_lo(lba)),         // 5:  /
        lui(A2, imm_hi(DEST)),            // 6:  \ a2 = dest
        ori(A2, A2, imm_lo(DEST)),        // 7:  /
        jal(LOADER_FN),                   // 8:  FUN_8005E4D4(sectors, lba, dest)
        nop(),                            // 9:  (delay)
        addiu(T2, ZERO, BIOS_DISPATCH_A), // 10: t2 = 0xA0
        jalr(T2),                         // 11: FlushCache()
        addiu(T1, ZERO, FLUSH_CACHE_FN),  // 12: (delay) t1 = 0x44
        lui(T0, imm_hi(DEST)),            // 13: \ t0 = dest
        ori(T0, T0, imm_lo(DEST)),        // 14: /
        jalr(T0),                         // 15: run the loaded overlay
        nop(),                            // 16: (delay)
        // REPLAY (idx 17): the displaced arm-edge instructions, then return.
        SHOP_DISPLACED[0], // 17: sw s6,-0x4bb0(s0)
        SHOP_DISPLACED[1], // 18: lbu v0,0(s6)
        j(SHOP_RETURN_VA), // 19: back to the dispatcher
        nop(),             // 20: (delay)
    ]
}

/// Assemble the **option-1 warp trigger** stub (lives at [`STUB_VA`], reached by
/// the op-0x49 detour's `j STUB_VA`). Instead of a raw mid-tick CD read (which
/// reentrantly froze), it MIRRORS the field-VM op-0x3E minigame door-warp arm
/// (`0x801E078C`): zero the two housekeeping words, set the minigame `sub_id`,
/// request mode 24 (`MODE_OTHER_INIT`), and call the SysFlag setter - then replay
/// the op-0x49 displaced pair and return to the dispatcher. The current frame
/// finishes normally; next frame the mode SM enters mode 24 (`FUN_80025980`),
/// which loads the slot-A overlay for `sub_id` from the SAFE between-frames CD
/// context and warps back to field on exit. `sub_id` selects which overlay
/// `FUN_80025980` loads + dispatches - wiring our own overlay there is the
/// payload-hosting step (the FUN_80025980 switch fork), separate from this
/// trigger. Does NOT clear the op-0x3E `player[+0x10]&~0x80000` bit (that needs
/// the live VM-ctx player pointer, absent at this hook; it is session cleanup,
/// not part of the mode handoff).
pub fn assemble_warp_trigger_stub(sub_id: u16, fire_once: bool) -> Vec<u32> {
    assemble_warp_trigger_stub_opts(sub_id, fire_once, false)
}

/// As [`assemble_warp_trigger_stub`], but `gate_shop` adds a sub-op gate so the
/// warp only fires for a **merchant** (op-0x49 sub-op `0` - the shop record),
/// skipping name-entry / inn / save. `s6` is the live operand pointer at the
/// detour, so `*s6` is the sub-op. (Shops don't auto-retrigger, so the real
/// feature uses `gate_shop=true, fire_once=false`.) Both guards branch to the
/// shared `.replay` tail (offsets patched once it's placed).
pub fn assemble_warp_trigger_stub_opts(sub_id: u16, fire_once: bool, gate_shop: bool) -> Vec<u32> {
    let mut w: Vec<u32> = Vec::new();
    // bne placeholders (index, rs-register) that must branch to .replay.
    let mut to_replay: Vec<(usize, u32)> = Vec::new();
    if gate_shop {
        // if (*s6 != 0) -> .replay  (only sub-op 0 = a shop runs the warp)
        w.push(lbu(T3, S6, 0));
        to_replay.push((w.len(), T3));
        w.push(bne(T3, ZERO, 0));
        w.push(nop());
    }
    if fire_once {
        // Skip the warp (just replay) if we have already fired once.
        w.push(lui(AT, hi(WARP_FIRED_VA)));
        w.push(lhu(V0, AT, lo(WARP_FIRED_VA)));
        to_replay.push((w.len(), V0));
        w.push(bne(V0, ZERO, 0));
        w.push(nop());
        w.push(addiu(V0, ZERO, 1)); // flag = 1
        w.push(lui(AT, hi(WARP_FIRED_VA)));
        w.push(sh(V0, AT, lo(WARP_FIRED_VA)));
    }
    // The warp arm: mirror the op-0x3E minigame door-warp.
    w.push(lui(AT, hi(WARP_HOUSEKEEP_VA))); //  \ _DAT_8007BAC0 = 0
    w.push(sw(ZERO, AT, lo(WARP_HOUSEKEEP_VA))); //  /
    w.push(lui(AT, hi(WINNINGS_VA))); //  \ _DAT_80084440 = 0
    w.push(sw(ZERO, AT, lo(WINNINGS_VA))); //  /
    w.push(addiu(V0, ZERO, sub_id)); //  \ _DAT_8007BA34 = sub_id
    w.push(lui(AT, hi(SUBID_VA))); //  |
    w.push(sh(V0, AT, lo(SUBID_VA))); //  /
    w.push(addiu(V0, ZERO, MODE_OTHER_INIT)); //  \ _DAT_8007B83C = 0x18 (mode 24)
    w.push(lui(AT, hi(MODE_INDEX_VA))); //  |
    w.push(sh(V0, AT, lo(MODE_INDEX_VA))); //  /
    w.push(addiu(A0, ZERO, SYSFLAG_WARP_ARG)); //  a0 = 0xE
    w.push(jal(SYSFLAG_SET_FN)); //  func_0x8003CE08(0xE)
    w.push(nop()); //  (delay)
    // .replay: the op-0x49 displaced pair, then return to the dispatcher.
    let replay = w.len();
    w.push(SHOP_DISPLACED[0]); // replay sw s6,-0x4bb0(s0)
    w.push(SHOP_DISPLACED[1]); // replay lbu v0,0(s6)
    w.push(j(SHOP_RETURN_VA)); // back to the dispatcher
    w.push(nop()); // (delay)
    for (i, rs) in to_replay {
        w[i] = bne(rs, ZERO, (replay as i32 - (i as i32 + 1)) as i16);
    }
    w
}

/// Assemble the **dead-mode request trigger** at [`TRIGGER_VA`] (reached by the
/// op-0x49 detour). It just requests [`DEAD_MODE_INDEX`] (writes the master mode
/// index), replays the op-0x49 displaced pair, and returns to the dispatcher.
/// The current frame finishes; next frame the mode SM enters our dead mode and
/// calls [`assemble_mode_init_loader_stub`] in the safe between-frames context.
/// No CD read here - the load is deferred to the mode handler.
pub fn assemble_mode_request_trigger() -> Vec<u32> {
    vec![
        lui(AT, hi(MODE_INDEX_VA)),       // 0:  \ v0 = current mode index
        lhu(V0, AT, lo(MODE_INDEX_VA)),   // 1:  /  (the mode we are interrupting)
        lui(AT, hi(ORIGIN_MODE_VA)),      // 2:  \ stash it for the loader to restore
        sh(V0, AT, lo(ORIGIN_MODE_VA)),   // 3:  /
        addiu(V0, ZERO, DEAD_MODE_INDEX), // 4:  \ _DAT_8007B83C = dead mode
        lui(AT, hi(MODE_INDEX_VA)),       // 5:  |  (request the mode)
        sh(V0, AT, lo(MODE_INDEX_VA)),    // 6:  /
        SHOP_DISPLACED[0],                // 7:  replay sw s6,-0x4bb0(s0)
        SHOP_DISPLACED[1],                // 8:  replay lbu v0,0(s6)
        j(SHOP_RETURN_VA),                // 9:  back to the dispatcher
        nop(),                            // 10: (delay)
    ]
}

/// Assemble the **mode-INIT loader** at [`MODE_INIT_VA`] (what the repurposed
/// dead mode's handler points at). Called by the mode SM via `jal` in the safe
/// between-frames context, so the proven load sequence runs without mid-tick
/// reentrancy: CD-read the pochi overlay (baked `lba`/`sectors`) to [`DEST`],
/// FlushCache, run it, then request [`FIELD_MODE_INDEX`] to return to the field
/// (the field overlay stays resident in slot A - we load to slot B) and `jr ra`.
/// `ra` is saved across the inner calls on the stack.
pub fn assemble_mode_init_loader_stub(lba: u32, sectors: u16) -> Vec<u32> {
    vec![
        addiu(SP, SP, 0xFFF8),            // 0:  addiu sp,sp,-8
        sw(RA, SP, 4),                    // 1:  sw ra,4(sp)
        addiu(A0, ZERO, sectors),         // 2:  a0 = sector_count
        lui(A1, imm_hi(lba)),             // 3:  \ a1 = lba
        ori(A1, A1, imm_lo(lba)),         // 4:  /
        lui(A2, imm_hi(DEST)),            // 5:  \ a2 = dest
        ori(A2, A2, imm_lo(DEST)),        // 6:  /
        jal(LOADER_FN),                   // 7:  FUN_8005E4D4(sectors, lba, dest)
        nop(),                            // 8:  (delay)
        addiu(T2, ZERO, BIOS_DISPATCH_A), // 9:  t2 = 0xA0
        jalr(T2),                         // 10: FlushCache()
        addiu(T1, ZERO, FLUSH_CACHE_FN),  // 11: (delay) t1 = 0x44
        lui(T0, imm_hi(DEST)),            // 12: \ t0 = dest
        ori(T0, T0, imm_lo(DEST)),        // 13: /
        jalr(T0),                         // 14: run the loaded overlay
        nop(),                            // 15: (delay)
        lui(AT, hi(ORIGIN_MODE_VA)),      // 16: \ v0 = the stashed origin mode
        lhu(V0, AT, lo(ORIGIN_MODE_VA)),  // 17: /
        lui(AT, hi(MODE_INDEX_VA)),       // 18: \ _DAT_8007B83C = origin mode
        sh(V0, AT, lo(MODE_INDEX_VA)),    // 19: /  (resume what we interrupted)
        lw(RA, SP, 4),                    // 20: lw ra,4(sp)
        addiu(SP, SP, 8),                 // 21: addiu sp,sp,8
        jr(RA),                           // 22: return to the mode SM
        nop(),                            // 23: (delay)
    ]
}

/// Assemble the **mode-24 overlay-load redirect** at [`WARP_REDIRECT_VA`]. It is
/// reached by a detour planted at [`WARP_INIT_DETOUR_VA`] inside `FUN_80025980`
/// (the mode-24 INIT), in place of the game's per-sub-id `jal FUN_8003EBE4`. For
/// **our** sub-id ([`WARP_SUBID`]) it baked-LBA-loads our pochi overlay to slot A
/// ([`SLOT_A_BASE`]), FlushCaches, runs the overlay's init, then calls the mode-24
/// return warp [`MODE24_RETURN_FN`] (restore scene + request mode 2 reload) and
/// runs `FUN_80025980`'s epilogue itself - bypassing the function's mode-0x19
/// hand-off (we go straight back to the field, not into a mode-25 minigame loop).
/// For any other sub-id it replays the original loader and rejoins at
/// [`WARP_INIT_REJOIN_VA`], so all 7 retail minigames are unaffected. `a0` still
/// holds `sub_id+0x4D` from the code preceding the detour (for the replayed load).
pub fn assemble_warp_init_redirect(lba: u32, sectors: u16) -> Vec<u32> {
    assemble_warp_init_redirect_opts(lba, sectors, true)
}

/// As [`assemble_warp_init_redirect`], but `call_return_warp` selects what happens
/// after our overlay's INIT returns. `true` (sentinel slice): call
/// [`MODE24_RETURN_FN`] for an immediate field reload. `false` (draw side): skip it
/// - the overlay's INIT itself requests the persistent draw mode (mode 13), so the
///   game keeps calling the overlay's TICK each frame until the TICK returns to field.
pub fn assemble_warp_init_redirect_opts(
    lba: u32,
    sectors: u16,
    call_return_warp: bool,
) -> Vec<u32> {
    // .ours is index 9; beq is index 3 -> offset = 9 - (3 + 1) = 5.
    const OURS: i16 = 5;
    let (ret0, ret1) = if call_return_warp {
        (jal(MODE24_RETURN_FN), nop())
    } else {
        (nop(), nop())
    };
    vec![
        lui(AT, hi(WARP_SUBID_VA)),     // 0:  \ v0 = current sub-id
        lhu(V0, AT, lo(WARP_SUBID_VA)), // 1:  /
        addiu(T0, ZERO, WARP_SUBID),    // 2:  t0 = our sub-id
        beq(V0, T0, OURS),              // 3:  if ours -> .ours (idx 9)
        nop(),                          // 4:  (delay)
        // .default: original loader (a0 = sub_id+0x4D intact), then rejoin.
        jal(OVERLAY_LOADER_A_FN), // 5:  FUN_8003EBE4(a0)
        nop(),                    // 6:  (delay)
        j(WARP_INIT_REJOIN_VA),   // 7:  back into FUN_80025980
        nop(),                    // 8:  (delay)
        // .ours: baked-LBA load our overlay to slot A, run it, return to field.
        addiu(A0, ZERO, sectors),         // 9:  a0 = sector_count
        lui(A1, imm_hi(lba)),             // 10: \ a1 = lba
        ori(A1, A1, imm_lo(lba)),         // 11: /
        lui(A2, imm_hi(SLOT_A_BASE)),     // 12: \ a2 = slot A base
        ori(A2, A2, imm_lo(SLOT_A_BASE)), // 13: /
        jal(LOADER_FN),                   // 14: FUN_8005E4D4(sectors, lba, slotA)
        nop(),                            // 15: (delay)
        addiu(T2, ZERO, BIOS_DISPATCH_A), // 16: t2 = 0xA0
        jalr(T2),                         // 17: FlushCache()
        addiu(T1, ZERO, FLUSH_CACHE_FN),  // 18: (delay) t1 = 0x44
        lui(T3, imm_hi(SLOT_A_BASE)),     // 19: \ t3 = slot-A overlay entry
        ori(T3, T3, imm_lo(SLOT_A_BASE)), // 20: /
        jalr(T3),                         // 21: run our overlay's init
        nop(),                            // 22: (delay)
        ret0,                             // 23: FUN_80026018 (sentinel) or nop (draw)
        ret1,                             // 24: (delay / nop)
        lw(RA, SP, WARP_INIT_RA_OFF),     // 25: \ FUN_80025980 epilogue, bypassing
        lw(S0, SP, WARP_INIT_S0_OFF),     // 26: |  its mode-0x19 hand-off
        addiu(SP, SP, WARP_INIT_FRAME),   // 27: /
        jr(RA),                           // 28: return from FUN_80025980
        nop(),                            // 29: (delay)
    ]
}

/// Assemble the **draw-side slot-A overlay** (loaded to [`SLOT_A_BASE`]). INIT
/// (offset 0) hands off to mode 13 so the game calls the TICK (offset
/// [`SLOT_A_TICK_OFFSET`]) each frame. The TICK draws a native window box
/// ([`BOX_FN`]) with a `"SERU TRADE"` title and the party lead's **learnable-seru
/// list** - it reads the count ([`SERU_COUNT_VA`]) + ids ([`SERU_IDS_VA`]) live
/// from the character record, looks each id up in the spell display-name table
/// ([`SERU_NAME_PTRS`]), and draws the name with [`TEXT_DRAW_FN`] (native font).
/// CROSS returns to the field; heartbeats the frame counter to [`SENTINEL_ADDR`].
pub fn assemble_draw_overlay() -> Vec<u32> {
    const STR_WORD: usize = 5;
    const COUNTER_WORD: usize = 13;
    let tick_word = (SLOT_A_TICK_OFFSET / 4) as usize; // 14
    let va = |word: usize| SLOT_A_BASE + (word as u32) * 4;
    let (str_va, counter_va) = (va(STR_WORD), va(COUNTER_WORD));

    let s = b"SERU TRADE\0";
    let sw_word = |i: usize| -> u32 {
        let b = |k: usize| -> u8 { s.get(i + k).copied().unwrap_or(0) };
        u32::from_le_bytes([b(0), b(1), b(2), b(3)])
    };

    let mut w = vec![0u32; tick_word];
    // INIT: request mode 13 (MAPDSIP MODE), whose per-frame handler ticks us.
    w[0] = addiu(V0, ZERO, MAPDISP_MODE_INDEX);
    w[1] = lui(AT, hi(MODE_INDEX_VA));
    w[2] = sh(V0, AT, lo(MODE_INDEX_VA));
    w[3] = jr(RA);
    w[4] = nop();
    w[STR_WORD] = sw_word(0);
    w[STR_WORD + 1] = sw_word(4);
    w[STR_WORD + 2] = sw_word(8);

    // Native FUN_80036888(str, 0, 0, x, y): y is the 5th arg -> stack at sp+0x10.
    let draw = |t: &mut Vec<u32>, ptr: u32, x: u16, y: u16| {
        t.push(addiu(V0, ZERO, y));
        t.push(sw(V0, SP, 0x10));
        t.push(lui(A0, hi(ptr)));
        t.push(addiu(A0, A0, lo(ptr)));
        t.push(addiu(A1, ZERO, 0));
        t.push(addiu(A2, ZERO, 0));
        t.push(addiu(A3, ZERO, x));
        t.push(jal(TEXT_DRAW_FN));
        t.push(nop());
    };
    // Native window/box frame: FUN_8002C69C(x, y, w, h) - 4 register args.
    let box_frame = |t: &mut Vec<u32>, x: u16, y: u16, bw: u16, bh: u16| {
        t.push(addiu(A0, ZERO, x));
        t.push(addiu(A1, ZERO, y));
        t.push(addiu(A2, ZERO, bw));
        t.push(addiu(A3, ZERO, bh));
        t.push(jal(BOX_FN));
        t.push(nop());
    };

    // TICK. Frame 0x28: sp+0x10 = stacked text arg (y), sp+0x14/0x18/0x1c = saved
    // s0/s1/s2 (loop vars survive the FUN_80036888 calls), sp+0x20 = saved ra.
    // Text is drawn first (lands in front); the window box last (behind the text).
    let mut t: Vec<u32> = vec![
        addiu(SP, SP, 0xFFD8), // sp -= 0x28
        sw(RA, SP, 0x20),
        sw(S0, SP, 0x14),
        sw(S1, SP, 0x18),
        sw(S2, SP, 0x1C),
    ];
    // Refresh the pad ourselves (mode 13 doesn't poll it) so PAD_CUR_VA is live.
    t.push(jal(PAD_POLL_FN));
    t.push(nop());
    draw(&mut t, str_va, 0x40, 0x30); // title: "SERU TRADE"

    // --- learnable-seru list: for i in 0..min(count, MAX): draw name(ids[i]) ---
    t.push(addiu(S0, ZERO, 0)); // s0 = i = 0
    t.push(lui(AT, hi(SERU_COUNT_VA))); // s1 = count
    t.push(lbu(S1, AT, lo(SERU_COUNT_VA)));
    t.push(slti(T0, S1, (SERU_MAX_ROWS + 1) as i16)); // count <= MAX ?
    let capb = t.len();
    t.push(0); // bne -> .capok (placeholder)
    t.push(nop());
    t.push(addiu(S1, ZERO, SERU_MAX_ROWS)); // else cap
    let capok = t.len();
    t[capb] = bne(T0, ZERO, (capok as i32 - (capb as i32 + 1)) as i16);
    t.push(addiu(S2, ZERO, 0x44)); // s2 = row y
    let lloop = t.len();
    t.push(slt(T0, S0, S1)); // i < count ?
    let endb = t.len();
    t.push(0); // beq -> .ldone (placeholder)
    t.push(nop());
    t.push(lui(T1, hi(SERU_IDS_VA))); // id = ids[i]
    t.push(addiu(T1, T1, lo(SERU_IDS_VA)));
    t.push(addu(T1, T1, S0));
    t.push(lbu(T4, T1, 0));
    t.push(sll(T6, T4, 2)); // id*0xC
    t.push(sll(T7, T6, 1));
    t.push(addu(T6, T7, T6));
    t.push(lui(T7, hi(SERU_NAME_PTRS))); // a0 = *(SERU_NAME_PTRS + id*0xC)
    t.push(addiu(T7, T7, lo(SERU_NAME_PTRS)));
    t.push(addu(T7, T7, T6));
    t.push(lw(A0, T7, 0));
    t.push(sw(S2, SP, 0x10)); // FUN_80036888(name, 0, 0, 0x48, y=s2)
    t.push(addiu(A1, ZERO, 0));
    t.push(addiu(A2, ZERO, 0));
    t.push(addiu(A3, ZERO, 0x48));
    t.push(jal(TEXT_DRAW_FN));
    t.push(nop());
    t.push(addiu(S2, S2, 0xE)); // y += 0xe
    t.push(addiu(S0, S0, 1)); // i++
    t.push(j(va(tick_word + lloop))); // j .lloop (absolute)
    t.push(nop());
    let ldone = t.len();
    t[endb] = beq(T0, ZERO, (ldone as i32 - (endb as i32 + 1)) as i16);

    box_frame(&mut t, 0x28, 0x28, 0xB0, 0x80); // window box (behind the text)

    // Exit on CROSS (held): if (pad & PAD_EXIT_MASK) -> FUN_80026018 (return).
    t.push(lui(AT, hi(PAD_CUR_VA)));
    t.push(lw(T0, AT, lo(PAD_CUR_VA)));
    t.push(andi(T1, T0, PAD_EXIT_MASK));
    let xb = t.len();
    t.push(0); // beq -> .noexit (placeholder)
    t.push(nop());
    t.push(jal(MODE24_RETURN_FN));
    t.push(nop());
    let noexit = t.len();
    t[xb] = beq(T1, ZERO, (noexit as i32 - (xb as i32 + 1)) as i16);
    // Heartbeat: counter++ -> SENTINEL_ADDR (proves the tick is live).
    t.push(lui(AT, hi(counter_va)));
    t.push(lw(V0, AT, lo(counter_va)));
    t.push(addiu(V0, V0, 1));
    t.push(sw(V0, AT, lo(counter_va)));
    t.push(lui(AT, hi(SENTINEL_ADDR)));
    t.push(sw(V0, AT, lo(SENTINEL_ADDR)));
    // Epilogue: restore s0/s1/s2 + ra.
    t.push(lw(RA, SP, 0x20));
    t.push(lw(S0, SP, 0x14));
    t.push(lw(S1, SP, 0x18));
    t.push(lw(S2, SP, 0x1C));
    t.push(addiu(SP, SP, 0x28));
    t.push(jr(RA));
    t.push(nop());
    w.extend(t);
    w
}

/// The two detour words written at the hook: `j STUB_VA` then `nop`.
pub fn detour_words() -> [u32; 2] {
    [j(STUB_VA), nop()]
}

/// The two words planted at [`WARP_INIT_DETOUR_VA`] inside `FUN_80025980`:
/// `j WARP_REDIRECT_VA` then `nop` (replacing its `jal FUN_8003EBE4` + delay).
pub fn warp_init_detour_words() -> [u32; 2] {
    [j(WARP_REDIRECT_VA), nop()]
}

/// Number of disc sectors needed to hold `byte_len` bytes (2048-byte sectors).
pub fn sectors_for(byte_len: usize) -> u16 {
    byte_len.div_ceil(2048) as u16
}

/// Serialize a word list to a little-endian byte blob.
pub fn words_to_bytes(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}
