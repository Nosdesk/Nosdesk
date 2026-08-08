#!/usr/bin/env node
/**
 * Guard against a display utility passed to a component that already sets one.
 *
 * Vue MERGES a class passed to a component with the class on that component's
 * root element. Two display utilities then coexist in one class list, and which
 * one wins is decided by their order in the generated stylesheet, not by the
 * order they appear in the attribute. The result is a control that ignores the
 * class you gave it.
 *
 * This shipped. `<ListDensityToggle class="hidden lg:inline-flex">` merged with
 * the toggle's own root `inline-flex`, `hidden` lost, and a desktop-only
 * control rendered on phones and overflowed the toolbar (its buttons measured
 * right edges of 400px and 414px against a 390px viewport).
 *
 * The fix is always the same: wrap the component in a plain element and put the
 * display class on the wrapper, as `TicketsHeader` already does.
 *
 *     <div class="hidden lg:block">
 *       <ListDensityToggle ... />
 *     </div>
 *
 * Only flags when BOTH sides declare a display utility, resolved by reading the
 * imported component's own root class. A display class passed to a component
 * whose root sets none is fine and stays quiet.
 *
 * Limits, deliberately: static `class` attributes only. A `:class` binding is
 * computed and cannot be resolved here, and dynamic display switching on a
 * component root is rare enough that a false sense of coverage would be worse
 * than the gap. Multi-root (fragment) components are skipped: Vue cannot
 * auto-inherit attributes onto them, so the merge never happens.
 *
 * Runs in the frontend `lint` script, alongside the other guards.
 */
import { readFileSync, readdirSync, statSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const __dirname = fileURLToPath(new URL('.', import.meta.url))
const srcDir = join(__dirname, '..', 'src')

const DISPLAY = String.raw`(?:flex|inline-flex|block|inline-block|inline|grid|inline-grid|table|contents|hidden)`

/**
 * UNPREFIXED display utilities only.
 *
 * A variant-prefixed one (`print:hidden`, `sm:flex`, `group-hover:hidden`) is
 * not the hazard: either it applies in conditions the base utility does not, or
 * Tailwind emits variants after base utilities so it wins deterministically.
 * The bug is two unconditional display utilities in one class list, where
 * nothing decides between them but stylesheet order — which is what
 * `class="hidden lg:inline-flex"` on a root that already sets `inline-flex`
 * came down to.
 */
const BARE_DISPLAY = new RegExp(String.raw`(?:^|\s)${DISPLAY}(?=\s|$)`, 'g')

/** The unprefixed display utilities in a class list. */
function bareDisplayUtilities(classList) {
  return [...classList.matchAll(BARE_DISPLAY)].map((m) => m[0].trim())
}

function walk(dir, out = []) {
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry)
    const st = statSync(full)
    if (st.isDirectory()) walk(full, out)
    else if (entry.endsWith('.vue')) out.push(full)
  }
  return out
}

/** Template body of an SFC, so `<script>` contents never match. */
function templateOf(source) {
  const open = source.indexOf('<template>')
  if (open === -1) return ''
  const close = source.lastIndexOf('</template>')
  if (close <= open) return ''
  return source.slice(open + '<template>'.length, close)
}

/** Strip HTML comments so commented-out markup is not scanned. */
const stripComments = (s) => s.replace(/<!--[\s\S]*?-->/g, '')

/** Map of local component name -> resolved .vue path, from the SFC's imports. */
function componentImports(source, fromFile) {
  const map = new Map()
  const re = /import\s+([A-Z][A-Za-z0-9_]*)\s*(?:,\s*\{[^}]*\})?\s+from\s+['"]([^'"]+)['"]/g
  let m
  while ((m = re.exec(source))) {
    const [, name, spec] = m
    if (!spec.endsWith('.vue')) continue
    let path
    if (spec.startsWith('@/')) path = join(srcDir, spec.slice(2))
    else if (spec.startsWith('.')) path = resolve(dirname(fromFile), spec)
    else continue // workspace package; not ours to resolve
    map.set(name, path)
  }
  return map
}

/** The static class on a component's single root element, or null when the
 *  component has no single root or no static class on it. */
const rootClassCache = new Map()
function rootClassOf(file) {
  if (rootClassCache.has(file)) return rootClassCache.get(file)
  let result = null
  try {
    const tpl = stripComments(templateOf(readFileSync(file, 'utf8'))).trim()
    // The first tag is the root. If a sibling tag follows at depth 0 the
    // component is multi-root and Vue does not auto-inherit onto it.
    const first = tpl.match(/^<([A-Za-z][\w.-]*)([^>]*)>/)
    if (first) {
      const attrs = first[2]
      const cls = attrs.match(/(?:^|\s)class="([^"]*)"/)
      result = cls ? cls[1] : null
    }
  } catch {
    result = null
  }
  rootClassCache.set(file, result)
  return result
}

const failures = []
let scanned = 0

for (const file of walk(srcDir)) {
  const source = readFileSync(file, 'utf8')
  const imports = componentImports(source, file)
  if (imports.size === 0) continue
  const tpl = stripComments(templateOf(source))
  if (!tpl) continue
  scanned++

  // Every opening tag whose name is an imported component.
  const tagRe = /<([A-Z][A-Za-z0-9_]*)((?:[^>"']|"[^"]*"|'[^']*')*)\/?>/g
  let m
  while ((m = tagRe.exec(tpl))) {
    const [, tag, attrs] = m
    const target = imports.get(tag)
    if (!target) continue
    const cls = attrs.match(/(?:^|\s)class="([^"]*)"/)
    if (!cls) continue
    const passed = bareDisplayUtilities(cls[1])
    if (passed.length === 0) continue

    const rootClass = rootClassOf(target)
    if (!rootClass) continue
    const root = bareDisplayUtilities(rootClass)
    if (root.length === 0) continue

    // Passing the same utility the root already sets is redundant, not a
    // conflict: the merged list says the same thing twice.
    if (passed.every((u) => root.includes(u))) continue

    const line = tpl.slice(0, m.index).split('\n').length
    const offset = source.slice(0, source.indexOf('<template>')).split('\n').length - 1
    failures.push({
      file: file.replace(join(__dirname, '..', '..'), '.'),
      line: line + offset,
      tag,
      passed: cls[1],
      root: rootClass,
    })
  }
}

if (failures.length > 0) {
  console.error('[check-component-display-classes] display utility passed to a component that sets its own\n')
  for (const f of failures) {
    console.error(`  ${f.file}:${f.line}  <${f.tag} class="${f.passed}">`)
    console.error(`    ${f.tag}'s root already sets display: "${f.root}"`)
    console.error(
      '    Vue merges these; the winner is stylesheet order, not attribute order.\n' +
        '    Put the display class on a wrapper element instead.\n',
    )
  }
  process.exit(1)
}

console.log(`[check-component-display-classes] OK (${scanned} templates scanned)`)
