<script setup lang="ts">
import { RouterLink } from 'vue-router'
import { useFluent } from 'fluent-vue'
import StatusBadge from '@/components/StatusBadge.vue'
import { formatDate } from '@nosdesk/core/utils/dateUtils'
import { docUrl } from '@nosdesk/core/utils/docUrl'

// fluent-vue registers $t on the template render context; the
// composable call ensures the plugin is initialised before render.
useFluent()

interface Props {
  id: string;
  slug?: string;
  title: string;
  description: string;
  author: string;
  lastUpdated: string;
  status: 'published' | 'draft';
  showFullTitle?: boolean;
  showEditButton?: boolean;
  showStatus?: boolean;
}

withDefaults(defineProps<Props>(), {
  showFullTitle: false,
  showEditButton: true,
  showStatus: true
})
</script>

<template>
  <div class="bg-surface-alt rounded-lg hover:bg-surface-hover transition-colors">
    <div class="flex items-start justify-between">
      <RouterLink
        :to="docUrl({ slug, id })"
        class="block flex-1 p-4"
      >
        <div>
          <h3 :class="[
            'font-medium text-primary',
            showFullTitle ? 'text-lg' : ''
          ]">{{ title }}</h3>
          <p class="text-sm text-tertiary mt-1">{{ description }}</p>
          <div class="flex items-center gap-4 mt-2 text-xs text-tertiary">
            <span>{{ author }}</span>
            <span>{{ $t('docs-article-card-updated', { date: formatDate(lastUpdated, 'MMMM d, yyyy') }) }}</span>
          </div>
        </div>
      </RouterLink>

      <div v-if="showStatus || showEditButton" class="flex items-center gap-2">
        <StatusBadge
          v-if="showStatus"
          type="status"
          :value="status === 'published' ? 'open' : 'in-progress'"
        />

        <RouterLink
          v-if="showEditButton"
          :to="docUrl({ slug, id })"
          class="ml-2 p-1.5 text-tertiary hover:text-primary hover:bg-surface-hover rounded transition-colors"
          :title="$t('docs-article-card-edit')"
        >
          <svg xmlns="http://www.w3.org/2000/svg" class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M11 5H6a2 2 0 00-2 2v11a2 2 0 002 2h11a2 2 0 002-2v-5m-1.414-9.414a2 2 0 112.828 2.828L11.828 15H9v-2.828l8.586-8.586z" />
          </svg>
        </RouterLink>
      </div>
    </div>
  </div>
</template> 