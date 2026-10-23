// @ts-check
// The detection of one slot, on race control (as RotorHazard's node panel):
// pick the gate, choose automatic or manual for this channel there, watch
// the signal of the last 30 s with the Enter and Exit lines, and in manual
// mode move them. In automatic mode the lines are the learned ones, and the
// minimum swing can be set for this channel. At the finish gate, what is set
// here can be kept as a preset of the pilot in this slot.

import { useEffect, useRef, useState } from 'react'
import { FloppyDiskIcon, GaugeIcon, PlusIcon, XIcon } from '@phosphor-icons/react'
import { api, saveSettings } from '../api.js'
import { channelMinRise, channelMode, presetChanged } from '../model.js'
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
  const [gate, setGate] = useState(/** @type {'finish' | 'arming'} */ ('finish'))
  const manual = channelMode(d, gate, index) === 'classic'
  const saved = d[gate][index] ?? DEFAULT
  // Shown at once; saved when the taps rest (Stepper `delayed`).
  const [t, setT] = useState(saved)
  /** The line being dragged in the graph, if any. */
  const drag = useRef(/** @type {'enter' | 'exit' | null} */ (null))
  useEffect(() => setT(saved), [saved.enter_db, saved.exit_db, saved.mode, saved.min_rise_db, gate]) // eslint-disable-line react-hooks/exhaustive-deps

  /** @param {Partial<import('../types').Thresholds>} part */
  function save(part) {
    // The rest as the server has it (a preset may have changed it meanwhile).
    const next = { ...saved, enter_db: t.enter_db, exit_db: t.exit_db, ...part }
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

  /** dBFS at a pointer's height in the graph, to 0.5 dB. @param {React.PointerEvent<HTMLDivElement>} e */
  function dbAt(e) {
    const r = e.currentTarget.getBoundingClientRect()
    const frac = 1 - (e.clientY - r.top) / r.height
    const db = MIN_DB + frac * (MAX_DB - MIN_DB)
    return Math.round(Math.max(MIN_DB, Math.min(MAX_DB, db)) * 2) / 2
  }
  /** Grab the Enter or Exit line near the pointer (within 14 px). @param {React.PointerEvent<HTMLDivElement>} e */
  function down(e) {
    if (!manual) return
    const r = e.currentTarget.getBoundingClientRect()
    const py = e.clientY - r.top
    /** @param {number} db */
    const px = (db) => (y(db) / H) * r.height
    const dEnter = Math.abs(px(t.enter_db) - py)
    const dExit = Math.abs(px(t.exit_db) - py)
    const which = dEnter <= dExit ? 'enter' : 'exit'
    if (Math.min(dEnter, dExit) > 14) return
    drag.current = which
    e.currentTarget.setPointerCapture(e.pointerId)
  }
  /** @param {React.PointerEvent<HTMLDivElement>} e */
  function move(e) {
    if (!drag.current) return
    const db = dbAt(e)
    // Exit stays at least 0.5 dB below Enter.
    setT((old) =>
      drag.current === 'enter'
        ? { ...old, enter_db: Math.max(db, old.exit_db + 0.5) }
        : { ...old, exit_db: Math.min(db, old.enter_db - 0.5) },
    )
  }
  function up() {
    if (!drag.current) return
    drag.current = null
    save({ enter_db: t.enter_db, exit_db: t.exit_db })
  }
  const line = points
    .map((p, k) => (p.level == null ? null : `${((k / (n - 1)) * W).toFixed(1)},${y(p.level).toFixed(1)}`))
    .filter(Boolean)
    .join(' ')

  return (
    <div className="panel" style={/** @type {React.CSSProperties} */ ({ '--lane': color, padding: 16, display: 'flex', flexDirection: 'column', gap: 12 })}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap' }}>
        <span className="dot" style={{ background: color }} />
        <span style={{ fontWeight: 600 }}>{callsign}</span>
        <span className="muted small">{s.channels[index]?.name} · Erkennung</span>
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
          <XIcon size={18} />
        </button>
      </div>

      <div className="seg">
        <button className={!manual ? 'on' : ''} onClick={() => save({ mode: 'auto' })}>
          Automatisch
        </button>
        <button className={manual ? 'on' : ''} onClick={() => save({ mode: 'classic' })}>
          Manuell (Enter/Exit)
        </button>
      </div>

      <div
        className={manual ? 'spark tune-graph manual' : 'spark tune-graph'}
        style={{ height: H }}
        onPointerDown={down}
        onPointerMove={move}
        onPointerUp={up}
        onPointerCancel={up}
      >
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
        {/* Labels in HTML: SVG text would be stretched with the graph. */}
        {enter != null && (
          <span className="line-label enter" style={{ top: `${(y(enter) / H) * 100}%` }}>
            Enter {enter.toFixed(1)}
          </span>
        )}
        {exit != null && (
          <span className="line-label exit" style={{ top: `${(y(exit) / H) * 100}%` }}>
            Exit {exit.toFixed(1)}
          </span>
        )}
      </div>
      <span className="muted small tnum">
        Jetzt {now != null ? now.toFixed(1) : '–'} · Spitze (30 s) {peak != null ? peak.toFixed(1) : '–'} dBFS
        {manual ? ' · Linien zum Verschieben ziehen' : ''}
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
            <GaugeIcon size={16} /> Aus Spitze
          </button>
        </div>
      ) : (
        <div style={{ display: 'flex', gap: 16, flexWrap: 'wrap', alignItems: 'flex-end' }}>
          <Stepper
            label="Mindestausschlag"
            value={channelMinRise(d, gate, index)}
            unit="dB"
            step={1}
            min={3}
            max={30}
            delayed
            onSet={(v) => save({ min_rise_db: v })}
          />
          {saved.min_rise_db != null && saved.min_rise_db !== d.min_rise_db && (
            <button className="btn btn-ghost" onClick={() => save({ min_rise_db: null })}>
              Standard ({d.min_rise_db} dB)
            </button>
          )}
          <span className="muted small" style={{ flex: '1 1 220px' }}>
            Die Schwellen werden aus dem Flug gelernt (gestrichelt: die aktuellen). Mindestausschlag höher, wenn Überflüge zählen; niedriger, wenn echte fehlen.
          </span>
        </div>
      )}
      {manual && t.exit_db >= t.enter_db && <span className="error small">Exit muss unter Enter liegen.</span>}
      {gate === 'finish' && <PresetRow state={state} index={index} />}
    </div>
  )
}

