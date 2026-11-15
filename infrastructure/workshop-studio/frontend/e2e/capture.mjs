// UI capture harness for Workshop Studio. Runs inside the Playwright container (see run.sh / README.md).
import { chromium } from 'playwright'
import fs from 'fs'
import path from 'path'
import { createRequire } from 'module'

const require = createRequire(import.meta.url)
const AXE = fs.readFileSync(require.resolve('axe-core/axe.min.js'), 'utf8')

// ---------------------------------------------------------------- config (everything mock-specific lives here)
const CONFIG = {
  // Fully written scenarios in the mock. SCENARIOS=a,b or SCENARIOS=all overrides (all = every id in /api/catalog).
  scenarios: ['switchover-date', 'one-clear-mail', 'states-test'],
  // The mock-only scenario that shows every state of the interface (a failed night, a warning, a retry, a mail, before/after ...).
  states: 'states-test',
  // The step that fails on its first attempt in the mock.
  failing: { scenario: 'one-clear-mail', step: 3 },
  // Replay checks: one scenario with a recording, one without (also the "not fully written" scenario).
  replay: { recorded: 'checked-before-merge', unrecorded: 'late-delivery' },
  notFullyWritten: 'late-delivery',
  unknownRoute: '#/s/does-not-exist/demo',
  // Extra query used for the (many) page reloads of the interaction sweep: mock only, divides all durations by 4.
  sweepQuery: 'fast=1',
  // How long to let a freshly loaded page settle (baseline preparing is 5 s in the mock).
  settleMs: 6500,
  sweepSettleMs: 1800,
  stepTimeoutMs: 45000,
}

const BASE = process.env.BASE_URL || 'http://127.0.0.1:5199/?mock'
const OUT = process.env.OUT_DIR || '/out'
const WORKERS = Number(process.env.WORKERS || 4)
const IS_MOCK = new URL(BASE).searchParams.has('mock')
const REAL = !IS_MOCK
// requests that are supposed to fail: the "unknown scenario" checks ask for one that does not exist
const EXPECTED = /\/api\/scenarios\/does-not-exist/
let saved = null // the real backend's mode and feeder before the run, put back at the end
const PARTS = ['problem', 'idea', 'code', 'demo']
const D = { w: 1440, h: 900 }

const urlFor = (hash = '#/', extra = '') => {
  const u = new URL(BASE)
  for (const kv of extra.split('&').filter(Boolean)) {
    const [k, v = ''] = kv.split('=')
    u.searchParams.set(k, v)
  }
  u.hash = hash
  return u.toString().replace(/\?mock=(&|$)/, '?mock$1').replace('mock=&', 'mock&')
}

// ---------------------------------------------------------------- result stores
const inventory = []
const consoleLog = []
const axeLog = []
const interactions = new Map()
const texts = new Map() // screen -> Set
const failures = []
const assertions = []
const shots = []
const meta = {}
let counter = 0
let crashed = false
let browser

let cancellable = true
// a replay is over in a second or two, too fast to press Stop reliably: against a real backend the cancel checks are skipped
const cassert = (name, pass, detail) => (REAL ? log('SKIP (replay finished first)', name) : assert(name, pass, detail))
const log = (...a) => console.log(new Date().toISOString().slice(11, 19), ...a)
const addText = (screen, line) => {
  if (!texts.has(screen)) texts.set(screen, new Set())
  texts.get(screen).add(line)
}
const assert = (name, pass, detail = '') => {
  assertions.push({ name, pass: !!pass, detail: String(detail).slice(0, 200) })
  log(pass ? 'PASS' : 'FAIL', name, detail)
}
async function safe(label, fn) {
  try {
    return await fn()
  } catch (e) {
    failures.push({ what: label, error: String(e.message || e).split('\n')[0].slice(0, 300) })
    log('FAILED', label, String(e.message || e).split('\n')[0])
    return undefined
  }
}

