<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useFluent } from 'fluent-vue'
import { useTitleManager } from '@/composables/useTitleManager'
import { restorePage, permanentlyDeletePage } from '@nosdesk/core/services/documentationService'
import { useDocPages } from '@/composables/useDocPages'
import BackButton from '@/components/common/BackButton.vue'
import EmptyState from '@/components/common/EmptyState.vue'
import Icon from '@/components/common/Icon.vue'
import PullToRefresh from '@/components/common/PullToRefresh.vue'
import { formatDate } from '@nosdesk/core/utils/dateUtils'

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const titleManager = useTitleManager()

// Trashed pages (status = deleted) derive from the sync pool. Restore
// flips the status and permanent-delete removes the row, both of which
// flow in as sync events, so the list reconciles itself.
const { trashed: pages } = useDocPages()
const confirmingDeleteId = ref<string | number | null>(null)

// Pull-to-refresh (Tauri app) binds to the scroll container below the
// sticky header; defaults to the global re-sync.
const scrollEl = ref<HTMLElement | null>(null)

const handleRestore = async (pageId: string | number) => {
  await restorePage(pageId)
}

const handlePermanentDelete = async (pageId: string | number) => {
  if (String(confirmingDeleteId.value) !== String(pageId)) {
    confirmingDeleteId.value = pageId
    return
  }
  await permanentlyDeletePage(pageId)
  confirmingDeleteId.value = null
}

onMounted(() => {
  titleManager.setCustomTitle(t('docs-trash-title'))
})
</script>

<template>
  <div class="bg-app flex flex-col h-full">
    <PullToRefresh :target="scrollEl" />
    <!-- Header -->
    <div class="sticky top-0 z-20 bg-surface border-b border-default shadow-md">
      <div class="p-2 flex items-center gap-2">
        <BackButton fallbackRoute="/documentation" :label="$t('docs-trash-back')" />
        <div class="flex-1"></div>
      </div>
    </div>

    <!-- Main Content -->
    <div ref="scrollEl" class="flex flex-col flex-1 overflow-auto bg-gradient-to-b from-bg-app to-bg-surface items-center">
      <div class="flex flex-col max-w-7xl mx-auto w-full px-4 py-6 gap-6">
        <!-- Header -->
        <div class="flex items-center justify-between gap-4 pb-4 border-b border-default">
          <div class="flex items-center gap-3">
            <span class="text-status-error inline-flex">
              <Icon name="trash" size="lg" />
            </span>
            <div>
              <h2 class="text-xl font-semibold text-primary">{{ $t('docs-trash-heading') }}</h2>
              <p class="text-sm text-tertiary mt-0.5">{{ $t('docs-trash-description') }}</p>
            </div>
          </div>
          <span class="text-xs bg-surface-alt px-2 py-1 rounded-full text-tertiary">
            {{ $t('docs-trash-count', { count: pages.length }) }}
          </span>
        </div>

        <!-- Empty state -->
        <EmptyState
          v-if="pages.length === 0"
          icon="trash"
          :title="$t('empty-documentation-trash-title')"
          :description="$t('empty-documentation-trash-description')"
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
              <span class="text-sm font-medium text-primary truncate block">{{ page.title }}</span>
              <span v-if="page.deleted_at" class="text-xs text-tertiary">
                {{ $t('docs-trash-deleted-at', { date: formatDate(page.deleted_at) }) }}
              </span>
            </div>
            <div class="flex items-center gap-2 flex-shrink-0">
              <button
                @click="handleRestore(page.id)"
                class="px-3 py-1.5 text-xs rounded-md border border-default text-secondary hover:text-primary hover:bg-surface-hover transition-colors flex items-center gap-1.5"
              >
                <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M3 10h10a8 8 0 018 8v2M3 10l6 6m-6-6l6-6" />
                </svg>
                {{ $t('docs-trash-restore') }}
              </button>
              <button
                @click="handlePermanentDelete(page.id)"
                class="px-3 py-1.5 text-xs rounded-md transition-colors flex items-center gap-1.5"
                :class="String(confirmingDeleteId) === String(page.id)
                  ? 'bg-status-error text-white hover:bg-status-error/90'
                  : 'border border-default text-status-error hover:bg-status-error/10'"
              >
                <Icon name="trash" />
                {{ String(confirmingDeleteId) === String(page.id) ? $t('docs-trash-confirm-delete') : $t('docs-trash-delete-forever') }}
              </button>
            </div>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
