// @ts-check
// Announcements played by this browser (Settings → Ansagen → Ausgabe
// „Browser“). The server sends each announcement as the WebSocket message
// `announce`: a list of audio URLs, or texts the browser speaks itself when
// the server has no speech synthesis. Only a browser switched on as speaker
// plays them (remembered in this browser), so phones stay quiet.

const KEY = 'laptimer.speaker'

/** @typedef {{url?: string, text?: string}} AnnounceItem */

/** @type {AnnounceItem[][]} */
const queue = []
let playing = false
/** Browsers only play sound after a tap on the page; until then: true. */
let blocked = false
/** @type {Set<(blocked: boolean) => void>} */
const listeners = new Set()
/** Loaded clips, by URL (they are short and come again and again). @type {Map<string, HTMLAudioElement>} */
const cache = new Map()

export function isSpeaker() {
  try {
    return localStorage.getItem(KEY) === '1'
  } catch {
    return false
  }
}

/** @param {boolean} on */
export function setSpeaker(on) {
  try {
    if (on) localStorage.setItem(KEY, '1')
    else localStorage.removeItem(KEY)
  } catch {
    // private window: then only until the page is reloaded
  }
  if (on) setBlocked(false) // switched on by a tap: sound is allowed now
}

/** Called for every `announce` message. @param {AnnounceItem[]} items */
export function announce(items) {
  if (!isSpeaker()) return
  queue.push(items)
  if (!playing) playNext()
}

/** Tells `cb` when sound gets blocked or allowed again. @param {(blocked: boolean) => void} cb */
export function onBlocked(cb) {
  listeners.add(cb)
  cb(blocked)
  return () => {
    listeners.delete(cb)
  }
}

/** After a tap on "Ton freigeben": sound is allowed from now on. */
export function unblock() {
  setBlocked(false)
}

/** @param {boolean} b */
function setBlocked(b) {
  blocked = b
  for (const cb of listeners) cb(b)
}

async function playNext() {
  const items = queue.shift()
  if (!items) {
    playing = false
    return
  }
  playing = true
  for (const item of items) {
    try {
      if (item.url) await playUrl(item.url)
      else if (item.text) await speak(item.text)
    } catch (e) {
      if (e instanceof DOMException && e.name === 'NotAllowedError') {
        setBlocked(true)
        queue.length = 0 // stale by the time someone taps
        break
      }
      console.warn('announce', e)
    }
  }
  playNext()
}

/** @param {string} url @returns {Promise<void>} */
function playUrl(url) {
  let audio = cache.get(url)
  if (!audio) {
    audio = new Audio(url)
    // Spoken texts are made per announcement; only clips are worth keeping.
    if (url.includes('/clips/')) cache.set(url, audio)
  }
  const a = audio
  a.currentTime = 0
  return new Promise((resolve, reject) => {
    a.onended = () => resolve()
    a.onerror = () => resolve() // missing file: go on with the next part
    a.play().catch(reject)
  })
}

/** @param {string} text @returns {Promise<void>} */
function speak(text) {
  return new Promise((resolve) => {
    if (!('speechSynthesis' in window)) return resolve()
    const u = new SpeechSynthesisUtterance(text)
    u.lang = 'de-DE'
    u.onend = () => resolve()
    u.onerror = () => resolve()
    speechSynthesis.speak(u)
  })
}
