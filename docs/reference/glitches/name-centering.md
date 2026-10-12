# Off-centre attack names

Three related presentation defects in the battle name captions of the US release: the "Ultimate
Rave" caption of a failed Lippian Flute, the sibling captions that share its cause, and the Super
Art / Miracle Art finisher banner. Every position is derived from the disc's code and data; none is
an emulator capture, and predictions are marked as such. Pixel positions are on the 320-pixel-wide
battle screen.

## How a battle name is placed

The width routine `FUN_80035F04` in `SCUS_942.54` sums one advance per byte: `widths[c] + 1` from
the table at `0x80073F1C` (see [dialog-font.md](../../formats/dialog-font.md)), with an icon escape
(`0xCE`) adding the advance byte of its record at `0x80074050`. A correctly placed name gets `x =
0xA0 - (width >> 1)` (`sra v0,v0,1; li v1,0xa0; subu`). "Centred X" below is that formula applied to
the string actually drawn.

Two draw paths carry attack names. Summon cast modules (see
[cast-module.md](../../subsystems/cast-module.md)) call the text-label routine `FUN_8003541C`, whose
fourth argument is the pen X (stored at node `+0x0A` at `0x8003556C`). Arts, items and monster moves
write the X into four halfwords of the HUD placement table at `0x80076C10` (`+0x722`, `+0x72A`,
`+0x73A`, `+0x742`, records `0x4C` / `0x4D`; see [battle-hud.md](../../subsystems/battle-hud.md))
and raise the banner through `FUN_801D8DE8` in PROT entry 0898.

The width model reproduces the one banner position the battle notes record from a capture:
"Poisonous Sting" sums to 86 px, giving `x = 117`.

## "Ultimate Rave" is drawn off-centre

**Status:** explained
**Reported as:** After a Lippian Flute fails to kill, the attack name changes from a centred "Ultimate Death" to an "Ultimate Rave" that sits too far left (community report).

The Lippian Flute summons Lippian, whose attack tries an instant kill with a 1-in-128 chance. While
the attack plays the caption reads **Ultimate Death**. If the kill does not land, the game swaps the
caption for **Ultimate Rave**.

The two captions are drawn by two different pieces of code in the same cast module. The first one
measures its text and subtracts half the width from the screen centre, like almost every other name
in battle. The second one never measures anything: its left edge is the number **100**, written
directly into the instruction that draws it.

A left edge of 100 is right for a caption 120 pixels wide. "Ultimate Rave" is 78 pixels wide in the
game's proportional font, so it runs from 100 to 178 and its middle sits at 139 instead of 160.
Measured and centred it would start at 121: the caption is **21 pixels too far left** on a 320-pixel
screen.

This is read from the disc's code and data, not captured on an emulator. The community screenshot
that prompted the trace agrees with the prediction: its caption starts about 99 native pixels in and
is about 78 wide.

A plausible origin, not provable from the US disc alone: 120 pixels is ten cells of a 12-pixel
fixed-width font, which fits a ten-character Japanese name whose position was worked out by hand and
never recomputed for the English text.

| Caption | Width | Drawn X | Drawn span | Drawn centre | Centred X | Error |
|---|---|---|---|---|---|---|
| Ultimate Death | 83 | 119 | 119..202 | 160.5 | 119 | 0 |
| Ultimate Rave | 78 | 100 | 100..178 | 139 | 121 | -21 px |

### Mechanism

1. The cast module rolls the kill once at the start: random number AND 0x7F, kill only on zero. (`0x801F6B0C`)
2. First caption: the spell name from the spell table, measured, X = 0xA6 - width/2. (`0x801F7020`)
3. Second caption: the summon creature's attack name ("Ultimate Death"), measured, X = 0xA0 - width/2 = 119. (`0x801F70F4`)
4. Near the end the module tests the kill flag; if the kill landed no new caption is drawn. (`0x801F74E8`)
5. Otherwise it destroys the old caption and loads the module's own head string with X = 0x64 as a literal. (`0x801F7508`)
6. The text routine puts the pen exactly at that X, so the 78 px string covers 100..178. (`FUN_8003541C`)

