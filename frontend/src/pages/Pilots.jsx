// @ts-check
// The pilot list: callsign and name, personal best, races and wins; in a
// pilot's dialog their presets (renamed or deleted here; made in race control
// at a slot's detection). Who flies in which slot is set at the slot's gear
// on the live page.

import { useState } from 'react'
import { TrashIcon, UserPlusIcon } from '@phosphor-icons/react'
import { api } from '../api.js'
import { useAuth } from '../auth.js'
import { initials, secs, slotColor } from '../format.js'
import { presetSummary } from '../model.js'
import Dialog from '../components/Dialog.jsx'

/** @param {{state: import('../types').State, stats: import('../types').Stats | null}} props */
export default function Pilots({ state, stats }) {
  const [edit, setEdit] = useState(/** @type {import('../types').Pilot | 'new' | null} */ (null))
  const { canEdit } = useAuth()
  return (
    <>
      <div style={{ display: 'flex', flexWrap: 'wrap', gap: 10, alignItems: 'center' }}>
        <span className="muted small">Wer in welchem Slot fliegt, stellst du in der Live-Ansicht am Zahnrad des Slots ein.</span>
        {canEdit && (
          <button className="btn btn-primary" style={{ marginLeft: 'auto' }} onClick={() => setEdit('new')}>
            <UserPlusIcon size={18} /> Pilot hinzufügen
          </button>
        )}
      </div>
      {!state.pilots.length && <p className="muted">Noch keine Piloten.</p>}
      <div className="roster">
        {state.pilots.map((p) => {
          const st = stats?.all.find((x) => x.pilot_id === p.id)
          const slot = state.settings.seats.findIndex((s) => s.pilot_id === p.id && s.enabled !== false && !s.label.trim())
          const color = slot >= 0 ? slotColor(slot) : undefined
          const presets = state.presets.filter((x) => x.pilot_id === p.id).length
          return (
            <button key={p.id} className="pilot" style={color ? { boxShadow: `0 0 0 1px ${color}55` } : undefined} disabled={!canEdit} onClick={() => setEdit(p)}>
              <span style={{ display: 'flex', alignItems: 'center', gap: 12, width: '100%' }}>
                <span className="avatar" style={color ? { borderColor: color, color } : undefined}>
                  {initials(p.full_name, p.name)}
                </span>
                <span style={{ display: 'flex', flexDirection: 'column', lineHeight: 1.25, minWidth: 0 }}>
                  <span style={{ fontWeight: 600 }}>{p.name}</span>
                  <span className="muted small">{p.full_name}</span>
                </span>
                <span className="small" style={{ marginLeft: 'auto', color: color ?? 'var(--n600)' }}>
                  {slot >= 0 ? `Slot ${slot + 1}` : ''}
                </span>
              </span>
              <span className="muted small tnum" style={{ display: 'flex', justifyContent: 'space-between', width: '100%' }}>
                <span>
                  {st?.races ?? 0} Rennen · {st?.wins ?? 0} {st?.wins === 1 ? 'Sieg' : 'Siege'}
                  {presets ? ` · ${presets} ${presets === 1 ? 'Preset' : 'Presets'}` : ''}
                </span>
                <span>
                  PB <span className="best-color">{secs(st?.best_lap_us)}</span>
                </span>
              </span>
            </button>
          )
        })}
      </div>
      {edit && <PilotDialog pilot={edit === 'new' ? null : edit} presets={edit === 'new' ? [] : state.presets.filter((x) => x.pilot_id === edit.id)} onClose={() => setEdit(null)} />}
    </>
  )
}

/** Add or edit a pilot.
 * @param {{pilot: import('../types').Pilot | null, presets: import('../types').Preset[], onClose: () => void}} props */
