<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { storeToRefs } from 'pinia'
import { useFluent } from 'fluent-vue'
import { useTitleManager } from '@/composables/useTitleManager'
import { useDocumentation } from '@/composables/useDocumentation'
import { useDocPages, toPage } from '@/composables/useDocPages'
import { useSyncDocsStore, isActivePage } from '@nosdesk/core/sync/stores/documentation'
import { useDocumentationNavStore } from '@/stores/documentationNav'
import { useKnowledgeGaps } from '@/composables/useKnowledgeGaps'
import CollectionBrowser from '@/components/documentationComponents/CollectionBrowser.vue'
import CollectionModal from '@/components/documentationComponents/CollectionModal.vue'
import DocumentationIndexToolbar from '@/components/documentationComponents/DocumentationIndexToolbar.vue'
import DocumentationHubRow from '@/components/documentationComponents/DocumentationHubRow.vue'
import EmptyState from '@/components/common/EmptyState.vue'
import SectionCard from '@/components/common/SectionCard.vue'
import PullToRefresh from '@/components/common/PullToRefresh.vue'
import Icon from '@/components/common/Icon.vue'
import { usePageCreateAction } from '@/composables/usePageCreateAction'
import { docUrl } from '@nosdesk/core/utils/docUrl'
import { formatRelativeTime } from '@nosdesk/core/utils/dateUtils'
import {
  pageNeedsVerificationAttention,
  pageVerificationState,
} from '@/utils/pageVerification'
import type { Page } from '@nosdesk/core/services/documentationService'
import type { KnowledgeGap } from '@nosdesk/core/services/knowledgeGapsService'

defineOptions({ name: 'DocumentationIndexView' })

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const titleManager = useTitleManager()

// Pull-to-refresh (Tauri app) binds to the scroll container below the
// toolbar; defaults to the global re-sync.
const scrollEl = ref<HTMLElement | null>(null)

const { createNewPage } = useDocumentation()
const { allTree: pages } = useDocPages()
const docs = useSyncDocsStore()
const { gaps, isLoading: gapsLoading } = useKnowledgeGaps()

const docNavStore = useDocumentationNavStore()
const { starredPages } = storeToRefs(docNavStore)

const showCreateCollectionModal = ref(false)
const collectionBrowserRef = ref<InstanceType<typeof CollectionBrowser> | null>(null)

const uncollectedCount = computed(() => docs.uncollectedPages.length)
const gapCount = computed(() => gaps.value.length)

const handleCreatePage = async () => {
  try {
    await createNewPage()
  } catch (error) {
    console.error('Failed to create page:', error)
  }
}

function flattenTree(nodes: Page[]): Page[] {
  const out: Page[] = []
  for (const node of nodes) {
    out.push(node)
    if (node.children?.length) out.push(...flattenTree(node.children))
  }
  return out
}

const recentlyUpdated = computed<Page[]>(() => {
  const flat = flattenTree(pages.value)
  return flat
    .filter((p) => p.updated_at && p.status !== 'archived')
    .sort((a, b) => (b.updated_at ?? '').localeCompare(a.updated_at ?? ''))
    .slice(0, 6)
})

const visibleStarred = computed(() => starredPages.value.slice(0, 8))

/** The attention rail keeps each queue short; the full lists live behind
 *  the footer links. */
const visibleGaps = computed(() => gaps.value.slice(0, 3))

/** Collection ids whose pages require verification. Used to gate the
 *  never-verified case so unverified pages in neutral collections
 *  don't show up as needing attention. */
const requireVerificationCollectionIds = computed(
  () => new Set(docs.allCollections.filter((c) => c.require_verification).map((c) => c.id)),
)

function pageNeedsAttention(p: (typeof docs.allPages)[number]): boolean {
  const requires =
    p.collection_id != null && requireVerificationCollectionIds.value.has(p.collection_id)
  return isActivePage(p) && pageNeedsVerificationAttention(p, requires)
}

const verificationAttention = computed<Page[]>(() => {
  const rows = docs.allPages.filter(pageNeedsAttention)
  rows.sort((a, b) => {
    const sa = pageVerificationState(a)
    const sb = pageVerificationState(b)
    if (sa !== sb) return sa === 'never' ? -1 : sb === 'never' ? 1 : 0
    if (sa === 'stale' && a.verified_at && b.verified_at) {
      return a.verified_at.localeCompare(b.verified_at)
    }
    return (b.updated_at ?? '').localeCompare(a.updated_at ?? '')
  })
  return rows.slice(0, 3).map((r) => toPage(r))
})

