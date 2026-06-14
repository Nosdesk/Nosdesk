/**
 * User profile bundle query.
 *
 * Wraps `/api/users/{uuid}/profile` (sparse fieldsets) in a Pinia
 * Colada query so the user profile page renders user, devices,
 * groups, emails, and ticket badge counts from one cache entry
 * instead of fanning out to four request handlers.
 *
 * The cache key includes the requested groups so two callers that
 * ask for different slices share nothing, the canonical caller
 * is `UserProfileView.vue` which always asks for the full set.
 */
import { computed, type MaybeRefOrGetter, toValue } from 'vue'
import { useQuery } from '@pinia/colada'
import userService, {
  type ProfileBundleGroup,
  type UserProfileBundle,
} from '@/services/userService'

export interface UseUserProfileBundleOptions {
  /** User UUID to fetch. Reactive, query refires on change. */
  uuid: MaybeRefOrGetter<string | null | undefined>
  /** Sub-resources to include. Defaults to every group. */
  include?: readonly ProfileBundleGroup[]
  /** Gate the fetch. Defaults to "uuid is present". Callers that have a
   *  non-uuid placeholder (e.g. the creation form's `new`) pass their own
   *  condition so the query stays idle until a real user is in view. */
  enabled?: MaybeRefOrGetter<boolean>
}

const ALL_GROUPS: readonly ProfileBundleGroup[] = [
  'devices',
  'groups',
  'emails',
  'counts',
]

export function useUserProfileBundle(options: UseUserProfileBundleOptions) {
  const include = (options.include ?? ALL_GROUPS).slice().sort()

  const uuid = computed(() => toValue(options.uuid) ?? '')

  const query = useQuery({
    key: () => ['user', uuid.value, 'profile', include.join(',')],
    query: () => userService.getUserProfileBundle(uuid.value, include as ProfileBundleGroup[]),
    enabled: () =>
      options.enabled !== undefined ? !!toValue(options.enabled) : !!uuid.value,
  })

  // `isLoading` reflects the *initial* fetch (no cached bundle yet).
  // `isRefreshing` covers background refetches that fire on remount
  // while cached data is already being served, splitting the two so
  // callers can keep content visible during the refresh instead of
  // blanking out to a skeleton.
  return {
    bundle: computed<UserProfileBundle | undefined>(() => query.data.value),
    isLoading: computed(
      () => query.status.value === 'pending' && query.data.value === undefined,
    ),
    isRefreshing: computed(
      () => query.asyncStatus.value === 'loading' && query.data.value !== undefined,
    ),
    isError: computed(() => query.status.value === 'error'),
    error: computed(() => query.error.value),
    refetch: () => query.refetch(),
  }
}
