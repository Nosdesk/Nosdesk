/**
 * A link to a ticket in the web apps comes from `ticketRoute` /
 * `ticketRouteById` (`packages/core/src/utils/ticketRoutes.ts`) or the paths
 * built on them, never a hand-built `/tickets/${...}`. The route takes the
 * number people know the ticket by; an id there opens a different ticket, or
 * none. A template literal or concatenation starting `/tickets/` outside the
 * helper fails this check.
 *
 * API calls name tickets by id under `/api` and are not app routes: the
 * service modules that make them are skipped.
 */
import { readFileSync, readdirSync, statSync } from 'node:fs'
import { join, relative } from 'node:path'

const REPO = new URL('../..', import.meta.url).pathname
const ROOTS = ['frontend/src', 'packages/core/src', 'mobile/src']

// The helper itself, and modules that only build API request paths.
const SKIP = [
  'packages/core/src/utils/ticketRoutes.ts',
  'packages/core/src/services/',
  'frontend/src/services/',
  'frontend/src/portal/service.ts',
]

function walk(dir, out = []) {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name)
    if (statSync(p).isDirectory()) {
      if (name === 'node_modules' || name === '__tests__') continue
      walk(p, out)
    } else if (/\.(ts|vue)$/.test(name) && !/\.spec\.ts$/.test(name)) {
      out.push(p)
    }
  }
  return out
}

// `/tickets/${…}` or `/tickets/id/${…}` opening a template literal or
// following an interpolation (`${origin}/tickets/${n}`), or the same prefix
// + … in a concatenation. API paths such as `/collaboration/tickets/${id}`
// don't match.
const HAND_BUILT = /[`}]\/tickets\/(?:id\/)?\$\{|['"]\/tickets\/(?:id\/)?['"]\s*\+/g

const problems = []
for (const root of ROOTS) {
  for (const file of walk(join(REPO, root))) {
    const rel = relative(REPO, file)
    if (SKIP.some((s) => rel === s || (s.endsWith('/') && rel.startsWith(s)))) continue
    const text = readFileSync(file, 'utf8')
    for (const m of text.matchAll(HAND_BUILT)) {
      const line = text.slice(0, m.index).split('\n').length
      problems.push(`${rel}:${line}`)
    }
  }
}

if (problems.length) {
  console.error(
    'Ticket routes built by hand (use ticketRoute / ticketRouteById / ticketPath):\n  ' +
      problems.join('\n  '),
  )
  process.exit(1)
}
console.log('check-ticket-routes: ok')