The centred caption, in `FUN_801F6A18` of PROT entry 0924:

```
801f70c0  jal   0x80035f04      ; measure the name
801f70d4  sra   v0,v0,1
801f70dc  addiu a3,zero,0xa0
801f70f4  jal   0x8003541c
801f70f8  subu  a3,a3,v0        ; x = 160 - width/2
```

The fixed caption:

```
801f74e0  lw    v0,0x7c4c(v0)   ; kill flag
801f74e8  bnez  v0,0x801f7544   ; kill landed: no new caption
801f74f0  jal   0x800319a8      ; remove the old caption
801f7504  addiu a2,a2,0x69d8    ; string at 0x801F69D8
801f7508  addiu a3,zero,0x64    ; x = 100
801f750c  addiu v0,zero,0x96    ; y = 150
801f751c  jal   0x8003541c
```

### Addresses

| Address | Image | What | Evidence |
|---|---|---|---|
| `0x801F69D8` | PROT 0924 | Head of the Lippian flute cast module: the "Ultimate Rave" string (file offset 0) | disc data |
| `0x801F69E8` | PROT 0924 | 12-entry phase jump table that follows the string | disc data |
| `0x801F6B0C` | PROT 0924 | Kill roll: `jal 0x80056798` (rand), then `andi v0,v0,0x7f` at `0x801F6B14`; the kill flag is raised only on 0 | disassembly |
| `0x801F7C4C` | PROT 0924 | Kill flag (non-zero = the 1-in-128 kill landed) | disassembly |
| `0x801F7020` | PROT 0924 | Caption 1: spell name, X = `0xA6 - (width >> 1)` | disassembly |
| `0x801F70C0` | PROT 0924 | Caption 2 measure call; the draw at `0x801F70F4` uses X = `0xA0 - (width >> 1)` ("Ultimate Death") | disassembly |
| `0x801F74E8` | PROT 0924 | `bnez v0` on the kill flag: caption 3 is skipped when the kill landed | disassembly |
| `0x801F7508` | PROT 0924 | `addiu a3,zero,0x64` - the hard-coded X = 100 (file offset `0xB30`, word `0x24070064`) | disassembly |
| `0x801F751C` | PROT 0924 | Caption 3 draw call for the head string, Y = `0x96` | disassembly |
| `0x8003541C` | SCUS_942.54 | Text-label register-and-draw; argument 4 is the pen X, stored at node `+0x0A` | disassembly |
| `0x800319A8` | SCUS_942.54 | Text-label destroy (removes the previous caption) | repo doc |
| `0x80035F04` | SCUS_942.54 | String width measure | disassembly |
| `0x80073F1C` | SCUS_942.54 | Proportional width table, `u8[256]`; a glyph advances `width + 1` in battle | disc data |
| `0x800754C8` | SCUS_942.54 | Spell table, stride `0xC`, name pointer at `+8` | disc data |
| `0x801C9358` | RAM | Record pointer of battle slot 7 (the summon creature); its first word points at the attack name | repo doc |
| slot 65 | PROT 0893 (summon.dat) | Lippian actor record, attack name "Ultimate Death" | disc data |

### Scope

Only the failed outcome of the Lippian Flute shows this caption. The same shortcut exists in three
other places, listed under the sibling captions.

### Verifying it

Break on `0x801F751C` during a Lippian Flute cast (it only runs on the failed roll): `a2 =
0x801F69D8`, `a3 = 0x64`. Or log `a2` / `a3` at `0x8003541C` for the whole cast: three hits with X =
126, 119 and 100. Changing the halfword at `0x801F7508` from `0x0064` to `0x0079` before that phase
runs centres the caption.

### Open questions

- Whether the Japanese release carries the same 0x64 with a ten-character head string (expected, not checked: only the US disc was read).

## Sibling captions with a fixed or shifted position

**Status:** explained
**Reported as:** Other summon and item captions look slightly off-centre too (follow-up to the same community report).

