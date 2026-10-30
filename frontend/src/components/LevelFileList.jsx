// @ts-check
// Recordings and sessions with their level files, to play one instead of
// the boards (SourcePanel) or open it on the debug page (BoardDebug). One
// time line, grouped by day, with filters and a search; per entry its
// time, format, channels with pilots, boards and length (from the file's
// header, `GET /api/debug/session-files` and `info` of the recordings).
// A compact list: per entry a title line (kind, id, format) and a line of
// details; below 720 px of width each entry is a tile (container query in
// index.css). Design: "Aufnahme-Auswahl" on Claude Design.

import { FilmStripIcon, FlagCheckeredIcon, InfinityIcon, MagnifyingGlassIcon, PlayIcon } from '@phosphor-icons/react'
import { useEffect, useState } from 'react'
import { api } from '../api.js'
import { dateTime, secs, sessionKind, slotColor, time } from '../format.js'
import { standings } from '../model.js'

/**
 * What a level file holds.
 * @typedef {object} FileInfo
 * @property {string[]} channels   "R1@5658", …
 * @property {{index: number, id: string, name: string | null, role: import('../types').BoardRole}[]} boards
 * @property {number | null} duration_s
 *
 * @typedef {{name: string, size: number, recording: boolean, info?: FileInfo | null}} Recording
 *
 * One line of the list.
 * @typedef {object} Entry
 * @property {string} source   "rec:<name>" or "session:<id>"
 * @property {string} label    for the page that opens it
 * @property {'race' | 'training' | 'rec'} type
 * @property {number} at_us
 * @property {string} id       "#30", or the size of a recording
 * @property {string} kind     "Rennen", "Training", "Aufnahme"
 * @property {string} format   "Holeshot · 3 Runden · NITRO"
 * @property {{name: string, pilot: string, color: string}[]} chans
 * @property {FileInfo | null} info
 * @property {boolean} noLevels
 * @property {string} search   lower case, what the search looks in
 * @property {Recording} [rec]
 */

const FILTERS = /** @type {const} */ ([
  ['all', 'Alle'],
  ['race', 'Rennen'],
  ['training', 'Training'],
  ['rec', 'Aufnahmen'],
])

/**
 * @param {object} props
 * @param {string} props.action   the button: "Abspielen", "Öffnen"
 * @param {string | null} [props.current]   the source playing now (marked, no button)
 * @param {boolean} [props.disabled]
 * @param {boolean} [props.withRunning]   also the recording still being written
 * @param {(source: string, label: string) => void} props.onPick
 * @param {(rec: Recording, reload: () => void) => import('react').ReactNode} [props.recActions]   more buttons beside a recording's
 */
