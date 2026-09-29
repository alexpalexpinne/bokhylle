import { execFileSync } from 'node:child_process'
import { readFileSync } from 'node:fs'

for (const [name, installed, shipped] of [
  ['Epub.js', '../node_modules/epubjs/license', '../public/licenses/epubjs.txt'],
  ['PDF.js', '../node_modules/pdfjs-dist/LICENSE', '../public/licenses/pdfjs.txt'],
]) {
  if (readFileSync(new URL(installed, import.meta.url), 'utf8') !==
      readFileSync(new URL(shipped, import.meta.url), 'utf8')) {
    console.error(`${name} distribution notice differs from the installed package`)
    process.exitCode = 1
  }
}

const allowed = new Set([
  'Apache-2.0',
  'BSD-2-Clause',
  'BSD-3-Clause',
  'ISC',
  'MIT',
  'MPL-2.0',
  'OFL-1.1',
])
// These licenses were reviewed for the OpenAPI type generator's development
// dependencies and the EPUB renderer's archive dependencies. Keep exceptions
// scoped to the named packages. JSZip is used under MIT; pako's MIT and Zlib
// notices are both retained in the installed package.
const reviewed = new Map([
  ['Python-2.0', new Set(['argparse'])],
  ['(MIT OR CC0-1.0)', new Set(['type-fest'])],
  ['(MIT OR GPL-3.0-or-later)', new Set(['jszip'])],
  ['(MIT AND Zlib)', new Set(['pako'])],
])
const report = JSON.parse(execFileSync('pnpm', ['licenses', 'list', '--json'], {
  cwd: new URL('..', import.meta.url),
  encoding: 'utf8',
}))
const unknown = Object.entries(report).flatMap(([license, packages]) =>
  packages
    .filter((item) => !allowed.has(license) && !reviewed.get(license)?.has(item.name))
    .map((item) => `${item.name}: ${license}`),
)
if (unknown.length > 0) {
  console.error(`Review new frontend licenses: ${unknown.join(', ')}`)
  process.exitCode = 1
} else {
  console.log('Frontend dependency licenses match the reviewed allow list')
}
