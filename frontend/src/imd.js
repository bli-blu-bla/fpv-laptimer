// @ts-check
// Frequency bands and the IMD rating of a channel set, like ET's IMDTabler
// (the one RotorHazard calls as a Java program; here recomputed, checked
// against etserv.etheli.com/IMDTabler for Raceband, IMD 6C, ET5, Racebnd4).
//
// Two video transmitters at f1 and f2 produce intermodulation at 2·f1 − f2.
// If that lands near a third channel, the pilot there sees noise. For every
// ordered pair the product is compared with the nearest selected frequency;
// each one closer than 35 MHz costs (35 − Abstand)², the sum is divided by
// the number of channels and by 5 and subtracted from 100.

/** Bands as the transmitters number them (A and E are not in order):
 * [letter, name, MHz of channels 1–8].
 * @type {[string, string, number[]][]} */
export const BANDS = [
  ['R', 'Raceband', [5658, 5695, 5732, 5769, 5806, 5843, 5880, 5917]],
  ['A', 'Boscam A', [5865, 5845, 5825, 5805, 5785, 5765, 5745, 5725]],
  ['B', 'Boscam B', [5733, 5752, 5771, 5790, 5809, 5828, 5847, 5866]],
  ['E', 'Boscam E', [5705, 5685, 5665, 5645, 5885, 5905, 5925, 5945]],
  ['F', 'Fatshark', [5740, 5760, 5780, 5800, 5820, 5840, 5860, 5880]],
  ['L', 'Low band', [5362, 5399, 5436, 5473, 5510, 5547, 5584, 5621]],
]

/** Name of a frequency ("R3"), preferring Raceband; null for a free one.
 * @param {number} mhz */
export function channelName(mhz) {
  for (const [band, , freqs] of BANDS) {
    const i = freqs.indexOf(mhz)
    if (i >= 0) return `${band}${i + 1}`
  }
  return null
}

/** IMDTabler leaves products above this out of its table. */
const IMD_MAX = 6100
const IMD_RANGE = 35

/** @typedef {{a: number, b: number, imd: number, near: number, diff: number}} ImdHit
 *  2·a − b lands `diff` MHz from the selected channel `near`. */

/**
 * Rating (100 = best, may go negative) and the products that cost points.
 * Fewer than 3 different channels: no products can hit a third, rating null.
 * @param {number[]} mhz
 * @returns {{rating: number | null, hits: ImdHit[]}}
 */
export function imdRating(mhz) {
  const f = [...new Set(mhz)]
  if (f.length < 3) return { rating: null, hits: [] }
  /** @type {ImdHit[]} */
  const hits = []
  let total = 0
  for (const a of f) {
    for (const b of f) {
      if (a === b) continue
      const imd = 2 * a - b
      if (imd > IMD_MAX) continue
      let near = f[0]
      for (const x of f) if (Math.abs(imd - x) < Math.abs(imd - near)) near = x
      const diff = Math.abs(imd - near)
      if (diff < IMD_RANGE) {
        total += (IMD_RANGE - diff) ** 2
        hits.push({ a, b, imd, near, diff })
      }
    }
  }
  return { rating: Math.round(100 - total / f.length / 5), hits: hits.sort((x, y) => x.diff - y.diff) }
}

/** Smallest distance between two of the channels (Infinity for one). @param {number[]} mhz */
export function minSpacing(mhz) {
  let min = Infinity
  for (let i = 0; i < mhz.length; i++) for (let j = i + 1; j < mhz.length; j++) min = Math.min(min, Math.abs(mhz[i] - mhz[j]))
  return min
}

/** Closest two channels may be for "Optimal verteilen": nearer ones blur
 * into each other at the gate. */
const BEST_GAP = 35

/**
 * The best `n` channels from bands R, A, B, E, F: highest IMD rating, then
 * the most Raceband channels (every VTX has them), then the widest spacing.
 * Tries every set with ≥ 35 MHz between neighbours; a few tens of ms.
 * @param {number} n
 */
export function bestSet(n) {
  const raceband = new Set(BANDS[0][2])
  const all = [...new Set(BANDS.filter(([b]) => b !== 'L').flatMap(([, , f]) => f))].sort((a, b) => a - b)
  let best = { set: /** @type {number[]} */ ([]), rating: -Infinity, race: -1, gap: 0 }
  /** @type {number[]} */
  const cur = []
  /** @param {number} start */
  function walk(start) {
    if (cur.length === n) {
      const rating = imdRating(cur).rating ?? 100
      const race = cur.filter((f) => raceband.has(f)).length
      const gap = minSpacing(cur)
      if (rating > best.rating || (rating === best.rating && (race > best.race || (race === best.race && gap > best.gap))))
        best = { set: [...cur], rating, race, gap }
      return
    }
    for (let i = start; i < all.length; i++) {
      if (cur.length && all[i] - cur[cur.length - 1] < BEST_GAP) continue
      cur.push(all[i])
      walk(i + 1)
      cur.pop()
    }
  }
  walk(0)
  return best.set
}

/**
 * Which slot gets which of the new frequencies: a slot whose frequency stays
 * keeps it, the others take the new ones in their place (pilot and settings
 * stay with the slot), extra frequencies become new slots at the end, slots
 * left over are removed. `from`: the old slot index, null for a new slot.
 * @param {number[]} old  MHz per slot
 * @param {number[]} next  the chosen frequencies
 * @returns {{mhz: number, from: number | null}[]}
 */
export function assignSlots(old, next) {
  const left = [...new Set(next)]
  /** @type {({mhz: number, from: number | null} | null)[]} */
  const out = old.map((mhz, i) => {
    const k = left.indexOf(mhz)
    if (k < 0) return null
    left.splice(k, 1)
    return { mhz, from: i }
  })
  left.sort((a, b) => a - b)
  const placed = out.map((r, i) => r ?? (left.length ? { mhz: /** @type {number} */ (left.shift()), from: i } : null))
  return [...placed.filter((r) => r != null), ...left.map((mhz) => ({ mhz, from: null }))]
}
