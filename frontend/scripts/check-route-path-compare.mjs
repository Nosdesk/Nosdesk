/**
 * The current route is matched by name, meta or `workspacePath(route)`
 * (`frontend/src/router/workspaceRouting.ts`), never by comparing
 * `route.path` to an app path. On Cloud every app path starts with the
 * workspace slug (`/acme/assets/catalog`), so `route.path === '/assets/catalog'`
 * holds self-hosted and never on Cloud. This fails on such a comparison:
 * `===` / `!==` either way round, `.startsWith(` / `.endsWith(` / `.includes(`
 * / `.match(` on the path, or a regex `.test(route.path)`. It also fails on
 * `route.path` handed to a function or kept in a variable, where the
 * comparison happens out of sight (`isActive(route.path)`). Watching the path
 * and navigating to it (`{ path: route.path, query }`) are fine: the path
 * already carries the slug. `fullPath` as a cache key is fine too.
 *
 * Skipped: the router itself, which handles the slug, and the requester
 * portal, which has its own router and serves bare paths in both modes. A
 * comparison that is right as written (the target is a public route that
 * never takes a slug) carries `route-path-compare: <reason>` in a comment on
 * the same line.
 */
import { readFileSync, readdirSync, statSync } from 'node:fs'
import { join, relative } from 'node:path'

const REPO = new URL('../..', import.meta.url).pathname
const ROOTS = ['frontend/src', 'packages/core/src']

const SKIP = ['frontend/src/router/', 'frontend/src/portal/']

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

// The current route's path: `route.path`, `$route.fullPath`,
// `router.currentRoute.value.path`, `viewRoute.path`, a guard's `to.path`.
const ROUTE = String.raw`(?:\$?route|\w*Route(?:\.value)?|\bto|\bfrom)`
const REF = String.raw`${ROUTE}\.(?:path|fullPath)\b`
const PATH = String.raw`${ROUTE}\.path\b`
const COMPARES = [
  new RegExp(String.raw`${REF}\s*[!=]==?`),
  new RegExp(String.raw`[!=]==?\s*${REF}`),
  new RegExp(String.raw`${REF}\s*\.\s*(?:startsWith|endsWith|includes|match)\s*\(`),
  new RegExp(String.raw`\.test\(\s*${REF}`),
  // `route.path` as an argument or a variable's value.
  new RegExp(String.raw`\(\s*${PATH}\s*[,)]`),
  new RegExp(String.raw`[^=!<>]=\s*${PATH}\s*;?\s*$`),
]

/** Comment lines carry examples, not code. */
const isComment = (line) => /^\s*(\*|\/\/|\/\*|<!--)/.test(line)

const problems = []
for (const root of ROOTS) {
  for (const file of walk(join(REPO, root))) {
    const rel = relative(REPO, file)
    if (SKIP.some((s) => rel.startsWith(s))) continue
    const lines = readFileSync(file, 'utf8').split('\n')
    lines.forEach((line, i) => {
      if (isComment(line) || line.includes('route-path-compare:')) return
      if (COMPARES.some((re) => re.test(line))) problems.push(`${rel}:${i + 1}: ${line.trim()}`)
    })
  }
}

if (problems.length) {
  console.error(
    'The current route path compared to an app path; on Cloud it starts with the workspace slug.\n' +
      'Match the route name or meta, or compare workspacePath(route):\n  ' +
      problems.join('\n  '),
  )
  process.exit(1)
}
console.log('check-route-path-compare: ok')
