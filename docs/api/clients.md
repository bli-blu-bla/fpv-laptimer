# Clients

Fertige Clients im Repo unter
[`clients/`](https://github.com/bli-blu-bla/fpv-laptimer/tree/main/clients): jeder
[Befehl](befehle.md) als Methode, [Events](events.md) über den [WebSocket](websocket.md) mit
automatischem Neuverbinden, Fehler als Ausnahme mit der Meldung des Servers.

## Python

[`clients/python`](https://github.com/bli-blu-bla/fpv-laptimer/tree/main/clients/python),
Python ab 3.10. Befehle brauchen nur die Standardbibliothek, Events zusätzlich `websockets`.

```sh
pip install "fpv-laptimer-client[events]"   # oder aus dem Repo: "./clients/python[events]"
```

```python
from laptimer_client import Laptimer

lt = Laptimer("http://pi:5800", key="lt_…")

@lt.on("pilot_pass")
def lap(ev):
    if ev.data["kind"] == "lap":
        print(ev.data["pilot"], ev.data["lap"], ev.data["lap_time_s"])

lt.start_session(race=True, laps=3)
lt.run_forever()
```

Ohne Installation geht auch `laptimer_client/__init__.py` als `laptimer_client.py` neben dem
eigenen Script, etwa in einem [Hook](hooks.md). Alles Weitere im
[README](https://github.com/bli-blu-bla/fpv-laptimer/blob/main/clients/python/README.md).

## JavaScript

[`clients/js`](https://github.com/bli-blu-bla/fpv-laptimer/tree/main/clients/js), für Node ≥ 22
und den Browser, ohne Abhängigkeiten, mit JSDoc-Typen (der Editor ergänzt Befehle und
Event-Felder).

```js
import { Laptimer } from 'laptimer-client'

const lt = new Laptimer({ url: 'http://pi:5800', key: 'lt_…' })
lt.on('pilot_pass', (d) => {
  if (d.kind === 'lap') console.log(d.pilot, d.lap, d.lap_time_s)
})
await lt.connect()
await lt.session.start({ race: true, laps: 3 })
```

Alles Weitere im
[README](https://github.com/bli-blu-bla/fpv-laptimer/blob/main/clients/js/README.md).
