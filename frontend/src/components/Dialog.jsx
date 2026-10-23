// @ts-check
// A modal dialog: a title row with a close button, then the content. A tap
// on the dimmed background closes it too.

import { useRef } from 'react'
import { XIcon } from '@phosphor-icons/react'

/**
 * @param {object} props
 * @param {React.ReactNode} props.title
 * @param {() => void} props.onClose
 * @param {boolean} [props.wide]
 * @param {React.ReactNode} props.children
 */
export default function Dialog({ title, onClose, wide, children }) {
  // Close only on a tap that started on the background: a drag that starts
  // inside and ends outside (moving a graph) also "clicks" the background.
  const downOnBackdrop = useRef(false)
  return (
    <div
      className="backdrop"
      onPointerDown={(e) => (downOnBackdrop.current = e.target === e.currentTarget)}
      onClick={(e) => downOnBackdrop.current && e.target === e.currentTarget && onClose()}
    >
      <div className={wide ? 'dialog wide' : 'dialog'} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-head">
          {title}
          <button className="btn btn-ghost btn-icon" onClick={onClose} title="Schließen">
            <XIcon size={18} />
          </button>
        </div>
        {children}
      </div>
    </div>
  )
}
