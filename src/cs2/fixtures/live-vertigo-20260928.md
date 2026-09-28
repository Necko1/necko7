# Live Vertigo Regression Fixture

Source: user-supplied `necko7.log.2026-09-28`, analyzed continuously before edits.

Device `3c8e2b50-4832-4848-9556-1133c5efbeea`, channel `847261392`,
GSI session `98763c39-464b-4d8e-9b57-36df3500542d`, source seq 8 through 461.
All 454 payloads retain their original seq and shape for provider identity,
map/round phases, score, viewed identity, health and local round/match stats.
Weapons, cosmetic names, delta annotations and unrelated team fields are omitted.
`received_at` is the logged backend acceptance time, not the signed envelope time.

The main match starts at seq 13, halftime occurs at 231, final completion at 458.
Seq 458 changes live/live/counter14/8:6 to gameover/freezetime/counter15/9:6.
The next snapshot repeats gameover; no additional round is implied.

Local kills: 41, 48, 118, 224, 226, 275, 321, 403.
Local deaths: 63, 85, 133, 150, 179, 202, 334, 356, 410, 435, 458.
Final authoritative totals: kills 8, assists 2, deaths 11, MVPs 5, score 26.

Replay checks continuous source correlation and semantic/recorder behavior; ingestion
is established by the original accepted/RAW/HTTP log records. It does not establish
when a production script was enabled/published or whether every production execution
was persisted. Those facts require production database evidence, not raw GSI alone.