Scanning every caption draw in the summon and enemy cast modules, and every write to the battle name
banner, turns up the same shortcut in three more places: a position typed in as a number instead of
measured.

**Hell's Music** (Nighto) is drawn at X = 106 on one of its two outcomes, **Dark Eclipse**
(Juggernaut) at X = 112, and the **Points returned** banner of the battle overlay at X = 124.
Against the widths of the English strings those are 21 and 14 pixels too far left and 6 pixels too
far right. The Hell's Music case is a prediction from the code; nobody has reported or captured it.

The three summon cases share one trait with Ultimate Rave: they are the only cast modules whose file
begins with the attack name as text, and that head string is exactly the one drawn at a fixed
position.

A milder inconsistency sits next to them. Each summon first shows its spell name, and that caption
*is* measured - but ten modules subtract the half-width from 166 or 172 instead of 160, so those
captions sit 6 or 12 pixels right of centre while their sibling modules use 160. Whether that is a
deliberate nudge or a leftover is not established, so it is recorded as a measured inconsistency,
not as a proven bug.

Everything else that shows a name in battle - monster attacks, monster spells, items, regular and
Hyper Arts, the summon's own attack name - measures the very string it draws and is centred to
within a pixel.

### Mechanism

1. Nighto: the module stores rand() % 8; non-zero draws the head string at X = 0x6A, zero draws the measured record name 20 px lower. (`0x801F7964`)
2. Juggernaut: after the measured first caption, the head string is drawn at X = 0x70. (`0x801F7710`)
3. Battle overlay item leg: the item name is measured and centred, then id 0xFE overwrites X with 0x7C and swaps the string. (`0x801E3C50`)
4. Summon spell-name captions: measured, but ten modules subtract from 0xA6 or 0xAC instead of 0xA0. (`0x801F700C`)

Hard-coded X:

| Caption | Where | Shown when | Width | Drawn X | Drawn span | Centred X | Error |
|---|---|---|---|---|---|---|---|
| Ultimate Rave | PROT 0924, `0x801F7508` | Lippian flute, kill roll failed | 78 | 100 | 100..178 | 121 | -21 px |
| Hell's Music | PROT 0907, `0x801F7980` | Nighto, non-zero outcome roll | 67 | 106 | 106..173 | 127 | -21 px |
| Dark Eclipse | PROT 0927, `0x801F7710` | Juggernaut cast, always | 69 | 112 | 112..181 | 126 | -14 px |
| Points returned | PROT 0898, `0x801E3C50` | item leg, id `0xFE` | 84 | 124 | 124..208 | 118 | +6 px |

The implied design widths are 120, 108, 96 and 72 px, i.e. 10, 9, 8 and 6 cells of 12 px. That fits
positions computed by hand for fixed-pitch Japanese names (inference; only the US disc was read).

In PROT entry 0907 the outcome word `0x801F8534` holds `rand() % 8`. Non-zero draws the head string
at `(106, 150)`; zero draws the creature record name - also "Hell's Music" on the US disc - measured
and centred at `y = 0xAA`. The on-screen result is predicted from those instructions.

Measured, but not all centred on 160 (a measured inconsistency between sibling modules, not a proven bug):

