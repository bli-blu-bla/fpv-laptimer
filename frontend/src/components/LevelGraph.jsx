// @ts-check
// The level graph of one pilot in a session, for corrections: the signal
// (boards combined, each board faint), the detection thresholds, and the
// passes. Tap the graph to add a missed pass (the server moves it to the
// nearest peak), tap a pass to delete or restore it. Drag or use the
// buttons to move, +/− to zoom; the strip on top shows the whole session.

import { useEffect, useRef, useState } from 'react'
import { CaretLeft, CaretRight, MagnifyingGlassMinus, MagnifyingGlassPlus } from '@phosphor-icons/react'
import { api } from '../api.js'
import { secs } from '../format.js'

const HEIGHT = 230
const OVERVIEW_HEIGHT = 46
const PAD_LEFT = 34
const PAD_BOTTOM = 18
/** dBFS shown at the bottom of the graph. */
const MIN_DB = -62
/** How close (px) a tap must be to a pass to select it. */
const HIT_PX = 14

/**
 * @typedef {object} Graph
 * @property {number[]} time_us
 * @property {(number | null)[]} level
 * @property {(number | null)[][]} boards
 * @property {(number | null)[]} enter
 * @property {(number | null)[]} exit
 *
 * @typedef {{start_us: number | null, end_us: number | null, graph: Graph | null}} GraphReply
 */

/**
 * @param {object} props
 * @param {number} props.sessionId
 * @param {number} props.channel
 * @param {number} props.t0                 times are shown in seconds from here
 * @param {import('../types').Pass[]} props.passes   this channel's passes
 * @param {import('../types').PilotResult} props.result
 * @param {() => void} props.onChanged      a pass was added, deleted or restored
 */
