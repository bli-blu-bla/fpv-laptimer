// @ts-check
// A modal dialog: a title row with a close button, then the content. A tap
// on the dimmed background closes it too.

import { X } from '@phosphor-icons/react'

/**
 * @param {object} props
 * @param {React.ReactNode} props.title
 * @param {() => void} props.onClose
 * @param {boolean} [props.wide]
 * @param {React.ReactNode} props.children
 */
export default function Dialog({ title, onClose, wide, children }) {
  return (
    <div className="backdrop" onClick={onClose}>
      <div className={wide ? 'dialog wide' : 'dialog'} onClick={(e) => e.stopPropagation()}>
        <div className="dialog-head">
          {title}
          <button className="btn btn-ghost btn-icon" onClick={onClose} title="Schließen">
            <X size={18} />
          </button>
        </div>
        {children}
      </div>
    </div>
  )
}
