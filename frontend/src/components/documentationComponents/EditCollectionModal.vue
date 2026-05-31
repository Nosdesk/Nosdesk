<script setup lang="ts">
/**
 * Edit collection modal. Replaces the inline-rename affordance
 * with a richer surface that lets a technician change the
 * collection's identity (name, slug, icon, color), short
 * description tagline, and the title-leak guard for
 * cross-collection wikilinks.
 *
 * Permissions, page contents, and the rich Yjs description live
 * elsewhere — this modal is for the collection's metadata only.
 */
import { ref, watch } from 'vue'
import { useFluent } from 'fluent-vue'
import type { CollectionWithDetails } from '@/services/collectionService'
import { updateCollection } from '@/services/collectionService'
import Modal from '@/components/Modal.vue'
import Checkbox from '@/components/common/Checkbox.vue'

const fluent = useFluent()
const t = (key: string) => fluent.$t(key)

interface Props {
  collection: CollectionWithDetails | null
}

const props = defineProps<Props>()

const emit = defineEmits<{
  (e: 'close'): void
  (e: 'saved', collection: CollectionWithDetails): void
}>()

const name = ref('')
const slug = ref('')
const description = ref('')
const icon = ref('')
const color = ref('')
const hideTitles = ref(false)
const saving = ref(false)
const saveError = ref('')

// Re-seed the form whenever the modal binds to a different
// collection. Done as a watcher rather than computed init so the
// user can keep typing into a field even if the source data
// updates underneath them; only the source-collection switch
// triggers a re-seed.
watch(
  () => props.collection?.id,
  () => {
    const c = props.collection
    if (!c) return
    name.value = c.name
    slug.value = c.slug
    description.value = c.description ?? ''
    icon.value = c.icon ?? ''
    color.value = c.color ?? ''
    hideTitles.value = c.hide_titles_from_non_members ?? false
    saveError.value = ''
  },
  { immediate: true },
)

const isOpen = ref(false)
watch(
  () => props.collection,
  (c) => { isOpen.value = c !== null },
  { immediate: true },
)

async function handleSave() {
  if (!props.collection) return
  const trimmedName = name.value.trim()
  if (!trimmedName) {
    saveError.value = t('docs-edit-collection-name-required')
    return
  }
  saving.value = true
  saveError.value = ''
  try {
    const updated = await updateCollection(props.collection.id, {
      name: trimmedName,
      slug: slug.value.trim() || undefined,
      description: description.value.trim(),
      icon: icon.value.trim() || undefined,
      color: color.value.trim() || undefined,
      hide_titles_from_non_members: hideTitles.value,
    })
    if (!updated) {
      saveError.value = t('docs-edit-collection-save-error')
      return
    }
    emit('saved', { ...props.collection, ...updated } as CollectionWithDetails)
    emit('close')
  } catch {
    saveError.value = t('docs-edit-collection-save-error')
  } finally {
    saving.value = false
  }
}

function handleCancel() {
  emit('close')
}
</script>

<template>
  <Modal :show="isOpen" :title="$t('docs-edit-collection-title')" size="md" @close="handleCancel">
    <form class="flex flex-col gap-4" @submit.prevent="handleSave">
      <div class="flex flex-col gap-1.5">
        <label for="ec-name" class="text-sm font-medium text-primary">{{ $t('docs-edit-collection-name') }}</label>
        <input
          id="ec-name"
          v-model="name"
          type="text"
          required
          autofocus
          class="w-full rounded-lg border border-default bg-surface-alt px-3 py-2 text-sm text-primary placeholder:text-tertiary focus:border-accent focus:ring-2 focus:ring-accent/20 focus:outline-none"
        />
      </div>

      <div class="flex flex-col gap-1.5">
        <label for="ec-slug" class="text-sm font-medium text-primary">{{ $t('docs-edit-collection-slug') }}</label>
        <input
          id="ec-slug"
          v-model="slug"
          type="text"
          class="w-full rounded-lg border border-default bg-surface-alt px-3 py-2 font-mono text-sm text-primary placeholder:text-tertiary focus:border-accent focus:ring-2 focus:ring-accent/20 focus:outline-none"
        />
        <p class="text-xs text-tertiary">
          {{ $t('docs-edit-collection-slug-help') }}
        </p>
      </div>

      <div class="grid grid-cols-2 gap-3">
        <div class="flex flex-col gap-1.5">
          <label for="ec-icon" class="text-sm font-medium text-primary">{{ $t('docs-edit-collection-icon') }}</label>
          <input
            id="ec-icon"
            v-model="icon"
            type="text"
            placeholder="📁"
            maxlength="4"
            class="w-full rounded-lg border border-default bg-surface-alt px-3 py-2 text-center text-lg focus:border-accent focus:ring-2 focus:ring-accent/20 focus:outline-none"
          />
        </div>
        <div class="flex flex-col gap-1.5">
          <label for="ec-color" class="text-sm font-medium text-primary">{{ $t('docs-edit-collection-color') }}</label>
          <input
            id="ec-color"
            v-model="color"
            type="color"
            class="h-10 w-full cursor-pointer rounded-lg border border-default bg-surface-alt"
          />
        </div>
      </div>

      <div class="flex flex-col gap-1.5">
        <label for="ec-description" class="text-sm font-medium text-primary">
          {{ $t('docs-edit-collection-description') }}
        </label>
        <input
          id="ec-description"
          v-model="description"
          type="text"
          :placeholder="$t('docs-edit-collection-description-placeholder')"
          class="w-full rounded-lg border border-default bg-surface-alt px-3 py-2 text-sm text-primary placeholder:text-tertiary focus:border-accent focus:ring-2 focus:ring-accent/20 focus:outline-none"
        />
        <p class="text-xs text-tertiary">
          {{ $t('docs-edit-collection-description-help') }}
        </p>
      </div>

      <div class="flex items-start gap-3 rounded-lg border border-default bg-surface-alt p-3">
        <Checkbox
          v-model="hideTitles"
          size="sm"
          :aria-label="$t('docs-edit-collection-hide-titles-aria')"
        />
        <div class="flex flex-col gap-0.5">
          <span class="text-sm font-medium text-primary">
            {{ $t('docs-edit-collection-hide-titles-label') }}
          </span>
          <span class="text-xs text-tertiary">
            {{ $t('docs-edit-collection-hide-titles-help') }}
          </span>
        </div>
      </div>

      <p v-if="saveError" class="text-sm text-status-error" role="alert">
        {{ saveError }}
      </p>
    </form>

    <template #footer>
      <button
        type="button"
        @click="handleCancel"
        :disabled="saving"
        class="px-4 py-2 text-sm text-secondary transition-colors hover:text-primary disabled:opacity-50"
      >
        {{ $t('docs-edit-collection-cancel') }}
      </button>
      <button
        type="button"
        @click="handleSave"
        :disabled="saving"
        class="rounded-lg bg-accent px-4 py-2 text-sm font-medium text-on-accent transition-colors hover:bg-accent-hover disabled:cursor-not-allowed disabled:opacity-50"
      >
        {{ saving ? $t('docs-edit-collection-saving') : $t('docs-edit-collection-save') }}
      </button>
    </template>
  </Modal>
</template>