export default function LevelGraph({ sessionId, channel, t0, passes, result, onChanged }) {
  const box = useRef(/** @type {HTMLDivElement | null} */ (null))
  const width = useWidth(box)
  const [overview, setOverview] = useState(/** @type {GraphReply | null} */ (null))
  const [detail, setDetail] = useState(/** @type {Graph | null} */ (null))
  const [range, setRange] = useState(/** @type {[number, number] | null} */ (null))
  /** What was tapped: a time (to add a pass; `snap`: where it would go) or
   * a pass (to delete/restore). */
  const [sel, setSel] = useState(
    /** @type {{kind: 'time', t: number, snap?: {time_us: number | null, peak_db: number | null}} | {kind: 'pass', pass: import('../types').Pass} | null} */ (
      null
    ),
  )
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)
  // A tapped time: ask the server where the pass would snap to.
  const selTime = sel?.kind === 'time' ? sel.t : null
  useEffect(() => {
    if (selTime == null) return
    let stale = false
    api(`/sessions/${sessionId}/snap?channel=${channel}&time_us=${Math.round(selTime)}`)
      .then((snap) => !stale && setSel((s) => (s?.kind === 'time' && s.t === selTime ? { ...s, snap } : s)))
      .catch(() => {})
    return () => {
      stale = true
    }
  }, [sessionId, channel, selTime])

  // Dragging moves the range; a short move counts as a tap. (Hooks must come
  // before any `return`: React calls them in the same order every time.)
  const drag = useRef(/** @type {{x: number, from: number, to: number, moved: boolean} | null} */ (null))

  // The whole session, once (and again when passes change: the file may have grown).
  useEffect(() => {
    api(`/sessions/${sessionId}/graph?channel=${channel}&points=${Math.round(width)}`)
      .then((/** @type {GraphReply} */ r) => {
        setOverview(r)
        if (r.start_us != null && r.end_us != null) {
          const [s, e] = [r.start_us, r.end_us]
          // Start with the first 30 s after the first pass, or everything.
          const first = passes.find((p) => !p.deleted)?.time_us
          setRange((old) => old ?? (first ? [Math.max(s, first - 5e6), Math.min(e, first + 25e6)] : [s, e]))
        }
      })
      .catch((e) => setError(e.message))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionId, channel, Math.round(width / 100), passes.length])

  // The visible part, at screen resolution.
  useEffect(() => {
    if (!range) return
    const id = setTimeout(() => {
      api(`/sessions/${sessionId}/graph?channel=${channel}&from_us=${range[0]}&to_us=${range[1]}&points=${Math.round(width)}`)
        .then((/** @type {GraphReply} */ r) => setDetail(r.graph))
        .catch((e) => setError(e.message))
    }, 60) // while dragging, only ask once the range rests
    return () => clearTimeout(id)
  }, [sessionId, channel, range, width])

  // Always the same outer element: it is what `useWidth` measures.
  /** @param {string} text @param {string} cls */
  const note = (text, cls) => (
    <div className="graph" ref={box}>
      <p className={cls}>{text}</p>
    </div>
  )
  if (error) return note(error, 'error')
  if (!overview || !range) return note('Lade Pegel …', 'muted')
  if (overview.start_us == null || overview.end_us == null) return note('Keine Pegel aufgezeichnet.', 'muted')
  const [start, end] = [overview.start_us, overview.end_us]
  const [from, to] = range
  const span = to - from

  /** Keeps a range inside the session. @param {number} a @param {number} b @returns {[number, number]} */
  function clamp(a, b) {
    const len = Math.min(b - a, end - start)
    const s = Math.max(start, Math.min(a, end - len))
    return [s, s + len]
  }
  /** @param {number} f */
  const zoom = (f) => {
    const mid = (from + to) / 2
    const len = Math.max(2e6, Math.min(end - start, span * f))
    setRange(clamp(mid - len / 2, mid + len / 2))
  }
  /** @param {number} f */
  const pan = (f) => setRange(clamp(from + span * f, to + span * f))

  // --- detail graph geometry ---
  const plotW = width - PAD_LEFT
  const plotH = HEIGHT - PAD_BOTTOM
  const levels = detail ? detail.level.filter((v) => v != null) : []
  const maxDb = Math.max(-20, ...levels.map((v) => /** @type {number} */ (v) + 3))
  /** @param {number} t */
  const x = (t) => PAD_LEFT + ((t - from) / span) * plotW
  /** @param {number} db */
  const y = (db) => plotH - ((db - MIN_DB) / (maxDb - MIN_DB)) * plotH
  const visible = passes.filter((p) => p.time_us >= from && p.time_us <= to)
  const lapOf = new Map(result.laps.map((l) => [l.pass_id, l]))

  /** Tap: a pass near it, else a time. @param {React.PointerEvent<SVGSVGElement>} e */
  function tap(e) {
    const rect = e.currentTarget.getBoundingClientRect()
    const px = ((e.clientX - rect.left) / rect.width) * width
    if (px < PAD_LEFT) return
    const near = visible
      .map((p) => ({ p, d: Math.abs(x(p.time_us) - px) }))
      .filter(({ d }) => d < HIT_PX)
      .sort((a, b) => a.d - b.d)[0]
    if (near) setSel({ kind: 'pass', pass: near.p })
    else setSel({ kind: 'time', t: from + ((px - PAD_LEFT) / plotW) * span })
  }

  /** @param {React.PointerEvent<SVGSVGElement>} e */
  function down(e) {
    e.currentTarget.setPointerCapture(e.pointerId)
    drag.current = { x: e.clientX, from, to, moved: false }
  }
  /** @param {React.PointerEvent<SVGSVGElement>} e */
  function move(e) {
    const d = drag.current
    if (!d) return
    const dx = e.clientX - d.x
    if (Math.abs(dx) > 5) d.moved = true
    if (d.moved) {
      const rect = e.currentTarget.getBoundingClientRect()
      const dt = (-dx / rect.width) * width * ((d.to - d.from) / plotW)
      setRange(clamp(d.from + dt, d.to + dt))
    }
  }
  /** @param {React.PointerEvent<SVGSVGElement>} e */
  function up(e) {
    if (drag.current && !drag.current.moved) tap(e)
    drag.current = null
  }

  /** @param {() => Promise<unknown>} f */
  async function act(f) {
    setBusy(true)
    try {
      await f()
      setSel(null)
      onChanged()
    } catch (e) {
      setError(/** @type {Error} */ (e).message)
    } finally {
      setBusy(false)
    }
  }

  // Seconds ticks: about one per 80 px, at round steps.
  const step = [1, 2, 5, 10, 20, 30, 60, 120, 300].find((s) => (s * 1e6 * plotW) / span >= 80) ?? 600
  const ticks = []
  for (let s = Math.ceil((from - t0) / 1e6 / step) * step; t0 + s * 1e6 <= to; s += step) ticks.push(s)

  return (
    <div className="graph" ref={box}>
      <Overview reply={overview} width={width} range={range} passes={passes} onPick={(t) => setRange(clamp(t - span / 2, t + span / 2))} />
      <svg
        width="100%"
        viewBox={`0 0 ${width} ${HEIGHT}`}
        className="detail"
        onPointerDown={down}
        onPointerMove={move}
        onPointerUp={up}
      >
        {[-60, -50, -40, -30, -20, -10, 0].filter((d) => d < maxDb).map((d) => (
          <g key={d}>
            <line x1={PAD_LEFT} x2={width} y1={y(d)} y2={y(d)} className="grid" />
            <text x={PAD_LEFT - 4} y={y(d) + 4} className="axis" textAnchor="end">
              {d}
            </text>
          </g>
        ))}
        {ticks.map((s) => (
          <text key={s} x={x(t0 + s * 1e6)} y={HEIGHT - 4} className="axis" textAnchor="middle">
            {s} s
          </text>
        ))}
        {detail && (
          <>
            {detail.boards.map((b, i) => (
              <path key={i} d={line(detail.time_us, b, x, y)} className={`board b${i}`} />
            ))}
            <path d={line(detail.time_us, detail.enter, x, y)} className="threshold" />
            <path d={line(detail.time_us, detail.exit, x, y)} className="threshold exit" />
            <path d={line(detail.time_us, detail.level, x, y)} className="combined" />
          </>
        )}
        {visible.map((p) => {
          const lap = lapOf.get(p.id)
          const cls = p.deleted ? 'pass deleted' : p.manual ? 'pass manual' : result.ignored.includes(p.id) ? 'pass ignored' : 'pass'
          return (
            <g key={p.id} className={cls}>
              <line x1={x(p.time_us)} x2={x(p.time_us)} y1={14} y2={plotH} />
              {lap && (
                <text x={x(p.time_us) - 4} y={11} textAnchor="end" className="lap-label">
                  {secs(lap.duration_us)}
                </text>
              )}
            </g>
          )
        })}
        {sel?.kind === 'time' && <line x1={x(sel.t)} x2={x(sel.t)} y1={0} y2={plotH} className="cursor faint" />}
        {sel?.kind === 'time' && sel.snap?.time_us != null && (
          <line x1={x(sel.snap.time_us)} x2={x(sel.snap.time_us)} y1={0} y2={plotH} className="cursor" />
        )}
        {sel?.kind === 'pass' && <line x1={x(sel.pass.time_us)} x2={x(sel.pass.time_us)} y1={0} y2={plotH} className="cursor" />}
      </svg>
      <div className="graph-tools">
        <button className="btn btn-secondary btn-icon" onClick={() => pan(-0.5)} title="Zurück">
          <CaretLeft />
        </button>
        <button className="btn btn-secondary btn-icon" onClick={() => zoom(2)} title="Weniger Zoom">
          <MagnifyingGlassMinus />
        </button>
        <button className="btn btn-secondary btn-icon" onClick={() => zoom(0.5)} title="Mehr Zoom">
          <MagnifyingGlassPlus />
        </button>
        <button className="btn btn-secondary btn-icon" onClick={() => pan(0.5)} title="Weiter">
          <CaretRight />
        </button>
        <button className="btn btn-secondary" onClick={() => setRange([start, end])}>
          Alles
        </button>
        <span className="muted">
          {secs(from - t0)}–{secs(to - t0)} s
        </span>
      </div>
      {sel && (
        <div className="graph-action">
          {sel.kind === 'time' ? (
            <>
              <span>
                {sel.snap?.time_us != null
                  ? `Durchflug am Gipfel bei ${secs(sel.snap.time_us - t0)} s einfügen?`
                  : sel.snap
                    ? `Kein Gipfel in der Nähe: bei ${secs(sel.t - t0)} s einfügen?`
                    : 'Suche den Gipfel …'}
              </span>
              <button
                className="btn btn-primary"
                disabled={busy || !sel.snap}
                onClick={() =>
                  act(() =>
                    api(`/sessions/${sessionId}/passes`, {
                      body: { channel, time_us: Math.round(sel.snap?.time_us ?? sel.t), snap: false },
                    }),
                  )
                }
              >
                Einfügen
              </button>
            </>
          ) : (
            <>
              <span>
                Durchflug bei {secs(sel.pass.time_us - t0)} s{sel.pass.manual ? ' (von Hand)' : ''}
                {sel.pass.deleted ? ' – gelöscht' : ''}
              </span>
              <button
                className={sel.pass.deleted ? 'btn btn-primary' : 'btn btn-danger'}
                disabled={busy}
                onClick={() => act(() => api(`/passes/${sel.pass.id}`, { body: { deleted: !sel.pass.deleted } }))}
              >
                {sel.pass.deleted ? 'Wiederherstellen' : 'Löschen'}
              </button>
            </>
          )}
          <button className="btn btn-secondary" onClick={() => setSel(null)}>
            Abbrechen
          </button>
        </div>
      )}
      <p className="muted small">
        Farbig: beide Boards zusammen, blass: die einzelnen Boards, gestrichelt: Eintritts- und Austrittsschwelle.
        Grün: erkannt, blau: von Hand, grau: gelöscht. Tippen nahe einem Gipfel fügt dort einen Durchflug ein, Tippen
        auf einen Durchflug löscht ihn; ziehen verschiebt den Ausschnitt.
      </p>
    </div>
  )
}

