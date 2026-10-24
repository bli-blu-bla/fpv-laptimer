// @ts-check
// Choosing the channels like widscan58's "Channels to scan": one row per
// band on a common frequency axis, each channel a bell; tap one to select
// it, tap a band name for all eight. Dashed lines show where a selected
// frequency lies in the other bands. Below: the IMD rating of the selection
// (imd.js) with the products that land too close, also as red marks on the
// axis. The slots get their frequencies by `assignSlots`, so the bells
// already show the slot colours.

import { useState } from 'react'
import { MagicWandIcon, PlusIcon } from '@phosphor-icons/react'
import { BANDS, assignSlots, bestSet, channelName, imdRating } from '../imd.js'
import { slotColor } from '../format.js'
import Dialog from './Dialog.jsx'
import ImdBadge from './ImdBadge.jsx'

const MAX = 8

// Drawing in SVG units; the SVG scales with the dialog (and scrolls
// sideways on a phone, see .chpick).
const W = 1000
const X0 = 150 // plot starts right of the band names
const X1 = 990
const AXIS_H = 34
const ROW_H = 74
const BELL_H = 60
const BELL_MHZ = 19 // half the width of a bell at its foot

/** Frequency ranges of the two axes: the 5.8 GHz bands, and L below. */
const MAIN = { from: 5630, to: 5960 }
const LOW = { from: 5330, to: 5660 }

/**
 * @param {object} props
 * @param {number[]} props.value  MHz per slot now
 * @param {(mhz: number[]) => void} props.onSave
 * @param {() => void} props.onClose
 */
