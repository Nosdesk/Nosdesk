/**
 * A trigger's `aria-haspopup` must name the surface it opens. Every
 * literal `aria-haspopup="…"` in a file is checked against the literal
 * `role="…"` values the same file hands `<ResponsiveMenu>` or `<Popover>`,
 * and `dialog` for every `<BottomSheet>` (the surfaces a hand-rolled
 * trigger opens); a file with no such surface is left alone. `aria-haspopup="true"` is never allowed: it
 * means `menu`, and says so less clearly than the word.
 *
 * "menu" over a dialog promises arrow keys the dialog does not have;
 * "dialog" over a menu hides the ones it does.
 */
import { readFileSync, readdirSync, statSync } from 'node:fs'
import { join, relative } from 'node:path'

const ROOT = new URL('../src', import.meta.url).pathname
const REPO = new URL('..', import.meta.url).pathname

function walk(dir, out = []) {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name)
    if (statSync(p).isDirectory()) walk(p, out)
    else if (name.endsWith('.vue')) out.push(p)
  }
  return out
}

const HASPOPUP = /(?<![:\w-])aria-haspopup="([^"]*)"/g
const SURFACE = /<(ResponsiveMenu|Popover|BottomSheet)\b[\s\S]*?>/g
const ROLE = /(?<![:\w-])role="([^"]*)"/

const problems = []
for (const file of walk(ROOT)) {
  const text = readFileSync(file, 'utf8')
  const rel = relative(REPO, file)
  const roles = new Set()
  for (const m of text.matchAll(SURFACE)) {
    if (m[1] === 'BottomSheet') {
      roles.add('dialog')
      continue
    }
    const role = m[0].match(ROLE)?.[1]
    if (role) roles.add(role)
  }
  for (const m of text.matchAll(HASPOPUP)) {
    const value = m[1]
    const line = text.slice(0, m.index).split('\n').length
    if (value === 'true') {
      problems.push(`${rel}:${line} aria-haspopup="true": name the surface (menu, dialog, listbox)`)
    } else if (roles.size > 0 && !roles.has(value)) {
      problems.push(`${rel}:${line} aria-haspopup="${value}" but the surface here is role="${[...roles].join('" / "')}"`)
    }
  }
}

if (problems.length) {
  console.error('aria-haspopup does not match the surface it opens:\n  ' + problems.join('\n  '))
  process.exit(1)
}
console.log('check-popup-roles: ok')
