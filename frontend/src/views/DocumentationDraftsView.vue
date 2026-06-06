<script setup lang="ts">
import { onMounted } from 'vue'
import { useFluent } from 'fluent-vue'
import { useTitleManager } from '@/composables/useTitleManager'
import { useDocPages } from '@/composables/useDocPages'
import BackButton from '@/components/common/BackButton.vue'
import DocumentationCardGrid from '@/components/documentationComponents/DocumentationCardGrid.vue'

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const titleManager = useTitleManager()

// Drafts are the workspace's uncollected pages. Derived from the sync
// pool, so bootstrap and live SSE / delta updates flow through without
// a fetch or a discrete-event listener.
const { drafts } = useDocPages()

onMounted(() => {
  titleManager.setCustomTitle(t('docs-drafts-title'))
})
</script>

<template>
  <div class="bg-app flex flex-col h-full">
    <!-- Header -->
    <div class="sticky top-0 z-20 bg-surface border-b border-default shadow-md">
      <div class="p-2 flex items-center gap-2">
        <BackButton fallbackRoute="/documentation" :label="$t('docs-drafts-back')" />
        <div class="flex-1"></div>
      </div>
    </div>

    <!-- Main Content -->
    <div class="flex flex-col flex-1 overflow-auto bg-gradient-to-b from-bg-app to-bg-surface items-center">
      <div class="flex flex-col max-w-7xl mx-auto w-full px-4 py-6 gap-6">
        <!-- Drafts Header -->
        <div class="flex items-center justify-between gap-4 pb-4 border-b border-default">
          <div class="flex items-center gap-3">
            <span class="text-3xl">✏️</span>
            <div>
              <h2 class="text-xl font-semibold text-primary">{{ $t('docs-drafts-heading') }}</h2>
              <p class="text-sm text-tertiary mt-0.5">{{ $t('docs-drafts-description') }}</p>
            </div>
          </div>
          <span class="text-xs bg-surface-alt px-2 py-1 rounded-full text-tertiary">
            {{ $t('docs-drafts-count', { count: drafts.length }) }}
          </span>
        </div>

        <!-- Pages -->
        <DocumentationCardGrid :pages="drafts" />
      </div>
    </div>
  </div>
</template>
