#!/usr/bin/env node
/**
 * Text fields stay at 16px or more on touch, and the viewport lets people zoom.
 *
 * iOS zooms the page into a field under 16px when it takes focus. The app
 * used to hide that with `maximum-scale=1.0` in the viewport, which also
 * blocked pinch zoom for everyone (WCAG 1.4.4; the ACT rule "Meta viewport
 * allows for zoom"). The cap is gone, so every field has to be 16px or more
 * on a touch screen instead.
 *
 * `main.css` sets the floor: under `(pointer: coarse)` a field takes
 * `max(1rem, 1em)` and the small `--text-*` tokens are redeclared on it as
 * body size, so `text-sm` on an <input> renders 16px there. What gets around
 * the floor, and what this script refuses:
 *
 *   1. A viewport meta with `maximum-scale` under 2 or `user-scalable=no`.
 *   2. An arbitrary size class (`text-[13px]`) or an inline font size on a
 *      field: neither reads the tokens.
 *   3. A size utility under body size (`text-sm`) on a contenteditable
 *      itself, in its template or in an editor's `attributes: { class }`,
 *      without `pointer-coarse:text-base`. The token floor covers inputs,
 *      textareas and selects, not editors, whose content must keep the
 *      sizes the posted text shows.
 *   4. A `font-size` under 16px in scoped or global CSS on a rule that
 *      targets a field (an input, textarea, select, contenteditable,
 *      ProseMirror, or a class a field in the same file carries), unless
 *      the same selector is raised in a `@media (pointer: coarse)` block.
 *      Print styles are skipped.
 *
 * Runs in the frontend `lint` script, alongside the other guards.
 */
import { readFileSync, readdirSync, statSync } from 'node:fs'
import { join, relative } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = join(fileURLToPath(new URL('.', import.meta.url)), '..')
const srcDir = join(root, 'src')

const NOT_TEXT = new Set(['checkbox', 'radio', 'range', 'color', 'file', 'button', 'submit', 'reset', 'image', 'hidden'])
/** Reka parts and our own primitives that render a text field. */
const FIELD_COMPONENT = /^[A-Z][A-Za-z]*(?:Input|Filter)$/
const FIELD_SELECTOR = /(?:^|[^\w-])(?:input|textarea|select)(?![\w-])|\[contenteditable|\.ProseMirror(?![\w-])/

/** A size utility under body size, unprefixed. */
const SMALL_SIZE = /(?:^|\s)text-(?:5xs|4xs|3xs|2xs|xs|xs-plus|sm|sm-plus)(?=\s|$)/
const smallOnTouch = (classes) => SMALL_SIZE.test(` ${classes} `) && !/pointer-coarse:text-(?:base|lg|xl)/.test(classes)
const editable = (attrs) => /\scontenteditable(?:=(?:"(?!false)[^"]*"|'(?!false)[^']*'))?(?=[\s>]|$)/.test(attrs)

const failures = []
const fail = (file, message) => failures.push(`${relative(root, file)}: ${message}`)

// 1. The viewport.
for (const page of ['index.html', 'portal.html']) {
  const file = join(root, page)
  const html = readFileSync(file, 'utf8')
  const meta = /<meta\s+name="viewport"\s+content="([^"]*)"/i.exec(html)
  if (!meta) {
    fail(file, 'no viewport meta')
    continue
  }
  const content = meta[1].toLowerCase()
  const max = /maximum-scale\s*=\s*([\d.]+)/.exec(content)
  if (max && Number(max[1]) < 2) fail(file, `viewport sets maximum-scale=${max[1]}, which blocks zoom`)
  if (/user-scalable\s*=\s*(?:no|0)\b/.test(content)) fail(file, 'viewport sets user-scalable=no, which blocks zoom')
}

function walk(dir, out = []) {
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry)
    if (statSync(full).isDirectory()) {
      if (entry !== '__tests__') walk(full, out)
    } else if (entry.endsWith('.vue') || (entry.endsWith('.ts') && !entry.endsWith('.spec.ts'))) out.push(full)
  }
  return out
}

function templateOf(source) {
  const open = source.indexOf('<template')
  const close = source.lastIndexOf('</template>')
  return open === -1 || close <= open ? '' : source.slice(open, close)
}

