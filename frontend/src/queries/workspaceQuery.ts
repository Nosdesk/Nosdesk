/**
 * Query keys that carry the workspace. Every workspace-scoped Pinia Colada key
 * starts `['ws', <slug>]`, or `['ws', 'host']` in host mode (no slug; the
 * origin is the workspace), so two workspaces never share a cache entry.
 *
 * Callers write the rest of the key and these helpers add the prefix: a query
 * through `useWorkspaceQuery`, a paged one through `useWorkspaceInfiniteQuery`,
 * and cache reads and writes through `useWorkspaceQueryCache`. The prefix is
 * read each time a key is built, so a query alive across a workspace switch
 * moves to the new workspace's entry rather than serving the old one.
 *
 * Until the workspace is known (the instance config is loading, or path mode
 * has no slug yet, as at boot and partway through a switch) the queries wait
 * and the cache helpers do nothing. Otherwise the key would fall back to
 * `'host'` and the request would go out with no workspace.
 *
 * Keys that belong to the account rather than a workspace live in
 * `stores/accountQueryKeys` and skip the prefix. The requester portal keeps its
 * own keys. The `nosdesk/workspace-query-key` lint warns on a literal key that
 * bypasses these helpers.
 */
import { toValue, type MaybeRefOrGetter } from 'vue'
import {
  useInfiniteQuery,
  useQuery,
  useQueryCache,
  type DefineInfiniteQueryOptions,
  type EntryKey,
  type UseInfiniteQueryData,
  type UseInfiniteQueryOptions,
  type UseInfiniteQueryReturn,
  type UseQueryEntryFilter,
  type UseQueryOptions,
  type UseQueryReturn,
} from '@pinia/colada'
import { activeWorkspaceSlug, workspaceReady } from '@/services/activeWorkspace'

/** First element of every workspace-scoped key. */
export const WORKSPACE_KEY_ROOT = 'ws'

/** Stands in for the slug in host mode, where the origin is the workspace. */
export const HOST_WORKSPACE_KEY = 'host'

type KeyPart = EntryKey[number]

/**
 * The full cache key for `parts` in the active workspace. Reads the slug
 * reactively, so inside a getter or computed the key follows a switch.
 * With no parts it is the workspace's own prefix, which matches every entry
 * of the active workspace.
 */
export function workspaceKey(...parts: KeyPart[]): EntryKey {
  return [WORKSPACE_KEY_ROOT, activeWorkspaceSlug() ?? HOST_WORKSPACE_KEY, ...parts]
}

/** The caller's `enabled`, held off until the workspace is known. */
function whenWorkspaceReady(enabled: MaybeRefOrGetter<boolean> | undefined): () => boolean {
  return () => workspaceReady() && (toValue(enabled) ?? true)
}

/** `useQuery` options whose `key` is the part after the workspace prefix. */
export type WorkspaceQueryOptions<
  TData,
  TError = Error,
  TDataInitial extends TData | undefined = undefined,
> = Omit<UseQueryOptions<TData, TError, TDataInitial>, 'key'> & {
  key: MaybeRefOrGetter<EntryKey>
}

/**
 * `useQuery` with the key scoped to the active workspace. Pass the key
 * without the prefix (`['tickets', id]`, or a getter for one); the query's
 * real key is always a getter, so it follows the slug as well as the parts.
 * `enabled` still applies, once the workspace is known.
 */
export function useWorkspaceQuery<
  TData,
  TError = Error,
  TDataInitial extends TData | undefined = undefined,
>(
  options: WorkspaceQueryOptions<TData, TError, TDataInitial>,
): UseQueryReturn<TData, TError, TDataInitial> {
  const parts = options.key
  return useQuery<TData, TError, TDataInitial>({
    ...options,
    key: () => workspaceKey(...toValue(parts)),
    enabled: whenWorkspaceReady(options.enabled),
  })
}

/** `useInfiniteQuery` options whose `key` is the part after the workspace prefix. */
export type WorkspaceInfiniteQueryOptions<
  TData,
  TError = Error,
  TPageParam = unknown,
  TDataInitial extends UseInfiniteQueryData<TData, TPageParam> | undefined = undefined,
> = Omit<UseInfiniteQueryOptions<TData, TError, TPageParam, TDataInitial>, 'key'> & {
  key: MaybeRefOrGetter<EntryKey>
}

/**
 * `useInfiniteQuery` with the key scoped to the active workspace. Takes the
 * options, or a getter returning them as `useInfiniteQuery` does.
 */
export function useWorkspaceInfiniteQuery<
  TData,
  TError = Error,
  TPageParam = unknown,
  TDataInitial extends UseInfiniteQueryData<TData, TPageParam> | undefined = undefined,
>(
  options:
    | WorkspaceInfiniteQueryOptions<TData, TError, TPageParam, TDataInitial>
    | (() => DefineInfiniteQueryOptions<TData, TError, TPageParam, TDataInitial>),
): UseInfiniteQueryReturn<TData, TError, TPageParam, TDataInitial> {
  if (typeof options === 'function') {
    return useInfiniteQuery<TData, TError, TPageParam, TDataInitial>(() => {
      const current = options()
      return {
        ...current,
        key: workspaceKey(...current.key),
        enabled: whenWorkspaceReady(current.enabled)(),
      }
    })
  }
  const parts = options.key
  return useInfiniteQuery<TData, TError, TPageParam, TDataInitial>({
    ...options,
    key: () => workspaceKey(...toValue(parts)),
    enabled: whenWorkspaceReady(options.enabled),
  })
}

/**
 * The query cache with every key scoped to the active workspace. Keys and
 * filter keys are written without the prefix; a filter with no key reaches
 * every entry of the active workspace and no other. The workspace is read at
 * each call, not when this is set up. Before the workspace is known, writes and
 * invalidations are dropped and reads find nothing.
 */
export function useWorkspaceQueryCache() {
  const cache = useQueryCache()

  function scoped(filters?: UseQueryEntryFilter): UseQueryEntryFilter {
    return { ...filters, key: workspaceKey(...(filters?.key ?? [])) } as UseQueryEntryFilter
  }

  return {
    async invalidateQueries(filters?: UseQueryEntryFilter, refetchActive?: boolean | 'all') {
      if (!workspaceReady()) return
      await cache.invalidateQueries(scoped(filters), refetchActive)
    },
    cancelQueries(filters?: UseQueryEntryFilter, reason?: unknown) {
      if (!workspaceReady()) return
      cache.cancelQueries(scoped(filters), reason)
    },
    setQueryData<TData = unknown>(
      key: EntryKey,
      data: TData | ((oldData: TData | undefined) => TData),
    ) {
      if (!workspaceReady()) return
      cache.setQueryData<TData>(workspaceKey(...key), data)
    },
    getQueryData<TData = unknown>(key: EntryKey): TData | undefined {
      if (!workspaceReady()) return undefined
      return cache.getQueryData<TData>(workspaceKey(...key))
    },
  }
}

export type WorkspaceQueryCache = ReturnType<typeof useWorkspaceQueryCache>
