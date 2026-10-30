// @ts-check
// Board debugging (Boards → Debug): per channel a graph with every board's
// envelope, the boards of each gate combined (as the laps are counted), the
// thresholds and the passes each of them would detect on its own; below, a
// table that lines those passes up. Live (the last minutes, polled from
// /api/debug/live), or a level file: a recording made here, or a session's.
// Server side: `server/src/debug.rs`.

import {
  CaretLeftIcon,
  CaretRightIcon,
  DownloadSimpleIcon,
  MagnifyingGlassMinusIcon,
  MagnifyingGlassPlusIcon,
  PauseIcon,
  PlayIcon,
  RecordIcon,
  StopIcon,
  TrashIcon,
} from '@phosphor-icons/react'
import { useEffect, useRef, useState } from 'react'
import { api } from '../api.js'
import LevelFileList from './LevelFileList.jsx'
import { recLabel } from '../format.js'

/**
 * @typedef {object} DebugBoard
 * @property {number} index
 * @property {string} id
 * @property {string | null} name
 * @property {import('../types').BoardRole} role
 *
 * @typedef {object} DebugPass   a pass one detector reported
 * @property {number} series   0 finish gate combined, 1 arming gate combined, 2 + i board i
 * @property {number} channel
 * @property {number} time_us
 * @property {number} peak_db
 * @property {boolean} retracted
 *
 * @typedef {object} Line   one series of one channel, dBFS (null: nothing)
 * @property {number[]} t
 * @property {(number | null)[]} level
 * @property {(number | null)[]} enter
 * @property {(number | null)[]} exit
 *
 * @typedef {object} View   what the graphs show (the chips above them)
 * @property {number[]} channels
 * @property {number[]} hidden   series switched off
 * @property {boolean} thresholds
 * @property {boolean} passes
 */

const HEIGHT = 230
const PAD_LEFT = 34
const PAD_TOP = 26
const PAD_BOTTOM = 18
const MIN_DB = -62
/** Live: what the page keeps (pause and look back), µs. */
const KEEP_US = 180e6
const SLOT_US = 10_000
/** Passes of different series this close count as the same crossing (table). */
const SAME_PASS_US = 1e6

const BOARD_COLORS = ['#4cc9f0', '#b388ff', '#7ae582', '#ff6b9a', '#5eead4', '#f4a261']

/** The end of the id (the MAC's last three bytes). @param {string} id */
const shortId = (id) => (id.length > 8 ? '…' + id.slice(-8) : id)
/** Level code (0.25 dB, 0 = none) → dBFS. @param {number} c */
const codeDb = (c) => (c > 0 ? c / 4 - 63.75 : null)
/** µs → "1,25 s" with sign. @param {number} us */
const offset = (us) => (us >= 0 ? '+' : '−') + (Math.abs(us) / 1e6).toFixed(2).replace('.', ',')

/** Name and colour of a series. @param {number} s @param {DebugBoard[]} boards */
export function seriesInfo(s, boards) {
  if (s === 0) return { label: 'Ziel kombiniert', color: 'var(--text)' }
  if (s === 1) return { label: 'Freigabe kombiniert', color: '#ffd166' }
  const b = boards.find((x) => x.index === s - 2)
  const name = b ? (b.name ?? shortId(b.id)) : `Board ${s - 1}`
  const role = b?.role === 'arming' ? ' (Freigabe)' : b?.role === 'off' ? ' (aus)' : ''
  return { label: name + role, color: BOARD_COLORS[(s - 2) % BOARD_COLORS.length] }
}