| Module | Spell id | Caption | Width | Centre constant | Drawn span | Offset |
|---|---|---|---|---|---|---|
| PROT 0903 | `0x81` | fire plate + Gimard | 63 | `0xAC` (172) | 141..204 | +12 px |
| PROT 0905 | `0x83` | light plate + Vera | 51 | `0xAC` (172) | 147..198 | +12 px |
| PROT 0904 | `0x82` | thunder plate + Theeder | 68 | `0xA6` (166) | 132..200 | +6 px |
| PROT 0924 | `0x96` | <Rare> Lippian | 80 | `0xA6` (166) | 126..206 | +6 px |
| PROT 0925 | `0x97` | <Rare> Spikefish | 91 | `0xA6` (166) | 121..212 | +6 px |
| PROT 0930 | `0x9C` | winged icon + Horn | 57 | `0xA6` (166) | 138..195 | +6 px |
| PROT 0931 | `0x9D` | winged icon + Jedo | 58 | `0xA6` (166) | 137..195 | +6 px |
| PROT 0932 | `0x9E` | winged icon + Meta | 60 | `0xA6` (166) | 136..196 | +6 px |
| PROT 0933 | `0x9F` | winged icon + Terra | 64 | `0xA6` (166) | 134..198 | +6 px |
| PROT 0934 | `0xA0` | winged icon + Ozma | 62 | `0xA6` (166) | 135..197 | +6 px |
| PROT 0906..0923, 0928, 0929 | `0x84..0x95`, `0x9A`, `0x9B` | Gizam .. Gilium, Palma, Mule | 43..78 | `0xA0` (160) | 160 - width/2 | 0 |

The record-name caption of PROT entry 0966 (`0x801F7E28`) also uses `0xA6`. Spell names come from
the table described in [spell-table.md](../../formats/spell-table.md); summon attack names from the
actor records in [summon-readef.md](../../formats/summon-readef.md).

Paths that measure the string they draw:

| Name shown | Code | String source |
|---|---|---|
| Monster move / special attack | `FUN_8004AD80` in SCUS, `0x8004AF44..0x8004AF88` | spell table `0x800754C8 + id*0xC`, `+8` |
| Monster cast | `FUN_801E295C` in PROT 0898, `0x801E43E4..0x801E4458` | same table |
| Spell leg of action state `0x3C` | PROT 0898, `0x801E3D54..0x801E3D9C` | same table |
| Item use | PROT 0898, `0x801E3BF8..0x801E3C40` | item table `0x80074368 + id*0xC`, `+4` |
| Regular / Hyper Art | `FUN_8004C650` in SCUS | arts-name table `0x80075EC4`, `+0xC` |
| Fatal Decision option | PROT 0954, `0x801F7C88..0x801F7CD8` | module table `0x801F8D50`, stride `0x28` |
| Summon attack name (second caption of PROT 0903..0934) | each module, `addiu a3,zero,0xa0; subu a3,a3,v0` | summon actor record name through `0x801C9358` |
| Evolved-Cort stage banner | PROT 0968, `0x801F7098` | monster record name |
| Death Game roulette labels | PROT 0957, `0x801F7FF0..0x801F80F0` | module head strings, measured per cell |

### Addresses

| Address | Image | What | Evidence |
|---|---|---|---|
| `0x801F7980` | PROT 0907 | `addiu a3,zero,0x6a` - "Hell's Music" at X = 106 | disassembly |
| `0x801F7964` | PROT 0907 | Loads the outcome word `0x801F8534`; `beqz` at `0x801F796C` picks the measured draw instead | disassembly |
| `0x801F6B7C` | PROT 0907 | Stores `rand() % 8` into `0x801F8534` | disassembly |
| `0x801F79E8` | PROT 0907 | Draw call shared by both outcomes | disassembly |
| `0x801F7710` | PROT 0927 | `addiu a3,zero,0x70` - "Dark Eclipse" at X = 112; draw call at `0x801F7724` | disassembly |
| `0x801E3C50` | PROT 0898 | `addiu v0,zero,0x7c` then `sh v0,0x72a(a0)` / `sh v0,0x722(a0)` - "Points returned" at X = 124 | disassembly |
| `0x801E3C4C` | PROT 0898 | Gate for the above: `actor[+0x1DF] == 0xFE` | disassembly |
| `0x801CED34` | PROT 0898 | The "Points returned" string | disc data |
| `0x801F700C` | PROT 0903 | Gimard spell-name caption, centre constant `0xAC` | disassembly |
| `0x801F727C` | PROT 0905 | Vera spell-name caption, centre constant `0xAC` | disassembly |
| `0x801F707C` | PROT 0904 | Theeder spell-name caption, centre constant `0xA6` | disassembly |
| `0x801F7020` | PROT 0924 | Lippian spell-name caption, centre constant `0xA6` | disassembly |
| `0x801F6EE8` | PROT 0925 | Spikefish spell-name caption, centre constant `0xA6` | disassembly |
| `0x801F6FCC` | PROT 0930 | Horn spell-name caption, centre constant `0xA6` | disassembly |
| `0x801F75AC` | PROT 0931 | Jedo spell-name caption, centre constant `0xA6` | disassembly |
| `0x801F7458` | PROT 0932 | Meta spell-name caption, centre constant `0xA6` | disassembly |
| `0x801F7224` | PROT 0933 | Terra spell-name caption, centre constant `0xA6` | disassembly |
| `0x801F7D98` | PROT 0934 | Ozma spell-name caption, centre constant `0xA6` | disassembly |
| `0x801F7E28` | PROT 0966 | Record-name caption of the Evil Seru Magic cast module, centre constant `0xA6` | disassembly |
| `0x80074050` | SCUS_942.54 | Icon escape table (4-byte records, advance at `+2`): element plates advance 20, winged icons 28 | disc data |
| `0x80076C10` | SCUS_942.54 | HUD placement table, stride `0x18`; record `0x4C` X fields at `+0x722` / `+0x72A`, string pointer at `+0x734` | disassembly |

