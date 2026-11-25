#!/usr/bin/env node
/**
 * Text colours come from the theme, at full strength.
 *
 * Every theme's text tokens are held to 4.5:1 on every background
 * (themes/__tests__/textContrast.spec.ts), and the status, priority and
 * accent colours have text versions derived per theme. Two things step
 * around that, and this refuses both in `.vue` and `.ts` under src/ and
 * packages/core/src:
 *
 *   - An opacity modifier on a text colour (`text-tertiary/60`,
 *     `text-accent/70`): the text is mixed into its background and falls
 *     under the ratio the token was tuned to.
 *   - A raw palette text colour (`text-amber-700`): it is one value for
 *     every theme, so on a coloured surface or its own tint it can fail.
 *     Use the status tokens (`text-status-warning` on
 *     `bg-status-warning-muted`) or `text-tertiary`.
 *   - A focus ring mixed into its background (`focus:ring-accent/30`)
 *     with no solid accent border beside it: the only focus indicator
 *     then falls under 3:1 (WCAG 1.4.11).
 *
 * Files that let a person pick a colour from a fixed palette are listed
 * in ALLOWED with the reason. Runs in the frontend `lint` script.
 */
import { readFileSync, readdirSync, statSync } from 'node:fs'
import { join, relative } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = join(fileURLToPath(new URL('.', import.meta.url)), '..')
const dirs = [join(root, 'src'), join(root, '..', 'packages', 'core', 'src')]

const ALLOWED = {
  // Tag colours a person picks, each a light and dark pair.
  'src/components/ticketComponents/TicketTagsField.vue': 'person-picked palette',
  // Workflow state colours a person picks.
  '../packages/core/src/utils/workflowColors.ts': 'person-picked palette',
}

const TOKENS = String.raw`(?:primary|secondary|tertiary|overdue|accent(?:-hover)?|on-accent|status-[a-z-]+|priority-[a-z]+)`
const DIMMED = new RegExp(String.raw`(?<![\w-])text-${TOKENS}/\d+`, 'g')
const PALETTE = /(?<![\w-])text-(?:red|rose|amber|yellow|orange|green|emerald|lime|teal|cyan|sky|blue|indigo|violet|purple|fuchsia|pink|slate|gray|zinc|neutral|stone)-\d{2,3}(?![\w-])/g

function walk(dir, out = []) {
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry)
    if (entry === 'node_modules' || entry === '__tests__') continue
    if (statSync(full).isDirectory()) walk(full, out)
    else if (/\.(vue|ts)$/.test(entry) && !entry.endsWith('.spec.ts')) out.push(full)
  }
  return out
}

const FADED_RING = /(?<![\w-])(?:focus|focus-visible|focus-within):ring-accent\/\d+/
const SOLID_BORDER = /(?<![\w-])(?:focus|focus-visible):border-accent(?![\w/-])/

const failures = []
for (const file of dirs.flatMap((d) => walk(d))) {
  const rel = relative(root, file)
  const source = readFileSync(file, 'utf8')
  const line = (i) => source.slice(0, i).split('\n').length
  for (const m of source.matchAll(DIMMED)) {
    failures.push(`${rel}:${line(m.index)}: ${m[0]} dims text under its contrast; use the token at full strength`)
  }
  // A class list is one quoted string; judge each on its own.
  for (const m of source.matchAll(/(["'`])((?:(?!\1)[^\n])*)\1/g)) {
    if (FADED_RING.test(m[2]) && !SOLID_BORDER.test(m[2])) {
      failures.push(`${rel}:${line(m.index)}: a faded focus ring is the only focus indicator; use ring-accent at full strength`)
    }
  }
  if (ALLOWED[rel]) continue
  for (const m of source.matchAll(PALETTE)) {
    failures.push(`${rel}:${line(m.index)}: ${m[0]} is one colour for every theme; use a status token or text-tertiary`)
  }
}

if (failures.length > 0) {
  console.error('check-text-colors: text colours must come from the theme at full strength.\n')
  for (const f of failures) console.error(`  ${f}`)
  console.error(`\n${failures.length} found.`)
  process.exit(1)
}
console.log('check-text-colors: ok')
