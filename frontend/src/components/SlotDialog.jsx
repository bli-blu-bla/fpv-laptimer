// @ts-check
// Settings of one slot, from its gear on the live page: who flies (a pilot,
// or a name typed in), with which of the pilot's presets, on which channel,
// or nobody. Takes effect at once and is saved; a pilot or channel used by
// another slot is swapped with it. The server copies the preset into the
// channel's detection (a pilot with one preset gets it on their own).

import { useState } from 'react'
import { saveSettings } from '../api.js'
import { RACEBAND, secs } from '../format.js'
import { presetSummary } from '../model.js'
import Dialog from './Dialog.jsx'

/**
 * @param {object} props
 * @param {import('../types').State} props.state
 * @param {import('../types').Stats | null} props.stats
 * @param {number} props.index
 * @param {string} props.color
 * @param {() => void} props.onClose
 */
export default function SlotDialog({ state, stats, index, color, onClose }) {
  const s = state.settings
  const seats = s.channels.map((_, i) => s.seats[i] ?? { pilot_id: null, label: '', enabled: true })
  const seat = seats[index]
  const running = state.active != null
  const [label, setLabel] = useState(seat.label)
  const [error, setError] = useState('')

  /** @param {Partial<import('../types').Settings>} patch */
  async function save(patch) {
    setError('')
    try {
      await saveSettings(s, patch)
    } catch (e) {
      setError(/** @type {Error} */ (e).message)
    }
  }

  /** Pilot for this slot; the slot that had them gets this slot's pilot.
   * Their presets go with them. @param {number | null} id */
  function pickPilot(id) {
    const next = seats.map((x) => ({ ...x }))
    const other = id == null ? -1 : next.findIndex((x, j) => j !== index && x.pilot_id === id)
    const preset = other >= 0 ? next[other].preset_id : null
    if (other >= 0) next[other] = { ...next[other], pilot_id: seat.pilot_id, preset_id: seat.preset_id }
    next[index] = { ...next[index], pilot_id: id, preset_id: preset, label: '', enabled: true }
    setLabel('')
    save({ seats: next })
  }

  /** @param {number | null} id */
  const pickPreset = (id) => save({ seats: seats.map((x, j) => (j === index ? { ...x, preset_id: id } : x)) })

  /** Channel for this slot; the slot that had it gets this slot's channel. @param {number} mhz */
  function pickChannel(mhz) {
    const name = `R${RACEBAND.indexOf(mhz) + 1}`
    const channels = s.channels.map((c) => ({ ...c }))
    const other = channels.findIndex((c, j) => j !== index && c.mhz === mhz)
    if (other >= 0) channels[other] = { ...channels[index] }
    channels[index] = { name, mhz }
    save({ channels })
  }

  function saveLabel() {
    if (label.trim() === seat.label.trim()) return
    const next = seats.map((x, j) => (j === index ? { ...x, label: label.trim(), pilot_id: label.trim() ? null : x.pilot_id } : x))
    save({ seats: next })
  }

  /** @param {boolean} enabled */
  const setEnabled = (enabled) => save({ seats: seats.map((x, j) => (j === index ? { ...x, enabled } : x)) })

  const pb = (/** @type {number} */ id) => stats?.all.find((p) => p.pilot_id === id)?.best_lap_us
  const ch = s.channels[index]
  const flying = seat.label.trim() ? null : seat.pilot_id
  const presets = state.presets.filter((p) => p.pilot_id === flying)
  return (
    <Dialog
      onClose={() => {
        saveLabel()
        onClose()
      }}
      title={
        <>
          <span className="dot" style={{ background: color }} /> Slot {index + 1} einstellen
        </>
      }
    >
      {running && <p className="muted small">Es läuft eine Session: Änderungen gelten ab dem nächsten Start, den Kanal erst danach.</p>}

      <div className="field">
        <span>Pilot</span>
        <div className="options" style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(120px, 1fr))' }}>
          {state.pilots.map((p) => {
            const on = seat.pilot_id === p.id && !seat.label.trim()
            const elsewhere = seats.findIndex((x, j) => j !== index && x.pilot_id === p.id)
            return (
              <button key={p.id} className={'option' + (on ? ' on' : elsewhere >= 0 ? ' taken' : '')} onClick={() => pickPilot(on ? null : p.id)}>
                <span className="opt-main">{p.name}</span>
                <span className="note">{on ? 'aktuell' : elsewhere >= 0 ? `in Slot ${elsewhere + 1}` : pb(p.id) ? `PB ${secs(pb(p.id))}` : p.full_name || ' '}</span>
              </button>
            )
          })}
        </div>
        {!state.pilots.length && <span className="muted small">Noch keine Piloten – unter „Piloten“ anlegen, oder hier einen Namen eintragen.</span>}
        <input className="input" value={label} placeholder={`oder Name eintragen (sonst „${ch.name}“)`} onChange={(e) => setLabel(e.target.value)} onBlur={saveLabel} />
      </div>

      {presets.length > 0 && (
        <div className="field">
          <span>Preset · Erkennung</span>
          <div className="options" style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(140px, 1fr))' }}>
            {presets.map((p) => (
              <button key={p.id} className={'option' + (seat.preset_id === p.id ? ' on' : '')} onClick={() => seat.preset_id !== p.id && pickPreset(p.id)}>
                <span className="opt-main">{p.name}</span>
                <span className="note">{presetSummary(p)}</span>
              </button>
            ))}
            <button className={'option' + (seat.preset_id == null ? ' on' : '')} onClick={() => seat.preset_id != null && pickPreset(null)}>
              <span className="opt-main">Keins</span>
              <span className="note">Kanal behält seine Werte</span>
            </button>
          </div>
        </div>
      )}

      <div className="field">
        <span>Kanal · Raceband</span>
        <div className="options" style={{ gridTemplateColumns: 'repeat(4, 1fr)' }}>
          {RACEBAND.map((f, k) => {
            const on = ch.mhz === f
            const taken = s.channels.some((c, j) => j !== index && c.mhz === f)
            return (
              <button key={f} disabled={running} className={'option center' + (on ? ' on' : taken ? ' taken' : '')} onClick={() => !on && pickChannel(f)}>
                <span className="opt-main">R{k + 1}</span>
                <span className="note tnum">{f}</span>
              </button>
            )
          })}
        </div>
        <span className="muted small">
          Grau: von einem anderen Slot belegt (wird getauscht).{RACEBAND.includes(ch.mhz) ? '' : ` Aktuell: ${ch.name} (${ch.mhz} MHz).`}
        </span>
      </div>

      <button className="row-toggle" onClick={() => setEnabled(!seat.enabled)}>
        <span className="text">
          <span>Slot belegt</span>
          <span className="hint">Aus: niemand fliegt hier, zählt nicht für Rangliste und Ansagen</span>
        </span>
        <span className={seat.enabled ? 'switch on' : 'switch'} />
      </button>

      {error && <p className="error">{error}</p>}
      <button
        className="btn btn-primary"
        onClick={() => {
          saveLabel()
          onClose()
        }}
      >
        Fertig
      </button>
    </Dialog>
  )
}
