// @ts-check
// Past sessions: the list, and one session with its laps and passes to correct.

import { useEffect, useState } from 'react'
import { api } from '../api.js'
import { dateTime, secs, sessionKind } from '../format.js'
import Ranking from '../components/Ranking.jsx'

/**
 * @param {object} props
 * @param {number} props.changed   counts changes announced by the server
 * @param {import('../types').State} props.state
 */
export default function Sessions({ changed, state }) {
  const [list, setList] = useState(/** @type {import('../types').SessionDetail[]} */ ([]))
  const [open, setOpen] = useState(/** @type {number | null} */ (null))
  const activeId = state.active?.session.id

  useEffect(() => {
    api('/sessions').then(setList).catch(() => {})
  }, [changed, activeId])

  if (open != null) return <SessionView id={open} changed={changed} onBack={() => setOpen(null)} />
  if (!list.length) return <p className="muted">Noch keine Sessions.</p>
  return (
    <ul className="session-list">
      {list.map(({ session: s, results }) => (
        <li key={s.id} onClick={() => setOpen(s.id)}>
          <div>
            <b>{dateTime(s.created_us)}</b> · {sessionKind(s.config)}
            {s.ended_us == null && <span className="warn"> · läuft</span>}
          </div>
          <div className="muted">
            {s.seats
              .map((seat, i) => ({ seat, r: results[i] }))
              .filter(({ r }) => r?.laps.length)
              .map(({ seat, r }) => `${seat.label}: ${r.laps.length} Runden, beste ${secs(r.best_lap_us)}`)
              .join(' · ') || 'keine Runden'}
          </div>
        </li>
      ))}
    </ul>
  )
}

/**
 * One session: ranking, and per pilot every pass, to delete or restore.
 * @param {object} props
 * @param {number} props.id
 * @param {number} props.changed
 * @param {() => void} props.onBack
 */
function SessionView({ id, changed, onBack }) {
  const [d, setD] = useState(/** @type {import('../types').SessionDetail | null} */ (null))
  const [confirm, setConfirm] = useState(false)
  const [error, setError] = useState('')

  useEffect(() => {
    api(`/sessions/${id}`).then(setD).catch((e) => setError(e.message))
  }, [id, changed])

  /** @param {number} passId @param {boolean} deleted */
  async function setDeleted(passId, deleted) {
    await api(`/passes/${passId}`, { body: { deleted } })
    setD(await api(`/sessions/${id}`))
  }

  async function remove() {
    try {
      await api(`/sessions/${id}`, { method: 'DELETE' })
      onBack()
    } catch (e) {
      setError(/** @type {Error} */ (e).message)
    }
  }

  if (!d) return <p className="muted">{error || 'Lade …'}</p>
  const s = d.session
  const t0 = s.config.race ? s.start_us : s.created_us
  return (
    <section>
      <button onClick={onBack}>← Zurück</button>
      <h2>
        {dateTime(s.created_us)} · {sessionKind(s.config)}
      </h2>
      {error && <p className="error">{error}</p>}
      <Ranking session={s} results={d.results} />
      <div className="cards">
        {s.seats.map((seat, i) => {
          const r = d.results[i]
          const passes = (d.passes ?? []).filter((p) => p.channel === seat.channel)
          if (!passes.length) return null
          /** @type {Map<number, number>} pass id → lap that it ended */
          const lapOf = new Map(r.laps.map((l) => [l.pass_id, l.number]))
          return (
            <div className="card" key={seat.channel}>
              <div className="card-head">
                <span className="name">{seat.label}</span>
                <span className="chan">{seat.channel_name}</span>
              </div>
              <table className="passes">
                <tbody>
                  {passes.map((p) => {
                    const lap = r.laps.find((l) => l.pass_id === p.id)
                    return (
                      <tr key={p.id} className={p.deleted ? 'deleted' : ''}>
                        <td className="muted">{secs(p.time_us - t0)} s</td>
                        <td>
                          {p.deleted
                            ? 'gelöscht'
                            : lap
                              ? `Runde ${lapOf.get(p.id)}: ${secs(lap.duration_us)}`
                              : r.ignored.includes(p.id)
                                ? 'zählt nicht'
                                : 'Start'}
                          {p.manual ? ' (von Hand)' : ''}
                        </td>
                        <td>
                          <button className="x" onClick={() => setDeleted(p.id, !p.deleted)}>
                            {p.deleted ? '↺' : '×'}
                          </button>
                        </td>
                      </tr>
                    )
                  })}
                </tbody>
              </table>
            </div>
          )
        })}
      </div>
      {s.ended_us != null &&
        (confirm ? (
          <p>
            Session mit allen Runden löschen?{' '}
            <button className="stop" onClick={remove}>
              Ja, löschen
            </button>{' '}
            <button onClick={() => setConfirm(false)}>Nein</button>
          </p>
        ) : (
          <button onClick={() => setConfirm(true)}>Session löschen …</button>
        ))}
    </section>
  )
}
