// @ts-check
// Showing times, settings and slots.

/** µs → "12.34" (seconds), "–" for none. @param {number | null | undefined} us */
export function secs(us) {
  return us == null ? '–' : (us / 1e6).toFixed(2)
}

/** µs → "01:05.30" (minutes:seconds). @param {number} us */
export function clock(us) {
  const t = Math.max(0, us) / 1e6
  const m = Math.floor(t / 60)
  const s = t - m * 60
  return `${String(m).padStart(2, '0')}:${s.toFixed(2).padStart(5, '0')}`
}

/** Wall clock µs → "15:42". @param {number} us */
export function time(us) {
  return new Date(us / 1000).toLocaleTimeString('de-DE', { hour: '2-digit', minute: '2-digit' })
}

/** Wall clock µs → "22.10. 15:42", or "Heute 15:42". @param {number} us */
export function dateTime(us) {
  const d = new Date(us / 1000)
  if (d.toDateString() === new Date().toDateString()) return 'Heute ' + time(us)
  return d.toLocaleString('de-DE', { day: '2-digit', month: '2-digit', hour: '2-digit', minute: '2-digit' })
}

/** @type {Record<import('./types').StartMode, string>} */
export const START_MODES = {
  holeshot: 'Holeshot',
  from_start: 'Ab Startton',
  flying: 'Fliegend',
}

/** @param {import('./types').EndRule} r */
export function endRule(r) {
  if (r.kind === 'laps') return `${r.value} Runden`
  if (r.kind === 'seconds') return r.value % 60 === 0 ? `${r.value / 60} min` : `${r.value} s`
  return 'frei'
}

/** "Holeshot · 3 Runden" or "Training". @param {import('./types').SessionConfig} c */
export function sessionKind(c) {
  return c.race ? `${START_MODES[c.start_mode]} · ${endRule(c.end_rule)}` : 'Training · freie Runden'
}

/** Raceband, MHz. */
export const RACEBAND = [5658, 5695, 5732, 5769, 5806, 5843, 5880, 5917]

/** The colour of slot `i` (its place in the channel list). */
const SLOT_COLORS = ['#ff8a3d', '#5ec8e5', '#a6d96a', '#e38ad6', '#f2d45c', '#7b9cff', '#ff6b6b', '#5fd3b5']
/** @param {number} i */
export function slotColor(i) {
  return SLOT_COLORS[i % SLOT_COLORS.length]
}

/** "KB" from "Kai Berger", else from the callsign. @param {string} name @param {string} callsign */
export function initials(name, callsign) {
  const parts = (name.trim() || callsign.trim()).split(/\s+/)
  return parts.length > 1 ? (parts[0][0] + parts[1][0]).toUpperCase() : parts[0].slice(0, 2).toUpperCase()
}
