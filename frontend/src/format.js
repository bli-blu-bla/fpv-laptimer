// @ts-check
// Showing times and settings as text.

/** µs → "12.34" (seconds), "–" for none. @param {number | null | undefined} us */
export function secs(us) {
  return us == null ? '–' : (us / 1e6).toFixed(2)
}

/** µs → "1:05.3" (minutes:seconds). @param {number} us */
export function clock(us) {
  const t = Math.max(0, us) / 1e6
  const m = Math.floor(t / 60)
  const s = t - m * 60
  return `${m}:${s.toFixed(1).padStart(4, '0')}`
}

/** Wall clock µs → "22.10. 15:42". @param {number} us */
export function dateTime(us) {
  const d = new Date(us / 1000)
  return d.toLocaleString('de-DE', { day: '2-digit', month: '2-digit', hour: '2-digit', minute: '2-digit' })
}

/** @type {Record<import('./types').StartMode, string>} */
export const START_MODES = {
  holeshot: 'Holeshot',
  from_start: 'Ab Startton',
  flying: 'Fliegender Start',
}

/** @param {import('./types').EndRule} r */
export function endRule(r) {
  if (r.kind === 'laps') return `${r.value} Runden`
  if (r.kind === 'seconds') return `${r.value} s`
  return 'offen'
}

/** "Rennen · Holeshot · 3 Runden" or "Training". @param {import('./types').SessionConfig} c */
export function sessionKind(c) {
  return c.race ? `Rennen · ${START_MODES[c.start_mode]} · ${endRule(c.end_rule)}` : 'Training'
}
