// Flat ESLint config for the Vue 3 + TypeScript frontend.
//
// Stack:
//   - eslint-plugin-vue (flat/essential)  parser + Vue 3 bug-catching rules.
//   - @vue/eslint-config-typescript       wires vue-eslint-parser to use
//                                         @typescript-eslint/parser inside
//                                         <script lang="ts">. Recommended
//                                         TS rules without type-check (the
//                                         type-aware preset doubles lint
//                                         time and is redundant alongside
//                                         our existing vue-tsc step).
//
// Why "essential" not "strongly-recommended" or "recommended":
//   The codebase predates ESLint, so layering style rules on top would
//   produce thousands of warnings on day one. essential limits us to
//   correctness rules, which we can act on. Style/strict tiers can be
//   added in a follow-up PR.
//
// Companion check: scripts/check-view-roots.mjs enforces the contextual
// "route views must be single-root" rule that's specific to App.vue's
// <Transition mode="out-in">. eslint-plugin-vue dropped multi-root
// detection for Vue 3 (fragments are legal at the language level), so
// that script remains the only enforcement of this constraint and is
// invoked alongside ESLint via `npm run lint`.

import pluginVue from 'eslint-plugin-vue'
import unusedImports from 'eslint-plugin-unused-imports'
import { defineConfigWithVueTs, vueTsConfigs } from '@vue/eslint-config-typescript'

export default defineConfigWithVueTs(
  {
    // Anything ESLint would otherwise crawl that we don't author.
    ignores: [
      'dist/**',
      'node_modules/**',
      'public/**',
      'scripts/**',
    ],
  },

  pluginVue.configs['flat/essential'],
  vueTsConfigs.recommended,

  {
    plugins: {
      // Auto-fixable companion to no-unused-vars: removes unused
      // import declarations (the TS rule reports them but won't
      // strip them). Kept on so future unused imports get cleaned
      // up by `npm run lint:fix`.
      'unused-imports': unusedImports,
    },
    rules: {
      // View files use PascalCase filenames as their de-facto component
      // name. defineOptions({ name }) is added on a per-file basis when
      // the framework needs the name (KeepAlive matching, devtools).
      'vue/multi-word-component-names': 'off',

      // Auto-strip unused imports. Pairs with no-unused-vars below
      // (which keeps reporting unused locals/args/caught errors).
      'unused-imports/no-unused-imports': 'error',

      // The codebase is heavy on intentional `any` at integration
      // boundaries (axios error shapes, pre-typed third-party code).
      // Promote to 'warn' later once those sites are typed.
      '@typescript-eslint/no-explicit-any': 'off',

      // Frequent in event handlers like `(e) => void store.action()` and
      // in defineEmits typings. Strict mode would flag valid patterns.
      '@typescript-eslint/no-unused-expressions': 'off',

      // False-positive on TypeScript union types in template attribute
      // bindings, e.g. `:status="value as 'open' | 'closed'"` reads
      // the type-cast `|` as a Vue 1.x filter pipe. The codebase
      // doesn't use Vue 1 filters anywhere (Vue 3 removed them), so
      // disabling the rule loses no real coverage.
      'vue/no-deprecated-filter': 'off',

      // Allow `_prefix` to mark intentionally unused params/destructures,
      // matching the convention already in use across the codebase.
      '@typescript-eslint/no-unused-vars': [
        'error',
        {
          argsIgnorePattern: '^_',
          varsIgnorePattern: '^_',
          caughtErrorsIgnorePattern: '^_',
          destructuredArrayIgnorePattern: '^_',
        },
      ],
    },
  },
)
