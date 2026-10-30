/*
 * The workbench's sound: the Audio tab (audio.toml - the pack's own music
 * tracks and sounds, and its music picks for battles, victories and modes)
 * and the map painter's Sound box (a map's music, its music zones and its
 * positional ambient sources).
 *
 * A retail cue is picked by its sound-test name from the disc's catalog; a
 * pack file is imported into assets/audio/ and previewed with the browser's
 * own decoder (an <audio> element over a blob URL). The probe beside each
 * file - rate, length, its own loop - is the validator's
 * (legaia_expansion_format::audio_clip::probe), so what the page shows is
 * what the engine reads. Fields follow docs/formats/expansion-pack-audio.md.
 */
import { h, grid, splitEd, listEd } from './expansion-workbench-forms.js';

const MODE_KINDS = ['poker', 'card_duel', 'golf', 'race', 'prop_hunt', 'party_night'];

let previewUrl = null;
let previewEl = null;

/** Play asset `path` through the browser's decoder (one preview at a time). */
function preview(ctx, path) {
  try {
    if (previewEl) { previewEl.pause(); previewEl = null; }
    if (previewUrl) { URL.revokeObjectURL(previewUrl); previewUrl = null; }
    const bytes = ctx.assetBytes ? ctx.assetBytes(path) : null;
    if (!bytes || !bytes.length) return;
    const type = /\.ogg$/i.test(path) ? 'audio/ogg' : 'audio/wav';
    previewUrl = URL.createObjectURL(new Blob([bytes], { type }));
    previewEl = new Audio(previewUrl);
    previewEl.play().catch(() => { /* blocked until a gesture; the button is one */ });
  } catch (e) {
    /* no audio in this browser */
  }
}

function stopPreview() {
  if (previewEl) { previewEl.pause(); previewEl = null; }
}

/** Import a .wav / .ogg into assets/audio/; `onDone(path)`. */
function importAudio(ctx, onDone) {
  const input = h('input', { type: 'file', accept: '.wav,.ogg', 'aria-label': 'Import an audio file' });
  input.addEventListener('change', async () => {
    const f = input.files && input.files[0];
    if (!f || !ctx.addAsset) return;
    const bytes = new Uint8Array(await f.arrayBuffer());
    const path = 'audio/' + f.name.replace(/[^A-Za-z0-9._-]/g, '_');
    ctx.addAsset(path, bytes);
    onDone(path);
  });
  return h('label', { class: 'xw-field' }, h('span', {}, 'Import WAV / Ogg', h('span', { class: 'xw-hint' }, ' into assets/audio/ (stays in your browser)')), input);
}

function audioPaths(ctx) {
  return ctx.assetPaths ? ctx.assetPaths().filter((p) => /\.(wav|ogg)$/i.test(p)) : [];
}

/** One track or sound: its file, gain, loop and a preview with the probe. */
function clipEd(ctx, box, c, p, isTrack) {
  const paths = audioPaths(ctx);
  box.append(grid(ctx, c, [
    paths.length ? { k: 'file', t: 'sel', opts: paths, label: 'File' } : { k: 'file', label: 'File', hint: 'relative to assets/' },
    { k: 'gain', t: 'ofloat', label: 'Gain', hint: '0..4 on top of normalisation; 1 when empty' },
    { k: 'normalize', t: 'dbool', label: 'Normalise to the reference loudness' },
    { k: 'loop_start', t: 'oint', label: 'Loop start (frame)', hint: 'the file\'s own loop, else 0' },
    { k: 'loop_end', t: 'oint', label: 'Loop end (frame)', hint: 'exclusive; the file\'s own loop, else its end' },
  ], p));
  // `loop`: a bool in the TOML, unset taking the kind's default.
  const loop = h('select', { 'aria-label': 'Loop' },
    h('option', { value: '' }, isTrack ? '(loops)' : '(plays once)'),
    h('option', { value: 'true' }, 'loop'),
    h('option', { value: 'false' }, 'play once'));
  loop.value = c.loop == null ? '' : String(c.loop);
  loop.addEventListener('change', () => {
    if (loop.value === '') delete c.loop; else c.loop = loop.value === 'true';
    ctx.changed(false);
  });
  box.append(h('label', { class: 'xw-field', 'data-path': `${p}.loop` }, h('span', {}, 'Loop'), loop));
  const info = c.file && ctx.audioInfo ? ctx.audioInfo(c.file) : null;
  const row = h('div', { class: 'xw-inline wb-inline' });
  row.append(h('button', { type: 'button', class: 'wb-mini', onclick: () => preview(ctx, c.file) }, 'Play'));
  row.append(h('button', { type: 'button', class: 'wb-mini', onclick: stopPreview }, 'Stop'));
  if (info) {
    row.append(h('span', { class: 'xw-hint' }, info.error
      ? ` ${info.error}`
      : ` ${info.rate} Hz, ${info.channels === 2 ? 'stereo' : 'mono'}, ${info.seconds.toFixed(2)} s (${info.frames} frames)${info.loop ? `, own loop ${info.loop[0]}..${info.loop[1]}` : ''}`));
  }
  box.append(row);
  box.append(importAudio(ctx, (path) => { c.file = path; ctx.changed(true); }));
}