export default function LevelFileList({ action, current = null, disabled, withRunning, onPick, recActions }) {
  const [recs, setRecs] = useState(/** @type {Recording[] | null} */ (null))
  const [sessions, setSessions] = useState(/** @type {import('../types').SessionDetail[] | null} */ (null))
  const [infos, setInfos] = useState(/** @type {Record<string, FileInfo>} */ ({}))
  const [filter, setFilter] = useState(/** @type {string} */ ('all'))
  const [query, setQuery] = useState('')
  const [error, setError] = useState('')

  const loadRecs = () =>
    api('/debug/recordings')
      .then((r) => setRecs(r.recordings))
      .catch((e) => setError(e.message))
  useEffect(() => {
    loadRecs()
    api('/sessions')
      .then(setSessions)
      .catch((e) => setError(e.message))
    api('/debug/session-files')
      .then(setInfos)
      .catch(() => {})
  }, [])

  /** @type {Entry[]} */
  const entries = [
    ...(recs ?? []).filter((r) => withRunning || !r.recording).map((r) => recEntry(r)),
    ...(sessions ?? []).map((d) => sessionEntry(d, infos[d.session.id] ?? null)),
  ].sort((a, b) => b.at_us - a.at_us)
  const q = query.trim().toLowerCase()
  const shown = entries.filter((e) => (filter === 'all' || e.type === filter) && (!q || e.search.includes(q)))
  const groups = byDay(shown)

  return (
    <div className="lf">
      {error && <div className="error small">{error}</div>}
      <div className="lf-tools">
        <div className="lf-filters" role="group" aria-label="Filter">
          {FILTERS.map(([id, label]) => (
            <button key={id} className={'chip-btn' + (filter === id ? ' on' : '')} onClick={() => setFilter(id)}>
              {label} <span className="lf-count">{entries.filter((e) => id === 'all' || e.type === id).length}</span>
            </button>
          ))}
        </div>
        <label className="search lf-search">
          <MagnifyingGlassIcon size={16} />
          <input value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Pilot, Kanal oder #ID" aria-label="Suchen" />
        </label>
      </div>
      {recs && sessions && !shown.length && (
        <div className="muted small">{entries.length ? 'Nichts gefunden.' : 'Noch keine Aufnahmen oder Sessions – unter Debug → Live auf „Aufnehmen“ tippen.'}</div>
      )}
      <div className="lf-scroll">
        {groups.map((g) => (
          <div key={g.key}>
            <div className="lf-day">
              <span>{g.day}</span>
              {g.sub && <span className="lf-day-sub">{g.sub}</span>}
            </div>
            {g.entries.map((e) => {
              const playing = e.source === current
              return (
                <div key={e.source} className={'lf-row' + (playing ? ' on' : '') + (e.noLevels ? ' off' : '')}>
                  <span className="lf-icon">
                    {e.type === 'race' ? (
                      <FlagCheckeredIcon size={18} color="var(--accent)" aria-label="Rennen" />
                    ) : e.type === 'training' ? (
                      <InfinityIcon size={18} color="var(--n400)" aria-label="Training" />
                    ) : (
                      <FilmStripIcon size={18} color="var(--n400)" aria-label="Aufnahme" />
                    )}
                  </span>
                  <span className="lf-time">{time(e.at_us)}</span>
                  <span className="lf-main">
                    <span className="lf-title">
                      <span className="lf-kind">{e.kind}</span> {e.id} · {e.format}
                    </span>
                    <span className="lf-meta">
                      {e.chans.length > 0 && (
                        <span className="lf-chans" title={e.chans.map((c) => (c.pilot ? `${c.name}: ${c.pilot}` : c.name)).join(', ')}>
                          <span className="lf-dots">
                            {e.chans.map((c) => (
                              <span key={c.name} className="dot" style={{ background: c.color }} />
                            ))}
                          </span>
                          {e.chans.map((c) => c.name).join(' ')}
                        </span>
                      )}
                      {e.info?.boards.length ? <Boards info={e.info} /> : e.noLevels && <span>keine Pegel</span>}
                      {e.info?.duration_s != null && <span className="lf-dur">{minutes(e.info.duration_s)} min</span>}
                    </span>
                  </span>
                  <span className="lf-act">
                    {e.rec && recActions?.(e.rec, loadRecs)}
                    {playing ? (
                      <span className="lf-playing">
                        <span className="status-dot warn" /> läuft
                      </span>
                    ) : (
                      <button
                        className="btn btn-secondary"
                        disabled={disabled || e.noLevels}
                        aria-label={`${action}: ${e.kind} ${e.id}, ${time(e.at_us)}`}
                        onClick={() => onPick(e.source, e.label)}
                      >
                        {action === 'Abspielen' && <PlayIcon size={14} weight="fill" />} {action}
                      </button>
                    )}
                  </span>
                </div>
              )
            })}
          </div>
        ))}
      </div>
    </div>
  )
}

/** Finish boards green, arming boards yellow; their names if they have them. @param {{info: FileInfo}} props */
function Boards({ info }) {
  /** @param {import('../types').BoardRole} role */
  const of = (role) => info.boards.filter((b) => b.role === role)
  const finish = of('finish')
  const arming = of('arming')
  /** @param {FileInfo['boards']} bs */
  const names = (bs) => (bs.every((b) => b.name) ? bs.map((b) => b.name).join(' ') : '')
  return (
    <span className="lf-boards">
      <span className="lf-gate" title="Ziel-Boards">
        <span className="lf-gate-mark finish" />
        {finish.length}
        {names(finish) && <span className="lf-dim"> {names(finish)}</span>}
      </span>
      {arming.length > 0 && (
        <span className="lf-gate" title="Freigabe-Boards">
          <span className="lf-gate-mark arming" />
          {arming.length}
          {names(arming) && <span className="lf-dim"> {names(arming)}</span>}
        </span>
      )}
    </span>
  )
}