/** Every opening tag in a template, with its name and attribute text. */
function* tags(template) {
  const re = /<([A-Za-z][\w.-]*)((?:\s+(?:[^\s"'>/=]+)(?:\s*=\s*(?:"[^"]*"|'[^']*'))?)*)\s*\/?>/g
  for (const m of template.matchAll(re)) yield { name: m[1], attrs: m[2], at: m.index }
}

function isField({ name, attrs }) {
  if (name === 'textarea' || name === 'select') return true
  if (name === 'input') {
    const type = /\stype="([^"]+)"/.exec(attrs)
    return !type || !NOT_TEXT.has(type[1])
  }
  if (FIELD_COMPONENT.test(name)) return true
  if (name === 'component' && /\s:is="[^"]*Input\b/.test(attrs)) return true
  return /\scontenteditable(?:=(?:"(?!false)[^"]*"|'(?!false)[^']*'))?(?=[\s>]|$)/.test(attrs)
}

const lineAt = (text, index) => text.slice(0, index).split('\n').length

/** The rules of a stylesheet, each with its selector and the at-rules around it. */
function rules(css) {
  const out = []
  const text = css.replace(/\/\*[\s\S]*?\*\//g, (c) => c.replace(/[^\n]/g, ' '))
  let i = 0
  function block(context) {
    while (i < text.length) {
      const open = text.indexOf('{', i)
      const close = text.indexOf('}', i)
      if (close !== -1 && (open === -1 || close < open)) {
        i = close + 1
        return
      }
      if (open === -1) {
        i = text.length
        return
      }
      const prelude = text.slice(i, open).trim()
      const start = open
      i = open + 1
      if (prelude.startsWith('@')) {
        block([...context, prelude])
      } else {
        const end = text.indexOf('}', i)
        const body = text.slice(i, end)
        // A nested rule (CSS nesting) is rare here; treat the body flat.
        out.push({ selector: prelude, body, context, at: start })
        i = end + 1
      }
    }
  }
  block([])
  return out
}

/** A font size that is 16px or more, or reads a token of body size or up. */
function touchSafe(value) {
  const v = value.replace(/!important/, '').trim()
  if (/^(inherit|unset|initial|revert|larger)$/.test(v)) return true
  if (/^var\(--text-(base|lg|xl|[2-9]xl|metric-[a-z]+)\b/.test(v)) return true
  if (/^max\(/.test(v)) return true
  const n = /^([\d.]+)(px|rem|em)$/.exec(v)
  if (n) return n[2] === 'px' ? Number(n[1]) >= 16 : Number(n[1]) >= 1
  return false
}

/** The last compound of each selector in a list, `:deep()` unwrapped. */
function targets(selector) {
  return selector.split(',').map((s) => {
    const flat = s.replace(/:deep\(([^)]*)\)/g, ' $1').trim()
    const parts = flat.split(/\s+|\s*[>+~]\s*/).filter(Boolean)
    return { full: s.trim(), last: parts[parts.length - 1] ?? '' }
  })
}

function checkStyles(file, css, offset, source, fieldClasses) {
  const all = rules(css)
  const coarse = new Set()
  for (const r of all) {
    if (r.context.some((c) => /pointer\s*:\s*coarse/.test(c)) && /font-size\s*:/.test(r.body)) {
      for (const t of targets(r.selector)) coarse.add(t.full)
    }
  }
  for (const r of all) {
    if (r.context.some((c) => /\bprint\b|pointer\s*:\s*coarse/.test(c))) continue
    const size = /(?:^|;)\s*font-size\s*:\s*([^;]+)/.exec(r.body)
    if (!size || touchSafe(size[1])) continue
    for (const t of targets(r.selector)) {
      const classes = [...t.last.matchAll(/\.([\w-]+)/g)].map((m) => m[1])
      const hitsField = FIELD_SELECTOR.test(` ${t.last}`) || classes.some((c) => fieldClasses.has(c))
      if (!hitsField || coarse.has(t.full)) continue
      fail(
        file,
        `line ${lineAt(source, offset + r.at)}: \`${t.full}\` sets font-size: ${size[1].trim()} on a text field; ` +
          'raise it to var(--text-base) under @media (pointer: coarse), or size it with a text-* utility',
      )
    }
  }
}

for (const file of walk(srcDir)) {
  const source = readFileSync(file, 'utf8')
  // An editor's own element, given its classes by ProseMirror.
  for (const m of source.matchAll(/attributes:\s*\{[^}]*?\bclass:\s*['"`]([^'"`]*)['"`]/g)) {
    if (smallOnTouch(m[1])) {
      fail(file, `line ${lineAt(source, m.index)}: an editor's own class sets a size under 16px; add pointer-coarse:text-base`)
    }
  }
  if (file.endsWith('.ts')) continue
  const template = templateOf(source)
  const fieldClasses = new Set()
  for (const tag of tags(template)) {
    if (!isField(tag)) continue
    const where = `line ${lineAt(source, source.indexOf(template) + tag.at)}`
    if (/\btext-\[(?!(?:color|var|#|rgb|hsl)[:(]?)[^\]]*\d[^\]]*\]/.test(tag.attrs)) {
      fail(file, `${where}: <${tag.name}> takes an arbitrary text size; use a text-* utility so the touch floor applies`)
    }
    if (/font-?size/i.test(tag.attrs)) {
      fail(file, `${where}: <${tag.name}> sets an inline font size; use a text-* utility so the touch floor applies`)
    }
    const cls = /\sclass="([^"]*)"/.exec(tag.attrs)
    if (cls && editable(tag.attrs) && smallOnTouch(cls[1])) {
      fail(file, `${where}: a contenteditable sets a size under 16px; add pointer-coarse:text-base`)
    }
    if (cls) for (const c of cls[1].split(/\s+/)) if (c) fieldClasses.add(c)
  }
  for (const m of source.matchAll(/<style[^>]*>([\s\S]*?)<\/style>/g)) {
    checkStyles(file, m[1], m.index + m[0].indexOf('>') + 1, source, fieldClasses)
  }
}

const mainCss = join(srcDir, 'assets', 'main.css')
const globalCss = readFileSync(mainCss, 'utf8')
checkStyles(mainCss, globalCss, 0, globalCss, new Set())

if (failures.length > 0) {
  console.error('check-touch-inputs: text fields must stay at 16px or more on touch, and the viewport must allow zoom.\n')
  for (const f of failures) console.error(`  ${f}`)
  process.exit(1)
}
console.log('check-touch-inputs: ok')
