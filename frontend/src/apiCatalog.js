// @ts-check
// The public API (/api/v1, server/src/api.rs) described for the API page:
// its reference and its tester both read this list. The same in long form:
// docs/api/.

/**
 * @typedef {object} ApiCommand
 * @property {string} cmd        name for the WebSocket and /api/v1/cmd/<name>
 * @property {string} method     HTTP method of the REST route
 * @property {string} path       REST route below /api/v1; {x} comes from the arguments
 * @property {string} group
 * @property {string} title
 * @property {string} [desc]
 * @property {[string, string][]} [args]   [name, description]
 * @property {Record<string, unknown>} [example]   arguments for the tester
 */

/** @type {ApiCommand[]} */
export const COMMANDS = [
  { cmd: 'ping', method: 'POST', path: '/cmd/ping', group: 'Allgemein', title: 'Verbindung prüfen', desc: 'Antwortet mit pong und der Serverzeit.' },
  {
    cmd: 'state',
    method: 'GET',
    path: '/state',
    group: 'Allgemein',
    title: 'Gesamtzustand',
    desc: 'Laufende Session (mit Ergebnissen), das Rennen, das auf Bestätigung wartet, Kanäle, Piloten, Boards.',
  },
  {
    cmd: 'stats',
    method: 'GET',
    path: '/stats',
    group: 'Allgemein',
    title: 'Bestzeiten und Siege',
    desc: 'Pro Pilot: Bestzeit, beste N in Folge, Rennen, Siege; gesamt und heute.',
  },

  {
    cmd: 'channels.list',
    method: 'GET',
    path: '/channels',
    group: 'Kanäle',
    title: 'Kanäle mit Piloten',
    desc: 'index (0, 1, …), Frequenz, belegt, Pilot, angezeigter Name.',
  },
  {
    cmd: 'channels.set',
    method: 'PATCH',
    path: '/channels/{index}',
    group: 'Kanäle',
    title: 'Einen Kanal einstellen',
    desc: 'Nur die angegebenen Felder ändern sich. Ein Pilot oder eine Frequenz, die schon ein anderer Kanal hat, wird getauscht (wie auf der Seite).',
    args: [
      ['index', 'Kanal, ab 0'],
      ['pilot', 'Callsign, oder null = niemand'],
      ['pilot_id', 'statt pilot: die ID'],
      ['create', 'true: unbekannten Callsign als Piloten anlegen'],
      ['label', 'Freitext statt eines Piloten ("" = keiner)'],
      ['enabled', 'Kanal belegt (false: fliegt nicht mit)'],
      ['frequency', '"R1" … "R8" oder MHz; nicht während einer Session'],
    ],
    example: { index: 0, pilot: 'NITRO', create: true },
  },
  {
    cmd: 'channels.assign',
    method: 'POST',
    path: '/channels/assign',
    group: 'Kanäle',
    title: 'Alle Kanäle auf einmal belegen',
    desc: 'Ein Eintrag pro Kanal, der Reihe nach (Callsign, Pilot-ID oder null). Kanäle hinter der Liste bleiben, wie sie sind.',
    args: [
      ['pilots', 'Liste, z. B. ["NITRO", "ACE", null, 4]'],
      ['create', 'true: unbekannte Callsigns anlegen'],
    ],
    example: { pilots: ['NITRO', 'ACE', null, null], create: true },
  },

  { cmd: 'pilots.list', method: 'GET', path: '/pilots', group: 'Piloten', title: 'Alle Piloten' },
  { cmd: 'pilots.get', method: 'GET', path: '/pilots/{id}', group: 'Piloten', title: 'Ein Pilot', args: [['id', 'Pilot-ID']], example: { id: 1 } },
  {
    cmd: 'pilots.create',
    method: 'POST',
    path: '/pilots',
    group: 'Piloten',
    title: 'Pilot anlegen',
    desc: 'Callsigns sind eindeutig.',
    args: [
      ['name', 'Callsign'],
      ['full_name', 'Name (optional)'],
    ],
    example: { name: 'NITRO', full_name: 'Nina' },
  },
  {
    cmd: 'pilots.update',
    method: 'PATCH',
    path: '/pilots/{id}',
    group: 'Piloten',
    title: 'Pilot ändern',
    args: [
      ['id', 'Pilot-ID'],
      ['name', 'neuer Callsign (optional)'],
      ['full_name', 'neuer Name (optional)'],
    ],
    example: { id: 1, full_name: 'Nina M.' },
  },
  {
    cmd: 'pilots.delete',
    method: 'DELETE',
    path: '/pilots/{id}',
    group: 'Piloten',
    title: 'Pilot löschen',
    desc: 'Alte Sessions behalten den Namen; der Kanal des Piloten wird frei.',
    args: [['id', 'Pilot-ID']],
    example: { id: 1 },
  },

  {
    cmd: 'session.start',
    method: 'POST',
    path: '/session/start',
    group: 'Session',
    title: 'Training oder Rennen starten',
    desc: 'Bei einem Rennen werden angegebene Werte als Rennformat gespeichert (die Seite zeigt sie auch). Abgelehnt, solange eine Session läuft oder ein Rennen auf Bestätigung wartet.',
    args: [
      ['race', 'true: Rennen, false: Training (Standard)'],
      ['laps', 'Rennen: Ende nach so vielen Runden'],
      ['seconds', 'Rennen: Ende nach so vielen Sekunden (statt laps)'],
      ['countdown_s', 'Rennen: Countdown 0–60 s'],
      ['start_mode', 'Rennen: holeshot, from_start oder flying'],
    ],
    example: { race: true, laps: 3, countdown_s: 5 },
  },
  {
    cmd: 'session.stop',
    method: 'POST',
    path: '/session/stop',
    group: 'Session',
    title: 'Laufende Session beenden',
    desc: 'Ein Rennen wartet danach auf Bestätigung.',
  },
  {
    cmd: 'session.cancel',
    method: 'POST',
    path: '/session/cancel',
    group: 'Session',
    title: 'Laufende Session abbrechen',
    desc: 'Stoppt und löscht sie mit allen Runden.',
  },
  {
    cmd: 'session.confirm',
    method: 'POST',
    path: '/session/confirm',
    group: 'Session',
    title: 'Ergebnis bestätigen',
    desc: 'Erst danach startet die nächste Session. Event race_confirmed.',
    args: [['id', 'Session (Standard: das Rennen, das auf Bestätigung wartet)']],
  },
  {
    cmd: 'session.discard',
    method: 'POST',
    path: '/session/discard',
    group: 'Session',
    title: 'Ergebnis verwerfen',
    desc: 'Löscht eine beendete Session.',
    args: [['id', 'Session (Standard: das Rennen, das auf Bestätigung wartet)']],
  },
  {
    cmd: 'sessions.list',
    method: 'GET',
    path: '/sessions',
    group: 'Session',
    title: 'Letzte Sessions mit Ergebnis',
    args: [['limit', 'Anzahl (Standard 20; REST: ?limit=)']],
  },
  {
    cmd: 'sessions.get',
    method: 'GET',
    path: '/sessions/{id}',
    group: 'Session',
    title: 'Eine Session mit allen Durchflügen',
    args: [['id', 'Session-ID']],
    example: { id: 1 },
  },

  {
    cmd: 'passes.add',
    method: 'POST',
    path: '/passes',
    group: 'Durchflüge',
    title: 'Durchflug von Hand',
    desc: 'Zählt wie ein erkannter (Ansage, Event pilot_pass), z. B. von einem Taster am Gate. Abgelehnt, wenn näher als die Mindestrunde an einem anderen (ein Taster prellt; in der Seite gilt die Mindestrunde nicht) oder außerhalb der Session (vor dem Start, nach dem Ende, mehr als 1 s in der Zukunft).',
    args: [
      ['channel', 'Kanal, ab 0'],
      ['time_us', 'Zeit, µs seit 1970 (Standard: jetzt)'],
      ['session_id', 'Standard: die laufende Session'],
    ],
    example: { channel: 0 },
  },
  {
    cmd: 'passes.delete',
    method: 'DELETE',
    path: '/passes/{id}',
    group: 'Durchflüge',
    title: 'Durchflug löschen',
    desc: 'Erkannte werden nur markiert (wiederherstellbar), von Hand eingefügte gelöscht.',
    args: [['id', 'Durchflug-ID']],
    example: { id: 1 },
  },
  {
    cmd: 'passes.restore',
    method: 'POST',
    path: '/passes/{id}/restore',
    group: 'Durchflüge',
    title: 'Durchflug wiederherstellen',
    args: [['id', 'Durchflug-ID']],
    example: { id: 1 },
  },
]

