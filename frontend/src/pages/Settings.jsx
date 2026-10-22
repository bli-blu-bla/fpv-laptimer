// @ts-check
// Settings that rarely change: which channels are scanned, the boards and
// their gates, the detection, the sound. (Slots and the race format: on the
// race control page.)

import { useEffect, useRef, useState } from 'react'
import { Cpu, Minus, Plus, SpeakerHigh, Sliders, Trash, Broadcast } from '@phosphor-icons/react'
import { saveSettings } from '../api.js'
import { RACEBAND, slotColor } from '../format.js'
import { Gauge } from '@phosphor-icons/react'

/** @param {{state: import('../types').State, live: import('../types').Live | null}} props */
export default function SettingsPage({ state, live }) {
  const s = state.settings
  const running = state.active != null
  const [error, setError] = useState('')
  /** @param {Partial<import('../types').Settings>} patch */
  const save = (patch) => {
    setError('')
    return saveSettings(s, patch).catch((e) => setError(e.message))
  }

  function addChannel() {
    const used = new Set(s.channels.map((c) => c.mhz))
    const mhz = RACEBAND.find((f) => !used.has(f)) ?? 5800
    const name = RACEBAND.includes(mhz) ? `R${RACEBAND.indexOf(mhz) + 1}` : String(mhz)
    save({ channels: [...s.channels, { name, mhz }], seats: [...s.channels.map((_, i) => s.seats[i] ?? defSeat()), defSeat()] })
  }
  /** @param {number} i */
  const removeChannel = (i) =>
    save({ channels: s.channels.filter((_, j) => j !== i), seats: s.channels.map((_, j) => s.seats[j] ?? defSeat()).filter((_, j) => j !== i) })
  /** @param {number} i @param {string} value  "R3" or MHz */
  function setChannel(i, value) {
    const mhz = value.startsWith('R') ? RACEBAND[Number(value.slice(1)) - 1] : Number(value)
    if (!mhz || mhz < 5300 || mhz > 6000) return
    const name = RACEBAND.includes(mhz) ? `R${RACEBAND.indexOf(mhz) + 1}` : String(mhz)
    save({ channels: s.channels.map((c, j) => (j === i ? { name, mhz } : c)) })
  }

  return (
    <div className="two-col" style={{ gridTemplateColumns: 'repeat(auto-fit, minmax(min(100%, 420px), 1fr))' }}>
      <div className="panel">
        <div className="panel-head">
          <Broadcast size={18} /> Kanäle
          <span className="end muted small">{s.channels.length} von 8</span>
        </div>
        <div className="panel-body" style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
          <span className="muted small">
            Alle Boards scannen alle Kanäle. Weniger oder nah beieinander liegende Kanäle messen öfter. Wer auf welchem Kanal fliegt, stellst du in
            der Live-Ansicht am Zahnrad ein.
          </span>
          {running && <span className="error small">Während einer Session lassen sich Kanäle nicht ändern.</span>}
          {s.channels.map((c, i) => (
            <div key={i} style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <span className="dot" style={{ background: slotColor(i) }} />
              <span style={{ width: 52 }}>Slot {i + 1}</span>
              <select
                className="input"
                style={{ width: 'auto', flex: 1 }}
                disabled={running}
                value={RACEBAND.includes(c.mhz) ? `R${RACEBAND.indexOf(c.mhz) + 1}` : 'x'}
                onChange={(e) => e.target.value !== 'x' && setChannel(i, e.target.value)}
              >
                {RACEBAND.map((f, k) => (
                  <option key={f} value={`R${k + 1}`}>
                    R{k + 1} · {f} MHz
                  </option>
                ))}
                {!RACEBAND.includes(c.mhz) && <option value="x">{c.mhz} MHz</option>}
              </select>
              <MhzInput value={c.mhz} disabled={running} onSet={(v) => setChannel(i, String(v))} />
              <button className="btn btn-ghost btn-icon" disabled={running || s.channels.length <= 1} onClick={() => removeChannel(i)} title="Kanal entfernen">
                <Trash size={16} />
              </button>
            </div>
          ))}
          {s.channels.length < 8 && (
            <button className="btn btn-secondary" disabled={running} onClick={addChannel}>
              <Plus size={16} /> Kanal hinzufügen
            </button>
          )}
        </div>
      </div>

      <div style={{ display: 'flex', flexDirection: 'column', gap: 14 }}>
        <div className="panel">
          <div className="panel-head">
            <Sliders size={18} /> Auswertung
          </div>
          <div className="panel-body" style={{ display: 'flex', flexWrap: 'wrap', gap: 20 }}>
            <Stepper label="Mindest-Rundenzeit" value={s.min_lap_s} unit="s" step={0.5} min={1} max={60} onSet={(v) => save({ min_lap_s: v })} />
            <Stepper label="Beste Runden in Folge" value={s.consecutive} step={1} min={2} max={20} onSet={(v) => save({ consecutive: v })} />
            <Stepper
              label="Verstärkung (Gain)"
              value={s.gain}
              step={1}
              min={0}
              max={60}
              hint="30 bei 25 mW; kleiner, wenn das Signal am Gate übersteuert"
              delayed
              onSet={(v) => save({ gain: v })}
            />
          </div>
        </div>
        <div className="panel">
          <div className="panel-head">
            <SpeakerHigh size={18} /> Ansagen
          </div>
          <div className="panel-body">
            <button className="row-toggle" onClick={() => save({ sound: !s.sound })}>
              <span className="text">
                <span>Ansagen und Töne</span>
                <span className="hint">Countdown, Startton, Rundenpiep, „Pilot N fertig“, „Neue Bestzeit“ – über den Lautsprecher am Server</span>
              </span>
              <span className={s.sound ? 'switch on' : 'switch'} />
            </button>
          </div>
        </div>
        <BoardsPanel state={state} live={live} save={save} />
        {error && <p className="error">{error}</p>}
      </div>
      <div style={{ gridColumn: '1 / -1' }}>
        <DetectionPanel state={state} live={live} save={save} />
      </div>
    </div>
  )
}

