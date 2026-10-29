<!-- Find in a PDF: the field, the match count, next/previous and match case.
The searching itself is pdf.js's PDFFindController; this asks it and shows
what it reports. -->
<script setup lang="ts">
import { computed, onMounted, ref, useId, watch } from 'vue'
import { useFluent } from 'fluent-vue'
import IconButton from '@/components/common/IconButton.vue'

export interface PdfFindRequest {
  query: string
  caseSensitive: boolean
  /** Step to the next or previous match of the same search. */
  again: boolean
  previous: boolean
}

const props = defineProps<{
  /** The current match and how many there are; total 0 when none. */
  matches: { current: number; total: number }
  /** pdf.js is still looking. */
  pending: boolean
}>()

const emit = defineEmits<{
  find: [request: PdfFindRequest]
  close: []
}>()

const { $t } = useFluent()
const inputId = useId()
const query = ref('')
const caseSensitive = ref(false)

const send = (again: boolean, previous = false) => {
  emit('find', { query: query.value, caseSensitive: caseSensitive.value, again, previous })
}

watch([query, caseSensitive], () => send(false))

const onKeydown = (event: KeyboardEvent) => {
  if (event.key === 'Escape') {
    // Closes the find bar only, not the viewer around it.
    event.preventDefault()
    event.stopPropagation()
    emit('close')
  } else if (event.key === 'Enter') {
    event.preventDefault()
    if (query.value) send(true, event.shiftKey)
  }
}

const status = computed(() => {
  if (!query.value || props.pending) return ''
  return props.matches.total
    ? $t('ticket-media-pdf-find-count', { current: props.matches.current, total: props.matches.total })
    : $t('ticket-media-pdf-find-none')
})

onMounted(() => document.getElementById(inputId)?.focus())

/** Focus the field again, its text selected (Cmd/Ctrl+F with the bar open). */
const focus = () => {
  const input = document.getElementById(inputId) as HTMLInputElement | null
  input?.focus()
  input?.select()
}
defineExpose({ focus })
</script>

<template>
  <div
    class="flex items-center gap-1 p-1.5 rounded-lg border border-default bg-surface shadow-lg"
    role="search"
    data-pdf-find-bar
    @keydown="onKeydown"
  >
    <input
      :id="inputId"
      v-model="query"
      type="search"
      class="w-40 sm:w-52 px-2 py-1 text-sm rounded-md border border-default bg-surface-alt text-primary placeholder-tertiary focus:outline-none focus:border-accent focus:ring-1 focus:ring-accent"
      :placeholder="$t('ticket-media-pdf-find-placeholder')"
      :aria-label="$t('ticket-media-pdf-find')"
      data-testid="pdf-find-input"
    />
    <span class="min-w-16 px-1 text-xs text-tertiary whitespace-nowrap" aria-live="polite" data-testid="pdf-find-status">
      {{ status }}
    </span>
    <IconButton
      icon="chevronUp"
      size="sm"
      :label="$t('ticket-media-pdf-find-previous')"
      :disabled="!matches.total"
      @click="send(true, true)"
    />
    <IconButton
      icon="chevronDown"
      size="sm"
      :label="$t('ticket-media-pdf-find-next')"
      :disabled="!matches.total"
      @click="send(true)"
    />
    <button
      type="button"
      class="px-2 py-1 text-xs rounded-md text-secondary hover:bg-surface-hover aria-pressed:bg-accent/15 aria-pressed:text-accent focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
      :aria-pressed="caseSensitive"
      @click="caseSensitive = !caseSensitive"
    >
      {{ $t('ticket-media-pdf-find-match-case') }}
    </button>
    <IconButton icon="close" size="sm" :label="$t('ticket-media-pdf-find-close')" @click="emit('close')" />
  </div>
</template>
