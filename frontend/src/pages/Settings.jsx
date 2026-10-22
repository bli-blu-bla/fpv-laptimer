// @ts-check
// Settings: channels and who flies on them, pilots, race rules, detection.

import { useEffect, useState } from 'react'
import { api } from '../api.js'
import { START_MODES } from '../format.js'

/** Raceband, MHz. */
const RACEBAND = [5658, 5695, 5732, 5769, 5806, 5843, 5880, 5917]

/** @param {{state: import('../types').State}} props */
export default function SettingsPage({ state }) {
  const [s, setS] = useState(state.settings)
  const [msg, setMsg] = useState('')
  const dirty = JSON.stringify(s) !== JSON.stringify(state.settings)

  // Take over changes from elsewhere while nothing is edited here.
  useEffect(() => {
    if (!dirty) setS(state.settings)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [state.settings])

  /** Changes one part of the settings. @param {Partial<import('../types').Settings>} part */
  const set = (part) => setS({ ...s, ...part })

  async function save() {
    setMsg('Speichere …')
    try {
      await api('/settings', { method: 'PUT', body: s })
      setMsg('Gespeichert.')
    } catch (e) {
      setMsg('Fehler: ' + /** @type {Error} */ (e).message)
    }
  }

  /** @param {number} i @param {Partial<import('../types').ChannelSetting>} part */
  function setChannel(i, part) {
    set({ channels: s.channels.map((c, j) => (j === i ? { ...c, ...part } : c)) })
  }
  /** @param {number} i @param {Partial<import('../types').SeatSetting>} part */
  function setSeat(i, part) {
    const seats = s.channels.map((_, j) => s.seats[j] ?? { pilot_id: null, label: '' })
    seats[i] = { ...seats[i], ...part }
    set({ seats })
  }
  function addChannel() {
    const used = new Set(s.channels.map((c) => c.mhz))
    const mhz = RACEBAND.find((f) => !used.has(f)) ?? 5800
    const name = RACEBAND.includes(mhz) ? `R${RACEBAND.indexOf(mhz) + 1}` : String(mhz)
    set({ channels: [...s.channels, { name, mhz }], seats: [...s.seats, { pilot_id: null, label: '' }] })
  }
  /** @param {number} i */
  function removeChannel(i) {
    set({ channels: s.channels.filter((_, j) => j !== i), seats: s.seats.filter((_, j) => j !== i) })
  }

  const r = s.race
  return (
    <section className="settings">
      <h2>Kanäle und Piloten</h2>
      <table>
        <thead>
          <tr>
            <th>Kanal</th>
            <th>MHz</th>
            <th>Pilot</th>
            <th>oder Freitext</th>
            <th />
          </tr>
        </thead>
        <tbody>
          {s.channels.map((c, i) => (
            <tr key={i}>
              <td>
                <select
                  value={RACEBAND.includes(c.mhz) && c.name === `R${RACEBAND.indexOf(c.mhz) + 1}` ? c.mhz : ''}
                  onChange={(e) => {
                    const mhz = Number(e.target.value)
                    if (mhz) setChannel(i, { mhz, name: `R${RACEBAND.indexOf(mhz) + 1}` })
                  }}
                >
                  <option value="">{c.name}</option>
                  {RACEBAND.map((f, k) => (
                    <option key={f} value={f}>
                      R{k + 1}
                    </option>
                  ))}
                </select>
              </td>
              <td>
                <input
                  type="number"
                  className="narrow"
                  value={c.mhz}
                  onChange={(e) => setChannel(i, { mhz: Number(e.target.value), name: e.target.value })}
                />
              </td>
              <td>
                <select
                  value={s.seats[i]?.pilot_id ?? ''}
                  onChange={(e) => setSeat(i, { pilot_id: e.target.value ? Number(e.target.value) : null })}
                >
                  <option value="">–</option>
                  {state.pilots.map((p) => (
                    <option key={p.id} value={p.id}>
                      {p.name}
                    </option>
                  ))}
                </select>
              </td>
              <td>
                <input
                  value={s.seats[i]?.label ?? ''}
                  placeholder={c.name}
                  onChange={(e) => setSeat(i, { label: e.target.value })}
                />
              </td>
              <td>
                {s.channels.length > 1 && (
                  <button className="x" onClick={() => removeChannel(i)}>
                    ×
                  </button>
                )}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      {s.channels.length < 8 && <button onClick={addChannel}>+ Kanal</button>}
      <p className="muted">
        Beide ESPs scannen alle Kanäle. Neue Kanäle oder eine andere Verstärkung starten die Boards neu (eine
        Sekunde). Pilot und Freitext gelten ab der nächsten Session.
      </p>

      <h2>Rennen</h2>
      <label>
        Start
        <select value={r.start_mode} onChange={(e) => set({ race: { ...r, start_mode: /** @type {any} */ (e.target.value) } })}>
          {Object.entries(START_MODES).map(([k, v]) => (
            <option key={k} value={k}>
              {v}
            </option>
          ))}
        </select>
      </label>
      <label>
        Ende nach
        <input
          type="number"
          className="narrow"
          min={1}
          value={r.end_rule.kind === 'open' ? 3 : r.end_rule.value}
          onChange={(e) =>
            set({ race: { ...r, end_rule: { kind: r.end_rule.kind === 'seconds' ? 'seconds' : 'laps', value: Number(e.target.value) } } })
          }
        />
        <select
          value={r.end_rule.kind === 'seconds' ? 'seconds' : 'laps'}
          onChange={(e) =>
            set({
              race: {
                ...r,
                end_rule: {
                  kind: /** @type {'laps' | 'seconds'} */ (e.target.value),
                  value: r.end_rule.kind === 'open' ? 3 : r.end_rule.value,
                },
              },
            })
          }
        >
          <option value="laps">Runden</option>
          <option value="seconds">Sekunden</option>
        </select>
      </label>
      <label>
        Countdown
        <input
          type="number"
          className="narrow"
          min={0}
          max={30}
          value={r.countdown_s}
          onChange={(e) => set({ race: { ...r, countdown_s: Number(e.target.value) } })}
        />
        s
      </label>

      <h2>Auswertung</h2>
      <label>
        Mindestrunde
        <input
          type="number"
          className="narrow"
          min={1}
          step={0.5}
          value={s.min_lap_s}
          onChange={(e) => set({ min_lap_s: Number(e.target.value) })}
        />
        s
      </label>
      <label>
        Beste Runden in Folge
        <input
          type="number"
          className="narrow"
          min={2}
          max={20}
          value={s.consecutive}
          onChange={(e) => set({ consecutive: Number(e.target.value) })}
        />
      </label>
      <label>
        Verstärkung (Gain)
        <input
          type="number"
          className="narrow"
          min={0}
          max={60}
          value={s.gain}
          onChange={(e) => set({ gain: Number(e.target.value) })}
        />
        <span className="muted">30 bei 25 mW; kleiner, wenn es übersteuert</span>
      </label>

      <div className="save-bar">
        <button className="primary" disabled={!dirty} onClick={save}>
          Speichern
        </button>
        {dirty && <button onClick={() => setS(state.settings)}>Verwerfen</button>}
        <span className="muted">{msg}</span>
      </div>

      <Pilots pilots={state.pilots} />
      <Boards boards={state.boards} />
    </section>
  )
}

/** The pilot list. @param {{pilots: import('../types').Pilot[]}} props */
function Pilots({ pilots }) {
  const [name, setName] = useState('')
  const [error, setError] = useState('')
  /** @param {() => Promise<unknown>} f */
  const run = async (f) => {
    setError('')
    try {
      await f()
    } catch (e) {
      setError(/** @type {Error} */ (e).message)
    }
  }
  return (
    <>
      <h2>Piloten</h2>
      <ul className="pilots">
        {pilots.map((p) => (
          <li key={p.id}>
            <input
              defaultValue={p.name}
              onBlur={(e) => e.target.value.trim() !== p.name && run(() => api(`/pilots/${p.id}`, { method: 'PUT', body: { name: e.target.value } }))}
            />
            <button className="x" onClick={() => run(() => api(`/pilots/${p.id}`, { method: 'DELETE' }))}>
              ×
            </button>
          </li>
        ))}
      </ul>
      <form
        onSubmit={(e) => {
          e.preventDefault()
          if (name.trim()) run(() => api('/pilots', { body: { name } }).then(() => setName('')))
        }}
      >
        <input value={name} placeholder="Neuer Pilot" onChange={(e) => setName(e.target.value)} />
        <button type="submit">Hinzufügen</button>
      </form>
      {error && <p className="error">{error}</p>}
    </>
  )
}

/** Board status. @param {{boards: import('../types').Board[]}} props */
function Boards({ boards }) {
  return (
    <>
      <h2>Boards</h2>
      {!boards.length && <p className="warn">Keine Boards gefunden.</p>}
      <ul className="boards">
        {boards.map((b, i) => (
          <li key={i}>
            <b>{b.port}</b> <span className="muted">{b.info}</span>
            {b.failed && <div className="error">{b.failed}</div>}
          </li>
        ))}
      </ul>
    </>
  )
}
