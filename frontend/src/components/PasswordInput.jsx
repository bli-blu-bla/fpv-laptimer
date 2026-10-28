// @ts-check
// A password field with an eye at its right end that shows what was typed
// (login, the secrets of the server connection). Takes the props of <input>.

import { useState } from 'react'
import { EyeIcon, EyeSlashIcon } from '@phosphor-icons/react'

/** @param {Omit<React.InputHTMLAttributes<HTMLInputElement>, 'type'>} props */
export default function PasswordInput({ className = 'input', ...props }) {
  const [shown, setShown] = useState(false)
  return (
    <span className="password">
      <input {...props} className={className} type={shown ? 'text' : 'password'} />
      <button
        type="button"
        className="password-eye"
        title={shown ? 'Verbergen' : 'Anzeigen'}
        aria-label={shown ? 'Passwort verbergen' : 'Passwort anzeigen'}
        aria-pressed={shown}
        // mousedown would take the focus (and the cursor) from the input
        onMouseDown={(e) => e.preventDefault()}
        onClick={(e) => (e.preventDefault(), setShown(!shown))}
      >
        {shown ? <EyeSlashIcon size={18} /> : <EyeIcon size={18} />}
      </button>
    </span>
  )
}
