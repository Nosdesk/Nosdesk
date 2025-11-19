import { defineStore } from 'pinia'
import { logger } from '@/utils/logger';
import { ref } from 'vue'

interface User {
  id: number;
  name: string;
  email: string;
}

export const useUserStore = defineStore('user', () => {
  const currentUser = ref<User | null>(null)
  const isAuthenticated = ref(false)

  function setUser(user: User) {
    currentUser.value = user
    isAuthenticated.value = true
  }

  function clearUser() {
    currentUser.value = null
    isAuthenticated.value = false
  }

  return {
    currentUser,
    isAuthenticated,
    setUser,
    clearUser,
    user: currentUser
  }
}) 