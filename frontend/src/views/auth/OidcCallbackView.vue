<script setup lang="ts">
import { ref, onMounted, computed } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useAuthStore } from '@/stores/auth'
import apiClient from '@/services/apiConfig'
import AuthCallbackCard, { type ErrorInfo } from '@/components/auth/AuthCallbackCard.vue'

const route = useRoute()
const router = useRouter()
const authStore = useAuthStore()

const error = ref<string | null>(null)
const detailedError = ref<string | null>(null)
const loading = ref(true)
const message = ref('Completing sign-in...')
const showTechnicalDetails = ref(false)

// Computed property to determine error type and appropriate messaging
const errorInfo = computed<ErrorInfo | null>(() => {
  if (!error.value) return null

  const errorMsg = error.value.toLowerCase()

  if (errorMsg.includes('already connected') || errorMsg.includes('already linked')) {
    return {
      type: 'already_connected',
      title: 'Account Already Connected',
      message: 'This account is already linked to another user in the system.',
      suggestion: 'Try signing in with a different account, or contact your administrator.',
      icon: 'link',
      actions: [
        { label: 'Return to Login', action: 'login', primary: true }
      ]
    }
  }

  if (errorMsg.includes('not found') || errorMsg.includes('invalid')) {
    return {
      type: 'invalid_request',
      title: 'Authentication Failed',
      message: 'The authentication request was invalid or has expired.',
      suggestion: 'Please try signing in again.',
      icon: 'warning',
      actions: [
        { label: 'Try Again', action: 'login', primary: true }
      ]
    }
  }

  // Generic error
  return {
    type: 'generic',
    title: 'Authentication Failed',
    message: error.value,
    suggestion: 'Please try again or contact support if the problem persists.',
    icon: 'error',
    actions: [
      { label: 'Return to Login', action: 'login', primary: true }
    ]
  }
})

const handleAction = (action: string) => {
  if (action === 'login') {
    router.push('/login')
  }
}

onMounted(async () => {
  const code = route.query.code as string | undefined
  const state = route.query.state as string | undefined
  const errorParam = route.query.error as string | undefined
  const errorDescription = route.query.error_description as string | undefined

  // Handle errors returned by the OIDC provider
  if (errorParam) {
    error.value = errorDescription || errorParam
    loading.value = false
    return
  }

  // Validate required parameters
  if (!code || !state) {
    error.value = 'Missing required authentication parameters'
    detailedError.value = `Missing: ${!code ? 'code' : ''} ${!state ? 'state' : ''}`
    loading.value = false
    return
  }

  try {
    message.value = 'Processing authentication...'

    const response = await apiClient.get('/auth/oauth/callback', {
      params: { code, state }
    })

    const data = response.data

    if (data?.success && data.csrf_token) {
      message.value = 'Success! Redirecting...'
      loading.value = false

      authStore.setAuthProvider('oidc')

      if (data.user) {
        authStore.user = data.user
      }

      // Redirect to original destination or home
      let redirectPath = sessionStorage.getItem('authRedirect') || '/'
      if (redirectPath.includes('/auth/oidc/callback')) {
        redirectPath = '/'
      }
      sessionStorage.removeItem('authRedirect')

      // Brief delay to show success state
      setTimeout(() => router.push(redirectPath), 500)
    } else {
      error.value = 'Invalid response from server'
      detailedError.value = JSON.stringify(data, null, 2)
      loading.value = false
    }
  } catch (err) {
    interface AxiosError {
      response?: { status?: number; data?: { message?: string; error?: string } }
      request?: unknown
      message?: string
    }
    const axiosError = err as AxiosError

    error.value = axiosError.response?.data?.message ||
                  axiosError.response?.data?.error ||
                  'An unexpected error occurred during authentication'

    if (axiosError.response) {
      detailedError.value = `Status: ${axiosError.response.status}\n${JSON.stringify(axiosError.response.data, null, 2)}`
    } else if (axiosError.request) {
      detailedError.value = 'No response received from server'
    } else {
      detailedError.value = axiosError.message || 'Unknown error'
    }

    loading.value = false
  }
})
</script>

<template>
  <AuthCallbackCard
    :loading="loading"
    :loading-message="message"
    :error="error"
    :error-info="errorInfo"
    :detailed-error="detailedError"
    v-model:show-technical-details="showTechnicalDetails"
    @action="handleAction"
  />
</template>
