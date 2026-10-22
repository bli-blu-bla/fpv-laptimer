// @ts-check
// One slot on the live page: who flies, position, last and best lap, gap,
// and the signal of the last seconds. The gear opens the slot settings.

import { FlagCheckered, GearSix } from '@phosphor-icons/react'
import { secs } from '../format.js'
import Sparkline from './Sparkline.jsx'

/** Below this, only noise: the transmitter is off (as ON_DB in detect.rs). */
const ON_DB = -46

/**
 * @param {object} props
 * @param {string} props.color
 * @param {string} props.callsign
 * @param {string} props.fullName
 * @param {string} props.channel
 * @param {number} props.mhz
 * @param {boolean} [props.off]                    slot not used
 * @param {import('../types').PilotResult} [props.result]   during/after a session
 * @param {import('../model.js').Standing} [props.standing]
 * @param {import('../types').EndRule} [props.endRule]
 * @param {import('../types').ChannelLive} [props.live]
 * @param {import('./Sparkline.jsx').SparkPoint[]} [props.spark]   none: no signal shown (spectator view)
 * @param {() => void} [props.onGear]                 none: no gear (spectator view)
 * @param {() => void} [props.onSelect]   a tap on the card (race control: its thresholds)
 * @param {boolean} [props.selected]
 * @param {boolean} [props.manual]   fixed thresholds at the finish gate
 */
export default function Lane({ color, callsign, fullName, channel, mhz, off, result, standing, endRule, live, spark, onGear, onSelect, selected, manual }) {
  const style = /** @type {React.CSSProperties} */ ({ '--lane': color })
  const head = (
    <div className="lane-head">
      {!off && <div className={standing?.pos === 1 ? 'pos first' : 'pos'}>{standing?.pos ? 'P' + standing.pos : '–'}</div>}
      <div className="who">
        <span className="cs">{callsign}</span>
        {fullName && <span className="name">{fullName}</span>}
        {off && <span className="name">frei</span>}
      </div>
      <span className="tag tnum">
        {channel} · {mhz}
      </span>
      {onGear && (
        <button
          className="btn btn-ghost btn-icon"
          style={{ marginLeft: -6 }}
          onClick={(e) => {
            e.stopPropagation() // not a tap on the card
            onGear()
          }}
          title="Slot einstellen"
        >
          <GearSix size={18} />
        </button>
      )}
    </div>
  )
  if (off) {
    return (
      <div className="lane off" style={style}>
        {head}
      </div>
    )
  }

  const laps = result?.laps ?? []
  const last = laps.at(-1)
  const lapsLabel = endRule?.kind === 'laps' ? `${Math.min(laps.length, endRule.value)} / ${endRule.value}` : String(laps.length)
  const level = live?.level_db
  const signal = live?.in_pass
    ? 'im Gate'
    : level == null
      ? 'keine Daten'
      : level < ON_DB
        ? 'kein Sender'
        : manual
          ? 'manuell'
          : live?.enter_db == null
            ? 'lernt die Pegel'
            : 'automatisch'

  return (
    <div
      className={
        'lane' +
        (live?.in_pass ? ' gate' : '') +
        (result?.finished_us && standing?.pos === 1 ? ' leader' : '') +
        (onSelect ? ' selectable' : '') +
        (selected ? ' selected' : '')
      }
      style={style}
      onClick={onSelect}
    >
      {head}
      <div style={{ display: 'flex', alignItems: 'flex-end', justifyContent: 'space-between', gap: 8 }}>
        <div style={{ display: 'flex', flexDirection: 'column' }}>
          <span className="kicker">Letzte Runde</span>
          <span className="last">{last ? secs(last.duration_us) : result?.holeshot_us != null ? 'Start' : '––.––'}</span>
        </div>
        {result?.finished_us != null && (
          <span className="tag tag-accent">
            <FlagCheckered /> Im Ziel
          </span>
        )}
      </div>
      <div className="stats3">
        <div>
          <span className="k">Runde</span>
          <span className="v">{result ? lapsLabel : '–'}</span>
        </div>
        <div>
          <span className="k">Bestzeit</span>
          <span className="v best-color">{secs(result?.best_lap_us)}</span>
        </div>
        <div>
          <span className="k">Abstand</span>
          <span className="v">{standing?.gap ?? '–'}</span>
        </div>
      </div>
      {spark && (
      <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
        <div className="spark-label">
          <span>Signal · {signal}</span>
          <span className="tnum">{level != null && level >= ON_DB ? `${level.toFixed(1)} dBFS` : ''}</span>
        </div>
        <div className="spark">
          <Sparkline points={spark} color={color} />
        </div>
      </div>
      )}
    </div>
  )
}
