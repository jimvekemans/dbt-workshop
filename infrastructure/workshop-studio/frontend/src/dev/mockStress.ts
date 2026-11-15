// A scenario that is not in the catalog (open it at #/s/stress-test/problem in the mock): every field at the maximum
// the backend's validators allow, to look at layout limits. Terms like [[feed]] sit in every text field.
import type { CodeBlockData, Scenario } from '../api/types'

const words = (n: number) => Array.from({ length: n }, (_, i) => ['the', 'carrier', 'sends', 'every', '[[feed]]', 'scan', 'through', 'a', 'small', 'message'][i % 10]).join(' ')
const chars = (n: number, seed: string) => (seed + ' ').repeat(Math.ceil(n / (seed.length + 1))).slice(0, n).trim()

const bigCode: CodeBlockData = {
  title: chars(80, 'A title that uses the whole width a code block title may have'),
  file_label: chars(80, 'The plain name of the file, as long as the backend allows'),
  file: 'models/edw_dv_bdv/carrier/a_rather_long_directory_name/another_long_directory_name/stg_carrier_events_with_a_long_name.sql',
  language: 'sql',
  gitlab_url: 'http://gitlab.local/workshop/dbt-vdp/-/blob/dev/models/x.sql#L1-L40',
  lines: Array.from({ length: 40 }, (_, i) => {
    const note = [3, 11, 24, 38].indexOf(i) >= 0 ? chars(200, `A note that explains line ${i + 1} and mentions a [[release]] term`) : undefined
    return { n: i + 1, text: i % 7 === 0 ? `-- ${'comment '.repeat(14)}` : `    select column_${i}, json_extract_string(payload, 'key_${i}') as value_${i}, _ingested_at from {{ ref("a_model_${i}") }}`, ...(note ? { note } : {}) }
  }),
}

const sides = (state: 'ok' | 'bad') => ({ title: chars(40, 'Title at its limit'), state, items: Array.from({ length: 5 }, (_, i) => chars(100, `Item ${i + 1} of five uses all hundred characters`)) })

export const STRESS: Scenario = {
  id: 'stress-test',
  group: 'daily',
  title: 'A late [[feed]] is named, the rest loads',
  promise: 'A promise that names a [[nightly load]] and stays readable on a card',
  minutes: 10,
  environment: 'dev',
  related: [{ id: 'switchover-date', why: chars(160, 'Why this one is related, at the full length of a hundred and sixty characters') }],
  problem: {
    headline: chars(90, 'A headline of ninety characters which should wrap onto two lines at most'),
    story: words(90),
    cost: chars(200, 'The cost sentence runs to two hundred characters and has a [[nightly load]] in it'),
    visual: {
      type: 'flow',
      nodes: Array.from({ length: 6 }, (_, i) => ({ id: `n${i}`, label: chars(30, `Node ${i + 1} label`), state: (['ok', 'bad', 'warn', 'fix', 'ok', 'weird'] as const)[i] as 'ok', note: chars(80, `Note of node ${i + 1} at eighty characters`) })),
      links: [['n0', 'n1'], ['n1', 'n2'], ['n2', 'n3'], ['n3', 'n4'], ['n4', 'n5']],
      caption: chars(140, 'A caption at the full length of one hundred and forty characters with a [[feed]]'),
    },
  },
  idea: {
    headline: chars(90, 'An idea headline of ninety characters which should also wrap onto two lines at most'),
    points: [0, 1, 2].map((i) => chars(160, `Point ${i + 1} uses all one hundred and sixty characters and has a [[release]] term`)),
    visual: { type: 'compare', left: sides('bad'), right: sides('ok'), caption: chars(140, 'Compare caption at its limit') },
  },
  code: [
    bigCode,
    { ...bigCode, title: 'A file outside the repository (no link is shown)', file: 'airflow/utils/other.py', language: 'python', gitlab_url: '', lines: bigCode.lines.slice(0, 6) },
    { title: 'Plain text', file_label: 'A plain note', file: 'notes.txt', language: 'text', gitlab_url: '', lines: [{ n: 1, text: 'No highlighting here.' }] },
  ],
  demo: {
    intro: chars(300, 'An introduction of three hundred characters that says what the audience is about to see in the demo'),
    steps: [
      { id: 'quiet', title: chars(60, 'A step with no inputs and a long title'), say: chars(300, 'What you will see, at the full three hundred characters, mentioning a [[feed]]'), button: chars(40, 'Press this long button label'), usual_seconds: 140, inputs: [], show: [{ type: 'result', title: 'One item' }], outcome: chars(300, 'The outcome sentence at three hundred characters with a [[release]] in it') },
      {
        id: 'many',
        title: 'A step with every kind of input',
        say: 'Four inputs and four evidence items.',
        button: 'Run with these settings',
        usual_seconds: 23,
        inputs: [
          { id: 'day', type: 'day', label: chars(60, 'Which day'), help: chars(160, 'Help text of one hundred and sixty characters'), default: '2026-11-11', min: '2026-11-08', max: '2026-11-15', step: null, unit: null, options: null },
          { id: 'slices', type: 'number', label: 'How many', help: null, default: 4, min: 2, max: 8, step: 1, unit: 'slices', options: null },
          { id: 'who', type: 'choice', label: 'Who', help: null, default: 'a', min: null, max: null, step: null, unit: null, options: ['a', 'b', 'c', 'd'].map((v) => ({ value: v, label: chars(30, `Option ${v} at thirty characters`) })) },
          { id: 'flag', type: 'toggle', label: 'A switch', help: { on: 'It is on: the extra check runs too.', off: 'It is off: the extra check is skipped.' }, default: false, min: null, max: null, step: null, unit: null, options: null },
        ],
        show: [
          { type: 'table', title: 'Eight columns, 200 rows', mock_rows: 200, columns: Array.from({ length: 8 }, (_, i) => ({ key: `c${i}`, label: `Column number ${i + 1}` })), highlight: { column: 'c0', equals: 'Flagged', as: 'bad' } },
          { type: 'bars', title: 'Bars', good: 'readable', bad: 'unreadable', x_label: 'Day' },
          { type: 'versions', title: 'Versions of the [[release]]', caption: 'Caption under versions.' },
          { type: 'pipeline', title: 'Checks' },
          { type: 'result', title: 'The answer' },
          { type: 'run_history', title: 'History', limit: 6, caption: 'With a [[feed]] caption.' },
        ],
        outcome: 'Done, with a [[feed]].',
      },
      { id: 'risky', title: 'A step that fails the first time', say: 'Press it twice.', button: 'Try it', inputs: [], show: [{ type: 'tasks', title: 'Tasks' }, { type: 'freshness', title: 'Freshness' }, { type: 'mail', title: 'Mail' }], outcome: 'Second time it works.' },
    ],
  },
}
