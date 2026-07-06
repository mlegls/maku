export const LIB_FILES = [
  'cards/lib/prelude.dmk',
  'cards/lib/touhou.dmk',
  'cards/lib/player-rig.dmk',
];

export const DEMO_CARDS = [
  { title: 'Reimu vs Mima', detail: 'full playable fight', path: 'cards/reimu_vs_mima.dmk' },
  { title: 'Co-op Rig', detail: 'two player host inputs', path: 'cards/coop.dmk' },
  { title: 'Duel Sandbox', detail: 'player/enemy interaction', path: 'cards/duel.dmk' },
  { title: 'PH Boss Spell 2', detail: 'translated boss pattern', path: 'cards/translations/ph_boss2_spell2.dmk' },
  { title: 'Player Homing', detail: 'translated player shots', path: 'cards/translations/player_homing.dmk' },
  { title: 'BOWAP', detail: 'folded bullet wall', path: 'cards/translations/130_bowap.dmk' },
  { title: 'Cradle', detail: 'translated cradle pattern', path: 'cards/translations/200_cradle.dmk' },
  { title: 'Aimed', detail: 'basic aimed shots', path: 'cards/translations/080_aimed.dmk' },
  { title: 'Dynamic Lasers', detail: 'pathers and lasers', path: 'cards/translations/070_dynamic_lasers.dmk' },
  { title: 'Exploding Stars', detail: 'guided fire translation', path: 'cards/translations/110_exploding_stars.dmk' },
];

export const TUTORIALS = [
  {
    title: '01 First Bullets',
    detail: 'frames, rings, fans',
    path: 'cards/tutorials/t01.dmk',
    doc: 'docs/tutorials/01-first-bullets.md',
  },
  {
    title: '02 Bullet Controls',
    detail: 'style, bursts, culling',
    path: 'cards/tutorials/t02.dmk',
    doc: 'docs/tutorials/02-bullet-controls.md',
  },
  {
    title: '03 Two Spells',
    detail: 'composition and reuse',
    path: 'cards/tutorials/t03.dmk',
    doc: 'docs/tutorials/03-two-spells.md',
  },
  {
    title: '04 Pathers and Lasers',
    detail: 'curves, beams, lifecycle',
    path: 'cards/tutorials/t04.dmk',
    doc: 'docs/tutorials/04-pathers-and-lasers.md',
  },
  {
    title: '05 Channels',
    detail: 'host inputs and exported state',
    path: 'cards/tutorials/t05.dmk',
    doc: 'docs/tutorials/05-channels.md',
  },
  {
    title: '06 Bosses',
    detail: 'states, phases, spawn-boss',
    path: 'cards/tutorials/t06.dmk',
    doc: 'docs/tutorials/06-bosses.md',
  },
  {
    title: '07 Guided Fires',
    detail: 'handles and tracking',
    path: 'cards/tutorials/t07.dmk',
    doc: 'docs/tutorials/07-guided-fires.md',
  },
  {
    title: '08 Stages',
    detail: 'waves, midbosses, campaigns',
    path: 'cards/tutorials/t08.dmk',
    doc: 'docs/tutorials/08-stages-and-campaigns.md',
  },
];

export const ALL_CARDS = [...TUTORIALS, ...DEMO_CARDS];

export const CARD_FILES = Array.from(new Set([
  ...LIB_FILES,
  ...ALL_CARDS.map(card => card.path),
  'cards/translations/020_gsrepeat.dmk',
  'cards/translations/040_spread.dmk',
  'cards/translations/060_polar.dmk',
]));

export function assetBase() {
  const configured = document.body?.dataset.assetBase || new URLSearchParams(location.search).get('base') || '/';
  return configured.endsWith('/') ? configured : `${configured}/`;
}

export function assetUrl(path) {
  return new URL(path, new URL(assetBase(), location.href)).toString();
}