const defSeat = () => ({ pilot_id: null, label: '', enabled: true })

/**
 * − value + ; `delayed`: saves 0.8 s after the last tap (gain restarts the boards).
 * @param {{label: string, value: number, unit?: string, step: number, min: number, max: number, hint?: string, delayed?: boolean, onSet: (v: number) => void}} props
 */
function Stepper({ label, value, unit, step, min, max, hint, delayed, onSet }) {
  const [v, setV] = useState(value)
  const timer = useRef(/** @type {ReturnType<typeof setTimeout> | undefined} */ (undefined))
  useEffect(() => setV(value), [value])
  /** @param {number} d */
  function change(d) {
    const next = Math.round(Math.max(min, Math.min(max, v + d)) * 10) / 10
    setV(next)
    clearTimeout(timer.current)
    if (delayed) timer.current = setTimeout(() => onSet(next), 800)
    else onSet(next)
  }
  return (
    <div className="field">
      <span>{label}</span>
      <div className="stepper">
        <button className="btn btn-secondary btn-icon" onClick={() => change(-step)}>
          <Minus />
        </button>
        <span className="value">
          {v}
          {unit ? ` ${unit}` : ''}
        </span>
        <button className="btn btn-secondary btn-icon" onClick={() => change(step)}>
          <Plus />
        </button>
      </div>
      {hint && <span className="muted small" style={{ maxWidth: 260 }}>{hint}</span>}
    </div>
  )
}

/** Free frequency, saved on leaving the field. @param {{value: number, disabled: boolean, onSet: (v: number) => void}} props */
function MhzInput({ value, disabled, onSet }) {
  const [v, setV] = useState(String(value))
  useEffect(() => setV(String(value)), [value])
  return (
    <input
      className="input tnum"
      style={{ width: 84 }}
      inputMode="numeric"
      disabled={disabled}
      value={v}
      onChange={(e) => setV(e.target.value)}
      onBlur={() => Number(v) !== value && onSet(Number(v))}
      title="Frequenz in MHz"
    />
  )
}

/** Shown with the boards: the last 8 characters of the id (the MAC's end). @param {string} id */
const shortId = (id) => (id.length > 8 ? '…' + id.slice(-8) : id)

/** The bar spans this range, dBFS. */
const BAR_MIN = -64
const BAR_MAX = -5
/** @param {number | null | undefined} db */
const pct = (db) => (db == null ? 0 : Math.max(0, Math.min(100, ((db - BAR_MIN) / (BAR_MAX - BAR_MIN)) * 100)))

/**
 * The boards: which gate each watches, its live level (wave a quad in front of
 * one to find it), link health.
 * @param {{state: import('../types').State, live: import('../types').Live | null, save: (p: Partial<import('../types').Settings>) => unknown}} props
 */
