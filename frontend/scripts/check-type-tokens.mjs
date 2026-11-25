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
 *   - The same through other doors: the `font` shorthand, Tailwind's
 *     arbitrary properties (`[font-size:13px]`), a bound `:style`, SVG
 *     `font-size` and `font-weight` attributes, keyword and viewport
 *     sizes (`small`, `2vw`), and a `--text-*` token held as a plain
 *     length outside `.type-fixed`.
 *
 * Covers frontend/src and every packages/<name>/src. A file that cannot
 * read the tokens at all is listed in ALLOWED with the reason. Runs in the frontend `lint` script, alongside the other guards.
 */
import { readFileSync, readdirSync, statSync } from 'node:fs'
import { join, relative } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = join(fileURLToPath(new URL('.', import.meta.url)), '..')
const srcDir = join(root, 'src')
const packagesDir = join(root, '..', 'packages')

/** Files whose type cannot come from the tokens, and why. */
const ALLOWED = {
  // The email renders in a sandboxed iframe document, which the app's
  // custom properties do not reach.
  'src/components/ticketComponents/EmailHtmlBody.vue': 'iframe document',
  // The plugin kit styles each plugin's own sandboxed document, which has
  // its own --nd-* tokens and no access to the app's.
  '../packages/plugin-runtime/src/pluginUiCss.ts': 'plugin sandbox document',
  // A monogram drawn as an SVG image (a data URL), not page text.
  'src/utils/monogram.ts': 'SVG image',
  // The add-widget thumbnails: aria-hidden drawings in their own viewBox
  // units, scaled with the picker cell; their numbers are decoration.
  'src/views/dashboard/WidgetPreview.vue': 'aria-hidden illustration',
  // The 404 numerals are sized to fill the viewport, then drawn into the
  // background shader.
  'src/views/ErrorView.vue': 'numerals sized to the viewport',
  // A hidden input focused to raise the keyboard early; 16px keeps iOS
  // from zooming, and it is never seen.
  'src/composables/keyboardPrimer.ts': 'hidden input',
}

