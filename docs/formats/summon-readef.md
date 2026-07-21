# summon.dat / readef.DAT - battle side-band streaming slots

`\data\battle\summon.dat` and `\data\battle\readef.DAT` are the two battle
side-band streaming files (CDNAME block `bat_back_dat`): per-special-attack
VRAM texture pages plus summon-creature actor records, streamed from disc
mid-battle in fixed `0x10800`-byte (33-sector) slots while a cast plays.

Parser: `crates/asset/src/summon_readef.rs`. Confidence: **Confirmed**
(byte-verified RAM↔disc and VRAM↔disc in a mid-cast battle save state).

## PROT entries and how the dev paths resolve

The battle overlay's streaming handler `FUN_801F17F8` opens both files via the
file-open shim `FUN_800558FC(path, 0, 0, prot_index)`:

```c
uVar5 = func_0x800558fc(s_data_battle_readef_DAT_801f64cc, 0, 0, 0x380);
uVar5 = func_0x800558fc(s_data_battle_summon_DAT_801f64b4, 0, 0, 0x37f);
```

In the retail build the ISO9660 open `FUN_800608F0` is a trap stub and
`_DAT_8007B8C2 != 0` (verified live: the halfword is `1` in battle save
states), so `FUN_800558FC` ignores the path string entirely and passes its
**fourth argument straight to `FUN_8003E8A8` as a PROT TOC index**. The dev
path is a debug-build referent only.

`FUN_8003E8A8` resolves `start_lba = word[(idx + 2) * 4 + 0x801C70F0]` against
the in-RAM TOC copy. The boot TOC loader (`FUN_8003E4E8`) fills `0x801C70F0`
with PROT.DAT's first 3 sectors **verbatim, 8-byte header included**, while
`legaia_prot::archive::Archive` strips the header and indexes entry `p` at
`toc[p + 2]` - so a retail TOC index maps to the extraction-space entry index
**minus 2**:

| File | Retail TOC index | Extraction entry | Footprint | Slots |
|---|---|---|---|---|
| `summon.dat` | `0x37F` | **893** | 6 961 152 B | exactly 103 × `0x10800` |
| `readef.DAT` | `0x380` | **894** | 5 271 552 B | exactly 78 × `0x10800` |

Verification (executed comparisons, not inference):

- The raw TOC words at `(0x37F + 2)` / `(0x380 + 2)` equal the extraction
  entries' start LBAs and footprints (asserted by the disc-gated
  `summon_readef_real` test).
- In the `battle_gimard_tail_fire_a` save state the entire 67 584-byte stream
  buffer at `*0x8007BD74` is byte-identical to extraction entry 894 at offset
  `1 * 0x10800` (slot 1).
- Slot 0 of entry 894 (mode 2) matches the same state's VRAM byte-for-byte:
  512-byte CLUT row at `(0, 488)` and 65 536-byte texture page at `(512, 0)`
  (128 halfwords × 256 rows).

