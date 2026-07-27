// Generate the backend's machine-readable slot allowlist from the single source
// of truth in src/types/pluginSlots.ts. Run via `pnpm --filter @nosdesk/core
// build:slots`; CI regenerates and `git diff --exit-code`s the output, so a
// registry edit that wasn't regenerated fails the build (mirrors runtime.js).
//
// Node (>=24) strips the TypeScript types from the imported registry natively,
// so no bundler is needed — the registry is pure data with no runtime imports.

import { writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';
import { SLOT_REGISTRY } from '../src/types/pluginSlots.ts';

const here = dirname(fileURLToPath(import.meta.url));
const out = resolve(here, '../../../backend/src/services/plugins/plugin_slots.generated.json');

writeFileSync(out, JSON.stringify(SLOT_REGISTRY, null, 2) + '\n');
console.log(`Wrote ${SLOT_REGISTRY.length} slot definitions to ${out}`);