### Scope

Four hard-coded positions exist in total (Ultimate Rave, Hell's Music, Dark Eclipse, Points
returned). Ten summon spell-name captions are centred on 166 or 172. Every other battle name path
measures the string it draws.

### Verifying it

Log `a2` (string), `a3` (X) and the word at `sp+0x10` (Y) at `0x8003541C` across one cast of each
summon; any X that is not 160 minus half the string width is on the lists here. For Nighto, repeat
the cast until both outcomes appear: predicted X = 106 at Y = 150 on one, X = 127 at Y = 170 on the
other.

### Open questions

- Whether the 166 / 172 spell-name centres are a deliberate nudge or a leftover.
- Whether Nighto's fixed-position caption is visibly off-centre in play (predicted from the code, never captured).
- Which in-game event produces id 0xFE and the "Points returned" banner (taken from the existing battle-action notes, not traced here).

## Super Art and Miracle finisher names are off-centre

**Status:** explained
**Reported as:** Super Art and Miracle Art finisher names are only centred when the name is about 12 or 13 characters long (community report; root cause traced by ZetaPhoenix).

Regular and Hyper Art names are centred. Super Art names, and the last hit of a Miracle Art, all
start at the same left edge no matter how long the name is - so a short name like **Love You** hangs
left and a long one like **Neo Static Raising** hangs right.

When an art animation starts, the banner code first centres the banner for one fixed name, **Vulture
Blade**, whichever art is playing. It then looks the art up in the executable's arts-name table and,
if it finds a row, replaces both the text and the position with the right ones. Regular and Hyper
Arts have rows. Super Art finishers and Miracle finishers do not, so they keep the name from their
animation record together with Vulture Blade's position.

"Vulture Blade" is 13 characters and 75 pixels wide, which puts every finisher at X = 123. That is
the "12 or 13 characters" of the community rule; because the font is proportional the exact
condition is a width of 74 or 75 pixels, which Super Javelin and Dragon Fangs happen to hit.

The root cause was traced by ZetaPhoenix, whose fix re-measures the installed name just before the
banner is raised; this project ships that fix with his Super Arts Pack. The page adds the per-name
numbers.

This and the Ultimate Rave caption are different routines on different draw paths, but the same
class of mistake: the position is derived from a string other than the one that gets drawn, a few
instructions away from code that does it correctly.

### Mechanism

1. An art with action constant 0x1B or above is staged; the animation commit enters its banner block. (`0x8004BB5C`)
2. It measures the fixed pointer at 0x80076024 - always "Vulture Blade", 75 px. (`0x8004BBB8`)
3. It writes X = 160 - 37 = 123 into the four banner X fields. (`0x8004BBD8`)
4. It points the banner at the name inside the art's own animation record. (`0x8004BC10`)
5. It calls the arts-name lookup with (character, constant - 0x1B). (`0x8004BC34`)
6. A matching row re-measures and rewrites X and the text; no row means nothing is written. (`FUN_8004C650`)
7. The banner is raised with whatever is in the fields: for a finisher, its record name at X = 123. (`0x8004BC3C`)

