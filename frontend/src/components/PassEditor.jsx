// @ts-check
// Correcting a session pilot by pilot (as RotorHazard's marshal page): the
// channel is picked at the top, below it the big level graph with the
// passes. Drag a pass to move it, tap it to delete or restore it, tap an
// empty spot to add one (it moves to the nearest peak). Nothing is saved
// until "Speichern"; the laps under the graph already show the result.
//
// "Neu rechnen" (`RecalcPanel`): the chosen channel through the detector
// again with another mode, minimum swing, thresholds or minimum lap
// (`GET /api/sessions/{id}/recompute`). The graph then shows that run's level
// and thresholds, and the passes it finds as violet preview markers next to
// the saved ones. Nothing is saved by it; "Übernehmen" puts the differences
// into the draft like corrections made by hand.

import { useEffect, useRef, useState } from 'react'
import { ArrowCounterClockwiseIcon, ArrowsClockwiseIcon, CaretLeftIcon, CaretRightIcon, CrosshairIcon, FloppyDiskIcon, MagnifyingGlassMinusIcon, MagnifyingGlassPlusIcon, ScissorsIcon, TrashIcon } from '@phosphor-icons/react'
import { api } from '../api.js'
import { secs, slotColor } from '../format.js'
import { DETECT_MODES, channelMinRise, channelMode, computeLaps, modeInfo, notCountedReason, reasonHelp } from '../model.js'
import Stepper from './Stepper.jsx'

const HEIGHT = 340
const OVERVIEW_HEIGHT = 44
const PAD_LEFT = 34
const PAD_TOP = 34
const PAD_BOTTOM = 18
/** dBFS at the bottom of the graph. */
const MIN_DB = -62
/** How close (px) a tap or drag must start to a pass to grab it. */
const HIT_PX = 16
/** Trimming keeps this long after the last pass (`TRIM_AFTER_US` in core.rs). */
const TRIM_AFTER_US = 10e6
/** "Neu rechnen": a pass found this close to a saved one is the same pass. */
const SAME_PASS_US = 1e6

/**
 * Unsaved corrections of one channel.
 * @typedef {{moves: Record<number, number>, deletes: number[], restores: number[], adds: {key: string, t: number}[]}} Draft
 *
 * A pass as shown: `key` is "p<id>" for stored passes, "a…" for added ones.
 * `rejected`: deleted by the server, and why (see `Pass`).
 * @typedef {{key: string, id: number | null, t: number, kind: 'auto' | 'manual' | 'moved' | 'added' | 'deleted', rejected?: string | null, detected?: number | null}} Marker
 * `detected`: where the detector put the pass (null: added by hand).
 *
 * @typedef {object} Graph
 * @property {number[]} time_us
 * @property {(number | null)[]} level
 * @property {(number | null)[][]} boards
 * @property {string[]} board_names   name of each line in `boards` ("S1", or "B2")
 * @property {(number | null)[]} enter
 * @property {(number | null)[]} exit
 * @property {(number | null)[][]} [board_shift]   mode auto_relative: per line in `boards`, how many dB the board was shifted up for the combined line (null: it did not count); its own thresholds are enter/exit minus this
 * @typedef {{start_us: number | null, end_us: number | null, graph: Graph | null, passes?: RecalcPass[]}} GraphReply   `passes`: only from /recompute
 *
 * "Neu rechnen": what the channel is evaluated with, and a pass found that way.
 * @typedef {{mode: import('../types').DetectMode, enter_db: number, exit_db: number, min_rise_db: number, min_lap_s: number}} RecalcParams
 * @typedef {{time_us: number, peak_db: number, retracted: boolean}} RecalcPass   retracted: taken back by the detector later
 */

/** What the session itself used for a channel (finish gate); sessions from
 * before the settings were saved: the defaults.
 * @param {import('../types').Session} s @param {number} channel @returns {RecalcParams} */
function sessionParams(s, channel) {
  const d = s.config.detection
  const t = d?.finish[channel]
  return {
    mode: d ? channelMode(d, 'finish', channel) : 'auto',
    enter_db: t?.enter_db ?? -25,
    exit_db: t?.exit_db ?? -30,
    min_rise_db: d ? channelMinRise(d, 'finish', channel) : 12,
    min_lap_s: s.config.min_lap_s,
  }
}

