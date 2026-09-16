/**
 * Button lints, each checking one property the button system relies on
 * (the design is in src/recipes/button.ts):
 *
 *   VARIANT   a raw <button>, <a> or <router-link> whose static `class`
 *             carries a variant fill (`bg-accent`, `bg-status-*`, not a
 *             `/10` wash). That is a re-implementation of Button /
 *             LinkButton; the recipe in src/recipes/button.ts is the one
 *             place a variant is defined. A fill inside `:class` is state
 *             (a selected chip, the active nav item) and is not a button
 *             variant, so it is deliberately out of scope.
 *   LABEL     a raw <button> whose only content is an icon and that has no
 *             `aria-label` / `aria-labelledby`. Screen readers announce it
 *             as "button". IconButton makes the label a required prop.
 *
 * Both are ratchets against scripts/button-baseline.json: a file may not
 * exceed its baseline count, a new file may not appear, and a baseline
 * that is higher than reality must be lowered (so it only ever shrinks).
 * Regenerate after converting sites with:
 *
 *   UPDATE_BUTTON_BASELINE=1 node scripts/check-buttons.mjs
 *
 * Known gap: an open tag is matched up to its first `>`, so a button whose
 * attributes contain one (an inline arrow handler) is skipped by LABEL. A
 * false negative, never a false positive.
 */
import { readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs'
import { join, relative } from 'node:path'

const ROOT = new URL('../src', import.meta.url).pathname
const REPO = new URL('..', import.meta.url).pathname
const BASELINE = new URL('./button-baseline.json', import.meta.url).pathname

function walk(dir, out = []) {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name)
    if (statSync(p).isDirectory()) walk(p, out)
    else if (name.endsWith('.vue')) out.push(p)
  }
  return out
}

/** A solid variant fill that is not a hover/focus state and not a `/10` wash. */
const VARIANT_FILL = /(?<![:\w-])bg-(?:accent|status-(?:error|warning|success))(?![/\w-])/
/** The static class attribute of an open tag. */
const STATIC_CLASS = /(?:^|\s)class="([^"]*)"/
/** Raw tags that can wear a variant. */
const RAW_OPEN = /<(button|a|router-link|RouterLink)\b[\s\S]*?>/g
/** Raw button with its content. */
const RAW_BUTTON = /<button\b([\s\S]*?)>([\s\S]*?)<\/button>/g
/** Content that is only an icon (comments stripped). */
const ICON_ONLY =
  /^(?:<Icon\b[^>]*\/?>(?:<\/Icon>)?|<svg\b(?:(?!<\/svg>)[\s\S])*<\/svg>|<Spinner\b[^>]*\/?>|<i\b[^>]*><\/i>)$/
const HAS_LABEL = /(?:^|\s)(?::?aria-label|:?aria-labelledby|v-bind)=/

const findings = { VARIANT: new Map(), LABEL: new Map() }
const add = (kind, file, line) => {
  const m = findings[kind]
  m.set(file, [...(m.get(file) ?? []), line])
}
const lineOf = (src, index) => src.slice(0, index).split('\n').length

for (const file of walk(ROOT)) {
  const src = readFileSync(file, 'utf8')
  const rel = relative(REPO, file)
  for (const m of src.matchAll(RAW_OPEN)) {
    const cls = m[0].match(STATIC_CLASS)?.[1] ?? ''
    if (VARIANT_FILL.test(cls)) add('VARIANT', rel, lineOf(src, m.index))
  }
  for (const m of src.matchAll(RAW_BUTTON)) {
    const inner = m[2].replace(/<!--[\s\S]*?-->/g, '').trim()
    if (ICON_ONLY.test(inner) && !HAS_LABEL.test(m[1])) add('LABEL', rel, lineOf(src, m.index))
  }
}

const counts = Object.fromEntries(
  Object.entries(findings).map(([k, m]) => [
    k,
    Object.fromEntries([...m].map(([f, lines]) => [f, lines.length]).sort()),
  ]),
)

if (process.env.UPDATE_BUTTON_BASELINE) {
  writeFileSync(BASELINE, JSON.stringify(counts, null, 2) + '\n')
  console.log(`[check-buttons] baseline written: ${summary(counts)}`)
  process.exit(0)
}

const baseline = JSON.parse(readFileSync(BASELINE, 'utf8'))
const problems = []
for (const kind of Object.keys(findings)) {
  const now = counts[kind]
  const then = baseline[kind] ?? {}
  for (const [file, n] of Object.entries(now)) {
    const allowed = then[file] ?? 0
    if (n > allowed) {
      const lines = findings[kind].get(file).slice(allowed).join(', ')
      problems.push(`  ${kind}  ${file}: ${n} (baseline ${allowed})  lines ${lines}`)
    }
  }
  for (const [file, allowed] of Object.entries(then)) {
    const n = now[file] ?? 0
    if (n < allowed) problems.push(`  ${kind}  ${file}: ${n} (baseline ${allowed}); lower the baseline`)
  }
}

if (problems.length) {
  console.error('\n[check-buttons] button lints failed:\n')
  console.error(problems.join('\n'))
  console.error(
    '\nVARIANT: use Button / LinkButton / IconButton (src/recipes/button.ts) instead of a raw tag with a variant fill.',
  )
  console.error('LABEL: give the icon-only button an aria-label, or use IconButton (label is required).')
  console.error('After converting sites: UPDATE_BUTTON_BASELINE=1 node scripts/check-buttons.mjs\n')
  process.exit(1)
}
console.log(`[check-buttons] OK (${summary(counts)})`)

function summary(c) {
  return Object.entries(c)
    .map(([k, m]) => `${k} ${Object.values(m).reduce((a, b) => a + b, 0)}`)
    .join(', ')
}
