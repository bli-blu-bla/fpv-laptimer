// @ts-check
// Tests against a running server (no boards needed):
//   LAPTIMER_URL=http://localhost:5800 LAPTIMER_KEY=lt_… npm test
// They start and stop sessions and add pilots: use a test server with its own
// data folder, not the one at the race site.

import { test, before, beforeEach, after } from 'node:test'
import assert from 'node:assert/strict'
import { Laptimer, LaptimerError } from '../src/index.js'

const url = process.env.LAPTIMER_URL
const key = process.env.LAPTIMER_KEY
const skip = !url || !key ? 'LAPTIMER_URL und LAPTIMER_KEY setzen' : false

/** @type {Laptimer} */
let lt
/** Unique per run, so callsigns don't clash with earlier runs. */
const tag = Date.now().toString(36).toUpperCase()

/** Nothing running, no race waiting for confirmation (also after a failed test). */
async function clean() {
  const state = await lt.state()
  if (state.session) await lt.session.cancel()
  if (state.unconfirmed) await lt.session.discard()
}

before(async () => {
  if (skip) return
  lt = new Laptimer({ url: /** @type {string} */ (url), key: /** @type {string} */ (key) })
  await clean()
})

beforeEach(async () => {
  if (!skip) await clean()
})

after(() => lt?.close())

test('HTTP: ping, state, channels, stats', { skip }, async () => {
  const pong = await lt.ping()
  assert.equal(pong.pong, true)
  assert.ok(pong.now_us > 1.7e15)

  const state = await lt.state()
  assert.ok(Array.isArray(state.channels) && state.channels.length > 0)
  assert.equal(state.session, null)
  assert.deepEqual(await lt.channels.list(), state.channels)

  const stats = await lt.stats()
  assert.ok(Array.isArray(stats.all) && Array.isArray(stats.today))
  assert.equal(typeof stats.today_races, 'number')
})

test('HTTP: pilots and channels', { skip }, async () => {
  const p = await lt.pilots.create(`A${tag}`, 'Anna')
  assert.equal(p.name, `A${tag}`)
  assert.equal(p.full_name, 'Anna')
  assert.deepEqual(await lt.pilots.get(p.id), p)

  const changed = await lt.pilots.update(p.id, { full_name: 'Anna B.' })
  assert.equal(changed.name, `A${tag}`)
  assert.equal(changed.full_name, 'Anna B.')
  assert.ok((await lt.pilots.list()).some((x) => x.id === p.id))

  // By callsign (any case), and create: true makes a new one.
  const ch = await lt.channels.set(0, { pilot: `a${tag}` })
  assert.equal(ch.index, 0)
  assert.equal(ch.pilot_id, p.id)
  const all = await lt.channels.assign([`B${tag}`, null], { create: true })
  assert.equal(all[0].pilot, `B${tag}`)
  assert.equal(all[1].pilot_id, null)
  const label = await lt.channels.set(1, { label: 'Gast' })
  assert.equal(label.shown, 'Gast')

  // Clean up: free the channels, delete the pilots.
  await lt.channels.assign([null, null])
  await lt.channels.set(1, { label: '' })
  await lt.pilots.delete(p.id)
  await lt.pilots.delete(all[0].pilot_id ?? -1)
  await assert.rejects(lt.pilots.get(p.id), (e) => e instanceof LaptimerError && e.status === 400)
})

