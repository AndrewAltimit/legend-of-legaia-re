use std::fmt;

/// A decoded instruction.
#[derive(Debug, Clone)]
pub struct Insn {
    /// Byte offset where the instruction starts.
    pub pc: usize,
    /// Total number of bytes consumed (header + operands).
    pub size: usize,
    /// Raw opcode byte (0x80 bit cleared if `extended` is `Some`).
    pub opcode: u8,
    /// Cross-context target script ID, when bit 0x80 was set on the leading byte.
    pub extended: Option<u8>,
    /// Decoded mnemonic + structured payload.
    pub info: InsnInfo,
}

/// Structured payload for each opcode the disassembler recognises.
///
/// Variants carry the operand fields the printer needs for a readable
/// rendering. Variants are intentionally conservative - they hold the
/// operand bytes the VM actually consumes, with extra opcode-specific
/// detail only where it adds value (e.g. FMV index, jump target).
#[derive(Debug, Clone)]
pub enum InsnInfo {
    /// `0x21 / 0x24 / 0x25 / 0x48` no-op cluster.
    Nop,
    /// `0x22 EXEC_MOVE` - schedule move-table playback.
    ExecMove { move_id: u8 },
    /// `0x23 MOVE_TO` - teleport to grid (xb, zb).
    MoveTo { xb: u8, zb: u8 },
    /// `0x26 JMP_REL` - relative jump. `target` is the absolute byte offset.
    JmpRel { delta: u16, target: usize },
    /// Local-flag set/clear/test.
    LFlag { kind: FlagKind, bit: u8 },
    /// Global-flag (story flag) set/clear/test.
    GFlag { kind: FlagKind, bit: u8 },
    /// Context-flag set/clear/test (`ctx.flags`).
    CFlag { kind: FlagKind, bit: u8 },
    /// `0x35 BGM` - 4-byte instruction.
    Bgm { text_id: u16, sub_op: u8 },
    /// `0x37 / 0x41 / 0x47` yield variants.
    Yield { kind: YieldKind },
    /// `0x38 CAM_CFG` - 3-byte camera config.
    CamCfg { op0: u8, op1: u8 },
    /// `0x39 GIVE_ITEM` - add one of inline item `item_id` to the inventory
    /// (`FUN_8004313C` window setup + `FUN_800421D4(item_id, 1)`; dispatcher
    /// `FUN_801DE840` case `0x39`). This is the treasure-chest item-give op; the
    /// earlier `PLAY_SFX` label was wrong (SFX cues go through `FUN_80035B50`).
    GiveItem { item_id: u8 },
    /// `0x3A ADD_MONEY` - 24-bit signed delta.
    AddMoney { signed_24: i32 },
    /// `0x3B SET_ITEM_COUNT`.
    SetItemCount { slot: u8, count: u8 },
    /// `0x3C PARTY_ADD`.
    PartyAdd { char_id: u8 },
    /// `0x3D PARTY_REMOVE`.
    PartyRemove { char_id: u8 },
    /// `0x42 COND_JMP` - conditional jump on flags / screen mode. `target`
    /// is the absolute byte offset of the jump destination when taken.
    CondJmp {
        mode: u8,
        op1: u8,
        delta: u16,
        target: usize,
    },
    /// `0x3E` - WARP (op0 >= 100) / INTERACT.
    WarpOrInteract { op0: u8, op1: u8, is_warp: bool },
    /// `0x46 RENDER_CFG`.
    RenderCfg {
        long: bool,
        op0: u8,
        bytes: [u8; 5],
        used: usize,
    },
    /// `0x4F SCENE_REGISTER_WRITE`.
    SceneRegisterWrite { b0: u8, b1: u8, b2: u8 },
    /// `0x44 SPAWN_RECORD` - spawn a MAN partition-2 record as a new
    /// field-VM context (`FUN_8003BDE0(0, 0, global_index - N0 - N1, 1)`).
    SpawnRecord { global_index: u8 },
    /// `0x4B ANIMATE` - variable-length keyframe block.
    Animate { count: u8, base_id: u8 },
    /// `0x36 SCENE_FADE`.
    SceneFade { word0: u16, word1: u16 },
    /// `0x45 CAMERA` - sub-dispatched.
    Camera { op0: u8, kind: CameraKind },
    /// `0x4D BBOX_TEST` - inside-the-box → next instr; outside → forward jump.
    BBoxTest {
        x_min: u8,
        z_min: u8,
        x_max: u8,
        z_max: u8,
        skip_delta: u16,
        skip_target: usize,
    },
    /// `0x3F` - **named scene-change** ("warp by name").
    ///
    /// Copies a length-prefixed destination scene *name* from the bytecode and
    /// hands it to the scene-change packet (`FUN_8001FD44`, which writes the
    /// name into the active scene-name buffers `0x8007050C`/`0x80084548`), then
    /// sets the destination entry tile + facing. Operand layout (after the
    /// opcode byte): `[i16 index][u8 name_len][name_len name bytes][entry_x]`
    /// `[entry_z][dir]`, so the instruction is `header + 6 + name_len` bytes.
    ///
    /// The destination name is a *slice of the bytecode* at `operand + 3`, not
    /// carried inline here; recover it with [`scene_change_name`]. This is
    /// **not** a dialog opcode - field dialogue is the `0x4C` nibble-5 sub-3/4
    /// path ([`MenuCtrlKind::Nibble5Dialog`]); only the over-approximating walk
    /// desyncing on a literal `?` (`0x3F`) in message text makes it *look* like
    /// one in text-heavy records.
    SceneChange {
        /// Sign-extended `i16` at `operand[0..2]` - the destination's
        /// story/entry index (`FUN_8003CE9C` read). Not the 7-id door-warp
        /// `map_id`; distinct id space (observed values reach 155+).
        index: i16,
        /// Length of the inline destination-name string at `operand + 3`.
        name_len: u8,
        /// Entry tile X byte (`& 0x7F` = tile, `& 0x80` = +half-tile).
        entry_x: u8,
        /// Entry tile Z byte (same encoding as `entry_x`).
        entry_z: u8,
        /// Facing/depth selector (`& 7` indexes the entry-direction table).
        dir: u8,
    },
    /// `0x40 DATA_BLOCK`.
    DataBlock { len: u8 },
    /// `0x4A WAIT_FRAMES`.
    WaitFrames { target: u16 },
    /// `0x4E` inventory comparison-and-jump (sub-dispatched).
    InventoryCmp {
        page: u8,
        mode_byte: u8,
        kind: InventoryCmpKind,
    },
    /// `0x49 STATE_RESUME` (sub-dispatched).
    StateResume { sub_op: u8, kind: StateResumeKind },
    /// `0x34` EFFECT (sub-dispatched).
    Effect { op0: u8, kind: EffectKind },
    /// `0x43` ACTOR_CTRL (sub-dispatched).
    ActorCtrl { sub_op: u8, kind: ActorCtrlKind },
    /// `0x4C` MENU_CTRL (sub-dispatched).
    MenuCtrl { op0: u8, kind: MenuCtrlKind },
    /// `0x5x / 0x6x / 0x7x` system flag set/clear/test (4-bank flags).
    SystemFlag {
        kind: FlagKind,
        idx: u16,
        delta: Option<u16>,
        target: Option<usize>,
    },
    /// One opaque byte. Used as a fallback when the decoder cannot make
    /// sense of the leading byte; the caller resumes one byte later.
    Byte { value: u8 },
}

