#!/usr/bin/env node
/**
 * Guard against "declared but not mounted" plugin slots.
 *
 * The plugin UI-slots redesign found 8 of 10 slots validated + installed but
 * rendered nothing (a silent no-op). To keep that from regressing, every slot
 * the registry marks `stable` MUST be consumed somewhere in the frontend — the
 * host has to actually render it. Reserved / experimental slots are exempt (a
 * plugin can author against them before the mount lands, the VS Code
 * contribution-point model).
 *
 * "Consumed" = the slot's canonical name appears as a quoted literal in a
 * `src/**` .ts / .vue file outside the registry source. That covers both mount
 * mechanisms: a `<PluginSlot target="x.y.z">` panel mount and the
 * `getSlotRegistrations('x.y.z')` path (the dashboard widget renders through the
 * synthetic-widget shell, not a `<PluginSlot>`).
 *
 * The registry lives in @nosdesk/core; we read the generated JSON the backend
 * also consumes (single source of truth, already in the repo + drift-checked).
 *
 * Runs in the frontend `lint` script, alongside check-view-roots.
 */
import { readFileSync, readdirSync, statSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'

const __dirname = fileURLToPath(new URL('.', import.meta.url))
const srcDir = join(__dirname, '..', 'src')
const registryJson = join(
  __dirname,
  '..',
  '..',
  'backend',
  'src',
  'services',
  'plugins',
  'plugin_slots.generated.json',
)

const registry = JSON.parse(readFileSync(registryJson, 'utf8'))
const stableSlots = registry.filter((s) => s.status === 'stable').map((s) => s.name)

function* walk(dir) {
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry)
    if (statSync(full).isDirectory()) {
      yield* walk(full)
    } else if (entry.endsWith('.ts') || entry.endsWith('.vue')) {
      yield full
    }
  }
}

// One pass over the source: collect the set of slot names that appear as a
// quoted literal anywhere. Cheaper than re-scanning per slot.
const referenced = new Set()
for (const file of walk(srcDir)) {
  const source = readFileSync(file, 'utf8')
  for (const slot of stableSlots) {
    if (referenced.has(slot)) continue
    if (source.includes(`'${slot}'`) || source.includes(`"${slot}"`)) {
      referenced.add(slot)
    }
  }
}

const unmounted = stableSlots.filter((s) => !referenced.has(s))
if (unmounted.length > 0) {
  console.error('\n[check-plugin-slot-mounts] Stable slots with no host mount:\n')
  for (const slot of unmounted) console.error(`  ${slot}`)
  console.error(
    '\nA slot marked `status: "stable"` in packages/core/src/types/pluginSlots.ts ' +
      'must be rendered by the host (a <PluginSlot target="..."> mount or a ' +
      "getSlotRegistrations('...') consumer). Either mount it, or set its status " +
      'to "reserved" until a mount lands.\n',
  )
  process.exit(1)
}

console.log(
  `[check-plugin-slot-mounts] OK (${stableSlots.length} stable slot${stableSlots.length === 1 ? '' : 's'} mounted)`,
)