const LENGTH_SIZE = /^-?[\d.]+(?:px|pt|rem)$/
/** A size built from lengths alone, `max(1rem, 16px)`: no token in it. */
const fixedLength = (value) => LENGTH_SIZE.test(value) || (/\d(?:px|pt|rem)\b/.test(value) && !/var\(--(?:text|leading)-/.test(value))
/** A keyword or viewport size: `small`, `x-large`, `2vw`. */
const OTHER_SIZE = /^(?:xx-small|x-small|small|medium|large|x-large|xx-large|xxx-large|smaller|-?[\d.]+(?:vw|vh|vmin|vmax|svw|svh|dvw|dvh|lvw|lvh|cqw|cqh))$/
const ARBITRARY_SIZE = /(?<![\w-])text-\[(?:length:)?[\d.]+(?:px|pt|rem|vw|vh)\]/g
const ARBITRARY_WEIGHT = /(?<![\w-])font-\[\d+\]/g
const ARBITRARY_LEADING = /(?<![\w-])leading-\[[\d.]+(?:px|pt|rem)\]/g
/** Tailwind's arbitrary properties: `[font-size:13px]`, `[font:600_13px/1.4_Inter]`. */
const ARBITRARY_PROPERTY = /(?<![\w-])\[(?:font-size|font-weight|line-height|font):[^\]]+\]/g
/** SVG presentation attributes with a literal value: `font-size="11"`. */
const SVG_ATTRIBUTE = /(?<![\w:-])(?:font-size|font-weight)=["'][^"']*\d[^"']*["']|:(?:font-size|font-weight)=["']\s*[\d.]+\s*["']/g
/** A bound style that sets a font size or weight. */
const BOUND_STYLE = /:style="[^"]*\b(?:fontSize|fontWeight|font-size|font-weight)\b[^"]*"/g

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
    if (d.prop === 'font-size' && !print && OTHER_SIZE.test(value)) {
      fail(file, where, `font-size: ${value}; keyword and viewport sizes ignore the text size, use a --text-* token`)
    }
    // The shorthand sets size, weight and line height at once.
    if (d.prop === 'font' && !print && !fontFace && !/^(inherit|initial|unset|revert|-apple-system-[\w-]+|caption|menu|message-box|small-caption|status-bar|icon)$/.test(value)) {
      const parts = value.split(/\s+/)
      const size = parts.find((p) => /^[\d.]+(?:px|pt|rem|em|%|vw|vh)(?:\/|$)/.test(p) || /^var\(--text-/.test(p))
      if (!size || !/^var\(--text-/.test(size)) {
        fail(file, where, `font: ${value}; set font-size from a --text-* token (the shorthand hides a fixed size)`)
      }
      const weight = parts.find((p) => /^(?:\d{3}|bold|bolder|lighter)$/.test(p))
      if (weight) fail(file, where, `font: ${value}; a literal weight in the shorthand; use font-weight: var(--font-weight-*)`)
    }
    if ((d.prop === 'line-height' || /^--text-[\w-]+--line-height$/.test(d.prop)) && !print && fixedLength(value)) {
      fail(file, where, `${d.prop}: ${value}; a fixed line box clips larger text, use a ratio or a --leading-* token`)
    }
    // A size token held as a plain length does not follow the scale. Only
    // `.type-fixed`, which pins sizes inside fixed boxes on purpose, may.
    const fixedBox = d.context.some((c) => /(^|[\s,])\.type-fixed\b/.test(c))
    if (/^--text-[\w-]+$/.test(d.prop) && !/--line-height|--letter-spacing|--font-weight|^--text-color-/.test(d.prop) && LENGTH_SIZE.test(value) && !fixedBox) {
      fail(file, where, `${d.prop}: ${value}; a size token multiplies by --text-scale (calc(${value} * var(--text-scale)))`)
    }
    if (d.prop === 'font-weight' && !fontFace && /^(\d+|bold|bolder|normal|lighter)$/.test(value)) {
      fail(file, where, `font-weight: ${value}; use a --font-weight-* token (var(--font-weight-semibold)) so Bold Text can remap it`)
    }
  }
}

const packageSources = readdirSync(packagesDir)
  .map((name) => join(packagesDir, name, 'src'))
  .filter((dir) => { try { return statSync(dir).isDirectory() } catch { return false } })
  .flatMap((dir) => walk(dir))

for (const file of [...walk(srcDir), ...packageSources]) {
  const rel = relative(root, file)
  if (ALLOWED[rel]) continue
  const source = readFileSync(file, 'utf8')
  for (const re of [ARBITRARY_SIZE, ARBITRARY_WEIGHT, ARBITRARY_LEADING, ARBITRARY_PROPERTY]) {
    for (const m of source.matchAll(re)) {
      fail(file, lineAt(source, m.index), `${m[0]}; use a text-*, font-* or leading-* utility from the type tokens`)
    }
  }
  // SVG text attributes, in templates and in markup built in script.
  for (const m of source.matchAll(SVG_ATTRIBUTE)) {
    fail(file, lineAt(source, m.index), `${m[0]}; size SVG text with a text-* class (or a size read from one) so it follows the text size`)
  }
  if (file.endsWith('.ts')) continue
  if (file.endsWith('.css')) {
    checkCss(file, source, 0, source)
    continue
  }
  // Inline styles in the template.
  for (const m of source.matchAll(/(?<![\w:-])style="[^"]*font(?:-size|-weight)?\s*:[^"]*"/g)) {
    fail(file, lineAt(source, m.index), 'inline font size or weight; use the type utilities')
  }
  for (const m of source.matchAll(BOUND_STYLE)) {
    fail(file, lineAt(source, m.index), 'bound font size or weight; use the type utilities')
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