/** @param {{state: import('../types').State}} props */
export default function BoardDebug({ state }) {
  const [mode, setMode] = useState(/** @type {'live' | 'files'} */ ('live'))
  const [source, setSource] = useState(/** @type {{key: string, label: string} | null} */ (null))
  const [view, setView] = useState(/** @type {View} */ ({ channels: state.settings.channels.map((_, i) => i), hidden: [], thresholds: true, passes: true }))

  /** @param {{key: string, label: string}} s */
  function open(s) {
    setSource(s)
    setMode('files')
  }

  return (
    <div className="panel">
      <div className="panel-head">
        <div className="seg" style={{ flex: 'none' }}>
          <button className={mode === 'live' ? 'on' : ''} onClick={() => setMode('live')}>
            Live
          </button>
          <button className={mode === 'files' ? 'on' : ''} onClick={() => setMode('files')}>
            Aufnahmen
          </button>
        </div>
        <span className="end muted small">Jedes Board für sich: Pegel, Schwellen und die Durchflüge, die es allein erkennen würde.</span>
      </div>
      <div className="panel-body debug">
        {mode === 'live' ? (
          <LiveView state={state} view={view} setView={setView} onOpen={open} />
        ) : source ? (
          <FileView source={source} view={view} setView={setView} onClose={() => setSource(null)} />
        ) : (
          <FileList onOpen={open} />
        )}
      </div>
    </div>
  )
}

// --- live ---

/**
 * @param {object} props
 * @param {import('../types').State} props.state
 * @param {View} props.view
 * @param {(v: View) => void} props.setView
 * @param {(s: {key: string, label: string}) => void} props.onOpen
 */
function LiveView({ state, view, setView, onOpen }) {
  const live = useLiveDebug(view.channels)
  const [windowS, setWindowS] = useState(30)
  /** Paused: the end of the visible range stays here. */
  const [pausedAt, setPausedAt] = useState(/** @type {number | null} */ (null))
  const [busy, setBusy] = useState(false)
  const [note, setNote] = useState('')
  const [error, setError] = useState('')
  const rec = live.recording

  const to = pausedAt ?? live.now
  const from = to - windowS * 1e6
  /** Paused: move the range, not past the newest data. @param {number} dt */
  const pan = (dt) => setPausedAt((p) => Math.min(live.now, Math.max(live.now - KEEP_US + windowS * 1e6, (p ?? live.now) + dt)))

  async function record() {
    setBusy(true)
    setError('')
    try {
      if (rec) {
        const r = await api('/debug/record/stop', { body: {} })
        setNote(r.name)
      } else {
        await api('/debug/record', { body: {} })
        setNote('')
      }
    } catch (e) {
      setError(/** @type {Error} */ (e).message)
    } finally {
      setBusy(false)
    }
  }

  const series = [...new Set([...live.lines.keys()].map((k) => Number(k.split(':')[0])))].sort((a, b) => a - b)
  return (
    <>
      <div className="debug-tools">
        <button className={rec ? 'btn btn-danger' : 'btn btn-secondary'} disabled={busy} onClick={record}>
          {rec ? <StopIcon size={16} weight="fill" /> : <RecordIcon size={16} weight="fill" style={{ color: 'var(--bad)' }} />}
          {rec ? `Aufnahme stoppen · ${fmtDuration(live.now - rec.started_us)}` : 'Aufnehmen'}
        </button>
        <div className="seg">
          {[10, 30, 60].map((s) => (
            <button key={s} className={windowS === s ? 'on' : ''} onClick={() => setWindowS(s)}>
              {s} s
            </button>
          ))}
        </div>
        {pausedAt == null ? (
          <button className="btn btn-secondary" onClick={() => setPausedAt(live.now)}>
            <PauseIcon size={16} /> Anhalten
          </button>
        ) : (
          <>
            <button className="btn btn-secondary btn-icon" title="Zurück" onClick={() => pan((-windowS * 1e6) / 2)}>
              <CaretLeftIcon />
            </button>
            <button className="btn btn-secondary btn-icon" title="Weiter" onClick={() => pan((windowS * 1e6) / 2)}>
              <CaretRightIcon />
            </button>
            <button className="btn btn-primary" onClick={() => setPausedAt(null)}>
              <PlayIcon size={16} /> Weiter
            </button>
          </>
        )}
      </div>
      {note && (
        <div className="muted small">
          Gespeichert als {note}.{' '}
          <button className="btn btn-ghost" onClick={() => onOpen({ key: 'rec:' + note, label: note })}>
            Ansehen
          </button>
        </div>
      )}
      {(error || live.error) && <div className="error small">{error || live.error}</div>}
      <Chips
        channels={state.settings.channels.map((c) => `${c.name} · ${c.mhz}`)}
        series={series}
        boards={live.boards}
        view={view}
        setView={setView}
      />
      {!state.boards.length && <div className="empty">Keine Boards angesteckt.</div>}
      {view.channels.map((c) => {
        const ch = state.settings.channels[c]
        if (!ch) return null
        const lines = series
          .filter((s) => !view.hidden.includes(s))
          .map((s) => ({ s, d: live.lines.get(`${s}:${c}`) }))
          .filter((x) => x.d != null)
        const passes = live.passes.filter((p) => p.channel === c)
        return (
          <ChannelBlock
            key={c}
            title={`${ch.name} · ${ch.mhz} MHz`}
            boards={live.boards}
            lines={/** @type {{s: number, d: Line}[]} */ (lines)}
            passes={passes}
            from={from}
            to={to}
            view={view}
            tick={(t) => `${Math.round((t - to) / 1e6)} s`}
            onPan={pausedAt != null ? pan : undefined}
          />
        )
      })}
    </>
  )
}

