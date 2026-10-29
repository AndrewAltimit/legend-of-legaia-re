# Legend of Legaia: Rebirth

The flagship expansion pack for the port, and the worked example of the
expansion-pack format. It is written with exactly the files, rules and
tooling a modder uses - nothing in the engine knows this pack by name - so
anything Rebirth does, another pack can do.

## What it will contain

- An endgame tower: new fights and superbosses for a party near level 99,
  with the experience and drops to make the climb worth it.
- A quest chain that sends the party back through the existing world.
- Custom maps assembled from the disc's own scenery.
- Trading cards won in battle, then traded or sold.
- New modes the original never had.

Today the pack is its manifest only; the content sections arrive as the
engine grows the runtime for each.

## No game data

The pack ships no bytes from the disc. Every monster, mesh, texture,
scene or music cue it uses is named by a coordinate the engine resolves
against the player's own copy, so the pack is safe to share. The
validator enforces this (`asset expansion validate packs/rebirth`).

## Working with it

```bash
asset expansion validate packs/rebirth
asset expansion bundle packs/rebirth -o rebirth.lxpack
asset expansion unbundle rebirth.lxpack -o rebirth-copy
```

The format reference is
[`docs/formats/expansion-pack.md`](../../docs/formats/expansion-pack.md).
