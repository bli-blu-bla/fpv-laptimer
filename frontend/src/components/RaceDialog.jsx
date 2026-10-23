// @ts-check
// The race format, from the gear next to "Rennen starten": how it starts,
// when it ends, the countdown. Saved at once.

import { saveSettings } from '../api.js'
import { START_MODES } from '../format.js'
import Dialog from './Dialog.jsx'
import { MinusIcon, PlusIcon } from '@phosphor-icons/react'

const START_HINTS = {
  holeshot: 'Erster Durchflug nach dem Startton ist der Holeshot, danach zählen die Runden.',
  from_start: 'Runde 1 läuft vom Startton bis zum ersten Durchflug.',
  flying: 'Jeder startet seine Zeit mit dem eigenen ersten Durchflug.',
}

/** @param {{state: import('../types').State, onClose: () => void}} props */
export default function RaceDialog({ state, onClose }) {
  const s = state.settings
  const r = s.race
  /** @param {Partial<import('../types').Settings['race']>} patch */
  const set = (patch) => saveSettings(s, { race: { ...r, ...patch } })
  const byTime = r.end_rule.kind === 'seconds'
  const value = r.end_rule.kind === 'open' ? 3 : r.end_rule.value
  /** @param {number} v */
  const setValue = (v) => set({ end_rule: { kind: byTime ? 'seconds' : 'laps', value: Math.max(1, v) } })

  return (
    <Dialog title="Rennformat" onClose={onClose}>
      <div className="field">
        <span>Start</span>
        <div className="seg">
          {Object.entries(START_MODES).map(([k, label]) => (
            <button key={k} className={r.start_mode === k ? 'on' : ''} onClick={() => set({ start_mode: /** @type {any} */ (k) })}>
              {label}
            </button>
          ))}
        </div>
        <span className="muted small">{START_HINTS[r.start_mode]}</span>
      </div>

      <div className="field">
        <span>Ende</span>
        <div className="seg">
          <button className={!byTime ? 'on' : ''} onClick={() => set({ end_rule: { kind: 'laps', value: 3 } })}>
            Nach Runden
          </button>
          <button className={byTime ? 'on' : ''} onClick={() => set({ end_rule: { kind: 'seconds', value: 120 } })}>
            Nach Zeit
          </button>
        </div>
        <div className="stepper">
          <button className="btn btn-secondary btn-icon" onClick={() => setValue(value - (byTime ? 30 : 1))}>
            <MinusIcon />
          </button>
          <span className="value">{byTime ? `${Math.floor(value / 60)}:${String(value % 60).padStart(2, '0')}` : value}</span>
          <button className="btn btn-secondary btn-icon" onClick={() => setValue(value + (byTime ? 30 : 1))}>
            <PlusIcon />
          </button>
          <span className="muted small">{byTime ? 'Minuten – danach zählt die laufende Runde noch' : 'Runden'}</span>
        </div>
      </div>

      <div className="field">
        <span>Countdown</span>
        <div className="seg">
          {[0, 3, 5, 10].map((n) => (
            <button key={n} className={r.countdown_s === n ? 'on' : ''} onClick={() => set({ countdown_s: n })}>
              {n ? `${n} s` : 'aus'}
            </button>
          ))}
        </div>
      </div>
      <button className="btn btn-primary" onClick={onClose}>
        Fertig
      </button>
    </Dialog>
  )
}
