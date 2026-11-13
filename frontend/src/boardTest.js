// @ts-check
// Judging the sensor test (Boards → Sensortest, server `boardtest.rs`): the
// boards compared with each other, channel by channel. A board can only be
// called odd in comparison: alone, a slightly high floor may be the room.
// No React here.

/**
 * @typedef {object} TestChannel
 * @property {string} name
 * @property {number} mhz
 * @property {number} count
 * @property {number} per_s
 * @property {number | null} p10_db
 * @property {number | null} median_db
 * @property {number | null} p90_db
 * @property {number | null} max_db
 * @property {number} max_gap_ms
 *
 * @typedef {object} TestBoard
 * @property {number} index
 * @property {string} id
 * @property {string | null} name
 * @property {import('./types').BoardRole | null} role
 * @property {string | null} failed
 * @property {number | null} lost         during the test
 * @property {number | null} crc_errors   during the test
 * @property {number | null} clock_uncertainty_us
 * @property {number | null} drift_ppm
 * @property {TestChannel[]} channels
 *
 * @typedef {object} BoardTestResult   GET /api/boards/test
 * @property {'noise' | 'reference'} kind
 * @property {number} seconds
 * @property {number} elapsed_s
 * @property {boolean} running
 * @property {TestBoard[]} boards
 *
 * @typedef {{level: 'warn' | 'bad', channel: number | null, text: string}} Finding
 * @typedef {{board: TestBoard, label: string, findings: Finding[]}} BoardVerdict
 * @typedef {{boards: BoardVerdict[], notes: string[], sender: number | null}} Verdict
 */

/** With all transmitters off, a board's floor this far above the others' is odd, dB. */
export const NOISE_ABOVE_DB = 3
/** One channel this far above the board's other channels: a spur, dB. */
export const SPUR_DB = 4
/** p10 to p90 wider than this with no transmitter: a restless floor, dB. */
export const RESTLESS_DB = 8
/** "All transmitters off", but half the boards or more above this on a
 * channel: something sends there (a board alone above it is judged instead). */
export const SOMETHING_SENDS_DB = -50
/** With the reference transmitter: this far below the best board is odd, dB. */
export const WEAKER_DB = 6
/** Fewer values per second than this share of the other boards': odd. */
export const RATE_SHARE = 0.8
/** A gap between two values of a channel longer than this, ms. */
export const MAX_GAP_MS = 50

/** @param {number[]} v */
const median = (v) => {
  const s = [...v].sort((a, b) => a - b)
  return s.length ? s[Math.floor(s.length / 2)] : null
}

/** "S1", or "B2" by its number (as in the graphs). @param {TestBoard} b */
export const testBoardLabel = (b) => b.name || `B${b.index + 1}`

