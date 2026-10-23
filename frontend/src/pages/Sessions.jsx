// @ts-check
// Past sessions: the list, and the chosen one with its result; each pilot
// can be corrected on the level graph. Export: the chosen session, or
// several picked in the list ("Auswählen").

import { useEffect, useState } from 'react'
import { CheckSquareIcon, DownloadSimpleIcon, FlagCheckeredIcon, InfinityIcon, PencilSimpleIcon, SquareIcon, TrashIcon } from '@phosphor-icons/react'
import { api } from '../api.js'
import { useAuth } from '../auth.js'
import { dateTime, secs, sessionKind, time } from '../format.js'
import { standings } from '../model.js'
import Dialog from '../components/Dialog.jsx'
import ExportDialog from '../components/ExportDialog.jsx'
import PassEditor from '../components/PassEditor.jsx'
import Ranking from '../components/Ranking.jsx'

/** @param {{changed: number, state: import('../types').State}} props */
export default function Sessions({ changed, state }) {
  const [list, setList] = useState(/** @type {import('../types').SessionDetail[] | null} */ (null))
  const [sel, setSel] = useState(/** @type {number | null} */ (null))
  // While correcting, the session takes the whole width (a big graph).
  const [editing, setEditing] = useState(false)
  // Picking sessions for one export: null = not picking.
  const [picked, setPicked] = useState(/** @type {number[] | null} */ (null))
  const [exporting, setExporting] = useState(/** @type {import('../types').Session[] | null} */ (null))
  const activeId = state.active?.session.id
  const passCount = state.active?.passes.length

  useEffect(() => {
    api('/sessions')
      .then(setList)
      .catch(() => setList([]))
  }, [changed, activeId, passCount])

  if (!list) return <p className="muted">Lade …</p>
  if (!list.length) return <p className="muted">Noch keine Sessions. Starte unter „Live“ ein Training oder Rennen.</p>
  const chosen = sel ?? list[0].session.id
  const exportDialog = exporting && <ExportDialog sessions={exporting} onClose={() => setExporting(null)} />
  if (editing)
    return (
      <>
        <SessionView key={chosen} id={chosen} changed={changed} editing onEditing={setEditing} onExport={setExporting} />
        {exportDialog}
      </>
    )
  /** @param {number} id */
  const togglePick = (id) => picked && setPicked(picked.includes(id) ? picked.filter((x) => x !== id) : [...picked, id])

  return (
    <div className="two-col" style={{ gridTemplateColumns: 'repeat(auto-fit, minmax(min(100%, 340px), 1fr))' }}>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 6, minHeight: 36 }}>
          <span className="kicker">{picked ? `${picked.length} von ${list.length} gewählt` : `${list.length} Sessions`}</span>
          {picked ? (
            <>
              <button
                className="btn btn-ghost"
                style={{ marginLeft: 'auto' }}
                onClick={() => setPicked(picked.length === list.length ? [] : list.map((d) => d.session.id))}
              >
                {picked.length === list.length ? 'Keine' : 'Alle'}
              </button>
              <button className="btn btn-ghost" onClick={() => setPicked(null)}>
                Fertig
              </button>
              <button
                className="btn btn-primary"
                disabled={!picked.length}
                onClick={() => setExporting(list.filter((d) => picked.includes(d.session.id)).map((d) => d.session))}
              >
                <DownloadSimpleIcon size={16} /> Exportieren
              </button>
            </>
          ) : (
            <button className="btn btn-ghost" style={{ marginLeft: 'auto' }} onClick={() => setPicked([])} title="Mehrere Sessions für einen Export wählen">
              <CheckSquareIcon size={16} /> Auswählen
            </button>
          )}
        </div>
        {list.map((d) => {
          const s = d.session
          const st = standings(s, d.results)
          const first = st.findIndex((x) => x.pos === 1)
          const best = Math.min(...d.results.map((r) => r.best_lap_us ?? Infinity))
          const bestSeat = s.seats[d.results.findIndex((r) => r.best_lap_us === best)]
          return (
            <button
              key={s.id}
              className={'session-item' + ((picked ? picked.includes(s.id) : s.id === chosen) ? ' on' : '')}
              onClick={() => (picked ? togglePick(s.id) : setSel(s.id))}
            >
              {picked ? (
                picked.includes(s.id) ? (
                  <CheckSquareIcon size={22} weight="fill" color="var(--accent)" />
                ) : (
                  <SquareIcon size={22} color="var(--n400)" />
                )
              ) : s.config.race ? (
                <FlagCheckeredIcon size={22} color="var(--accent)" />
              ) : (
                <InfinityIcon size={22} color="var(--n400)" />
              )}
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
      <SessionView key={chosen} id={chosen} changed={changed} editing={false} onEditing={setEditing} onExport={setExporting} />
      {exportDialog}
    </div>
  )
}

/** One session: result, lap times, correcting, exporting, deleting.
 * @param {{id: number, changed: number, editing: boolean, onEditing: (on: boolean) => void, onExport: (s: import('../types').Session[]) => void}} props */
function SessionView({ id, changed, editing, onEditing, onExport }) {
  const { canEdit } = useAuth()
  const [d, setD] = useState(/** @type {import('../types').SessionDetail | null} */ (null))
  const [confirm, setConfirm] = useState(false)
  const [error, setError] = useState('')
  const reload = () =>
    api(`/sessions/${id}`)
      .then(setD)
      .catch((e) => setError(e.message))
  useEffect(() => {
    reload()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id, changed])

  if (!d) return <div className="panel panel-body">{error || 'Lade …'}</div>
  const s = d.session
  return (
    <div className="panel" style={{ display: 'flex', flexDirection: 'column', gap: 12, padding: 16 }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap' }}>
        <div style={{ display: 'flex', flexDirection: 'column', lineHeight: 1.3, flex: 1, minWidth: 0 }}>
          <span style={{ fontSize: 18, fontWeight: 500 }}>{s.config.race ? 'Rennen' : 'Training'}</span>
          <span className="muted small">
            {dateTime(s.created_us)}
            {s.ended_us != null && `–${time(s.ended_us)}`} · {sessionKind(s.config)}
          </span>
        </div>
        <span style={{ display: 'flex', marginLeft: 'auto' }}>
          <button className="btn btn-ghost btn-icon" title="Exportieren (CSV, JSON)" onClick={() => onExport([s])}>
            <DownloadSimpleIcon size={18} />
          </button>
          {canEdit && s.ended_us != null && (
            <button className="btn btn-ghost btn-icon" title="Session löschen" onClick={() => setConfirm(true)}>
              <TrashIcon size={18} />
            </button>
          )}
        </span>
      </div>
      {error && <p className="error">{error}</p>}
      <Ranking detail={d} />
      {editing && canEdit ? (
        <>
          <button className="btn btn-secondary" style={{ alignSelf: 'flex-start' }} onClick={() => onEditing(false)}>
            Bearbeiten schließen
          </button>
          <PassEditor detail={d} onSaved={reload} />
        </>
      ) : (
        canEdit && <button className="btn btn-secondary" style={{ alignSelf: 'flex-start' }} onClick={() => onEditing(true)}>
          <PencilSimpleIcon size={16} /> Runden bearbeiten
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