const verificationCount = computed(() => docs.allPages.filter(pageNeedsAttention).length)

const attentionCount = computed(() => gapCount.value + verificationCount.value)

const totalPages = computed(() => flattenTree(pages.value).length)

function onCollectionCreated() {
  showCreateCollectionModal.value = false
  void collectionBrowserRef.value?.reload()
}

function pageAuthor(page: Page) {
  return page.last_edited_by ?? page.created_by
}

function isSearchGap(title: string): boolean {
  return title.startsWith('Customers searched:')
}

function gapImpactLabel(gap: KnowledgeGap): string {
  return isSearchGap(gap.title)
    ? t('docs-index-gap-impact-searches', { count: gap.impact_score })
    : t('docs-index-gap-impact-tickets', { count: gap.impact_score })
}

function verificationMeta(page: Page): string {
  if (!page.verified_at) {
    return t('docs-index-verification-never')
  }
  return formatRelativeTime(page.verified_at, { addSuffix: true })
}

onMounted(() => {
  titleManager.setCustomTitle(t('docs-index-title'))
})

usePageCreateAction(handleCreatePage)
</script>

<template>
  <div class="flex-1 flex flex-col min-h-0">
    <PullToRefresh :target="scrollEl" />
    <header class="shrink-0 border-b border-subtle bg-surface">
      <div class="@container px-4 sm:px-6 py-2.5 mx-auto w-full max-w-8xl">
        <DocumentationIndexToolbar @create-collection="showCreateCollectionModal = true" />
      </div>
    </header>

    <CollectionModal
      mode="create"
      :show="showCreateCollectionModal"
      @close="showCreateCollectionModal = false"
      @created="onCollectionCreated"
    />

    <!-- @container: the two-column split keys off THIS panel's width, not the
         viewport, so the layout stays correct whatever the nav sidebar does
         (same convention as the gantt). -->
    <div ref="scrollEl" class="@container flex-1 min-h-0 overflow-auto">
      <div class="docs-index-layout px-4 sm:px-6 py-4 mx-auto w-full max-w-8xl">
        <!-- Main column: the library leads; drafts collapse to one strip. -->
        <div class="flex flex-col gap-4 sm:gap-5 min-w-0">
          <CollectionBrowser
            ref="collectionBrowserRef"
            @create="showCreateCollectionModal = true"
          />

          <EmptyState
            v-if="totalPages === 0"
            variant="compact"
            icon="document"
            :title="$t('empty-documentation-index-title')"
            :description="$t('empty-documentation-index-description')"
            :action-label="$t('docs-index-new-page')"
            @action="handleCreatePage"
          />

          <RouterLink
            v-if="uncollectedCount > 0"
            to="/documentation/drafts"
            class="group flex items-center gap-2.5 px-4 py-2.5 bg-surface rounded-xl border border-default hover:border-strong transition-colors text-xs text-secondary"
          >
            <Icon name="documentEdit" size="sm" class="text-tertiary shrink-0" aria-hidden="true" />
            <span>
              <span class="font-medium text-primary">{{ $t('docs-index-drafts-strip-count', { count: uncollectedCount }) }}</span>
              {{ $t('docs-index-drafts-strip-suffix') }}
            </span>
            <span class="ml-auto text-[11px] font-medium text-accent group-hover:underline whitespace-nowrap">
              {{ $t('docs-index-drafts-strip-review') }}
            </span>
          </RouterLink>
        </div>

        <!-- Rail: attention first, then activity, then personal. -->
        <div v-if="totalPages > 0" class="flex flex-col gap-4 sm:gap-5 min-w-0">
          <SectionCard content-padding="p-0" :clip-content="true">
            <template #title>{{ $t('docs-index-attention-heading') }}</template>
            <template #headerActions>
              <span
                v-if="attentionCount > 0"
                class="inline-flex items-center h-[18px] px-1.5 rounded-full bg-status-warning-muted text-amber-700 text-[11px] font-semibold tabular-nums"
              >
                {{ attentionCount }}
              </span>
            </template>

            <p v-if="gapsLoading && attentionCount === 0" class="px-3 py-2.5 text-[13px] text-tertiary">
              {{ $t('docs-index-gaps-loading') }}
            </p>
            <p v-else-if="attentionCount === 0" class="px-3 py-2.5 text-[13px] text-tertiary">
              {{ $t('docs-index-attention-empty') }}
            </p>

            <template v-else>
              <template v-if="visibleGaps.length > 0">
                <p class="px-3 pt-2 pb-1 text-[11px] font-semibold text-tertiary uppercase tracking-wide">
                  {{ $t('docs-index-gaps-heading') }}
                </p>
                <ul class="flex flex-col px-1.5">
                  <li v-for="gap in visibleGaps" :key="gap.id">
                    <RouterLink
                      :to="`/documentation/gaps/${gap.id}`"
                      class="group flex items-center gap-2 py-1.5 px-1.5 rounded hover:bg-surface-hover transition-colors"
                    >
                      <Icon name="warning" size="xs" class="shrink-0 text-status-warning" aria-hidden="true" />
                      <span class="truncate min-w-0 flex-1 text-[13px] leading-snug text-primary group-hover:text-accent transition-colors">
                        {{ gap.title }}
                      </span>
                      <span class="shrink-0 text-[10px] px-1.5 py-0.5 rounded bg-surface-alt text-tertiary tabular-nums whitespace-nowrap">
                        {{ gapImpactLabel(gap) }}
                      </span>
                    </RouterLink>
                  </li>
                </ul>
              </template>

              <template v-if="verificationAttention.length > 0">
                <p
                  class="px-3 pt-2 pb-1 text-[11px] font-semibold text-tertiary uppercase tracking-wide"
                  :class="{ 'border-t border-subtle mt-1': visibleGaps.length > 0 }"
                >
                  {{ $t('docs-index-attention-verification') }}
                </p>
                <ul class="flex flex-col px-1.5 pb-1.5">
                  <li v-for="page in verificationAttention" :key="page.id">
                    <RouterLink
                      :to="docUrl(page)"
                      class="group flex items-center gap-2 py-1.5 px-1.5 rounded hover:bg-surface-hover transition-colors"
                    >
                      <span class="shrink-0 w-4 text-center text-sm leading-none opacity-80" aria-hidden="true">
                        {{ page.icon || '📄' }}
                      </span>
                      <span class="truncate min-w-0 flex-1 text-[13px] leading-snug text-primary group-hover:text-accent transition-colors">
                        {{ page.title }}
                      </span>
                      <span class="shrink-0 text-[11px] text-amber-700 whitespace-nowrap">
                        {{ verificationMeta(page) }}
                      </span>
                    </RouterLink>
                  </li>
                </ul>
              </template>

              <div class="flex items-center justify-between px-3 py-2 border-t border-default bg-surface-alt/50">
                <RouterLink to="/documentation/gaps" class="text-[11px] font-medium text-accent hover:underline">
                  {{ $t('docs-index-attention-gaps-link') }}
                </RouterLink>
                <span class="text-[11px] text-tertiary tabular-nums">
                  {{ $t('docs-index-attention-totals', { gaps: gapCount, stale: verificationCount }) }}
                </span>
              </div>
            </template>
          </SectionCard>

          <SectionCard content-padding="p-1.5">
            <template #title>{{ $t('docs-index-recently-updated') }}</template>
            <ul v-if="recentlyUpdated.length > 0" class="flex flex-col">
              <li v-for="page in recentlyUpdated" :key="page.id">
                <DocumentationHubRow
                  :page="page"
                  :author="pageAuthor(page)"
                  :updated-at="page.updated_at"
                  compact
                />
              </li>
            </ul>
            <p v-else class="px-2 py-1.5 text-[13px] text-tertiary">
              {{ $t('docs-index-no-recent-activity') }}
            </p>
          </SectionCard>

          <SectionCard content-padding="p-1.5">
            <template #title>{{ $t('docs-index-starred') }}</template>
            <template #headerActions>
              <span
                v-if="visibleStarred.length > 0"
                class="text-[11px] text-tertiary tabular-nums font-normal"
              >
                {{ starredPages.length }}
              </span>
            </template>
            <ul v-if="visibleStarred.length > 0" class="flex flex-col">
              <li v-for="sp in visibleStarred" :key="sp.page_id">
                <DocumentationHubRow
                  :href="docUrl({ slug: sp.slug, id: sp.page_id })"
                  :title="sp.title"
                  :icon="sp.icon"
                  compact
                />
              </li>
            </ul>
            <p v-else class="px-2 py-1.5 text-[13px] text-tertiary">
              {{ $t('docs-index-starred-hint') }}
            </p>
          </SectionCard>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.docs-index-layout {
  display: grid;
  gap: 1rem;
  grid-template-columns: 1fr;
  align-items: start;
}

/* Rail splits off only when the content panel itself is wide enough for a
   useful main column beside the fixed 21.75rem rail. */
@container (min-width: 60rem) {
  .docs-index-layout {
    gap: 1.25rem;
    grid-template-columns: minmax(0, 1fr) 21.75rem;
  }
}
</style>
