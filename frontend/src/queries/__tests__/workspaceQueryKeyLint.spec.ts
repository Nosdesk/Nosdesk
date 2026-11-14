// @vitest-environment node
/**
 * The workspace-query-key lint rule: a literal key handed to raw Colada warns;
 * one that goes through the workspace helpers, or belongs to the account,
 * doesn't.
 */
import { describe, it } from 'vitest'
import { RuleTester } from 'eslint'
import tseslint from 'typescript-eslint'
import rule from '../../../scripts/eslint-rules/workspace-query-key.mjs'

RuleTester.describe = describe
RuleTester.it = it

const tester = new RuleTester({
  languageOptions: { parser: tseslint.parser, ecmaVersion: 'latest', sourceType: 'module' },
})

const error = { messageId: 'literalKey' }

tester.run('workspace-query-key', rule, {
  valid: [
    "useWorkspaceQuery({ key: ['tickets'], query })",
    "useWorkspaceQuery({ key: () => ['ticket', id.value], query })",
    "useQuery({ key: () => workspaceKey('tickets'), query })",
    "useQuery({ key: () => [...workspaceKey('tickets'), page.value], query })",
    "const cache = useWorkspaceQueryCache(); cache.invalidateQueries({ key: ['tickets'] })",
    "const cache = useWorkspaceQueryCache(); cache.setQueryData(['tickets'], [])",
    "useWorkspaceQueryCache().getQueryData(['tickets'])",
    `import { MY_WORKSPACES_KEY } from '@/stores/accountQueryKeys'
     useQuery({ key: () => [...MY_WORKSPACES_KEY, uuid], query })`,
    // A key built elsewhere isn't a literal here.
    'useQuery({ key: () => ticketDetailKey(id), query })',
    // Not a query.
    "useRoute({ key: ['tickets'] })",
    "items.push(['x'])",
  ],
  invalid: [
    { code: "useQuery({ key: ['tickets'], query })", errors: [error] },
    { code: "useQuery({ key: () => ['ticket', id.value], query })", errors: [error] },
    { code: "useQuery({ key: ['admin-edition'] as const, query })", errors: [error] },
    {
      code: "useInfiniteQuery(() => ({ key: ['audit', filters.value], query, initialPageParam: 1 }))",
      errors: [error],
    },
    {
      code: "useInfiniteQuery(() => { return { key: ['audit'], query } })",
      errors: [error],
    },
    {
      code: "const queryCache = useQueryCache(); queryCache.invalidateQueries({ key: ['tickets'] })",
      errors: [error],
    },
    { code: "useQueryCache().setQueryData(['tickets', 7], ticket)", errors: [error] },
    { code: "cache.getQueryData(['tickets'])", errors: [error] },
    { code: "cache.cancelQueries({ key: ['tickets'], exact: true })", errors: [error] },
    {
      // Spreading a key that isn't the account's is still unscoped.
      code: `import { TICKETS } from '@/queries/keys'
             useQuery({ key: () => [...TICKETS, id], query })`,
      errors: [error],
    },
  ],
})
