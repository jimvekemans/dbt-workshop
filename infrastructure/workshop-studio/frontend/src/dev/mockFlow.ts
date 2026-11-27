// A sample FLOW (a long scenario) and a sample closing overview for the mock: six stages with mixed statuses (the last
// one, "everything switched on", has none), five code blocks with badges, and a `_roadmap.yaml` with two extra rows.
// The mock adds the flow to its own group and the roadmap to the catalog (see mockContent.withFlowAndRoadmap).
import type { Catalog, CatalogScenario, CodeBlockData, EvidenceItem, Quiz, Roadmap, Scenario, StepDef } from '../api/types'

const codeBlock = (n: number, title: string, label: string, status?: 'today' | 'next'): CodeBlockData => ({
  title,
  file_label: label,
  file: `models/flow_sample/stage_${n}.sql`,
  language: 'sql',
  gitlab_url: `http://gitlab.local/workshop/dbt-vdp/-/blob/dev/models/flow_sample/stage_${n}.sql#L1-L6`,
  ...(status ? { status } : {}),
  lines: [
    { n: 1, text: `-- stage ${n}: ${label.toLowerCase()}` },
    { n: 2, text: 'select order_id, customer_id, ordered_at' },
    { n: 3, text: "from {{ ref('stg_orders') }}", note: 'It reads the cleaned orders, never the raw feed.' },
    { n: 4, text: "where ordered_at >= current_date - interval '7' day" },
    { n: 5, text: 'order by ordered_at desc', note: 'The newest orders come first, so a late feed is easy to see.' },
    { n: 6, text: 'limit 100' },
  ],
})

const show = (type: EvidenceItem['type'], title: string, extra: Record<string, unknown> = {}): EvidenceItem => ({ type, title, ...extra })

const stage = (id: string, title: string, say: string, button: string, feature: string | null, status: 'today' | 'next' | null, items: EvidenceItem[], usual = 6): StepDef => ({
  id,
  title,
  say,
  button,
  ...(feature && status ? { feature, status } : {}),
  inputs: [],
  show: items,
  outcome: `${title}: done.`,
  usual_seconds: usual,
})

export const FLOW: Scenario = {
  id: 'flow-test',
  kind: 'flow',
  group: 'flows',
  title: 'From a new order to a trusted report',
  promise: 'One long walk from the moment data arrives to the moment a report is released, stage by stage.',
  minutes: 25,
  environment: 'dev',
  related: [{ id: 'switchover-date', why: 'The same story, but only the part about a feed that changes.' }],
  problem: {
    headline: 'Nobody can say which parts of the route are safe',
    story:
      'A report is the end of a long route. Data arrives from other companies, is checked, turned into tidy tables, tried out on a private copy and finally released.\n\nToday each part is looked after by a different person, and nobody can say which parts are solid and which are still being built. When the report is wrong, everybody starts looking in a different place.',
    cost: 'Without one picture of the route, every mistake costs hours of searching and every promise is a guess.',
    visual: {
      type: 'flow',
      nodes: [
        { id: 'a', label: 'Data arrives', state: 'ok' },
        { id: 'b', label: 'Checks', state: 'ok' },
        { id: 'c', label: 'Tidy tables', state: 'warn' },
        { id: 'd', label: 'Release', state: 'idle' },
      ],
      links: [['a', 'b'], ['b', 'c'], ['c', 'd']],
      caption: 'Some stages are in place, others are not yet.',
    },
  },
  idea: {
    headline: 'Walk the whole route once, and mark each stage honestly',
    points: ['Every stage has one plain name.', 'Every stage is marked: in place today, or the next step.', 'The demo walks through all of them in order.'],
    visual: {
      type: 'flow',
      nodes: [
        { id: 'a', label: 'Data arrives', state: 'ok' },
        { id: 'b', label: 'Checks', state: 'ok' },
        { id: 'c', label: 'Tidy tables', state: 'ok' },
        { id: 'd', label: 'Release', state: 'fix' },
      ],
      links: [['a', 'b'], ['b', 'c'], ['c', 'd']],
    },
  },
  code: [
    codeBlock(1, 'How the data is picked up', 'The recipe for the daily delivery', 'today'),
    codeBlock(2, 'How the data is checked', 'The checks on arrival', 'today'),
    codeBlock(3, 'How the tidy tables are built', 'The recipe for the tidy orders', 'today'),
    codeBlock(4, 'How a change is tried out', 'The recipe for the private copy', 'next'),
    codeBlock(5, 'How a release is ordered', 'The recipe for the release steps', 'next'),
  ],
  demo: {
    intro: 'Six short stages, one after the other. Each one says whether it is in place today or still a next step.',
    steps: [
      stage('pick_up', 'Pick up the data', 'The delivery of the day is picked up and counted.', 'Pick up the data', 'Daily data delivery', 'today', [show('number', 'Orders picked up', { unit: 'orders' })]),
      stage('check', 'Check the data on arrival', 'Every [[feed]] is checked before anybody uses it.', 'Check the data', 'Automatic data checks', 'today', [show('pipeline', 'The checks')]),
      stage('build', 'Build the tidy tables', 'The nightly build turns the raw data into tidy tables.', 'Build the tables', 'Nightly table build', 'today', [show('tasks', 'The nightly build')]),
      stage('try', 'Try a change on a private copy', 'A change is tried on a private copy first.', 'Try the change', 'Private test copy', 'next', [show('table', 'What the copy shows')]),
      stage('release', 'Release in a fixed order', 'The change moves from development to production, one step at a time.', 'Start the release', 'Step-by-step releases', 'next', [show('versions', 'Versions in use')]),
      stage('all', 'Everything switched on', 'All stages together, as they would run on a normal day.', 'Run the whole route', null, null, [show('run_history', 'The last runs', { limit: 4 })]),
    ],
  },
  prepare_usual_seconds: 90,
}

