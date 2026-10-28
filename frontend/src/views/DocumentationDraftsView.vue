<script setup lang="ts">
import { ref } from 'vue'
import { useFluent } from 'fluent-vue'
import { usePageTitle } from '@/composables/usePageTitle'
import { useDocPages } from '@/composables/useDocPages'
import DocumentationIndexToolbar from '@/components/documentationComponents/DocumentationIndexToolbar.vue'
import DocumentationCardGrid from '@/components/documentationComponents/DocumentationCardGrid.vue'
import PullToRefresh from '@/components/common/PullToRefresh.vue'

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)


// Drafts are the workspace's uncollected pages. Derived from the sync
// pool, so bootstrap and live SSE / delta updates flow through without
// a fetch or a discrete-event listener.
const { drafts } = useDocPages()

// Pull-to-refresh (Tauri app) binds to the scroll container below the
// band; defaults to the global re-sync.
const scrollEl = ref<HTMLElement | null>(null)

usePageTitle({ title: () => t('docs-drafts-title') })
</script>

<template>
  <div class="bg-app flex flex-col h-full">
    <PullToRefresh :target="scrollEl" />
    <!-- The index band: these views are its Drafts, Archived and Trash tabs -->
    <DocumentationIndexToolbar />

    <!-- Main Content -->
    <div ref="scrollEl" class="flex flex-col flex-1 overflow-auto bg-gradient-to-b from-bg-app to-bg-surface items-center">
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
