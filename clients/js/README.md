# fpv-laptimer-client (JavaScript)

Client für die API des Laptimers (`/api/v1`, ausführlich in [`docs/api/`](../../docs/api/index.md)):
jeder Befehl als Methode, Events live über den WebSocket, mit automatischem Neuverbinden.

- läuft in **Node ≥ 22** und im **Browser** (nutzt das eingebaute `fetch` und `WebSocket`)
- **keine Abhängigkeiten**, reines JavaScript mit JSDoc-Typen (`// @ts-check`), der Editor
  ergänzt also Befehle, Argumente und Event-Felder

## Einbinden

```sh
npm install fpv-laptimer-client
# oder aus dem Repo: npm install ../pfad/zu/fpv-laptimer/clients/js
```

```js
import { Laptimer, LaptimerError } from 'fpv-laptimer-client'
// oder ohne npm, mit dem Pfad: import { Laptimer } from './clients/js/src/index.js'

const lt = new Laptimer({ url: 'http://pi:5800', key: 'lt_…' })
```

Den Schlüssel legst du auf der Seite unter *API & Hooks → Schlüssel* an.

Optionen: `url`, `key`, `transport` (`'auto'` oder `'http'`, siehe unten), `timeout_ms`
(pro Befehl, Standard 15 000), `max_backoff_ms` (längste Pause beim Neuverbinden, Standard
10 000).

## Beispiele

**Rennen über 3 Runden starten:**

```js
await lt.channels.assign(['NITRO', 'ACE', null, 'BOB'], { create: true })
const session = await lt.session.start({ race: true, laps: 3, countdown_s: 5 })
console.log('Rennen', session.id, session.phase) // "countdown"
```

**Jede Runde ansagen** (hier: ausgeben):

```js
lt.on('pilot_pass', (d) => {
  if (d.kind === 'lap') console.log(`${d.pilot}: Runde ${d.lap} in ${d.lap_time_s?.toFixed(2)} s, Platz ${d.position}`)
})
lt.on('pilot_finished', (d) => console.log(`${d.pilot} im Ziel, Platz ${d.position}`))
await lt.connect()
```

**Ergebnis bestätigen**, sobald das Rennen vorbei ist (alle im Ziel oder gestoppt):

```js
lt.on('session_stopped', async (d) => {
  if (!d.needs_confirmation) return
  console.log('Sieger:', d.winner)
  await lt.session.confirm(d.session_id)
})
```

Oder der Reihe nach mit `waitFor`:

```js
const stopped = await lt.waitFor('session_stopped', { filter: (d) => d.session_id === session.id, timeout_ms: 30 * 60_000 })
await lt.session.confirm(stopped.session_id)
```

**Fehler** kommen als `LaptimerError` mit der Meldung des Servers (deutsch, zum direkten
Anzeigen):

```js
try {
  await lt.session.start()
} catch (e) {
  if (e instanceof LaptimerError) console.log(e.message, e.status) // "Es läuft schon eine Session – erst stoppen." 400
  else throw e
}
```

`status`: HTTP-Status (400 falsche Argumente oder nicht erlaubt, 401 Schlüssel fehlt oder ist
falsch, 404 unbekannter Befehl; über den WebSocket dieselben Zahlen); `null`, wenn der Server
nicht erreichbar ist. `command`: der Befehl.

Zum Ausprobieren in [`examples/`](examples/):

```sh
LAPTIMER_URL=http://pi:5800 LAPTIMER_KEY=lt_… node examples/events.js        # alle Events anzeigen
LAPTIMER_URL=http://pi:5800 LAPTIMER_KEY=lt_… node examples/race.js NITRO ACE  # Rennen, Runden, bestätigen
```

## Methoden

Alle geben ein Promise zurück. Kanäle zählen ab 0 (Slot 1 auf der Seite = Kanal 0), Zeiten
sind µs seit 1970.

