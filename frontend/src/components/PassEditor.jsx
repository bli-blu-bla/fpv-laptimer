// @ts-check
// Correcting a session pilot by pilot (as RotorHazard's marshal page): the
// channel is picked at the top, below it the big level graph with the
// passes. Drag a pass to move it, tap it to delete or restore it, tap an
// empty spot to add one (it moves to the nearest peak). Nothing is saved
// until "Speichern"; the laps under the graph already show the result.

import { useEffect, useRef, useState } from 'react'
import { ArrowCounterClockwiseIcon, CaretLeftIcon, CaretRightIcon, CrosshairIcon, FloppyDiskIcon, MagnifyingGlassMinusIcon, MagnifyingGlassPlusIcon, ScissorsIcon, TrashIcon } from '@phosphor-icons/react'
import { api } from '../api.js'
import { secs, slotColor } from '../format.js'
import { computeLaps } from '../model.js'

const HEIGHT = 340
const OVERVIEW_HEIGHT = 44
const PAD_LEFT = 34
const PAD_TOP = 34
const PAD_BOTTOM = 18
/** dBFS at the bottom of the graph. */
const MIN_DB = -62
/** How close (px) a tap or drag must start to a pass to grab it. */
const HIT_PX = 16

/**
 * Unsaved corrections of one channel.
 * @typedef {{moves: Record<number, number>, deletes: number[], restores: number[], adds: {key: string, t: number}[]}} Draft
 *
 * A pass as shown: `key` is "p<id>" for stored passes, "a…" for added ones.
 * @typedef {{key: string, id: number | null, t: number, kind: 'auto' | 'manual' | 'moved' | 'added' | 'deleted'}} Marker
 *
 * @typedef {object} Graph
 * @property {number[]} time_us
 * @property {(number | null)[]} level
 * @property {(number | null)[][]} boards
 * @property {(number | null)[]} enter
 * @property {(number | null)[]} exit
 * @typedef {{start_us: number | null, end_us: number | null, graph: Graph | null}} GraphReply
 */

/** @returns {Draft} */
const emptyDraft = () => ({ moves: {}, deletes: [], restores: [], adds: [] })
/** @param {Draft | undefined} d */
const changes = (d) => (d ? Object.keys(d.moves).length + d.deletes.length + d.restores.length + d.adds.length : 0)

/** @param {import('../types').Pass[]} passes @param {Draft} d @returns {Marker[]} */
function markersOf(passes, d) {
  /** @type {Marker[]} */
  const out = passes.map((p) => {
    const deleted = (p.deleted && !d.restores.includes(p.id)) || d.deletes.includes(p.id)
    const moved = d.moves[p.id]
    return {
      key: 'p' + p.id,
      id: p.id,
      t: moved ?? p.time_us,
      kind: deleted ? 'deleted' : moved != null ? 'moved' : p.manual ? 'manual' : 'auto',
    }
  })
  for (const a of d.adds) out.push({ key: a.key, id: null, t: a.t, kind: 'added' })
  return out.sort((a, b) => a.t - b.t)
}

/**
 * @param {object} props
 * @param {import('../types').SessionDetail} props.detail   with passes
 * @param {() => void} props.onSaved
 */
