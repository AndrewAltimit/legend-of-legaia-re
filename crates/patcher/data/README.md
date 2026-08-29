# `crates/patcher/data`

Third-party mod payloads the patcher installs verbatim. Everything here is
**authored content contributed to this project**, not disc data.

## `zetaphoenix-super-arts-pack.bin` - Super Arts Pack, by ZetaPhoenix

3764 bytes, the RAM image of ZetaPhoenix's **Super Arts Pack** exactly as he
authored it, loaded at virtual address `0x801FD000`. It carries, in one block,
the pack's data tables, its fifteen Super Art names, four MIPS routines and
seventeen animation edit-lists; the layout is tabulated in
[`docs/tooling/randomizer.md`](../../../docs/tooling/randomizer.md#super-arts-pack-by-zetaphoenix).

The bytes are installed **unmodified** - the patcher parks this file in the
disc's `DMY.DAT` annex and streams it to `0x801FD000` at battle load, then
writes ten small same-size word edits so retail code reaches it. Nothing in the
block is rewritten, relocated or re-assembled.

**Provenance and licence.** Contributed by ZetaPhoenix as a GameShark-style RAM
patch. The pack's find/replace slots 0..4 per character reproduce the retail
Super Art trigger patterns, which this repository already documents in
[`crates/art/src/super_art.rs`](../../art/src/super_art.rs); the seventeen
animation edit-lists are `(offset, value)` scripts over art records, not copied
game data. **The licence has not been agreed in writing yet - confirm terms with
ZetaPhoenix before this file ships in a tagged release.** If he asks for it to be
withdrawn, delete this file and the `super_arts_pack` module: nothing else in the
crate depends on it.
