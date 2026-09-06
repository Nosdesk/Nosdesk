<script setup lang="ts">
/**
 * Create or edit a documentation collection. One modal, one form
 * layout; mode switches title, validation, and save path.
 */
import { ref, computed, watch } from 'vue'
import axios from 'axios'
import { useFluent } from 'fluent-vue'
import {
  createCollection,
  updateCollection,
  setCollectionVisibility,
  type Collection,
  type CollectionWithDetails,
} from '@nosdesk/core/services/collectionService'
import { useSyncDocsStore } from '@nosdesk/core/sync/stores/documentation'
import {
  slugFromCollectionTitle,
  slugifyCollectionTitle,
} from '@nosdesk/core/utils/collectionSlug'
import { randomAccentColor } from '@nosdesk/core/utils/accentColor'
import Modal from '@/components/Modal.vue'
import Button from '@/components/common/Button.vue'
import Checkbox from '@/components/common/Checkbox.vue'
import ColorHueSlider from '@/components/common/ColorHueSlider.vue'
import DocumentIconSelector from '@/components/DocumentIconSelector.vue'
import FormInput from '@/components/common/FormInput.vue'
import AssignmentPicker from '@/components/common/AssignmentPicker.vue'
import type { SelectedPrincipal } from '@/components/common/AssignmentPicker.vue'

const props = defineProps<{
  mode: 'create' | 'edit'
  show: boolean
  collection?: CollectionWithDetails | null
}>()

const emit = defineEmits<{
  (e: 'close'): void
  (e: 'created', collection: Collection): void
  (e: 'saved', collection: CollectionWithDetails): void
}>()

const fluent = useFluent()
const t = (key: string) => fluent.$t(key)

const docs = useSyncDocsStore()

const name = ref('')
const slug = ref('')
const slugManuallyEdited = ref(false)
const description = ref('')
const icon = ref('📁')
const color = ref(randomAccentColor())
const hideTitles = ref(false)
const requireVerification = ref(false)
const selectedItems = ref<SelectedPrincipal[]>([])
const saving = ref(false)
const saveError = ref('')

const modalTitle = computed(() =>
  props.mode === 'create'
    ? t('docs-create-collection-title')
    : t('docs-edit-collection-title'),
)

const existingSlugs = computed(() => {
  const slugs = new Set(docs.allCollections.map((collection) => collection.slug))
  if (props.mode === 'edit' && props.collection) {
    slugs.delete(props.collection.slug)
  }
  return slugs
})

const resolvedSlug = computed(() => slugifyCollectionTitle(slug.value.trim()))

const slugMissing = computed(
  () => resolvedSlug.value.length === 0 && name.value.trim().length > 0,
)

const slugTaken = computed(
  () => !slugMissing.value && existingSlugs.value.has(resolvedSlug.value),
)

const slugFieldError = computed(() => {
  if (slugTaken.value) return t('docs-create-collection-slug-taken')
  if (slugMissing.value) return t('docs-create-collection-slug-required')
  return undefined
})

const slugFieldDescription = computed(() => {
  if (slugFieldError.value) return undefined
  if (props.mode === 'create' && !slugManuallyEdited.value && slug.value.trim()) {
    return t('docs-create-collection-slug-auto-help')
  }
  return t('docs-edit-collection-slug-help')
})

const isPublic = computed(() => selectedItems.value.length === 0)

const canSubmit = computed(
  () =>
    name.value.trim().length > 0
    && !slugMissing.value
    && !slugTaken.value
    && !saving.value,
)

const submitLabel = computed(() => {
  if (saving.value) {
    return props.mode === 'create'
      ? t('docs-create-collection-creating')
      : t('docs-edit-collection-saving')
  }
  return props.mode === 'create'
    ? t('docs-create-collection-submit')
    : t('docs-edit-collection-save')
})

function principalsFromCollection(collection: CollectionWithDetails): SelectedPrincipal[] {
  const items: SelectedPrincipal[] = collection.visible_to_groups.map((group) => ({
    type: 'group',
    id: String(group.id),
    name: group.name,
  }))
  for (const user of collection.visible_to_users ?? []) {
    items.push({
      type: 'user',
      id: user.uuid,
      name: user.name,
      avatar: user.avatar_url,
    })
  }
  return items
}

function visibilityFromSelection(items: SelectedPrincipal[]) {
  const groupIds = items
    .filter((i) => i.type === 'group')
    .map((i) => parseInt(i.id, 10))
  const userUuids = items
    .filter((i) => i.type === 'user')
    .map((i) => i.id)
  const visible_to_groups = items
    .filter((i) => i.type === 'group')
    .map((i) => ({ id: parseInt(i.id, 10), name: i.name }))
  const visible_to_users = items
    .filter((i) => i.type === 'user')
    .map((i) => ({ uuid: i.id, name: i.name, avatar_url: i.avatar ?? null }))
  return {
    groupIds,
    userUuids,
    visible_to_groups,
    visible_to_users,
    is_public: items.length === 0,
  }
}

