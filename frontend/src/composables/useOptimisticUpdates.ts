import { useDataStore } from '@/stores/dataStore'
import userService from '@/services/userService'
import type { User } from '@/services/userService'

export const useOptimisticUpdates = () => {
  const dataStore = useDataStore()

  // Optimistically update user data
  const updateUser = async (uuid: string, updates: Partial<User>) => {
    // 1. Immediately update the cache (optimistic update)
    dataStore.updateUserInCache(uuid, updates)
    
    try {
      // 2. Send update to backend
      const updatedUser = await userService.updateUser(uuid, updates)
      
      if (updatedUser) {
        // 3. Update cache with server response
        dataStore.updateUserInCache(uuid, updatedUser)
      }
      
      return updatedUser
    } catch (error) {
      // 4. Revert optimistic update on error
      console.error('Failed to update user, reverting optimistic update:', error)
      
      // Fetch fresh data from server to revert
      try {
        const freshUser = await dataStore.getUserByUuid(uuid, true) // Force refresh
        if (freshUser) {
          dataStore.updateUserInCache(uuid, freshUser)
        }
      } catch (revertError) {
        console.error('Failed to revert optimistic update:', revertError)
      }
      
      throw error
    }
  }

  // Optimistically create user
  const createUser = async (userData: { name: string; email: string; role: string }) => {
    try {
      const newUser = await userService.createUser(userData)

      // Add to cache
      dataStore.addUserToCache(newUser)

      // Invalidate paginated caches to trigger refresh
      dataStore.invalidateAllUsers()

      return newUser
    } catch (error) {
      console.error('Failed to create user:', error)
      throw error
    }
  }

  // Optimistically delete user
  const deleteUser = async (uuid: string) => {
    // Store original data for potential revert
    const originalUser = await dataStore.getUserByUuid(uuid)
    
    // 1. Immediately remove from cache (optimistic update)
    dataStore.removeUserFromCache(uuid)
    
    try {
      // 2. Send delete to backend
      const success = await userService.deleteUser(uuid)
      
      if (!success) {
        throw new Error('Delete operation failed')
      }
      
      return true
    } catch (error) {
      // 3. Revert optimistic update on error
      console.error('Failed to delete user, reverting optimistic update:', error)
      
      if (originalUser) {
        dataStore.addUserToCache(originalUser)
      }
      
      throw error
    }
  }

  return {
    updateUser,
    createUser,
    deleteUser
  }
} 