```
8004bbb8  lw    a0,0x6024(v0)   ; *(0x80076024) = "Vulture Blade"
8004bbbc  jal   0x80035f04      ; measure: 75
8004bbcc  sra   v0,v0,1         ; 37
8004bbd0  li    v1,0xa0
8004bbd4  subu  v1,v1,v0        ; 123
8004bbd8  sh    v1,0x742(a0)
8004bbe4  sh    v1,0x722(a0)
8004bc0c  addiu v0,v0,-0xcf0    ; art record name
8004bc10  sw    v0,0x74c(a0)
8004bc34  jal   0x8004c650      ; table lookup (character, id - 0x1B)
8004bc40  jal   0x801d8de8      ; raise the banner
```

`0x80076024` is the `+0xC` name field of row 17 of the arts-name table (`0x80075EC4 + 17*0x14 +
0xC`), the `[character 1, art 4]` row. The table holds rows for art indices `0..=0xE` (Vahn, Gala)
and `0`, `1`, `4..=0x10` (Noa), so action constants above `0x29` (Noa: above `0x2B`), and Noa's
`0x1D` / `0x1E`, match nothing. Art records and action constants:
[art-data.md](../../formats/art-data.md). The shipped fix:
[randomizer-internals.md](../../tooling/randomizer-internals.md).

### Addresses

| Address | Image | What | Evidence |
|---|---|---|---|
| `0x8004AD80` | SCUS_942.54 | Animation commit; its art-name banner block is `0x8004BB5C..0x8004BC48`, entered when `actor[+0x1DA] >= 0x1B` | disassembly |
| `0x8004BBB8` | SCUS_942.54 | `lw a0,0x6024(v0)` - loads the fixed name pointer that gets measured | disassembly |
| `0x80076024` | SCUS_942.54 | The fixed pointer: the name field of arts-name row 17 (Noa, art 4) = `0x80013DAC` "Vulture Blade", 75 px | disc data |
| `0x8004BBD8` | SCUS_942.54 | Four stores of X = 123 into `+0x742` / `+0x73A` / `+0x72A` / `+0x722` | disassembly |
| `0x8004BC10` | SCUS_942.54 | Banner string = the art record name (`(id - 0x10) * 0xD0 + 0x10` into the art block), stored to `+0x74C` / `+0x734` | disassembly |
| `0x8004BC34` | SCUS_942.54 | Call to the table lookup with `(character, id - 0x1B)` | disassembly |
| `0x8004C650` | SCUS_942.54 | Arts-name lookup: on a match re-measures the row name and rewrites X and both pointers (`0x8004C6E0..0x8004C71C`); on no match writes nothing (`beq` at `0x8004C6D8`) | disassembly |
| `0x8004C728` | SCUS_942.54 | Adds 3 px when the row name starts with a `0xCF` colour escape | disassembly |
| `0x80075EC4` | SCUS_942.54 | Arts-name table: stride `0x14`, `+0` character, `+1` art index, `+0xC` name pointer, terminator byte `0x63` at `0x80076248` | disc data |
| `0x80077332` | RAM | Banner X halfwords (`0x80076C10 + 0x722`); the others are `0x8007733A`, `0x8007734A`, `0x80077352` | disassembly |
| `0x8004BC3C` | SCUS_942.54 | Site of the community fix: `li a0,0x4C; jal 0x801D8DE8`, the banner raise | repo doc |
| record0 | PROT 0863 / 0864 / 0865 | Player battle files; the art records sit in the LZS-packed first record (stream at file `+0x10`), art block at decoded `0x432C` / `0x5A34` / `0x5A60`, stride `0xD0` | disc data |

### Scope

