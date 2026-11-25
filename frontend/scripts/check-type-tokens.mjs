#!/usr/bin/env node
/**
 * Every font size and weight comes from the type tokens.
 *
 * Text size is one number, `--text-scale` (main.css): the device's text
 * size, or the person's own. It reaches text through the `--text-*` tokens
 * and nothing else, so a size written as a length stays put while the rest
 * of the page grows around it. A px size also ignores the browser's own
 * text setting (WCAG 1.4.4). Weights go through `--font-weight-*` for the
 * same reason: iOS Bold Text remaps those tokens, and a literal weight
 * would not follow.
 *
 * Refused, in `.vue` and `.css` under src/:
 *
 *   - `font-size` as a px, pt or rem length, and a `--text-*` token
 *     defined in px. Relative sizes (`em`, `%`) follow their parent and
 *     are fine. Print styles are exempt: paper has no text setting.
 *   - `line-height` as a px or rem length, and an arbitrary leading in
 *     px or rem (`leading-[18px]`): a fixed line box clips text that
 *     grows. Use a ratio or a scaled `--leading-*` token.
 *   - An arbitrary size class in px, pt or rem (`text-[13px]`) or an
 *     inline font size.
 *   - `font-weight` as a number or keyword, and an arbitrary weight class
 *     (`font-[600]`). `@font-face` ranges are exempt.
 *
 * A file that cannot read the tokens at all is listed in ALLOWED with the
 * reason. Runs in the frontend `lint` script, alongside the other guards.
 */
import { readFileSync, readdirSync, statSync } from 'node:fs'
import { join, relative } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = join(fileURLToPath(new URL('.', import.meta.url)), '..')
const srcDir = join(root, 'src')

/** Files whose type cannot come from the tokens, and why. */
const ALLOWED = {
  // The email renders in a sandboxed iframe document, which the app's
  // custom properties do not reach.
  'src/components/ticketComponents/EmailHtmlBody.vue': 'iframe document',
}

const LENGTH_SIZE = /^-?[\d.]+(?:px|pt|rem)$/
/** A size built from lengths alone, `max(1rem, 16px)`: no token in it. */
const fixedLength = (value) => LENGTH_SIZE.test(value) || (/\d(?:px|pt|rem)\b/.test(value) && !/var\(--(?:text|leading)-/.test(value))
const ARBITRARY_SIZE = /(?<![\w-])text-\[(?:length:)?[\d.]+(?:px|pt|rem)\]/g
const ARBITRARY_WEIGHT = /(?<![\w-])font-\[\d+\]/g
const ARBITRARY_LEADING = /(?<![\w-])leading-\[[\d.]+(?:px|pt|rem)\]/g

const failures = []
const fail = (file, line, message) => failures.push(`${relative(root, file)}:${line}: ${message}`)

function walk(dir, out = []) {
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry)
    if (statSync(full).isDirectory()) {
      if (entry !== '__tests__') walk(full, out)
    } else if (/\.(vue|css|ts)$/.test(entry) && !entry.endsWith('.spec.ts')) out.push(full)
  }
  return out
}

const lineAt = (text, index) => text.slice(0, index).split('\n').length

/** Declarations in a stylesheet, each with the at-rules and selector around it. */
function* declarations(css) {
  const text = css.replace(/\/\*[\s\S]*?\*\//g, (c) => c.replace(/[^\n]/g, ' '))
  const stack = []
  let start = 0
  for (let i = 0; i < text.length; i++) {
    const ch = text[i]
    if (ch === '{') {
      stack.push(text.slice(start, i).trim())
      start = i + 1
    } else if (ch === '}' || ch === ';') {
      const decl = text.slice(start, i)
      const m = /^\s*([\w-]+)\s*:\s*([\s\S]+?)\s*$/.exec(decl)
      if (m && stack.length > 0) yield { prop: m[1], value: m[2], at: start + decl.search(/\S/), context: [...stack] }
      if (ch === '}') stack.pop()
      start = i + 1
    }
  }
}

function checkCss(file, css, offset, source) {
  for (const d of declarations(css)) {
    const where = lineAt(source, offset + d.at)
    const print = d.context.some((c) => /^@media\b[^{]*\bprint\b/.test(c))
    const fontFace = d.context.some((c) => c.startsWith('@font-face'))
    const value = d.value.replace(/\s*!important$/, '')
    if (d.prop === 'font-size' && !print && fixedLength(value)) {
      fail(file, where, `font-size: ${value}; use a --text-* token (var(--text-sm)) so it follows the text size`)
    }
    if ((d.prop === 'line-height' || /^--text-[\w-]+--line-height$/.test(d.prop)) && !print && fixedLength(value)) {
      fail(file, where, `${d.prop}: ${value}; a fixed line box clips larger text, use a ratio or a --leading-* token`)
    }
    if (/^--text-[\w-]+$/.test(d.prop) && !/--line-height|--letter-spacing|--font-weight/.test(d.prop) && /^[\d.]+px$/.test(value)) {
      fail(file, where, `${d.prop}: ${value}; type tokens are rem, scaled by --text-scale`)
    }
    if (d.prop === 'font-weight' && !fontFace && /^(\d+|bold|bolder|normal|lighter)$/.test(value)) {
      fail(file, where, `font-weight: ${value}; use a --font-weight-* token (var(--font-weight-semibold)) so Bold Text can remap it`)
    }
  }
}

for (const file of walk(srcDir)) {
  const rel = relative(root, file)
  if (ALLOWED[rel]) continue
  const source = readFileSync(file, 'utf8')
  for (const re of [ARBITRARY_SIZE, ARBITRARY_WEIGHT, ARBITRARY_LEADING]) {
    for (const m of source.matchAll(re)) {
      fail(file, lineAt(source, m.index), `${m[0]}; use a text-*, font-* or leading-* utility from the type tokens`)
    }
  }
  if (file.endsWith('.ts')) continue
  if (file.endsWith('.css')) {
    checkCss(file, source, 0, source)
    continue
  }
  // Inline styles in the template.
  for (const m of source.matchAll(/\sstyle="[^"]*font-(?:size|weight)\s*:[^"]*"/g)) {
    fail(file, lineAt(source, m.index), 'inline font size or weight; use the type utilities')
  }
  // SFC style blocks: at the start of a line, outside the script.
  for (const m of source.matchAll(/^<style\b[^>]*>([\s\S]*?)^<\/style>/gm)) {
    checkCss(file, m[1], m.index + m[0].indexOf('>') + 1, source)
  }
}

if (failures.length > 0) {
  console.error('check-type-tokens: font sizes and weights must come from the type tokens.\n')
  for (const f of failures) console.error(`  ${f}`)
  console.error(`\n${failures.length} found.`)
  process.exit(1)
}
console.log('check-type-tokens: ok')
