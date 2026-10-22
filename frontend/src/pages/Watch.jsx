// @ts-check
// The spectator view: the running (or last) session, large and without
// any controls. Also as a full screen page of its own: "?view=zuschauer",
// for a TV or tablet at the field.

import { useEffect, useState } from 'react'
import { api, useNow } from '../api.js'
import { clock, sessionKind } from '../format.js'
import { slotsOf, standings } from '../model.js'
import Lane from '../components/Lane.jsx'
import Ranking from '../components/Ranking.jsx'

/**
 * @param {object} props
 * @param {ReturnType<typeof import('../api.js').useServer>} props.server
 * @param {import('../types').State} props.state
 */
export default function Watch({ server, state }) {
  const { live, clockOffset, changed } = server
  const active = state.active
  const now = useNow(clockOffset, active != null)
  // Without a running session: the last one.
  const [last, setLast] = useState(/** @type {import('../types').SessionDetail | null} */ (null))
  const activeId = active?.session.id
  useEffect(() => {
    if (activeId != null) return
    api('/sessions')
      .then((/** @type {import('../types').SessionDetail[]} */ list) => {
        const id = list[0]?.session.id
        return id != null ? api(`/sessions/${id}`) : null
      })
      .then(setLast)
      .catch(() => {})
  }, [activeId, changed])

  const detail = active ?? last
  const s = detail?.session
  const t0 = s ? (s.config.race ? s.start_us : s.created_us) : 0
  const countdown = active != null && s != null && s.config.race && now < s.start_us
  const elapsed = !s ? 0 : active ? (countdown ? 0 : now - t0) : (s.ended_us ?? t0) - t0
  const st = detail ? standings(detail.session, detail.results) : []
  const chip = !s ? ['Warte auf Start', ''] : countdown ? ['Countdown', 'live'] : active ? [s.config.race ? 'Läuft' : 'Training', 'live'] : ['Beendet', 'done']

  return (
    <>
      <div className="race-head">
        <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
            <span className={'chip ' + chip[1]}>
              <span className="blink" />
              {chip[0]}
            </span>
            {s && <span className="muted small">{sessionKind(s.config)}</span>}
          </div>
          <div className="clock">{clock(elapsed)}</div>
        </div>
      </div>

      <div className="lanes">
        {slotsOf(state)
          .filter((slot) => (s ? s.seats.some((x) => x.channel === slot.index) : slot.enabled))
          .map((slot) => {
            const i = s ? s.seats.findIndex((x) => x.channel === slot.index) : -1
            const seat = i >= 0 ? s?.seats[i] : undefined
            return (
              <Lane
                key={slot.index}
                color={slot.color}
                callsign={seat?.label ?? slot.callsign}
                fullName={seat ? (state.pilots.find((p) => p.id === seat.pilot_id)?.full_name ?? '') : slot.fullName}
                channel={seat?.channel_name ?? slot.channel}
                mhz={seat?.mhz ?? slot.mhz}
                result={i >= 0 ? detail?.results[i] : undefined}
                standing={i >= 0 ? st[i] : undefined}
                endRule={s?.config.race ? s.config.end_rule : undefined}
                live={active ? live?.channels[slot.index] : undefined}
              />
            )
          })}
      </div>

      {detail && (
        <div className="panel" style={{ padding: 16, display: 'flex', flexDirection: 'column', gap: 12 }}>
          <Ranking detail={detail} />
        </div>
      )}

      {countdown && s && (
        <div className="countdown">
          <div>
            <span className="kicker" style={{ fontSize: 14 }}>
              Startbereit machen
            </span>
            <span className="n">{Math.max(1, Math.ceil((s.start_us - now) / 1e6))}</span>
          </div>
        </div>
      )}
    </>
  )
}
