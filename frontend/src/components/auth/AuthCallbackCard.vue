<script setup lang="ts">
import { computed } from 'vue'
import { useFluent } from 'fluent-vue'
import LogoIcon from '@/components/icons/LogoIcon.vue'
import Icon from '@/components/common/Icon.vue'
import { useBrandingStore } from '@/stores/branding'
import { useThemeStore } from '@/stores/theme'

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

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
        <h2 class="text-lg font-medium text-primary">{{ loadingMessage || t('auth-callback-loading-default') }}</h2>
        <p class="text-sm text-tertiary text-center">{{ t('auth-callback-loading-subtitle') }}</p>
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
          <Icon v-if="errorInfo.icon === 'link'" name="link" size="lg" />
          <Icon v-else-if="errorInfo.icon === 'warning'" name="warning" size="lg" />
          <Icon v-else name="info" size="lg" />
        </div>

        <!-- Error Content -->
        <div class="flex flex-col gap-2 text-center">
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
              ? 'bg-accent text-on-accent hover:bg-accent/90'
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
            <span
              class="transition-transform duration-200 inline-flex"
              :class="{ 'rotate-90': showTechnicalDetails }"
            >
              <Icon name="chevronRight" />
            </span>
            {{ t('auth-callback-technical-details') }}
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
          <Icon name="checkCircle" size="lg" />
        </div>
        <h2 class="text-lg font-medium text-primary">{{ t('auth-callback-success-title') }}</h2>
        <p class="text-sm text-tertiary">{{ t('auth-callback-success-subtitle') }}</p>
      </div>
    </div>
  </div>
</template>
