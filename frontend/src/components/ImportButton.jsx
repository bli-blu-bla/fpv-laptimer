// @ts-check
// Sessions from another laptimer (server/src/transfer.rs, POST /api/import):
// a package from Export → Paket, e.g. the Pi's sessions with their levels
// into a test setup. Uploaded with progress (the files are tens of MB);
// what came in is shown in a dialog. The list reloads by itself
// (`session_changed`).

import { UploadSimpleIcon } from '@phosphor-icons/react'
import { useRef, useState } from 'react'
import Dialog from './Dialog.jsx'

/**
 * @typedef {object} ImportResult
 * @property {{id: number, from: number}[]} imported
 * @property {{id: number, from: number}[]} skipped   already here (same start)
 * @property {string[]} pilots_created
 * @property {number[]} confirmed   races whose result was not confirmed yet (now it is)
 */

/** Sends the file, `onProgress` 0–1 while uploading. @param {File} file @param {(p: number) => void} onProgress
 * @returns {Promise<ImportResult>} */
function upload(file, onProgress) {
  // XMLHttpRequest instead of fetch: only it reports the upload's progress.
  return new Promise((resolve, reject) => {
    const xhr = new XMLHttpRequest()
    xhr.open('POST', '/api/import')
    xhr.upload.onprogress = (e) => e.lengthComputable && onProgress(e.loaded / e.total)
    xhr.onload = () => {
      let data = {}
      try {
        data = JSON.parse(xhr.responseText)
      } catch {
        // not JSON: the status says enough
      }
      if (xhr.status === 200) resolve(/** @type {ImportResult} */ (data))
      else reject(new Error(/** @type {{error?: string}} */ (data).error ?? `Fehler ${xhr.status}`))
    }
    xhr.onerror = () => reject(new Error('Upload abgebrochen.'))
    const form = new FormData()
    form.append('package', file)
    xhr.send(form)
  })
}

export default function ImportButton() {
  const input = useRef(/** @type {HTMLInputElement | null} */ (null))
  const [progress, setProgress] = useState(/** @type {number | null} */ (null))
  const [result, setResult] = useState(/** @type {ImportResult | null} */ (null))
  const [error, setError] = useState('')

  /** @param {File | undefined} file */
  async function start(file) {
    if (!file) return
    setError('')
    setProgress(0)
    try {
      setResult(await upload(file, setProgress))
    } catch (e) {
      setError(/** @type {Error} */ (e).message)
    } finally {
      setProgress(null)
      if (input.current) input.current.value = '' // the same file again works
    }
  }

  const busy = progress != null
  const ids = (/** @type {{id: number}[]} */ list) => list.map((x) => '#' + x.id).join(', ')
  return (
    <>
      <input ref={input} type="file" accept=".gz,.tar.gz,application/gzip" hidden onChange={(e) => start(e.target.files?.[0])} />
      <button
        className="btn btn-ghost"
        disabled={busy}
        onClick={() => input.current?.click()}
        title="Sessions aus einem Paket eines anderen Laptimers übernehmen (Export → Paket)"
      >
        <UploadSimpleIcon size={16} />
        {busy ? (progress < 1 ? ` ${Math.round(progress * 100)} %` : ' Entpacken …') : ' Importieren'}
      </button>
      {(result || error) && (
        <Dialog
          title="Import"
          onClose={() => {
            setResult(null)
            setError('')
          }}
        >
          {error && <p className="error">{error}</p>}
          {result && (
            <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
              <span>
                {result.imported.length === 0
                  ? 'Keine neuen Sessions.'
                  : `${result.imported.length} Session${result.imported.length === 1 ? '' : 's'} übernommen: ${ids(result.imported)}`}
              </span>
              {result.skipped.length > 0 && (
                <span className="muted small">
                  Schon vorhanden: {result.skipped.length} ({ids(result.skipped)}).
                </span>
              )}
              {result.pilots_created.length > 0 && <span className="muted small">Neu angelegte Piloten: {result.pilots_created.join(', ')}.</span>}
              {result.confirmed.length > 0 && (
                <span className="muted small">
                  {result.confirmed.length === 1 ? 'Ein Rennen war' : `${result.confirmed.length} Rennen waren`} noch nicht bestätigt und gilt jetzt als
                  bestätigt.
                </span>
              )}
              {result.imported.length > 0 && (
                <span className="muted small">Mit den Pegeln lassen sie sich unter Boards → Quelle abspielen und unter Debug neu auswerten.</span>
              )}
            </div>
          )}
          <div style={{ display: 'flex', justifyContent: 'flex-end' }}>
            <button
              className="btn btn-primary"
              onClick={() => {
                setResult(null)
                setError('')
              }}
            >
              OK
            </button>
          </div>
        </Dialog>
      )}
    </>
  )
}
