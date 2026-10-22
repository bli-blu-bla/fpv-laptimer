// @ts-check
// Correcting one pilot of a session (as RotorHazard's "marshal" page): the
// level graph, where passes are added or removed.

import { slotColor } from '../format.js'
import Dialog from './Dialog.jsx'
import LevelGraph from './LevelGraph.jsx'

/**
 * @param {object} props
 * @param {import('../types').SessionDetail} props.detail   with passes
 * @param {number} props.channel
 * @param {() => void} props.onChanged
 * @param {() => void} props.onClose
 */
export default function CorrectDialog({ detail, channel, onChanged, onClose }) {
  const s = detail.session
  const i = s.seats.findIndex((x) => x.channel === channel)
  const seat = s.seats[i]
  const color = slotColor(channel)
  return (
    <Dialog
      wide
      onClose={onClose}
      title={
        <>
          <span className="dot" style={{ background: color }} /> {seat.label} korrigieren
          <span className="muted small">{seat.channel_name}</span>
        </>
      }
    >
      <div style={/** @type {React.CSSProperties} */ ({ '--lane': color })}>
        <LevelGraph
          sessionId={s.id}
          channel={channel}
          t0={s.config.race ? s.start_us : s.created_us}
          passes={(detail.passes ?? []).filter((p) => p.channel === channel)}
          result={detail.results[i]}
          onChanged={onChanged}
        />
      </div>
    </Dialog>
  )
}
