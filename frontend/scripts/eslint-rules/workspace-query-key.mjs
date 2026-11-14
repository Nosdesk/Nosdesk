// ESLint rule: a literal Pinia Colada query key must carry the workspace.
//
// Workspace-scoped keys start `['ws', <slug>]` so two workspaces never share
// a cache entry (src/queries/workspaceQuery.ts). This reports an array
// literal used as a key where nothing adds that prefix: the `key` of a raw
// `useQuery` / `useInfiniteQuery` / `useQueryState` (as the value or as a
// getter's result, in an options object or an options getter), the `key` of
// a filter passed to a raw cache's `invalidateQueries` / `cancelQueries` /
// `setQueriesData` / `getEntries`, and the key argument of a raw cache's
// `setQueryData` / `getQueryData`.
//
// Not reported: a cache bound to `useWorkspaceQueryCache()` (it adds the
// prefix), an array that spreads `workspaceKey(...)`, and an array built from
// a key imported from `@/stores/accountQueryKeys` (account-scoped, shared by
// every workspace). Keys built elsewhere (a key helper, a constant) are not
// literals here and are not seen.

const RAW_QUERY_FNS = new Set(['useQuery', 'useInfiniteQuery', 'useQueryState'])
const CACHE_FILTER_METHODS = new Set([
  'invalidateQueries',
  'cancelQueries',
  'setQueriesData',
  'getEntries',
])
const CACHE_KEY_METHODS = new Set(['setQueryData', 'getQueryData'])
const WORKSPACE_CACHE_FN = 'useWorkspaceQueryCache'
const WORKSPACE_KEY_FN = 'workspaceKey'
const ACCOUNT_KEYS_MODULE = '@/stores/accountQueryKeys'
const TS_WRAPPERS = new Set(['TSAsExpression', 'TSSatisfiesExpression', 'TSNonNullExpression'])

function findVariable(scope, name) {
  for (let s = scope; s; s = s.upper) {
    const variable = s.set.get(name)
    if (variable) return variable
  }
  return null
}

function isCallTo(node, name) {
  return node?.type === 'CallExpression' && node.callee.type === 'Identifier' && node.callee.name === name
}

/** The call an object literal is an argument of, directly or as an options getter's result. */
function callTakingObject(obj) {
  const parent = obj.parent
  if (parent.type === 'CallExpression' && parent.arguments.includes(obj)) return parent
  let fn = null
  if (parent.type === 'ArrowFunctionExpression' && parent.body === obj) fn = parent
  else if (parent.type === 'ReturnStatement') {
    for (let n = parent.parent; n; n = n.parent) {
      if (n.type === 'FunctionExpression' || n.type === 'ArrowFunctionExpression') {
        fn = n
        break
      }
    }
  }
  if (fn && fn.parent.type === 'CallExpression' && fn.parent.arguments.includes(fn)) return fn.parent
  return null
}

/** @type {import('eslint').Rule.RuleModule} */
export default {
  meta: {
    type: 'suggestion',
    docs: { description: 'Require workspace-scoped Pinia Colada query keys' },
    schema: [],
    messages: {
      literalKey:
        'This query key has no workspace, so workspaces would share its cache entry. Use useWorkspaceQuery, useWorkspaceInfiniteQuery or useWorkspaceQueryCache from @/queries/workspaceQuery (or build the key with workspaceKey(...)). A key that belongs to the account goes in @/stores/accountQueryKeys.',
    },
  },
  create(context) {
    const sourceCode = context.sourceCode

    function resolvesTo(identifier, test) {
      const variable = findVariable(sourceCode.getScope(identifier), identifier.name)
      return !!variable && variable.defs.some(test)
    }

    function isWorkspaceCache(node) {
      if (isCallTo(node, WORKSPACE_CACHE_FN)) return true
      if (node.type !== 'Identifier') return false
      return resolvesTo(
        node,
        (def) => def.type === 'Variable' && isCallTo(def.node.init, WORKSPACE_CACHE_FN),
      )
    }

    function isAccountKey(node) {
      return (
        node?.type === 'Identifier' &&
        resolvesTo(
          node,
          (def) => def.type === 'ImportBinding' && def.parent.source.value === ACCOUNT_KEYS_MODULE,
        )
      )
    }

    /** True when an array literal already carries a scope: the workspace's, or the account's. */
    function isScoped(array) {
      return array.elements.some((el) => {
        if (!el) return false
        const inner = el.type === 'SpreadElement' ? el.argument : el
        return isCallTo(inner, WORKSPACE_KEY_FN) || isAccountKey(inner)
      })
    }

    function isRawQueryCall(call) {
      return call.callee.type === 'Identifier' && RAW_QUERY_FNS.has(call.callee.name)
    }

    function isRawCacheCall(call, methods) {
      const callee = call.callee
      return (
        callee.type === 'MemberExpression' &&
        !callee.computed &&
        callee.property.type === 'Identifier' &&
        methods.has(callee.property.name) &&
        !isWorkspaceCache(callee.object)
      )
    }

    /** `{ key: [...] }` or `{ key: () => [...] }` in an object a raw query or cache call takes. */
    function isKeyOfRawCall(keyProperty) {
      const obj = keyProperty.parent
      if (obj.type !== 'ObjectExpression') return false
      const call = callTakingObject(obj)
      if (!call) return false
      return isRawQueryCall(call) || isRawCacheCall(call, CACHE_FILTER_METHODS)
    }

    function isKeyProperty(node) {
      return (
        node?.type === 'Property' &&
        !node.computed &&
        ((node.key.type === 'Identifier' && node.key.name === 'key') ||
          (node.key.type === 'Literal' && node.key.value === 'key'))
      )
    }

    return {
      ArrayExpression(array) {
        if (isScoped(array)) return
        // `[...] as const` and the like: the key is the outermost expression.
        let outer = array
        while (TS_WRAPPERS.has(outer.parent.type)) outer = outer.parent
        const parent = outer.parent
        let keyProperty = null
        if (isKeyProperty(parent) && parent.value === outer) keyProperty = parent
        else if (
          parent.type === 'ArrowFunctionExpression' &&
          parent.body === outer &&
          isKeyProperty(parent.parent) &&
          parent.parent.value === parent
        ) {
          keyProperty = parent.parent
        }

        if (keyProperty) {
          if (isKeyOfRawCall(keyProperty)) context.report({ node: array, messageId: 'literalKey' })
          return
        }

        if (
          parent.type === 'CallExpression' &&
          parent.arguments[0] === outer &&
          isRawCacheCall(parent, CACHE_KEY_METHODS)
        ) {
          context.report({ node: array, messageId: 'literalKey' })
        }
      },
    }
  },
}
