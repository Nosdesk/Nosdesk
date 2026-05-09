import { defineStore } from 'pinia'
import { logger } from '@/utils/logger';
import { ref, computed } from 'vue'
import userService from '@/services/userService'
import type { User, PaginatedResponse, UserPaginationParams } from '@/services/userService'

// Cache configuration
const CACHE_TTL = 5 * 60 * 1000 // 5 minutes
const BACKGROUND_REFRESH_THRESHOLD = 4 * 60 * 1000 // 4 minutes - increased to reduce unnecessary background refreshes

interface CacheEntry<T> {
  data: T
  timestamp: number
  loading: boolean
  error: string | null
}

interface PaginatedCacheEntry {
  data: User[]
  total: number
  totalPages: number
  timestamp: number
  loading: boolean
  error: string | null
  params: UserPaginationParams
}

// Active user requests to prevent duplicate network calls
const activePaginatedRequests = new Map<string, Promise<PaginatedResponse<User>>>()
const activeUserRequests = new Map<string, Promise<User | null>>()
const activeBackgroundRefreshes = new Map<string, Promise<void>>()

// Batch request management
const pendingBatchRequests = new Set<string>()
const batchTimeout = ref<number | null>(null)
const BATCH_DELAY_MS = 50 // Wait 50ms to collect multiple requests
const MAX_BATCH_SIZE = 20 // Maximum users per batch request

/**
 * Per-uuid waiter resolvers. `getUserByUuid` enqueues a uuid and
 * registers a resolver here; `processBatchRequests` calls each
 * resolver when it writes the corresponding cache entry. Replaces
 * the earlier polling pattern (setTimeout(checkCache, 10) loop)
 * which added 10-50ms latency on every individual user call and
 * burned CPU per uncached uuid in flight. Multiple waiters per
 * uuid (concurrent callers for the same UUID) get the same
 * resolved value via fan-out.
 */
type UserResolver = (user: User | null) => void
const pendingUserResolvers = new Map<string, UserResolver[]>()

function registerUserWaiter(uuid: string, resolve: UserResolver): void {
  const arr = pendingUserResolvers.get(uuid) ?? []
  arr.push(resolve)
  pendingUserResolvers.set(uuid, arr)
}

function settleUserWaiters(uuid: string, user: User | null): void {
  const arr = pendingUserResolvers.get(uuid)
  if (!arr || arr.length === 0) return
  pendingUserResolvers.delete(uuid)
  for (const resolve of arr) resolve(user)
}

