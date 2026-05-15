<script setup lang="ts">
import { useFluent } from 'fluent-vue'
import Icon from '@/components/common/Icon.vue'

const fluent = useFluent()
const t = (k: string, args?: Record<string, string | number>) => fluent.$t(k, args)

defineProps<{
  message: string
  dismissible?: boolean
  showRetry?: boolean
}>()

const emit = defineEmits<{
  dismiss: []
  retry: []
}>()
</script>

<template>
  <div class="bg-status-error-muted border border-status-error/30 text-status-error px-4 py-3 rounded-lg flex items-start gap-3">
    <svg class="w-5 h-5 flex-shrink-0 mt-0.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
      <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 8v4m0 4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
    </svg>
    <div class="flex-1 min-w-0">
      <p class="text-sm">{{ message }}</p>
      <button
        v-if="showRetry"
        @click="emit('retry')"
        class="mt-2 text-xs font-medium text-status-error hover:underline focus:outline-none"
      >
        Try again
      </button>
    </div>
    <button
      v-if="dismissible"
      @click="emit('dismiss')"
      class="flex-shrink-0 text-status-error hover:opacity-80 focus:outline-none"
      :aria-label="t('common-error-banner-dismiss')"
    >
      <Icon name="close" />
    </button>
  </div>
</template>