/** Technical keywords of the sample flow (scenario level and per stage) and of a few ordinary scenarios (for the presenter's switch). */
export const FLOW_TECH = ['vdp_ci', 'state:modified+', '--empty']
export const STAGE_TECH: Record<string, string> = {
  check: 'dbt source freshness',
  build: 'dbt build --select tag:nightly',
  release: 'deploy_prd (manual)',
}
export const MOCK_TECH: Record<string, string[]> = {
  'switchover-date': ['dbt_env_vars', 'DBT_CARRIER_V2_FROM', 'stg_carrier_events'],
  'late-delivery': ['dbt source freshness'],
  'checked-before-merge': ['CI verify', 'data tests on a merge request'],
}

FLOW.tech = FLOW_TECH
for (const st of FLOW.demo.steps) if (STAGE_TECH[st.id]) st.tech = STAGE_TECH[st.id]

export const ROADMAP: Roadmap = {
  title: 'Where we stand, and the road ahead',
  intro: 'Here is what already works in this setup, and what we would add next.',
  closing: 'Nothing on the right-hand side is a promise of a date; it is the list we would work through together.',
  extras: [
    { area: 'Access', feature: 'Each team sees only its own data', status: 'next', note: 'Needs one more round of agreements with the teams.', why: 'Every team can read the data of the other teams.' },
    { area: 'Access', feature: 'A named person approves every release', status: 'today' },
  ],
}

/** The `why` line of a few stages (next steps, and one stage in place today). */
const STAGE_WHY: Record<string, string> = {
  try: 'A change goes live without anybody having seen it work.',
  release: 'A release is a manual job, so it waits for one person.',
  check: 'Bad data is only noticed when somebody reads the report.',
}

/** The mock's catalog: the fixture plus a group with the sample flow, a few status labels and the closing overview. */
export function withFlowAndRoadmap(c: Catalog, recorded: (id: string) => boolean): Catalog {
  const out = structuredClone(c)
  const mark = (id: string, feature: string, status: 'today' | 'next', why?: string) => {
    const s = out.groups.flatMap((g) => g.scenarios).find((x) => x.id === id)
    if (s) Object.assign(s, { feature, status, ...(why ? { why } : {}) })
  }
  mark('switchover-date', 'Feeds that change format', 'today')
  mark('late-delivery', 'A late feed is reported', 'today')
  mark('right-keys-only', 'Every environment has its own keys', 'next', 'One leaked key opens every environment.')
  const entry: CatalogScenario = {
    id: FLOW.id,
    kind: 'flow',
    title: FLOW.title,
    promise: FLOW.promise,
    minutes: FLOW.minutes,
    environment: FLOW.environment,
    related: FLOW.related,
    recorded: recorded(FLOW.id),
    tech: FLOW_TECH,
    steps: FLOW.demo.steps.map((s) => ({ id: s.id, title: s.title, feature: s.feature ?? null, status: s.status ?? null, ...(STAGE_WHY[s.id] ? { why: STAGE_WHY[s.id] } : {}), ...(STAGE_TECH[s.id] ? { tech: STAGE_TECH[s.id] } : {}) })),
  }
  for (const s of out.groups.flatMap((g) => g.scenarios)) if (MOCK_TECH[s.id]) s.tech = MOCK_TECH[s.id]
  out.groups.push({ id: 'flows', title: 'The whole route', blurb: 'One long walk through every stage, with what is in place today and what is next.', scenarios: [entry] })
  out.roadmap = structuredClone(ROADMAP)
  return out
}