/// Kind discriminator for set / clear / test flag opcodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlagKind {
    Set,
    Clear,
    Test,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YieldKind {
    /// `0x37 / 0x41` yields - resume at `pc + header + 2`.
    Standard,
    /// `0x47` yield - resume at `pc + header + 3`.
    Wide,
}

#[derive(Debug, Clone)]
pub enum CameraKind {
    Load,
    Save,
    Apply { abs_target: usize },
    Configure { mask: u16, apply_trigger: u16 },
}

#[derive(Debug, Clone)]
pub enum InventoryCmpKind {
    /// Sub-ops 0..=9 - the shared 7-byte compare-and-skip form. The raw
    /// jump table at `0x801CEE30` routes every sub-op to a value loader
    /// that joins the shared compare at `0x801E0B40` (only 0/1 apply the
    /// `factor * arg >> 8` scaling). Value source per sub-op: 0/1 =
    /// char-record HP/MP fraction-of-max, 2 = char level byte `+0x130`,
    /// 3 = party gold `_DAT_8008459C`, 4 = BIOS `Rand() & 0xFF` (a random
    /// chance branch), 5..=8 = slot table `0x801C6460[sub - 5]`, 9 = coin
    /// bank `_DAT_800845A4`. The earlier "absolute jump" (5..8) and
    /// "rand -> next PC" (4) readings were the Ghidra decomp's collapsed
    /// switch arms and are falsified by the raw loader bodies at
    /// `0x801E0AC0..0x801E0B34`.
    Compare {
        sub_op: u8,
        arg: u16,
        skip_delta: u16,
        skip_target: usize,
    },
    /// Sub-op 10/11 - 9-byte party-bank comparison.
    PartyBank {
        sub_op: u8,
        scaled: i32,
        skip_delta: u16,
        skip_target: usize,
    },
    /// Sub-op 12..=15 - dispatcher's default arm (PC += 7).
    DefaultAdvance,
}