/**
 * Polls /api/debug/live: per "series:channel" the levels of the last
 * minutes, and the passes. Starts over when the channels change.
 * @param {number[]} channels
 */
function useLiveDebug(channels) {
  const [data, setData] = useState({
    now: 0,
    /** @type {Map<string, Line>} */ lines: new Map(),
    /** @type {DebugPass[]} */ passes: [],
    /** @type {DebugBoard[]} */ boards: [],
    /** @type {{name: string, started_us: number, levels: number} | null} */ recording: null,
    error: '',
  })
  const key = channels.join(',')
  useEffect(() => {
    /** @type {Map<string, Line>} */
    const lines = new Map()
    /** @type {Map<string, DebugPass>} */
    const passes = new Map()
    let stop = false
    /** @type {ReturnType<typeof setTimeout> | undefined} */
    let timer
    async function poll() {
      try {
        // From where the series that is furthest behind ends (the others skip
        // what they already have).
        const ends = [...lines.values()].map((d) => d.t[d.t.length - 1])
        const after = ends.length ? Math.min(...ends) : null
        const r = await api(`/debug/live?window_s=60&channels=${key}${after != null ? `&after_us=${after}` : ''}`)
        if (stop) return
        for (const s of r.series) {
          for (const c of s.channels) {
            const k = `${s.series}:${c.channel}`
            const d = lines.get(k) ?? { t: [], level: [], enter: [], exit: [] }
            const last = d.t.length ? d.t[d.t.length - 1] : -Infinity
            for (let i = 0; i < c.level.length; i++) {
              const t = c.t0 + i * SLOT_US
              if (t <= last) continue
              d.t.push(t)
              d.level.push(codeDb(c.level[i]))
              d.enter.push(codeDb(c.enter[i]))
              d.exit.push(codeDb(c.exit[i]))
            }
            // Drop what is older than KEEP_US.
            const cut = lowerBound(d.t, r.now_us - KEEP_US)
            if (cut > 0) for (const a of [d.t, d.level, d.enter, d.exit]) a.splice(0, cut)
            lines.set(k, d)
          }
        }
        for (const p of r.passes) passes.set(`${p.series}:${p.channel}:${p.time_us}`, p)
        for (const [k, p] of passes) if (p.time_us < r.now_us - KEEP_US) passes.delete(k)
        setData({ now: r.now_us, lines, passes: [...passes.values()], boards: r.boards, recording: r.recording, error: '' })
      } catch (e) {
        setData((d) => ({ ...d, error: /** @type {Error} */ (e).message }))
      }
      if (!stop) timer = setTimeout(poll, 300)
    }
    poll()
    return () => {
      stop = true
      clearTimeout(timer)
    }
  }, [key])
  return data
}

// --- files ---

/** Recordings and sessions to open; recordings can also be downloaded and deleted.
 * @param {{onOpen: (s: {key: string, label: string}) => void}} props */
