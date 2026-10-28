# HTTP

Der einfachste Weg: ein Befehl pro Anfrage, mit dem [Schlüssel](index.md#schlüssel) im
Header. Gut für Scripts, curl, einen Taster am Gate, alles, was nicht dauerhaft verbunden
sein muss. Events gibt es über HTTP nicht; dafür der [WebSocket](websocket.md).

Basis-Adresse: `http://<pi>:5800/api/v1`

## Zwei Formen pro Befehl

Jeder [Befehl](befehle.md) geht auf zwei Arten:

1. **REST-Route**, z. B. `POST /api/v1/session/start` mit `{"race": true}` als Body. Welche
   Route zu welchem Befehl gehört, steht in der Tabelle der [Befehle](befehle.md). Teile der
   Route (`/pilots/{id}`) und Query-Parameter (`?limit=5`) werden zu Argumenten.
2. **Generisch**: `POST /api/v1/cmd/<befehl>` mit den Argumenten als Body, z. B.
   `POST /api/v1/cmd/session.start` `{"race": true}`. Praktisch, wenn ein Programm alle Befehle
   gleich behandeln will, oder für dieselben Nachrichten wie im WebSocket.

Der Body ist JSON (`content-type: application/json`); ohne Argumente darf er fehlen.

## Antworten

| Status | Bedeutung | Body |
|---|---|---|
| 200 | ausgeführt | das Ergebnis als JSON (siehe Befehl) |
| 400 | abgelehnt (falsche Argumente, geht gerade nicht) | `{"error": "Es läuft schon eine Session – erst stoppen."}` |
| 401 | Schlüssel fehlt oder ist falsch | `{"error": "…"}` |
| 404 | unbekannter Befehl oder Route | `{"error": "…"}` |

Die Meldung in `error` ist deutsch und zum direkten Anzeigen gedacht.

## Beispiele

**curl** (Schlüssel in `K`):

```sh
K=lt_…; B=http://pi:5800/api/v1
curl -H "Authorization: Bearer $K" $B/state
curl -H "Authorization: Bearer $K" -H 'content-type: application/json' \
     -d '{"pilots": ["NITRO", "ACE", "BOB", null], "create": true}' $B/channels/assign
curl -H "Authorization: Bearer $K" -H 'content-type: application/json' \
     -d '{"race": true, "laps": 3, "countdown_s": 5}' $B/session/start
curl -X POST -H "Authorization: Bearer $K" $B/session/confirm
```

**Taster am Gate** (ein Durchflug für Kanal 0, jetzt):

```sh
curl -X POST -H "Authorization: Bearer $K" -H 'content-type: application/json' \
     -d '{"channel": 0}' $B/passes
```

Der Server prüft dabei die Mindest-Rundenzeit, ein prellender Taster zählt also nur einmal.

**Python** ohne Zusatzpakete:

```python
import json, urllib.request

def cmd(name, **args):
    req = urllib.request.Request(
        f"http://pi:5800/api/v1/cmd/{name}", data=json.dumps(args).encode(),
        headers={"Authorization": "Bearer lt_…", "content-type": "application/json"})
    with urllib.request.urlopen(req) as r:
        return json.load(r)

print(cmd("session.start", race=True, laps=3))
```

Ein `HTTPError` hat die Meldung im Body (`json.load(e)["error"]`). Bequemer mit dem
[Python-Client](clients.md).
