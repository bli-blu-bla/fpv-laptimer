// @ts-check
// One session in the list of the Sessions page: race or training, date,
// format, pilots, winner and best lap.

import { FlagCheckeredIcon, InfinityIcon } from '@phosphor-icons/react'
import { dateTime, secs, sessionKind } from '../format.js'
import { standings } from '../model.js'

/**
 * @param {object} props
 * @param {import('../types').SessionDetail} props.detail
 * @param {boolean} [props.on]   chosen (orange frame)
 * @param {import('react').ReactNode} [props.icon]   instead of the race/training icon (e.g. a checkbox)
 * @param {string} [props.note]   a line instead of winner and best lap (e.g. why it can't be opened)
 * @param {boolean} [props.disabled]
 * @param {() => void} props.onClick
 */
export default function SessionItem({ detail, on, icon, note, disabled, onClick }) {
  const s = detail.session
  const st = standings(s, detail.results)
  const first = st.findIndex((x) => x.pos === 1)
  const best = Math.min(...detail.results.map((r) => r.best_lap_us ?? Infinity))
  const bestSeat = s.seats[detail.results.findIndex((r) => r.best_lap_us === best)]
  return (
    <button className={'session-item' + (on ? ' on' : '')} disabled={disabled} onClick={onClick}>
      {icon ?? (s.config.race ? <FlagCheckeredIcon size={22} color="var(--accent)" /> : <InfinityIcon size={22} color="var(--n400)" />)}
      <span className="t">
        <span style={{ fontWeight: 500 }}>
          {dateTime(s.created_us)}
          {s.ended_us == null && <span style={{ color: 'var(--accent)' }}> · läuft</span>}
        </span>
        <span className="muted small">
          #{s.id} · {sessionKind(s.config)} · {s.seats.length} {s.seats.length === 1 ? 'Pilot' : 'Piloten'}
        </span>
      </span>
      <span className="r">
        {note ? (
          <span className="small muted">{note}</span>
        ) : (
          <>
            <span className="small">{s.config.race && first >= 0 ? s.seats[first].label : ''}</span>
            {bestSeat && (
              <span className="small best-color">
                {secs(best)} {bestSeat.label}
              </span>
            )}
          </>
        )}
      </span>
    </button>
  )
}
