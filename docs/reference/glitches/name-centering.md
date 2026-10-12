# Off-centre attack names

Three related presentation defects in the battle name captions of the English release: the
"Ultimate Rave" caption of a failed Lippian Flute, the sibling captions that share its cause, and
the Super Art / Miracle Art finisher banner. All three come from the localisation: the Japanese
build places names by a different rule, under which the same numbers land on or near centre.

Addresses and pixel positions are those of the US release (`SCUS_942.54`) unless a row says
otherwise; the Japanese (`SCPS_100.59`) and French (`SCES_019.44`) builds were disassembled for the
same code. Every position is derived from the disc's code and data. The summon captions were then
logged at the text routine in a running game and match to the pixel; the banner positions (arts,
items, "Points returned") and every Japanese on-screen position are computed, not captured. Pixel
positions are on the 320-pixel-wide battle screen.

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
"Poisonous Sting" sums to 86 px, giving `x = 117`. It also reproduces the X of all seventeen summon
captions logged at `FUN_8003541C` (see [What was captured](#what-was-captured)).

### The Japanese build

The Japanese build has no width routine. A name is a length-prefixed string, `[u8 N][N two-byte
characters]`, and every character occupies a 12-pixel cell, so a centred name is placed by
arithmetic on the length byte alone:

```
x = C - 6 * N          ; C = 0xA0 for a screen-centred name
```

Each US site of the form `x = C - (measure(str) >> 1)` is, in the Japanese build, `lbu v1,(str)`,
a multiply by 6 and `subu a3,C,..`. PROT entry 0924 calls the width routine twice on the US disc
and has no counterpart call on the Japanese one.

The formula is exact only for full-width characters. The Japanese draw routine narrows a few of
them, which is what moves a hand-placed name off its nominal centre:

| Character | Advance | Where |
|---|---|---|
| Ordinary full-width character | 12 px (`addiu s4,s4,0xc`) | `0x80037AE0` |
| Middle dot (`81 45`) | 8 px: pulled in 2 px on each side | `0x8003740C`, `0x80037B18` |
| Opening or closing bracket | 8 px: loses 4 px | `0x80037488`, `0x80037B94` |
| Full-width space | 6 px | `0x800373CC` |

The caption register `0x80032D10` in `SCPS_100.59` is the counterpart of `FUN_8003541C`: it walks
the length-prefixed lines (`0x80032DC4..0x80032DE8`), sets the 12-pixel cell (`addiu a0,zero,0xc`
into the byte at `0x8007D238`, `0x80032D44..0x80032D54`) and stores the pen X at node `+0x0A`
(`0x80032E7C`) as the US routine does. The draw routine is `0x80036FC0`.

The Japanese overlays load higher than the US ones: PROT entries 0897 / 0898 / 0899 at `0x801D05C8`
(US `0x801CE818`), the cast modules PROT 0903..0966 at `0x801F9DB0` (US `0x801F69D8`). The Japanese
build is a separate compile, so its routines are located by content, not by address. The French
build is the US code relocated (cast modules at `0x801F7400`): every routine on this page matches
it instruction for instruction, constants included, so the PAL release carries each defect
described here.

### What was captured

A breakpoint at `FUN_8003541C` on the US disc logged the string pointer, the pen X (`a3`) and the Y
(`sp+0x10`) of seventeen caption draws across eight summon casts. Every measured X equals the
formula above applied to the logged string with that module's centre constant, and every fixed X
equals its literal.

The casts were injected, not played: at the start of a party member's action the queued action was
rewritten to the summon's spell id, after which the game loaded and ran the cast module itself; HP
values were poked to keep the fight alive, and some casts ran in a fight started from a forced
formation. One row, Nighto's rarer outcome, is captured on a poked state: the outcome word was
overwritten with zero to take that branch. What is logged is the pen position handed to the text
routine, not a framebuffer screenshot.

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

"Ultimate Rave" is 78 pixels wide in the game's proportional font, so it runs from 100 to 178 and
its middle sits at 139 instead of 160. Measured and centred it would start at 121: the caption is
**21 pixels too far left** on a 320-pixel screen.

The number is the Japanese one. The Japanese cast module carries the same instruction with the same
100, in front of a name of ten characters: `160 - 6 x 10 = 100`. One of the ten is a narrow middle
dot, so that name is 116 pixels wide, covers 100..216 and sits two pixels left of centre. The
English string was written over it and the number was kept.

Both captions are logged from a running cast (an injected one, see
[What was captured](#what-was-captured)): Ultimate Death is drawn at X = 119 and, after the failed
kill, Ultimate Rave at X = 100, Y = 150. The community screenshot that prompted the trace agrees:
its caption starts about 99 native pixels in and is about 78 wide.

| Caption | Build | Width | Drawn X | Drawn span | Drawn centre | Centred X | Error |
|---|---|---|---|---|---|---|---|
| Ultimate Death | US | 83 | 119 | 119..202 | 160.5 | 119 | 0 |
| Ultimate Rave | US | 78 | 100 | 100..178 | 139 | 121 | -21 px |
| Japanese failed-kill name (ten characters, one a middle dot) | Japan | 116 | 100 | 100..216 | 158 | 102 | -2 px |

### Mechanism

1. The cast module rolls the kill once at the start: random number AND 0x7F, kill only on zero. (`0x801F6B0C`)
2. First caption: the spell name from the spell table, measured, X = 0xA6 - width/2. (`0x801F7020`)
3. Second caption: the summon creature's attack name ("Ultimate Death"), measured, X = 0xA0 - width/2 = 119. (`0x801F70F4`)
4. Near the end the module tests the kill flag; if the kill landed no new caption is drawn. (`0x801F74E8`)
5. Otherwise it destroys the old caption and loads the module's own head string with X = 0x64 as a literal. (`0x801F7508`)
6. The text routine puts the pen exactly at that X, so the 78 px string covers 100..178. (`FUN_8003541C`)

The Japanese module (PROT entry 0924 at load base `0x801F9DB0`) has the same sequence around the
same literal: the kill-flag gate at `0x801FA8D4..0x801FA8E0`, the destroy call, the module head as
the string, `addiu a3,zero,0x64` at `0x801FA900`, `y = 0x96`, the register call. Its head string
begins with the length byte `0x0A` and ten katakana characters that read "Ultimate Miss"; being
eight bytes longer than the English one, it pushes the phase table from file offset `0x10` to
`0x18`. The French module has `addiu a3,zero,0x64` at `0x801F7F30`.

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
| `0x8003541C` | SCUS_942.54 | Entry logged during an injected Lippian cast: X = 126, 119, then 100, each at Y = 150 | capture |
| `0x800319A8` | SCUS_942.54 | Text-label destroy (removes the previous caption) | repo doc |
| `0x80035F04` | SCUS_942.54 | String width measure | disassembly |
| `0x80073F1C` | SCUS_942.54 | Proportional width table, `u8[256]`; a glyph advances `width + 1` in battle | disc data |
| `0x800754C8` | SCUS_942.54 | Spell table, stride `0xC`, name pointer at `+8` | disc data |
| `0x801C9358` | RAM | Record pointer of battle slot 7 (the summon creature); its first word points at the attack name | repo doc |
| slot 65 | PROT 0893 (summon.dat) | Lippian actor record, attack name "Ultimate Death" | disc data |
| `0x801FA900` | PROT 0924, Japan | `addiu a3,zero,0x64` - the same literal in the Japanese module (file offset `0xB50`) | disassembly |
| `0x801FA8D4` | PROT 0924, Japan | Kill-flag gate in front of it (`lw` / `bnez`, to `0x801FA8E0`) | disassembly |
| `0x801F9DB0` | PROT 0924, Japan | Module head: length byte `0x0A`, then ten two-byte characters; the phase table follows at `0x801F9DC8` (file `0x18`) | disc data |
| `0x801F7F30` | PROT 0924, France | `addiu a3,zero,0x64` in the French module | disassembly |
| `0x80032D10` | SCPS_100.59 | Japanese caption register, the counterpart of `FUN_8003541C`; X stored at node `+0x0A` at `0x80032E7C` | disassembly |
| `0x80037AE0` | SCPS_100.59 | Pen advance of the Japanese draw routine `0x80036FC0`: 12 px a character | disassembly |

### Scope

Only the failed outcome of the Lippian Flute shows this caption, on the US and PAL releases. The
Japanese release draws its own failed-kill name at the same X, two pixels off centre. The same
shortcut exists in three other places, listed under the sibling captions.

### Verifying it

Break on `0x801F751C` during a Lippian Flute cast (it only runs on the failed roll): `a2 =
0x801F69D8`, `a3 = 0x64`. Or log `a2` / `a3` at `0x8003541C` for the whole cast: three hits with X =
126, 119 and 100, which is what the injected cast logged. Changing the halfword at `0x801F7508` from
`0x0064` to `0x0079` before that phase runs centres the caption.

On the Japanese disc the equivalent check is a breakpoint on the register call after `0x801FA900`;
that build was read, not run.

## Sibling captions with a fixed or shifted position

**Status:** explained
**Reported as:** Other summon and item captions look slightly off-centre too (follow-up to the same community report).

Scanning every caption draw in the summon and enemy cast modules, and every write to the battle name
banner, turns up the same shortcut in three more places: a position typed in as a number instead of
measured.

**Hell's Music** (Nighto) is drawn at X = 106 on the commoner of its two outcomes, **Dark Eclipse**
(Juggernaut) at X = 112, and the **Points returned** banner of the battle overlay at X = 124.
Against the widths of the English strings those are 21 and 14 pixels too far left and 6 pixels too
far right. The Hell's Music case was found in the code and then captured: X = 106, Y = 150.

The three summon cases share one trait with Ultimate Rave: they are the only cast modules whose file
begins with the attack name as text, and that head string is exactly the one drawn at a fixed
position.

All four numbers are in the Japanese build unchanged, and there they work. Japanese names are
fixed-pitch, so a position typed in by hand stays right for as long as the name does: the four
Japanese captions land 2 pixels left, 4 right, 4 right and exactly on centre. The English release
replaced the names with shorter proportional text, taught most of the code to measure it, and left
these four numbers alone.

A milder inconsistency sits next to them. Each summon first shows its spell name, and that caption
*is* measured - but ten modules subtract the half-width from 166 or 172 instead of 160, so those
captions sit 6 or 12 pixels right of centre while their sibling modules use 160. The same modules
carry the same constants on the Japanese disc, so this one is not a localisation artefact. Why
those modules differ is not established, so it is recorded as a measured inconsistency, not as a
proven bug.

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

The same four literals on the Japanese disc, where a character is 12 px and a middle dot 8 px:

| Literal | Japanese site | File offset | Japanese name | Width | Drawn span | Drawn centre | Error | `160 - 6 x length` |
|---|---|---|---|---|---|---|---|---|
| `0x64` (100) | PROT 0924, `0x801FA900` | `0xB50` | 10 characters, one a middle dot | 116 | 100..216 | 158 | -2 px | 100 |
| `0x6A` (106) | PROT 0907, `0x801FAE50` | `0x10A0` | 10 characters, one a middle dot | 116 | 106..222 | 164 | +4 px | 100 |
| `0x70` (112) | PROT 0927, `0x801FAB00` | `0xD50` | 9 characters, one a middle dot | 104 | 112..216 | 164 | +4 px | 106 |
| `0x7C` (124) | PROT 0898, `0x801E6A14` | `0x1644C` | 6 characters | 72 | 124..196 | 160 | 0 | 124 |

The literals are not the output of one rule. The first and last equal `160 - 6 x length`. The Hell's
Music and Dark Eclipse literals are one cell less than that (`160 - 6 x 9` for a ten-character name,
`160 - 6 x 8` for a nine-character one), which lands closer to the true centre than the plain
formula would because the middle dot is narrow. That they were set by hand is an inference from
those values. The tempting reading - four slots of 10, 9, 8 and 6 cells, i.e. 120 / 108 / 96 /
72 px - fits only the first and last: the names are 10, 10, 9 and 6 characters long.

The Japanese code around each literal is the US code: destroy the previous caption, load the module
head, the literal, `y = 0x96`, register.

In PROT entry 0907 the outcome word `0x801F8534` holds `rand() % 8`. Non-zero draws the head string
at `(106, 150)`; zero draws the creature record name - also "Hell's Music" on the US disc - measured
and centred at `y = 0xAA`. Both draws are captured: `(106, 150)` with the head string at
`0x801F69D8`, and, with the outcome word forced to zero, `(127, 170)` with the record name at
`0x800B9AB4`. Nighto draws two captions per cast: the spell name and one "Hell's Music". The
Japanese module has the same gate (`0x801FAE30..0x801FAE3C`) and its zero arm draws the record name
at `0xA0 - 6N`, `y = 0xAA`.

The distinct captions among the pen positions logged at `FUN_8003541C` (US disc, injected casts;
"icon" is the leading icon escape of a name):

| Caption | String at | Width | Logged X, Y | `C - width/2` | Drawn centre |
|---|---|---|---|---|---|
| icon + Nighto | `0x80012D58` | 59 | 131, 150 | 131 | 160.5 |
| Hell's Music, module head (outcome word 6) | `0x801F69D8` | 67 | 106, 150 | literal | 139.5 |
| Hell's Music, record name (outcome word poked to 0) | `0x800B9AB4` | 67 | 127, 170 | 127 | 160.5 |
| icon + Juggernaut | `0x800B9ACC` | 85 | 118, 150 | 118 | 160.5 |
| Dark Eclipse, module head | `0x801F69D8` | 69 | 112, 150 | literal | 146.5 |
| <Rare> Lippian | `0x80012CA8` | 80 | 126, 150 | 126 (`C = 0xA6`) | 166 |
| Ultimate Death | `0x800B9ABC` | 83 | 119, 150 | 119 | 160.5 |
| Ultimate Rave, module head (kill roll failed) | `0x801F69D8` | 78 | 100, 150 | literal | 139 |
| icon + Theeder | `0x80012D70` | 68 | 132, 150 | 132 (`C = 0xA6`) | 166 |
| Turning Laser | `0x800B9AB0` | 77 | 122, 150 | 122 | 160.5 |
| icon + Mushura | `0x80012D0C` | 70 | 125, 150 | 125 | 160 |
| Crazy Driver | `0x800B9ABC` | 69 | 126, 150 | 126 | 160.5 |
| icon + Orb | `0x8007B54C` | 43 | 139, 150 | 139 | 160.5 |
| Holy Orb | `0x800B9AB4` | 46 | 137, 150 | 137 | 160 |
| icon + Mule | `0x8007B534` | 58 | 131, 150 | 131 | 160 |
| Deep Avalanche | `0x800B9AB4` | 86 | 117, 150 | 117 | 160 |

Measured, but not all centred on 160 (a measured inconsistency between sibling modules, not a proven
bug). The last two columns are the Japanese build: the call site, and where the constant puts a
Japanese spell name.


| Module | Spell id | Caption | Width | Centre constant | Drawn span | Offset | Japanese site | Japanese centre |
|---|---|---|---|---|---|---|---|---|
| PROT 0903 | `0x81` | fire plate + Gimard | 63 | `0xAC` (172) | 141..204 | +12 px | `0x801FA408` | 168 (+8) |
| PROT 0905 | `0x83` | light plate + Vera | 51 | `0xAC` (172) | 147..198 | +12 px | `0x801FA710` | 168 (+8) |
| PROT 0904 | `0x82` | thunder plate + Theeder | 68 | `0xA6` (166) | 132..200 | +6 px | `0x801FA494` | 162 (+2) |
| PROT 0924 | `0x96` | <Rare> Lippian | 80 | `0xA6` (166) | 126..206 | +6 px | `0x801FA3E4` | 162 (+2) |
| PROT 0925 | `0x97` | <Rare> Spikefish | 91 | `0xA6` (166) | 121..212 | +6 px | `0x801FA2E0` | 162 (+2) |
| PROT 0930 | `0x9C` | winged icon + Horn | 57 | `0xA6` (166) | 138..195 | +6 px | `0x801FA3D8` | 162 (+2) |
| PROT 0931 | `0x9D` | winged icon + Jedo | 58 | `0xA6` (166) | 137..195 | +6 px | `0x801FA9FC` | 162 (+2) |
| PROT 0932 | `0x9E` | winged icon + Meta | 60 | `0xA6` (166) | 136..196 | +6 px | `0x801FA82C` | 162 (+2) |
| PROT 0933 | `0x9F` | winged icon + Terra | 64 | `0xA6` (166) | 134..198 | +6 px | `0x801FA680` | 162 (+2) |
| PROT 0934 | `0xA0` | winged icon + Ozma | 62 | `0xA6` (166) | 135..197 | +6 px | `0x801FB264` | 162 (+2) |
| PROT 0906..0923, 0928, 0929 | `0x84..0x95`, `0x9A`, `0x9B` | Gizam .. Gilium, Palma, Mule | 43..78 | `0xA0` (160) | 160 - width/2 | 0 | same constant in every module | 156 (-4) |
| PROT 0927 | - | icon + Juggernaut (creature record name, not a spell-table name) | 85 | `0xA0` (160) | 118..203 | 0 | same constant | - |

The record-name caption of PROT entry 0966 (`0x801F7E28`, Japanese site `0x801FB30C`) also uses
`0xA6`, as does PROT entry 0926, whose tail repeats the caption code of 0925. Spell names come from
the table described in [spell-table.md](../../formats/spell-table.md); summon attack names from the
actor records in [summon-readef.md](../../formats/summon-readef.md).

On the Japanese disc the spell names (name pointers at `0x80077200 + id*0xC`) are plain strings -
two or three kanji and a bracketed name, 7 to 10 characters - with **no icon escape**: the element
plates and winged icons in front of the English names are an addition of the localisation. Two
brackets at 8 px each make such a name `12N - 8` px wide, so `C - 6N` draws it centred on `C - 4`
whatever its length: 156, 162 or 168 for the three constants.

That rules out the two natural explanations of the shifted constants. They are not an allowance for
the English icons, which they predate. They are not a per-name correction either: PROT 0903 and
PROT 0908 draw Japanese names of the same shape and the same eight-character length (spells `0x81`
and `0x86`) with `0xAC` and `0xA0`.

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
| `0x801F79F0` | PROT 0907 | Return address of the "Hell's Music" draw: logged X = 106, Y = 150 with the module head, and X = 127, Y = 170 with the record name on a poked outcome word | capture |
| `0x801F7900` | PROT 0907 | Return address of the spell-name draw: logged X = 131, Y = 150 | capture |
| `0x801F7964` | PROT 0907 | Loads the outcome word `0x801F8534`; `beqz` at `0x801F796C` picks the measured draw instead | disassembly |
| `0x801F6B7C` | PROT 0907 | Stores `rand() % 8` into `0x801F8534` | disassembly |
| `0x801F79E8` | PROT 0907 | Draw call shared by both outcomes | disassembly |
| `0x801F7710` | PROT 0927 | `addiu a3,zero,0x70` - "Dark Eclipse" at X = 112; draw call at `0x801F7724` | disassembly |
| `0x801F7724` | PROT 0927 | The "Dark Eclipse" draw, logged at X = 112, Y = 150 in an injected cast | capture |
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
| `0x801FAE50` | PROT 0907, Japan | `addiu a3,zero,0x6a` (file offset `0x10A0`); outcome-word gate at `0x801FAE30..0x801FAE3C` | disassembly |
| `0x801FAB00` | PROT 0927, Japan | `addiu a3,zero,0x70` (file offset `0xD50`) | disassembly |
| `0x801E6A14` | PROT 0898, Japan | `0x7C` written to the banner X fields (file offset `0x1644C`); overlay load base `0x801D05C8` | disassembly |
| `0x801D0AF0` | PROT 0898, Japan | The six-character Japanese "Points returned" string (file offset `0x528`) | disc data |
| `0x80077200` | SCPS_100.59 | Japanese spell table name pointers, `+ id*0xC`; the names carry no icon escape | disc data |

### Scope

Four hard-coded positions exist in total (Ultimate Rave, Hell's Music, Dark Eclipse, Points
returned). They are the same four in the Japanese, US and French builds; only on the Japanese disc
do they fit the names. Ten summon spell-name captions are centred on 166 or 172 in all three builds.
Every other battle name path measures the string it draws (US, French) or centres on its length
byte (Japanese).

### Verifying it

Log `a2` (string), `a3` (X) and the word at `sp+0x10` (Y) at `0x8003541C` across one cast of each
summon; any X that is not 160 minus half the string width is on the lists here. Eight injected
casts logged this way produced the table of pen positions above, with no X off the lists. For Nighto
the common outcome logs X = 106 at Y = 150; the other (one cast in eight, reached in the capture by
writing zero to `0x801F8534` at the load at `0x801F7964`) logs X = 127 at Y = 170.

"Points returned" goes through the HUD placement table, not `FUN_8003541C`, and is not captured:
read the halfwords at `0x80076C10 + 0x722` / `+0x72A` while the banner is up and expect `0x007C`.

### Open questions

- Why ten modules centre on 166 or 172. They do so on the Japanese disc as well, so it is not a localisation artefact; the intent is unknown.
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

This is a regression introduced by the localisation, not a design that was merely left alone. The
Japanese code at the same place reads the length byte of the art's own animation-record name and
centres on that, so on the Japanese disc every finisher is centred. An English record name is plain
text with no length byte to read, and the replacement code measures one fixed table name instead of
the name being shown. The French build carries the US code, so the PAL release has the defect too.

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

The Japanese counterpart of the block is `0x8004D2E4..0x8004D400` in `SCPS_100.59`: the same gate
(`sltiu v0,v1,0x1b` on `actor+0x1DA`), the same four X halfwords, the same two string pointers and
the same call into the arts-name lookup. Only the X computation differs, and it has no fixed string:

```
8004d350  lbu   v1,0x1da(s2)    ; action constant
8004d370  lbu   v1,-0xcf0(v0)   ; first byte of THIS art record's name = its length N
8004d384  addiu v1,zero,0xa0
8004d388  subu  v1,v1,v0        ; x = 160 - 6N  (v0 = N * 6)
8004d390  sh    v1,...          ; the four banner X fields, to 0x8004D3A8
8004d3fc  jal   0x8004de40      ; arts-name lookup (character, id - 0x1B)
```

| Step | Japan (`SCPS_100.59`) | US and France |
|---|---|---|
| What sets the first X | the length byte of the art record's own name | the width of arts-name row 17, "Vulture Blade" |
| Instruction | `lbu v1,-0xcf0(v0)` at `0x8004D370` | `lw a0,0x6024(v0)` at `0x8004BBB8` (France: block at `0x8004C500`) |
| X for a finisher with no table row | `160 - 6N`, its own centre | 123, whatever the name |
| Table row found | recomputes `160 - 6N` from the row name | re-measures the row name |

The Japanese record names are length-prefixed (`[N][N two-byte characters][00]` at record `+0x10`;
the art block sits at the same decoded offsets as on the US disc), and every Super Art and Miracle
finisher name is 3 to 9 full-width characters with no narrow ones, so `160 - 6N` centres each
exactly. The Japanese lookup `0x8004DE40` (table `0x80077BF4`, same stride and terminator) adds 6
when a row name starts with an `FF` colour escape, which counts as one character
(`0x8004DF34..0x8004DF50`); the US `+3` at `0x8004C728` is that correction carried over.

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
| record0 | PROT 0863 / 0864 / 0865 | Player battle files; the art records sit in the LZS-packed first record (stream at file `+0x10`), art block at decoded `0x432C` / `0x5A34` / `0x5A60`, stride `0xD0`. Same offsets on the Japanese disc, where each name is length-prefixed | disc data |
| `0x8004D2E4` | SCPS_100.59 | Japanese art-name banner block, to `0x8004D400` (file offset `0x3DAE4`) | disassembly |
| `0x8004D370` | SCPS_100.59 | `lbu v1,-0xcf0(v0)` - reads the length byte of the art record's own name; X = `0xA0 - 6N` at `0x8004D388` | disassembly |
| `0x8004DE40` | SCPS_100.59 | Japanese arts-name lookup; table at `0x80077BF4`, stride `0x14`, terminator `0x63` | disassembly |
| `0x8004C500` | SCES_019.44 | French art-name banner block: the US code, instruction for instruction | disassembly |

### Scope

Nineteen staged names across the three characters: all fifteen Super Arts, the four Miracle finisher
records (Tornado Flame, Jurassic Blow twice, Thunder Punch) and Noa's two row-less Hurricane Kick
clips. Regular and Hyper Arts are unaffected. The largest errors are Love You (12 px left) and Neo
Static Raising (14 px right). The US and PAL releases are affected; the Japanese release is not.

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
regular art. Or break at `0x8004BBBC`: `a0` is `0x80013DAC` whichever art is being installed. The
banner goes through the HUD placement table and has not been captured; these positions are read
from the code and the width table.

### Open questions

- Why the pre-measure uses arts-name row 17 specifically; the binary does not say whether it was a placeholder or a default. The Japanese build has no fixed row at all.
- Whether Noa's row-less 0x1D / 0x1E clips are ever visible as separate captions during Hurricane Kick.
