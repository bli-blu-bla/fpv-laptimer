// @ts-check
// One pilot: live signal, laps, best times; corrections by tap.

import { secs } from '../format.js'

/** The level bar spans this range, dBFS. */
const MIN_DB = -64
const MAX_DB = -10
/** Below this, only noise: the transmitter is off (as ON_DB in detect.rs). */
const ON_DB = -46

/** @param {number | null | undefined} db */
function pct(db) {
  if (db == null) return 0
  return Math.max(0, Math.min(100, ((db - MIN_DB) / (MAX_DB - MIN_DB)) * 100))
}

/**
 * @param {object} props
 * @param {string} props.label
 * @param {string} props.channel
 * @param {import('../types').ChannelLive | undefined} props.live
 * @param {import('../types').PilotResult} [props.result]   only during a session
 * @param {number} [props.consecutive]
 * @param {(passId: number) => void} [props.onDeletePass]
 * @param {() => void} [props.onAddPass]
 */
export default function PilotCard({ label, channel, live, result, consecutive, onDeletePass, onAddPass }) {
  const laps = result?.laps ?? []
  const last = laps.at(-1)
  return (
    <div className={'card' + (live?.in_pass ? ' gate' : '') + (result?.finished_us ? ' finished' : '')}>
      <div className="card-head">
        <span className="name">{label}</span>
        {channel !== label && <span className="chan">{channel}</span>}
      </div>
      <div className="level" title="Signal (Hüllkurve) mit Eintritts- und Austrittsschwelle">
        <div className="fill" style={{ width: pct(live?.level_db) + '%' }} />
        {live?.enter_db != null && <div className="mark enter" style={{ left: pct(live.enter_db) + '%' }} />}
        {live?.exit_db != null && <div className="mark exit" style={{ left: pct(live.exit_db) + '%' }} />}
      </div>
      <div className="level-text muted">
        {live?.level_db == null
          ? 'keine Daten'
          : live.level_db < ON_DB
            ? 'kein Sender'
            : `${live.level_db.toFixed(1)} dBFS` + (live.enter_db == null ? ' · lernt die Pegel' : '')}
      </div>
      {result && (
        <>
          <div className="stats">
            <div>
              <div className="big-num">{secs(last?.duration_us)}</div>
              <div className="muted">letzte</div>
            </div>
            <div>
              <div className="num">{laps.length}</div>
              <div className="muted">Runden</div>
            </div>
            <div>
              <div className="num">{secs(result.best_lap_us)}</div>
              <div className="muted">beste</div>
            </div>
            <div>
              <div className="num">{secs(result.best_consecutive?.[0])}</div>
              <div className="muted">beste {consecutive} in Folge</div>
            </div>
          </div>
          {result.finished_us && <div className="done">Fertig</div>}
          <ul className="laps">
            {[...laps].reverse().map((l) => (
              <li key={l.pass_id} className={l.duration_us === result.best_lap_us ? 'best' : ''}>
                <span className="lap-no">{l.number}.</span>
                <span>{secs(l.duration_us)}</span>
                {onDeletePass && (
                  <button className="x" title="Durchflug löschen (war keiner)" onClick={() => onDeletePass(l.pass_id)}>
                    ×
                  </button>
                )}
              </li>
            ))}
            {result.holeshot_us != null && <li className="muted">Holeshot {secs(result.holeshot_us)}</li>}
          </ul>
          {onAddPass && (
            <button className="add" title="Verpasster Durchflug in den letzten 2 Sekunden: am Gipfel eintragen" onClick={onAddPass}>
              + Durchflug eben verpasst
            </button>
          )}
        </>
      )}
    </div>
  )
}