function FileList({ onOpen }) {
  const [confirm, setConfirm] = useState('')
  const [error, setError] = useState('')

  /** @param {string} name @param {() => void} reload */
  async function remove(name, reload) {
    if (confirm !== name) return setConfirm(name)
    setConfirm('')
    try {
      await api(`/debug/recordings/${encodeURIComponent(name)}`, { method: 'DELETE' })
      reload()
    } catch (e) {
      setError(/** @type {Error} */ (e).message)
    }
  }

  return (
    <>
      {error && <div className="error small">{error}</div>}
      <LevelFileList
        action="Öffnen"
        withRunning
        onPick={(key, label) => onOpen({ key, label: key.startsWith('rec:') ? recLabel(label) : label })}
        recActions={(r, reload) => (
          <>
            <a className="btn btn-ghost btn-icon" title="Herunterladen (CSV, auch für laptimer replay)" href={`/api/debug/recordings/${encodeURIComponent(r.name)}`} download>
              <DownloadSimpleIcon size={16} />
            </a>
            <button
              className={confirm === r.name ? 'btn btn-danger' : 'btn btn-ghost btn-icon'}
              disabled={r.recording}
              title="Löschen"
              onClick={() => remove(r.name, reload)}
              onBlur={() => setConfirm('')}
            >
              <TrashIcon size={16} />
              {confirm === r.name && ' Löschen?'}
            </button>
          </>
        )}
      />
    </>
  )
}

/**
 * A level file worked through (`/api/debug/graph`): all channels share the
 * visible range; drag or the buttons move it.
 * @param {object} props
 * @param {{key: string, label: string}} props.source
 * @param {View} props.view
 * @param {(v: View) => void} props.setView
 * @param {() => void} props.onClose
 */
