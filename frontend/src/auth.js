// @ts-check
// The login for the race control (server: `auth.rs`). Reading works
// without it; changing needs it. The state lives here once for the whole
// page: `useAuth()` in the components, `api()` reports a 401 (`loginNeeded`),
// which opens the login dialog (`App.jsx`).

import { useEffect, useState } from 'react'

/** @typedef {{checked: boolean, enabled: boolean, loggedIn: boolean, dialog: boolean}} AuthState */

/** @type {AuthState} */
let current = { checked: false, enabled: true, loggedIn: false, dialog: false }
/** @type {Set<(s: AuthState) => void>} */
const listeners = new Set()

/** @param {Partial<AuthState>} patch */
function set(patch) {
  current = { ...current, ...patch }
  listeners.forEach((f) => f(current))
}

/** Asks the server whether this browser is logged in (on load). */
export async function checkAuth() {
  try {
    const res = await fetch('/api/auth')
    const data = await res.json()
    set({ checked: true, enabled: data.enabled, loggedIn: data.logged_in })
  } catch {
    // No server yet: try again with the next page load or login.
  }
}

/** @param {string} password  @returns {Promise<void>} throws with the server's message */
export async function login(password) {
  const res = await fetch('/api/login', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ password }),
  })
  const data = await res.json().catch(() => ({}))
  if (!res.ok) throw new Error(data.error ?? res.statusText)
  set({ loggedIn: true, dialog: false })
}

/** @param {boolean} [everywhere]  all devices (this one too) */
export async function logout(everywhere = false) {
  const res = await fetch(everywhere ? '/api/logout/all' : '/api/logout', { method: 'POST' })
  if (!res.ok) {
    const data = await res.json().catch(() => ({}))
    throw new Error(data.error ?? res.statusText)
  }
  set({ loggedIn: false })
}

/** A request was refused for want of a login: the login is gone (expired,
 * or logged out elsewhere), ask for it. */
export function loginNeeded() {
  set({ loggedIn: false, dialog: true })
}

/** @param {boolean} open */
export function showLogin(open) {
  set({ dialog: open })
}

/** The login state; `canEdit`: may change things (logged in, or no password). */
export function useAuth() {
  const [s, setS] = useState(current)
  useEffect(() => {
    listeners.add(setS)
    setS(current)
    return () => {
      listeners.delete(setS)
    }
  }, [])
  return { ...s, canEdit: !s.enabled || s.loggedIn }
}