function resetCreateForm() {
  name.value = ''
  slug.value = ''
  slugManuallyEdited.value = false
  description.value = ''
  icon.value = '📁'
  color.value = randomAccentColor()
  hideTitles.value = false
  requireVerification.value = false
  selectedItems.value = []
  saveError.value = ''
}

function seedEditForm(collection: CollectionWithDetails) {
  name.value = collection.name
  slug.value = collection.slug
  slugManuallyEdited.value = true
  description.value = collection.description ?? ''
  icon.value = collection.icon || '📁'
  color.value = collection.color || '#6366f1'
  hideTitles.value = collection.hide_titles_from_non_members ?? false
  requireVerification.value = collection.require_verification ?? false
  selectedItems.value = principalsFromCollection(collection)
  saveError.value = ''
}

watch(
  () => [props.show, props.mode, props.collection?.id] as const,
  ([show, mode, collectionId]) => {
    if (!show) return
    if (mode === 'create') {
      resetCreateForm()
      return
    }
    if (collectionId && props.collection) {
      seedEditForm(props.collection)
    }
  },
  { immediate: true },
)

watch(name, (newName) => {
  if (props.mode !== 'create' || slugManuallyEdited.value) return
  const trimmed = newName.trim()
  if (!trimmed) {
    slug.value = ''
    return
  }
  slug.value = slugFromCollectionTitle(trimmed, existingSlugs.value)
})

function onSlugInput() {
  slugManuallyEdited.value = true
  saveError.value = ''
}

function onSlugBlur() {
  const trimmed = slug.value.trim()
  if (!trimmed) {
    slug.value = ''
    return
  }
  slug.value = slugifyCollectionTitle(trimmed)
}

async function handleCreate() {
  const trimmedName = name.value.trim()
  if (!trimmedName) {
    saveError.value = t('docs-edit-collection-name-required')
    return
  }

  const finalSlug = resolvedSlug.value
  if (!finalSlug) {
    saveError.value = t('docs-create-collection-slug-required')
    return
  }
  if (existingSlugs.value.has(finalSlug)) {
    saveError.value = t('docs-create-collection-slug-taken')
    return
  }

  saving.value = true
  saveError.value = ''

  const { groupIds, userUuids } = visibilityFromSelection(selectedItems.value)

  try {
    const created = await createCollection({
      name: trimmedName,
      slug: finalSlug,
      description: description.value.trim() || undefined,
      icon: icon.value || undefined,
      color: color.value || undefined,
      visible_to_group_ids: groupIds.length > 0 ? groupIds : undefined,
    })

    if (!created) {
      saveError.value = t('docs-create-collection-error')
      return
    }

    if (userUuids.length > 0) {
      const visibilityOk = await setCollectionVisibility(created.id, groupIds, userUuids)
      if (!visibilityOk) {
        saveError.value = t('docs-create-collection-error')
        return
      }
    }

    if (hideTitles.value || requireVerification.value) {
      const updated = await updateCollection(created.id, {
        hide_titles_from_non_members: hideTitles.value,
        require_verification: requireVerification.value,
      })
      if (!updated) {
        saveError.value = t('docs-create-collection-error')
        return
      }
    }

    emit('created', created)
    emit('close')
  } catch (error) {
    if (axios.isAxiosError(error) && error.response?.status === 409) {
      saveError.value = t('docs-create-collection-slug-taken')
    } else {
      saveError.value = t('docs-create-collection-error')
    }
  } finally {
    saving.value = false
  }
}

async function handleEdit() {
  if (!props.collection) return

  const trimmedName = name.value.trim()
  if (!trimmedName) {
    saveError.value = t('docs-edit-collection-name-required')
    return
  }

  const finalSlug = resolvedSlug.value
  if (!finalSlug) {
    saveError.value = t('docs-create-collection-slug-required')
    return
  }
  if (existingSlugs.value.has(finalSlug)) {
    saveError.value = t('docs-create-collection-slug-taken')
    return
  }

  saving.value = true
  saveError.value = ''

  const visibility = visibilityFromSelection(selectedItems.value)

  try {
    const updated = await updateCollection(props.collection.id, {
      name: trimmedName,
      slug: finalSlug,
      description: description.value.trim(),
      icon: icon.value.trim() || undefined,
      color: color.value.trim() || undefined,
      hide_titles_from_non_members: hideTitles.value,
      require_verification: requireVerification.value,
    })
    if (!updated) {
      saveError.value = t('docs-edit-collection-save-error')
      return
    }

    const visibilityOk = await setCollectionVisibility(
      props.collection.id,
      visibility.groupIds,
      visibility.userUuids,
    )
    if (!visibilityOk) {
      saveError.value = t('docs-edit-collection-save-error')
      return
    }

    emit('saved', {
      ...props.collection,
      ...updated,
      visible_to_groups: visibility.visible_to_groups,
      visible_to_users: visibility.visible_to_users,
      is_public: visibility.is_public,
    } as CollectionWithDetails)
    emit('close')
  } catch (error) {
    if (axios.isAxiosError(error) && error.response?.status === 409) {
      saveError.value = t('docs-create-collection-slug-taken')
    } else {
      saveError.value = t('docs-edit-collection-save-error')
    }
  } finally {
    saving.value = false
  }
}

