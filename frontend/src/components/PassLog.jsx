// @ts-check
// The passes of a session, newest first; a wrong one is deleted with a tap.

import { ListNumbersIcon, TrashIcon } from '@phosphor-icons/react'
import { api } from '../api.js'
import { passLog } from '../model.js'

/**
 * @param {object} props
 * @param {import('../types').SessionDetail | null} props.detail
 * @param {number} [props.limit]
 * @param {() => void} [props.onChanged]
 */
export default function PassLog({ detail, limit = 8, onChanged }) {
  const log = detail ? passLog(detail) : []
  return (
    <div className="panel">
      <div className="panel-head">
        <ListNumbersIcon size={18} />
        Durchflüge
        <span className="end muted small">Falsch erkannt? Einfach löschen.</span>
      </div>
      {!log.length && <div className="empty">Noch keine Durchflüge. Starte ein Rennen oder Training.</div>}
      {log.slice(0, limit).map((e) => (
        <div className="list-row" key={e.pass.id}>
          <span className="dot" style={{ background: e.color, width: 8, height: 8 }} />
          <span style={{ width: 86, fontWeight: 500, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{e.seat.label}</span>
          <span className="muted small" style={{ width: 76 }}>
            {e.what}
          </span>
          <span style={{ flex: 1, fontSize: 16 }}>{e.time}</span>
          <span className="small" style={{ color: e.best ? 'var(--accent-300)' : 'var(--n500)' }}>
            {e.delta}
          </span>
          <button
            className="btn btn-ghost btn-icon"
            title="Durchflug löschen"
            onClick={async () => {
              await api(`/passes/${e.pass.id}`, { body: { deleted: true } })
              onChanged?.()
            }}
          >
            <TrashIcon size={16} />
          </button>
        </div>
      ))}
    </div>
  )
}