/**
 * The whole session as a small strip; the visible range is marked. Tap to go there.
 * @param {object} props
 * @param {GraphReply} props.reply
 * @param {number} props.width
 * @param {[number, number]} props.range
 * @param {import('../types').Pass[]} props.passes
 * @param {(t: number) => void} props.onPick
 */
function Overview({ reply, width, range, passes, onPick }) {
  const g = reply.graph
  if (!g || reply.start_us == null || reply.end_us == null) return null
  const [s, e] = [reply.start_us, reply.end_us]
  /** @param {number} t */
  const x = (t) => PAD_LEFT + ((t - s) / (e - s)) * (width - PAD_LEFT)
  /** @param {number} db */
  const y = (db) => OVERVIEW_HEIGHT - ((db - MIN_DB) / (0 - MIN_DB)) * OVERVIEW_HEIGHT
  return (
    <svg
      width="100%"
      viewBox={`0 0 ${width} ${OVERVIEW_HEIGHT}`}
      className="overview"
      onClick={(ev) => {
        const rect = ev.currentTarget.getBoundingClientRect()
        const px = ((ev.clientX - rect.left) / rect.width) * width
        onPick(s + ((px - PAD_LEFT) / (width - PAD_LEFT)) * (e - s))
      }}
    >
      <rect x={x(range[0])} width={Math.max(2, x(range[1]) - x(range[0]))} y={0} height={OVERVIEW_HEIGHT} className="window" />
      <path d={line(g.time_us, g.level, x, y)} className="combined" />
      {passes
        .filter((p) => !p.deleted)
        .map((p) => (
          <line key={p.id} x1={x(p.time_us)} x2={x(p.time_us)} y1={0} y2={OVERVIEW_HEIGHT} className="pass-tick" />
        ))}
    </svg>
  )
}

/**
 * An SVG path through the points; gaps (null) break the line.
 * @param {number[]} ts @param {(number | null)[]} vs
 * @param {(t: number) => number} x @param {(v: number) => number} y
 */
function line(ts, vs, x, y) {
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
