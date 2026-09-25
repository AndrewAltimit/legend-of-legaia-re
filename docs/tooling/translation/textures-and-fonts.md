# Textures and fonts

Two kinds of text sit outside the string / dialog / `ui_menu` scope of a
language pack: text drawn as pixels in a texture, and any character the retail
font has no glyph for. Both need art rather than a string edit.

## Textures with baked-in text

Some UI text is not a string at all - it is pixels in a TIM. Replacing one
means authoring new glyph art at the **exact same TIM footprint** (identical
width / height / bpp / CLUT layout) so the same-size in-place `DiscPatcher`
write applies.

The `tim` crate can encode a PNG back to a TIM, so a byte-identical-footprint
swap is mechanically possible (the texture-replacement flow in the
[randomizer reference](../randomizer.md#texture-replacement)). The blocker is
art authoring (and, for logos, rights), not the pipeline. None are patched by
the translation pipeline - each is a scoped follow-up. Legally, the boot /
publisher logos must be left untouched regardless.

The text-bearing textures on the retail disc:

| Texture | Where | Baked text | Footprint / notes |
|---|---|---|---|
| Title wordmark | PROT 0888 (dup 0889 / 0890), `legaia_asset::title_pak` | "Legend of Legaia" logo, `PRESS START BUTTON`, TM / (C) copyright bands; an unused `<DEMO>` band retail never samples | Bands are sub-rects of one 256x256 TIM (`TITLE_BAND_*`). The logo is title art (a proper noun); `PRESS START BUTTON` is a candidate for a same-footprint band swap. Copyright bands must stay. |
| Title menu `NEW GAME` / `CONTINUE` | title overlay (inside PROT entry 0899 at `+0xEB44`) | rendered at runtime from the **dialog-font glyph atlas**, not a baked band (retail ignores the embedded footer band) | So this is *text*, but it lives in the title overlay code region the pipeline does not address by coordinate. Follow-up: pin the two label strings' VA window like a `ui_menu` pool. |
| Save/Load UI | PROT 0899 `+0x16908` (`SLOT n` pill) + the pre-`init_data` `PROT.DAT` gap (`Load` panel TIM) + the title-overlay memcard atlas `0x801E5120` | baked `SLOT 1..` pill label, the `Load` panel wordmark, and Japanese memcard strings in the atlas | Small 4bpp TIMs at fixed offsets; a same-footprint pill/panel swap is feasible. See [`save-screen.md`](../../subsystems/save-screen.md). |
| Config-screen TIMs | PROT 0899 `+0x169DC` / `+0x1F91C` | small option-screen chrome TIMs that sit after the config **string** pool (the strings are the `ui_menu` menu labels, already translatable) | Chrome art; only replace if a label is baked rather than drawn from the string pool. |
| Boot / publisher logos | PROT 0895 `init.pak` (`legaia_asset::init_pak`) - PROKION, SCEA / Sony | brand logos ("licensed by", studio marks) | **Do not alter** - trademark art, not localizable text. Listed only so a sweep does not mistake them for translatable UI. |
| Opening prologue caption | opening-sequence baked caption TIM (the narration **crawl** itself is `0x1F`-framed text and *is* covered via the dialog corpus) | a baked caption still shows English under the crawl | The crawl narration translates through `scene_dialog` / `inline_text`; only the baked caption TIM would need an art swap. |

## Font-patch scope

The retail font draws printable ASCII only, which is why a pack must be
written unaccented ([`pack-format.md`](pack-format.md#text-markup-and-encoding)).

A font patch - new glyph tiles + width table in the menu glyph atlas at
`PROT.DAT` offset `0x11218` (see [`boot.md`](../../subsystems/boot.md) and
[`dialog-font.md`](../../formats/dialog-font.md)) - is the separate, larger
effort that would lift the printable-ASCII-only limitation for accented Latin
and other scripts across *all* text, not just these textures. It is out of
scope here.

The official PAL discs already carry such an atlas. See
[`pal-localizations.md`](../pal-localizations.md) for:

- the CP437-aligned accent byte→glyph map and the enumerated font-patch cell
  set;
- how the official French/German/Italian text aligns id-/order-for-order to
  the USA disc (`legaia-patcher translate diff-disc`);
- how to lift it onto USA coordinates (`translate lift-official`);
- the per-string versus per-MAN fit rate (`translate fit-report`).
