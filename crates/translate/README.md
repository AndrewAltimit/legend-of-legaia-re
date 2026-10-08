# legaia-translate

The language-pack pipeline behind `legaia-patcher translate`: a user-supplied
disc's text out to an editable YAML pack, and a filled pack back in as an
in-place reimport. Split out of [`legaia-patcher`](../patcher/README.md), which
re-exports it at its old path (`legaia_patcher::translation`) and keeps the
CLI.

Everything lives under the one `translation` module:

- **Pack + markup** - `pack` (the YAML schema and coverage stats), `markup`
  (the reversible text <-> game-byte codec), `symbols` (the `0xCE` escape
  names), `accents` (language-specific characters against the disc font).
- **Export / import** - `export` (walk a disc into a source pack), `import`
  (apply a filled pack), `segments` (the `0x1F`-lead dialog segment scanner
  both share), `stream_man` (the uncompressed scene MAN of a streaming scene),
  `ui` (the overlay-resident menu-string pools), `monster_names`.
- **Room for longer text** - `space` (how much room every string has),
  `code_strings` / `code_refs` / `name_pool` (moving `ui_menu`,
  `system_text` and `SCUS_942.54` names to longer homes and repointing every
  reference).
- **Other builds** - `build` (which retail build a disc is), `sjis` (the
  Japanese build's count-led Shift-JIS lines), `diff` / `refpair` / `fit`
  (cross-region alignment and fit rates), `lift` (re-keying another Latin
  disc's text onto USA coordinates), `coverage` (does the export carry every
  drawable line).

It writes through [`legaia-disc-patch`](../disc-patch/README.md)
(`DiscPatcher`, the space ledger, the MAN re-pack budget). Exported packs
carry game text: they are gitignored and never committed.

## See also

- [`docs/tooling/translation/`](../../docs/tooling/translation/index.md) -
  pack format, space and budgets, dialog import, UI strings, textures and
  fonts.
- [`docs/guides/translating.md`](../../docs/guides/translating.md)