/** Fields every event about a pilot has. */
const SEAT = 'channel, channel_name, mhz, pilot_id, pilot'

/** @type {{name: string, desc: string, fields: string}[]} */
export const EVENTS = [
  {
    name: 'session_started',
    desc: 'Start gedrückt (vor dem Countdown).',
    fields: 'session_id, race, start_mode, end (open/laps/seconds), end_value, countdown_s, created_us, start_us, pilot_count, seats[]',
  },
  { name: 'countdown', desc: 'Jede Sekunde des Countdowns.', fields: 'session_id, race, seconds_left' },
  { name: 'start_signal', desc: 'Startsignal (Training: sofort nach dem Start).', fields: 'session_id, race, start_us' },
  {
    name: 'pilot_pass',
    desc: 'Ein Pilot fliegt durchs Ziel. kind: lap (beendet Runde lap), holeshot, start (erster Durchflug im Training), ignored (vor dem Start/nach dem Ziel).',
    fields: `${SEAT}, session_id, race, pass_id, time_us, manual, kind, lap, lap_time_us, lap_time_s, laps, best_lap_us, best_lap_s, personal_best, session_best, holeshot_us, finished, position`,
  },
  {
    name: 'pilot_finished',
    desc: 'Ein Pilot hat das Rennende erreicht.',
    fields: `${SEAT}, session_id, laps, total_us, total_s, best_lap_us, best_lap_s, position`,
  },
  {
    name: 'session_stopped',
    desc: 'Session beendet. reason: stopped (von Hand), finished (alle im Ziel), cancelled (wird gelöscht).',
    fields: 'session_id, race, reason, ended_us, needs_confirmation, winner, results[]',
  },
  { name: 'race_confirmed', desc: 'Ergebnis eines Rennens bestätigt.', fields: 'session_id, race, confirmed_us, winner, results[]' },
  { name: 'session_deleted', desc: 'Session gelöscht (verworfen, abgebrochen).', fields: 'session_id, race' },
  { name: 'sessions_reset', desc: 'Alle Sessions gelöscht (Einstellungen → Daten); Piloten und Einstellungen bleiben.', fields: 'deleted' },
  { name: 'passes_changed', desc: 'Durchflüge korrigiert (Editor, Löschen, Erkennung nimmt einen zurück): Ergebnisse neu holen.', fields: 'session_id' },
  { name: 'channels_changed', desc: 'Kanäle oder ihre Piloten geändert.', fields: 'channels[]' },
  { name: 'pilot_added', desc: 'Pilot angelegt.', fields: 'pilot_id, name, full_name' },
  { name: 'pilot_updated', desc: 'Pilot geändert.', fields: 'pilot_id, name, full_name' },
  { name: 'pilot_deleted', desc: 'Pilot gelöscht.', fields: 'pilot_id, name' },
]

/** The REST path with the arguments filled in; the other arguments go into
 * the query (GET, DELETE) or the JSON body. @param {ApiCommand} c @param {Record<string, unknown>} args */
export function restCall(c, args) {
  const rest = { ...args }
  let path = c.path.replace(/\{(\w+)\}/g, (_, k) => {
    const v = rest[k]
    delete rest[k]
    return encodeURIComponent(String(v ?? ''))
  })
  const hasBody = c.method !== 'GET' && c.method !== 'DELETE'
  if (!hasBody && Object.keys(rest).length) path += '?' + new URLSearchParams(Object.entries(rest).map(([k, v]) => [k, String(v)]))
  return { path, body: rest, hasBody }
}
