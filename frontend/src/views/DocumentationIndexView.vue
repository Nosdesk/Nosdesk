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

const { createNewPage } = useDocumentation()
const { allTree: pages, drafts: uncollectedPages } = useDocPages()
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
    .slice(0, 12)
})

const visibleStarred = computed(() => starredPages.value.slice(0, 10))

const visibleGaps = computed(() => gaps.value.slice(0, 6))

const visibleUncollected = computed<Page[]>(() =>
  [...uncollectedPages.value]
    .sort((a, b) => (b.updated_at ?? '').localeCompare(a.updated_at ?? ''))
    .slice(0, 12),
)

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
  return rows.slice(0, 10).map((r) => toPage(r))
})

const verificationCount = computed(() => docs.allPages.filter(pageNeedsAttention).length)

const totalPages = computed(() => flattenTree(pages.value).length)

function onCollectionCreated() {
  showCreateCollectionModal.value = false
  void collectionBrowserRef.value?.reload()
}

function pageAuthor(page: Page) {
  return page.last_edited_by ?? page.created_by
}

function gapMeta(gap: KnowledgeGap): string {
  const parts: string[] = []
  if (gap.evidence_count > 0) {
    parts.push(t('docs-index-gap-evidence', { count: gap.evidence_count }))
  }
  if (gap.last_evidence_at) {
    parts.push(formatRelativeTime(gap.last_evidence_at))
  }
  return parts.join(' · ')
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
    return t('doc-detail-needs-verification')
  }
  return t('docs-index-verification-stale-meta', {
    time: formatRelativeTime(page.verified_at),
  })
}

onMounted(() => {
  titleManager.setCustomTitle(t('docs-index-title'))
})

usePageCreateAction(handleCreatePage)
</script>