export default function PassEditor({ detail, onSaved }) {
  const s = detail.session
  const t0 = s.config.race ? s.start_us : s.created_us
  const passes = detail.passes ?? []
  const [channel, setChannel] = useState(() => (passes.find((p) => !p.deleted) ?? { channel: s.seats[0]?.channel ?? 0 }).channel)
  const [drafts, setDrafts] = useState(/** @type {Record<number, Draft>} */ ({}))
  const [sel, setSel] = useState(/** @type {string | null} */ (null))
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)
  const [confirmTrim, setConfirmTrim] = useState(false)
  // Bumped after trimming: the graph loads again.
  const [trims, setTrims] = useState(0)

  const draft = drafts[channel] ?? emptyDraft()
  /** @param {(d: Draft) => Draft} f */
  const setDraft = (f) => setDrafts((ds) => ({ ...ds, [channel]: f(ds[channel] ?? emptyDraft()) }))
  const markers = markersOf(
    passes.filter((p) => p.channel === channel && !p.gate),
    draft,
  )
  // Arming-gate passes: shown, not edited.
  const arming = passes.filter((p) => p.channel === channel && p.gate === 1 && !p.deleted).map((p) => p.time_us)
  const counting = markers.filter((m) => m.kind !== 'deleted')
  const preview = computeLaps(
    counting.map((m) => ({ id: /** @type {any} */ (m.key), time_us: m.t })),
    s.config,
    t0,
  )
  const selected = markers.find((m) => m.key === sel) ?? null
  // Laps under the minimum lap: only a hint, a pass set by hand counts anyway.
  const minLapUs = s.config.min_lap_s * 1e6
  const tooShort = preview.laps.filter((l) => l.duration_us < minLapUs).length
  const total = Object.values(drafts).reduce((n, d) => n + changes(d), 0)

  /** Moves a pass (stored or added) to `t`. @param {string} key @param {number} t */
  function move(key, t) {
    t = Math.round(t)
    setDraft((d) =>
      key.startsWith('p')
        ? { ...d, moves: { ...d.moves, [Number(key.slice(1))]: t } }
        : { ...d, adds: d.adds.map((a) => (a.key === key ? { ...a, t } : a)) },
    )
  }

  /** Delete, or restore a deleted one. @param {Marker} m */
  function toggleDelete(m) {
    setDraft((d) => {
      if (m.id == null) return { ...d, adds: d.adds.filter((a) => a.key !== m.key) }
      const id = m.id
      if (d.deletes.includes(id)) return { ...d, deletes: d.deletes.filter((x) => x !== id) }
      if (d.restores.includes(id)) return { ...d, restores: d.restores.filter((x) => x !== id) }
      if (m.kind === 'deleted') return { ...d, restores: [...d.restores, id] }
      return { ...d, deletes: [...d.deletes, id] }
    })
    if (m.id == null) setSel(null)
  }

  /** Where the server would put a pass near `t` (the peak), or `t`. @param {number} t */
  async function snap(t) {
    const r = await api(`/sessions/${s.id}/snap?channel=${channel}&time_us=${Math.round(t)}`).catch(() => null)
    return Math.round(r?.time_us ?? t)
  }

  /** A tap on an empty spot: a new pass there (at the nearest peak). @param {number} t */
  async function add(t) {
    const key = 'a' + Date.now()
    const at = await snap(t)
    setDraft((d) => ({ ...d, adds: [...d.adds, { key, t: at }] }))
    setSel(key)
  }

  async function save() {
    setBusy(true)
    setError('')
    try {
      for (const [ch, d] of Object.entries(drafts)) {
        if (!changes(d)) continue
        await api(`/sessions/${s.id}/edit`, {
          body: {
            channel: Number(ch),
            moves: Object.entries(d.moves).map(([id, t]) => [Number(id), t]),
            deletes: d.deletes,
            restores: d.restores,
            adds: d.adds.map((a) => a.t),
          },
        })
        setDrafts((ds) => ({ ...ds, [ch]: emptyDraft() }))
      }
      setSel(null)
      onSaved()
    } catch (e) {
      setError(/** @type {Error} */ (e).message)
    } finally {
      setBusy(false)
    }
  }

  /** Cuts the levels 10 s after the last pass (all channels). */
  async function trim() {
    setBusy(true)
    setError('')
    try {
      await api(`/sessions/${s.id}/trim`, { method: 'POST' })
      setConfirmTrim(false)
      setTrims((n) => n + 1)
      onSaved()
    } catch (e) {
      setError(/** @type {Error} */ (e).message)
    } finally {
      setBusy(false)
    }
  }

  const color = slotColor(channel)
  return (
    <div className="panel editor" style={/** @type {React.CSSProperties} */ ({ '--lane': color })}>
      <div className="editor-chans">
        {s.seats.map((seat) => {
          const n = changes(drafts[seat.channel])
          return (
            <button
              key={seat.channel}
              className={'option' + (seat.channel === channel ? ' on' : '')}
              onClick={() => {
                setChannel(seat.channel)
                setSel(null)
              }}
            >
              <span className="opt-main" style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                <span className="bar" style={{ background: slotColor(seat.channel) }} />
                {seat.label}
              </span>
              <span className="note">
                {seat.channel_name}
                {n ? ` · ${n} Änderung${n === 1 ? '' : 'en'}` : ''}
              </span>
            </button>
          )
        })}
      </div>

      <EditGraph
        key={`${channel}-${trims}`}
        sessionId={s.id}
        channel={channel}
        t0={t0}
        markers={markers}
        arming={arming}
        lapEnds={new Map(preview.laps.map((l) => [String(l.pass_id), l.duration_us]))}
        selected={sel}
        onSelect={setSel}
        onMove={move}
        onTapEmpty={add}
      />

      {selected && (
        <div className="graph-action">
          <span style={{ flex: 1, minWidth: 160 }}>
            Durchflug bei <b className="tnum">{secs(selected.t - t0)} s</b>
            {selected.kind === 'deleted' ? ' – gelöscht' : selected.kind === 'added' ? ' – neu' : selected.kind === 'moved' ? ' – verschoben' : ''}
          </span>
          {selected.kind !== 'deleted' && (
            <button className="btn btn-secondary" onClick={async () => move(selected.key, await snap(selected.t))}>
              <CrosshairIcon size={16} /> Am Gipfel einrasten
            </button>
          )}
          <button className={selected.kind === 'deleted' ? 'btn btn-primary' : 'btn btn-danger'} onClick={() => toggleDelete(selected)}>
            {selected.kind === 'deleted' ? (
              <>
                <ArrowCounterClockwiseIcon size={16} /> Wiederherstellen
              </>
            ) : (
              <>
                <TrashIcon size={16} /> Löschen
              </>
            )}
          </button>
        </div>
      )}

      <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
        <span className="muted small">
          {preview.holeshot_us != null ? `Holeshot ${secs(preview.holeshot_us)} · ` : ''}
          {preview.laps.length} Runden{preview.finished_us != null ? ' · im Ziel' : ''}
        </span>
        <div className="lap-chips">
          {preview.laps.map((l) => (
            <span key={l.number} className={l.duration_us < minLapUs ? 'short' : l.duration_us === preview.best_lap_us ? 'best' : ''}>
              {l.number}. {secs(l.duration_us)}
            </span>
          ))}
        </div>
      </div>

      <p className="muted small" style={{ margin: 0 }}>
        Ziehen verschiebt einen Durchflug (oder den Ausschnitt), Tippen wählt ihn aus, Tippen auf eine freie Stelle fügt einen
        am nächsten Gipfel ein. Grün: erkannt, blau: von Hand, grau gestrichelt: gelöscht, gelb gepunktet: Freigabe-Gate.
      </p>
      {tooShort > 0 && (
        <p className="muted small" style={{ margin: 0 }}>
          {tooShort === 1 ? 'Eine Runde ist' : `${tooShort} Runden sind`} kürzer als die Mindest-Rundenzeit ({s.config.min_lap_s} s). Von Hand
          gesetzt zählt sie trotzdem.
        </p>
      )}
      {error && <p className="error">{error}</p>}
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
        <span className="muted small" style={{ flex: 1, minWidth: 200 }}>
          {total ? 'Erst speichern oder verwerfen.' : confirmTrim ? 'Pegel und Durchflüge nach dem letzten Durchflug + 10 s, alle Kanäle. Nicht rückgängig zu machen.' : 'Pegel spätestens 10 s nach dem letzten Durchflug abschneiden.'}
        </span>
        {confirmTrim && (
          <button className="btn btn-ghost" onClick={() => setConfirmTrim(false)}>
            Abbrechen
          </button>
        )}
        <button
          className={confirmTrim ? 'btn btn-danger btn-danger-solid' : 'btn btn-secondary'}
          disabled={busy || total > 0}
          onClick={() => (confirmTrim ? trim() : setConfirmTrim(true))}
        >
          <ScissorsIcon size={16} /> {confirmTrim ? 'Endgültig kürzen' : 'Graph kürzen …'}
        </button>
      </div>
      <div className="editor-save">
        <span className="muted small">{total ? `${total} ungespeicherte Änderung${total === 1 ? '' : 'en'}` : 'Keine Änderungen'}</span>
        <button
          className="btn btn-secondary"
          disabled={!total || busy}
          onClick={() => {
            setDrafts({})
            setSel(null)
          }}
        >
          Verwerfen
        </button>
        <button className="btn btn-primary" disabled={!total || busy} onClick={save}>
          <FloppyDiskIcon size={18} /> Speichern
        </button>
      </div>
    </div>
  )
}