| Methode | Befehl | Ergebnis |
|---|---|---|
| `ping()` | `ping` | `{pong, now_us}` |
| `state()` | `state` | Gesamtzustand: `session`, `unconfirmed`, `channels`, `pilots`, `boards` |
| `stats()` | `stats` | Bestzeiten, Rennen, Siege pro Pilot (`all`, `today`) |
| `channels.list()` | `channels.list` | Kanäle |
| `channels.set(index, {pilot, pilot_id, create, label, enabled, frequency})` | `channels.set` | der Kanal; nur Angegebenes ändert sich |
| `channels.assign(pilots, {create})` | `channels.assign` | alle Kanäle; `pilots` = Callsign, ID oder `null` pro Kanal |
| `pilots.list()` | `pilots.list` | Piloten |
| `pilots.get(id)` | `pilots.get` | Pilot |
| `pilots.create(name, full_name?)` | `pilots.create` | Pilot |
| `pilots.update(id, {name, full_name})` | `pilots.update` | Pilot |
| `pilots.delete(id)` | `pilots.delete` | `{}` |
| `session.start({race, laps, seconds, countdown_s, start_mode})` | `session.start` | die Session; ohne Argumente ein Training |
| `session.stop()` | `session.stop` | `{session_id}` |
| `session.cancel()` | `session.cancel` | stoppt und löscht die laufende Session |
| `session.confirm(id?)` | `session.confirm` | `{session_id}`; ohne `id` das Rennen, das auf Bestätigung wartet |
| `session.discard(id?)` | `session.discard` | löscht eine beendete Session |
| `sessions.list({limit})` | `sessions.list` | die letzten Sessions (Standard 20) |
| `sessions.get(id)` | `sessions.get` | Session mit allen Durchflügen (`passes`) |
| `passes.add(channel, {time_us, session_id})` | `passes.add` | `{pass_id, session_id, time_us}`; Standard: jetzt, laufende Session |
| `passes.delete(id)` | `passes.delete` | erkannte werden nur markiert, von Hand eingefügte entfernt |
| `passes.restore(id)` | `passes.restore` | einen markierten wiederherstellen |
| `command(name, args)` | jeder | für Befehle, die neuer sind als dieser Client |

Was die Felder bedeuten, steht in [`docs/api/events.md`](../../docs/api/events.md) und [`docs/api/datenformate.md`](../../docs/api/datenformate.md); die Typen in
[`src/types.js`](src/types.js).

## Events

```js
const off = lt.on('pilot_pass', (data, msg) => { … })   // msg: {event, seq, time_us, data}
lt.on('*', (data, msg) => console.log(msg.event, data))  // jedes Event
off()                                                    // oder lt.off('pilot_pass', fn)
await lt.connect()                                       // liefert das hello mit state
lt.close()
```

Events des Servers: `session_started`, `countdown`, `start_signal`, `pilot_pass`,
`pilot_finished`, `session_stopped`, `race_confirmed`, `session_deleted`, `sessions_reset`, `passes_changed`,
`channels_changed`, `pilot_added`, `pilot_updated`, `pilot_deleted`.

Dazu drei vom Client selbst:

| Name | wann |
|---|---|
| `hello` | verbunden (auch nach jedem Neuverbinden), `data.state` ist der aktuelle Zustand |
| `disconnected` | Verbindung weg; der Client verbindet sich wieder |
| `lagged` | der Client war zu langsam, `data.missed` Events fehlen: `state()` neu holen |

`waitFor(name, {filter, timeout_ms})` wartet auf das nächste passende Event und liefert seine
Daten (Standard-Zeitlimit 30 s, dann ein Fehler).

**Neuverbinden:** Bricht die Verbindung ab (Pi neu gestartet, WLAN weg), verbindet sich der
Client von selbst wieder, mit wachsenden Pausen (0,5 s, 1 s, 2 s … bis `max_backoff_ms`), bis
`close()`. Events, die in der Pause passiert sind, kommen nicht nach: nach `hello` den Zustand
aus `data.state` nehmen. Klappt schon der **erste** `connect()` nicht (falsche Adresse,
falscher Schlüssel), lehnt das Promise mit der Meldung des Servers ab, und es wird nichts
wiederholt.