/** The query for /recompute. @param {RecalcParams} p */
function recalcQuery(p) {
  const fixed = modeInfo(p.mode).manual ? `&enter_db=${p.enter_db}&exit_db=${p.exit_db}` : `&min_rise_db=${p.min_rise_db}`
  return `mode=${p.mode}${fixed}&min_lap_s=${p.min_lap_s}`
}

/** "Automatisch (experimentell) · 12 dB · Mindestrunde 3 s". @param {RecalcParams} p */
function paramsText(p) {
  const m = modeInfo(p.mode)
  const num = (/** @type {number} */ v) => String(v).replace('.', ',')
  const what = m.manual ? `Enter ${num(p.enter_db)} / Exit ${num(p.exit_db)} dB` : `Mindestausschlag ${num(p.min_rise_db)} dB`
  return `${m.label.replace(' (Enter/Exit)', '')} · ${what} · Mindestrunde ${num(p.min_lap_s)} s`
}

/**
 * The saved passes (as the draft has them) against the ones "Neu rechnen"
 * found: `added` are found ones without a counting pass within 1 s,
 * `missing` counting passes without a found one.
 * @param {Marker[]} markers @param {RecalcPass[]} found
 */
function compare(markers, found) {
  const counting = markers.filter((m) => m.kind !== 'deleted')
  const kept = found.filter((p) => !p.retracted)
  /** @param {number} t @param {number[]} ts */
  const near = (t, ts) => ts.some((x) => Math.abs(x - t) < SAME_PASS_US)
  return {
    saved: counting.length,
    found: kept.length,
    added: kept.filter((p) => !near(p.time_us, counting.map((m) => m.t))).map((p) => p.time_us),
    missing: counting.filter((m) => !near(m.t, kept.map((p) => p.time_us))),
  }
}


/** What happened to a saved pass, from the change log: "gelöscht (live) · wiederhergestellt".
 * @param {import('../types').PassChange[]} changes @param {number} id */
function historyOf(changes, id) {
  const words = { add: 'eingefügt', delete: 'gelöscht', restore: 'wiederhergestellt', move: 'verschoben', reject: 'abgelehnt' }
  return changes
    .filter((c) => c.pass_id === id)
    .map((c) => words[c.action] + (c.origin === 'server' ? ' vom Server' : '') + (c.live ? ' (live)' : ''))
    .join(' · ')
}

/** @returns {Draft} */
const emptyDraft = () => ({ moves: {}, deletes: [], restores: [], adds: [] })
/** @param {Draft | undefined} d */
const changes = (d) => (d ? Object.keys(d.moves).length + d.deletes.length + d.restores.length + d.adds.length : 0)

/** @param {import('../types').Pass[]} passes @param {Draft} d @returns {Marker[]} */
function markersOf(passes, d) {
  /** @type {Marker[]} */
  const out = passes.map((p) => {
    const stillDeleted = p.deleted && !d.restores.includes(p.id)
    const deleted = stillDeleted || d.deletes.includes(p.id)
    const t = d.moves[p.id] ?? p.time_us
    const detected = p.detected_us ?? null
    return {
      key: 'p' + p.id,
      id: p.id,
      t,
      // Moved: now (draft) or saved earlier; a pass moved back onto its detected time isn't.
      kind: deleted ? 'deleted' : detected == null ? 'manual' : t !== detected ? 'moved' : 'auto',
      rejected: stillDeleted ? (p.rejected ?? null) : null,
      detected,
    }
  })
  for (const a of d.adds) out.push({ key: a.key, id: null, t: a.t, kind: 'added' })
  return out.sort((a, b) => a.t - b.t)
}

/**
 * @param {object} props
 * @param {import('../types').SessionDetail} props.detail   with passes
 * @param {() => void} props.onSaved
 * @param {(n: number) => void} [props.onUnsaved]   how many changes are not saved yet (to ask before leaving)
 */
