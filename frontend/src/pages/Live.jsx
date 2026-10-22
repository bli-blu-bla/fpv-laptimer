// @ts-check
// The page during a session: start/stop, a card per pilot, the ranking.

import { useEffect, useState } from 'react'
import { api, serverNow } from '../api.js'
import { clock, endRule, sessionKind, START_MODES } from '../format.js'
import PilotCard from '../components/PilotCard.jsx'
import Ranking from '../components/Ranking.jsx'

/**
 * @param {object} props
 * @param {ReturnType<typeof import('../api.js').useServer>} props.server
 * @param {import('../types').State} props.state
 */
export default function Live({ server, state }) {
  const { live, clockOffset } = server
  const active = state.active
  const [error, setError] = useState('')
  const now = useNow(clockOffset, active != null)

  /** @param {() => Promise<unknown>} action */
  async function run(action) {
    setError('')
    try {
      await action()
    } catch (e) {
      setError(String(/** @type {Error} */ (e).message))
    }
  }

  if (!active) {
    const r = state.settings.race
    return (
      <section className="start">
        <button className="big" onClick={() => run(() => api('/session/start', { body: { race: false } }))}>
          Training starten
        </button>
        <button className="big race" onClick={() => run(() => api('/session/start', { body: { race: true } }))}>
          Rennen starten
          <small>
            {START_MODES[r.start_mode]} · {endRule(r.end_rule)} · {r.countdown_s} s Countdown
          </small>
        </button>
        {error && <p className="error">{error}</p>}
        <div className="cards">
          {state.settings.channels.map((ch, i) => (
            <PilotCard key={i} label={seatLabel(state, i)} channel={ch.name} live={live?.channels[i]} />
          ))}
        </div>
      </section>
    )
  }

  const s = active.session
  const countdown = s.config.race && now < s.start_us
  const t0 = s.config.race ? s.start_us : s.created_us
  return (
    <section>
      <div className="session-bar">
        <span>{sessionKind(s.config)}</span>
        <span className="timer">{countdown ? Math.ceil((s.start_us - now) / 1e6) : clock(now - t0)}</span>
        <button className="stop" onClick={() => run(() => api('/session/stop', { body: {} }))}>
          Stopp
        </button>
      </div>
      {countdown && <div className="countdown">{Math.ceil((s.start_us - now) / 1e6)}</div>}
      {error && <p className="error">{error}</p>}
      <div className="cards">
        {s.seats.map((seat, i) => (
          <PilotCard
            key={seat.channel}
            label={seat.label}
            channel={seat.channel_name}
            live={live?.channels[seat.channel]}
            result={active.results[i]}
            consecutive={s.config.consecutive}
            onDeletePass={(id) => run(() => api(`/passes/${id}`, { body: { deleted: true } }))}
            onAddPass={() =>
              // The pass was a moment ago: look for its peak in the last 2 s
              // (the server snaps within ±1 s of the time given).
              run(() =>
                api(`/sessions/${s.id}/passes`, {
                  body: { channel: seat.channel, time_us: Math.round(serverNow(clockOffset) - 1e6), snap: true },
                }),
              )
            }
          />
        ))}
      </div>
      <Ranking session={s} results={active.results} />
    </section>
  )
}

/** What a channel's card is called before a session: the seat's setting.
 * @param {import('../types').State} state @param {number} i */
function seatLabel(state, i) {
  const seat = state.settings.seats[i]
  if (seat?.label.trim()) return seat.label.trim()
  const pilot = state.pilots.find((p) => p.id === seat?.pilot_id)
  return pilot?.name ?? state.settings.channels[i].name
}

/** The server time, updated 10 times a second while `running`.
 * @param {{current: number}} clockOffset @param {boolean} running */
function useNow(clockOffset, running) {
  const [now, setNow] = useState(() => serverNow(clockOffset))
  useEffect(() => {
    if (!running) return
    const id = setInterval(() => setNow(serverNow(clockOffset)), 100)
    return () => clearInterval(id)
  }, [clockOffset, running])
  return now
}
