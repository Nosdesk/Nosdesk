/**
 * Reactive UUID → user resolver.
 *
 * Backed by the sync engine's object pool. The `user` aggregate
 * (`backend/sync-models/user.json`) is bootstrapped at workspace
 * load and kept current via `sync_actions` SSE frames, so the
 * directory's job collapses to a thin wrapper: take a uuid, ask
 * the pool, return a reactive computed.
 *
 * Three reasons to stay a wrapper rather than have callers call
 * `useReference('user', uuid)` directly:
 *
 *  1. The `getUserHandle(uuid)` API predates the sync engine and is
 *     used by ~10 surfaces (UserCell, RevisionList, QuickTooltip,
 *     filterFacets, etc.). Keeping the API stable made the
 *     dataStore → sync migration a one-file rewrite.
 *
 *  2. The `status` computed (loading / resolved / missing) is a
 *     directory concern that needs to combine pool membership with
 *     bootstrap-completed signal — easier to do here than at every
 *     call site.
 *
 *  3. Future "additional fetch on miss" logic (e.g. retry policy
 *     for the lazy fetcher) lives here, not in every consumer.
 *
 * Pool membership is the source of truth: bootstrap loads every
 * workspace user, and SSE delivers user.created / .updated /
 * .deleted as `sync_actions` frames the lifecycle layer pipes into
 * `pool.upsert` / `pool.remove`. Avatar / name changes propagate
 * within a single SSE round-trip, no manual cache coordination.
 */
import { computed, type ComputedRef } from 'vue'
import * as pool from '@/sync/pool'
import { useReference } from '@/sync/composables'
import type { User } from '@/types/user'

export type UserStatus = 'loading' | 'resolved' | 'missing'

export interface UserHandle {
  user: ComputedRef<User | null>
  status: ComputedRef<UserStatus>
}

/**
 * Pool projection of the User row (the subset bootstrap streams).
 * Frontend `User` type carries fields the projection deliberately
 * omits (mfa, signature, dashboard_layout, etc.); narrow here so
 * the directory only exposes what's actually in the pool.
 */
type PoolUser = Pick<
  User,
  'uuid' | 'name' | 'email' | 'role' | 'pronouns' | 'avatar_url' | 'avatar_thumb'
>

/**
 * Coerce the pool projection to the broader frontend `User` shape.
 * Defaults the omitted fields to nullish/empty so consumers reading
 * `.signature` etc. against a directory user don't blow up on
 * undefined property access. This is purely a type-shape adapter;
 * any consumer that genuinely needs MFA / signature / dashboard
 * data should call userService directly, those fields aren't part
 * of the sync projection by design.
 */
function asUser(u: PoolUser): User {
  return {
    uuid: u.uuid,
    name: u.name,
    email: u.email,
    role: u.role,
    pronouns: u.pronouns ?? null,
    avatar_url: u.avatar_url ?? null,
    avatar_thumb: u.avatar_thumb ?? null,
    banner_url: null,
    theme: null,
    signature: null,
    dashboard_layout: null,
    created_at: '',
    updated_at: '',
  }
}

const handleCache = new Map<string, UserHandle>()

if (import.meta.hot) {
  import.meta.hot.dispose(() => {
    handleCache.clear()
  })
}

function makeHandle(uuid: string): UserHandle {
  // useReference: returns a reactive computed over `pool.get` AND
  // schedules a lazy fetch through the per-aggregate referenceFetcher
  // wired in `sync/lifecycle.ts`. Bootstrap covers the common case;
  // the fetcher only fires for uuids created mid-session before
  // their `user.created` SSE arrived.
  const ref = useReference<PoolUser>('user', uuid)
  return {
    user: computed<User | null>(() => {
      const u = ref.value
      return u ? asUser(u) : null
    }),
    status: computed<UserStatus>(() => {
      if (ref.value) return 'resolved'
      // Use the sync engine's last-known cursor as a "bootstrap
      // happened" signal. Before any data has been pulled the
      // cursor is 0; after bootstrap (or warm-rehydrate) it's > 0.
      // A miss before bootstrap means "still loading"; a miss after
      // means "this uuid isn't in the workspace" (deleted, orphan
      // FK, or pending the lazy fetcher's round-trip — close enough
      // to 'missing' to render the fallback).
      return pool.getLastSyncId() === 0 ? 'loading' : 'missing'
    }),
  }
}

export function useUsersDirectory() {
  function getUserHandle(uuid: string): UserHandle {
    let handle = handleCache.get(uuid)
    if (!handle) {
      handle = makeHandle(uuid)
      handleCache.set(uuid, handle)
    }
    return handle
  }

  /** Back-compat: original API returned just the user computed.
   * New call sites should prefer `getUserHandle` so they can
   * distinguish loading from missing. */
  function getUser(uuid: string): ComputedRef<User | null> {
    return getUserHandle(uuid).user
  }

  return { getUser, getUserHandle }
}