/**
 * The preset of the pilot in this slot: which one is applied, whether the
 * channel was tuned since (then save it into the preset), or save as a new
 * one. Nothing for free text or an empty slot (guests keep the channel's values).
 * @param {{state: import('../types').State, index: number}} props
 */
function PresetRow({ state, index }) {
  const s = state.settings
  const seat = s.seats[index]
  const pilotId = seat && !seat.label.trim() ? seat.pilot_id : null
  const [name, setName] = useState(/** @type {string | null} */ (null))
  const [error, setError] = useState('')
  if (pilotId == null) return null
  const preset = state.presets.find((p) => p.id === seat?.preset_id && p.pilot_id === pilotId)
  const changed = preset != null && presetChanged(s, index, preset)
  const pilot = state.pilots.find((p) => p.id === pilotId)?.name ?? ''

  /** @param {() => Promise<unknown>} f */
  async function run(f) {
    setError('')
    try {
      await f()
      setName(null)
    } catch (e) {
      setError(/** @type {Error} */ (e).message)
    }
  }

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 8, borderTop: '1px solid var(--n900)', paddingTop: 12 }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
        <span className="muted small">Preset</span>
        {preset ? <span className="tag tag-accent">{preset.name}</span> : <span className="muted small">keins</span>}
        {changed && <span className="small" style={{ color: 'var(--accent)' }}>geändert</span>}
        <span style={{ marginLeft: 'auto', display: 'flex', gap: 8, flexWrap: 'wrap', justifyContent: 'flex-end' }}>
          {changed && preset && (
            <button className="btn btn-secondary" style={{ whiteSpace: 'nowrap' }} onClick={() => run(() => api(`/presets/${preset.id}`, { method: 'PUT', body: { slot: index } }))}>
              <FloppyDiskIcon size={16} /> In „{preset.name}“ speichern
            </button>
          )}
          {name == null && (
            <button className="btn btn-ghost" style={{ whiteSpace: 'nowrap' }} onClick={() => setName('')}>
              <PlusIcon size={16} /> Neues Preset
            </button>
          )}
        </span>
      </div>
      {name != null && (
        <form
          style={{ display: 'flex', gap: 8 }}
          onSubmit={(e) => {
            e.preventDefault()
            run(() => api('/presets', { body: { pilot_id: pilotId, name, slot: index } }))
          }}
        >
          <input className="input" autoFocus value={name} placeholder={`Name für ${pilot}, z. B. die Drohne`} onChange={(e) => setName(e.target.value)} />
          <button type="submit" className="btn btn-primary" disabled={!name.trim()}>
            Speichern
          </button>
          <button type="button" className="btn btn-ghost" onClick={() => setName(null)}>
            Abbrechen
          </button>
        </form>
      )}
      {preset && preset.gain !== s.gain && channelMode(s.detection, 'finish', index) === 'classic' && (
        <span className="muted small">
          Gespeichert bei Gain {preset.gain}, jetzt {s.gain}: die festen Schwellen passen evtl. nicht mehr.
        </span>
      )}
      {error && <span className="error small">{error}</span>}
    </div>
  )
}