<template>
  <div class="flex-1 flex flex-col min-h-0">
    <header class="shrink-0 border-b border-subtle bg-surface">
      <div class="px-4 sm:px-6 py-2.5 mx-auto w-full max-w-8xl">
        <DocumentationIndexToolbar @create-collection="showCreateCollectionModal = true" />
      </div>
    </header>

    <CollectionModal
      mode="create"
      :show="showCreateCollectionModal"
      @close="showCreateCollectionModal = false"
      @created="onCollectionCreated"
    />

    <div class="flex-1 min-h-0 overflow-auto">
      <div class="flex flex-col gap-4 sm:gap-5 px-4 sm:px-6 py-4 mx-auto w-full max-w-8xl">
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

        <template v-if="totalPages > 0">
          <div class="docs-index-pages-grid">
            <SectionCard content-padding="p-1">
              <template #title>{{ $t('docs-index-recently-updated') }}</template>
              <template #headerActions>
                <span
                  v-if="recentlyUpdated.length > 0"
                  class="text-[11px] text-tertiary tabular-nums font-normal"
                >
                  {{ $t('docs-index-recently-updated-count', { count: recentlyUpdated.length }) }}
                </span>
              </template>
              <ul v-if="recentlyUpdated.length > 0" class="flex flex-col">
                <li v-for="page in recentlyUpdated" :key="page.id">
                  <DocumentationHubRow
                    :page="page"
                    :author="pageAuthor(page)"
                    :updated-at="page.updated_at"
                  />
                </li>
              </ul>
              <p v-else class="px-2 py-2 text-[13px] text-tertiary">
                {{ $t('docs-index-no-recent-activity') }}
              </p>
            </SectionCard>

            <SectionCard content-padding="p-1">
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
                  />
                </li>
              </ul>
              <p v-else class="px-2 py-2 text-[13px] text-tertiary">
                {{ $t('docs-index-starred-hint') }}
              </p>
            </SectionCard>
          </div>

          <div class="docs-index-queue-grid">
            <SectionCard
              content-padding="p-1"
              action-to="/documentation/gaps"
              :action-label="$t('docs-index-gaps-view-all')"
            >
              <template #title>{{ $t('docs-index-gaps-heading') }}</template>
              <template #headerActions>
                <span
                  v-if="gapCount > 0"
                  class="text-[11px] text-tertiary tabular-nums font-normal"
                >
                  {{ gapCount }}
                </span>
              </template>
              <p v-if="gapsLoading" class="px-2 py-2 text-[13px] text-tertiary">
                {{ $t('docs-index-gaps-loading') }}
              </p>
              <ul v-else-if="visibleGaps.length > 0" class="flex flex-col">
                <li v-for="gap in visibleGaps" :key="gap.id">
                  <RouterLink
                    :to="`/documentation/gaps/${gap.id}`"
                    class="group flex items-center gap-2 py-1.5 min-h-7 px-2 -mx-2 rounded hover:bg-surface-hover transition-colors"
                  >
                    <Icon name="warning" size="xs" class="shrink-0 text-amber-500" aria-hidden="true" />
                    <div class="flex items-center gap-1.5 min-w-0 flex-1">
                      <span class="truncate text-[13px] leading-snug text-primary group-hover:text-accent transition-colors">
                        {{ gap.title }}
                      </span>
                      <template v-if="gapMeta(gap)">
                        <span class="shrink-0 text-[11px] text-tertiary/60" aria-hidden="true">·</span>
                        <span class="shrink-0 text-[11px] text-tertiary whitespace-nowrap">
                          {{ gapMeta(gap) }}
                        </span>
                      </template>
                    </div>
                    <span
                      class="shrink-0 text-[10px] px-1.5 py-0.5 rounded bg-surface-alt text-tertiary tabular-nums"
                    >
                      {{ gapImpactLabel(gap) }}
                    </span>
                  </RouterLink>
                </li>
              </ul>
              <p v-else class="px-2 py-2 text-[13px] text-tertiary">
                {{ $t('docs-index-gaps-empty') }}
              </p>
            </SectionCard>

            <SectionCard content-padding="p-1">
              <template #title>{{ $t('docs-index-verification-heading') }}</template>
              <template #headerActions>
                <span
                  v-if="verificationCount > 0"
                  class="text-[11px] text-tertiary tabular-nums font-normal"
                >
                  {{ verificationCount }}
                </span>
              </template>
              <ul v-if="verificationAttention.length > 0" class="flex flex-col">
                <li v-for="page in verificationAttention" :key="page.id">
                  <DocumentationHubRow
                    :page="page"
                    :meta="verificationMeta(page)"
                  />
                </li>
              </ul>
              <p v-else class="px-2 py-2 text-[13px] text-tertiary">
                {{ $t('docs-index-verification-empty') }}
              </p>
            </SectionCard>
          </div>

          <SectionCard
            content-padding="p-1"
            action-to="/documentation/drafts"
            :action-label="$t('docs-index-uncollected-view-all')"
          >
            <template #title>{{ $t('docs-index-uncollected-heading') }}</template>
            <template #headerActions>
              <span class="text-[11px] text-tertiary tabular-nums font-normal">
                {{ uncollectedCount }}
              </span>
            </template>
            <ul v-if="visibleUncollected.length > 0" class="docs-index-uncollected-grid">
              <li v-for="page in visibleUncollected" :key="page.id">
                <DocumentationHubRow
                  :page="page"
                  :meta="page.children?.length
                    ? $t('docs-index-page-children', { count: page.children.length })
                    : undefined"
                  :author="page.children?.length ? undefined : pageAuthor(page)"
                  :updated-at="page.children?.length ? undefined : page.updated_at"
                />
              </li>
            </ul>
            <p v-else class="px-2 py-2 text-[13px] text-tertiary">
              {{ $t('docs-index-uncollected-empty') }}
            </p>
          </SectionCard>
        </template>
      </div>
    </div>
  </div>
</template>

<style scoped>
.docs-index-queue-grid {
  display: grid;
  gap: 1rem;
  grid-template-columns: 1fr;
}

@media (min-width: 1024px) {
  .docs-index-queue-grid {
    gap: 1.25rem;
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
}

.docs-index-pages-grid {
  display: grid;
  gap: 1rem;
  grid-template-columns: 1fr;
}

@media (min-width: 1024px) {
  .docs-index-pages-grid {
    gap: 1.25rem;
    grid-template-columns: repeat(12, minmax(0, 1fr));
  }

  .docs-index-pages-grid > * {
    grid-column: span 6;
  }
}

@media (min-width: 1280px) {
  .docs-index-pages-grid > :first-child {
    grid-column: span 7;
  }

  .docs-index-pages-grid > :nth-child(2) {
    grid-column: span 5;
  }
}

.docs-index-uncollected-grid {
  display: grid;
  gap: 0;
  grid-template-columns: 1fr;
}

@media (min-width: 640px) {
  .docs-index-uncollected-grid {
    grid-template-columns: repeat(2, minmax(0, 1fr));
    column-gap: 0.5rem;
  }
}

@media (min-width: 1024px) {
  .docs-index-uncollected-grid {
    grid-template-columns: repeat(3, minmax(0, 1fr));
  }
}

@media (min-width: 1536px) {
  .docs-index-uncollected-grid {
    grid-template-columns: repeat(auto-fill, minmax(16rem, 1fr));
  }
}
</style>