// Sample questions for the room (the quiz): one on two ordinary scenarios and one on three stages of the sample flow.
export const MOCK_QUIZ: Record<string, Quiz> = {
  'skip-one-table': {
    situation: 'Het is nacht. De feed van de pakjesdienst komt aan met 3 scans zonder scannummer, en de pakjesdienst heeft een dag nodig om dat te herstellen. Morgenvroeg opent het management zijn rapport.',
    question: 'Wat doe je vannacht met de load?',
    options: [
      { label: 'Alles laten wachten tot de feed hersteld is', impact: 'Geen enkel fout cijfer, maar elk rapport blijft een dag oud. Ook de rapporten die niets met de pakjesdienst te maken hebben.', tone: 'bad' },
      { label: 'De check uitzetten en alles laten doorlopen', impact: 'Elk rapport is op tijd, maar de 3 foute scans zitten erin. Niemand weet nog welke cijfers te vertrouwen zijn.', tone: 'bad' },
      { label: 'Enkel de pakjesdienst en wat ervan afhangt een nacht overslaan', impact: 'Alle andere rapporten zijn op tijd en gecheckt. Wat van de pakjesdienst afhangt, wacht een nacht, zonder nieuwe [[release]].', tone: 'good', platform: true },
    ],
    takeaway: 'Sla het kapotte deel over, niet de hele nacht.',
    minutes: 2,
  },
  'late-delivery': {
    situation: 'De feed met bestellingen is vannacht niet aangekomen. De andere feeds zijn er wel.',
    question: 'Wat doet de nachtelijke load?',
    options: [
      { label: 'De late feed melden en de rest gewoon laden', impact: 'Elk ander rapport is op tijd, en iedereen ziet welke feed ontbreekt.', tone: 'good', platform: true },
      { label: 'Wachten tot alles binnen is', impact: 'Geen enkel rapport is klaar tegen de ochtend.', tone: 'bad' },
    ],
    takeaway: 'Eén late feed houdt de rest niet tegen.',
    minutes: 1,
  },
  'flow-test/check': {
    situation: 'Een feed komt binnen met rijen die niet kloppen. Niemand heeft er al naar gekeken.',
    question: 'Wanneer kijk je de data na?',
    options: [
      { label: 'Pas als iemand een fout cijfer meldt', impact: 'Het foute cijfer stond al in een rapport voor iemand het zag.', tone: 'bad' },
      { label: 'Bij aankomst, voor iemand ze gebruikt', impact: 'Een kapotte feed stopt aan de deur. Dat kost een paar minuten per load.', tone: 'good', platform: true },
    ],
    takeaway: 'Nakijken bij aankomst is goedkoper dan herstellen achteraf.',
    minutes: 2,
  },
  'flow-test/try': {
    situation: 'Een developer wil een berekening aanpassen. De tabel wordt elke dag door collega\'s gelezen.',
    question: 'Waar probeer je de wijziging uit?',
    options: [
      { label: 'In de gedeelde tabel, buiten de kantooruren', impact: 'Het werkt meestal, maar wie vroeg begint, leest een half aangepaste tabel.', tone: 'warn' },
      { label: 'Op een privékopie', impact: 'Niemand merkt iets tot de wijziging goedgekeurd is. Het platform heeft dit nog niet.', tone: 'good', platform: true },
      { label: 'Rechtstreeks in productie, met een collega erbij', impact: 'Een fout staat meteen in de rapporten van iedereen.', tone: 'bad' },
    ],
    takeaway: 'Uitproberen hoort op een plek waar niemand anders leest.',
    minutes: 3,
  },
  'flow-test/release': {
    situation: 'De wijziging is goedgekeurd. Ze moet nu naar de mensen die de rapporten lezen.',
    question: 'Hoe breng je ze naar productie?',
    options: [
      { label: 'Stap per stap, in een vaste volgorde', impact: 'Een fout valt op in development of validatie, voor productie ze ziet.', tone: 'good', platform: true },
      { label: 'Meteen overal tegelijk', impact: 'Het gaat snel, en een fout is meteen overal.', tone: 'bad' },
    ],
    minutes: 2,
  },
}

/** Adds the sample questions to a catalog (dev mock and tests only; the real ones come from the scenario files). */
/** Sample "Bij ons gebeurd" lines: what really happened, next to the question of a scenario or a stage. */
export const MOCK_REALITY: Record<string, string> = {
  'skip-one-table': 'De lijst om kapotte tabellen een nacht over te slaan bestaat al van de eerste nachtelijke load. Ze moest één keer hersteld worden: na een verhuis van de load werkte ze niet meer zoals bedoeld.',
  'flow-test/try': 'Eén wijziging werd in zeven dagen vijf keer goedgekeurd en op releasedag vier keer teruggedraaid.',
}

export function withQuiz(c: Catalog): Catalog {
  const out = structuredClone(c)
  for (const s of out.groups.flatMap((g) => g.scenarios)) {
    if (MOCK_QUIZ[s.id]) s.quiz = MOCK_QUIZ[s.id]
    if (MOCK_REALITY[s.id]) s.reality = MOCK_REALITY[s.id]
    for (const st of s.steps ?? []) {
      if (MOCK_QUIZ[`${s.id}/${st.id}`]) st.quiz = MOCK_QUIZ[`${s.id}/${st.id}`]
      if (MOCK_REALITY[`${s.id}/${st.id}`]) st.reality = MOCK_REALITY[`${s.id}/${st.id}`]
    }
  }
  return out
}
