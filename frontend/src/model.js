// @ts-check
// Turning the server's data into what the pages show: slots, standings,
// the pass log. No React here.

import { secs, slotColor } from './format.js'

/**
 * @typedef {object} Slot   one place in the channel list
 * @property {number} index
 * @property {string} channel     e.g. "R3"
 * @property {number} mhz
 * @property {string} color
 * @property {boolean} enabled
 * @property {import('./types').Pilot | null} pilot
 * @property {string} callsign    what to show: free text, pilot callsign, or channel
 * @property {string} fullName
 */

/** The slots as set up now (before a session).
 * @param {import('./types').State} state @returns {Slot[]} */
export function slotsOf(state) {
  return state.settings.channels.map((ch, i) => {
    const seat = state.settings.seats[i] ?? { pilot_id: null, label: '', enabled: true }
    const pilot = state.pilots.find((p) => p.id === seat.pilot_id) ?? null
    return {
      index: i,
      channel: ch.name,
      mhz: ch.mhz,
      color: slotColor(i),
      enabled: seat.enabled !== false,
      pilot,
      callsign: seat.label.trim() || pilot?.name || ch.name,
      fullName: seat.label.trim() ? '' : (pilot?.full_name ?? ''),
    }
  })
}

/**
 * @typedef {object} Standing
 * @property {number | null} pos     1, 2, …; null before the first lap
 * @property {string} gap           "Führt", "+0.42", "+1 Rd"
 */

/** Positions and gaps, by seat index. In a race: more laps first, then who
 * got there earlier; in training: the best lap.
 * @param {import('./types').Session} s @param {import('./types').PilotResult[]} results
 * @returns {Standing[]} */
export function standings(s, results) {
  const idx = results.map((_, i) => i).filter((i) => results[i].laps.length > 0)
  if (s.config.race) {
    idx.sort((a, b) => results[b].laps.length - results[a].laps.length || (results[a].total_us ?? Infinity) - (results[b].total_us ?? Infinity))
  } else {
    idx.sort((a, b) => (results[a].best_lap_us ?? Infinity) - (results[b].best_lap_us ?? Infinity))
  }
  const out = results.map(() => /** @type {Standing} */ ({ pos: null, gap: '–' }))
  const lead = results[idx[0]]
  idx.forEach((i, k) => {
    const r = results[i]
    let gap = 'Führt'
    if (k > 0) {
      if (!s.config.race) gap = '+' + secs((r.best_lap_us ?? 0) - (lead.best_lap_us ?? 0))
      else if (r.laps.length < lead.laps.length) gap = `+${lead.laps.length - r.laps.length} Rd`
      else gap = '+' + secs((r.total_us ?? 0) - (lead.total_us ?? 0))
    }
    out[i] = { pos: k + 1, gap }
  })
  return out
}

/**
 * @typedef {object} LogEntry
 * @property {import('./types').Pass} pass
 * @property {import('./types').Seat} seat
 * @property {string} color
 * @property {string} what     "Runde 3", "Holeshot", "Start", "zählt nicht"
 * @property {string} time     the lap time (or holeshot), "" if none
 * @property {string} delta    "Bestzeit", "+0.42" or ""
 * @property {boolean} best
 */

/** Every pass of a session that is not deleted, newest first.
 * @param {import('./types').SessionDetail} d @returns {LogEntry[]} */
export function passLog(d) {
  const out = []
  for (const p of d.passes ?? []) {
    if (p.deleted) continue
    const i = d.session.seats.findIndex((s) => s.channel === p.channel)
    if (i < 0) continue
    const r = d.results[i]
    const lap = r.laps.find((l) => l.pass_id === p.id)
    const best = lap != null && lap.duration_us === r.best_lap_us
    let what = 'Start'
    let time = ''
    if (lap) {
      what = `Runde ${lap.number}`
      time = secs(lap.duration_us)
    } else if (r.ignored.includes(p.id)) {
      what = 'zählt nicht'
    } else if (r.holeshot_us != null) {
      what = 'Holeshot'
      time = secs(r.holeshot_us)
    }
    out.push({
      pass: p,
      seat: d.session.seats[i],
      color: slotColor(p.channel),
      what,
      time,
      best,
      delta: lap ? (best ? 'Bestzeit' : '+' + secs(lap.duration_us - (r.best_lap_us ?? lap.duration_us))) : '',
    })
  }
  return out.sort((a, b) => b.pass.time_us - a.pass.time_us)
}

/**
 * Laps of one channel from pass times, as the server computes them
 * (server/src/laps.rs, `compute`): the pass editor shows the laps of its
 * draft before it is saved.
 * @param {{id: number, time_us: number}[]} passes   this channel's passes that count
 * @param {import('./types').SessionConfig} c
 * @param {number} startUs   race: the start signal; training: the session start
 * @returns {{laps: {number: number, duration_us: number, end_us: number, pass_id: number}[], holeshot_us: number | null, finished_us: number | null, ignored: number[], best_lap_us: number | null}}
 */
export function computeLaps(passes, c, startUs) {
  const sorted = [...passes].sort((a, b) => a.time_us - b.time_us)
  /** @type {ReturnType<typeof computeLaps>} */
  const r = { laps: [], holeshot_us: null, finished_us: null, ignored: [], best_lap_us: null }
  let lapStart = c.start_mode === 'from_start' ? startUs : null
  for (const p of sorted) {
    if (p.time_us < startUs || r.finished_us != null) {
      r.ignored.push(p.id)
      continue
    }
    if (lapStart == null) {
      if (c.start_mode === 'holeshot') r.holeshot_us = p.time_us - startUs
      lapStart = p.time_us
      continue
    }
    r.laps.push({ number: r.laps.length + 1, duration_us: p.time_us - lapStart, end_us: p.time_us, pass_id: p.id })
    lapStart = p.time_us
    const e = c.end_rule
    if ((e.kind === 'laps' && r.laps.length >= e.value) || (e.kind === 'seconds' && p.time_us - startUs >= e.value * 1e6)) {
      r.finished_us = p.time_us
    }
  }
  r.best_lap_us = r.laps.length ? Math.min(...r.laps.map((l) => l.duration_us)) : null
  return r
}
