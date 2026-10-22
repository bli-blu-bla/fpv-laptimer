// @ts-check
// Manual thresholds of one slot, on race control (as RotorHazard's node
// panel): pick the gate, watch the signal of the last 30 s with the Enter
// and Exit lines, and move them. A gate in automatic mode shows its learned
// lines instead.

import { useEffect, useState } from 'react'
import { Gauge, X } from '@phosphor-icons/react'
import { saveSettings } from '../api.js'
import Stepper from './Stepper.jsx'

const W = 600
const H = 170
const MIN_DB = -62
const MAX_DB = 0
/** Default thresholds of a channel without its own (as the server's). */
const DEFAULT = { enter_db: -25, exit_db: -30 }

/**
 * @param {object} props
 * @param {import('../types').State} props.state
 * @param {number} props.index   the slot (channel position)
 * @param {string} props.color
 * @param {string} props.callsign
 * @param {{finish: import('./Sparkline.jsx').SparkPoint[], arming: import('./Sparkline.jsx').SparkPoint[]}} props.history   ~10 points/s
 * @param {boolean} props.hasArming   a board watches the arming gate
 * @param {() => void} props.onClose
 */
export default function ThresholdPanel({ state, index, color, callsign, history, hasArming, onClose }) {
  const s = state.settings
  const d = s.detection
  const [gate, setGate] = useState(/** @type {'finish' | 'arming'} */ (d.finish_mode === 'classic' || !hasArming ? 'finish' : 'arming'))
  const manual = (gate === 'finish' ? d.finish_mode : d.arming_mode) === 'classic'
  const saved = d[gate][index] ?? DEFAULT
  // Shown at once; saved when the taps rest (Stepper `delayed`).
  const [t, setT] = useState(saved)
  useEffect(() => setT(saved), [saved.enter_db, saved.exit_db, gate]) // eslint-disable-line react-hooks/exhaustive-deps

  /** @param {Partial<import('../types').Thresholds>} part */
  function save(part) {
    const next = { ...t, ...part }
    setT(next)
    const list = s.channels.map((_, j) => d[gate][j] ?? DEFAULT)
    list[index] = next
    saveSettings(s, { detection: { ...d, [gate]: list } })
  }

  const points = history[gate]
  const levels = points.map((p) => p.level).filter((v) => v != null)
  const peak = levels.length ? Math.max(.../** @type {number[]} */ (levels)) : null
  const now = points.at(-1)?.level ?? null
  // Lines: the manual thresholds, or what the automatic mode learned.
  const enter = manual ? t.enter_db : (points.at(-1)?.enter ?? null)
  const exit = manual ? t.exit_db : (points.at(-1)?.exit ?? null)
  /** @param {number} db */
  const y = (db) => H - ((Math.min(MAX_DB, Math.max(MIN_DB, db)) - MIN_DB) / (MAX_DB - MIN_DB)) * H
  const n = Math.max(points.length, 2)
  const line = points
    .map((p, k) => (p.level == null ? null : `${((k / (n - 1)) * W).toFixed(1)},${y(p.level).toFixed(1)}`))
    .filter(Boolean)
    .join(' ')

  return (
    <div className="panel" style={/** @type {React.CSSProperties} */ ({ '--lane': color, padding: 16, display: 'flex', flexDirection: 'column', gap: 12 })}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap' }}>
        <span className="dot" style={{ background: color }} />
        <span style={{ fontWeight: 600 }}>{callsign}</span>
        <span className="muted small">{s.channels[index]?.name} · Schwellen</span>
        {hasArming && (
          <div className="seg" style={{ marginLeft: 'auto' }}>
            <button className={gate === 'finish' ? 'on' : ''} onClick={() => setGate('finish')}>
              Zielgate
            </button>
            <button className={gate === 'arming' ? 'on' : ''} onClick={() => setGate('arming')}>
              Freigabe-Gate
            </button>
          </div>
        )}
        <button className="btn btn-ghost btn-icon" style={hasArming ? undefined : { marginLeft: 'auto' }} onClick={onClose} title="Schließen">
          <X size={18} />
        </button>
      </div>

      <div className="spark" style={{ height: H }}>
        <svg viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none">
          {[-50, -40, -30, -20, -10].map((db) => (
            <line key={db} x1={0} x2={W} y1={y(db)} y2={y(db)} stroke="var(--n900)" vectorEffect="non-scaling-stroke" />
          ))}
          {exit != null && <line x1={0} x2={W} y1={y(exit)} y2={y(exit)} stroke="var(--n400)" strokeDasharray="6 4" vectorEffect="non-scaling-stroke" />}
          {enter != null && <line x1={0} x2={W} y1={y(enter)} y2={y(enter)} stroke="var(--accent)" strokeDasharray="6 4" strokeWidth={1.5} vectorEffect="non-scaling-stroke" />}
          {line && (
            <>
              <polygon points={`0,${H} ${line} ${W},${H}`} fill={color} fillOpacity={0.12} />
              <polyline points={line} fill="none" stroke={color} strokeWidth={1.8} vectorEffect="non-scaling-stroke" strokeLinejoin="round" />
            </>
          )}
        </svg>
      </div>
      <span className="muted small tnum">
        Jetzt {now != null ? now.toFixed(1) : '–'} · Spitze (30 s) {peak != null ? peak.toFixed(1) : '–'} dBFS ·{' '}
        <span style={{ color: 'var(--accent)' }}>— Enter</span> <span style={{ color: 'var(--n400)' }}>— Exit</span>
      </span>

      {manual ? (
        <div style={{ display: 'flex', gap: 16, flexWrap: 'wrap', alignItems: 'flex-end' }}>
          <Stepper label="Enter" value={t.enter_db} unit="dB" step={0.5} min={-60} max={0} delayed onSet={(v) => save({ enter_db: v })} />
          <Stepper label="Exit" value={t.exit_db} unit="dB" step={0.5} min={-60} max={0} delayed onSet={(v) => save({ exit_db: v })} />
          <button
            className="btn btn-secondary"
            disabled={peak == null}
            title="Enter 4 dB unter die Spitze der letzten 30 s, Exit 9 dB darunter: einmal durchs Gate fliegen, dann tippen"
            onClick={() => peak != null && save({ enter_db: Math.round((peak - 4) * 2) / 2, exit_db: Math.round((peak - 9) * 2) / 2 })}
          >
            <Gauge size={16} /> Aus Spitze
          </button>
        </div>
      ) : (
        <span className="muted small">
          Dieses Gate läuft automatisch (gestrichelt: die gelernten Schwellen). Manuell stellst du es auf der Boards-Seite um.
        </span>
      )}
      {manual && t.exit_db >= t.enter_db && <span className="error small">Exit muss unter Enter liegen.</span>}
    </div>
  )
}