#[derive(Debug, Clone)]
pub enum StateResumeKind {
    /// Done arm, sub-op 0 - inline MES bytecode walker.
    DoneSub0Mes { length: u8, mes_bytes: usize },
    /// Done arm, sub-ops 1/3/7 (PC += 3).
    DoneSubShort,
    /// Done arm, sub-ops 2/4 (PC += 7).
    DoneSubMid,
    /// Done arm, sub-op 5 (PC += 14).
    DoneSubLong,
    /// Done arm, sub-ops 6/8/9/0xC/0xD (PC += 4).
    DoneSubAdvance4,
    /// Idle / Armed arm - the decoder doesn't try to track runtime state,
    /// so it always reports the encoded sub-op width.
    Encoded { sub_op: u8 },
}

#[derive(Debug, Clone)]
pub enum EffectKind {
    /// Sub-0: 7-byte color + intensity setup.
    ColorIntensity { rgb: [u8; 3], intensity: i16 },
    /// Sub-1: 13-byte spawn (or 13 + 2 + payload_len with capture flag).
    Spawn { has_capture: bool, payload_len: u8 },
    /// Sub-2: 3-byte capture-yield.
    CaptureYield { b1: u8 },
    /// Sub-3: 2-byte 3D anim trigger.
    AnimTrigger { arg: u8 },
}

#[derive(Debug, Clone)]
pub enum ActorCtrlKind {
    /// Halt-acquire dispatcher (sub-0 / sub-1: 5 bytes, sub-A / sub-B: 9 bytes).
    HaltAcquire { sub_op: u8, target_offset_pc: usize },
    /// Sub-2: 8-byte 3-actor talk.
    ThreeActorTalk {
        actors: [u8; 3],
        arg_word: u16,
        b6: u8,
    },
    /// Sub-3..=6: 10-byte sound register ramp.
    SoundRegisterRamp {
        sub_op: u8,
        bytes: [u8; 4],
        ticks: u16,
        curve: u16,
    },
    /// Sub-7: 17-byte face / body rotation setup.
    FaceRotation { face_id: u8, target: i16 },
    /// Sub-8: 2-byte face reset.
    FaceReset,
    /// Sub-9: 10-byte tween / immediate position write.
    Sub9Tween { x: u16, y: u16, z: u16, ticks: u16 },
    /// Sub-C: 5-byte allocate scripted actor.
    AllocScripted { b: [u8; 3] },
    /// Sub-D / Sub-F: 6-byte allocate actor with mode.
    AllocActorMode { sub_op: u8, b: [u8; 4] },
    /// Sub-E: 2-byte mark currently-iterating actor flag bit 8.
    MarkActorFlag8,
    /// Sub-0x10: 21-byte sprite-widget spawn (PROT-0900 family,
    /// `FUN_801F8004`; inline 19-byte record).
    WidgetSpriteSpawn,
    /// Sub-0x11: 12-byte screen-mask (iris) rect tween
    /// (`FUN_801F8D4C(l, t, r, b, dur)`).
    WidgetMaskRect { words: [u16; 5] },
    /// Sub-0x12 (VRAM rect copy via `FUN_800468A4`) / Sub-0x13 (image-panel
    /// spawn `FUN_801F88FC`): 14-byte 6-word payloads.
    WidgetPayload14,
    /// Sub-0x14: 10-byte panel move/scale (`FUN_801F8E6C(x, y, scale, dur)`).
    WidgetPanelMove { words: [i16; 4] },
    /// Sub-0x15: 14-byte letterbox config (`FUN_801F8F28`).
    WidgetLetterbox,
}

