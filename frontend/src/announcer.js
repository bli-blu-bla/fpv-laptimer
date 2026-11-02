// @ts-check
// Announcements played by this browser (Settings → Ansagen → Ausgabe
// „Browser“). The server sends each announcement as the WebSocket message
// `announce`: a list of audio URLs, or texts the browser speaks itself when
// the server has no speech synthesis. Only a browser switched on as speaker
// plays them (remembered in this browser), so phones stay quiet.

const LAPTIME_SPEAKER = 'laptimer.speaker'

/** @typedef {{url?: string, text?: string, lang?: string}} AnnounceItem */

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
    return localStorage.getItem(LAPTIME_SPEAKER) === '1'
  } catch {
    return false
  }
}

/** @param {boolean} on */
export function setSpeaker(on) {
  try {
    if (on) localStorage.setItem(LAPTIME_SPEAKER, '1')
    else localStorage.removeItem(LAPTIME_SPEAKER)
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
  audioContext()?.resume()
}

/** For the countdown beeps. Made at the first use; starts "suspended" until a tap on the page. @type {AudioContext | null} */
let ctx = null
/** Session id of the countdown planned last, and its beeps (to stop them when the session goes away). */
let countdownSession = 0
/** @type {OscillatorNode[]} */
let beeps = []

function audioContext() {
  if (!ctx && 'AudioContext' in window) ctx = new AudioContext()
  return ctx
}

// Any tap on the page allows the AudioContext to play.
document.addEventListener('pointerdown', () => audioContext()?.resume(), { capture: true })

/**
 * Plans the countdown beeps of a race: a short one in each of the last 3
 * seconds, a longer, higher one at the start (like the server's `Tone`).
 * Timed by the browser's own clock, the same one as the number on the
 * screen, and scheduled in the AudioContext (sample-exact, no download, no
 * WebSocket delay). Called for every `state`; plans each session once.
 * `sessionId` 0 (no countdown running) stops planned beeps (aborted).
 * @param {number} sessionId
 * @param {number} startUs   start signal, server clock
 * @param {number} countdownS
 * @param {number} clockOffsetUs   server − browser clock (`useServer`)
 */
export function scheduleCountdown(sessionId, startUs, countdownS, clockOffsetUs) {
  if (sessionId === countdownSession) return
  countdownSession = sessionId
  for (const b of beeps) b.stop()
  beeps = []
  if (!sessionId || !isSpeaker()) return
  const ac = audioContext()
  if (!ac) return
  if (ac.state !== 'running') {
    ac.resume().catch(() => {})
    // Still not running a moment later: no tap on the page yet, show "Ton freigeben".
    setTimeout(() => {
      if (/** @type {string} */ (ac.state) !== 'running') setBlocked(true)
    }, 200)
  }
  // Seconds from now (browser clock) to the start, then on the AudioContext's clock.
  const toStart = (startUs - clockOffsetUs - Date.now() * 1000) / 1e6
  const start = ac.currentTime + toStart
  for (let n = Math.min(3, countdownS); n >= 1; n--) beep(ac, start - n, 880, 0.15)
  beep(ac, start, 1760, 0.6)
}

/** @param {AudioContext} ac @param {number} at   AudioContext time, s @param {number} freq @param {number} length   s */
function beep(ac, at, freq, length) {
  if (at < ac.currentTime) return // already over (page opened during the countdown)
  const osc = ac.createOscillator()
  const gain = ac.createGain()
  osc.frequency.value = freq
  // 5 ms fade in and out, so it does not click.
  gain.gain.setValueAtTime(0, at)
  gain.gain.linearRampToValueAtTime(0.6, at + 0.005)
  gain.gain.setValueAtTime(0.6, at + length - 0.005)
  gain.gain.linearRampToValueAtTime(0, at + length)
  osc.connect(gain).connect(ac.destination)
  osc.start(at)
  osc.stop(at + length)
  beeps.push(osc)
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
      console.log("playNext", item);
      //if (item.url) await playUrl(item.url)
      if (item.text) await speak(item.text, item.lang)
      else if (item.url) await playUrl(item.url)
    } catch (e) {
      if (e instanceof DOMException && e.name === 'NotAllowedError') {
        console.log("test")
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
  console.log("playUrl", audio);
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

/** @param {string} text @param {string | undefined} lang @returns {Promise<void>} */
function speak(text, lang) {
  console.log("speak", lang, text);
  return new Promise((resolve) => {
    if (!('speechSynthesis' in window)) return resolve()
    const u = new SpeechSynthesisUtterance(text)
    u.lang = lang || 'en-US'
    u.onend = () => resolve()
    u.onerror = () => resolve()
    speechSynthesis.speak(u)
  })
}
