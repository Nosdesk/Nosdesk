<script setup lang="ts">
import { computed } from 'vue'
import LogoIcon from '@/components/icons/LogoIcon.vue'
import { useBrandingStore } from '@/stores/branding'
import { useThemeStore } from '@/stores/theme'

export interface ErrorAction {
  label: string
  action: string
  primary?: boolean
}

export interface ErrorInfo {
  type: string
  title: string
  message: string
  suggestion: string
  icon: 'error' | 'warning' | 'link'
  actions: ErrorAction[]
}

const props = defineProps<{
  loading: boolean
  loadingMessage?: string
  error?: string | null
  errorInfo?: ErrorInfo | null
  detailedError?: string | null
  showTechnicalDetails?: boolean
}>()

const emit = defineEmits<{
  action: [action: string]
  'update:showTechnicalDetails': [value: boolean]
}>()

const brandingStore = useBrandingStore()
const themeStore = useThemeStore()

const logoUrl = computed(() => brandingStore.getLogoUrl(themeStore.isDarkMode))

const toggleTechnicalDetails = () => {
  emit('update:showTechnicalDetails', !props.showTechnicalDetails)
}
</script>

<template>
  <div class="min-h-screen flex items-center justify-center bg-app p-4">
    <div class="bg-surface p-8 rounded-xl shadow-lg max-w-md w-full border border-default flex flex-col gap-6">
      <!-- Logo -->
      <div class="flex justify-center">
        <img
          v-if="logoUrl"
          :src="logoUrl"
          :alt="brandingStore.appName"
          class="h-10 max-w-[200px] object-contain"
        />
        <LogoIcon v-else class="h-10 text-accent" />
      </div>

      <!-- Loading State -->
      <div v-if="loading" class="flex flex-col items-center justify-center gap-4">
        <div class="relative">
          <div class="w-12 h-12 rounded-full border-2 border-surface-hover"></div>
          <div class="absolute inset-0 w-12 h-12 rounded-full border-2 border-accent border-t-transparent animate-spin"></div>
        </div>
        <h2 class="text-lg font-medium text-primary">{{ loadingMessage || 'Completing sign-in...' }}</h2>
        <p class="text-sm text-tertiary text-center">Please wait while we complete authentication</p>
      </div>

      <!-- Error State -->
      <div v-else-if="error && errorInfo" class="flex flex-col items-center justify-center gap-5">
        <!-- Error Icon -->
        <div
          class="rounded-full p-3"
          :class="{
            'bg-red-500/10 text-red-500': errorInfo.icon === 'error',
            'bg-amber-500/10 text-amber-500': errorInfo.icon === 'warning',
            'bg-accent/10 text-accent': errorInfo.icon === 'link'
          }"
        >
          <!-- Link Icon -->
          <svg v-if="errorInfo.icon === 'link'" class="w-7 h-7" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
            <path stroke-linecap="round" stroke-linejoin="round" d="M13.828 10.172a4 4 0 00-5.656 0l-4 4a4 4 0 105.656 5.656l1.102-1.101m-.758-4.899a4 4 0 005.656 0l4-4a4 4 0 00-5.656-5.656l-1.1 1.1" />
          </svg>
          <!-- Warning Icon -->
          <svg v-else-if="errorInfo.icon === 'warning'" class="w-7 h-7" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
            <path stroke-linecap="round" stroke-linejoin="round" d="M12 9v3.75m-9.303 3.376c-.866 1.5.217 3.374 1.948 3.374h14.71c1.73 0 2.813-1.874 1.948-3.374L13.949 3.378c-.866-1.5-3.032-1.5-3.898 0L2.697 16.126zM12 15.75h.007v.008H12v-.008z" />
          </svg>
          <!-- Error Icon -->
          <svg v-else class="w-7 h-7" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
            <path stroke-linecap="round" stroke-linejoin="round" d="M12 9v3.75m9-.75a9 9 0 11-18 0 9 9 0 0118 0zm-9 3.75h.008v.008H12v-.008z" />
          </svg>
        </div>

        <!-- Error Content -->
        <div class="text-center space-y-2">
          <h2 class="text-lg font-medium text-primary">{{ errorInfo.title }}</h2>
          <p class="text-sm text-secondary">{{ errorInfo.message }}</p>
          <p class="text-xs text-tertiary">{{ errorInfo.suggestion }}</p>
        </div>

        <!-- Action Buttons -->
        <div class="flex flex-col gap-2 w-full mt-2">
          <button
            v-for="(action, index) in errorInfo.actions"
            :key="action.action"
            @click="emit('action', action.action)"
            class="w-full px-4 py-2.5 rounded-lg text-sm font-medium transition-colors"
            :class="index === 0
              ? 'bg-accent text-white hover:bg-accent/90'
              : 'bg-surface-alt text-secondary hover:bg-surface-hover border border-default'"
          >
            {{ action.label }}
          </button>
        </div>

        <!-- Technical Details -->
        <div v-if="detailedError" class="w-full pt-2 border-t border-default">
          <button
            @click="toggleTechnicalDetails"
            class="flex items-center gap-2 text-xs text-tertiary hover:text-secondary transition-colors"
          >
            <svg
              class="w-3.5 h-3.5 transition-transform duration-200"
              :class="{ 'rotate-90': showTechnicalDetails }"
              fill="none"
              viewBox="0 0 24 24"
              stroke="currentColor"
              stroke-width="2"
            >
              <path stroke-linecap="round" stroke-linejoin="round" d="M9 5l7 7-7 7" />
            </svg>
            Technical Details
          </button>

          <Transition
            enter-active-class="transition-all duration-200 ease-out"
            enter-from-class="opacity-0 max-h-0"
            enter-to-class="opacity-100 max-h-40"
            leave-active-class="transition-all duration-150 ease-in"
            leave-from-class="opacity-100 max-h-40"
            leave-to-class="opacity-0 max-h-0"
          >
            <div v-if="showTechnicalDetails" class="mt-2 overflow-hidden">
              <div class="overflow-auto max-h-32 bg-surface-alt p-3 rounded-lg border border-default">
                <pre class="text-xs text-tertiary font-mono whitespace-pre-wrap">{{ detailedError }}</pre>
              </div>
            </div>
          </Transition>
        </div>
      </div>

      <!-- Success State (brief flash before redirect) -->
      <div v-else class="flex flex-col items-center justify-center gap-4">
        <div class="rounded-full p-3 bg-green-500/10 text-green-500">
          <svg class="w-7 h-7" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
            <path stroke-linecap="round" stroke-linejoin="round" d="M9 12.75L11.25 15 15 9.75M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
          </svg>
        </div>
        <h2 class="text-lg font-medium text-primary">Authentication successful</h2>
        <p class="text-sm text-tertiary">Redirecting...</p>
      </div>
    </div>
  </div>
</template>