/** @param {BoardTestResult} r @returns {Verdict} */
export function judge(r) {
  // Boards without any value (e.g. unplugged) are judged, but not compared.
  const boards = r.boards
  const live = boards.filter((b) => !b.failed && b.channels.some((c) => c.count > 0))
  const nCh = Math.max(0, ...boards.map((b) => b.channels.length))
  /** @type {string[]} */
  const notes = []

  // Reference: the channel the transmitter is on = the loudest median.
  let sender = /** @type {number | null} */ (null)
  if (r.kind === 'reference') {
    let best = -Infinity
    for (let c = 0; c < nCh; c++) {
      const m = Math.max(...live.map((b) => b.channels[c]?.median_db ?? -Infinity))
      if (m > best) [best, sender] = [m, c]
    }
    if (sender == null || best < -45) {
      notes.push('Kein Sender erkannt: auf keinem Kanal liegt der Pegel deutlich über dem Rauschen. Sender an, auf einem eingestellten Kanal?')
      sender = null
    }
  }
  // Noise: a channel where half the boards or more hear something is the
  // room (a transmitter left on), not a board: not judged.
  /** @type {Set<number>} */
  const loudRoom = new Set()
  if (r.kind === 'noise' && live.length > 1) {
    for (let c = 0; c < nCh; c++) {
      const loud = live.filter((b) => (b.channels[c]?.median_db ?? -Infinity) > SOMETHING_SENDS_DB).length
      if (loud * 2 >= live.length) loudRoom.add(c)
    }
    if (loudRoom.size) {
      const names = [...loudRoom].map((c) => live[0].channels[c]?.name).join(', ')
      notes.push(
        `Auf ${names} empfangen mehrere Boards ein Signal: war wirklich jeder Sender aus (auch Brillen; WLAN auf 5,8 GHz)? Diese Kanäle sind nicht bewertet; den Test ohne Sender wiederholen.`,
      )
    }
  }

  const verdicts = boards.map((b) => {
    /** @type {Finding[]} */
    const f = []
    if (b.failed) f.push({ level: 'bad', channel: null, text: `Board meldet einen Fehler: ${b.failed}` })
    if (b.lost) f.push({ level: 'warn', channel: null, text: `${b.lost} Messwerte verloren (Board kam mit dem Senden nicht nach).` })
    if (b.crc_errors) f.push({ level: 'warn', channel: null, text: `${b.crc_errors} fehlerhafte Pakete (CRC): USB-Kabel oder Port prüfen.` })
    const others = live.filter((o) => o !== b)
    const ownMedians = /** @type {number[]} */ (b.channels.map((c) => c.median_db).filter((v) => v != null))
    const ownMid = median(ownMedians)

    b.channels.forEach((ch, c) => {
      const name = ch.name || `Kanal ${c + 1}`
      if (!ch.count) {
        if (!b.failed) f.push({ level: 'bad', channel: c, text: `${name}: keine Messwerte.` })
        return
      }
      const rates = others.map((o) => o.channels[c]?.per_s ?? 0).filter((v) => v > 0)
      const rate = median(rates)
      if (rate != null && ch.per_s < RATE_SHARE * rate) {
        f.push({ level: 'warn', channel: c, text: `${name}: nur ${ch.per_s.toFixed(0)} Werte/s, die anderen ${rate.toFixed(0)}/s.` })
      }
      if (ch.max_gap_ms > MAX_GAP_MS) {
        f.push({ level: 'warn', channel: c, text: `${name}: Lücke von ${ch.max_gap_ms.toFixed(0)} ms zwischen zwei Werten.` })
      }
      if (ch.median_db == null) return

      if (r.kind === 'noise' && !loudRoom.has(c)) {
        const ref = median(/** @type {number[]} */ (others.map((o) => o.channels[c]?.median_db).filter((v) => v != null)))
        const spread = ch.p90_db != null && ch.p10_db != null ? ch.p90_db - ch.p10_db : 0
        if (ref != null && ch.median_db - ref > NOISE_ABOVE_DB) {
          f.push({
            level: 'warn',
            channel: c,
            text: `${name}: Rauschboden ${ch.median_db.toFixed(1)} dBFS, ${(ch.median_db - ref).toFixed(1)} dB über den anderen Boards.`,
          })
        } else if (ref == null && ch.median_db > SOMETHING_SENDS_DB) {
          f.push({ level: 'warn', channel: c, text: `${name}: Rauschboden ${ch.median_db.toFixed(1)} dBFS ist hoch (üblich: unter −50).` })
        } else if (ownMid != null && ch.median_db - ownMid > SPUR_DB) {
          f.push({ level: 'warn', channel: c, text: `${name}: ${(ch.median_db - ownMid).toFixed(1)} dB über den anderen Kanälen dieses Boards (Störlinie?).` })
        }
        if (spread > RESTLESS_DB) {
          f.push({ level: 'warn', channel: c, text: `${name}: unruhiger Rauschboden (${spread.toFixed(1)} dB zwischen leise und laut).` })
        }
      }
      if (r.kind === 'reference' && c === sender) {
        const best = Math.max(...live.map((o) => o.channels[c]?.median_db ?? -Infinity))
        if (best - ch.median_db > WEAKER_DB) {
          f.push({ level: 'warn', channel: c, text: `${name}: ${(best - ch.median_db).toFixed(1)} dB schwächer als das beste Board.` })
        }
      }
    })
    return { board: b, label: testBoardLabel(b), findings: f }
  })
  return { boards: verdicts, notes, sender }
}
