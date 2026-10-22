// @ts-check
// Past sessions: the list, and the chosen one with its result; each pilot
// can be corrected on the level graph.

import { useEffect, useState } from 'react'
import { FlagCheckered, Infinity as InfinityIcon, PencilSimple, Trash } from '@phosphor-icons/react'
import { api } from '../api.js'
import { dateTime, secs, sessionKind, time } from '../format.js'
import { standings } from '../model.js'
import Dialog from '../components/Dialog.jsx'
import PassEditor from '../components/PassEditor.jsx'
import Ranking from '../components/Ranking.jsx'

/** @param {{changed: number, state: import('../types').State}} props */
export default function Sessions({ changed, state }) {
  const [list, setList] = useState(/** @type {import('../types').SessionDetail[] | null} */ (null))
  const [sel, setSel] = useState(/** @type {number | null} */ (null))
  // While correcting, the session takes the whole width (a big graph).
  const [editing, setEditing] = useState(false)
  const activeId = state.active?.session.id
  const passCount = state.active?.passes.length

  useEffect(() => {
    api('/sessions').then(setList).catch(() => setList([]))
  }, [changed, activeId, passCount])

  if (!list) return <p className="muted">Lade …</p>
  if (!list.length) return <p className="muted">Noch keine Sessions. Starte unter „Live“ ein Training oder Rennen.</p>
  const chosen = sel ?? list[0].session.id
  if (editing) return <SessionView key={chosen} id={chosen} changed={changed} editing onEditing={setEditing} />

  return (
    <div className="two-col" style={{ gridTemplateColumns: 'repeat(auto-fit, minmax(min(100%, 340px), 1fr))' }}>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
        <span className="kicker">{list.length} Sessions</span>
        {list.map((d) => {
          const s = d.session
          const st = standings(s, d.results)
          const first = st.findIndex((x) => x.pos === 1)
          const best = Math.min(...d.results.map((r) => r.best_lap_us ?? Infinity))
          const bestSeat = s.seats[d.results.findIndex((r) => r.best_lap_us === best)]
          return (
            <button key={s.id} className={'session-item' + (s.id === chosen ? ' on' : '')} onClick={() => setSel(s.id)}>
              {s.config.race ? <FlagCheckered size={22} color="var(--accent)" /> : <InfinityIcon size={22} color="var(--n400)" />}
              <span className="t">
                <span style={{ fontWeight: 500 }}>
                  {dateTime(s.created_us)}
                  {s.ended_us == null && <span style={{ color: 'var(--accent)' }}> · läuft</span>}
                </span>
                <span className="muted small">
                  {sessionKind(s.config)} · {s.seats.length} {s.seats.length === 1 ? 'Pilot' : 'Piloten'}
                </span>
              </span>
              <span className="r">
                <span className="small">{s.config.race && first >= 0 ? s.seats[first].label : ''}</span>
                {bestSeat && (
                  <span className="small best-color">
                    {secs(best)} {bestSeat.label}
                  </span>
                )}
              </span>
            </button>
          )
        })}
      </div>
      <SessionView key={chosen} id={chosen} changed={changed} editing={false} onEditing={setEditing} />
    </div>
  )
}

/** One session: result, lap times, correcting, deleting.
 * @param {{id: number, changed: number, editing: boolean, onEditing: (on: boolean) => void}} props */
function SessionView({ id, changed, editing, onEditing }) {
  const [d, setD] = useState(/** @type {import('../types').SessionDetail | null} */ (null))
  const [confirm, setConfirm] = useState(false)
  const [error, setError] = useState('')
  const reload = () => api(`/sessions/${id}`).then(setD).catch((e) => setError(e.message))
  useEffect(() => {
    reload()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id, changed])

  if (!d) return <div className="panel panel-body">{error || 'Lade …'}</div>
  const s = d.session
  return (
    <div className="panel" style={{ display: 'flex', flexDirection: 'column', gap: 12, padding: 16 }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap' }}>
        <div style={{ display: 'flex', flexDirection: 'column', lineHeight: 1.3 }}>
          <span style={{ fontSize: 18, fontWeight: 500 }}>{s.config.race ? 'Rennen' : 'Training'}</span>
          <span className="muted small">
            {dateTime(s.created_us)}
            {s.ended_us != null && `–${time(s.ended_us)}`} · {sessionKind(s.config)}
          </span>
        </div>
        {s.ended_us != null && (
          <button className="btn btn-ghost btn-icon" style={{ marginLeft: 'auto' }} title="Session löschen" onClick={() => setConfirm(true)}>
            <Trash size={18} />
          </button>
        )}
      </div>
      {error && <p className="error">{error}</p>}
      <Ranking detail={d} />
      {editing ? (
        <>
          <button className="btn btn-secondary" style={{ alignSelf: 'flex-start' }} onClick={() => onEditing(false)}>
            Bearbeiten schließen
          </button>
          <PassEditor detail={d} onSaved={reload} />
        </>
      ) : (
        <button className="btn btn-secondary" style={{ alignSelf: 'flex-start' }} onClick={() => onEditing(true)}>
          <PencilSimple size={16} /> Runden bearbeiten
        </button>
      )}
      {confirm && (
        <Dialog title="Session löschen?" onClose={() => setConfirm(false)}>
          <p className="muted" style={{ margin: 0 }}>
            Die Session wird mit allen Runden gelöscht.
          </p>
          <div style={{ display: 'flex', gap: 8, justifyContent: 'flex-end' }}>
            <button className="btn btn-secondary" onClick={() => setConfirm(false)}>
              Behalten
            </button>
            <button
              className="btn btn-danger"
              onClick={() =>
                api(`/sessions/${id}`, { method: 'DELETE' })
                  .then(() => setConfirm(false))
                  .catch((e) => setError(e.message))
              }
            >
              Löschen
            </button>
          </div>
        </Dialog>
      )}
    </div>
  )
}
