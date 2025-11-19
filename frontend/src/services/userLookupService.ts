// Efficient user lookup service for UserAvatar components
import { logger } from '@/utils/logger';
// Now uses the global data store for better caching and deduplication
import { ref } from 'vue'
import { useDataStore } from '@/stores/dataStore'

// Vue composable for user lookup that delegates to the global data store
export const useUserLookup = () => {
  const loading = ref(false)
  const error = ref<string | null>(null)
  const dataStore = useDataStore()

  const lookupUser = async (uuid: string) => {
    loading.value = true
    error.value = null
    
    try {
      const user = await dataStore.getUserByUuid(uuid)
      return user
    } catch (err) {
      error.value = 'Failed to lookup user'
      logger.error('User lookup error:', err)
      return null
    } finally {
      loading.value = false
    }
  }

  return {
    lookupUser,
    loading,
    error,
    getUserName: dataStore.getUserName,
    getUserAvatar: dataStore.getUserAvatar,
    addUsersToCache: (users: any[]) => {
      // Add users to the global data store
      users.forEach(user => {
        if (user.uuid) {
          dataStore.addUserToCache(user)
        }
      })
    }
  }
} 