function BoardsPanel({ state, live, save }) {
  const s = state.settings
  const boards = live?.boards ?? state.boards
  const finish = boards.filter((b) => b.role === 'finish' && !b.failed).length
  const arming = boards.some((b) => b.role === 'arming' && !b.failed)
  /** @param {string} id @param {import('../types').BoardRole} role */
  const setRole = (id, role) => save({ board_roles: { ...s.board_roles, [id]: role } })
  return (
    <div className="panel">
      <div className="panel-head">
        <Cpu size={18} /> Boards
      </div>
      {!boards.length && <div className="empty">Keine Boards gefunden – ESPs anstecken und den Server neu starten.</div>}
      {boards.map((b) => (
        <div key={b.id} className="thresh-row" style={{ padding: '12px 16px' }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap' }}>
            <span className={b.failed ? 'status-dot bad' : 'status-dot'} />
            <span style={{ fontWeight: 600 }} className="tnum" title={b.id}>
              {shortId(b.id)}
            </span>
            <span className="muted small" style={{ flex: 1, minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
              {b.port}
            </span>
            <div className="seg" style={{ flex: 'none' }}>
              {/** @type {const} */ ([
                ['finish', 'Ziel'],
                ['arming', 'Freigabe'],
                ['off', 'Aus'],
              ]).map(([r, label]) => (
                <button key={r} className={b.role === r ? 'on' : ''} onClick={() => setRole(b.id, r)}>
                  {label}
                </button>
              ))}
            </div>
          </div>
          <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
            <div className="level-bar">
              <div className="fill" style={{ width: pct(b.level_db) + '%' }} />
            </div>
            <span className="muted small tnum" style={{ width: 70, textAlign: 'right' }}>
              {b.level_db != null ? `${b.level_db.toFixed(1)} dBFS` : b.role === 'off' ? 'aus' : '–'}
            </span>
          </div>
          <span className="muted small">
            {b.levels_per_s.toFixed(0)} Messungen/s · {b.stats?.lost ?? 0} verloren
            {b.stats ? ` · Uhr ±${(b.stats.clock_uncertainty_us / 1000).toFixed(1)} ms` : ''}
          </span>
          {b.failed && <span className="error small">{b.failed}</span>}
        </div>
      ))}
      <div className="panel-body" style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
        <span className="muted small">Welches Board welches ist: mit dem Quad (Sender an) davor wedeln, der Balken schlägt aus.</span>
        {finish === 0 && boards.length > 0 && <span className="error small">Kein Board am Zielgate – es werden keine Runden gezählt.</span>}
        {finish === 1 && s.detection.mode === 'auto' && (
          <span className="small" style={{ color: 'var(--accent-300)' }}>
            Nur ein Board am Zielgate: Auto funktioniert, zwei Boards sind aber deutlich zuverlässiger.
          </span>
        )}
        {arming && (
          <span className="muted small">
            Freigabe-Gate aktiv: Ein Durchflug am Ziel zählt erst, wenn der Pilot seit dem letzten gezählten Durchflug durchs
            Freigabe-Gate geflogen ist (der erste nach dem Start zählt immer). Abgelehnte Durchflüge lassen sich in der
            Nachbearbeitung wiederherstellen.
          </span>
        )}
      </div>
    </div>
  )
}

/** Peak levels of the last 10 s, per channel, from the live messages.
 * @param {import('../types').ChannelLive[] | null | undefined} channels */
function usePeaks(channels) {
  const hist = useRef(/** @type {{t: number, db: number}[][]} */ ([]))
  const now = Date.now()
  channels?.forEach((c, i) => {
    const h = (hist.current[i] ??= [])
    if (c.level_db != null && (!h.length || h[h.length - 1].t !== now)) h.push({ t: now, db: c.level_db })
    while (h.length && h[0].t < now - 10_000) h.shift()
  })
  return hist.current.map((h) => (h.length ? Math.max(...h.map((x) => x.db)) : null))
}

/**
 * Auto or classic detection; in classic, Enter/Exit per channel (and gate),
 * with the live level and the peak of the last 10 s to find them.
 * @param {{state: import('../types').State, live: import('../types').Live | null, save: (p: Partial<import('../types').Settings>) => unknown}} props
 */
function DetectionPanel({ state, live, save }) {
  const s = state.settings
  const d = s.detection
  const finishPeaks = usePeaks(live?.channels)
  const armingPeaks = usePeaks(live?.arming)
  const hasArming = (live?.boards ?? state.boards).some((b) => b.role === 'arming')
  /** @param {'finish' | 'arming'} gate @param {number} i @param {Partial<import('../types').Thresholds>} part */
  function setThresh(gate, i, part) {
    const list = s.channels.map((_, j) => d[gate][j] ?? { enter_db: -25, exit_db: -30 })
    list[i] = { ...list[i], ...part }
    save({ detection: { ...d, [gate]: list } })
  }
  /** @param {'finish' | 'arming'} gate */
  const table = (gate) => {
    const peaks = gate === 'finish' ? finishPeaks : armingPeaks
    const chans = gate === 'finish' ? live?.channels : live?.arming
    return s.channels.map((c, i) => {
      const t = d[gate][i] ?? { enter_db: -25, exit_db: -30 }
      const now = chans?.[i]?.level_db
      const peak = peaks[i]
      return (
        <div key={gate + i} className="thresh-row wide" style={/** @type {React.CSSProperties} */ ({ '--lane': slotColor(i) })}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 10, flex: '1 1 260px' }}>
            <span className="dot" style={{ background: slotColor(i) }} />
            <span style={{ width: 40, fontWeight: 600 }}>{c.name}</span>
            <div className="level-bar">
              <div className="fill" style={{ width: pct(now) + '%' }} />
              {peak != null && <div className="mark peak" style={{ left: pct(peak) + '%' }} />}
              <div className="mark exit" style={{ left: pct(t.exit_db) + '%' }} />
              <div className="mark enter" style={{ left: pct(t.enter_db) + '%' }} />
            </div>
          </div>
          <span className="muted small tnum" style={{ flex: '0 1 230px' }}>
            Jetzt {now != null ? now.toFixed(1) : '–'} · Spitze (10 s) {peak != null ? peak.toFixed(1) : '–'} dBFS
            {chans?.[i]?.in_pass ? ' · im Gate' : ''}
          </span>
          <div style={{ display: 'flex', gap: 16, flexWrap: 'wrap', alignItems: 'flex-end' }}>
            <Stepper label="Enter" value={t.enter_db} unit="dB" step={0.5} min={-60} max={0} delayed onSet={(v) => setThresh(gate, i, { enter_db: v })} />
            <Stepper label="Exit" value={t.exit_db} unit="dB" step={0.5} min={-60} max={0} delayed onSet={(v) => setThresh(gate, i, { exit_db: v })} />
            <button
              className="btn btn-secondary"
              disabled={peak == null}
              title="Enter 4 dB unter die Spitze der letzten 10 s, Exit 5 dB darunter: einmal durchs Gate fliegen, dann tippen"
              onClick={() => peak != null && setThresh(gate, i, { enter_db: Math.round((peak - 4) * 2) / 2, exit_db: Math.round((peak - 9) * 2) / 2 })}
            >
              <Gauge size={16} /> Aus Spitze
            </button>
          </div>
        </div>
      )
    })
  }
  return (
    <div className="panel">
      <div className="panel-head">
        <Sliders size={18} /> Erkennung
      </div>
      <div className="panel-body" style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
        <div className="seg">
          <button className={d.mode === 'auto' ? 'on' : ''} onClick={() => save({ detection: { ...d, mode: 'auto' } })}>
            Auto
          </button>
          <button className={d.mode === 'classic' ? 'on' : ''} onClick={() => save({ detection: { ...d, mode: 'classic' } })}>
            Klassisch
          </button>
        </div>
        {d.mode === 'auto' ? (
          <span className="muted small">
            Die Schwellen lernt die Zeitmessung aus dem Flug: Kurspegel und die Gipfel der Durchflüge, pro Pilot. Nichts einzustellen; mit
            zwei Boards pro Gate am zuverlässigsten.
          </span>
        ) : (
          <>
            <span className="muted small">
              Feste Schwellen wie bei RotorHazard: Ein Durchflug beginnt über Enter und endet unter Exit. Einmal durchs Gate fliegen, dann
              „Aus Spitze“ tippen, oder von Hand einstellen. Bei mehreren Boards gilt der höhere Pegel.
            </span>
            <span className="kicker" style={{ marginTop: 8 }}>
              Zielgate
            </span>
            {table('finish')}
            {hasArming && (
              <>
                <span className="kicker" style={{ marginTop: 8 }}>
                  Freigabe-Gate
                </span>
                {table('arming')}
              </>
            )}
          </>
        )}
      </div>
    </div>
  )
}
