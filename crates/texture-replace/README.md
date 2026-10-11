# legaia-texture-replace

Image replacement on a user-supplied disc: the modules behind the patcher's
`tim-*`, battle-texture, monster-texture and save-icon subcommands and the
browser texture editor. Each decodes the retail image, encodes the user's
PNG against the same palettes and page budget, re-packs any LZS carrier and
writes the result through `legaia-disc-patch`'s `DiscPatcher`, refusing an
edit that does not fit before it writes anything.

It depends on [`legaia-disc-patch`](../disc-patch/README.md) and the parser
crates, and on no other patcher crate.
[`legaia-patcher`](../patcher/README.md), `legaia-delilas-party` and the web
viewer name its modules directly (`legaia_texture_replace::texture`,
`legaia_texture_replace::save_icon`, ...). The per-module
write-ups stay in the patcher README's
[texture replacement](../../docs/tooling/patcher-modules.md#texture-replacement-texture-module)
sections.

- `texture` - swap any TIM on the disc (raw and deep catalogs), with the
  multi-palette encode and LZS re-pack.
- `texture_palettes` - which palette the game draws each texel through, the
  texture editor's view of a multi-palette TIM.
- `battle_texture` - the party's in-battle art inside the player battle files
  (PROT 863..866).
- `monster_texture` - an enemy's 4bpp battle skin inside its monster-archive
  slot (PROT 867).
- `save_icon` - the save-slot portrait sheet in the menu overlay.

## See also

- [`docs/tooling/randomizer.md`](../../docs/tooling/randomizer.md#texture-replacement) -
  the user-facing texture replacement reference.
- [`docs/formats/tim.md`](../../docs/formats/tim.md) - the TIM format and the
  PNG-to-TIM encoder.
- [`docs/formats/save-icon.md`](../../docs/formats/save-icon.md) - the
  portrait sheet layout.
