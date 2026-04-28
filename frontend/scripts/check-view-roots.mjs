#!/usr/bin/env node
/**
 * Guard against multi-root template fragments in route views.
 *
 * Vue 3 allows fragment components (multiple root template
 * elements), but `<Transition>` cannot bind transition classes
 * to a fragment. Our App.vue wraps `<RouterView>` in
 * `<Transition mode="out-in">` so any multi-root view causes:
 *   - leave classes never attached
 *   - `transitionend` never fires
 *   - the leaving view's leave never completes
 *   - the entering view never enters → blank page
 *
 * Scans every top-level `*.vue` file under `src/views/` (route
 * views; subdirectories hold sub-components and don't apply)
 * and rejects any whose `<template>` has more than one logical
 * root. Adjacent `v-if` / `v-else-if` / `v-else` elements
 * collapse to one logical root since Vue renders one at a time.
 *
 * Run as `npm run lint:views`. Until we add ESLint properly this
 * is the minimum-viable protection against the multi-root
 * footgun (see commit history and `App.vue`'s KeepAlive comment
 * block for context).
 */
import { readFileSync, readdirSync, statSync } from 'node:fs'
import { join, relative } from 'node:path'
import { fileURLToPath } from 'node:url'
import { parse } from '@vue/compiler-sfc'

const __dirname = fileURLToPath(new URL('.', import.meta.url))
const viewsDir = join(__dirname, '..', 'src', 'views')
const projectRoot = join(__dirname, '..')

/**
 * Pre-existing multi-root views. Each has the same latent bug
 * we just discovered in DevicesListView (Transition deadlock on
 * nav-away), but they haven't been hit yet because users rarely
 * navigate away from them mid-session. Wrap each in a single
 * root element and remove from this list. Reported as warnings
 * so the linter doesn't gate the rest of the codebase on them.
 *
 * Paths are relative to the project root.
 */
const KNOWN_LEGACY_MULTI_ROOT_VIEWS = new Set([
  'src/views/CategoriesManagementView.vue',
  'src/views/ErrorView.vue',
  'src/views/GroupsManagementView.vue',
])

/**
 * Top-level `src/views/*.vue` only. Subdirectories (e.g.
 * `views/dashboard/DashboardEditBar.vue`) hold sub-components
 * that are rendered *inside* a route view, not directly under
 * `<RouterView>`, so the multi-root constraint doesn't apply
 * to them.
 */
function* walkVueFiles(dir) {
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry)
    if (statSync(full).isDirectory()) continue
    if (entry.endsWith('.vue')) yield full
  }
}

/**
 * @vue/compiler-sfc gives us the parsed template AST. The root
 * fragment's `children` array contains every top-level node,
 * including text/whitespace nodes and HTML comments. Filter to
 * actual element nodes, then collapse v-if/v-else-if/v-else
 * chains into one logical root.
 *
 * Element type === 1 (NodeTypes.ELEMENT) per @vue/compiler-core.
 */
function countLogicalRoots(template) {
  if (!template?.ast) return 0
  const elements = (template.ast.children ?? []).filter((n) => n.type === 1)

  let logical = 0
  for (const el of elements) {
    const directives = el.props ?? []
    const isAlt = directives.some(
      (p) =>
        p.type === 7 && // DIRECTIVE
        (p.name === 'else' || p.name === 'else-if'),
    )
    if (!isAlt) logical++
  }
  return logical
}

const failures = []
const legacyWarnings = []
const scanned = []

for (const file of walkVueFiles(viewsDir)) {
  scanned.push(file)
  const rel = relative(projectRoot, file)
  const source = readFileSync(file, 'utf8')
  const { descriptor, errors } = parse(source, { filename: file })
  if (errors.length > 0) {
    console.warn(`[check-view-roots] Skipping ${rel} due to parse errors:`)
    for (const err of errors) console.warn('  ', err.message)
    continue
  }
  const count = countLogicalRoots(descriptor.template)
  if (count <= 1) continue

  if (KNOWN_LEGACY_MULTI_ROOT_VIEWS.has(rel)) {
    legacyWarnings.push({ file: rel, count })
  } else {
    failures.push({ file: rel, count })
  }
}

if (legacyWarnings.length > 0) {
  console.warn('\n[check-view-roots] Pre-existing multi-root views (allowlisted, fix when you can):')
  for (const { file, count } of legacyWarnings) {
    console.warn(`  ${file}: ${count} root siblings`)
  }
}

if (failures.length > 0) {
  console.error('\n[check-view-roots] Multi-root templates detected in src/views:\n')
  for (const { file, count } of failures) {
    console.error(`  ${file}: ${count} root sibling${count === 1 ? '' : 's'}`)
  }
  console.error(
    '\nRoute views must have a single root element so Vue\'s ' +
    '<Transition mode="out-in"> in App.vue can attach leave/enter ' +
    'classes. Wrap siblings in a <div class="h-full"> (the wrapper ' +
    'has zero visual cost; teleported modals can stay inside).\n',
  )
  process.exit(1)
}

// Stale-allowlist guard: if a previously-bad view has been fixed,
// nag to remove it from KNOWN_LEGACY_MULTI_ROOT_VIEWS so the
// allowlist doesn't accumulate dead entries.
const stale = [...KNOWN_LEGACY_MULTI_ROOT_VIEWS].filter(
  (entry) => !legacyWarnings.some((w) => w.file === entry),
)
if (stale.length > 0) {
  console.error(
    '\n[check-view-roots] Allowlist entries are no longer needed (remove them from KNOWN_LEGACY_MULTI_ROOT_VIEWS):',
  )
  for (const entry of stale) console.error(`  ${entry}`)
  process.exit(1)
}

console.log(`[check-view-roots] OK (${scanned.length} views scanned${legacyWarnings.length ? `, ${legacyWarnings.length} legacy warnings` : ''})`)
