<script setup lang="ts">
/**
 * Publish all, in Done's and the import page's actions: "Publish all 36
 * pages" with a confirmation that says who will see them, then the result
 * in its place ("34 pages published · 2 left as they were"). The count is
 * the server's, read when the import is shown, so a page published or
 * changed since isn't counted; with nothing left to publish there is no
 * button.
 */
import { computed, ref } from 'vue'
import { useFluent } from 'fluent-vue'
import { useQuery, useQueryCache } from '@pinia/colada'

import Button from '@/components/common/Button.vue'
import ConfirmModal from '@/components/common/ConfirmModal.vue'
import Icon from '@/components/common/Icon.vue'
import { useToastStore } from '@nosdesk/core/stores/toast'
import {
  docsImportService,
  docsPublishQueryKey,
  type DocsImportJob,
  type DocsPublished,
  type DocsSummary,
} from '@nosdesk/core/services/docsImportService'

import { docsActionRefusalText, errorBody } from '../refusals'
import { publishOffered, publishedLine } from './docsRemoveModel'

const props = defineProps<{
  job: DocsImportJob
  summary: DocsSummary
  /** "IT Handbook", or null when the pages went into several collections. */
  collection: string | null
}>()

const emit = defineEmits<{
  published: [result: DocsPublished]
}>()

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)
const toast = useToastStore()
const queryCache = useQueryCache()

const offered = computed(() => publishOffered(props.job, props.summary))
const planQuery = useQuery({
  key: () => docsPublishQueryKey(props.job.id),
  query: () => docsImportService.publishPlan(props.job.id),
  enabled: () => offered.value,
})
const publishable = computed(() => planQuery.data.value?.publishable ?? 0)

const result = ref<DocsPublished | null>(null)
const showDialog = ref(false)
const publishing = ref(false)
const errorMessage = ref('')

async function confirm() {
  if (publishing.value) return
  publishing.value = true
  errorMessage.value = ''
  try {
    const done = await docsImportService.publishAll(props.job.id)
    result.value = done
    showDialog.value = false
    emit('published', done)
    toast.success(publishedLine(t, done.published, done.left))
    void queryCache.invalidateQueries({ key: docsPublishQueryKey(props.job.id) })
  } catch (e) {
    showDialog.value = false
    errorMessage.value = docsActionRefusalText(t, errorBody(e)) ?? t('docs-import-publish-failed')
  } finally {
    publishing.value = false
  }
}

const who = computed(() => props.collection ?? t('docs-import-publish-these-collections'))
</script>

<template>
  <span
    v-if="result"
    class="inline-flex items-center gap-2 text-sm text-secondary"
    role="status"
    data-test="docs-published"
  >
    <Icon name="checkCircle" size="sm" class="text-status-success shrink-0" />
    <span>{{ publishedLine(t, result.published, result.left) }}</span>
  </span>
  <Button
    v-else-if="offered && publishable > 0"
    variant="secondary"
    icon="eye"
    :loading="publishing"
    data-test="docs-publish-button"
    @click="showDialog = true"
  >
    {{ $t('docs-import-publish-button', { count: publishable }) }}
  </Button>
  <span v-if="errorMessage" role="alert" class="text-sm text-status-error" data-test="docs-publish-error">
    {{ errorMessage }}
  </span>
  <ConfirmModal
    :show="showDialog"
    :title="$t('docs-import-publish-title', { count: publishable })"
    :message="$t('docs-import-publish-message', { collection: who })"
    :confirm-label="$t('docs-import-publish-confirm', { count: publishable })"
    :loading="publishing"
    @close="showDialog = false"
    @confirm="confirm"
  />
</template>