export const useDataStore = defineStore('data', () => {
  // Users cache
  const usersCache = ref(new Map<string, PaginatedCacheEntry>())
  const individualUsersCache = ref(new Map<string, CacheEntry<User>>())

  // Global loading states
  const globalLoading = ref(false)
  
  // Helper function to create cache key
  const createCacheKey = (params: UserPaginationParams): string => {
    return JSON.stringify({
      page: params.page,
      pageSize: params.pageSize,
      sortField: params.sortField || 'id',
      sortDirection: params.sortDirection || 'asc',
      search: params.search || '',
      role: params.role || ''
    })
  }
  
  // Check if cache entry is valid
  const isCacheValid = (timestamp: number): boolean => {
    return Date.now() - timestamp < CACHE_TTL
  }
  
  // Check if data is stale
  const isDataStale = (timestamp: number): boolean => {
    return Date.now() - timestamp > CACHE_TTL
  }
  
  // Check if cache needs background refresh
  const needsBackgroundRefresh = (timestamp: number): boolean => {
    return Date.now() - timestamp > BACKGROUND_REFRESH_THRESHOLD
  }
  
  // Get paginated users with smart caching
  const getPaginatedUsers = async (params: UserPaginationParams, forceRefresh = false): Promise<PaginatedResponse<User>> => {
    const cacheKey = createCacheKey(params)
    const cached = usersCache.value.get(cacheKey)
    
    // Return cached data if valid and not forcing refresh
    if (!forceRefresh && cached && isCacheValid(cached.timestamp) && !cached.loading) {
      // Check if background refresh needed
      if (needsBackgroundRefresh(cached.timestamp)) {
        // Trigger background refresh without waiting
        refreshPaginatedUsersInBackground(params, cacheKey)
      }
      
      return {
        data: cached.data,
        total: cached.total,
        page: params.page,
        pageSize: params.pageSize,
        totalPages: cached.totalPages
      }
    }
    
    // Check for ongoing request
    const requestKey = `paginated-users-${cacheKey}`
    if (activePaginatedRequests.has(requestKey)) {
      return activePaginatedRequests.get(requestKey)!
    }
    
    // Create new request
    const requestPromise = fetchPaginatedUsersFromAPI(params, cacheKey)
    activePaginatedRequests.set(requestKey, requestPromise)
    
    try {
      const result = await requestPromise
      return result
    } finally {
      activePaginatedRequests.delete(requestKey)
    }
  }
  
  // Fetch from API and update cache
  const fetchPaginatedUsersFromAPI = async (params: UserPaginationParams, cacheKey: string): Promise<PaginatedResponse<User>> => {
    // Update loading state
    const existingCache = usersCache.value.get(cacheKey)
    usersCache.value.set(cacheKey, {
      data: existingCache?.data || [],
      total: existingCache?.total || 0,
      totalPages: existingCache?.totalPages || 0,
      timestamp: existingCache?.timestamp || 0,
      loading: true,
      error: null,
      params
    })
    
    try {
      const response = await userService.getPaginatedUsers(params, `cached-${cacheKey}`)
      
      // Update cache with new data
      usersCache.value.set(cacheKey, {
        data: response.data,
        total: response.total,
        totalPages: response.totalPages,
        timestamp: Date.now(),
        loading: false,
        error: null,
        params
      })
      
      // Also update individual user cache
      response.data.forEach(user => {
        individualUsersCache.value.set(user.uuid, {
          data: user,
          timestamp: Date.now(),
          loading: false,
          error: null
        })
      })
      
      return response
    } catch (error) {
      // Update cache with error
      usersCache.value.set(cacheKey, {
        data: existingCache?.data || [],
        total: existingCache?.total || 0,
        totalPages: existingCache?.totalPages || 0,
        timestamp: existingCache?.timestamp || 0,
        loading: false,
        error: error instanceof Error ? error.message : 'Unknown error',
        params
      })
      
      throw error
    }
  }
  
  // Background refresh without blocking UI
  const refreshPaginatedUsersInBackground = async (params: UserPaginationParams, cacheKey: string) => {
    // Check if background refresh is already in progress for this cache key
    const backgroundKey = `background-paginated-${cacheKey}`
    if (activeBackgroundRefreshes.has(backgroundKey)) {
      return activeBackgroundRefreshes.get(backgroundKey)
    }
    
    // Create new background refresh promise
    const refreshPromise = (async () => {
      try {
        logger.debug('Background refreshing users cache for:', cacheKey)
        await fetchPaginatedUsersFromAPI(params, cacheKey)
      } catch (error) {
        logger.warn('Background refresh failed:', error)
      }
    })()
    
    // Track the background refresh
    activeBackgroundRefreshes.set(backgroundKey, refreshPromise)
    
    try {
      await refreshPromise
    } finally {
      // Clean up the tracking
      activeBackgroundRefreshes.delete(backgroundKey)
    }
  }
  
  // Get individual user by UUID with smart caching and batching
  const getUserByUuid = async (uuid: string, forceRefresh = false): Promise<User | null> => {
    const cached = individualUsersCache.value.get(uuid)
    
    // Return cached data if valid
    if (!forceRefresh && cached && isCacheValid(cached.timestamp) && !cached.loading) {
      // Only trigger background refresh if user is NOT in pending batch queue
      // This prevents race conditions between batch requests and individual background refreshes
      if (needsBackgroundRefresh(cached.timestamp) && !pendingBatchRequests.has(uuid)) {
        // Trigger background refresh
        refreshUserInBackground(uuid)
      }
      return cached.data
    }
    
    // Check for ongoing request
    const requestKey = `user-${uuid}`
    if (activeUserRequests.has(requestKey)) {
      return activeUserRequests.get(requestKey)!
    }
    
    // Add to batch request queue
    pendingBatchRequests.add(uuid)
    
    // Set up debounced batch processing
    if (batchTimeout.value) {
      clearTimeout(batchTimeout.value)
    }
    
    batchTimeout.value = window.setTimeout(() => {
      processBatchRequests()
      batchTimeout.value = null
    }, BATCH_DELAY_MS)
    
    // Promise resolved by processBatchRequests when the cache
    // entry for this uuid lands. Replaces the earlier polling
    // pattern — see pendingUserResolvers comment above.
    const requestPromise = new Promise<User | null>((resolve) => {
      registerUserWaiter(uuid, resolve)
    })
    
    activeUserRequests.set(requestKey, requestPromise)
    
    try {
      const result = await requestPromise
      return result
    } finally {
      activeUserRequests.delete(requestKey)
    }
  }
  
  // Background refresh for individual user
  const refreshUserInBackground = async (uuid: string) => {
    // Check if background refresh is already in progress for this user
    const backgroundKey = `background-user-${uuid}`
    if (activeBackgroundRefreshes.has(backgroundKey)) {
      // Return the existing promise instead of starting a new request
      return activeBackgroundRefreshes.get(backgroundKey)
    }
    
    // Create new background refresh promise using the batching system
    const refreshPromise = (async () => {
      try {
        logger.debug('Background refreshing user cache for:', uuid)
        // Use the batching system instead of direct API call
        await getUserByUuid(uuid, true) // Force refresh through batching system
      } catch (error) {
        logger.warn('Background user refresh failed:', error)
      }
    })()
    
    // Track the background refresh
    activeBackgroundRefreshes.set(backgroundKey, refreshPromise)
    
    try {
      await refreshPromise
    } finally {
      // Clean up the tracking
      activeBackgroundRefreshes.delete(backgroundKey)
    }
  }
  
  // Get user name from cache (for quick lookups)
  const getUserName = (uuid: string): string | null => {
    const cached = individualUsersCache.value.get(uuid)
    if (!cached?.data) return null
    
    // Return cached data - background refresh is handled by getUserByUuid calls
    return cached.data.name || null
  }
  
  // Get user avatar from cache
  const getUserAvatar = (uuid: string, preferThumb = true): string | null => {
    const cached = individualUsersCache.value.get(uuid)
    if (!cached?.data) return null

    if (preferThumb && cached.data.avatar_thumb) {
      return cached.data.avatar_thumb
    }
    return cached.data.avatar_url || null
  }
  
  // Invalidate cache for specific user (old method - just clears cache)
  const invalidateUser = (uuid: string) => {
    individualUsersCache.value.delete(uuid)
    
    // Also invalidate paginated caches that might contain this user
    for (const [key, cache] of usersCache.value.entries()) {
      if (cache.data.some(user => user.uuid === uuid)) {
        usersCache.value.delete(key)
      }
    }
  }
  
  // Invalidate all user caches (old method - just clears cache)
  const invalidateAllUsers = () => {
    usersCache.value.clear()
    individualUsersCache.value.clear()
  }
  
  // IMPROVED: Invalidate and refresh user (clears cache AND fetches fresh data)
  const invalidateAndRefreshUser = async (uuid: string): Promise<User | null> => {
    logger.debug(`🔄 Invalidating and refreshing user cache for: ${uuid}`)
    
    // 1. Clear from cache
    invalidateUser(uuid)
    
    // 2. Force fetch fresh data (this will update cache and trigger reactivity)
    try {
      const freshUser = await getUserByUuid(uuid, true) // forceRefresh = true
      logger.debug(`✅ User ${uuid} refreshed successfully`)
      return freshUser
    } catch (error) {
      logger.error(`❌ Failed to refresh user ${uuid}:`, error)
      return null
    }
  }
  
  // IMPROVED: Invalidate and refresh multiple users
  const invalidateAndRefreshUsers = async (uuids: string[]): Promise<(User | null)[]> => {
    logger.debug(`🔄 Invalidating and refreshing ${uuids.length} users`)
    
    // 1. Clear all from cache
    uuids.forEach(uuid => invalidateUser(uuid))
    
    // 2. Force fetch fresh data for all users (using Promise.all with individual calls)
    try {
      const freshUsers = await Promise.all(uuids.map(uuid => getUserByUuid(uuid, true)))
      logger.debug(`✅ ${uuids.length} users refreshed successfully`)
      return freshUsers
    } catch (error) {
      logger.error(`❌ Failed to refresh users:`, error)
      return uuids.map(() => null)
    }
  }
  
  // IMPROVED: Smart cache refresh (keep showing stale data while fetching fresh)
  const refreshUserInPlace = async (uuid: string): Promise<User | null> => {
    const cached = individualUsersCache.value.get(uuid)
    
    if (cached) {
      // Mark as refreshing but keep existing data visible
      cached.loading = true
    }
    
    try {
      // Fetch fresh data
      const freshUser = await userService.getUserByUuid(uuid)
      
      if (freshUser) {
        // Update cache with fresh data
        individualUsersCache.value.set(uuid, {
          data: freshUser,
          timestamp: Date.now(),
          loading: false,
          error: null
        })
        
        // Also update in paginated caches
        for (const cache of usersCache.value.values()) {
          const userIndex = cache.data.findIndex(user => user.uuid === uuid)
          if (userIndex !== -1) {
            cache.data[userIndex] = freshUser
          }
        }
        
        return freshUser
      }
    } catch (error) {
      // Keep existing data on error, just mark as not loading
      if (cached) {
        cached.loading = false
        cached.error = error instanceof Error ? error.message : 'Refresh failed'
      }
      logger.error(`Failed to refresh user ${uuid}:`, error)
    }
    
    return cached?.data || null
  }
  
  // Update user in cache (for optimistic updates)
  const updateUserInCache = (uuid: string, updates: Partial<User>) => {
    const cached = individualUsersCache.value.get(uuid)
    if (cached) {
      cached.data = { ...cached.data, ...updates }
      cached.timestamp = Date.now()
    }
    
    // Update in paginated caches too
    for (const cache of usersCache.value.values()) {
      const userIndex = cache.data.findIndex(user => user.uuid === uuid)
      if (userIndex !== -1) {
        cache.data[userIndex] = { ...cache.data[userIndex], ...updates }
      }
    }
  }
  
  // Add new user to cache
  const addUserToCache = (user: User) => {
    individualUsersCache.value.set(user.uuid, {
      data: user,
      timestamp: Date.now(),
      loading: false,
      error: null
    })
    
    // Invalidate paginated caches since they need to be refreshed
    usersCache.value.clear()
  }
  
  // Remove user from cache
  const removeUserFromCache = (uuid: string) => {
    individualUsersCache.value.delete(uuid)
    
    // Remove from paginated caches
    for (const cache of usersCache.value.values()) {
      cache.data = cache.data.filter(user => user.uuid !== uuid)
    }
  }
  
  // Get cache statistics
  const getCacheStats = computed(() => ({
    paginatedCaches: usersCache.value.size,
    individualUsers: individualUsersCache.value.size,
    activeRequests: activePaginatedRequests.size + activeUserRequests.size
  }))
  
  // Cleanup expired cache entries
  const cleanupExpiredCache = () => {
    const now = Date.now()
    
    // Clean individual users cache
    for (const [uuid, cache] of individualUsersCache.value.entries()) {
      if (now - cache.timestamp > CACHE_TTL * 2) { // Keep cache longer than TTL for background refresh
        individualUsersCache.value.delete(uuid)
      }
    }
    
    // Clean paginated cache
    for (const [key, cache] of usersCache.value.entries()) {
      if (now - cache.timestamp > CACHE_TTL * 2) {
        usersCache.value.delete(key)
      }
    }
  }
  
  // Auto cleanup every 10 minutes (capture ID for HMR cleanup)
  const cleanupIntervalId = setInterval(cleanupExpiredCache, 10 * 60 * 1000)
  if (import.meta.hot) {
    import.meta.hot.dispose(() => clearInterval(cleanupIntervalId))
  }
  
  // Process batched user requests
  const processBatchRequests = async () => {
    if (pendingBatchRequests.size === 0) return
    
    // Get all pending UUIDs and clear the set
    const uuidsToFetch = Array.from(pendingBatchRequests)
    pendingBatchRequests.clear()
    
    // Filter to only UUIDs that aren't already cached or loading.
    // Two skip paths need to settle their waiters here so the
    // Promises don't hang:
    //   - "fresh" cache: another path (addUserToCache, optimistic
    //     update) populated the row between enqueue and now —
    //     settle with the cached value, skip the network.
    //   - "loading" cache: another in-flight batch is fetching
    //     this uuid — its processBatchRequests will settle our
    //     waiter when it lands. Leave alone.
    const uncachedUuids = uuidsToFetch.filter(uuid => {
      const cached = individualUsersCache.value.get(uuid)
      if (!cached) return true
      if (cached.loading) return false  // other batch handles it
      if (isDataStale(cached.timestamp)) return true  // refresh
      // Fresh cache: settle waiters with the cached value.
      const real = cached.data?.uuid ? cached.data : null
      settleUserWaiters(uuid, real)
      return false
    })

    if (uncachedUuids.length === 0) return
    
    // Only log batching in development mode
    if (import.meta.env.DEV) {
      logger.debug(`🚀 Batching ${uncachedUuids.length} user requests:`, uncachedUuids)
    }
    
    // Mark all as loading
    uncachedUuids.forEach(uuid => {
      individualUsersCache.value.set(uuid, {
        data: individualUsersCache.value.get(uuid)?.data || {} as User,
        timestamp: individualUsersCache.value.get(uuid)?.timestamp || 0,
        loading: true,
        error: null
      })
    })
    
    try {
      // Process in batches if needed
      const batches = []
      for (let i = 0; i < uncachedUuids.length; i += MAX_BATCH_SIZE) {
        batches.push(uncachedUuids.slice(i, i + MAX_BATCH_SIZE))
      }
      
      const allUsers = []
      for (const batch of batches) {
        const users = await userService.getUsersBatch(batch)
        allUsers.push(...users)
      }
      
      // Update cache with results
      const userMap = new Map(allUsers.map(user => [user.uuid, user]))
      
      uncachedUuids.forEach(uuid => {
        const user = userMap.get(uuid)
        if (user) {
          individualUsersCache.value.set(uuid, {
            data: user,
            timestamp: Date.now(),
            loading: false,
            error: null
          })
          settleUserWaiters(uuid, user)
        } else {
          // User not found
          individualUsersCache.value.set(uuid, {
            data: individualUsersCache.value.get(uuid)?.data || {} as User,
            timestamp: Date.now(),
            loading: false,
            error: 'User not found'
          })
          settleUserWaiters(uuid, null)
        }
      })

    } catch (error) {
      logger.error('Batch user request failed:', error)

      // Mark all as error AND settle their waiters with null so the
      // Promises returned by getUserByUuid don't hang indefinitely.
      uncachedUuids.forEach(uuid => {
        individualUsersCache.value.set(uuid, {
          data: individualUsersCache.value.get(uuid)?.data || {} as User,
          timestamp: individualUsersCache.value.get(uuid)?.timestamp || 0,
          loading: false,
          error: error instanceof Error ? error.message : 'Unknown error'
        })
        settleUserWaiters(uuid, null)
      })
    }
  }
  
  return {
    // Paginated users
    getPaginatedUsers,
    invalidateAllUsers,
    
    // Individual users
    getUserByUuid,
    refreshUserInBackground,
    addUserToCache,
    updateUserInCache,
    removeUserFromCache,
    getUserName,
    getUserAvatar,
    
    // Cache management
    invalidateUser,
    invalidateAndRefreshUser,
    invalidateAndRefreshUsers,
    refreshUserInPlace,
    
    // Utilities
    cleanupExpiredCache,
    
    // State
    globalLoading,
    
    // Batch request management
    processBatchRequests,
    
    // Multi-user convenience functions
    getUsersByUuids: async (uuids: string[]): Promise<(User | null)[]> => {
      // Use the batching system to efficiently fetch multiple users
      const userPromises = uuids.map(uuid => getUserByUuid(uuid))
      return await Promise.all(userPromises)
    },
    
    // Direct batch API call (bypasses cache)
    getUsersBatchDirect: userService.getUsersBatch,

    // Synchronous getter for cached user. Returns the user only
    // when a real row was loaded — entries that completed with
    // an error (most commonly "User not found", which fires when
    // the assignee_uuid references a deleted or never-existed
    // user) get a placeholder `{} as User` written into `data` by
    // processBatchRequests; returning that to consumers used to
    // leave the UserCell skeleton stuck forever because `name`
    // resolved to undefined and the cell couldn't tell loading
    // from missing apart. The `data.uuid` check narrows to "this
    // is a real user row," not a TypeScript-cast empty placeholder.
    getCachedUserByUuid: (uuid: string): User | undefined => {
      const cached = individualUsersCache.value.get(uuid)
      if (!cached || cached.error || !cached.data?.uuid) return undefined
      return cached.data
    },

    // Loading state for a user. Lets consumers distinguish:
    //   'loading'  — request in flight, show a skeleton
    //   'resolved' — user is in the cache and renderable
    //   'missing'  — request completed but no user (deleted /
    //                permission denied / orphan reference); show
    //                a fallback rather than an indefinite skeleton
    getUserStatus: (uuid: string): 'loading' | 'resolved' | 'missing' => {
      const cached = individualUsersCache.value.get(uuid)
      if (!cached) return 'loading'
      if (cached.loading) return 'loading'
      if (cached.error || !cached.data?.uuid) return 'missing'
      return 'resolved'
    },

    // Stats
    getCacheStats
  }
}) 