/** @param {Recording} r @returns {Entry} */
function recEntry(r) {
  const m = r.name.match(/^rec_(\d{4})-(\d\d)-(\d\d)_(\d\d)-(\d\d)-(\d\d)\.csv$/)
  const at = m ? new Date(+m[1], +m[2] - 1, +m[3], +m[4], +m[5], +m[6]).getTime() * 1000 : 0
  const chans = (r.info?.channels ?? []).map((c, i) => ({ name: c.split('@')[0], pilot: '', color: slotColor(i) }))
  return {
    source: 'rec:' + r.name,
    label: r.name,
    type: 'rec',
    at_us: at,
    id: `${(r.size / 1e6).toFixed(1).replace('.', ',')} MB`,
    kind: 'Aufnahme',
    format: r.recording ? 'läuft noch' : 'ohne Session',
    chans,
    info: r.info ?? null,
    noLevels: false,
    search: ['aufnahme', ...chans.map((c) => c.name)].join(' ').toLowerCase(),
    rec: r,
  }
}

/** @param {import('../types').SessionDetail} d @param {FileInfo | null} info @returns {Entry} */
function sessionEntry(d, info) {
  const s = d.session
  const st = standings(s, d.results)
  const first = st.findIndex((x) => x.pos === 1)
  const best = Math.min(...d.results.map((r) => r.best_lap_us ?? Infinity))
  const bestSeat = s.seats[d.results.findIndex((r) => r.best_lap_us === best)]
  const extra = s.config.race ? (first >= 0 ? s.seats[first].label : '') : bestSeat ? `${secs(best)} ${bestSeat.label}` : ''
  const chans = s.seats.map((seat) => ({ name: seat.channel_name, pilot: seat.label === seat.channel_name ? '' : seat.label, color: slotColor(seat.channel) }))
  return {
    source: 'session:' + s.id,
    label: `Session #${s.id} · ${dateTime(s.created_us)}`,
    type: s.config.race ? 'race' : 'training',
    at_us: s.created_us,
    id: `#${s.id}`,
    kind: s.config.race ? 'Rennen' : 'Training',
    format: [s.config.race ? sessionKind(s.config) : 'freie Runden', extra].filter(Boolean).join(' · '),
    chans,
    info,
    noLevels: s.levels_file == null,
    search: [`#${s.id}`, s.config.race ? 'rennen' : 'training', ...chans.flatMap((c) => [c.name, c.pilot])].join(' ').toLowerCase(),
  }
}

/** "6:12 min". @param {number} s */
function minutes(s) {
  const t = Math.round(s)
  return `${Math.floor(t / 60)}:${String(t % 60).padStart(2, '0')}`
}

/** The entries (newest first) by day: "Heute", "Gestern" with the date beside, else "Do, 29.10.".
 * @param {Entry[]} entries */
function byDay(entries) {
  /** @type {{key: string, day: string, sub: string, entries: Entry[]}[]} */
  const out = []
  const today = new Date()
  const yesterday = new Date(today.getTime() - 86400_000)
  for (const e of entries) {
    const d = new Date(e.at_us / 1000)
    const key = d.toDateString()
    let g = out.at(-1)
    if (!g || g.key !== key) {
      const date = d.toLocaleDateString('de-DE', { weekday: 'short', day: '2-digit', month: '2-digit' })
      const named = key === today.toDateString() ? 'Heute' : key === yesterday.toDateString() ? 'Gestern' : ''
      g = { key, day: named || date, sub: named ? date : '', entries: [] }
      out.push(g)
    }
    g.entries.push(e)
  }
  return out
}