/** The Audio tab: audio.toml. */
export function renderAudio(ctx, el) {
  const a = ctx.pack.audio || (ctx.pack.audio = {});
  el.append(h('p', { class: 'xw-note' }, 'audio.toml: the pack\'s own music and sounds (WAV or Ogg Vorbis under assets/), and which music the pack\'s fights, victories and modes play. Retail music is picked by its sound-test name; a pack track loops on its own loop points. Clips are normalised to the engine\'s reference loudness, and the game\'s music and sound volumes apply to them as to the disc\'s.'));
  el.append(h('h4', { class: 'xw-sub' }, 'Tracks', h('span', { class: 'xw-hint' }, ' music: a map, a zone, a boss or a mode plays one')));
  splitEd(ctx, el, a, 'tracks', '', {
    prefix: 'track',
    add: 'Track',
    make: () => ({ file: audioPaths(ctx)[0] || '' }),
    render: (box, c, p) => clipEd(ctx, box, c, p, true),
  });
  el.append(h('h4', { class: 'xw-sub' }, 'Sounds', h('span', { class: 'xw-hint' }, ' stingers, effects and ambient loops')));
  splitEd(ctx, el, a, 'sounds', '', {
    prefix: 'sound',
    add: 'Sound',
    make: () => ({ file: audioPaths(ctx)[0] || '' }),
    render: (box, c, p) => clipEd(ctx, box, c, p, false),
  });
  const m = a.music || (a.music = {});
  el.append(h('h4', { class: 'xw-sub' }, 'Music picks'));
  el.append(grid(ctx, m, [
    { k: 'battle', t: 'omusic', label: 'Battle', hint: 'fights on the pack\'s maps; retail\'s when unset' },
    { k: 'boss', t: 'omusic', label: 'Boss', hint: 'a boss with no music of its own' },
    { k: 'victory', t: 'omusic', label: 'Victory', hint: 'the results of a fight on the pack\'s maps' },
    { k: 'boss_victory', t: 'omusic', label: 'Boss victory', hint: 'victory when unset' },
  ], 'music'));
  const modes = m.modes || (m.modes = {});
  const keys = MODE_KINDS.concat(ctx.ids('mode'));
  el.append(h('h4', { class: 'xw-sub' }, 'Mode music', h('span', { class: 'xw-hint' }, ' by kind, or by instance (which wins)')));
  el.append(grid(ctx, modes, keys.map((k) => ({ k, t: 'omusic', label: k })), 'music.modes'));
}

/** The map painter's Sound box: `[audio]` of the map being edited. */
export function renderMapAudio(ctx, mctx, side, map) {
  const old = side.querySelector('.xw-soundbox');
  if (old) old.remove();
  const a = map.audio || (map.audio = {});
  const box = h('div', { class: 'xw-box xw-soundbox', 'data-path': 'audio' });
  box.append(h('div', { class: 'xw-box-head' }, 'Sound'));
  box.append(h('p', { class: 'xw-note' }, 'A zone plays its music while the player stands in it (the first match wins) and cross-fades as they cross its edge; an ambient source loops by distance from its tile, panned across the camera. A retail cue source holds one of four SPU voices.'));
  box.append(grid(mctx, a, [
    { k: 'music', t: 'omusic', label: 'Map music', hint: 'wins over the Music setting above' },
    { k: 'fade_frames', t: 'oint', label: 'Zone fade (frames)', hint: '45 when empty; 60 frames a second' },
  ], 'audio'));
  listEd(mctx, box, a, 'zones', 'audio', {
    title: 'Music zones',
    add: 'Zone',
    make: () => ({ id: `zone_${(a.zones || []).length + 1}`, rect: [0, 0, 1, 1], music: { kind: 'silence' } }),
    label: (z) => z.id || 'zone',
    render: (b2, z, p2) => b2.append(grid(mctx, z, [
      { k: 'id', label: 'Id' },
      { k: 'rect', t: 'tuple', n: 4, label: 'Tiles', hint: 'c0 r0 c1 r1, inclusive' },
      { k: 'music', t: 'music', label: 'Music' },
    ], p2)),
  });
  listEd(mctx, box, a, 'ambient', 'audio', {
    title: 'Ambient sources',
    add: 'Source',
    max: 16,
    make: () => ({ id: `source_${(a.ambient || []).length + 1}`, sound: { kind: 'sfx', id: 0x200 }, tile: [0, 0], radius: 768, inner: 128, volume: 1 }),
    label: (s) => s.id || 'source',
    render: (b2, s, p2) => b2.append(grid(mctx, s, [
      { k: 'id', label: 'Id' },
      { k: 'sound', t: 'sound', label: 'Sound' },
      { k: 'tile', t: 'tuple', n: 2, label: 'Tile' },
      { k: 'offset', t: 'tuple', n: 2, label: 'Offset', hint: 'x z, world units' },
      { k: 'radius', t: 'int', label: 'Silent at', hint: 'world units; a tile is 128' },
      { k: 'inner', t: 'int', label: 'Full volume within' },
      { k: 'volume', t: 'float', label: 'Volume 0..1' },
    ], p2)),
  });
  side.append(box);
}