export default function PassEditor({ detail, onSaved, onUnsaved }) {
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
  // "Neu rechnen" for the channel shown: what was asked (null: the session's
  // settings), what came back, and whether the server is still at it.
  const [recalc, setRecalc] = useState(/** @type {RecalcParams | null} */ (null))
  const [recalcPasses, setRecalcPasses] = useState(/** @type {RecalcPass[] | null} */ (null))
  const [recalcBusy, setRecalcBusy] = useState(false)
  const [recalcError, setRecalcError] = useState('')

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
  const selectedHistory = selected?.id != null ? historyOf(detail.changes ?? [], selected.id) : ''
  // Why the selected pass doesn't count ("" if it does), as in the pass log.
  const selectedReason = selected
    ? notCountedReason({ id: selected.key, time_us: selected.t, deleted: selected.kind === 'deleted', rejected: selected.rejected }, preview)
    : ''
  // Laps under the minimum lap: only a hint, a pass set by hand counts anyway.
  const minLapUs = s.config.min_lap_s * 1e6
  const tooShort = preview.laps.filter((l) => l.duration_us < minLapUs).length
  const total = Object.values(drafts).reduce((n, d) => n + changes(d), 0)
  useEffect(() => onUnsaved?.(total), [total, onUnsaved])
  // Where trimming cuts, as the server does it: the last finish-gate pass
  // of any channel + 10 s. Only stored passes count (unsaved ones block it).
  const lastPass = Math.max(-Infinity, ...passes.filter((p) => !p.gate && !p.deleted).map((p) => p.time_us))
  const cutUs = Number.isFinite(lastPass) ? lastPass + TRIM_AFTER_US : null
  const diff = recalc && recalcPasses ? compare(markers, recalcPasses) : null

  /** Back to the session's settings (also when the channel changes). */
  function resetRecalc() {
    setRecalc(null)
    setRecalcPasses(null)
    setRecalcBusy(false)
    setRecalcError('')
  }

  /** The graph got the recomputed channel (or the server refused).
   * @param {GraphReply | null} reply @param {string} [message] */
  function onRecalc(reply, message) {
    setRecalcBusy(false)
    if (reply) return setRecalcPasses(reply.passes ?? [])
    // Refused (e.g. Exit above Enter): show why, the graph goes back.
    setRecalc(null)
    setRecalcPasses(null)
    setRecalcError(message ?? '')
  }

  /** The differences of "Neu rechnen" into the draft, as if made by hand:
   * found passes are added (a deleted one at that spot is restored
   * instead), detected passes that were not found again are deleted.
   * Passes set by hand stay. */
  function adopt() {
    if (!diff) return
    setDraft((d) => {
      const next = { ...d, adds: [...d.adds], deletes: [...d.deletes], restores: [...d.restores] }
      diff.added.forEach((t, i) => {
        const gone = markers.find((m) => m.kind === 'deleted' && m.id != null && Math.abs(m.t - t) < SAME_PASS_US)
        if (gone?.id == null) next.adds.push({ key: `a${Date.now()}-${i}`, t })
        else if (next.deletes.includes(gone.id)) next.deletes = next.deletes.filter((x) => x !== gone.id)
        else next.restores.push(gone.id)
      })
      for (const m of diff.missing) {
        if (m.detected == null) continue // set by hand: the race director knew why
        const id = /** @type {number} */ (m.id)
        if (next.restores.includes(id)) next.restores = next.restores.filter((x) => x !== id)
        else next.deletes.push(id)
      }
      return next
    })
  }

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
                resetRecalc()
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
        cutUs={confirmTrim ? cutUs : null}
        recalc={recalc ? recalcQuery(recalc) : null}
        onRecalc={onRecalc}
        preview={recalc && recalcPasses ? recalcPasses : []}
        waiting={recalcBusy}
      />

      <RecalcPanel
        key={channel}
        session={sessionParams(s, channel)}
        applied={recalc}
        busy={recalcBusy}
        error={recalcError}
        diff={diff}
        retracted={recalcPasses?.filter((p) => p.retracted).length ?? 0}
        onApply={(p) => {
          setRecalcError('')
          // The same settings again: the graph already shows them (and would
          // not ask the server again, so nothing would end the waiting).
          if (recalc && recalcQuery(recalc) === recalcQuery(p)) return
          setRecalcBusy(true)
          setRecalc(p)
        }}
        onReset={resetRecalc}
        onAdopt={adopt}
      />

      {selected && (
        <div className="graph-action">
          <span style={{ flex: 1, minWidth: 160 }}>
            Durchflug bei <b className="tnum">{secs(selected.t - t0)} s</b>
            {selected.kind === 'added' ? ' – neu' : selected.kind === 'manual' ? ' – von Hand' : ''}
            {selected.kind === 'moved' && selected.detected != null && (
              <> – verschoben, erkannt bei <span className="tnum">{secs(selected.detected - t0)} s</span></>
            )}
            {selectedHistory && <span className="muted small" style={{ display: 'block' }}>Bisher: {selectedHistory}</span>}
            {selectedReason && (
              <>
                {' – zählt nicht: '}
                <b>{selectedReason}</b>
                <span className="muted small" style={{ display: 'block' }}>
                  {reasonHelp(selectedReason)}
                </span>
              </>
            )}
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
        am nächsten Gipfel ein. Grün: erkannt, blau: von Hand oder verschoben, grau gestrichelt: gelöscht, gelb gepunktet: Freigabe-Gate,
        violette Raute unten: neu gerechnet (Vorschau).
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
          {total
            ? 'Erst speichern oder verwerfen.'
            : confirmTrim
              ? cutUs == null
                ? 'Keine Durchflüge – nichts zum Kürzen.'
                : `Schnitt bei ${secs(cutUs - t0)} s (rote Linie): Pegel und Durchflüge danach, alle Kanäle. Nicht rückgängig zu machen.`
              : 'Pegel spätestens 10 s nach dem letzten Durchflug abschneiden.'}
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
 * "Neu rechnen": evaluate the channel again with other settings, to look
 * at. Closed until asked for; the form starts with the session's settings.
 * @param {object} props
 * @param {RecalcParams} props.session   what the session used
 * @param {RecalcParams | null} props.applied   what the graph shows now (null: the session's)
 * @param {boolean} props.busy
 * @param {string} props.error
 * @param {ReturnType<typeof compare> | null} props.diff
 * @param {number} props.retracted   passes the detector took back in this run
 * @param {(p: RecalcParams) => void} props.onApply
 * @param {() => void} props.onReset
 * @param {() => void} props.onAdopt
 */
function RecalcPanel({ session, applied, busy, error, diff, retracted, onApply, onReset, onAdopt }) {
  const [open, setOpen] = useState(false)
  const [form, setForm] = useState(session)
  /** @param {Partial<RecalcParams>} part */
  const set = (part) => setForm((f) => ({ ...f, ...part }))
  const manual = modeInfo(form.mode).manual
  const badFixed = manual && form.exit_db >= form.enter_db
  // Only detected passes are deleted by "Übernehmen" (see `adopt`).
  const removable = diff ? diff.missing.filter((m) => m.detected != null).length : 0
  if (!open)
    return (
      <div className="recalc-closed">
        <button className="chip-btn" onClick={() => setOpen(true)}>
          <ArrowsClockwiseIcon size={16} /> Neu rechnen …
        </button>
        <span className="muted small">Diesen Kanal mit anderem Modus oder Mindestausschlag neu auswerten. Ändert nichts Gespeichertes.</span>
      </div>
    )
  return (
    <div className="recalc">
      <div className="recalc-head">
        <ArrowsClockwiseIcon size={16} />
        <b>Neu rechnen</b>
        <span className="muted small">Vorschau, ändert nichts Gespeichertes</span>
        <button
          className="btn btn-ghost"
          style={{ marginLeft: 'auto' }}
          onClick={() => {
            setOpen(false)
            if (applied) onReset()
          }}
        >
          Schließen
        </button>
      </div>
      <div className="seg seg-wrap">
        {DETECT_MODES.map((m) => (
          <button key={m.id} className={form.mode === m.id ? 'on' : ''} title={m.help} onClick={() => set({ mode: m.id })}>
            {m.label}
          </button>
        ))}
      </div>
      <span className="muted small">{modeInfo(form.mode).help}</span>
      <div className="recalc-fields">
        {manual ? (
          <>
            <Stepper label="Enter" value={form.enter_db} unit="dB" step={0.5} min={-60} max={0} onSet={(v) => set({ enter_db: v })} />
            <Stepper label="Exit" value={form.exit_db} unit="dB" step={0.5} min={-60} max={0} onSet={(v) => set({ exit_db: v })} />
          </>
        ) : (
          <Stepper label="Mindestausschlag" value={form.min_rise_db} unit="dB" step={1} min={3} max={30} onSet={(v) => set({ min_rise_db: v })} />
        )}
        <Stepper label="Mindestrunde" value={form.min_lap_s} unit="s" step={0.5} min={0.5} max={60} onSet={(v) => set({ min_lap_s: v })} />
      </div>
      {badFixed && <span className="error small">Exit muss unter Enter liegen.</span>}
      <div className="recalc-buttons">
        <button className="btn btn-primary" disabled={busy || badFixed} onClick={() => onApply(form)}>
          <ArrowsClockwiseIcon size={16} /> {busy ? 'Rechne …' : 'Neu rechnen'}
        </button>
        <button
          className="btn btn-secondary"
          disabled={busy || (!applied && JSON.stringify(form) === JSON.stringify(session))}
          onClick={() => {
            setForm(session)
            onReset()
          }}
        >
          Zurück zu den Session-Einstellungen
        </button>
      </div>
      {busy && <span className="small">Rechne neu … die Pegel-Datei läuft noch einmal durch die Erkennung (auf dem Pi einige Sekunden).</span>}
      {error && <span className="error small">{error}</span>}
      <span className="muted small">
        Der Graph zeigt: {applied ? <b className="recalc-word">Neuberechnung</b> : 'Session-Einstellungen'} · {paramsText(applied ?? session)}
      </span>
      {applied && diff && !busy && (
        <div className="recalc-result">
          <span className="tnum">
            gespeichert <b>{diff.saved}</b> · neu gerechnet <b className="recalc-word">{diff.found}</b>: +{diff.added.length} / −{diff.missing.length}
            {retracted > 0 && <span className="muted"> · {retracted} zurückgenommen</span>}
          </span>
          <button
            className="btn btn-secondary"
            disabled={diff.added.length + removable === 0}
            title="Neu gefundene einfügen, nicht mehr gefundene erkannte löschen – als Entwurf, gespeichert wird erst mit „Speichern“"
            onClick={onAdopt}
          >
            In den Entwurf übernehmen (+{diff.added.length} / −{removable})
          </button>
          <span className="muted small" style={{ flexBasis: '100%' }}>
            + neu gefunden, − gespeichert, aber nicht wieder gefunden (innerhalb 1 s gilt als derselbe). Von Hand gesetzte bleiben beim Übernehmen. Die
            Freigabe-Regel und Start/Ziel des Rennens gelten hier nicht: gezeigt wird, was der Detektor am Zielgate meldet.
          </span>
        </div>
      )}
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
 * @param {number | null} props.cutUs   where trimming would cut (shown while asking)
 * @param {string | null} props.recalc   "Neu rechnen": the query with the changed settings (null: the session's)
 * @param {(reply: GraphReply | null, message?: string) => void} props.onRecalc   its result arrived, or the server's refusal
 * @param {RecalcPass[]} props.preview   the passes of "Neu rechnen", shown as preview markers
 * @param {boolean} props.waiting   the server is recomputing
 */
function EditGraph({ sessionId, channel, t0, markers, arming, lapEnds, selected, onSelect, onMove, onTapEmpty, cutUs, recalc, onRecalc, preview, waiting }) {
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
  /** Which level to show: null = all (combined, boards faint), or one board by name. */
  const [board, setBoard] = useState(/** @type {string | null} */ (null))
  /** What a pointer press is doing: moving a pass, or the visible range. */
  const drag = useRef(/** @type {{x: number, moved: boolean, key: string | null, from: number, to: number} | null} */ (null))

  // The session's graph, or the channel recomputed with other settings
  // (same reply, plus the passes found).
  const source = recalc ? `/sessions/${sessionId}/recompute?channel=${channel}&${recalc}` : `/sessions/${sessionId}/graph?channel=${channel}`

  useEffect(() => {
    // `stale`: a newer request was sent meanwhile, drop this answer.
    let stale = false
    api(`${source}&points=${Math.round(width)}`)
      .then((/** @type {GraphReply} */ r) => {
        if (stale) return
        setOverview(r)
        if (r.start_us != null && r.end_us != null) {
          const [s, e] = [r.start_us, r.end_us]
          setRange((old) => old ?? [s, Math.min(e, s + 40e6)])
        }
        if (recalc) onRecalc(r)
      })
      .catch((e) => {
        if (stale) return
        // A refused recomputation goes back to the session's graph.
        if (recalc) onRecalc(null, e.message)
        else setError(e.message)
      })
    return () => {
      stale = true
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [source, Math.round(width / 100)])

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
    let stale = false
    const id = setTimeout(() => {
      // The server takes whole µs: computed ranges have fractions.
      const [a, b] = range.map(Math.round)
      api(`${source}&from_us=${a}&to_us=${b}&points=${Math.round(width)}`)
        .then((/** @type {GraphReply} */ r) => !stale && setDetail(r.graph))
        // (A refused recomputation is reported by the overview's request.)
        .catch((e) => !stale && !recalc && setError(e.message))
    }, 60) // while dragging, ask once the range rests
    return () => {
      stale = true
      clearTimeout(id)
    }
  }, [source, range, width, recalc])

  // Asking whether to trim: bring the cut into view (at 70 % of the width).
  useEffect(() => {
    if (cutUs == null || overview?.start_us == null || overview.end_us == null) return
    const [s, e] = [overview.start_us, overview.end_us]
    setRange((old) => {
      if (!old || (cutUs >= old[0] && cutUs <= old[1])) return old
      const len = Math.min(old[1] - old[0], e - s)
      const a = Math.max(s, Math.min(cutUs - len * 0.7, e - len))
      return [a, a + len]
    })
  }, [cutUs, overview])

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

  const names = detail?.board_names ?? []
  /** Index of the board shown alone, or -1 for all (also if it has no data here). */
  const single = board == null ? -1 : names.indexOf(board)

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
  // Mode auto_relative, one board alone: the thresholds belong to the
  // combined line of the shifted boards; minus the board's shift they fit
  // its own level (no line where the board did not count).
  const shifts = single >= 0 ? detail?.board_shift?.[single] : undefined
  /** @param {(number | null)[]} vs */
  const own = (vs) => (shifts ? vs.map((v, i) => (v == null || shifts[i] == null ? null : v - /** @type {number} */ (shifts[i]))) : vs)
  // "Neu rechnen": found passes without a counting saved one within 1 s are new.
  const counting = markers.filter((m) => m.kind !== 'deleted')
  const previews = preview
    .filter((p) => p.time_us >= from && p.time_us <= to)
    .map((p) => ({ ...p, isNew: !p.retracted && !counting.some((m) => Math.abs(m.t - p.time_us) < SAME_PASS_US) }))

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
      {waiting && <div className="graph-wait">Rechne neu …</div>}
      <Overview reply={overview} width={width} range={range} markers={markers} preview={preview} cutUs={cutUs} onPick={(t) => setRange(clamp(t - span / 2, t + span / 2))} />
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
            {single < 0 &&
              detail.boards.map((b, i) => (
                <path key={i} d={line(detail.time_us, b, x, y)} className="board" />
              ))}
            <path d={line(detail.time_us, own(detail.enter), x, y)} className="threshold" />
            <path d={line(detail.time_us, own(detail.exit), x, y)} className="threshold exit" />
            <path d={line(detail.time_us, single < 0 ? detail.level : detail.boards[single], x, y)} className="combined" />
          </>
        )}
        {armDetail && (
          <>
            <path d={line(armDetail.time_us, armDetail.enter, x, y)} className="arming-threshold" />
            <path d={line(armDetail.time_us, armDetail.level, x, y)} className="arming-level" />
          </>
        )}
        {cutUs != null && cutUs < to && (
          <g className="cut">
            <rect x={Math.max(PAD_LEFT, x(cutUs))} width={width - Math.max(PAD_LEFT, x(cutUs))} y={0} height={plotH} />
            {cutUs >= from && <line x1={x(cutUs)} x2={x(cutUs)} y1={0} y2={plotH} />}
          </g>
        )}
        {arming
          .filter((t) => t >= from && t <= to)
          .map((t) => (
            <line key={'arm' + t} x1={x(t)} x2={x(t)} y1={PAD_TOP} y2={plotH} className="arming-tick" />
          ))}
        {previews.map((p) => (
          // Below the saved markers (drawn after): where both agree, only the
          // diamond at the bottom shows; a new one has its own dashed line.
          <g key={'re' + p.time_us} className={'recalc-mark' + (p.retracted ? ' retracted' : p.isNew ? ' new' : '')}>
            <line x1={x(p.time_us)} x2={x(p.time_us)} y1={PAD_TOP - 8} y2={plotH - 14} />
            <path d={`M${x(p.time_us)},${plotH - 15}l6,6l-6,6l-6,-6z`} />
            <title>
              Neu gerechnet: {secs(p.time_us - t0)} s, Gipfel {p.peak_db.toFixed(1)} dBFS{p.retracted ? ' (vom Detektor zurückgenommen)' : p.isNew ? ' (neu)' : ''}
            </title>
          </g>
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
        {detail?.board_shift?.length ? (
          <span className="muted small" title="Modus „Automatisch (experimentell)“: Gesamt zeigt die Boards auf den Streckenpegel des lautesten angehoben. Bei einem einzelnen Board sind die Schwellen um dessen Verschiebung zurückgerechnet, passend zu seinem Pegel.">
            {single < 0 ? 'Gesamt: Boards auf einen Streckenpegel verschoben' : 'Schwellen für dieses Board umgerechnet'}
          </span>
        ) : null}
        {names.length > 1 && (
          <div className="seg" style={{ marginLeft: 'auto' }} title="Pegel aller Boards des Gates (Gesamt) oder eines einzelnen">
            {[null, ...names].map((n) => (
              <button key={n ?? ''} style={{ padding: '0 12px', whiteSpace: 'nowrap' }} className={(n === null ? single < 0 : names[single] === n) ? 'on' : ''} onClick={() => setBoard(n)}>
                {n ?? 'Gesamt'}
              </button>
            ))}
          </div>
        )}
      </div>
    </div>
  )
}

/**
 * The whole session as a strip; the visible range marked. Tap to go there.
 * @param {{reply: GraphReply, width: number, range: [number, number], markers: Marker[], preview: RecalcPass[], cutUs: number | null, onPick: (t: number) => void}} props
 */
function Overview({ reply, width, range, markers, preview, cutUs, onPick }) {
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
          <line key={m.key} x1={x(m.t)} x2={x(m.t)} y1={0} y2={preview.length ? OVERVIEW_HEIGHT / 2 : OVERVIEW_HEIGHT} className="pass-tick" />
        ))}
      {/* "Neu rechnen": saved passes in the upper half, found ones in the lower. */}
      {preview
        .filter((p) => !p.retracted)
        .map((p) => (
          <line key={'re' + p.time_us} x1={x(p.time_us)} x2={x(p.time_us)} y1={OVERVIEW_HEIGHT / 2} y2={OVERVIEW_HEIGHT} className="recalc-tick" />
        ))}
      {cutUs != null && cutUs < e && (
        <g className="cut">
          <rect x={x(cutUs)} width={width - x(cutUs)} y={0} height={OVERVIEW_HEIGHT} />
          <line x1={x(cutUs)} x2={x(cutUs)} y1={0} y2={OVERVIEW_HEIGHT} />
        </g>
      )}
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