export default function ChannelPicker({ value, onSave, onClose }) {
  const [sel, setSel] = useState(() => [...new Set(value)])
  const [free, setFree] = useState('')
  const [hint, setHint] = useState('')

  const slots = assignSlots(value, sel)
  /** @param {number} mhz */
  const colorOf = (mhz) => {
    const i = slots.findIndex((s) => s.mhz === mhz)
    return i < 0 ? null : slotColor(i)
  }
  const imd = imdRating(sel)

  /** @param {number} mhz */
  function toggle(mhz) {
    setHint('')
    if (sel.includes(mhz)) setSel(sel.filter((f) => f !== mhz))
    else if (sel.length >= MAX) setHint(`Höchstens ${MAX} Kanäle.`)
    else setSel([...sel, mhz])
  }
  /** @param {readonly number[]} freqs */
  const pickBand = (freqs) => {
    setHint('')
    setSel([...freqs])
  }
  function addFree() {
    const mhz = Math.round(Number(free.replace(',', '.')))
    if (!(mhz >= 5300 && mhz <= 6000)) return setHint('Frequenz zwischen 5300 und 6000 MHz.')
    if (!sel.includes(mhz)) toggle(mhz)
    setFree('')
  }
  function optimize() {
    setHint('')
    setSel(bestSet(Math.max(sel.length, 1)))
  }

  // Frequencies chosen that are in no band (typed in).
  const inBands = new Set(BANDS.flatMap(([, , f]) => f))
  const extra = sel.filter((f) => !inBands.has(f))

  const mainBands = BANDS.filter(([b]) => b !== 'L')
  const lowBand = BANDS.filter(([b]) => b === 'L')
  const lowTop = AXIS_H + mainBands.length * ROW_H + 10
  const height = lowTop + AXIS_H + ROW_H + 4

  return (
    <Dialog title="Kanäle wählen" wide onClose={onClose}>
      <span className="muted small" style={{ marginTop: -10 }}>
        Bis zu {MAX} Kanäle. Bandnamen antippen wählt alle acht Kanäle des Bands. Die Farben zeigen, welcher Slot den Kanal bekommt.
      </span>
      <div className="chpick">
        <svg viewBox={`0 0 ${W} ${height}`} role="group" aria-label="Kanäle">
          <Axis top={0} range={MAIN} hits={imd.hits.map((h) => h.imd)} />
          <g>
            {[...sel].map((f) =>
              f >= MAIN.from && f <= MAIN.to ? (
                <line key={f} className="chpick-guide" x1={x(f, MAIN)} x2={x(f, MAIN)} y1={AXIS_H} y2={AXIS_H + mainBands.length * ROW_H} />
              ) : null,
            )}
          </g>
          {mainBands.map(([band, label, freqs], r) => (
            <BandRow key={band} top={AXIS_H + r * ROW_H} band={band} label={label} freqs={freqs} range={MAIN} colorOf={colorOf} onPick={toggle} onBand={pickBand} />
          ))}
          <Axis top={lowTop} range={LOW} hits={imd.hits.map((h) => h.imd)} />
          {lowBand.map(([band, label, freqs]) => (
            <BandRow key={band} top={lowTop + AXIS_H} band={band} label={label} freqs={freqs} range={LOW} colorOf={colorOf} onPick={toggle} onBand={pickBand} />
          ))}
        </svg>
      </div>

      {extra.length > 0 && (
        <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', alignItems: 'center' }}>
          <span className="muted small">Freie Frequenzen:</span>
          {extra.map((f) => (
            <button key={f} className="tag" style={{ border: 0, cursor: 'pointer', background: colorOf(f) ?? undefined, color: '#161826' }} onClick={() => toggle(f)} title="Entfernen">
              {f} MHz ✕
            </button>
          ))}
        </div>
      )}

      <div className="chpick-imd">
        <ImdBadge rating={imd.rating} />
        <div style={{ display: 'flex', flexDirection: 'column', gap: 2, minWidth: 0 }}>
          <span className="small">{imdText(imd.rating, sel.length)}</span>
          {imd.hits.slice(0, 3).map((h, i) => (
            <span key={i} className="muted small tnum">
              2 × {name(h.a)} − {name(h.b)} = {h.imd} MHz, nur {h.diff} MHz neben {name(h.near)}
            </span>
          ))}
          {imd.hits.length > 3 && <span className="muted small">… und {imd.hits.length - 3} weitere</span>}
        </div>
      </div>

      <div className="chpick-foot">
        <button className="btn btn-secondary" onClick={() => setSel([])}>
          Leeren
        </button>
        <button className="btn btn-secondary" onClick={optimize} title="Beste IMD-Bewertung für diese Anzahl Kanäle, bevorzugt Raceband">
          <MagicWandIcon size={16} /> Optimal verteilen
        </button>
        <span className="muted small tnum">
          {sel.length} von {MAX} gewählt
        </span>
        <span className="chpick-free">
          <input
            className="input tnum"
            inputMode="numeric"
            placeholder="MHz"
            value={free}
            onChange={(e) => setFree(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && addFree()}
            title="Andere Frequenz in MHz"
          />
          <button className="btn btn-ghost btn-icon" onClick={addFree} title="Frequenz hinzufügen" disabled={!free}>
            <PlusIcon size={18} />
          </button>
        </span>
        {hint && <span className="error small">{hint}</span>}
        <span style={{ marginLeft: 'auto', display: 'flex', gap: 8 }}>
          <button className="btn btn-secondary" onClick={onClose}>
            Abbrechen
          </button>
          <button className="btn btn-primary" disabled={!sel.length} onClick={() => onSave(sel)}>
            Übernehmen
          </button>
        </span>
      </div>
    </Dialog>
  )
}

/** @param {number} mhz */
const name = (mhz) => channelName(mhz) ?? `${mhz}`

/** @param {number} f @param {{from: number, to: number}} range */
const x = (f, range) => X0 + ((f - range.from) / (range.to - range.from)) * (X1 - X0)

/** @param {number | null} rating @param {number} count */
export function imdText(rating, count) {
  if (rating == null) return count < 2 ? 'Ein Kanal: keine Störungen untereinander.' : 'Zwei Kanäle: keine Intermodulation auf einem dritten.'
  if (rating >= 90) return 'IMD-Bewertung sehr gut: kaum Störungen zwischen den Sendern.'
  if (rating >= 50) return 'IMD-Bewertung gut: kleine Störungen möglich.'
  if (rating >= 0) return 'IMD-Bewertung mäßig: Sender stören einander sichtbar.'
  return 'IMD-Bewertung schlecht: viele Mischprodukte nahe an genutzten Kanälen.'
}

/** Frequency axis: ticks every 10 MHz, labels every 50, red marks at IMD
 * products that cost points.
 * @param {{top: number, range: {from: number, to: number}, hits: number[]}} props */
function Axis({ top, range, hits }) {
  const ticks = []
  for (let f = Math.ceil(range.from / 10) * 10; f <= range.to; f += 10) ticks.push(f)
  const y = top + AXIS_H - 4
  return (
    <g>
      <line className="chpick-axis" x1={X0 - 10} x2={X1} y1={y} y2={y} />
      {ticks.map((f) => (
        <g key={f}>
          <line className="chpick-axis" x1={x(f, range)} x2={x(f, range)} y1={y} y2={y - (f % 50 ? 5 : 11)} />
          {f % 50 === 0 && (
            <text className="chpick-axis-label" x={x(f, range)} y={top + 12} textAnchor="middle">
              {f}
            </text>
          )}
        </g>
      ))}
      {hits
        .filter((f) => f >= range.from && f <= range.to)
        .map((f, i) => (
          <path key={i} className="chpick-hit" d={`M ${x(f, range) - 5} ${y - 9} l 10 0 l -5 9 z`}>
            <title>Mischprodukt bei {f} MHz</title>
          </path>
        ))}
    </g>
  )
}

/**
 * @param {object} props
 * @param {number} props.top
 * @param {string} props.band
 * @param {string} props.label
 * @param {readonly number[]} props.freqs
 * @param {{from: number, to: number}} props.range
 * @param {(mhz: number) => string | null} props.colorOf
 * @param {(mhz: number) => void} props.onPick
 * @param {(freqs: readonly number[]) => void} props.onBand
 */
function BandRow({ top, band, label, freqs, range, colorOf, onPick, onBand }) {
  const base = top + ROW_H - 4
  return (
    <g>
      <line className="chpick-row" x1={0} x2={W} y1={top + ROW_H} y2={top + ROW_H} />
      <g className="chpick-band" onClick={() => onBand(freqs)}>
        <rect x={4} y={top + 8} width={X0 - 24} height={ROW_H - 16} rx={8} />
        <text x={4 + (X0 - 24) / 2} y={top + 34} textAnchor="middle" className="b">
          {band}
        </text>
        <text x={4 + (X0 - 24) / 2} y={top + 54} textAnchor="middle" className="n">
          {label}
        </text>
      </g>
      {freqs.map((f, i) => {
        const color = colorOf(f)
        const cx = x(f, range)
        return (
          <g key={f} className={color ? 'chpick-ch on' : 'chpick-ch'} onClick={() => onPick(f)}>
            <title>
              {band}
              {i + 1} · {f} MHz
            </title>
            <path d={bell(cx, base, x(f + BELL_MHZ, range) - cx)} style={color ? { fill: color, stroke: color } : undefined} />
            <text x={cx} y={base - BELL_H + 24} textAnchor="middle" className="b">
              {band}
              {i + 1}
            </text>
            <text x={cx} y={base - BELL_H + 42} textAnchor="middle" className="n">
              {f}
            </text>
          </g>
        )
      })}
    </g>
  )
}

/** A bell (Gaussian) of BELL_H standing on `base`, `half` units to each side. @param {number} cx @param {number} base @param {number} half */
function bell(cx, base, half) {
  const pts = []
  for (let k = -20; k <= 20; k++) {
    const t = k / 20
    pts.push(`${(cx + t * half).toFixed(1)} ${(base - BELL_H * Math.exp(-((t * 2.6) ** 2) / 2)).toFixed(1)}`)
  }
  return `M ${pts.join(' L ')} Z`
}