test('errors: bad key, unknown command, bad arguments', { skip }, async () => {
  const bad = new Laptimer({ url: /** @type {string} */ (url), key: 'lt_0000' })
  await assert.rejects(bad.ping(), (e) => {
    assert.ok(e instanceof LaptimerError)
    assert.equal(e.status, 401)
    assert.match(e.message, /Schlüssel/)
    return true
  })
  // The WebSocket can't tell why it failed; connect() asks over HTTP.
  await assert.rejects(bad.connect(), (e) => e instanceof LaptimerError && e.status === 401)

  await assert.rejects(lt.command('gibts.nicht'), (e) => {
    assert.ok(e instanceof LaptimerError)
    assert.equal(e.status, 404)
    assert.equal(e.command, 'gibts.nicht')
    assert.match(e.message, /Unbekannter Befehl/)
    return true
  })
  await assert.rejects(lt.session.start({ race: true, laps: 1000 }), (e) => e instanceof LaptimerError && e.status === 400 && /laps/.test(e.message))
  await assert.rejects(lt.session.stop(), (e) => e instanceof LaptimerError && e.status === 400 && /keine Session/.test(e.message))

  const nowhere = new Laptimer({ url: 'http://127.0.0.1:9', key: 'lt_x', timeout_ms: 2000 })
  await assert.rejects(nowhere.ping(), (e) => e instanceof LaptimerError && e.status === null && /nicht erreichbar/.test(e.message))
})

test('WebSocket: hello, commands, error replies', { skip }, async () => {
  const hello = await lt.connect()
  assert.equal(hello.type, 'hello')
  assert.equal(hello.api, 1)
  assert.ok(hello.commands.includes('session.start'))
  assert.ok(hello.state && Array.isArray(hello.state.channels))
  assert.equal(lt.connected, true)
  assert.equal(await lt.connect(), hello) // connecting twice: same connection

  // Now commands go over the socket.
  const pong = await lt.ping()
  assert.equal(pong.pong, true)
  const [a, b] = await Promise.all([lt.channels.list(), lt.state()])
  assert.deepEqual(a, b.channels)
  await assert.rejects(lt.command('gibts.nicht'), (e) => e instanceof LaptimerError && e.status === null && /Unbekannter Befehl/.test(e.message))
  await assert.rejects(lt.pilots.get(999999), (e) => e instanceof LaptimerError && e.command === 'pilots.get')
})

test('events: training with passes', { skip }, async () => {
  await lt.connect()
  /** @type {string[]} */
  const seen = []
  const offAll = lt.on('*', (_data, msg) => seen.push(msg.event))

  const started = lt.waitFor('session_started')
  const signal = lt.waitFor('start_signal')
  const session = await lt.session.start()
  assert.equal(session.race, false)
  assert.equal((await started).session_id, session.id)
  assert.equal((await signal).session_id, session.id)

  // Two passes with explicit times (at least the minimum lap apart).
  const t0 = session.start_us + 1_000_000
  const first = lt.waitFor('pilot_pass', { filter: (d) => d.session_id === session.id })
  const p1 = await lt.passes.add(0, { time_us: t0 })
  const d1 = await first
  assert.equal(d1.pass_id, p1.pass_id)
  assert.equal(d1.channel, 0)
  assert.equal(d1.manual, true)
  assert.equal(d1.kind, 'start')
  assert.equal(d1.laps, 0)

  const second = lt.waitFor('pilot_pass', { filter: (d) => d.session_id === session.id })
  await lt.passes.add(0, { time_us: t0 + 4_000_000 })
  const d2 = await second
  assert.equal(d2.kind, 'lap')
  assert.equal(d2.lap, 1)
  assert.equal(d2.lap_time_us, 4_000_000)
  assert.equal(d2.lap_time_s, 4)
  assert.equal(d2.personal_best, true)
  assert.equal(d2.position, 1)

  // Too close to another pass: refused with the server's message.
  await assert.rejects(lt.passes.add(0, { time_us: t0 + 5_000_000 }), (e) => e instanceof LaptimerError)

  // Delete a pass: passes_changed. A pass added by hand is removed for good
  // (only detected ones can be restored), so add it again.
  const changed = lt.waitFor('passes_changed')
  await lt.passes.delete(p1.pass_id)
  assert.equal((await changed).session_id, session.id)
  await assert.rejects(lt.passes.restore(p1.pass_id), (e) => e instanceof LaptimerError && /gibt es nicht/.test(e.message))
  const again = lt.waitFor('pilot_pass', { filter: (d) => d.session_id === session.id })
  await lt.passes.add(0, { time_us: t0 })
  assert.equal((await again).kind, 'start')

  const stopped = lt.waitFor('session_stopped')
  await lt.session.stop()
  const s = await stopped
  assert.equal(s.session_id, session.id)
  assert.equal(s.reason, 'stopped')
  assert.equal(s.race, false)
  assert.equal(s.needs_confirmation, false)
  assert.equal(s.results[0].laps, 1)

  const full = await lt.sessions.get(session.id)
  assert.equal(full.phase, 'ended')
  assert.equal(full.passes?.length, 2)
  assert.ok((await lt.sessions.list({ limit: 5 })).some((x) => x.id === session.id))

  offAll()
  for (const name of ['session_started', 'start_signal', 'pilot_pass', 'passes_changed', 'session_stopped']) assert.ok(seen.includes(name), name)
})