The same `idx + 2` arithmetic applies to every `FUN_8003E8A8` consumer
(including the overlay loaders' `param + 0x381`), so CDNAME `#define` numbers
live in the retail index space, not the extraction space - see
[`cdname.md`](cdname.md#numbering-space).

## Streaming state machine

The battle scene loader (`FUN_800520F0`) case `0xFF` dispatches `FUN_801F17F8`
each frame. Two coupled state machines live in the battle context at
`*0x8007BD24`:

- **Transfer SM** (`+0x26B` request byte, `+0x26C` stage): stage 1 opens the
  file selected by bit 7 of `(request - 1)` (set → `summon.dat`, clear →
  `readef.DAT`), seeks `((request - 1) & 0x7F) * 0x10800` bytes past the entry
  start (`FUN_80055A5C` → `FUN_8003E964`, sector-granular relative to the
  `FUN_8003E8A8`-saved base MSF) and reads `0x10800` bytes into `*0x8007BD74`
  (`FUN_800559EC` → `FUN_8003E800`); stage 2 closes. `FUN_80055B4C(slot)`
  arms a request (`+0x26B = slot + 1`).
- **Applier SM** (`FUN_801F12D0`, `+0x276` stage, `+0x277` base slot byte):
  odd stages request slots `base`, `base+1`, `base+2`, `base+3`; even stages
  consume the arrived slot.

Three sites feed the SMs (decomps `overlay_battle_action_801daba4.txt`,
`overlay_battle_action_801e295c.txt`):

- **Per turn** the initiative scheduler `FUN_801DABA4` seeds `+0x277` with
  the acting entity's group base - party actor: `3*(char−1)` from
  `DAT_8007BD10`; enemy actor: `3 * monster_record[+0x1C]` (each monster
  record carries its readef group byte at `+0x1C`); an AI-delegated party
  attacker substitutes the delegate's group - and sets `+0x276 = 1`. For
  readef groups the applier stops after `base+1`, leaving the character's
  **main "ME" archive** resident for the turn.
- **At battle end** the win-pose staging requests slot `3*char + 2` (the
  **base "ME" archive**) directly through `FUN_80055B4C`, bypassing the
  applier: `FUN_801DABA4`'s no-living-enemy branch and `FUN_801E295C`'s
  victory arm, which re-rolls `ctx+0x13` onto a living party member **only when
  the acting actor is dead** - a living acting actor skips the re-pick at
  `0x801E6690` (the hole behind the enemy-ally charm softlock; see
  [`battle.md`](../subsystems/battle.md#enemy-ally-charm-at-the-end-of-action-gate-the-charm-battle-softlock)).
  See
  [`battle-data-pack.md` § "ME" stream archives](battle-data-pack.md#me-stream-archives-readefdat)
  for the archive-side consequence (win-pose records read the base archive,
  everything else the main).
- **At cast time** the base slot byte is computed by the battle-action SM
  `FUN_801E295C` (case `0x32` of the cast sequence) from the actor's action
  id (`actor + 0x1DF`):

```text
id <  0x9A:  base = 3 * (id - 1)    (mod 256)
id >= 0x9A:  base = 4 * id + 0x63   (mod 256)
```

Bit 7 of `base` selects the file; `base & 0x7F` is the starting slot. Both
forms above are the *byte-truncated* result - the instructions compute
something that looks different but is congruent mod 256, see
[the slot map in instructions](#the-slot-map-in-instructions).

The id bands tile both files exactly:

| Action ids | File | Group shape | Slots |
|---|---|---|---|
| `0x01..=0x1A` | `readef.DAT` | 3 slots (`[texture, aux, aux]`; the all-texture band `base 0x0C..=0x36` ships two texture pages) | 26 × 3 = 78 |
| `0x81..=0x99` | `summon.dat` | 3 slots (`[texture, texture, actor record]`) | 25 × 3 = 75 |
| `0x9A..=0xA0` | `summon.dat` | 4 slots (`[texture, texture, raw CLUT+texture+part pool, actor record]`) | 7 × 4 = 28 |

`readef.DAT` sequences stop after the second slot (the applier resets unless
bit 7 is set or `base == 0x36`, the one readef id with a four-slot group);
the second texture upload is also skipped for `base < 0x0C` and
`base 0x37..=0x41`. Summon group 0 (spell id `0x81`, Gimard) carries the
"Burning Attack" actor record - consistent with the
[spell table](spell-table.md)'s player Seru-magic block.

### The slot map in instructions

`FUN_801E295C` splits on `sltiu v0,v0,0x9a` and stores the result with `sb`,
so both arms are only ever meaningful modulo 256:

```text
801e499c  lbu   v0,0x1df(s3)      ; the action id
801e49a4  sltiu v0,v0,0x9a
801e49a8  beq   v0,zero,0x801e49d0

; low arm (id < 0x9A)
801e49b8  addiu v1,v1,-0x81
801e49bc  sll   v0,v1,0x1
801e49c0  addu  v0,v0,v1          ; 3 * (id - 0x81)
801e49cc  addiu v0,v0,-0x80       ; ... minus 0x80

; high arm (id >= 0x9A)
801e49dc  sll   v0,v0,0x2
801e49e0  addiu v0,v0,-0x29d      ; 4 * id - 0x29D

801e49e4  sb    v0,0x277(v1)      ; stored as a BYTE
```

The two documented forms follow by congruence, and the check matters because
the instruction constants share no digits with them:

- Low arm: `3*(id - 0x81) - 0x80 ≡ 3*(id - 1)  (mod 256)`, since the two differ
  by `3*0x80 - (-0x80) = 0x200`. Spot-check `id = 0x81 → 0x80` and
  `id = 0x99 → 0xC8` from either form.
- High arm: `4*id - 0x29D ≡ 4*id + 0x63  (mod 256)`, since `0x29D + 0x63 = 0x300`.

The `-0x80` on the low arm is also **where bit 7 comes from**. The file-select
bit is not a separate flag the code sets; it falls out of the same expression,
which is why the whole `0x81..=0x99` band lands in `summon.dat` automatically.
The applier reads the byte back signed (`lb`) precisely to test that bit:

```text
801f1644  lb   v0,0x277(a0)
801f164c  bltz v0,0x801f17e4      ; bit 7 set -> summon.dat, keep streaming
801f1650  li   v0,0x36
801f1654  beq  v1,v0,0x801f17e4   ; the one readef four-slot group
801f1660  sb   zero,0x276(a0)     ; otherwise stop after base+1
```

(`FUN_801F12D0`, from `overlay_muscle_dome_801f12d0.txt` - see the dump warning
in [Tooling](#tooling).) The `base+2` and `base+3` arms at `801f1678` /
`801f179c` both `jal 0x80055b4c`, the staging arm.

## Slot formats

### Texture slot (`u32 mode` ∈ {0, 1, 2})

```text
+0x000  u32  mode
+0x004  CLUT rows - 256 BGR555 entries each (mode 1: 2 rows, else 1 row)
+0x204 / +0x404  4bpp texture page, 256 rows tall:
        mode 0: 64 halfwords wide (0x8000 bytes)
        mode 1: 128 halfwords wide (0x10000 bytes), CLUTs at +4..+0x404
        mode 2: 128 halfwords wide (0x10000 bytes)
```

VRAM targets are positional (`FUN_801F12D0` cases 2 / 4): the group's first
texture slot → CLUT at `(0, 488)` + page at `(512, 0)`; the second → CLUT at
`(0, 490)` + page at `(640, 0)`.

### Big-summon raw slot (3rd slot, `base >= 0xCB` only)

Consumed headerless by case 6 - the three regions tile the slot exactly
(`0x1E0 + 0x8000 + 0x8620 = 0x10800`):

```text
+0x0000  240 BGR555 entries - STP bit forced on non-zero entries,
         uploaded to VRAM (0, 486) as a 240×1 rect
+0x01E0  64×256-halfword texture page → VRAM (448, 256)
+0x81E0  0x8620-byte part pool → RAM *0x8007B85C + 0x44000
         (the summon creature's part data for the off-band fixup arm)
```

### Actor-record slot (last streamed slot of a group)

Consumed in place by `FUN_801F19EC` (offsets are slot-relative; the installer
adds the buffer base to each):

```text
+0x00  u32  name offset    - NUL-terminated attack-name string
+0x04  u32  TMD offset     - Legaia TMD, magic 0x80000002 (every record
                             in the corpus passes the magic check)
+0x08  u32  texture-pool offset
+0x4A  u8   part count
+0x4C  u32[part_count]  per-part offsets (each part gets *(p+0x88) = p+0x8C
       and indirection fixups at p+4 / p+8 through the table that follows
       the part offsets)
```

`FUN_801F19EC` then routes the TMD + texture pool through `FUN_80055468` - the
same mesh/texture installer the [monster archive](../formats/monster-animation.md)
uses - and stages the summon creature as a battle actor (`FUN_80024C88`
allocation, scale `(part_pool_byte_0x1F) << 5`, etc.).

For the **base + evolved-Seru summons (`0x81..=0x95`)** this actor-record TMD is
**byte-identical** to a record in the `battle_data` monster archive (PROT 867):
the summon reuses an ordinary enemy creature's mesh. Matching each group's
actor-record TMD against the archive by longest-common-prefix recovers the full
spell → creature map (e.g. Gimard `0x81`→ archive id 10; the otherwise
capture-less evolved legs `0x90`→ Kemaro 144, `0x91`→ Spoon 147). The map lives
in `legaia_asset::summon_creatures` and is byte-validated by the disc-gated
`summon_creature_tmd_map_real`. The **big-summon block `0x9A..=0xA0`** instead
carries a **bespoke mesh** in the group's third (raw CLUT+texture+part-pool)
slot - no archive byte-match - so those summons are not reused enemy bodies.
See [`open-rev-eng-threads.md`](../reference/open-rev-eng-threads.md) (Seru-magic
summon visual).

### Art `"ME"` stream-archive slot (readef groups 0..3)

The aux slots of `readef.DAT` groups 0..3 - slots `3*char + 1` and
`3*char + 2` for char = Vahn / Noa / Gala / Terra - carry the player
**art-animation keyframe-stream archives** at the slot head: magic
`'M' 'E'`, `u8 count`, `u16 entry_sizes[count]` (bit 15 = compressed),
concatenated bodies. The consumer is `FUN_8002B28C` (called by the anim
commit `FUN_8004AD80` with the `*0x8007BD74` streaming buffer as the
archive), and every retail entry decompresses through the channel-delta
codec `FUN_8002A9CC` into a packed
`[u8 parts][u8 frames][9-byte TRS]` stream - the art-bank side is decoded
in [`battle-data-pack.md` § "ME" stream
archives](battle-data-pack.md#me-stream-archives-readefdat). Parser
`legaia_asset::me_archive`; the side-band classifier reports these as
`SlotKind::MeArchive`. Slot `3*char` is the group's non-ME (texture) slot.

The aux slots of the **higher** readef groups (the enemy-special bands) are
still unattributed as content; the bytes LZS-decode plausibly but no consumer
is pinned. The group *selection* is now traced - each monster record names
its readef group at byte `+0x1C`, staged per enemy turn by `FUN_801DABA4`
(see [Streaming state machine](#streaming-state-machine)) - which narrows
the hunt to that group's `base+1` slot
(see [open threads](../reference/open-rev-eng-threads.md)).

## Tooling

`asset summon-readef <entry.BIN>` lists every slot's class (texture / actor
record / ME stream archive / payload), texture layout, and attack-name string; `--texture-png-dir`
decodes each texture slot's 4bpp page through a CLUT-row window (`--clut-sub`)
to PNG; `--action-id` resolves an action id to its `(file, slot)` stream
target. The attack-name sequence across the `summon.dat` actor records follows
the spell-id order exactly (slot group 0 = the `0x81` cast, group 1 = `0x82`,
…), independently corroborating the case-`0x32` banding; `readef.DAT` slot 0's
page decodes to a legible dev "back read test" texture.

> **Which `FUN_801F12D0` dump to read.** Use
> `ghidra/scripts/funcs/overlay_muscle_dome_801f12d0.txt` (330 instructions,
> proper prologue). The sibling `overlay_0897_801f12d0.txt` is a **VA-aliased
> mid-function fragment**: it opens with `lw v1,-0x6c84(v0)` and no
> `addiu sp,sp,-N`, yet closes restoring `s0`-`s3` and `ra` it never saved. Its
> body contains none of the slot-streaming logic described above, so anyone
> re-walking this thread from it will conclude the applier does something else
> entirely.

## See also

- [`effect.md`](effect.md) - the resident efect.dat 2-pack this side-band
  channel complements.
- [`prot.md`](prot.md) - TOC math and the in-RAM index space.
- [`subsystems/effect-vm.md`](../subsystems/effect-vm.md) - the effect pool
  and the streaming handler's place in it.

Provenance: `ghidra/scripts/funcs/overlay_battle_801f17f8.txt`,
`overlay_muscle_dome_801f12d0.txt`, `overlay_muscle_dome_801f19ec.txt`,
`800558fc.txt`, `80055a5c.txt`, `800559ec.txt`, `80055b4c.txt`,
`8003e8a8.txt`, `8003e964.txt`, `8003e4e8.txt`,
`overlay_magic_capture_801e295c.txt`, `overlay_battle_action_801daba4.txt`.