function PilotDialog({ pilot, presets, onClose }) {
  const [name, setName] = useState(pilot?.name ?? '')
  const [fullName, setFullName] = useState(pilot?.full_name ?? '')
  const [confirm, setConfirm] = useState(false)
  const [error, setError] = useState('')
  /** @param {() => Promise<unknown>} f */
  const run = async (f) => {
    setError('')
    try {
      await f()
      onClose()
    } catch (e) {
      setError(/** @type {Error} */ (e).message)
    }
  }
  const body = { name, full_name: fullName }
  return (
    <Dialog title={pilot ? pilot.name : 'Neuer Pilot'} onClose={onClose}>
      <form
        style={{ display: 'flex', flexDirection: 'column', gap: 14 }}
        onSubmit={(e) => {
          e.preventDefault()
          run(() => (pilot ? api(`/pilots/${pilot.id}`, { method: 'PUT', body }) : api('/pilots', { body })))
        }}
      >
        <label className="field">
          <span>Callsign</span>
          <input className="input" value={name} autoFocus onChange={(e) => setName(e.target.value)} placeholder="z. B. NITRO" />
        </label>
        <label className="field">
          <span>Name (optional)</span>
          <input className="input" value={fullName} onChange={(e) => setFullName(e.target.value)} />
        </label>
        {pilot && (
          <div className="field">
            <span>Presets · Erkennung</span>
            {presets.map((p) => (
              <PresetItem key={p.id} preset={p} onError={setError} />
            ))}
            <span className="muted small">
              {presets.length ? 'Mit nur einem Preset wird es beim Zuweisen zum Slot gleich übernommen.' : 'Noch keine.'} Neue legst du in der Rennleitung an: Slot antippen, Erkennung einstellen, „Neues Preset“.
            </span>
          </div>
        )}
        {error && <p className="error">{error}</p>}
        <div style={{ display: 'flex', gap: 8 }}>
          {pilot &&
            (confirm ? (
              <button type="button" className="btn btn-danger" onClick={() => run(() => api(`/pilots/${pilot.id}`, { method: 'DELETE' }))}>
                Wirklich löschen
              </button>
            ) : (
              <button type="button" className="btn btn-ghost" onClick={() => setConfirm(true)}>
                Löschen
              </button>
            ))}
          <button type="submit" className="btn btn-primary" style={{ marginLeft: 'auto' }} disabled={!name.trim()}>
            Speichern
          </button>
        </div>
        {pilot && confirm && <span className="muted small">Alte Sessions behalten den Namen.</span>}
      </form>
    </Dialog>
  )
}

/** One preset in the pilot dialog: rename (saved on leaving the field), delete.
 * @param {{preset: import('../types').Preset, onError: (msg: string) => void}} props */
function PresetItem({ preset, onError }) {
  const [name, setName] = useState(preset.name)
  const [confirm, setConfirm] = useState(false)
  /** @param {() => Promise<unknown>} f */
  const run = (f) => f().catch((e) => onError(/** @type {Error} */ (e).message))
  function rename() {
    if (!name.trim() || name.trim() === preset.name) return setName(preset.name)
    onError('')
    run(() => api(`/presets/${preset.id}`, { method: 'PUT', body: { name } }))
  }
  return (
    <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
      <input
        className="input"
        style={{ flex: '1 1 0', minWidth: 0 }}
        value={name}
        onChange={(e) => setName(e.target.value)}
        onBlur={rename}
        onKeyDown={(e) => {
          if (e.key === 'Enter') {
            e.preventDefault() // not the pilot form
            e.currentTarget.blur()
          }
        }}
      />
      <span className="muted small" style={{ whiteSpace: 'nowrap' }}>
        {presetSummary(preset)}
      </span>
      {confirm ? (
        <button type="button" className="btn btn-danger" onClick={() => run(() => api(`/presets/${preset.id}`, { method: 'DELETE' }))}>
          Löschen
        </button>
      ) : (
        <button type="button" className="btn btn-ghost btn-icon" title="Preset löschen" onClick={() => setConfirm(true)}>
          <TrashIcon size={18} />
        </button>
      )}
    </div>
  )
}
