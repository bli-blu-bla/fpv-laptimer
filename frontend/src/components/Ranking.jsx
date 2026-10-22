// @ts-check
// The ranking: in a race by laps, then by time; in training by best lap.

import { secs } from '../format.js'

/**
 * @param {object} props
 * @param {import('../types').Session} props.session
 * @param {import('../types').PilotResult[]} props.results
 */
export default function Ranking({ session, results }) {
  const rows = session.seats.map((seat, i) => ({ seat, r: results[i] })).filter((x) => x.r)
  if (session.config.race) {
    // More laps first; with equal laps, whoever got there earlier.
    rows.sort((a, b) => b.r.laps.length - a.r.laps.length || (a.r.total_us ?? Infinity) - (b.r.total_us ?? Infinity))
  } else {
    rows.sort((a, b) => (a.r.best_lap_us ?? Infinity) - (b.r.best_lap_us ?? Infinity))
  }
  const n = session.config.consecutive
  return (
    <table className="ranking">
      <thead>
        <tr>
          <th>#</th>
          <th>Pilot</th>
          <th>Runden</th>
          {session.config.race && <th>Zeit</th>}
          <th>Beste</th>
          <th>Beste {n}</th>
        </tr>
      </thead>
      <tbody>
        {rows.map(({ seat, r }, i) => (
          <tr key={seat.channel}>
            <td>{i + 1}</td>
            <td>
              {seat.label} {seat.label !== seat.channel_name && <span className="muted">{seat.channel_name}</span>}
              {r.finished_us ? ' ✓' : ''}
            </td>
            <td>{r.laps.length}</td>
            {session.config.race && <td>{secs(r.total_us)}</td>}
            <td>{secs(r.best_lap_us)}</td>
            <td>{secs(r.best_consecutive?.[0])}</td>
          </tr>
        ))}
      </tbody>
    </table>
  )
}
