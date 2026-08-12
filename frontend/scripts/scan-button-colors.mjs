/**
 * Scan for <button> elements painted with raw Tailwind palette colours instead
 * of the app's semantic tokens (see components/common/Button.vue for the
 * canonical variants).
 *
 * Reports two separate things, because they are different severities:
 *
 *   RAW    a literal palette colour (`bg-blue-600`, `text-gray-400`, `#1f2937`).
 *          These ignore the theme entirely and are the real defect.
 *   ONACC  `text-white` sitting on `bg-accent`. The on-accent foreground is
 *          theme-aware — the injector picks black or white per theme by WCAG
 *          luminance against the actual accent — so hardcoding white is
 *          invisible on a light accent (brand orange resolves to BLACK).
 *
 * Diagnostic only; not wired into `lint`.
 */
import { readFileSync, readdirSync, statSync } from 'node:fs'
import { join, relative } from 'node:path'

const ROOT = new URL('../src', import.meta.url).pathname
const REPO = new URL('..', import.meta.url).pathname

/** Tailwind palette families that are never semantic in this codebase. */
const PALETTE =
  'slate|gray|zinc|neutral|stone|red|orange|amber|yellow|lime|green|emerald|teal|cyan|sky|blue|indigo|violet|purple|fuchsia|pink|rose'
const RAW_CLASS = new RegExp(`\\b(?:bg|text|border|ring|from|to|via)-(?:${PALETTE})-\\d{2,3}\\b`, 'g')
const RAW_HEX = /#[0-9a-fA-F]{3,8}\b/g

/**
 * User-chosen entity colours (a group's or category's colour, stored per row
 * and applied via inline style) are data, not chrome. `#6366f1` is the shared
 * fallback for "this entity has no colour set" and is not a theme violation.
 */
const ENTITY_COLOR_FALLBACK = /#6366f1/i

function walk(dir, out = []) {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name)
    const s = statSync(p)
    if (s.isDirectory()) walk(p, out)
    else if (name.endsWith('.vue')) out.push(p)
  }
  return out
}

/** Extract each `<button ...>` open tag, with its line number. */
function buttonTags(src) {
  const out = []
  const re = /<button\b[\s\S]*?>/g
  let m
  while ((m = re.exec(src)) !== null) {
    out.push({ tag: m[0], line: src.slice(0, m.index).split('\n').length })
  }
  return out
}

const findings = []
for (const file of walk(ROOT)) {
  const src = readFileSync(file, 'utf8')
  const rel = relative(REPO, file)
  for (const { tag, line } of buttonTags(src)) {
    const raw = [
      ...new Set([...(tag.match(RAW_CLASS) ?? []), ...(tag.match(RAW_HEX) ?? [])]),
    ].filter((c) => !ENTITY_COLOR_FALLBACK.test(c))
    // Per class-group, not per tag: a ternary often puts `bg-accent
    // text-on-accent` in one branch and `bg-status-warning text-white` in the
    // other, and testing the whole tag reports that correct code as a finding.
    //
    // Single-quoted groups are the unit, because a `:class="[...]"` attribute's
    // own double quotes wrap every branch at once — testing those is no better
    // than testing the whole tag.
    const singles = tag.match(/'[^']*'/g) ?? []
    const doubles = (tag.match(/"[^"]*"/g) ?? []).filter((s) => !s.includes("'"))
    const onAccent = [...singles, ...doubles].some(
      (s) => /\bbg-accent\b/.test(s) && /\btext-white\b/.test(s),
    )
    if (raw.length) findings.push({ kind: 'RAW', file: rel, line, detail: raw.join(' ') })
    if (onAccent) findings.push({ kind: 'ONACC', file: rel, line, detail: 'text-white on bg-accent' })
  }
}

const byKind = (k) => findings.filter((f) => f.kind === k)
for (const kind of ['RAW', 'ONACC']) {
  const rows = byKind(kind)
  console.log(`\n=== ${kind}: ${rows.length}`)
  const byFile = new Map()
  for (const r of rows) byFile.set(r.file, [...(byFile.get(r.file) ?? []), r])
  for (const [file, rs] of [...byFile].sort((a, b) => b[1].length - a[1].length)) {
    console.log(`  ${file} (${rs.length})`)
    for (const r of rs.slice(0, 6)) console.log(`      :${r.line}  ${r.detail}`)
  }
}
console.log(`\ntotal findings: ${findings.length}`)