#[derive(Debug, Clone)]
pub enum MenuCtrlKind {
    /// Outer nibble 0: party leader change.
    PartyLeader { leader_id: u8 },
    /// Outer nibble 1: 7-byte menu/effect dispatcher.
    Menu1 { payload: [u8; 5] },
    /// Outer nibble 2: party view swap (op0 & 7).
    PartyViewSwap { op0_low3: u8 },
    /// Outer nibble 3: per-bit sub-ops; mostly 2-byte simple writes.
    Nibble3 { sub: u8 },
    /// Outer nibble 4: 6-byte immediate-or-ramp cluster (or 11-byte sub-5).
    Nibble4 { sub: u8, target: i16, ticks: u16 },
    /// Outer nibble 4 sub-5 - wider 11-byte form.
    Nibble4Sub5 {
        b1: u8,
        w94: i16,
        w96: i16,
        w98: i16,
        ticks: u16,
    },
    /// Outer nibble 5 sub-0 - 4-byte sound-directional.
    Nibble5Sub0 { value: i16 },
    /// Outer nibble 5 sub-1 - 6-byte NPC / player move-to-tile with run
    /// dispatch. Operand bytes are `[x_enc, z_enc, depth, move_id]`; tile
    /// coords use the same `(byte & 0x7F)<<7 + (bit7 ? 0x80 : 0x40)` decode
    /// the placement header uses, and the `is_player` selector reads
    /// `ctx.flags & 0x0100_0000`. Drives the step handler's
    /// `op4c_n5_sub1_npc_run` host hook.
    Nibble5NpcRun {
        x_enc: u8,
        z_enc: u8,
        depth: u8,
        move_id: u8,
    },
    /// Outer nibble 5 sub-2 - 3-byte menu-activation poll. Operand byte is the
    /// `menu_id`; the script halts at PC until the host's
    /// `op4c_n5_sub2_menu_activation` hook returns true.
    Nibble5MenuPoll { menu_id: u8 },
    /// Outer nibble 5 sub-3 / sub-4 - 2-byte dialog poll.
    Nibble5Dialog { sub: u8 },
    /// Outer nibble 6 sub-0 - 14-byte 6-word emitter call.
    Nibble6Emitter6 { words: [i16; 6] },
    /// Outer nibble 6 sub-1 - 16-byte scripted **CLUT-cell effect**
    /// (`FUN_801E4C58`): `frames == 0` is a one-shot 16x1 cell write (copy
    /// cell `b` -> `dest`, or flat-fill `dest` with colour `b.0` when
    /// `b.1 == 0`); `frames != 0` spawns the CLUT cross-fade actor
    /// (`FUN_801E4794`) fading `a` toward `b` into `dest` over `frames`
    /// vsyncs. Cells are `(x, y)` VRAM framebuffer coordinates.
    Nibble6ClutFx {
        a: (i16, i16),
        b: (i16, i16),
        dest: (i16, i16),
        frames: i16,
    },
    /// Outer nibble 7 - 7-byte VRAM tile-flag bulk operation.
    Nibble7Tile {
        sub: u8,
        x0: u8,
        z0: u8,
        x1: u8,
        z1: u8,
        mask: u8,
    },
    /// Outer nibble 8 - heterogeneous sub-ops.
    Nibble8 { sub: u8, encoded_size: usize },
    /// Outer nibble 0xA / 0xB / 0xC / 0xD / 0xE / 0xF - heterogeneous.
    HighNibble {
        outer: u8,
        sub: u8,
        encoded_size: usize,
    },
    /// Outer nibble 0xE sub-2 - **FMV trigger** (the 0x4C 0xE2 op).
    FmvTrigger { fmv_id: i16 },
}

/// Why the decoder couldn't decode an instruction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DisasmError {
    /// PC is past the end of the buffer.
    EndOfStream { pc: usize },
    /// The opcode byte is recognised but the operand bytes lie past the
    /// buffer end.
    Truncated { pc: usize, opcode: u8 },
    /// A sub-op variant the decoder doesn't know how to size. Caller
    /// typically prints a `.byte` line and resumes one byte later.
    UnknownSubOp { pc: usize, opcode: u8, sub_op: u8 },
    /// Top-level opcode falls outside the field VM's documented range.
    UnknownOpcode { pc: usize, opcode: u8 },
}

impl fmt::Display for DisasmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EndOfStream { pc } => write!(f, "end of stream at pc=0x{pc:04X}"),
            Self::Truncated { pc, opcode } => write!(
                f,
                "truncated operand for opcode 0x{opcode:02X} at pc=0x{pc:04X}"
            ),
            Self::UnknownSubOp { pc, opcode, sub_op } => write!(
                f,
                "unknown sub-op 0x{sub_op:02X} for opcode 0x{opcode:02X} at pc=0x{pc:04X}"
            ),
            Self::UnknownOpcode { pc, opcode } => {
                write!(f, "unknown opcode 0x{opcode:02X} at pc=0x{pc:04X}")
            }
        }
    }
}

impl std::error::Error for DisasmError {}