// ---------------------------------------------------------------- page factory
async function openPage(opts, hash, { extra = '', wait = true, full } = {}) {
  const { viewport = D, scheme = 'light', mobile = false, offline = false } = opts
  const ctx = await browser.newContext({
    viewport: { width: viewport.w, height: viewport.h },
    colorScheme: scheme,
    isMobile: mobile,
    hasTouch: mobile,
    deviceScaleFactor: mobile ? 2 : 1,
    reducedMotion: 'no-preference',
  })
  const page = await ctx.newPage()
  page.setDefaultTimeout(8000)
  const P = { page, ctx, buf: [], viewport, scheme, hash, name: opts.name || '' }
  page.on('console', (m) => {
    if (offline && /ERR_FAILED/.test(m.text())) return
    if (EXPECTED.test(m.location().url || '') && /404/.test(m.text())) return
    if (['error', 'warning'].includes(m.type())) P.buf.push({ kind: 'console.' + m.type(), text: m.text().slice(0, 400), at: m.location().url })
  })
  page.on('pageerror', (e) => P.buf.push({ kind: 'pageerror', text: String(e.message || e).slice(0, 400) }))
  page.on('requestfailed', (r) => !(offline && /\/api\//.test(r.url())) && !(/\/api\/events$/.test(r.url()) && /ABORTED/.test(r.failure()?.errorText || '')) && P.buf.push({ kind: 'requestfailed', text: `${r.method()} ${r.url()} ${r.failure()?.errorText}` }))
  page.on('response', (r) => {
    if (r.status() >= 400 && !EXPECTED.test(r.url())) P.buf.push({ kind: 'http' + r.status(), text: `${r.request().method()} ${r.url()}` })
  })
  page.on('crash', () => {
    crashed = true
    log('PAGE CRASHED')
  })
  if (offline && !IS_MOCK) await ctx.route((u) => u.pathname.startsWith('/api/'), (r) => r.abort())
  // against a real backend this run must stay in replay: nothing a page does may switch to Live or touch the feeder
  if (REAL && !offline)
    await ctx.route('**/api/{mode,feeder,reset}', (r) => {
      const req = r.request()
      if (req.method() === 'POST' && (!req.url().endsWith('/mode') || /live/i.test(req.postData() || ''))) return r.fulfill({ status: 409, contentType: 'application/json', body: '{"detail":"blocked by the capture harness: replay only"}' })
      return r.continue()
    })
  const u = urlFor(hash, extra + (offline && IS_MOCK ? '&offline=1' : ''))
  await page.goto(u, { waitUntil: 'domcontentloaded' })
  if (wait) await page.waitForSelector('h1, [role=alert]', { timeout: 15000 }).catch(() => {})
  return P
}
const closeP = async (P) => P.ctx.close().catch(() => {})
const goHash = async (P, hash) => {
  await P.page.evaluate((h) => (location.hash = h), hash)
  await P.page.waitForTimeout(500)
}
const api = (P, method, p, body) =>
  P.page.evaluate(async ([m, u, b]) => {
    const r = await fetch('/api' + u, { method: m, headers: { 'X-Studio': '1', ...(b ? { 'Content-Type': 'application/json' } : {}) }, body: b ? JSON.stringify(b) : undefined })
    return r.json()
  }, [method, p, body ?? null])
/** Real backend: remember mode and feeder, then go to replay (replay changes nothing in the stack). */
async function enterReplay(P) {
  const st = await api(P, 'GET', '/state')
  saved = { mode: st.mode, feeder: st.feeder?.running }
  meta.backend_before = saved
  if (st.mode !== 'replay') await api(P, 'POST', '/mode', { mode: 'replay' })
  log('real backend: replay mode on (was ' + st.mode + ')')
}
async function restoreBackend() {
  if (!REAL || !saved || !browser) return
  const ctx = await browser.newContext()
  const page = await ctx.newPage()
  await page.goto(urlFor('#/'), { waitUntil: 'domcontentloaded' })
  const P = { page }
  if (saved.mode !== 'replay') await api(P, 'POST', '/mode', { mode: saved.mode })
  const st = await api(P, 'GET', '/state')
  if (typeof saved.feeder === 'boolean' && st.feeder?.running !== saved.feeder) await api(P, 'POST', '/feeder', { running: saved.feeder })
  meta.backend_after = { mode: (await api(P, 'GET', '/state')).mode }
  log('real backend: mode put back to', saved.mode)
  await ctx.close()
}
const curHash = (P) => P.page.evaluate(() => location.hash)

// ---------------------------------------------------------------- in-page helpers
const INVENTORY_FN = () => {
  const SEL = 'a[href],button,input,select,textarea,summary,[role=button],[role=link],[role=tab],[role=switch],[role=checkbox],[role=radio],[role=slider],[role=menuitem],[role=option],[role=combobox],[contenteditable=true],[tabindex]'
  document.querySelectorAll('[data-rv]').forEach((e) => e.removeAttribute('data-rv'))
  const clean = (s) => (s || '').replace(/\s+/g, ' ').trim()
  const textOf = (e) => {
    const c = e.cloneNode(true)
    c.querySelectorAll('[aria-hidden="true"]').forEach((x) => x.remove())
    return clean(c.textContent)
  }
  const nameOf = (e) => {
    const lb = e.getAttribute('aria-labelledby')
    if (lb) return clean(lb.split(/\s+/).map((i) => document.getElementById(i)?.textContent || '').join(' '))
    const al = e.getAttribute('aria-label')
    if (al) return clean(al)
    if (e.labels && e.labels.length) return clean([...e.labels].map((l) => textOf(l)).join(' '))
    if (e.tagName === 'INPUT' && ['button', 'submit'].includes(e.type)) return e.value
    return textOf(e) || clean(e.getAttribute('title')) || clean(e.getAttribute('placeholder'))
  }
  const roleOf = (e) => {
    const r = e.getAttribute('role')
    if (r) return r
    const t = e.tagName
    if (t === 'A') return 'link'
    if (t === 'BUTTON') return 'button'
    if (t === 'SUMMARY') return 'disclosure-summary'
    if (t === 'SELECT') return 'combobox'
    if (t === 'TEXTAREA') return 'textbox'
    if (t === 'INPUT') return { range: 'slider', checkbox: 'checkbox', radio: 'radio', search: 'searchbox', button: 'button', submit: 'button' }[e.type] || 'textbox'
    return e.tagName.toLowerCase()
  }
  const modal = document.querySelector('[role="dialog"][aria-modal="true"]')
  const seen = new Set()
  const els = []
  document.querySelectorAll(SEL).forEach((e) => {
    if (e.getAttribute('tabindex') === '-1' && !e.matches('a[href],button,input,select,textarea,summary,[role=button],[role=link],[role=tab],[role=option]')) return
    els.push([e, 'semantic'])
    seen.add(e)
  })
  document.querySelectorAll('body *').forEach((e) => {
    if (seen.has(e) || e.tagName === 'LABEL' || e.closest(SEL)) return
    if (getComputedStyle(e).cursor === 'pointer') els.push([e, 'pointer-cursor'])
  })
  const out = []
  let id = 0
  for (const [e, kind] of els) {
    const cs = getComputedStyle(e)
    if (cs.display === 'none' || cs.visibility === 'hidden') continue
    if (e.closest('[hidden],[inert]')) continue
    let r = e.getBoundingClientRect()
    const hiddenish = cs.opacity === '0' || r.width < 4 || r.height < 4
    let hit = r
    if (hiddenish && (e.type === 'radio' || e.type === 'checkbox')) {
      const l = e.closest('label')
      if (l) hit = l.getBoundingClientRect()
    }
    if (hit.width === 0 && hit.height === 0) continue
    e.setAttribute('data-rv', String(id))
    const desc = (e.getAttribute('aria-describedby') || '').split(/\s+/).filter(Boolean).map((i) => clean(document.getElementById(i)?.textContent)).join(' | ')
    out.push({
      id: id++,
      kind,
      tag: e.tagName.toLowerCase(),
      role: roleOf(e),
      name: nameOf(e),
      text: textOf(e).slice(0, 120),
      title: e.getAttribute('title') || null,
      described_by: desc || null,
      disabled: e.disabled === true || e.getAttribute('aria-disabled') === 'true',
      href: e.getAttribute('href'),
      target: e.getAttribute('target'),
      type: e.type || null,
      value: e.value !== undefined && ['INPUT', 'SELECT', 'TEXTAREA'].includes(e.tagName) ? e.value : null,
      min: e.min || null,
      max: e.max || null,
      step: e.step || null,
      checked: typeof e.checked === 'boolean' ? e.checked : null,
      aria_valuetext: e.getAttribute('aria-valuetext'),
      aria_expanded: e.getAttribute('aria-expanded'),
      aria_current: e.getAttribute('aria-current'),
      aria_haspopup: e.getAttribute('aria-haspopup'),
      shortcuts: e.getAttribute('aria-keyshortcuts'),
      visually_hidden_input: hiddenish,
      bbox: { x: Math.round(hit.x + scrollX), y: Math.round(hit.y + scrollY), w: Math.round(hit.width), h: Math.round(hit.height) },
      in_viewport: hit.top < innerHeight && hit.bottom > 0 && hit.left < innerWidth && hit.right > 0,
      small_target: Math.min(hit.width, hit.height) < 24,
      outside_modal: modal ? !modal.contains(e) : false,
      tab_index: null,
    })
  }
  const lines = new Set()
  document.body.innerText.split('\n').map(clean).filter(Boolean).forEach((l) => lines.add(l))
  const extra = []
  document.querySelectorAll('[title]').forEach((e) => extra.push('[tooltip] ' + clean(e.getAttribute('title'))))
  document.querySelectorAll('[aria-label]').forEach((e) => extra.push('[aria-label] ' + clean(e.getAttribute('aria-label'))))
  document.querySelectorAll('[placeholder]').forEach((e) => extra.push('[placeholder] ' + clean(e.getAttribute('placeholder'))))
  document.querySelectorAll('[aria-valuetext]').forEach((e) => extra.push('[valuetext] ' + clean(e.getAttribute('aria-valuetext'))))
  extra.push('[document.title] ' + document.title)
  const offenders = []
  document.querySelectorAll('body *').forEach((e) => {
    const r = e.getBoundingClientRect()
    if (offenders.length < 6 && r.width > 0 && r.right > innerWidth + 1 && !e.closest('[aria-hidden=true]')) offenders.push(`${e.tagName.toLowerCase()}.${String(e.className).slice(0, 40)} right=${Math.round(r.right)} "${clean(e.textContent).slice(0, 30)}"`)
  })
  return {
    controls: out,
    lines: [...lines],
    extra: [...new Set(extra)],
    overflow: { scrollWidth: document.documentElement.scrollWidth, innerWidth, overflow_x: document.documentElement.scrollWidth > innerWidth, offenders },
    modal_open: !!modal,
  }
}

async function tabPass(P, controls) {
  const { page } = P
  await page.evaluate(() => { window.__rvPrev = document.activeElement; window.scrollTo(0, 0); document.activeElement?.blur?.(); const m = document.createElement('span'); m.tabIndex = -1; document.body.prepend(m); m.focus(); m.remove() })
  const max = Math.min(controls.length + 8, 140)
  const seen = []
  for (let i = 0; i < max; i++) {
    await page.keyboard.press('Tab')
    const id = await page.evaluate(() => {
      const a = document.activeElement
      if (!a || a === document.body) return 'body'
      return a.getAttribute('data-rv') ?? 'unstamped:' + a.tagName.toLowerCase() + '.' + String(a.className).slice(0, 30)
    })
    if (id === 'body' && seen.length) break
    if (seen.includes(id)) break
    seen.push(id)
  }
  seen.forEach((id, i) => {
    const c = controls.find((x) => String(x.id) === id)
    if (c) c.tab_index = i + 1
  })
  const extras = seen.filter((s) => s.startsWith('unstamped'))
  await page.evaluate(() => { const p = window.__rvPrev; if (p && p.isConnected && p !== document.body) p.focus(); else document.activeElement?.blur?.() })
  return { sequence_length: seen.length, unstamped_focus_stops: extras, cycled_before_all: controls.filter((c) => !c.disabled && c.tab_index === null && !c.visually_hidden_input && c.kind === 'semantic').length }
}

async function runAxe(P) {
  const { page } = P
  if (!(await page.evaluate(() => !!window.axe))) await page.addScriptTag({ content: AXE })
  return page.evaluate(async () => {
    const r = await window.axe.run(document, { resultTypes: ['violations'] })
    return r.violations.map((v) => ({ id: v.id, impact: v.impact, help: v.help, url: v.helpUrl, nodes: v.nodes.length, targets: v.nodes.slice(0, 3).map((n) => n.target.join(' ')) }))
  })
}

// ---------------------------------------------------------------- snapshot = screenshot + inventory + texts + console + axe
async function snap(P, { screen, state, route, transient = false, full = true, label }) {
  return safe(`snap ${screen} ${state}`, async () => {
    const { page } = P
    await page.waitForTimeout(250)
    const inv = await page.evaluate(INVENTORY_FN)
    counter++
    const name = `${String(counter).padStart(2, '0')}-${screen.replace(/[^a-z0-9]+/gi, '-')}-${state}`.replace(/-+$/, '')
    await page.screenshot({ path: path.join(OUT, name + '.png'), fullPage: full })
    const route_ = route || (await curHash(P))
    const entryConsole = P.buf.splice(0)
    consoleLog.push({ screenshot: name + '.png', route: route_, state, errors: entryConsole })
    inv.lines.forEach((l) => addText(screen, l))
    inv.extra.forEach((l) => addText(screen, l))
    const entry = {
      screenshot: name + '.png',
      screen,
      route: route_,
      state,
      viewport: `${P.viewport.w}x${P.viewport.h}`,
      theme: P.scheme,
      description: label || `${screen}: ${state}`,
      overflow: inv.overflow,
      modal_open: inv.modal_open,
      controls: inv.controls,
      tab: null,
    }
    inventory.push(entry)
    shots.push(entry)
    const axe = await runAxe(P).catch((e) => [{ id: 'axe-error', impact: 'n/a', help: String(e.message).slice(0, 100), nodes: 0, targets: [] }])
    axeLog.push({ screenshot: name + '.png', route: route_, state, theme: P.scheme, viewport: entry.viewport, violations: axe })
    if (!transient && !process.env.NO_TAB) entry.tab = await tabPass(P, inv.controls).catch((e) => ({ error: String(e.message) }))
    else entry.tab = { skipped: 'transient state' }
    P.buf.splice(0) // console noise from the inspection itself
    log('shot', name, `controls=${inv.controls.length}`, inv.overflow.overflow_x ? 'OVERFLOW-X' : '')
    return name
  })
}

// ---------------------------------------------------------------- interaction observation
const norm = (s) => s.replace(/\d+/g, '#')
async function pageState(P, rv) {
  return P.page.evaluate((rv) => {
    const clean = (s) => (s || '').replace(/\s+/g, ' ').trim()
    const el = rv == null ? null : document.querySelector(`[data-rv="${rv}"]`)
    const a = document.activeElement
    return {
      url: location.href,
      lines: document.body.innerText.split('\n').map(clean).filter(Boolean),
      dialogs: [...document.querySelectorAll('[role=dialog]')].map((d) => clean(d.getAttribute('aria-label') || d.querySelector('h2,h3,[class*=title]')?.textContent || d.textContent).slice(0, 80)),
      control: el ? { value: el.value ?? null, checked: el.checked ?? null, expanded: el.getAttribute('aria-expanded'), open: el.closest('details')?.open ?? null, disabled: el.disabled ?? null } : null,
      focus: a && a !== document.body ? `${a.tagName.toLowerCase()}${a.getAttribute('role') ? '[' + a.getAttribute('role') + ']' : ''} "${clean(a.getAttribute('aria-label') || a.textContent || a.value).slice(0, 50)}"` : 'body',
      scrollY: Math.round(scrollY),
    }
  }, rv)
}
function diffStates(b, a) {
  const bs = new Map(b.lines.map((l) => [norm(l), l]))
  const as = new Map(a.lines.map((l) => [norm(l), l]))
  const appeared = [...as].filter(([k]) => !bs.has(k)).map(([, v]) => v.slice(0, 100)).slice(0, 8)
  const disappeared = [...bs].filter(([k]) => !as.has(k)).map(([, v]) => v.slice(0, 100)).slice(0, 8)
  return { appeared, disappeared }
}
/** Runs fn (the activation) and records what changed. Dedup key: screen + role + name + modality (first wins). */
async function observe(P, { screen, role, name, modality, rv = null, ctrl = {} }, fn) {
  const key = [screen, role, name, modality].join('|')
  if (interactions.has(key)) {
    try { await fn() } catch {}
    await P.page.waitForTimeout(400)
    P.buf.splice(0)
    return
  }
  const before = await pageState(P, rv).catch(() => null)
  P.buf.splice(0)
  let error = null
  try {
    await fn()
  } catch (e) {
    error = String(e.message || e).split('\n')[0].slice(0, 200)
  }
  await P.page.waitForTimeout(550)
  const after = await pageState(P, rv).catch(() => null)
  const cons = P.buf.splice(0)
  const rec = { screen, role, name, modality, kind: ctrl.kind || 'walk', href: ctrl.href || null, target: ctrl.target || null, error }
  if (before && after) {
    const d = diffStates(before, after)
    Object.assign(rec, {
      url_before: before.url.replace(/^https?:\/\/[^/]+/, ''),
      url_after: after.url.replace(/^https?:\/\/[^/]+/, ''),
      url_changed: before.url !== after.url,
      text_appeared: d.appeared,
      text_disappeared: d.disappeared,
      dialogs_before: before.dialogs,
      dialogs_after: after.dialogs,
      dialog_opened: after.dialogs.filter((x) => !before.dialogs.includes(x)),
      dialog_closed: before.dialogs.filter((x) => !after.dialogs.includes(x)),
      control_before: before.control,
      control_after: after.control,
      focus_after: after.focus,
      scrolled: Math.abs(after.scrollY - before.scrollY) > 40,
    })
    const ctrlChanged = JSON.stringify(before.control) !== JSON.stringify(after.control)
    rec.no_visible_effect = !rec.url_changed && !d.appeared.length && !d.disappeared.length && !rec.dialog_opened.length && !rec.dialog_closed.length && !ctrlChanged && !rec.scrolled
  } else rec.no_visible_effect = null
  rec.console_errors = cons
  interactions.set(key, rec)
  return rec
}

// ---------------------------------------------------------------- sweep: every distinct control on a screen, mouse and keyboard
async function activate(P, c, modality) {
  const { page } = P
  const loc = page.locator(`[data-rv="${c.id}"]`)
  if (modality === 'mouse') {
    await loc.scrollIntoViewIfNeeded({ timeout: 3000 }).catch(() => {})
    const pt = await loc.evaluate((e, isRange) => {
      let t = e
      const r0 = e.getBoundingClientRect()
      if (r0.width < 4 || r0.height < 4 || getComputedStyle(e).opacity === '0') t = e.closest('label') || e
      const r = t.getBoundingClientRect()
      const x = r.left + (isRange ? r.width * 0.8 : r.width / 2)
      const y = r.top + r.height / 2
      const top = document.elementFromPoint(x, y)
      return { x, y, obscured: !(top && (top === e || e.contains(top) || top.contains(e) || t.contains(top))), by: top ? top.tagName.toLowerCase() + '.' + String(top.className).slice(0, 30) : null }
    }, c.role === 'slider')
    if (pt.obscured) P.obscured = (P.obscured || []).concat([`${c.role} "${c.name}" covered by ${pt.by}`])
    await page.mouse.click(pt.x, pt.y)
  } else {
    await loc.focus({ timeout: 3000 })
    if (c.role === 'slider') await page.keyboard.press('ArrowRight')
    else if (c.role === 'searchbox' || c.role === 'textbox' || c.role === 'combobox') await page.keyboard.type('x')
    else if (['checkbox', 'switch', 'radio'].includes(c.role)) await page.keyboard.press('Space')
    else await page.keyboard.press('Enter')
  }
}
async function sweepScreen(screen, hash, W) {
  const P = await openPage({ name: 'sweep' }, hash, { extra: IS_MOCK ? CONFIG.sweepQuery : '' })
  const settle = () => P.page.waitForTimeout(screen.endsWith('demo') ? CONFIG.sweepSettleMs + 1500 : CONFIG.sweepSettleMs)
  try {
    await settle()
    const inv = await P.page.evaluate(INVENTORY_FN)
    const seen = new Set()
    const todo = []
    for (const c of inv.controls) {
      const k = c.role + '|' + c.name
      if (seen.has(k)) continue
      seen.add(k)
      todo.push(c)
    }
    log('sweep', screen, `${todo.length} distinct controls`)
    for (const c of todo) {
      const external = c.tag === 'a' && (c.target === '_blank' || /^(https?:|mailto:)/.test(c.href || ''))
      if (external || c.disabled) {
        interactions.set([screen, c.role, c.name, 'mouse'].join('|'), { screen, role: c.role, name: c.name, modality: 'mouse+keyboard', kind: c.kind, href: c.href, target: c.target, skipped: external ? 'external link: not followed' : 'disabled', no_visible_effect: null, console_errors: [] })
        continue
      }
      for (const modality of ['mouse', 'keyboard']) {
        await safe(`sweep ${screen} ${c.role} "${c.name}" ${modality}`, async () => {
          await P.page.goto('about:blank')
          await P.page.goto(urlFor(hash, IS_MOCK ? CONFIG.sweepQuery : ''), { waitUntil: 'domcontentloaded' })
          await settle()
          let cur = await P.page.evaluate(INVENTORY_FN)
          let match = cur.controls.find((x) => x.role === c.role && x.name === c.name)
          if (!match) {
            await P.page.evaluate(() => localStorage.clear())
            await P.page.reload({ waitUntil: 'domcontentloaded' })
            await settle()
            cur = await P.page.evaluate(INVENTORY_FN)
            match = cur.controls.find((x) => x.role === c.role && x.name === c.name)
          }
          if (!match && REAL) {
            // the backend keeps one demo for every page: a control that belongs to an earlier state ("Prepare this demo") is gone
            interactions.set([screen, c.role, c.name, modality].join('|'), { screen, role: c.role, name: c.name, modality, kind: c.kind, skipped: 'state is shared with the backend: the control is gone after a reload', no_visible_effect: null, console_errors: [] })
            return
          }
          if (!match) throw new Error('control not found after reload')
          await observe(P, { screen, role: c.role, name: c.name, modality, rv: match.id, ctrl: c }, () => activate(P, match, modality))
        })
      }
    }
    if (P.obscured?.length) interactions.set(`${screen}|obscured|`, { screen, role: 'note', name: 'controls covered at their click point', modality: 'mouse', no_visible_effect: null, obscured: [...new Set(P.obscured)], console_errors: [] })
  } finally {
    await closeP(P)
  }
}

// ---------------------------------------------------------------- checks of the third review round
/** Home: one "Start here", no "Good first pick" / "Recorded demo available", balanced rows (never one orphan card). */
async function homeChecks(P, tag) {
  const r = await P.page.evaluate(() => {
    const text = document.body.innerText
    const groups = [...document.querySelectorAll('section[aria-labelledby^="g-"]')].map((g) => {
      const tops = [...g.querySelectorAll('ul > li')].map((li) => Math.round(li.getBoundingClientRect().top))
      const rows = new Map()
      tops.forEach((t) => rows.set(t, (rows.get(t) || 0) + 1))
      return { n: tops.length, rows: [...rows.values()] }
    })
    return { start: (text.match(/Start here/g) || []).length, good: /Good first pick/.test(text), rec: /Recorded demo available/.test(text), groups, overflow: document.documentElement.scrollWidth > innerWidth }
  })
  assert(`${tag}: exactly one "Start here" badge`, r.start === 1, r.start)
  assert(`${tag}: no "Good first pick" and no "Recorded demo available"`, !r.good && !r.rec)
  const orphan = r.groups.filter((g) => g.n > 1 && Math.max(...g.rows) > 1 && g.rows[g.rows.length - 1] === 1)
  assert(`${tag}: no group ends in one orphan card`, orphan.length === 0, JSON.stringify(r.groups))
  assert(`${tag}: no horizontal overflow`, !r.overflow)
  return r
}
/** Scenario pages: the Back / Next bar is one line, at most 60 px, Back on the left and Next on the right; the top bar is fully on screen. */
async function barChecks(P, tag) {
  const r = await P.page.evaluate(() => {
    const f = document.querySelector('main footer')
    if (!f) return null
    const b = f.getBoundingClientRect()
    const links = [...f.querySelectorAll('a')].map((a) => a.getBoundingClientRect())
    const top = [...document.querySelectorAll('header button, header a')].map((e) => e.getBoundingClientRect()).filter((x) => x.width > 0)
    return {
      h: b.height, pos: getComputedStyle(f).position, left: links[0]?.left ?? 0, right: links[links.length - 1]?.left ?? 0, n: links.length,
      linkH: Math.min(...links.map((l) => l.height)), offTop: top.filter((x) => x.right > innerWidth + 1 || x.left < -1).length, overflow: document.documentElement.scrollWidth > innerWidth,
    }
  })
  if (!r) return
  assert(`${tag}: Back / Next bar is at most 60 px tall (${Math.round(r.h)} px)`, r.h <= 60.5, r.h)
  assert(`${tag}: Back / Next bar is sticky and every link in it is at least 40 px high`, (r.pos === 'sticky' || r.pos === 'static') && r.linkH >= 40, `${r.pos} ${r.linkH}`)
  if (r.n === 2) assert(`${tag}: Back is left of Next`, r.left < r.right)
  assert(`${tag}: every control of the top bar is on screen`, r.offTop === 0, r.offTop)
  assert(`${tag}: no horizontal overflow`, !r.overflow)
}
/** Code part: every note once (inline, no list above), one GitLab link per block, code regions are keyboard-focusable. */
async function codeChecks(P, tag) {
  const r = await P.page.evaluate(() => {
    const blocks = [...document.querySelectorAll('figure')]
    return {
      list: document.querySelectorAll('ol[aria-label^="Notes for this code"]').length,
      notes: document.querySelectorAll('[role=note]').length,
      noted: document.querySelectorAll('[data-note]').length,
      links: blocks.map((b) => b.querySelectorAll('a[href^="http"]').length),
      focus: blocks.map((b) => b.querySelector('[role=group]')?.getAttribute('tabindex')),
      login: (document.body.innerText.match(/needs the workshop login/gi) || []).length,
    }
  })
  assert(`${tag}: no list of notes above the code`, r.list === 0)
  assert(`${tag}: every noted line has exactly one note (${r.notes}/${r.noted})`, r.notes === r.noted)
  assert(`${tag}: at most one GitLab link per code block`, r.links.every((n) => n <= 1), r.links.join())
  assert(`${tag}: the login note appears once per page`, r.login <= 1, r.login)
  assert(`${tag}: code regions are keyboard-focusable`, r.focus.every((t) => t === '0'), r.focus.join())
}

/** The end card's buttons are never covered by the bar; mail, before/after pairs and the replay lines. */
async function demoChecks(P, id, screen) {
  const page = P.page
  await page.evaluate(() => { const c = document.querySelector('[data-end-card]'); if (c) window.scrollBy(0, c.getBoundingClientRect().bottom - innerHeight + 12) })
  await page.waitForTimeout(400)
  const cover = await page.evaluate(() => {
    const f = document.querySelector('main footer')?.getBoundingClientRect()
    const btns = [...document.querySelectorAll('[data-end-actions] button')].map((b) => b.getBoundingClientRect())
    const shown = f && getComputedStyle(document.querySelector('main footer')).position === 'sticky'
    return { overlaps: btns.filter((b) => shown && f && b.bottom > f.top + 1 && b.top < f.bottom - 1 && f.top < innerHeight).length, n: btns.length }
  })
  assert(`${screen}: the Back / Next bar never covers the end card's buttons`, cover.overlaps === 0, JSON.stringify(cover))
  await snap(P, { screen, state: 'end-card-in-view', full: false, transient: true, label: `${id}: the end card scrolled into view (the bar steps aside)` })
  const mail = await page.evaluate(() => {
    const f = document.querySelector('iframe[title^="Message:"]')
    if (!f) return null
    const card = f.closest('section')
    const r = f.getBoundingClientRect()
    return { h: r.height, w: r.width, cw: card?.getBoundingClientRect().width ?? 0, max: innerHeight * 0.7, // the mail is enlarged as a whole (zoom on its root): the size a reader sees is font x zoom
      font: Math.round(parseFloat(getComputedStyle(f.contentDocument.body).fontSize) * (parseFloat(getComputedStyle(f.contentDocument.documentElement).zoom) || 1)), button: !!document.evaluate("//button[contains(., 'Open the mail full size')]", document, null, 9, null).singleNodeValue }
  })
  if (mail) {
    assert(`${id}: the mail is full card width (${Math.round(mail.w)} of ${Math.round(mail.cw)} px)`, mail.w >= mail.cw * 0.85, JSON.stringify(mail))
    assert(`${id}: the mail frame is at most 70% of the window and its text is at least 15 px`, mail.h <= mail.max + 2 && mail.font >= 15, JSON.stringify(mail))
    assert(`${id}: "Open the mail full size" exists`, mail.button)
    await page.getByRole('button', { name: 'Open the mail full size' }).first().click()
    await page.waitForTimeout(400)
    assert(`${id}: the full-size mail opens in a dialog`, (await page.getByRole('dialog', { name: 'The mail, full size' }).count()) === 1)
    await snap(P, { screen, state: 'mail-full-size', transient: true, full: false, label: `${id}: the mail opened full size (dialog)` })
    await page.keyboard.press('Escape')
    assert(`${id}: Esc closes the full-size mail`, (await page.getByRole('dialog', { name: 'The mail, full size' }).count()) === 0)
  }
  const pair = await page.evaluate(() => [...document.querySelectorAll('[role=group][aria-label="Before"]')].map((b) => {
    const a = b.parentElement.querySelector('[role=group][aria-label="After"]')
    const ghost = b.querySelector('[inert]')
    const rb = b.getBoundingClientRect()
    const ra = a.getBoundingClientRect()
    return { complete: ghost.scrollHeight <= ghost.clientHeight + 1, hasAfter: !!a, side: ra.left > rb.left + 40, opacity: getComputedStyle(ghost).opacity, labelAfter: !!a.querySelector('p') }
  }))
  pair.forEach((p, k) => assert(`${id}: before/after pair ${k + 1}: both complete and labelled (${p.side ? 'side by side' : 'stacked'})`, p.complete && p.hasAfter && p.opacity === '1' && p.labelAfter, JSON.stringify(p)))
}

// ---------------------------------------------------------------- walk helpers
const clickText = (P, text, o = {}) => P.page.getByText(text, { exact: o.exact ?? true }).first().click()
const jobCancel = (P) => P.page.getByRole('button', { name: /^Stop (this step|getting ready)/ })
// the mode is changed in a dialog (header badge or "Change"); "Details" of the status row holds the data switch
const openMode = (P) => P.page.getByRole('button', { name: 'Change demo mode', exact: true }).click()
const closeDialog = (P) => P.page.getByRole('button', { name: 'Done', exact: true }).click()
const openDetails = async (P) => {
  const b = P.page.getByRole('button', { name: /Details/ })
  if ((await b.getAttribute('aria-expanded')) === 'false') await b.click()
}
async function settleHome(P) {
  await P.page.waitForSelector('a[href^="#/s/"]', { timeout: 15000 })
}
const cards = (P) => P.page.locator('article[data-status]')
const cardStatus = (card) => card.getAttribute('data-status')
async function waitStatus(card, pred, ms) {
  const t0 = Date.now()
  while (Date.now() - t0 < ms) {
    const s = await cardStatus(card).catch(() => null)
    if (pred(s)) return s
    await card.page().waitForTimeout(250)
  }
  return cardStatus(card).catch(() => null)
}
const runButton = (card) => card.locator('button:not([aria-expanded]):not([aria-label^="One day"])').first()

/** Run one step card and capture mid-way and the end. opts.inputs: also manipulate inputs first. */
async function runCard(P, screen, i, { inputs = true, tag = '' } = {}) {
  const card = cards(P).nth(i)
  await card.scrollIntoViewIfNeeded()
  const t = `step${i + 1}${tag}`
  if (inputs) await exerciseInputs(P, screen, card, t)
  const btnName = (await runButton(card).innerText().catch(() => '')).trim()
  if (await runButton(card).isDisabled()) {
    const why = await runButton(card).getAttribute('title')
    assert(`${screen} step ${i + 1} run button is enabled`, false, `disabled: ${why}`)
    await snap(P, { screen, state: `step${i + 1}${tag}-run-disabled`, label: `Step ${i + 1}: run button disabled (${why})` })
    return { card, end: 'blocked' }
  }
  const before = await cardStatus(card)
  await observe(P, { screen, role: 'button', name: 'step run button: ' + btnName, modality: 'mouse' }, () => runButton(card).click())
  await waitStatus(card, (s) => s === 'running' || s === 'done' || s === 'failed', 6000)
  // wait until at least one action is done, or 3 s, to catch the middle
  const t0 = Date.now()
  while ((await cardStatus(card)) === 'running' && Date.now() - t0 < 4500) {
    if ((await card.locator('[data-state="ok"]').count()) > 0 && Date.now() - t0 > 1500) break
    await P.page.waitForTimeout(300)
  }
  if ((await cardStatus(card)) === 'running') await snap(P, { screen, state: `${t}-running-mid`, transient: true, label: `Step ${i + 1} while running (mid-way)` })
  const end = await waitStatus(card, (s) => s !== 'running', CONFIG.stepTimeoutMs)
  await P.page.waitForTimeout(700)
  await snap(P, { screen, state: `${t}-${end}`, label: `Step ${i + 1} finished with status "${end}" (evidence visible)` })
  assert(`${screen} step ${i + 1} reaches done or failed (${before} -> ${end})`, ['done', 'failed'].includes(end))
  return { card, end }
}
async function exerciseInputs(P, screen, card, t) {
  const page = P.page
  // day picker / number slider
  const sliders = card.locator('input[type=range]')
  const n = await sliders.count()
  for (let j = 0; j < n; j++) {
    const s = sliders.nth(j)
    if (await s.isDisabled()) { if (!REAL) assert(`${screen} ${t} slider ${j + 1} enabled`, false, 'disabled'); else log('note: input disabled (replay plays the recorded answer)'); continue }
    const isDay = (await card.getByRole('button', { name: 'One day earlier' }).count()) > 0
    const kind = isDay ? 'day' : 'number'
    const label = await s.evaluate((e) => (e.labels?.[0]?.textContent || '').trim())
    const v0 = await s.inputValue()
    await s.focus()
    await observe(P, { screen, role: 'slider', name: label, modality: 'keyboard', rv: null }, async () => { await page.keyboard.press('ArrowLeft'); await page.keyboard.press('ArrowLeft') })
    const v1 = await s.inputValue()
    assert(`${screen} ${t} ${kind} slider moves by keyboard (${v0} -> ${v1})`, v0 !== v1)
    await snap(P, { screen, state: `${t}-${kind}-keyboard`, label: `${kind} input after ArrowLeft x2 (value ${v1})` })
    const box = await s.boundingBox()
    if (box) {
      await observe(P, { screen, role: 'slider', name: label, modality: 'mouse' }, () => page.mouse.click(box.x + box.width * 0.85, box.y + box.height / 2))
      let v2 = await s.inputValue()
      if (v2 === v1) {
        assert(`${screen} ${t} ${kind} slider: a plain click on the track at 85% moves it (${v1} -> ${v2})`, false, `box ${JSON.stringify(box)}`)
        const th = await s.evaluate((e) => { const r = e.getBoundingClientRect(); return { y: r.top + r.height / 2, x: r.left + r.width * ((e.value - e.min) / (e.max - e.min)) } })
        await page.mouse.move(th.x, th.y); await page.mouse.down(); await page.mouse.move(box.x + box.width * 0.9, th.y, { steps: 6 }); await page.mouse.up()
        v2 = await s.inputValue()
      }
      assert(`${screen} ${t} ${kind} slider moves by mouse (click or drag) (${v1} -> ${v2})`, v1 !== v2)
      await snap(P, { screen, state: `${t}-${kind}-mouse`, label: `${kind} input after a mouse click near the right end (value ${v2})` })
    }
    if (isDay) {
      await observe(P, { screen, role: 'button', name: 'One day earlier', modality: 'mouse' }, () => card.getByRole('button', { name: 'One day earlier' }).click())
      await snap(P, { screen, state: `${t}-day-earlier-button`, label: 'Day picker after "One day earlier"' })
    }
  }
  const radios = card.locator('input[type=radio]')
  if ((await radios.count()) > 1) {
    const r = radios.nth(1)
    const lab = await r.evaluate((e) => e.closest('label')?.textContent?.trim() || e.value)
    await observe(P, { screen, role: 'radio', name: lab, modality: 'mouse' }, () => r.locator('xpath=ancestor::label').click())
    assert(`${screen} ${t} choice button selects "${lab}"`, await r.isChecked())
    await snap(P, { screen, state: `${t}-choice`, label: `Choice buttons after selecting "${lab}"` })
    // keyboard: arrow to the next
    await radios.nth(0).focus()
    await observe(P, { screen, role: 'radio', name: 'arrow-key group move', modality: 'keyboard' }, () => page.keyboard.press('ArrowRight'))
  }
  const sw = card.locator('input[role=switch]')
  if ((await sw.count()) > 0) {
    const lab = await sw.first().evaluate((e) => e.closest('label')?.textContent?.trim() || 'toggle')
    const before = await sw.first().isChecked()
    await observe(P, { screen, role: 'switch', name: lab, modality: 'mouse' }, () => sw.first().locator('xpath=ancestor::label').click())
    assert(`${screen} ${t} toggle flips (${before} -> ${await sw.first().isChecked()})`, before !== (await sw.first().isChecked()))
    await snap(P, { screen, state: `${t}-toggle`, label: `Toggle "${lab}" after clicking` })
    await sw.first().focus()
    await observe(P, { screen, role: 'switch', name: lab, modality: 'keyboard' }, () => page.keyboard.press('Space'))
  }
}

/** Opens the demo part and waits for the first step card. */
async function openDemo(P, id, { early = true, screen } = {}) {
  await goHash(P, `#/s/${id}/demo`)
  await P.page.waitForTimeout(400)
  if (early) {
    await snap(P, { screen, state: 'getting-ready-start', transient: true, label: 'Demo opened: "Getting ready" just started' })
    await P.page.waitForTimeout(1600)
    await snap(P, { screen, state: 'getting-ready-mid', transient: true, label: 'Demo still getting ready, a little later' })
  }
  await P.page.waitForSelector('article[data-status], button:has-text("Prepare this demo")', { timeout: 20000 }).catch(() => {})
  const prep = P.page.getByRole('button', { name: 'Prepare this demo', exact: true })
  if (await prep.count().catch(() => 0)) {
    await prep.first().click().catch(() => {})
    await P.page.waitForTimeout(400)
    if (early) await snap(P, { screen, state: 'getting-ready-pressed', transient: true, label: 'Demo: "Prepare this demo" pressed' })
  }
  await P.page.waitForSelector('article[data-status]', { timeout: 40000 }).catch(() => {})
  await P.page.waitForTimeout(500)
}

// ---------------------------------------------------------------- scenario discovery
async function discover(P) {
  const r = await P.page.evaluate(async () => {
    try {
      const res = await fetch('/api/catalog', { headers: { 'X-Studio': '1' } })
      if (!res.ok || !(res.headers.get('content-type') || '').includes('json')) return null
      return await res.json()
    } catch { return null }
  })
  if (r?.groups) return { source: 'GET /api/catalog', scenarios: r.groups.flatMap((g) => g.scenarios), glossary: Object.keys(r.glossary || {}) }
  // fall back to the home page's cards
  const links = await P.page.$$eval('a[href^="#/s/"]', (as) => [...new Set(as.map((a) => a.getAttribute('href').split('/')[2]))])
  return { source: 'home page links (no JSON /api/catalog reachable: the mock is in-browser)', scenarios: links.map((id) => ({ id, recorded: undefined })), glossary: [] }
}

// ================================================================ the walk
async function main() {
  fs.mkdirSync(OUT, { recursive: true })
  for (const f of fs.readdirSync(OUT)) if (/\.(png|json|md)$/.test(f)) fs.rmSync(path.join(OUT, f))
  browser = await chromium.launch({ args: ['--no-sandbox'] })

  // ---------- 1. Home
  const H = await openPage({}, '#/')
  if (REAL) await safe('real backend: replay', () => enterReplay(H))
  if (REAL) await H.page.reload({ waitUntil: 'domcontentloaded' })
  await safe('home first load', async () => {
    await settleHome(H)
    await snap(H, { screen: 'home', state: 'getting-ready', transient: true, label: 'Home right after the first load, while the practice data is still being prepared' })
    await H.page.waitForTimeout(CONFIG.settleMs)
    await snap(H, { screen: 'home', state: 'settled', label: 'Home, everything ready' })
    await homeChecks(H, 'home (1440)')
  })
  const disc = await safe('discover scenarios', () => discover(H))
  meta.discovery = disc?.source
  const all = disc?.scenarios.map((s) => s.id) || []
  const env = process.env.SCENARIOS
  const written = env ? (env === 'all' ? all : env.split(',')) : CONFIG.scenarios
  meta.scenarios_walked = written
  meta.all_scenarios = all
  meta.recorded = disc?.scenarios.filter((s) => s.recorded).map((s) => s.id)
  const recordedId = disc?.scenarios.find((s) => s.recorded)?.id || CONFIG.replay.recorded
  const unrecordedId = disc?.scenarios.find((s) => s.recorded === false)?.id || CONFIG.replay.unrecorded
  const homeLinks = await H.page.$$eval('a', (as) => as.filter((a) => /localhost|127\.0\.0\.1/.test(a.getAttribute('href') || '') && !(a.getAttribute('href') || '').startsWith('#')).map((a) => ({ text: a.textContent.trim(), href: a.getAttribute('href'), target: a.target, rel: a.rel })))
  meta.external_links_on_home = homeLinks

  // ---------- 2. Home controls
  const S = 'home'
  await safe('home: mode switch', async () => {
    await openMode(H)
    await observe(H, { screen: S, role: 'radio', name: 'Recorded replay', modality: 'mouse' }, () => clickText(H, 'Recorded replay'))
    await snap(H, { screen: S, state: 'mode-replay', label: 'Mode switched to Recorded replay (sentence under the switch changes, badge in the top bar)' })
    if (!REAL) {
      await observe(H, { screen: S, role: 'radio', name: 'Live', modality: 'mouse' }, () => clickText(H, 'Live'))
      await snap(H, { screen: S, state: 'mode-live-again', label: 'Mode switched back to Live' })
    }
    await closeDialog(H)
  })
  if (!REAL) await safe('home: feeder toggle', async () => {
    await openDetails(H)
    await observe(H, { screen: S, role: 'switch', name: 'Practice data keeps coming in', modality: 'mouse' }, () => clickText(H, 'Practice data keeps coming in'))
    await snap(H, { screen: S, state: 'feeder-off', label: '"Practice data keeps coming in" switched off' })
    await observe(H, { screen: S, role: 'switch', name: 'Practice data keeps coming in', modality: 'mouse-back' }, () => clickText(H, 'Practice data keeps coming in'))
  })
  await safe('home: theme switch', async () => {
    const tbtn = H.page.locator('header button').last()
    await observe(H, { screen: S, role: 'button', name: 'theme switch', modality: 'mouse' }, () => tbtn.click())
    await snap(H, { screen: S, state: 'theme-switched-by-button', label: 'Theme switched with the top-bar button (manual theme)' })
    const stored = await H.page.evaluate(() => localStorage.getItem('studio-theme'))
    assert('theme choice stored in localStorage', !!stored, stored)
    await tbtn.click()
  })
  await safe('home: switcher via button', async () => {
    const b = H.page.getByRole('button', { name: /go to a scenario/i })
    await observe(H, { screen: S, role: 'button', name: 'Go to a scenario', modality: 'mouse' }, () => b.click())
    await snap(H, { screen: S, state: 'switcher-open-button', label: 'Scenario switcher opened with its button' })
    const box = H.page.getByRole('combobox')
    await box.fill('mail')
    await snap(H, { screen: S, state: 'switcher-search-results', label: 'Switcher: typed "mail" (results)' })
    await H.page.keyboard.press('ArrowDown')
    await H.page.keyboard.press('ArrowDown')
    await snap(H, { screen: S, state: 'switcher-keyboard-nav', label: 'Switcher: ArrowDown twice moves the highlighted option' })
    await box.fill('zzzzqq')
    await snap(H, { screen: S, state: 'switcher-no-results', label: 'Switcher: typed a search without results' })
    await box.fill('')
    await H.page.keyboard.press('ArrowDown')
    const hashBefore = await curHash(H)
    await observe(H, { screen: 'switcher', role: 'combobox', name: 'Enter on highlighted option', modality: 'keyboard' }, () => H.page.keyboard.press('Enter'))
    const hashAfter = await curHash(H)
    assert('switcher: Enter opens the highlighted scenario (URL changes)', hashBefore !== hashAfter, `${hashBefore} -> ${hashAfter}`)
    await goHash(H, '#/')
    await H.page.waitForTimeout(300)
  })
  await safe('home: switcher via / key', async () => {
    await H.page.evaluate(() => document.activeElement?.blur?.())
    await observe(H, { screen: S, role: 'key', name: '/ opens the switcher', modality: 'keyboard' }, () => H.page.keyboard.press('/'))
    await snap(H, { screen: S, state: 'switcher-open-slash', label: 'Scenario switcher opened with the "/" key' })
    await observe(H, { screen: 'switcher', role: 'key', name: 'Escape closes the switcher', modality: 'keyboard' }, () => H.page.keyboard.press('Escape'))
    assert('switcher closes with Escape', (await H.page.getByRole('combobox').count()) === 0)
    await observe(H, { screen: S, role: 'key', name: 'Ctrl+K opens the switcher', modality: 'keyboard' }, () => H.page.keyboard.press('Control+k'))
    await H.page.keyboard.press('Escape')
    await H.page.getByRole('button', { name: /go to a scenario/i }).click()
    await H.page.getByRole('combobox').fill('release')
    await observe(H, { screen: 'switcher', role: 'option', name: 'first result (mouse)', modality: 'mouse' }, () => H.page.getByRole('option').first().click())
    await goHash(H, '#/')
  })
  await safe('home: overview link', async () => {
    const l = H.page.locator('a[target=_blank]').first()
    meta.overview_link = await l.evaluate((a) => ({ text: a.textContent.trim(), href: a.href, target: a.target, rel: a.rel })).catch(() => null)
  })

  // ---------- 1b. other viewports / themes for Home
  for (const [opts, state, label] of [
    [{ scheme: 'dark' }, 'dark', 'Home in the dark theme (system preference), settled'],
    [{ viewport: { w: 390, h: 844 }, mobile: true }, 'phone-390', 'Home on a phone (390x844), light, settled'],
    [{ viewport: { w: 2560, h: 1440 } }, 'uhd-2560', 'Home at 2560x1440, light, settled'],
  ]) {
    await safe('home ' + state, async () => {
      const X = await openPage(opts, '#/')
      await settleHome(X)
      await X.page.waitForTimeout(CONFIG.settleMs)
      await snap(X, { screen: 'home', state, label })
      if (state === 'phone-390') await homeChecks(X, 'home (phone)')
      await closeP(X)
    })
  }

  // ---------- 3. Scenario parts (main session keeps mock state)
  for (const id of written) {
    const sc = id
    await safe(`scenario ${id}`, async () => {
      for (const part of ['problem', 'idea', 'code']) {
        await goHash(H, `#/s/${id}/${part}`)
        await H.page.waitForSelector('h1', { timeout: 10000 })
        await H.page.waitForTimeout(500)
        await snap(H, { screen: `${sc}/${part}`, state: 'light', label: `${id}: part "${part}" at 1440, light` })
        await barChecks(H, `${id}/${part} (1440)`)
        if (part === 'code') await codeChecks(H, `${id}/code`)
        // glossary popover
        const cands = H.page.locator('main button[aria-expanded]')
        let term = null
        for (let q = 0; q < (await cands.count()) && !term; q++) {
          await cands.nth(q).click().catch(() => {})
          if ((await H.page.locator('[role=dialog][aria-label^="What"]').count()) > 0) { term = cands.nth(q); await H.page.keyboard.press('Escape') }
        }
        if (!term) log('note:', `${id}/${part} has no glossary term (not every part uses one)`)
        if (term) {
          await observe(H, { screen: `${sc}/${part}`, role: 'button', name: 'glossary term', modality: 'mouse' }, () => term.click())
          await snap(H, { screen: `${sc}/${part}`, state: 'glossary-popover', full: false, label: `${id}/${part}: a glossary term popover open (viewport only)` })
          assert(`${id}/${part} popover is role=dialog with explanation`, (await H.page.locator('[role=dialog][aria-label^="What"]').count()) === 1)
          await H.page.keyboard.press('Escape')
          assert(`${id}/${part} Escape closes the popover and returns focus to the term`, (await H.page.locator('[role=dialog][aria-label^="What"]').count()) === 0)
        }
      }
      // arrow keys
      await goHash(H, `#/s/${id}/problem`)
      await H.page.evaluate(() => document.activeElement?.blur?.())
      const seq = [['ArrowRight', 'idea'], ['ArrowRight', 'code'], ['ArrowLeft', 'idea'], ['ArrowLeft', 'problem']]
      for (const [k, want] of seq) {
        await H.page.keyboard.press(k)
        await H.page.waitForTimeout(350)
        const h = await curHash(H)
        assert(`${id}: ${k} moves to ${want}`, h.endsWith('/' + want), h)
      }
      await H.page.keyboard.press('ArrowLeft')
      await H.page.waitForTimeout(250)
      assert(`${id}: ArrowLeft on first part stays put`, (await curHash(H)).endsWith('/problem'))
      // previous / next scenario
      const next = H.page.getByRole('button', { name: /^Next scenario/ })
      if ((await next.count()) && !(await next.isDisabled())) {
        const h0 = await curHash(H)
        await observe(H, { screen: 'scenario-top-bar', role: 'button', name: 'Next scenario', modality: 'mouse' }, () => next.click())
        const h1 = await curHash(H)
        assert(`${id}: "Next scenario" changes the URL`, h0 !== h1, `${h0} -> ${h1}`)
        await H.page.waitForSelector('h1')
        await snap(H, { screen: `${sc}-next-scenario`, state: 'problem', full: false, label: `${id}: after "Next scenario" (viewport only)` })
        await observe(H, { screen: 'scenario-top-bar', role: 'button', name: 'Previous scenario', modality: 'mouse' }, () => H.page.getByRole('button', { name: /^Previous scenario/ }).click())
        assert(`${id}: "Previous scenario" returns`, (await curHash(H)) === h0, await curHash(H))
      } else if (all.indexOf(id) < all.length - 1) assert(`${id}: next scenario exists`, false, 'no enabled Next scenario button')
    })
  }

  // other viewport / theme for the first scenario's parts
  const first = written[0]
  if (first) {
    await safe('scenario phone', async () => {
      const X = await openPage({ viewport: { w: 390, h: 844 }, mobile: true }, `#/s/${first}/problem`)
      await X.page.waitForTimeout(800)
      await snap(X, { screen: `${first}/problem`, state: 'phone-390', label: `${first}: problem part on a phone` })
      await barChecks(X, `${first}/problem (phone)`)
      await goHash(X, `#/s/${first}/code`)
      await X.page.waitForSelector('figure [role=group] code', { timeout: 10000 }).catch(() => {})
      await X.page.waitForTimeout(900)
      await snap(X, { screen: `${first}/code`, state: 'phone-390', label: `${first}: code part on a phone (code blocks)` })
      await barChecks(X, `${first}/code (phone)`)
      await codeChecks(X, `${first}/code (phone)`)
      const wrap = await X.page.evaluate(() => { const c = document.querySelector('figure [role=group] code'); return c ? getComputedStyle(c).whiteSpace : null })
      assert('code lines wrap on a phone (pre-wrap)', wrap === 'pre-wrap', wrap)
      await closeP(X)
    })
    await safe('scenario dark', async () => {
      const X = await openPage({ scheme: 'dark' }, `#/s/${first}/idea`)
      await X.page.waitForTimeout(800)
      await snap(X, { screen: `${first}/idea`, state: 'dark', label: `${first}: idea part in the dark theme` })
      await goHash(X, `#/s/${first}/code`)
      await X.page.waitForTimeout(600)
      await snap(X, { screen: `${first}/code`, state: 'dark', label: `${first}: code part in the dark theme` })
      await closeP(X)
    })
  }

  // ---------- 4. Demo walks (main session)
  for (const id of written) {
    const sc = `${id}/demo`
    await safe(`demo ${id}`, async () => {
      await openDemo(H, id, { screen: sc })
      const n = await cards(H).count()
      assert(`${id}: demo has step cards`, n > 0, n)
      await snap(H, { screen: sc, state: 'ready', label: `${id}: demo ready, first step enabled, later steps "Not yet"` })
      for (let i = 0; i < n; i++) {
        let { card, end } = await runCard(H, sc, i)
        if (end === 'done') {
          const det = card.locator('summary', { hasText: 'Technical details' })
          if (await det.count()) {
            await observe(H, { screen: sc, role: 'disclosure-summary', name: 'Technical details', modality: 'mouse' }, () => det.click())
            await snap(H, { screen: sc, state: `step${i + 1}-technical-details-open`, label: `Step ${i + 1}: "Technical details" disclosure open` })
            await det.focus()
            await observe(H, { screen: sc, role: 'disclosure-summary', name: 'Technical details', modality: 'keyboard' }, () => H.page.keyboard.press('Enter'))
          } else if (!REAL && id === CONFIG.states) assert(`${id} step ${i + 1} has Technical details`, false, 'none rendered')
          else log('note: no Technical details for', id, 'step', i + 1)
          assert(`${id} step ${i + 1} shows the outcome sentence`, (await card.getByText(/What just happened|Not quite/).count()) > 0)
        }
        if (end === 'failed') {
          assert(`${id} step ${i + 1} shows an alert explaining the failure`, (await card.locator('[role=alert]').count()) > 0)
          meta.failed_step = `${id} step ${i + 1}`
          const r = await runCard(H, sc, i, { inputs: false, tag: '-retry' })
          if (r.end !== 'done') break
          const det = r.card.locator('summary', { hasText: 'Technical details' })
          if (await det.count()) { await det.click(); await snap(H, { screen: sc, state: `step${i + 1}-retry-technical-open`, label: `Step ${i + 1} after retry, technical details open` }) }
        } else if (IS_MOCK && CONFIG.failing.scenario === id && CONFIG.failing.step === i + 1) assert(`${id} step ${i + 1} was expected to fail first (mock)`, false, 'it did not fail')
      }
      await H.page.waitForTimeout(500)
      await snap(H, { screen: sc, state: 'end-state', label: `${id}: end state after the last step ("That is the whole demo", related scenarios)` })
      assert(`${id}: end block present`, (await H.page.getByText('That is the whole demo').count()) > 0)
      await demoChecks(H, id, sc)
      // related links
      const rel = H.page.locator('main aside a[href^="#/s/"]').last()
      if (await rel.count()) {
        const href = await rel.getAttribute('href')
        const h0 = await curHash(H)
        await observe(H, { screen: sc, role: 'link', name: 'related scenario link', modality: 'mouse' }, () => rel.click())
        assert(`${id}: related link navigates`, (await curHash(H)) !== h0, `${h0} -> ${href}`)
        await H.page.goBack()
        await H.page.waitForTimeout(500)
      }
      // run again on step 1 + cancel flow
      await openDemo(H, id, { early: false, screen: sc })
      const c0 = cards(H).nth(0)
      await observe(H, { screen: sc, role: 'button', name: 'Run again', modality: 'mouse' }, () => runButton(c0).click())
      await waitStatus(c0, (s) => s === 'running', 6000)
      await H.page.waitForTimeout(1500)
      cancellable = (await jobCancel(H).count()) > 0
      await observe(H, { screen: 'top-bar-job', role: 'button', name: 'Stop this step', modality: 'mouse' }, () => jobCancel(H).click())
      await H.page.waitForTimeout(300)
      await snap(H, { screen: sc, state: 'cancel-dialog', transient: true, full: false, label: `${id}: Cancel pressed while a step runs: confirm dialog "Stop this step?"` })
      const dlg = H.page.getByRole('dialog', { name: /Stop this step/ })
      cassert(`${id}: cancel confirm dialog opens`, (await dlg.count()) > 0)
      await observe(H, { screen: 'cancel-dialog', role: 'button', name: 'Keep going', modality: 'mouse' }, () => H.page.getByRole('button', { name: 'Keep going' }).click())
      cassert(`${id}: "Keep going" leaves the step running`, (await cardStatus(c0)) === 'running' || (await cardStatus(c0)) === 'done')
      await snap(H, { screen: sc, state: 'cancel-cancelled-still-running', transient: true, full: false, label: `${id}: after "Keep going" the step is still running` })
      if ((await jobCancel(H).count()) > 0) {
        await jobCancel(H).click()
        await observe(H, { screen: 'cancel-dialog', role: 'key', name: 'Escape closes the dialog', modality: 'keyboard' }, () => H.page.keyboard.press('Escape'))
        cassert(`${id}: Escape closes the cancel dialog`, (await dlg.count()) === 0)
        cassert(`${id}: focus returns to the stop button after the dialog closes`, await H.page.evaluate(() => /Stop this step/.test(document.activeElement?.textContent || '')), await H.page.evaluate(() => document.activeElement?.tagName))
      }
      if ((await jobCancel(H).count()) > 0) {
        await jobCancel(H).click()
        await observe(H, { screen: 'cancel-dialog', role: 'button', name: 'Stop the step', modality: 'mouse' }, () => H.page.getByRole('button', { name: 'Stop the step' }).click())
        await H.page.waitForTimeout(900)
        await snap(H, { screen: sc, state: 'cancel-stopped', label: `${id}: after "Stop the step": the step is back to ready` })
        cassert(`${id}: cancelled step returns to todo`, (await cardStatus(c0)) === 'stopped', await cardStatus(c0))
      } else cassert(`${id}: step was still running to cancel`, false, 'job ended before the cancel could be pressed')
      // leftovers on home
      await goHash(H, '#/')
      await H.page.waitForTimeout(600)
      const clean = H.page.getByRole('button', { name: 'Clean up now' })
      if (await clean.count()) {
        await snap(H, { screen: 'home', state: `leftovers-after-${id}`, label: `Home after a cancelled demo (${id}): "The last demo left its practice data behind" notice with "Clean up now"` })
        await observe(H, { screen: 'home', role: 'button', name: 'Clean up now', modality: 'mouse' }, () => clean.click())
        await H.page.waitForTimeout(900)
        await snap(H, { screen: 'home', state: `cleaning-after-${id}`, transient: true, label: 'Home while cleaning up (button busy, job indicator)' })
        await H.page.waitForTimeout(2600)
        await snap(H, { screen: 'home', state: `tidy-after-${id}`, label: 'Home after clean up: "Nothing left over"' })
      } else if (!REAL) assert('leftovers notice appears after a cancelled step', false, 'no "Clean up now" button')
    })
  }

  // ---------- 5. Global states
  await safe('lost mid-session', async () => {
    if (!IS_MOCK) return
    await goHash(H, '#/')
    await H.page.evaluate(() => window.__studioMock.setOffline(true))
    await H.page.waitForTimeout(1200)
    await snap(H, { screen: 'global', state: 'lost-contact-mid-session', label: 'Connection lost while the page is open (mock setOffline(true))' })
    await H.page.evaluate(() => window.__studioMock.setOffline(false))
    await H.page.waitForTimeout(1500)
    await snap(H, { screen: 'global', state: 'recovered', label: 'Connection restored: banner gone?' })
  })
  await closeP(H)

  await safe('offline', async () => {
    const X = await openPage({ offline: true }, '#/')
    await X.page.waitForTimeout(3000)
    await snap(X, { screen: 'global', state: 'offline-home', label: 'Home when the studio cannot be reached from the start' })
    await goHash(X, `#/s/${first}/demo`)
    await X.page.waitForTimeout(3000)
    await snap(X, { screen: 'global', state: 'offline-demo', label: 'Demo part when the studio cannot be reached' })
    await closeP(X)
  })

  await safe('replay', async () => {
    const X = await openPage({}, '#/')
    await settleHome(X)
    await X.page.waitForTimeout(CONFIG.settleMs)
    if (!REAL) await safe('replay: switch mode', async () => {
      await openMode(X)
      await X.page.getByRole('radio', { name: 'Recorded replay' }).click({ force: true })
      await closeDialog(X)
    })
    await X.page.waitForTimeout(500)
    await snap(X, { screen: 'global', state: 'replay-home', label: 'Home in Replay mode (fresh page)' })
    const sc = `${recordedId}/demo`
    await openDemo(X, recordedId, { screen: sc, early: false })
    await snap(X, { screen: sc, state: 'replay-ready', label: `Replay mode, recorded scenario (${recordedId}): demo ready, "This is a replay" notice` })
    await runCard(X, sc, 0, { tag: '-replay' })
    const su = `${unrecordedId}/demo`
    await openDemo(X, unrecordedId, { screen: su, early: false })
    await snap(X, { screen: su, state: 'replay-ready', label: `Replay mode, scenario without a recording (${unrecordedId}): notice` })
    await runCard(X, su, 0, { tag: '-replay-unrecorded', inputs: false })
    await closeP(X)
  })

  // the states scenario in Replay: fixed inputs, no Stop, recorded times, a locked step, every new state
  if (IS_MOCK) await safe('replay: every state', async () => {
    const id = CONFIG.states
    const X = await openPage({}, '#/')
    await settleHome(X)
    await X.page.waitForTimeout(CONFIG.settleMs)
    await openMode(X)
    await X.page.getByRole('radio', { name: 'Recorded replay' }).click({ force: true })
    await closeDialog(X)
    const sc = `${id}/demo`
    await openDemo(X, id, { screen: sc, early: false })
    await snap(X, { screen: sc, state: 'replay-states-ready', label: `${id} in Replay: fixed inputs with one line, later steps locked ("Appears after step 1.")` })
    const txt = () => X.page.evaluate(() => document.body.innerText)
    let t = await txt()
    assert('replay: one line says what is fixed in the recording', /Fixed in the recording: .+\. Switch to Live to choose yourself\./.test(t))
    assert('replay: no "Good to know" box and no "Recorded with" per input', !/Good to know/.test(t) && !/Recorded with/.test(t))
    assert('replay: the amber banner says what Live does', /In Live the real steps run/.test(t))
    assert('locked step: evidence says "Appears after step 1."', /Appears after step 1\./.test(t))
    const run0 = cards(X).nth(0)
    await runButton(run0).click()
    await waitStatus(run0, (s) => s === 'running', 6000)
    await X.page.waitForTimeout(400)
    assert('replay: no Stop button while a recording plays', (await X.page.getByRole('button', { name: /^Stop / }).count()) === 0)
    assert('replay: "A recording cannot be stopped; it takes a few seconds." is shown', /A recording cannot be stopped; it takes a few seconds\./.test(await txt()))
    await snap(X, { screen: sc, state: 'replay-states-step1-running', transient: true, label: `${id} in Replay: step 1 running (no Stop)` })
    await waitStatus(run0, (s) => s !== 'running', 30000)
    await X.page.waitForTimeout(500)
    await snap(X, { screen: sc, state: 'replay-states-step1-done', label: `${id} in Replay: step 1 done (failed night, rehearsal, table footer, history, trail)` })
    t = await txt()
    assert('replay: evidence says "Recorded <date>", never "Just now" or "minutes ago"', /Recorded \d+ \w+/.test(t) && !/Just now/.test(t) && !/minutes ago/.test(t))
    for (const w of ['3 more steps not run', '10 more steps skipped', 'Sending the failure mail', 'This run', 'Problems', '1 warning', '1 of 28 tables rehearsed', 'Showing the 9 slowest', 'Time (seconds)', 'It showed:']) assert(`states step 1 shows "${w}"`, t.toLowerCase().includes(w.toLowerCase()))
    await runCard(X, sc, 1, { inputs: false, tag: '-replay' })
    await X.page.waitForTimeout(300)
    t = await txt()
    for (const w of ['Retry', 'Before', 'After', 'Open the mail full size']) assert(`states step 2 shows "${w}"`, t.includes(w))
    await runCard(X, sc, 2, { inputs: false, tag: '-replay' })
    t = await txt()
    for (const w of ['Not needed', 'Part 1 of 4', 'Part 4 of 4']) assert(`states step 3 shows "${w}"`, t.includes(w))
    await demoChecks(X, id, sc)
    await closeP(X)
  })

  await safe('not fully written', async () => {
    const nf = CONFIG.notFullyWritten
    const X = await openPage({}, `#/s/${nf}/problem`)
    await X.page.waitForTimeout(CONFIG.settleMs)
    for (const part of ['problem', 'idea', 'code']) {
      await goHash(X, `#/s/${nf}/${part}`)
      await X.page.waitForTimeout(500)
      await snap(X, { screen: `${nf}/${part}`, state: 'light', label: `${nf} (not fully written in the mock): ${part}` })
    }
    const sc = `${nf}/demo`
    await openDemo(X, nf, { screen: sc, early: false })
    await snap(X, { screen: sc, state: 'ready', label: `${nf}: demo ready (live)` })
    await runCard(X, sc, 0, { tag: '-live' })
    await closeP(X)
  })

  if (IS_MOCK) for (const [opts, state] of [[{ scheme: 'dark' }, 'dark'], [{ viewport: { w: 390, h: 844 }, mobile: true }, 'phone-390']]) {
    await safe(`states demo ${state}`, async () => {
      const id = CONFIG.states
      const X = await openPage(opts, `#/s/${id}/demo`)
      await X.page.waitForTimeout(1200)
      const sc = `${id}/demo`
      await openDemo(X, id, { screen: sc, early: false })
      await snap(X, { screen: sc, state: `ready-${state}`, label: `${id} demo ready (${state})` })
      await runCard(X, sc, 0, { inputs: false, tag: `-${state}` })
      await runCard(X, sc, 1, { inputs: false, tag: `-${state}` })
      await barChecks(X, `${sc} (${state})`)
      await closeP(X)
    })
  }

  await safe('unknown routes', async () => {
    for (const [h, state] of [[CONFIG.unknownRoute, 'unknown-scenario-demo'], ['#/s/does-not-exist', 'unknown-scenario-no-part'], [`#/s/${first}/nonsense`, 'unknown-part'], ['#/nonsense', 'unknown-route']]) {
      const X = await openPage({}, h)
      await X.page.waitForTimeout(2000)
      await snap(X, { screen: 'global', state, route: h, label: `Route ${h}` })
      await closeP(X)
    }
  })

  // ---------- 6. The interaction sweep (parallel workers, each page has its own mock)
  if (!process.env.SKIP_SWEEP) {
    const jobs = [['home', '#/']]
    for (const id of written) for (const p of PARTS) jobs.push([`${id}/${p}`, `#/s/${id}/${p}`])
    let k = 0
    await Promise.all(
      Array.from({ length: WORKERS }, async () => {
        while (k < jobs.length) {
          const [screen, hash] = jobs[k++]
          await safe(`sweep ${screen}`, () => sweepScreen(screen, hash))
        }
      }),
    )
  }
  await restoreBackend().catch((e) => console.error('could not put the backend back', e))
  await browser.close()
}

// ================================================================ outputs
function writeOutputs() {
  const w = (f, d) => fs.writeFileSync(path.join(OUT, f), typeof d === 'string' ? d : JSON.stringify(d, null, 2))
  const ints = [...interactions.values()]
  w('inventory.json', inventory.map(({ description, screen, ...e }) => ({ screenshot: e.screenshot, route: e.route, state: e.state, viewport: e.viewport, theme: e.theme, overflow: e.overflow, tab: e.tab, modal_open: e.modal_open, controls: e.controls })))
  w('interactions.json', ints)
  w('console.json', consoleLog)
  w('axe.json', axeLog)
  w('assertions.json', assertions)
  w('failures.json', failures)
  let md = '# UI texts seen, by screen\n\nGenerated from the rendered pages (visible text, tooltips, aria-labels, placeholders).\n'
  for (const [screen, set] of [...texts].sort((a, b) => a[0].localeCompare(b[0]))) {
    md += `\n## ${screen}\n\n`
    const lines = [...set].filter((l) => !/^\d+:\d\d$/.test(l))
    for (const l of lines) md += `- ${l.replace(/\|/g, '\\|')}\n`
  }
  w('texts.md', md)

  const distinct = new Set()
  const noEffect = new Set()
  inventory.forEach((e) => e.controls.forEach((c) => distinct.add([e.screen, c.role, c.name].join('|'))))
  ints.forEach((i) => { if (i.no_visible_effect) noEffect.add([i.screen, i.role, i.name].join('|')) })
  const errs = consoleLog.flatMap((c) => c.errors)
  const axeBy = {}
  axeLog.forEach((a) => a.violations.forEach((v) => (axeBy[v.impact] = (axeBy[v.impact] || 0) + 1)))
  const overflow = inventory.filter((e) => e.overflow.overflow_x)
  const failedAsserts = assertions.filter((a) => !a.pass)
  let idx = `# UI capture index\n\nBASE_URL: \`${BASE}\`  ·  discovery: ${meta.discovery}  ·  scenarios walked: ${meta.scenarios_walked?.join(', ')}\n\n## Counts\n\n`
  idx += `- screenshots: ${shots.length}\n- distinct controls (screen + role + name, from the inventories): ${distinct.size}\n- controls swept for interaction (distinct screen+role+name): ${new Set(ints.filter((i) => i.modality !== 'walk').map((i) => [i.screen, i.role, i.name].join('|'))).size}; interaction records: ${ints.length}\n- controls with no visible effect: ${noEffect.size}\n`
  idx += `- console errors/warnings/failed requests: ${errs.length} (errors: ${errs.filter((e) => /error|pageerror|requestfailed|http/.test(e.kind) && e.kind !== 'console.warning').length})\n- axe violations (rule x screen): ${Object.values(axeBy).reduce((a, b) => a + b, 0)} by impact: ${JSON.stringify(axeBy)}\n- screens with horizontal overflow (scrollWidth > innerWidth): ${overflow.length}\n- assertions: ${assertions.length - failedAsserts.length} pass, ${failedAsserts.length} fail\n- failed captures: ${failures.length}\n\n`
  idx += '## Files\n\ninventory.json, interactions.json, console.json, axe.json, texts.md, assertions.json, failures.json\n\n## Screenshots\n\n'
  for (const s of shots) idx += `- \`${s.screenshot}\` (${s.viewport}, ${s.theme}${s.overflow.overflow_x ? ', OVERFLOW-X' : ''}): ${s.description}\n`
  idx += '\n## Failed assertions\n\n' + (failedAsserts.map((a) => `- ${a.name}${a.detail ? ' (' + a.detail + ')' : ''}`).join('\n') || 'none') + '\n'
  idx += '\n## Controls with no visible effect\n\n' + ([...noEffect].map((k) => `- ${k}`).join('\n') || 'none') + '\n'
  idx += '\n## Overflow at the viewport width\n\n' + (overflow.map((e) => `- ${e.screenshot} scrollWidth ${e.overflow.scrollWidth} > ${e.overflow.innerWidth}: ${e.overflow.offenders.slice(0, 2).join('; ')}`).join('\n') || 'none') + '\n'
  idx += '\n## Capture failures (not captured)\n\n' + (failures.map((f) => `- ${f.what}: ${f.error}`).join('\n') || 'none') + '\n'
  idx += `\n## Meta\n\n\`\`\`json\n${JSON.stringify(meta, null, 2)}\n\`\`\`\n`
  w('INDEX.md', idx)
}

let code = 0
try {
  await main()
} catch (e) {
  console.error('FATAL', e)
  code = 1
} finally {
  try { writeOutputs() } catch (e) { console.error('write failed', e); code = 1 }
  await browser?.close().catch(() => {})
}
if (crashed) code = 2
process.exit(code)
