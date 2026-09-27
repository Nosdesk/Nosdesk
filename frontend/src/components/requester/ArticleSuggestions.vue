<script setup lang="ts">
/**
 * Help articles that match what a requester is typing as the subject of a new
 * request, so they can solve it without waiting. Up to three public articles,
 * opened in a new tab so the half-written request isn't lost. Renders nothing
 * when there are no matches or the knowledge base is off (the search rejects).
 */
import { onBeforeUnmount, ref, watch } from 'vue'
import { useFluent } from 'fluent-vue'

import Icon from '@/components/common/Icon.vue'

import type { ArticleHit } from './types'

const props = defineProps<{
  subject: string
  search: (q: string) => Promise<ArticleHit[]>
}>()
const { $t: t } = useFluent()

const MIN_CHARS = 4
const DEBOUNCE_MS = 350
const SHOWN = 3

const hits = ref<ArticleHit[]>([])
let timer: ReturnType<typeof setTimeout> | undefined
let latest = 0

watch(
  () => props.subject.trim(),
  (q) => {
    clearTimeout(timer)
    if (q.length < MIN_CHARS) {
      hits.value = []
      return
    }
    timer = setTimeout(async () => {
      const ticket = ++latest
      try {
        const found = await props.search(q)
        if (ticket === latest) hits.value = found.slice(0, SHOWN)
      } catch {
        if (ticket === latest) hits.value = []
      }
    }, DEBOUNCE_MS)
  },
)
onBeforeUnmount(() => clearTimeout(timer))
</script>

<template>
  <div class="contents" aria-live="polite">
    <section
      v-if="hits.length"
      class="flex flex-col gap-2 rounded-lg border border-default bg-surface-alt px-3 py-2.5"
    >
      <p class="text-xs font-medium text-secondary">{{ t('requester-articles-heading') }}</p>
      <ul class="flex flex-col gap-1">
        <li v-for="hit in hits" :key="hit.id">
          <a
            :href="`/docs/${encodeURIComponent(hit.slug)}`"
            target="_blank"
            rel="noopener"
            class="inline-flex items-center gap-1.5 text-sm text-accent hover:underline"
          >
            <Icon name="document" size="xs" />
            {{ hit.title }}
          </a>
        </li>
      </ul>
    </section>
  </div>
</template>
