// @ts-check
// The tab bar of a page (API & Hooks, Settings): icon and label, the active
// one underlined in the accent colour; scrolls sideways if it gets too wide.
// A fourth entry `true` puts a warning dot on the tab.

/**
 * @template {string} T
 * @param {object} props
 * @param {readonly (readonly [T, string, import('@phosphor-icons/react').Icon, boolean?])[]} props.tabs  [id, label, icon, warn]
 * @param {T} props.value
 * @param {(id: T) => void} props.onChange
 * @param {React.ReactNode} [props.end]  shown at the right end (e.g. "Gespeichert")
 */
export default function Tabs({ tabs, value, onChange, end }) {
  return (
    <div className="tabs">
      <div className="tabs-list" role="tablist">
        {tabs.map(([id, label, Icon, warn]) => (
          <button key={id} role="tab" aria-selected={value === id} className={value === id ? 'on' : ''} onClick={() => onChange(id)}>
            <Icon size={18} /> {label}
            {warn && <span className="tabs-warn" />}
          </button>
        ))}
      </div>
      {end && <span className="tabs-end">{end}</span>}
    </div>
  )
}
