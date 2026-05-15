<script setup lang="ts">
/**
 * Hover-revealed action pair for sidebar nav rows. Both
 * collection rows and page rows render the same primitive so
 * the affordances stay consistent: a "more" three-dot menu
 * trigger, plus an inline add-page button. Visibility is
 * controlled by the parent's `group` class via
 * `group-hover:opacity-100`.
 *
 * Dumb component: emits events; the parent owns menu state and
 * pending-create state. Keeps it reusable for any future row
 * type.
 */
import { useFluent } from 'fluent-vue'
import Icon from '@/components/common/Icon.vue'

useFluent()

interface Props {
  /** Used in aria-label / title for both buttons. */
  label: string
  /** When true, the add button shows a spinner and disables. */
  creating?: boolean
}

const props = withDefaults(defineProps<Props>(), { creating: false })

const emit = defineEmits<{
  (e: 'more', event: MouseEvent): void
  (e: 'add'): void
}>()

const onMore = (event: MouseEvent) => {
  event.stopPropagation()
  emit('more', event)
}

const onAdd = (event: MouseEvent) => {
  event.stopPropagation()
  if (!props.creating) emit('add')
}
</script>

<template>
  <div
    class="row-actions ml-1 flex flex-shrink-0 items-center gap-0.5 opacity-0 transition-opacity focus-within:opacity-100 group-hover:opacity-100"
  >
    <button
      type="button"
      @click="onMore"
      :aria-label="$t('docs-nav-row-more', { label })"
      :title="$t('docs-nav-row-more', { label })"
      class="rounded p-0.5 text-tertiary hover:bg-surface-hover hover:text-primary"
    >
      <Icon name="more" size="xs" />
    </button>
    <button
      type="button"
      @click="onAdd"
      :disabled="creating"
      :aria-label="$t('docs-nav-row-add', { label })"
      :title="$t('docs-nav-row-add', { label })"
      class="rounded p-0.5 text-tertiary hover:bg-surface-hover hover:text-primary disabled:opacity-50"
    >
      <Icon name="add" size="xs" :class="{ 'animate-spin': creating }" />
    </button>
  </div>
</template>

<style scoped>
/* Touch devices reach the same actions via the row's long-press
   handler (see useLongPress in DocumentationNavItem / the
   collection row). Hiding the buttons there entirely is the
   right call: the row's resting state stays clean, and there's
   no platform mismatch where buttons appear that don't belong
   to a hover-driven UI. */
@media (hover: none) and (pointer: coarse) {
  .row-actions {
    display: none;
  }
}
</style>