function FileView({ source, view, setView, onClose }) {
  const box = useRef(/** @type {HTMLDivElement | null} */ (null))
  const width = useWidth(box)
  const [info, setInfo] = useState(/** @type {{start: number, end: number, boards: DebugBoard[], channels: string[]} | null} */ (null))
  const [range, setRange] = useState(/** @type {[number, number] | null} */ (null))
  /** Per channel: the lines of the range, and all passes. */
  const [graphs, setGraphs] = useState(/** @type {Map<number, {lines: {s: number, d: Line}[], passes: DebugPass[], all: DebugPass[]}>} */ (new Map()))
  const [error, setError] = useState('')
  const [loading, setLoading] = useState(true)

  // The first request gives the length and the channels; it may take a while.
  useEffect(() => {
    setInfo(null)
    setRange(null)
    setGraphs(new Map())
    setLoading(true)
    setError('')
    api(`/debug/graph?source=${encodeURIComponent(source.key)}&channel=0&points=10`)
      .then((r) => {
        setInfo({ start: r.start_us, end: r.end_us, boards: r.boards, channels: r.channels })
        setRange([r.start_us, r.end_us])
        setView({ ...view, channels: view.channels.filter((c) => c < r.channels.length).length ? view.channels.filter((c) => c < r.channels.length) : r.channels.map((/** @type {string} */ _, /** @type {number} */ i) => i) })
      })
      .catch((e) => setError(e.message))
      .finally(() => setLoading(false))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [source.key])

  const chKey = view.channels.join(',')
  useEffect(() => {
    if (!range) return
    const id = setTimeout(() => {
      // The server takes whole µs: computed ranges have fractions.
      const [a, b] = range.map(Math.round)
      Promise.all(
        view.channels.map((c) =>
          api(`/debug/graph?source=${encodeURIComponent(source.key)}&channel=${c}&from_us=${a}&to_us=${b}&points=${Math.round(width - PAD_LEFT)}`).then((r) => {
            const lines = r.series.map((/** @type {{series: number, level: (number|null)[], enter: (number|null)[], exit: (number|null)[]}} */ s) => ({
              s: s.series,
              d: { t: r.time_us, level: s.level, enter: s.enter, exit: s.exit },
            }))
            return /** @type {const} */ ([c, { lines, passes: r.passes, all: r.all_passes }])
          }),
        ),
      )
        .then((list) => setGraphs(new Map(list)))
        .catch((e) => setError(e.message))
    }, 80) // while dragging, ask once the range rests
    return () => clearTimeout(id)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [source.key, range, width, chKey])

  const series = [...new Set([...graphs.values()].flatMap((g) => g.lines.map((l) => l.s)))].sort((a, b) => a - b)
  /** @param {number} a @param {number} b @returns {[number, number]} */
  function clamp(a, b) {
    if (!info) return [a, b]
    const len = Math.max(1e6, Math.min(b - a, info.end - info.start))
    const s = Math.max(info.start, Math.min(a, info.end - len))
    return [s, s + len]
  }
  /** @param {number} f */
  const zoom = (f) => range && setRange(clamp((range[0] + range[1]) / 2 - ((range[1] - range[0]) * f) / 2, (range[0] + range[1]) / 2 + ((range[1] - range[0]) * f) / 2))
  /** @param {number} dt */
  const pan = (dt) => range && setRange(clamp(range[0] + dt, range[1] + dt))

  return (
    <div ref={box} className="debug">
      <div className="debug-tools">
        <button className="btn btn-ghost" onClick={onClose}>
          <CaretLeftIcon size={16} /> Alle Aufnahmen
        </button>
        <span style={{ fontWeight: 600, flex: 1, minWidth: 0 }}>{source.label}</span>
        {range && info && (
          <>
            <button className="btn btn-secondary btn-icon" title="Zurück" onClick={() => pan(-(range[1] - range[0]) / 2)}>
              <CaretLeftIcon />
            </button>
            <button className="btn btn-secondary btn-icon" title="Weniger Zoom" onClick={() => zoom(2)}>
              <MagnifyingGlassMinusIcon />
            </button>
            <button className="btn btn-secondary btn-icon" title="Mehr Zoom" onClick={() => zoom(0.5)}>
              <MagnifyingGlassPlusIcon />
            </button>
            <button className="btn btn-secondary btn-icon" title="Weiter" onClick={() => pan((range[1] - range[0]) / 2)}>
              <CaretRightIcon />
            </button>
            <button className="btn btn-secondary" onClick={() => setRange([info.start, info.end])}>
              Alles
            </button>
            <span className="muted small tnum">
              {((range[0] - info.start) / 1e6).toFixed(1)}–{((range[1] - info.start) / 1e6).toFixed(1)} s
            </span>
          </>
        )}
      </div>
      {error && <div className="error small">{error}</div>}
      {loading && <div className="muted small">Werte die Datei aus … (eine lange Session dauert auf dem Pi ein paar Sekunden)</div>}
      {info && (
        <>
          <div className="muted small">
            Neu ausgewertet mit frischen Detektoren: im automatischen Modus braucht die Schwelle anfangs ein paar Durchflüge zum Anlernen,
            in einer kurzen Aufnahme erscheinen deshalb evtl. keine.
          </div>
          <Chips channels={info.channels} series={series} boards={info.boards} view={view} setView={setView} />
        </>
      )}
      {info &&
        range &&
        view.channels.map((c) => {
          const g = graphs.get(c)
          if (!g) return null
          return (
            <ChannelBlock
              key={c}
              title={info.channels[c] ?? `Kanal ${c + 1}`}
              boards={info.boards}
              lines={g.lines.filter((l) => !view.hidden.includes(l.s))}
              passes={g.passes}
              allPasses={g.all}
              from={range[0]}
              to={range[1]}
              view={view}
              tick={(t) => `${Math.round((t - info.start) / 1e6)} s`}
              onPan={pan}
            />
          )
        })}
    </div>
  )
}

// --- shared ---

/**
 * Which channels and series are shown; thresholds and passes on/off.
 * @param {{channels: string[], series: number[], boards: DebugBoard[], view: View, setView: (v: View) => void}} props
 */
function Chips({ channels, series, boards, view, setView }) {
  /** @template T @param {T[]} list @param {T} x */
  const toggle = (list, x) => (list.includes(x) ? list.filter((y) => y !== x) : [...list, x])
  return (
    <div className="debug-chips">
      <div className="chip-row">
        {channels.map((c, i) => (
          <button
            key={i}
            className={view.channels.includes(i) ? 'chip-btn on' : 'chip-btn'}
            onClick={() => setView({ ...view, channels: toggle(view.channels, i).sort((a, b) => a - b) })}
          >
            {c}
          </button>
        ))}
      </div>
      <div className="chip-row">
        {series.map((s) => {
          const { label, color } = seriesInfo(s, boards)
          const on = !view.hidden.includes(s)
          return (
            <button key={s} className={on ? 'chip-btn on series' : 'chip-btn series'} onClick={() => setView({ ...view, hidden: toggle(view.hidden, s) })}>
              <span className="dot" style={{ background: on ? color : 'transparent', borderColor: color }} />
              {label}
            </button>
          )
        })}
        <span style={{ flex: 1 }} />
        <button className={view.thresholds ? 'chip-btn on' : 'chip-btn'} onClick={() => setView({ ...view, thresholds: !view.thresholds })}>
          Schwellen
        </button>
        <button className={view.passes ? 'chip-btn on' : 'chip-btn'} onClick={() => setView({ ...view, passes: !view.passes })}>
          Durchflüge
        </button>
      </div>
    </div>
  )
}

/**
 * One channel: its graph and the table of passes.
 * @param {object} props
 * @param {string} props.title
 * @param {DebugBoard[]} props.boards
 * @param {{s: number, d: Line}[]} props.lines   the visible series
 * @param {DebugPass[]} props.passes   in or near the range
 * @param {DebugPass[]} [props.allPasses]   for the table (default: `passes`)
 * @param {number} props.from
 * @param {number} props.to
 * @param {View} props.view
 * @param {(t: number) => string} props.tick
 * @param {(dt: number) => void} [props.onPan]
 */
function ChannelBlock({ title, boards, lines, passes, allPasses, from, to, view, tick, onPan }) {
  const shown = lines.map((l) => l.s)
  const visiblePasses = view.passes ? passes.filter((p) => shown.includes(p.series)) : []
  const counts = shown.map((s) => ({ s, n: (allPasses ?? passes).filter((p) => p.series === s && !p.retracted).length }))
  return (
    <div className="debug-channel">
      <div className="debug-channel-head">
        <span style={{ fontWeight: 600 }}>{title}</span>
        <span className="muted small">
          {counts.map(({ s, n }, i) => (
            <span key={s}>
              {i > 0 && ' · '}
              <span style={{ color: seriesInfo(s, boards).color }}>{seriesInfo(s, boards).label}</span> {n}
            </span>
          ))}
          {counts.length > 0 && ' Durchflüge'}
        </span>
      </div>
      <DebugGraph boards={boards} lines={lines} passes={visiblePasses} from={from} to={to} thresholds={view.thresholds} tick={tick} onPan={onPan} />
      {view.passes && <PassTable boards={boards} series={shown} passes={(allPasses ?? passes).filter((p) => shown.includes(p.series))} />}
    </div>
  )
}

/**
 * The graph of one channel: a line per series, dashed thresholds in the
 * series' colour, a mark per pass (dot row = series).
 * @param {object} props
 * @param {DebugBoard[]} props.boards
 * @param {{s: number, d: Line}[]} props.lines
 * @param {DebugPass[]} props.passes
 * @param {number} props.from
 * @param {number} props.to
 * @param {boolean} props.thresholds
 * @param {(t: number) => string} props.tick
 * @param {(dt: number) => void} [props.onPan]
 */
function DebugGraph({ boards, lines, passes, from, to, thresholds, tick, onPan }) {
  const box = useRef(/** @type {HTMLDivElement | null} */ (null))
  const width = useWidth(box)
  const drag = useRef(/** @type {{x: number} | null} */ (null))
  const span = Math.max(1, to - from)
  const plotW = width - PAD_LEFT
  const plotH = HEIGHT - PAD_BOTTOM
  const n = Math.max(10, Math.round(plotW / 2))
  // The gates (series 0, 1) last: on top of the boards.
  const drawn = lines
    .map(({ s, d }) => ({ s, b: bucket(d, from, to, n) }))
    .sort((a, b) => (a.s < 2 ? 1 : 0) - (b.s < 2 ? 1 : 0))
  let top = -20
  for (const { b } of drawn) for (const v of b.level) if (v != null && v + 3 > top) top = v + 3
  /** @param {number} t */
  const x = (t) => PAD_LEFT + ((t - from) / span) * plotW
  /** @param {number} db */
  const y = (db) => PAD_TOP + (plotH - PAD_TOP) - ((db - MIN_DB) / (top - MIN_DB)) * (plotH - PAD_TOP)

  const step = [1, 2, 5, 10, 20, 30, 60, 120, 300, 600].find((s) => (s * 1e6 * plotW) / span >= 70) ?? 1200
  const ticks = []
  for (let t = Math.ceil(from / 1e6 / step) * step * 1e6; t <= to; t += step * 1e6) ticks.push(t)
  const rows = lines.map((l) => l.s)

  return (
    <div className="graph" ref={box}>
      <svg
        width="100%"
        viewBox={`0 0 ${width} ${HEIGHT}`}
        className="detail"
        style={{ cursor: onPan ? 'grab' : 'default' }}
        onPointerDown={(e) => {
          if (!onPan) return
          e.currentTarget.setPointerCapture(e.pointerId)
          drag.current = { x: e.clientX }
        }}
        onPointerMove={(e) => {
          if (!drag.current || !onPan) return
          const r = e.currentTarget.getBoundingClientRect()
          const dx = ((e.clientX - drag.current.x) / r.width) * width
          if (Math.abs(dx) < 3) return
          drag.current = { x: e.clientX }
          onPan((-dx / plotW) * span)
        }}
        onPointerUp={() => (drag.current = null)}
      >
        {[-60, -50, -40, -30, -20, -10, 0]
          .filter((d) => d < top)
          .map((d) => (
            <g key={d}>
              <line x1={PAD_LEFT} x2={width} y1={y(d)} y2={y(d)} className="grid" />
              <text x={PAD_LEFT - 4} y={y(d) + 4} className="axis" textAnchor="end">
                {d}
              </text>
            </g>
          ))}
        {ticks.map((t) => (
          <text key={t} x={x(t)} y={HEIGHT - 4} className="axis" textAnchor="middle">
            {tick(t)}
          </text>
        ))}
        {drawn.map(({ s, b }) => {
          const color = seriesInfo(s, boards).color
          return (
            <g key={s}>
              {thresholds && (
                <>
                  <path d={path(b.t, b.enter, x, y)} className="debug-threshold" style={{ stroke: color }} />
                  <path d={path(b.t, b.exit, x, y)} className="debug-threshold exit" style={{ stroke: color }} />
                </>
              )}
              <path d={path(b.t, b.level, x, y)} className={s < 2 ? 'debug-line gate' : 'debug-line'} style={{ stroke: color }} />
            </g>
          )
        })}
        {passes
          .filter((p) => p.time_us >= from && p.time_us <= to)
          .map((p) => {
            const color = seriesInfo(p.series, boards).color
            const cy = 8 + Math.max(0, rows.indexOf(p.series)) * 8
            return (
              <g key={`${p.series}:${p.time_us}`} className={p.retracted ? 'debug-pass retracted' : 'debug-pass'}>
                <title>
                  {seriesInfo(p.series, boards).label}: {p.peak_db.toFixed(1)} dBFS{p.retracted ? ' (zurückgenommen)' : ''}
                </title>
                <line x1={x(p.time_us)} x2={x(p.time_us)} y1={cy} y2={plotH} style={{ stroke: color }} />
                <circle cx={x(p.time_us)} cy={cy} r={3.5} style={{ fill: p.retracted ? 'var(--surface-2)' : color, stroke: color }} />
              </g>
            )
          })}
      </svg>
    </div>
  )
}

/**
 * The passes of all shown series lined up: one row per crossing (passes
 * within 1 s), per series a tick with its time against the first series
 * that saw it, newest first.
 * @param {{boards: DebugBoard[], series: number[], passes: DebugPass[]}} props
 */
function PassTable({ boards, series, passes }) {
  if (!series.length) return null
  const sorted = [...passes].sort((a, b) => a.time_us - b.time_us)
  /** @type {{t: number, items: DebugPass[]}[]} */
  const groups = []
  for (const p of sorted) {
    const g = groups[groups.length - 1]
    if (g && p.time_us - g.t < SAME_PASS_US) g.items.push(p)
    else groups.push({ t: p.time_us, items: [p] })
  }
  if (!groups.length) return <div className="muted small" style={{ padding: '4px 0 8px' }}>Keine Durchflüge erkannt.</div>
  const shown = groups.reverse().slice(0, 50)
  return (
    <div className="debug-table">
      <table className="table">
        <thead>
          <tr>
            <th>Zeit</th>
            {series.map((s) => (
              <th key={s} style={{ color: seriesInfo(s, boards).color }}>
                {seriesInfo(s, boards).label}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {shown.map((g) => {
            // The reference: the combined finish gate if it saw it, else the first.
            const ref = g.items.find((p) => p.series === 0 && !p.retracted) ?? g.items[0]
            return (
              <tr key={g.t}>
                <td className="muted">{new Date(ref.time_us / 1000).toLocaleTimeString('de-DE')}</td>
                {series.map((s) => {
                  const p = g.items.find((q) => q.series === s)
                  if (!p) return <td key={s} className="muted">–</td>
                  return (
                    <td key={s} className={p.retracted ? 'muted' : ''} title={`${p.peak_db.toFixed(1)} dBFS`}>
                      {p.retracted ? 'zurückgenommen' : p === ref ? `✓ ${p.peak_db.toFixed(0)} dB` : `✓ ${offset(p.time_us - ref.time_us)} s`}
                    </td>
                  )
                })}
              </tr>
            )
          })}
        </tbody>
      </table>
      {groups.length > shown.length && <div className="muted small">… und {groups.length - shown.length} ältere</div>}
    </div>
  )
}

/**
 * The range `from`..`to` in `n` parts: per part the highest level, with
 * the thresholds at that point (no peak gets lost).
 * @param {Line} d @param {number} from @param {number} to @param {number} n
 * @returns {Line}
 */
function bucket(d, from, to, n) {
  /** @type {Line} */
  const out = { t: [], level: [], enter: [], exit: [] }
  const per = (to - from) / n
  let k = -1
  for (let i = lowerBound(d.t, from); i < d.t.length && d.t[i] <= to; i++) {
    const j = Math.min(n - 1, Math.floor((d.t[i] - from) / per))
    const v = d.level[i]
    if (j !== k) {
      out.t.push(d.t[i])
      out.level.push(v)
      out.enter.push(d.enter[i])
      out.exit.push(d.exit[i])
      k = j
    } else if (v != null) {
      const last = out.level.length - 1
      const cur = out.level[last]
      if (cur == null || v > cur) {
        out.level[last] = v
        out.t[last] = d.t[i]
        out.enter[last] = d.enter[i]
        out.exit[last] = d.exit[i]
      }
    }
  }
  return out
}

/** First index with `a[i] >= v` (a sorted). @param {number[]} a @param {number} v */
function lowerBound(a, v) {
  let lo = 0
  let hi = a.length
  while (lo < hi) {
    const mid = (lo + hi) >> 1
    if (a[mid] < v) lo = mid + 1
    else hi = mid
  }
  return lo
}

/**
 * An SVG path through the points; gaps (null) break the line.
 * @param {number[]} ts @param {(number | null)[]} vs
 * @param {(t: number) => number} x @param {(v: number) => number} y
 */
function path(ts, vs, x, y) {
  let d = ''
  let pen = false
  for (let i = 0; i < ts.length; i++) {
    const v = vs[i]
    if (v == null) {
      pen = false
      continue
    }
    d += `${pen ? 'L' : 'M'}${x(ts[i]).toFixed(1)},${y(v).toFixed(1)}`
    pen = true
  }
  return d
}

/** µs → "1:05". @param {number} us */
function fmtDuration(us) {
  const s = Math.max(0, Math.floor(us / 1e6))
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`
}

/** The element's width in pixels, kept current. @param {{current: HTMLElement | null}} ref */
function useWidth(ref) {
  const [w, setW] = useState(800)
  useEffect(() => {
    const el = ref.current
    if (!el) return
    const ro = new ResizeObserver(() => setW(Math.max(300, el.clientWidth)))
    ro.observe(el)
    return () => ro.disconnect()
  }, [ref])
  return w
}