function handleSubmit() {
  if (props.mode === 'create') {
    void handleCreate()
  } else {
    void handleEdit()
  }
}
</script>

<template>
  <Modal :show="show" :title="modalTitle" size="md" @close="emit('close')">
    <form class="flex flex-col gap-3" @submit.prevent="handleSubmit">
      <!-- Identity: icon + name/slug -->
      <div class="flex items-center gap-3">
        <DocumentIconSelector
          :initial-icon="icon"
          size="md"
          class="shrink-0"
          @update:icon="icon = $event"
        />
        <div class="flex-1 min-w-0 flex flex-col gap-2">
          <FormInput
            v-model="name"
            size="sm"
            required
            autofocus
            :label="$t('docs-edit-collection-name')"
          />
          <FormInput
            v-model="slug"
            size="sm"
            required
            class="[&_input]:font-mono"
            :label="$t('docs-edit-collection-slug')"
            :error="slugFieldError"
            :description="slugFieldDescription"
            @input="onSlugInput"
            @blur="onSlugBlur"
          />
        </div>
      </div>

      <FormInput
        v-model="description"
        size="sm"
        :label="$t('docs-edit-collection-description')"
        :placeholder="$t('docs-edit-collection-description-placeholder')"
        :description="mode === 'edit' ? $t('docs-edit-collection-description-help') : undefined"
      />

      <ColorHueSlider
        v-model="color"
        :label="$t('docs-edit-collection-color')"
      />

      <!-- Access -->
      <div class="flex flex-col gap-2 pt-1 border-t border-subtle">
        <div class="flex items-center justify-between gap-2 min-h-5">
          <span class="text-3xs font-semibold uppercase tracking-wide text-tertiary">
            {{ $t('docs-create-collection-access-heading') }}
          </span>
          <span
            v-if="isPublic"
            class="text-2xs text-status-success shrink-0"
          >
            {{ $t('collection-badge-public') }}
          </span>
        </div>
        <AssignmentPicker
          :selectedItems="selectedItems"
          @update:selectedItems="selectedItems = $event"
          :placeholder="$t('docs-collection-visibility-picker-placeholder')"
        />
      </div>

      <!-- Privacy -->
      <div class="flex gap-2.5">
        <div class="flex h-[18px] shrink-0 items-center">
          <Checkbox
            :id="`${mode}-collection-hide-titles`"
            v-model="hideTitles"
            size="sm"
            :aria-label="$t('docs-edit-collection-hide-titles-aria')"
          />
        </div>
        <div class="min-w-0 flex flex-col gap-0.5">
          <label
            :for="`${mode}-collection-hide-titles`"
            class="cursor-pointer text-xs font-medium leading-snug text-primary"
          >
            {{ $t('docs-edit-collection-hide-titles-label') }}
          </label>
          <p class="text-2xs leading-snug text-tertiary">
            {{ $t('docs-edit-collection-hide-titles-help') }}
          </p>
        </div>
      </div>

      <!-- Verification policy -->
      <div class="flex gap-2.5">
        <div class="flex h-[18px] shrink-0 items-center">
          <Checkbox
            :id="`${mode}-collection-require-verification`"
            v-model="requireVerification"
            size="sm"
            :aria-label="$t('docs-edit-collection-require-verification-aria')"
          />
        </div>
        <div class="min-w-0 flex flex-col gap-0.5">
          <label
            :for="`${mode}-collection-require-verification`"
            class="cursor-pointer text-xs font-medium leading-snug text-primary"
          >
            {{ $t('docs-edit-collection-require-verification-label') }}
          </label>
          <p class="text-2xs leading-snug text-tertiary">
            {{ $t('docs-edit-collection-require-verification-help') }}
          </p>
        </div>
      </div>

      <p v-if="saveError" class="text-xs text-status-error" role="alert">
        {{ saveError }}
      </p>
    </form>

    <template #footer>
      <div class="flex items-center justify-end gap-2">
        <Button variant="ghost" size="sm" :disabled="saving" @click="emit('close')">
          {{ $t('docs-edit-collection-cancel') }}
        </Button>
        <Button size="sm" :loading="saving" :disabled="!canSubmit" @click="handleSubmit">
          {{ submitLabel }}
        </Button>
      </div>
    </template>
  </Modal>
</template>