/**
 * The graph: overview strip, then the zoomable detail with the passes.
 * @param {object} props
 * @param {number} props.sessionId
 * @param {number} props.channel
 * @param {number} props.t0
 * @param {Marker[]} props.markers
 * @param {number[]} props.arming   times of the arming-gate passes
 * @param {Map<string, number>} props.lapEnds   marker key → the lap it ends
 * @param {string | null} props.selected
 * @param {(key: string | null) => void} props.onSelect
 * @param {(key: string, t: number) => void} props.onMove
 * @param {(t: number) => void} props.onTapEmpty
 */
function EditGraph({ sessionId, channel, t0, markers, arming, lapEnds, selected, onSelect, onMove, onTapEmpty }) {
  const box = useRef(/** @type {HTMLDivElement | null} */ (null))
  const width = useWidth(box)
  const [overview, setOverview] = useState(/** @type {GraphReply | null} */ (null))
  const [detail, setDetail] = useState(/** @type {Graph | null} */ (null))
  const [range, setRange] = useState(/** @type {[number, number] | null} */ (null))
  const [error, setError] = useState('')
  /** The arming gate's level line: whether the session has one, shown or not. */
  const [hasArming, setHasArming] = useState(false)
  const [showArming, setShowArming] = useState(false)
  const [armDetail, setArmDetail] = useState(/** @type {Graph | null} */ (null))
  /** What a pointer press is doing: moving a pass, or the visible range. */
  const drag = useRef(/** @type {{x: number, moved: boolean, key: string | null, from: number, to: number} | null} */ (null))

  useEffect(() => {
    api(`/sessions/${sessionId}/graph?channel=${channel}&points=${Math.round(width)}`)
      .then((/** @type {GraphReply} */ r) => {
        setOverview(r)
        if (r.start_us != null && r.end_us != null) {
          const [s, e] = [r.start_us, r.end_us]
          setRange((old) => old ?? [s, Math.min(e, s + 40e6)])
        }
      })
      .catch((e) => setError(e.message))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionId, channel, Math.round(width / 100)])

  useEffect(() => {
    setHasArming(false)
    api(`/sessions/${sessionId}/graph?channel=${channel}&gate=arming&points=10`)
      .then((/** @type {GraphReply} */ r) => setHasArming(r.start_us != null))
      .catch(() => {})
  }, [sessionId, channel])

  useEffect(() => {
    if (!range || !showArming || !hasArming) return setArmDetail(null)
    const id = setTimeout(() => {
      const [a, b] = range.map(Math.round)
      api(`/sessions/${sessionId}/graph?channel=${channel}&gate=arming&from_us=${a}&to_us=${b}&points=${Math.round(width)}`)
        .then((/** @type {GraphReply} */ r) => setArmDetail(r.graph))
        .catch((e) => setError(e.message))
    }, 60)
    return () => clearTimeout(id)
  }, [sessionId, channel, range, width, showArming, hasArming])

  useEffect(() => {
    if (!range) return
    const id = setTimeout(() => {
      // The server takes whole µs: computed ranges have fractions.
      const [a, b] = range.map(Math.round)
      api(`/sessions/${sessionId}/graph?channel=${channel}&from_us=${a}&to_us=${b}&points=${Math.round(width)}`)
        .then((/** @type {GraphReply} */ r) => setDetail(r.graph))
        .catch((e) => setError(e.message))
    }, 60) // while dragging, ask once the range rests
    return () => clearTimeout(id)
  }, [sessionId, channel, range, width])

  /** @param {string} text @param {string} cls */
  const note = (text, cls) => (
    <div className="graph" ref={box}>
      <p className={cls}>{text}</p>
    </div>
  )
  if (error) return note(error, 'error')
  if (!overview || !range) return note('Lade Pegel …', 'muted')
  if (overview.start_us == null || overview.end_us == null) return note('Für diesen Kanal sind keine Pegel aufgezeichnet.', 'muted')
  const [start, end] = [overview.start_us, overview.end_us]
  const [from, to] = range
  const span = to - from

  /** @param {number} a @param {number} b @returns {[number, number]} */
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

  const plotW = width - PAD_LEFT
  const plotH = HEIGHT - PAD_BOTTOM
  const levels = /** @type {number[]} */ ([...(detail?.level ?? []), ...(armDetail?.level ?? [])].filter((v) => v != null))
  const maxDb = Math.max(-20, ...levels.map((v) => v + 3))
  /** @param {number} t */
  const x = (t) => PAD_LEFT + ((t - from) / span) * plotW
  /** @param {number} db */
  const y = (db) => PAD_TOP + (plotH - PAD_TOP) - ((db - MIN_DB) / (maxDb - MIN_DB)) * (plotH - PAD_TOP)
  /** Pixel position → time. @param {number} px */
  const timeAt = (px) => from + ((px - PAD_LEFT) / plotW) * span
  /** @param {React.PointerEvent<SVGSVGElement>} e */
  const pxOf = (e) => {
    const r = e.currentTarget.getBoundingClientRect()
    return ((e.clientX - r.left) / r.width) * width
  }
  const visible = markers.filter((m) => m.t >= from - span * 0.02 && m.t <= to + span * 0.02)

  /** @param {React.PointerEvent<SVGSVGElement>} e */
  function down(e) {
    e.currentTarget.setPointerCapture(e.pointerId)
    const px = pxOf(e)
    const near = visible
      .map((m) => ({ m, d: Math.abs(x(m.t) - px) }))
      .filter(({ d }) => d < HIT_PX)
      .sort((a, b) => a.d - b.d)[0]
    drag.current = { x: e.clientX, moved: false, key: near?.m.key ?? null, from, to }
  }
  /** @param {React.PointerEvent<SVGSVGElement>} e */
  function move(e) {
    const d = drag.current
    if (!d) return
    if (Math.abs(e.clientX - d.x) > 4) d.moved = true
    if (!d.moved) return
    const m = d.key ? markers.find((k) => k.key === d.key) : null
    if (m && m.kind !== 'deleted') {
      onSelect(m.key)
      onMove(m.key, Math.max(start, Math.min(end, timeAt(pxOf(e)))))
    } else {
      // Move the visible range with the finger.
      const r = e.currentTarget.getBoundingClientRect()
      const dt = (-(e.clientX - d.x) / r.width) * width * ((d.to - d.from) / plotW)
      setRange(clamp(d.from + dt, d.to + dt))
    }
  }
  /** @param {React.PointerEvent<SVGSVGElement>} e */
  function up(e) {
    const d = drag.current
    drag.current = null
    if (!d || d.moved) return
    if (d.key) onSelect(d.key)
    else if (pxOf(e) >= PAD_LEFT) onTapEmpty(timeAt(pxOf(e)))
  }

  const step = [1, 2, 5, 10, 20, 30, 60, 120, 300].find((s) => (s * 1e6 * plotW) / span >= 80) ?? 600
  const ticks = []
  for (let s = Math.ceil((from - t0) / 1e6 / step) * step; t0 + s * 1e6 <= to; s += step) ticks.push(s)

  return (
    <div className="graph" ref={box}>
      <Overview reply={overview} width={width} range={range} markers={markers} onPick={(t) => setRange(clamp(t - span / 2, t + span / 2))} />
      <svg width="100%" viewBox={`0 0 ${width} ${HEIGHT}`} className="detail" onPointerDown={down} onPointerMove={move} onPointerUp={up}>
        {[-60, -50, -40, -30, -20, -10, 0]
          .filter((d) => d < maxDb)
          .map((d) => (
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
              <path key={i} d={line(detail.time_us, b, x, y)} className="board" />
            ))}
            <path d={line(detail.time_us, detail.enter, x, y)} className="threshold" />
            <path d={line(detail.time_us, detail.exit, x, y)} className="threshold exit" />
            <path d={line(detail.time_us, detail.level, x, y)} className="combined" />
          </>
        )}
        {armDetail && (
          <>
            <path d={line(armDetail.time_us, armDetail.enter, x, y)} className="arming-threshold" />
            <path d={line(armDetail.time_us, armDetail.level, x, y)} className="arming-level" />
          </>
        )}
        {arming
          .filter((t) => t >= from && t <= to)
          .map((t) => (
            <line key={'arm' + t} x1={x(t)} x2={x(t)} y1={PAD_TOP} y2={plotH} className="arming-tick" />
          ))}
        {visible.map((m) => {
          const lap = lapEnds.get(m.key)
          return (
            <g key={m.key} className={`pass ${m.kind}${m.key === selected ? ' sel' : ''}`}>
              <line x1={x(m.t)} x2={x(m.t)} y1={PAD_TOP - 8} y2={plotH} />
              <circle cx={x(m.t)} cy={PAD_TOP - 8} r={7} />
              {lap != null && (
                <text x={x(m.t)} y={12} textAnchor="middle" className="lap-label">
                  {secs(lap)}
                </text>
              )}
            </g>
          )
        })}
      </svg>
      <div className="graph-tools">
        <button className="btn btn-secondary btn-icon" onClick={() => pan(-0.5)} title="Zurück">
          <CaretLeftIcon />
        </button>
        <button className="btn btn-secondary btn-icon" onClick={() => zoom(2)} title="Weniger Zoom">
          <MagnifyingGlassMinusIcon />
        </button>
        <button className="btn btn-secondary btn-icon" onClick={() => zoom(0.5)} title="Mehr Zoom">
          <MagnifyingGlassPlusIcon />
        </button>
        <button className="btn btn-secondary btn-icon" onClick={() => pan(0.5)} title="Weiter">
          <CaretRightIcon />
        </button>
        <button className="btn btn-secondary" onClick={() => setRange([start, end])}>
          Alles
        </button>
        {hasArming && (
          <button
            className={showArming ? 'chip-btn on' : 'chip-btn'}
            title="Pegel und Eintrittsschwelle des Freigabe-Gates (gelb)"
            onClick={() => setShowArming(!showArming)}
          >
            Freigabe-Gate
          </button>
        )}
        <span className="muted small tnum">
          {secs(from - t0)}–{secs(to - t0)} s
        </span>
      </div>
    </div>
  )
}

/**
 * The whole session as a strip; the visible range marked. Tap to go there.
 * @param {{reply: GraphReply, width: number, range: [number, number], markers: Marker[], onPick: (t: number) => void}} props
 */
function Overview({ reply, width, range, markers, onPick }) {
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
        const r = ev.currentTarget.getBoundingClientRect()
        const px = ((ev.clientX - r.left) / r.width) * width
        onPick(s + ((px - PAD_LEFT) / (width - PAD_LEFT)) * (e - s))
      }}
    >
      <rect x={x(range[0])} width={Math.max(2, x(range[1]) - x(range[0]))} y={0} height={OVERVIEW_HEIGHT} className="window" />
      <path d={line(g.time_us, g.level, x, y)} className="combined" />
      {markers
        .filter((m) => m.kind !== 'deleted')
        .map((m) => (
          <line key={m.key} x1={x(m.t)} x2={x(m.t)} y1={0} y2={OVERVIEW_HEIGHT} className="pass-tick" />
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
