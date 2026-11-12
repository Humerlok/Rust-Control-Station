#!/usr/bin/env node
// Imports the description data of SpeedLOG (github.com/Humerlok/SpeedLOG, GPL-3.0) into this crate.
//
// SpeedLOG's descriptions.js holds `window.AP_DOCS_GZ = { common, pt, copter, plane, rover, sub }`, each a
// gzip + base64 JSON chunk built from the ArduPilot documentation (LogMessages.xml and apm.pdef.json) by its
// tools/build_descriptions.cjs. This script only base64-decodes the chunks into data/descriptions/<key>.json.gz;
// the crate embeds those files and unzips them on demand.
//
//   node tools/import_descriptions.cjs <path to SpeedLOG descriptions.js> <SpeedLOG commit>
//
// Record the commit in data/SOURCE.md so a refresh is reproducible.

const fs = require('fs')
const path = require('path')

const [src, commit] = process.argv.slice(2)
if (!src || !commit) {
    console.error('usage: node tools/import_descriptions.cjs <descriptions.js> <SpeedLOG commit>')
    process.exit(2)
}
const text = fs.readFileSync(src, 'utf8').replace(/^﻿/, '')
const start = text.indexOf('{')
const end = text.lastIndexOf('}')
const chunks = JSON.parse(text.slice(start, end + 1))
const out = path.join(__dirname, '..', 'data', 'descriptions')
fs.mkdirSync(out, { recursive: true })
for (const key of ['common', 'pt', 'copter', 'plane', 'rover', 'sub']) {
    if (!chunks[key]) throw new Error('missing chunk ' + key)
    const bytes = Buffer.from(chunks[key], 'base64')
    fs.writeFileSync(path.join(out, key + '.json.gz'), bytes)
    console.log(key + ': ' + bytes.length + ' bytes')
}
const source = path.join(__dirname, '..', 'data', 'SOURCE.md')
fs.writeFileSync(source, [
    '# Source of the embedded data',
    '',
    '`descriptions/*.json.gz` come from `descriptions.js` of SpeedLOG',
    '(https://github.com/Humerlok/SpeedLOG, GPL-3.0-or-later), commit `' + commit + '`,',
    'imported with `tools/import_descriptions.cjs`. SpeedLOG builds them from the ArduPilot documentation',
    '(https://autotest.ardupilot.org: `LogMessages.xml` per vehicle and `apm.pdef.json`, GPL-3.0) plus its own',
    'Brazilian Portuguese translations (`tools/translations_pt.json` there).',
    '',
    '- `common`: `{ msgs: { TYPE: { d, f: { Field: description } } }, enums: { LogErrorSubsystem, LogEvent } }`',
    '- `pt`: `{ english text: portuguese text }` for the message and field descriptions',
    '- `copter` / `plane` / `rover` / `sub`: `{ PARAM: { n, d, u?, r?, v?, b? } }`',
    '',
].join('\n'))
