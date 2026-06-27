<script setup lang="ts">
import { onMounted } from 'vue'
import { useFluent } from 'fluent-vue'
import { useTitleManager } from '@/composables/useTitleManager'
import { restorePage } from '@nosdesk/core/services/documentationService'
import { useDocPages } from '@/composables/useDocPages'
import BackButton from '@/components/common/BackButton.vue'
import EmptyState from '@/components/common/EmptyState.vue'
import Icon from '@/components/common/Icon.vue'
import { formatDate } from '@nosdesk/core/utils/dateUtils'
import { docUrl } from '@nosdesk/core/utils/docUrl'

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const titleManager = useTitleManager()

// Archived pages derive from the sync pool, so the list updates itself
// when a page is archived or restored (its status change flows in as a
// metadata_changed sync event) — no fetch, no discrete listener.
const { archived: pages } = useDocPages()

const handleRestore = async (pageId: string | number) => {
  // Restore flips the page's status; the pool reflects the change and
  // this list drops the row on its own.
  await restorePage(pageId)
}

onMounted(() => {
  titleManager.setCustomTitle(t('docs-archived-title'))
})
</script>

<template>
  <div class="bg-app flex flex-col h-full">
    <!-- Header -->
    <div class="sticky top-0 z-20 bg-surface border-b border-default shadow-md">
      <div class="p-2 flex items-center gap-2">
        <BackButton fallbackRoute="/documentation" :label="$t('docs-archived-back')" />
        <div class="flex-1"></div>
      </div>
    </div>

    <!-- Main Content -->
    <div class="flex flex-col flex-1 overflow-auto bg-gradient-to-b from-bg-app to-bg-surface items-center">
      <div class="flex flex-col max-w-7xl mx-auto w-full px-4 py-6 gap-6">
        <!-- Header -->
        <div class="flex items-center justify-between gap-4 pb-4 border-b border-default">
          <div class="flex items-center gap-3">
            <span class="text-tertiary inline-flex">
              <Icon name="archive" size="lg" />
            </span>
            <div>
              <h2 class="text-xl font-semibold text-primary">{{ $t('docs-archived-heading') }}</h2>
              <p class="text-sm text-tertiary mt-0.5">{{ $t('docs-archived-description') }}</p>
            </div>
          </div>
          <span class="text-xs bg-surface-alt px-2 py-1 rounded-full text-tertiary">
            {{ $t('docs-archived-count', { count: pages.length }) }}
          </span>
        </div>

        <!-- Empty state -->
        <EmptyState
          v-if="pages.length === 0"
          icon="inbox"
          :title="$t('empty-documentation-archived-title')"
          :description="$t('empty-documentation-archived-description')"
          variant="card"
        />

        <!-- Page list -->
        <div v-else class="flex flex-col gap-2">
          <div
            v-for="page in pages"
            :key="page.id"
            class="flex items-center gap-3 px-4 py-3 bg-surface border border-default rounded-lg hover:bg-surface-hover transition-colors"
          >
            <span class="text-xl flex-shrink-0">{{ page.icon || '📄' }}</span>
            <div class="flex-1 min-w-0">
              <RouterLink
                :to="docUrl(page)"
                class="text-sm font-medium text-primary hover:text-accent truncate block"
              >
                {{ page.title }}
              </RouterLink>
              <span v-if="page.archived_at" class="text-xs text-tertiary">
                {{ $t('docs-archived-archived-at', { date: formatDate(page.archived_at) }) }}
              </span>
            </div>
            <button
              @click="handleRestore(page.id)"
              class="px-3 py-1.5 text-xs rounded-md border border-default text-secondary hover:text-primary hover:bg-surface-hover transition-colors flex items-center gap-1.5 flex-shrink-0"
            >
              <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M3 10h10a8 8 0 018 8v2M3 10l6 6m-6-6l6-6" />
              </svg>
              {{ $t('docs-archived-restore') }}
            </button>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
