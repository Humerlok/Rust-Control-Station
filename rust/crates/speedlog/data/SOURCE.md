# Source of the embedded data

`descriptions/*.json.gz` come from `descriptions.js` of SpeedLOG
(https://github.com/Humerlok/SpeedLOG, GPL-3.0-or-later), commit `ee8253d6213e1b74a64541529eb48b180a141f9f`,
imported with `tools/import_descriptions.cjs`. SpeedLOG builds them from the ArduPilot documentation
(https://autotest.ardupilot.org: `LogMessages.xml` per vehicle and `apm.pdef.json`, GPL-3.0) plus its own
Brazilian Portuguese translations (`tools/translations_pt.json` there).

- `common`: `{ msgs: { TYPE: { d, f: { Field: description } } }, enums: { LogErrorSubsystem, LogEvent } }`
- `pt`: `{ english text: portuguese text }` for the message and field descriptions
- `copter` / `plane` / `rover` / `sub`: `{ PARAM: { n, d, u?, r?, v?, b? } }`
