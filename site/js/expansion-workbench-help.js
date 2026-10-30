/*
 * The workbench's contextual help: a "?" per tab and per section that opens
 * the format reference at the section the editor writes
 * (site/_content/formats/expansion-pack*.html, mirrors of docs/formats/).
 * The links are plain anchors, opened in a new tab so the pack stays put.
 */
import { h } from './expansion-workbench-forms.js';

const F = '../formats/';
const MODES = '../subsystems/expansion-modes.html';

/** A "?" that opens the format doc's section in a new tab. */
export function helpLink(href, what) {
  return h('a', { class: 'xw-help', href, target: '_blank', rel: 'noopener', title: `Format reference: ${what}`, 'aria-label': `Help: ${what}` }, '?');
}

/** Per tab: the file it edits and the reference section for it. */
export const TAB_HELP = {
  manifest: [`${F}expansion-pack.html#the-manifest`, 'the manifest'],
  battle: [`${F}expansion-pack-battle.html`, 'battles'],
  progression: [`${F}expansion-pack-battle.html#progression`, 'progression'],
  quests: [`${F}expansion-pack-quests.html#queststoml`, 'quests'],
  dialog: [`${F}expansion-pack-quests.html#dialogtoml`, 'dialogue'],
  cutscenes: [`${F}expansion-pack-cutscenes.html#cutscenestoml`, 'cutscenes'],
  cards: [`${F}expansion-pack.html#cards-toml`, 'trading cards'],
  items: [`${F}expansion-pack-items.html#itemstoml`, 'items'],
  arts: [`${F}expansion-pack-arts.html#artstoml`, 'arts'],
  spells: [`${F}expansion-pack-arts.html#spellstoml`, 'Seru spells'],
  modes: [`${F}expansion-pack.html#modes-toml`, 'game modes'],
  audio: [`${F}expansion-pack-audio.html#audiotoml`, 'audio'],
  maps: [`${F}expansion-pack-maps.html`, 'maps'],
  files: [`${F}expansion-pack.html#layout`, 'the pack layout'],
};

/** Per section heading (`tab:heading text`): the reference section. */
const SECTION_HELP = {
  'battle:Monsters': `${F}expansion-pack-battle.html#monsters`,
  'battle:Formations': `${F}expansion-pack-battle.html#formations`,
  'battle:Bosses': `${F}expansion-pack-battle.html#bosses`,
  'quests:Quests': `${F}expansion-pack-quests.html#queststoml`,
  'quests:Challenges': `${F}expansion-pack-quests.html#challenges`,
  'cards:Sell prices': `${MODES}#trading-cards`,
  'cards:Cards': `${F}expansion-pack.html#cards-toml`,
  'cards:Boosters': `${F}expansion-pack.html#cards-toml`,
  'cards:Starter deck': `${MODES}#trading-cards`,
  'cards:Dealers': `${MODES}#trading-cards`,
  'items:Items': `${F}expansion-pack-items.html#itemstoml`,
  'items:Sets': `${F}expansion-pack-items.html#itemstoml`,
  'items:Shops': `${F}expansion-pack-items.html#itemstoml`,
  'arts:Arts': `${F}expansion-pack-arts.html#artstoml`,
  'arts:Chains': `${F}expansion-pack-arts.html#how-a-pack-art-runs`,
  'modes:Poker tables': `${MODES}#poker-at-a-table`,
  'modes:Card duels': `${MODES}#grid-duel-at-a-table`,
  'modes:Golf courses': `${MODES}#in-world-golf-and-racing`,
  'modes:Race tracks': `${MODES}#racing`,
  'modes:Prop hunt arenas': `${MODES}#prop-hunt`,
  'modes:Party nights': `${MODES}#party-night`,
  'audio:Tracks': `${F}expansion-pack-audio.html#audiotoml`,
  'audio:Sounds': `${F}expansion-pack-audio.html#sounds-and-ambient-sources`,
  'audio:Music picks': `${F}expansion-pack-audio.html#music-and-sound-references`,
  'audio:Mode music': `${F}expansion-pack-audio.html#audiotoml`,
};

/** Put the tab's "?" at the top of its panel and one on each top-level
 * section heading that has a reference section. */
export function addSectionHelp(tab, file, panel) {
  const t = TAB_HELP[tab];
  if (t) {
    panel.prepend(h('div', { class: 'xw-tabhelp' }, file ? h('code', {}, file) : null, h('span', { class: 'xw-hint' }, ` - format reference: ${t[1]}`), helpLink(t[0], t[1])));
  }
  for (const hd of panel.querySelectorAll(':scope > h4.xw-sub')) {
    const text = (hd.firstChild && hd.firstChild.nodeType === 3 ? hd.firstChild.textContent : hd.textContent).trim();
    const href = SECTION_HELP[`${tab}:${text}`];
    if (href) hd.append(helpLink(href, text.toLowerCase()));
  }
}
