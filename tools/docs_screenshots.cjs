// Screenshots for the documentation (docs/assets/screens/), taken with
// playwright-core from a running laptimer, best one playing the demo recording:
//
//   tools/make_demo_recording.py /tmp/demo.csv
//   laptimer serve --replay /tmp/demo.csv …      (board 2 as "Freigabe", 4 pilots in the slots,
//                                                 one training and two races, the last one
//                                                 not confirmed yet)
//   NODE_PATH=<node_modules with playwright-core> node tools/docs_screenshots.cjs
//
// Environment: LAPTIMER_URL (default http://127.0.0.1:5800), LAPTIMER_PASSWORD,
// CHROME (a Chromium binary, if playwright's own isn't installed), OUT (folder),
// ONLY (the parts below to run, e.g. "3,4"; default all).
// Part 2 starts one more race (about 2 minutes) for the live pictures and
// confirms it. GPL-3.0-or-later.

const { chromium } = require('playwright-core')
const path = require('path')

const URL = process.env.LAPTIMER_URL || 'http://127.0.0.1:5800'
const PASSWORD = process.env.LAPTIMER_PASSWORD || ''
const OUT = process.env.OUT || path.join(__dirname, '..', 'docs', 'assets', 'screens')
const ONLY = (process.env.ONLY || '1,2,3,4').split(',')
const DESKTOP = { viewport: { width: 1280, height: 860 }, deviceScaleFactor: 1 }
const PHONE = { viewport: { width: 390, height: 844 }, deviceScaleFactor: 2, isMobile: true, hasTouch: true }

const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

async function main() {
  const browser = await chromium.launch({ executablePath: process.env.CHROME || undefined })
  const desk = await browser.newContext(DESKTOP)
  const phone = await browser.newContext(PHONE)
  for (const ctx of [desk, phone]) {
    const r = await ctx.request.post(`${URL}/api/login`, { data: { password: PASSWORD } })
    if (!r.ok()) throw new Error(`login: ${r.status()} ${await r.text()}`)
  }
  const api = (method, p, data) => desk.request.fetch(`${URL}/api${p}`, { method, data }).then((r) => r.json())
  // The race waiting for "confirm" (`unconfirmed` is its id).
  const confirm = async () => {
    const { unconfirmed } = await api('GET', '/state')
    if (unconfirmed) await api('POST', `/sessions/${unconfirmed}/confirm`)
  }

  const d = await desk.newPage()
  const m = await phone.newPage()
  const shot = async (page, name, opts = {}) => {
    await sleep(opts.wait ?? 800)
    await page.screenshot({ path: path.join(OUT, `${name}.png`) })
    console.log(name)
  }
  // The visible one: on the phone the side bar's buttons are hidden.
  const nav = (page, label) => page.locator('.nav-btn:visible', { hasText: label }).first().click()
  const top = (page) => page.evaluate(() => window.scrollTo(0, 0))

  // 1. The race that ended, still waiting for "confirm".
  if (ONLY.includes('1')) {
    await d.goto(URL)
    await m.goto(URL)
    await d.getByText('Ergebnis bestätigen').waitFor({ timeout: 180_000 })
    await shot(d, 'control-result')
    await shot(m, 'phone-result')
    await confirm()
  }

  // 2. A race running: control, thresholds, spectators, phone.
  if (ONLY.includes('2')) {
    const { id } = await api('POST', '/session/start', { race: true })
    console.log(`race ${id} started`)
    await d.goto(URL)
    await m.goto(URL)
    await sleep(48_000) // two laps or so
    await shot(d, 'control-live', { wait: 0 })
    await shot(m, 'phone-control', { wait: 0 })
    await d.locator('.lane.selectable').nth(1).click()
    await d.locator('.lane.selectable').nth(1).scrollIntoViewIfNeeded()
    await d.mouse.wheel(0, 400)
    await shot(d, 'control-tune', { wait: 1500 })
    await nav(d, 'Zuschauer')
    await top(d)
    await shot(d, 'watch')
    await nav(m, 'Zuschauer')
    await shot(m, 'phone-watch')
    for (let i = 0; i < 60 && (await api('GET', '/state')).active; i++) await sleep(3000)
    await confirm()
  }

  // 3. Correcting the oldest race (the list is newest first): the first
  // pilot's whole session, then another one with the arming gate.
  if (ONLY.includes('3')) {
    await d.goto(URL)
    await nav(d, 'Sessions')
    await d.locator('.session-item').filter({ hasNotText: 'Training' }).last().click()
    await d.getByText('Runden bearbeiten').click()
    await d.locator('button', { hasText: 'Ghost' }).filter({ hasText: 'R1' }).last().click()
    await d.getByRole('button', { name: 'Alles', exact: true }).click()
    await d.getByText('Graph kürzen').scrollIntoViewIfNeeded()
    await shot(d, 'editor', { wait: 2500 })
    await d.locator('button', { hasText: 'Bluebird' }).filter({ hasText: 'R6' }).last().click()
    await d.getByText('Freigabe-Gate', { exact: true }).click()
    await d.getByText('Graph kürzen').scrollIntoViewIfNeeded()
    await shot(d, 'editor-arming', { wait: 2500 })
  }

  // 4. Boards, debug, API, channels.
  if (ONLY.includes('4')) {
    await d.goto(URL)
    await nav(d, 'Boards')
    await shot(d, 'boards', { wait: 1500 })
    await d.getByRole('tab', { name: 'Debug' }).click()
    await shot(d, 'debug', { wait: 6000 })
    await nav(d, 'API & Hooks')
    await d.getByRole('tab', { name: 'Testen' }).click()
    await shot(d, 'api', { wait: 1500 })
    await nav(d, 'Einstellungen')
    await d.getByText('Kanäle wählen').click()
    await shot(d, 'channels', { wait: 1000 })
  }

  await browser.close()
}

main().catch((e) => {
  console.error(e)
  process.exit(1)
})
