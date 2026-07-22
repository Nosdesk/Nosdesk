// Build the three bridge actors from real source with esbuild:
//   dist/runtime.js  <- packages/plugin-runtime/src/runtime.ts  (the REAL runtime)
//   dist/host.js     <- src/host-entry.ts   (uses the REAL @nosdesk/plugin-sdk host side)
//   dist/plugin.js   <- src/plugin.ts        (a trivial plugin calling api.*)
//
// @nosdesk/plugin-sdk is aliased to its TS source; comlink resolves from the
// SDK's own node_modules (esbuild walks up from the aliased file). No type
// erasure surprises: types.ts's domain imports are all `import type`.
import * as esbuild from 'esbuild';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const DIR = dirname(fileURLToPath(import.meta.url));
const R = (p) => resolve(DIR, p);

const common = {
  bundle: true,
  format: 'esm',
  target: 'es2022',
  logLevel: 'info',
  alias: {
    '@nosdesk/plugin-sdk': R('../../packages/plugin-sdk/src/index.ts'),
  },
};

await Promise.all([
  esbuild.build({ ...common, entryPoints: [R('../../packages/plugin-runtime/src/runtime.ts')], outfile: R('dist/runtime.js') }),
  esbuild.build({ ...common, entryPoints: [R('src/host-entry.ts')], outfile: R('dist/host.js') }),
  esbuild.build({ ...common, entryPoints: [R('src/plugin.ts')], outfile: R('dist/plugin.js') }),
]);
console.log('bridge harness built -> dist/');
