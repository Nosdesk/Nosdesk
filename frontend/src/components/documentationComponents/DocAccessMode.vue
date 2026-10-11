<script setup lang="ts">
/**
 * Who can open a collection: everyone in the workspace, or only the people
 * and groups chosen. Chosen with nobody listed is admins only, and says so.
 */
const restricted = defineModel<boolean>({ required: true })

defineProps<{
  /** No people or groups are chosen yet. */
  nobodyChosen: boolean
}>()
</script>

<template>
  <div class="flex flex-col gap-2">
    <div class="flex gap-1 p-1 bg-surface-alt rounded-lg" role="radiogroup" :aria-label="$t('docs-access-label')">
      <button
        type="button"
        role="radio"
        :aria-checked="!restricted"
        class="flex-1 px-3 py-1.5 text-xs font-medium rounded-md transition-colors"
        :class="!restricted ? 'bg-surface text-primary shadow-sm' : 'text-tertiary hover:text-secondary'"
        @click="restricted = false"
      >
        {{ $t('docs-access-everyone') }}
      </button>
      <button
        type="button"
        role="radio"
        :aria-checked="restricted"
        class="flex-1 px-3 py-1.5 text-xs font-medium rounded-md transition-colors"
        :class="restricted ? 'bg-surface text-primary shadow-sm' : 'text-tertiary hover:text-secondary'"
        @click="restricted = true"
      >
        {{ $t('docs-access-chosen') }}
      </button>
    </div>
    <p v-if="restricted && nobodyChosen" class="text-xs text-tertiary" data-testid="doc-access-admins-only">
      {{ $t('docs-access-admins-only') }}
    </p>
  </div>
</template>
