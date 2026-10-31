// @ts-check
// Where we are on the page, kept in the URL's hash as a path:
// "#settings/system", "#sessions/12/edit". Part 0 is the page (App.jsx),
// part 1 a tab or the chosen session, part 2 below that. A reload stays where
// it was, and the browser's back button goes back through pages and tabs.

import { useEffect, useSyncExternalStore } from 'react'

/** @returns {string[]} the parts of the hash ("" → []) */
function parts() {
  const h = decodeURIComponent(location.hash.slice(1))
  return h ? h.split('/') : []
}

/** @param {() => void} cb */
function subscribe(cb) {
  window.addEventListener('hashchange', cb)
  return () => window.removeEventListener('hashchange', cb)
}

const getHash = () => location.hash

/** Set while something unsaved would be lost by leaving (see `useLeaveGuard`). */
let guard = /** @type {string | null} */ (null)
let lastHash = location.hash
// Registered when the module loads, so before any component's listener: a
// declined "leave?" stops the event and puts the old hash back (without a new
// "hashchange"), and the page never sees the change.
window.addEventListener('hashchange', (e) => {
  if (guard && location.hash !== lastHash && !window.confirm(guard)) {
    e.stopImmediatePropagation()
    history.replaceState(null, '', lastHash || location.pathname + location.search)
    return
  }
  lastHash = location.hash
})
window.addEventListener('beforeunload', (e) => {
  if (guard) e.preventDefault() // the browser asks before reloading or closing
})

/** Leaving is confirmed already (e.g. the page asked itself): no second question. */
export function allowLeave() {
  guard = null
}

/**
 * While `message` is set, leaving by the back button, a link or a reload asks
 * first (the editor's drafts live only in the page).
 * @param {string | null} message
 */
export function useLeaveGuard(message) {
  useEffect(() => {
    guard = message
    return () => {
      guard = null
    }
  }, [message])
}

/**
 * Sets part `level` of the hash and drops everything below it (a new tab
 * starts at its own top). Adds a history entry and fires "hashchange".
 * @param {number} level @param {string | null} value  null = drop this part too
 */
export function setPart(level, value) {
  const p = parts().slice(0, level)
  // Missing parts above (e.g. a tab set before the page is in the hash) stay empty.
  while (p.length < level) p.push('')
  if (value != null) p.push(value)
  const next = p.join('/').replace(/\/+$/, '')
  if (next !== parts().join('/')) location.hash = next
}

/**
 * One part of the hash as state: `[value, setValue]`. An unknown or missing
 * value reads as `fallback`; setting the fallback removes the part, so the
 * plain page keeps a short link.
 * @template {string} T
 * @param {number} level @param {readonly T[]} allowed @param {T} fallback
 * @returns {[T, (v: T) => void]}
 */
export function useHashPart(level, allowed, fallback) {
  useSyncExternalStore(subscribe, getHash) // re-render on every hash change
  const raw = parts()[level]
  const value = /** @type {T} */ (allowed.find((a) => a === raw) ?? fallback)
  return [value, (v) => setPart(level, v === fallback ? null : v)]
}

/**
 * A part that holds a number (e.g. a session ID), or null.
 * @param {number} level
 * @returns {[number | null, (v: number | null) => void]}
 */
export function useHashNumber(level) {
  useSyncExternalStore(subscribe, getHash)
  const n = Number(parts()[level])
  const value = Number.isInteger(n) && n > 0 ? n : null
  return [value, (v) => setPart(level, v == null ? null : String(v))]
}
