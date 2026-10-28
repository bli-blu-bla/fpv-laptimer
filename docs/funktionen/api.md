# API und Hooks

Der Laptimer soll mit anderen Dingen zusammenspielen: ein Stream-Overlay, ein Discord-Bot,
ein Taster am Gate, LEDs am Pi. Dafür gibt es zwei Wege.

## API

Alles unter `/api/v1`, per HTTP oder WebSocket, mit API-Schlüssel. Jede Aktion ist ein
**Befehl** mit Namen und JSON-Argumenten, etwa `session.start` oder `pilots.add`; derselbe
Befehl geht als REST-Route, als `POST /api/v1/cmd/<befehl>` oder über den WebSocket. Über den
WebSocket kommt außerdem jedes **Ereignis** live: `pilot_pass`, `start_signal`,
`pilot_finished`, `race_confirmed` und weitere.

```sh
# Rennen starten
curl -X POST -H "Authorization: Bearer lt_…" \
     -H "content-type: application/json" -d '{"race": true}' \
     http://laptimer.local:5800/api/v1/session/start
```

```js
// Jeden Durchflug mitlesen
const ws = new WebSocket('ws://laptimer.local:5800/api/v1/ws?key=lt_…')
ws.onmessage = (m) => {
  const e = JSON.parse(m.data)
  if (e.event === 'pilot_pass') console.log(e.data.pilot, e.data.lap_time_s)
}
```

Die Seite **API & Hooks** bringt alles mit: Schlüssel anlegen, Kurzreferenz, und einen Tester,
der jeden Befehl per HTTP oder WebSocket abschickt, die passende curl-Zeile zeigt und die
Ereignisse live mitschreibt.

![API-Seite mit Tester](../assets/screens/api.png){ .shot }

## Hooks

Für Scripts direkt auf dem Pi: Bei einem Ereignis startet der Server ein Script aus
`~/.laptimer/data/hooks/`, mit den Daten als Umgebungsvariablen (`LAPTIMER_EVENT`,
`LAPTIMER_JSON`, `LAPTIMER_<FELD>`). Welches Script bei welchem Ereignis läuft, stellt man auf
der Seite ein; dort stehen auch die letzten Läufe mit Ausgabe.

Ein Beispiel aus dem Paket (`hooks-examples/led.sh`): Hook auf `start_signal` mit den
Argumenten `17 blink` lässt eine LED am GPIO 17 dreimal blinken.

```sh
#!/bin/sh
# led.sh <gpio> on|off|blink
pin=$1
case "$2" in
  on) pinctrl set "$pin" op dh ;;
  off) pinctrl set "$pin" op dl ;;
  blink) for _ in 1 2 3; do pinctrl set "$pin" op dl; sleep 0.1; pinctrl set "$pin" op dh; sleep 0.1; done ;;
esac
```

Alle Befehle, Ereignisse und Datenformate: [API-Referenz](../API.md).
