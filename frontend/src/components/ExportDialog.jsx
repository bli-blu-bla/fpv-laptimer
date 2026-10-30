// @ts-check
// Export of one or several sessions as a file (server/src/data.rs,
// GET /api/export): laps or result, all channels or some, CSV (spreadsheet)
// or JSON; one session also as a debug package (.tar.gz with the raw levels);
// any number as a package for another laptimer (server/src/transfer.rs,
// GET /api/export/package; imported under Sessions → Importieren).

import { useState } from 'react'
import { DownloadSimpleIcon } from '@phosphor-icons/react'
import { slotColor } from '../format.js'
import Dialog from './Dialog.jsx'

/** Asks the server for a file and saves it under the name it gives.
 * fetch instead of a plain link: an error shows in the dialog, not as a page. @param {string} url */
async function download(url) {
  const res = await fetch(url)
  if (!res.ok) {
    const data = await res.json().catch(() => ({}))
    throw new Error(data.error ?? res.statusText)
  }
  const name = /filename="([^"]+)"/.exec(res.headers.get('content-disposition') ?? '')?.[1] ?? 'export'
  const href = URL.createObjectURL(await res.blob())
  const a = document.createElement('a')
  a.href = href
  a.download = name
  document.body.appendChild(a)
  a.click()
  a.remove()
  setTimeout(() => URL.revokeObjectURL(href), 1000)
}

/** @param {{sessions: import('../types').Session[], onClose: () => void}} props */
export default function ExportDialog({ sessions, onClose }) {
  const [content, setContent] = useState(/** @type {'laps' | 'results'} */ ('laps'))
  const [format, setFormat] = useState(/** @type {'csv' | 'json' | 'debug' | 'package'} */ ('csv'))
  /** Chosen channel indexes; empty: all. */
  const [channels, setChannels] = useState(/** @type {number[]} */ ([]))
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)

  // The channels of the chosen sessions; one session: with who flew there.
  /** @type {Map<number, string>} */
  const offered = new Map()
  for (const s of sessions) {
    for (const seat of s.seats) {
      if (!offered.has(seat.channel)) offered.set(seat.channel, sessions.length === 1 ? `${seat.channel_name} · ${seat.label}` : seat.channel_name)
    }
  }
  const channelList = [...offered.entries()].sort((a, b) => a[0] - b[0])
  /** @param {number} c */
  const toggle = (c) => setChannels(channels.includes(c) ? channels.filter((x) => x !== c) : [...channels, c])

  async function save() {
    setError('')
    setBusy(true)
    const ids = sessions.map((s) => s.id).join(',')
    let url = `/api/export?${new URLSearchParams({ sessions: ids, channels: channels.join(','), content, format })}`
    if (format === 'debug') url = `/api/export/debug?${new URLSearchParams({ session: ids, channels: channels.join(',') })}`
    if (format === 'package') url = `/api/export/package?${new URLSearchParams({ sessions: ids })}`
    try {
      await download(url)
      onClose()
    } catch (e) {
      setError(/** @type {Error} */ (e).message)
    } finally {
      setBusy(false)
    }
  }

  const title = sessions.length === 1 ? 'Session exportieren' : `${sessions.length} Sessions exportieren`
  return (
    <Dialog title={title} onClose={onClose}>
      <div className="field">
        <span>Format</span>
        <div className="seg">
          <button className={format === 'csv' ? 'on' : ''} onClick={() => setFormat('csv')}>
            CSV (Tabelle)
          </button>
          <button className={format === 'json' ? 'on' : ''} onClick={() => setFormat('json')}>
            JSON
          </button>
          {sessions.length === 1 && (
            <button className={format === 'debug' ? 'on' : ''} onClick={() => setFormat('debug')}>
              Debug
            </button>
          )}
          <button className={format === 'package' ? 'on' : ''} onClick={() => setFormat('package')}>
            Paket
          </button>
        </div>
      </div>
      {format === 'csv' && (
        <div className="field">
          <span>Inhalt</span>
          <div className="seg">
            <button className={content === 'laps' ? 'on' : ''} onClick={() => setContent('laps')}>
              Runden
            </button>
            <button className={content === 'results' ? 'on' : ''} onClick={() => setContent('results')}>
              Ergebnis
            </button>
          </div>
          <span className="muted small">
            {content === 'laps' ? 'Eine Zeile pro Runde mit Rundenzeit und Zeit seit dem Start.' : 'Eine Zeile pro Pilot: Platz, Runden, Bestzeit, Gesamtzeit.'}
          </span>
        </div>
      )}
      {format !== 'package' && (
        <div className="field">
          <span>Kanäle</span>
          <div className="seg" style={{ flexWrap: 'wrap' }}>
            <button className={channels.length === 0 ? 'on' : ''} onClick={() => setChannels([])}>
              Alle
            </button>
            {channelList.map(([c, label]) => (
              <button key={c} className={channels.includes(c) ? 'on' : ''} onClick={() => toggle(c)}>
                <span style={{ display: 'inline-flex', alignItems: 'center', gap: 6 }}>
                  <span className="dot" style={{ background: slotColor(c) }} /> {label}
                </span>
              </button>
            ))}
          </div>
        </div>
      )}
      {format === 'package' && (
        <span className="muted small">
          Die Sessions komplett, mit Durchflügen und Rohpegeln, für einen anderen Laptimer (z. B. vom Pi in die Testumgebung): dort unter Sessions →
          Importieren. Mit Pegeln einige MB pro Session.
        </span>
      )}
      {format === 'debug' && (
        <span className="muted small">
          Alles zum Fehlersuchen in einer .tar.gz: Rohpegel (neu auswertbar mit <code>laptimer replay</code>), Pegelverlauf mit Schwellen je 10 ms, was der Detektor
          meldete (auch Zurückgenommenes), alle Durchflüge, Einstellungen, Version.
        </span>
      )}
      {format === 'csv' && <span className="muted small">Für Excel und LibreOffice: Semikolon als Trenner, Dezimalkomma.</span>}
      {error && <p className="error">{error}</p>}
      <div style={{ display: 'flex', gap: 8, justifyContent: 'flex-end' }}>
        <button className="btn btn-secondary" onClick={onClose}>
          Abbrechen
        </button>
        <button className="btn btn-primary" disabled={busy} onClick={save}>
          <DownloadSimpleIcon size={16} /> Herunterladen
        </button>
      </div>
    </Dialog>
  )
}
