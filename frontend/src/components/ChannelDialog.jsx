// @ts-check
// The channel of a slot, from a tap on the channel in its card: the current
// one with the IMD rating of all channels, then the band (R, A, B, E, F, L;
// opens on the current one's band) and its eight channels, or a free
// frequency. A channel used by another slot is swapped with it. A tap takes
// effect at once; locked while a session runs.

import { useState } from 'react'
import { saveSettings } from '../api.js'
import { slotColor } from '../format.js'
import { BANDS, channelName, imdRating } from '../imd.js'
import Dialog from './Dialog.jsx'
import ImdBadge from './ImdBadge.jsx'

/**
 * @param {object} props
 * @param {import('../types').State} props.state
 * @param {number} props.index
 * @param {() => void} props.onClose
 */
export default function ChannelDialog({ state, index, onClose }) {
  const s = state.settings
  const ch = s.channels[index]
  const running = state.active != null
  const [band, setBand] = useState(() => BANDS.find(([, , f]) => f.includes(ch.mhz))?.[0] ?? 'R')
  const [free, setFree] = useState('')
  const [error, setError] = useState('')

  const seat = s.seats[index]
  const who = seat?.label.trim() || state.pilots.find((p) => p.id === seat?.pilot_id)?.name || ''
  const bandInfo = BANDS.find(([b]) => b === band) ?? BANDS[0]
  const curBand = BANDS.find(([, , f]) => f.includes(ch.mhz))

  /** Channel for this slot; the slot that had it gets this slot's channel. @param {number} mhz */
  async function pick(mhz) {
    if (mhz === ch.mhz) return
    const channels = s.channels.map((c) => ({ ...c }))
    const other = channels.findIndex((c, j) => j !== index && c.mhz === mhz)
    if (other >= 0) channels[other] = { ...channels[index] }
    channels[index] = { name: channelName(mhz) ?? String(mhz), mhz }
    setError('')
    try {
      await saveSettings(s, { channels })
    } catch (e) {
      setError(/** @type {Error} */ (e).message)
    }
  }
  function pickFree() {
    const mhz = Math.round(Number(free.replace(',', '.')))
    if (!(mhz >= 5300 && mhz <= 6000)) return setError('Frequenz zwischen 5300 und 6000 MHz.')
    setFree('')
    pick(mhz)
  }

  return (
    <Dialog
      onClose={onClose}
      title={
        <span className="dialog-title">
          <span className="dot" style={{ background: slotColor(index) }} />
          <span>
            <span className="muted small">
              Slot {index + 1}
              {who ? ` · ${who}` : ''}
            </span>
            <span>Kanal wählen</span>
          </span>
        </span>
      }
    >
      {running && <p className="error small" style={{ margin: 0 }}>Während einer Session lässt sich der Kanal nicht ändern.</p>}
      <div className="inset ch-current">
        <span className="big">{ch.name === String(ch.mhz) ? 'Eigene' : ch.name}</span>
        <span style={{ display: 'flex', flexDirection: 'column', lineHeight: 1.3 }}>
          <span className="tnum">{ch.mhz} MHz</span>
          <span className="muted small">{curBand ? curBand[1] : 'freie Frequenz'}</span>
        </span>
        <span style={{ marginLeft: 'auto' }}>
          <ImdBadge rating={imdRating(s.channels.map((c) => c.mhz)).rating} />
        </span>
      </div>

      <div className="field">
        <span>Band</span>
        <div className="seg band-seg">
          {BANDS.map(([b, label, freqs]) => (
            <button key={b} className={band === b ? 'on' : ''} onClick={() => setBand(b)} title={label}>
              {b}
              {freqs.includes(ch.mhz) && <span className="band-mark" />}
            </button>
          ))}
        </div>
        <span className="muted small">{bandInfo[1]}{band === 'R' ? ' · haben fast alle Sender' : ''}</span>
      </div>

      <div className="ch-grid">
        {bandInfo[2].map((f, k) => {
          const on = f === ch.mhz
          const other = s.channels.findIndex((c, j) => j !== index && c.mhz === f)
          return (
            <button key={f} disabled={running} className={'option center' + (on ? ' on' : '')} onClick={() => pick(f)}>
              <span className="opt-main">
                {band}
                {k + 1}
              </span>
              <span className="note tnum">{f}</span>
              {other >= 0 && (
                <span className="note taken-by">
                  <span className="dot" style={{ width: 6, height: 6, background: slotColor(other) }} /> Slot {other + 1}
                </span>
              )}
            </button>
          )
        })}
      </div>

      <div className="field">
        <span>Andere Frequenz</span>
        <div style={{ display: 'flex', gap: 8 }}>
          <input
            className="input tnum"
            inputMode="numeric"
            placeholder="z. B. 5800"
            value={free}
            disabled={running}
            onChange={(e) => setFree(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && pickFree()}
            aria-label="Frequenz in MHz"
          />
          <button className="btn btn-secondary" style={{ whiteSpace: 'nowrap' }} disabled={running || !free} onClick={pickFree}>
            MHz setzen
          </button>
        </div>
      </div>
      <span className="muted small">Belegt von einem anderen Slot: die beiden tauschen. IMD bewertet alle Kanäle zusammen, wie in Einstellungen → Messung.</span>
      {error && <p className="error">{error}</p>}
    </Dialog>
  )
}