Nineteen staged names across the three characters: all fifteen Super Arts, the four Miracle finisher
records (Tornado Flame, Jurassic Blow twice, Thunder Punch) and Noa's two row-less Hurricane Kick
clips. Regular and Hyper Arts are unaffected. The largest errors are Love You (12 px left) and Neo
Static Raising (14 px right).

Every row-less name the banner shows, all drawn at `x = 123`:

| Character | Constant | Name drawn | Kind | Chars | Width | Drawn span | Drawn centre | Centred X | Error |
|---|---|---|---|---|---|---|---|---|---|
| Vahn | `0x2A` | Tornado Flame | Miracle finisher | 13 | 82 | 123..205 | 164 | 119 | +4 px |
| Vahn | `0x2B` | Tri-Somersault | Super Art | 14 | 84 | 123..207 | 165 | 118 | +5 px |
| Vahn | `0x2C` | Maximum Blow | Super Art | 12 | 80 | 123..203 | 163 | 120 | +3 px |
| Vahn | `0x2D` | Fire Tackle | Super Art | 11 | 64 | 123..187 | 155 | 128 | -5 px |
| Vahn | `0x2E` | Power Slash | Super Art | 11 | 67 | 123..190 | 156.5 | 127 | -4 px |
| Vahn | `0x2F` | Rolling Combo | Super Art | 13 | 76 | 123..199 | 161 | 122 | +1 px |
| Noa | `0x1D`, `0x1E` | Hurricane Kick | follow-up clips | 14 | 80 | 123..203 | 163 | 120 | +3 px |
| Noa | `0x2C`, `0x2D` | Jurassic Blow | Miracle finisher | 13 | 77 | 123..200 | 161.5 | 122 | +1 px |
| Noa | `0x2E` | Triple Lizard | Super Art | 13 | 73 | 123..196 | 159.5 | 124 | -1 px |
| Noa | `0x2F` | Super Javelin | Super Art | 13 | 75 | 123..198 | 160.5 | 123 | 0 |
| Noa | `0x30` | Super Tempest | Super Art | 13 | 81 | 123..204 | 163.5 | 120 | +3 px |
| Noa | `0x31` | Love You | Super Art | 8 | 50 | 123..173 | 148 | 135 | -12 px |
| Noa | `0x32` | Dragon Fangs | Super Art | 12 | 74 | 123..197 | 160 | 123 | 0 |
| Gala | `0x2A` | Thunder Punch | Miracle finisher | 13 | 79 | 123..202 | 162.5 | 121 | +2 px |
| Gala | `0x2B` | Back Punch x3 | Super Art | 13 | 80 | 123..203 | 163 | 120 | +3 px |
| Gala | `0x2C` | Super Ironhead | Super Art | 14 | 81 | 123..204 | 163.5 | 120 | +3 px |
| Gala | `0x2D` | Rushing Crush | Super Art | 13 | 76 | 123..199 | 161 | 122 | +1 px |
| Gala | `0x2E` | Heaven's Drop | Super Art | 13 | 77 | 123..200 | 161.5 | 122 | +1 px |
| Gala | `0x2F` | Neo Static Raising | Super Art | 18 | 103 | 123..226 | 174.5 | 109 | +14 px |

The fix repairs this banner only. The fixed-position captions of the previous sections are separate
code; each of their strings is constant, so the equivalent repair is one changed immediate per site
(`0x801F7508`, `0x801F7980`, `0x801F7710`, `0x801E3C50`).

### Verifying it

With a finisher banner on screen read the halfwords at `0x80077332`, `0x8007733A`, `0x8007734A` and
`0x80077352`: `0x007B` (123) for every Super Art or Miracle finisher, a name-dependent value for a
regular art. Or break at `0x8004BBBC`: `a0` is `0x80013DAC` whichever art is being installed.

### Open questions

- Why the pre-measure uses arts-name row 17 specifically; the binary does not say whether it was a placeholder or a default.
- Whether Noa's row-less 0x1D / 0x1E clips are ever visible as separate captions during Hurricane Kick.