## HTTP oder WebSocket

Befehle gehen per HTTP (`POST /api/v1/cmd/<befehl>`, Schlüssel im Header
`Authorization: Bearer`). Solange der WebSocket verbunden ist, gehen sie über ihn (kein neuer
Verbindungsaufbau pro Befehl). Mit `transport: 'http'` bleiben sie immer bei HTTP. Bricht der
WebSocket mitten in einem Befehl ab, schlägt dieser mit einem `LaptimerError` fehl; er wird
nicht automatisch wiederholt (er könnte schon ausgeführt sein).

Der WebSocket bekommt den Schlüssel als `?key=…`, weil Browser bei WebSockets keine Header
setzen können. Im Browser gilt außerdem: eine Seite über `https://` darf keine Verbindung zu
`http://pi:5800` aufbauen; der Laptimer selbst läuft ohne HTTPS.

## Tests

Gegen einen laufenden Server ohne Boards, am besten mit eigenem Datenordner (die Tests legen
Piloten an, starten Trainings und Rennen und ändern das gespeicherte Rennformat):

```sh
cd server && ./target/debug/laptimer serve --port 5801 --data /tmp/lt-test --sounds /tmp/leer
curl -s -H 'content-type: application/json' -d '{"name":"test"}' http://localhost:5801/api/keys   # {"key": "lt_…"}
cd clients/js && LAPTIMER_URL=http://localhost:5801 LAPTIMER_KEY=lt_… npm test
```

Ohne die beiden Variablen werden die Tests übersprungen. Typprüfung: `npm run check` (lädt
TypeScript nur als Prüfer).

## Veröffentlichen (npm)

Die Version kommt aus dem Git-Tag und ist unabhängig von Server und Python-Client. Im Repo
steht in `package.json` nur der Platzhalter `0.0.0-dev`; der Workflow schreibt vor dem Packen
die echte Version hinein. Ablauf:

1. Tag `js-v<version>` pushen, z. B. `git tag js-v0.2.0 && git push origin js-v0.2.0`
   (SemVer, Vorabversionen wie `0.2.0-rc.1`).
2. Der Workflow `.github/workflows/npm.yml` setzt die Version, prüft die Typen, packt das Paket
   (`prepack` erzeugt dabei `types/*.d.ts` aus dem JSDoc, damit auch TypeScript-Projekte die
   Typen bekommen), installiert es testweise und lädt es per Trusted Publishing (ohne Token,
   mit Herkunftsnachweis) auf npm. Vorabversionen bekommen das Dist-Tag `next`, damit
   `npm install` weiter die letzte richtige Version holt.

Von Hand starten (*Actions → npm → Run workflow*) packt und prüft nur
(`npm publish --dry-run`); npm hat kein Test-Register wie TestPyPI.

Einmal einrichten: npm bietet Trusted Publishing nur für ein Paket an, das es schon gibt. Die
erste Version deshalb von Hand:

```sh
cd clients/js
npm login
npm version 0.1.0 --no-git-tag-version
npm publish --access public
git checkout package.json        # zurück auf den Platzhalter
```

Diese erste Version bekommt keinen Tag (der Workflow würde sie noch einmal hochladen wollen und
scheitern); die nächste kommt dann über `js-v0.2.0`.

Danach auf npmjs.com unter *Package → Settings → Trusted Publisher* GitHub Actions wählen:
Owner `bli-blu-bla`, Repo `fpv-laptimer`, Workflow `npm.yml`, Environment `npm`; im
GitHub-Repo unter *Settings → Environments* das Environment `npm` anlegen (am besten mit
Freigabe durch dich). Ab dann gehen alle Versionen über den Tag.

Lokal packen und ansehen: `npm pack --dry-run` (bzw. `npm pack` für die Datei).
