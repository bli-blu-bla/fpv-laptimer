// @ts-check
// − value + with a label; `delayed`: saves 0.8 s after the last tap (for
// settings whose change is costly, or many taps in a row).

import { useEffect, useRef, useState } from 'react'
import { MinusIcon, PlusIcon } from '@phosphor-icons/react'

/**
 * − value + ; `delayed`: saves 0.8 s after the last tap (gain restarts the boards).
 * @param {{label: string, value: number, unit?: string, step: number, min: number, max: number, hint?: string, delayed?: boolean, onSet: (v: number) => void}} props
 */
export default function Stepper({ label, value, unit, step, min, max, hint, delayed, onSet }) {
  const [v, setV] = useState(value)
  const timer = useRef(/** @type {ReturnType<typeof setTimeout> | undefined} */ (undefined))
  useEffect(() => setV(value), [value])
  /** @param {number} d */
  function change(d) {
    const next = Math.round(Math.max(min, Math.min(max, v + d)) * 10) / 10
    setV(next)
    clearTimeout(timer.current)
    if (delayed) timer.current = setTimeout(() => onSet(next), 800)
    else onSet(next)
  }
  return (
    <div className="field">
      <span>{label}</span>
      <div className="stepper">
        <button className="btn btn-secondary btn-icon" onClick={() => change(-step)}>
          <MinusIcon />
        </button>
        <span className="value">
          {v}
          {unit ? ` ${unit}` : ''}
        </span>
        <button className="btn btn-secondary btn-icon" onClick={() => change(step)}>
          <PlusIcon />
        </button>
      </div>
      {hint && <span className="muted small" style={{ maxWidth: 260 }}>{hint}</span>}
    </div>
  )
}