test('events: race to the finish, then confirm', { skip }, async () => {
  await lt.connect()
  const channels = (await lt.channels.list()).filter((c) => c.enabled)

  const session = await lt.session.start({ race: true, laps: 1, countdown_s: 0, start_mode: 'from_start' })
  assert.equal(session.race, true)
  assert.equal(session.end, 'laps')
  assert.equal(session.end_value, 1)

  // One lap each; the first channel is fastest. All finished: the race stops by itself.
  /** @type {import('../src/types.js').PilotFinishedData[]} */
  const finished = []
  const offFinished = lt.on('pilot_finished', (d) => finished.push(d))
  const stopped = lt.waitFor('session_stopped', { filter: (d) => d.session_id === session.id })
  for (const [i, c] of channels.entries()) {
    await lt.passes.add(c.index, { time_us: session.start_us + 5_000_000 + i * 100_000 })
  }
  const s = await stopped
  offFinished()
  assert.equal(s.reason, 'finished')
  assert.equal(s.needs_confirmation, true)
  assert.equal(s.winner, channels[0].shown)
  assert.equal(finished.length, channels.length)
  assert.equal(finished.find((f) => f.channel === channels[0].index)?.position, 1)

  // Waiting for confirmation: no new session.
  assert.equal((await lt.state()).unconfirmed?.id, session.id)
  await assert.rejects(lt.session.start(), (e) => e instanceof LaptimerError && /bestätig/i.test(e.message))

  const confirmed = lt.waitFor('race_confirmed')
  assert.deepEqual(await lt.session.confirm(), { session_id: session.id })
  const c = await confirmed
  assert.equal(c.session_id, session.id)
  assert.equal(c.winner, channels[0].shown)
  assert.equal((await lt.state()).unconfirmed, null)
})

test('cancel and discard: session_deleted', { skip }, async () => {
  await lt.connect()
  const s1 = await lt.session.start()
  const deleted = lt.waitFor('session_deleted', { filter: (d) => d.session_id === s1.id })
  const stopped = lt.waitFor('session_stopped', { filter: (d) => d.session_id === s1.id })
  await lt.session.cancel()
  assert.equal((await stopped).reason, 'cancelled')
  await deleted
  await assert.rejects(lt.sessions.get(s1.id), LaptimerError)

  const s2 = await lt.session.start()
  await lt.session.stop()
  const deleted2 = lt.waitFor('session_deleted')
  await lt.session.discard(s2.id)
  assert.equal((await deleted2).session_id, s2.id)
})

test('transport http keeps commands on HTTP; close() stops events', { skip }, async () => {
  const http = new Laptimer({ url: /** @type {string} */ (url), key: /** @type {string} */ (key), transport: 'http' })
  await http.connect()
  assert.equal(http.connected, true)
  assert.equal((await http.ping()).pong, true)
  /** @type {any[]} */
  const got = []
  http.on('disconnected', (d) => got.push(d))
  http.close()
  await new Promise((r) => setTimeout(r, 200))
  assert.equal(http.connected, false)
  assert.equal(got.length, 1)
  assert.equal((await http.ping()).pong, true) // HTTP still works
})
