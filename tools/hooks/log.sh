#!/bin/sh
# Hook-Script: in den Script-Ordner kopieren (Standard ~/.laptimer/data/hooks/, ausführbar).
# Jedes Event als eine JSON-Zeile anhängen; zeigt nebenbei die Variablen.
echo "$LAPTIMER_JSON" >> events.log
if [ "$LAPTIMER_EVENT" = pilot_pass ] && [ "$LAPTIMER_KIND" = lap ]; then
  echo "$LAPTIMER_PILOT: Runde $LAPTIMER_LAP in $LAPTIMER_LAP_TIME_S s (Platz $LAPTIMER_POSITION)"
fi
