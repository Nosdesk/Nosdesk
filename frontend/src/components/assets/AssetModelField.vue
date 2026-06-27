<!--
Asset model picker (NetBox-style "device type").

The primary "what is this?" control on the asset detail page. Pick a real
make+model from the catalog and the backend stamps the asset's
manufacturer, model, kind, and default specs. Not in the catalog yet?
Create it inline (manufacturer + name + kind) and it's there next time.
Selecting / clearing a model returns the updated asset, which the parent
re-hydrates from.
-->
<script setup lang="ts">
import { computed, ref } from 'vue'
import { useFluent } from 'fluent-vue'
import { useQueryCache } from '@pinia/colada'

import SearchableDropdown, { type DropdownOption } from '@/components/common/SearchableDropdown.vue'
import Button from '@/components/common/Button.vue'
import Icon from '@/components/common/Icon.vue'
import Modal from '@/components/Modal.vue'
import FormInput from '@/components/common/FormInput.vue'
import AlertMessage from '@/components/common/AlertMessage.vue'

import { useAssetModelsQuery, useManufacturersQuery } from '@/composables/useAssetCatalogQuery'
import { useAssetKindsQuery } from '@/composables/useAssetKindsQuery'
import {
  manufacturersService,
  assetModelsService,
  MANUFACTURERS_QUERY_KEY,
  ASSET_MODELS_QUERY_KEY,
} from '@nosdesk/core/services/assetCatalogService'
import { setAssetModel, clearAssetModel } from '@/services/assetService'
import { extractErrorMessage } from '@/utils/errors'
import type { Asset } from '@nosdesk/core/types/asset'

const props = defineProps<{
  assetId: number
  modelId: number | null
  kind: string
  manufacturerSnapshot: string | null
  modelSnapshot: string | null
  editable: boolean
}>()

const emit = defineEmits<{ (e: 'updated', asset: Asset): void }>()

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)
const queryCache = useQueryCache()

const { models } = useAssetModelsQuery()
const { manufacturers } = useManufacturersQuery()
const { kinds } = useAssetKindsQuery()

const picking = ref(false)
const saving = ref(false)
const error = ref<string | null>(null)
const selectModel = ref('')

const manufacturerName = (id: number) => manufacturers.value.find((m) => m.id === id)?.name ?? ''

const linkedModel = computed(() => models.value.find((m) => m.id === props.modelId) ?? null)

const linkedDisplay = computed(() => {
  if (linkedModel.value) {
    const mfr = manufacturerName(linkedModel.value.manufacturer_id)
    return mfr ? `${mfr} · ${linkedModel.value.name}` : linkedModel.value.name
  }
  // Fall back to the stamped snapshot columns on the asset.
  return [props.manufacturerSnapshot, props.modelSnapshot].filter(Boolean).join(' · ')
})

const CREATE = '__create__'
const modelOptions = computed<DropdownOption[]>(() => [
  { value: CREATE, label: t('asset-model-create-new') },
  ...models.value.map((m) => ({
    value: String(m.id),
    label: m.name,
    description: manufacturerName(m.manufacturer_id) || undefined,
  })),
])

async function stamp(modelId: number) {
  saving.value = true
  error.value = null
  try {
    const asset = await setAssetModel(props.assetId, modelId)
    emit('updated', asset)
    picking.value = false
  } catch (e) {
    error.value = extractErrorMessage(e, t('asset-model-set-failed'))
  } finally {
    saving.value = false
  }
}

function chooseModel(value: string) {
  selectModel.value = ''
  if (value === CREATE) {
    openCreate()
    return
  }
  const id = Number(value)
  if (id) void stamp(id)
}

async function clear() {
  saving.value = true
  error.value = null
  try {
    const asset = await clearAssetModel(props.assetId)
    emit('updated', asset)
  } catch (e) {
    error.value = extractErrorMessage(e, t('asset-model-clear-failed'))
  } finally {
    saving.value = false
  }
}

// ---- Inline quick-create -----------------------------------------
const NEW_MFR = '__new__'
const showCreate = ref(false)
const createMfr = ref('')
const newMfrName = ref('')
const createName = ref('')
const createKind = ref('')
const createError = ref<string | null>(null)
const creating = ref(false)

function openCreate() {
  createMfr.value = ''
  newMfrName.value = ''
  createName.value = ''
  createKind.value = props.kind || 'generic'
  createError.value = null
  showCreate.value = true
}

const manufacturerOptions = computed<DropdownOption[]>(() => [
  { value: NEW_MFR, label: t('asset-model-new-manufacturer') },
  ...manufacturers.value.map((m) => ({ value: String(m.id), label: m.name })),
])

const kindOptions = computed<DropdownOption[]>(() =>
  kinds.value.map((k) => ({ value: k.slug, label: k.label, icon: k.icon ?? undefined })),
)

const canCreate = computed(() => {
  const hasMfr = createMfr.value === NEW_MFR ? newMfrName.value.trim() !== '' : createMfr.value !== ''
  return hasMfr && createName.value.trim() !== '' && createKind.value !== ''
})

async function submitCreate() {
  if (!canCreate.value) return
  creating.value = true
  createError.value = null
  try {
    let manufacturerId: number
    if (createMfr.value === NEW_MFR) {
      const m = await manufacturersService.create({ name: newMfrName.value.trim() })
      manufacturerId = m.id
    } else {
      manufacturerId = Number(createMfr.value)
    }
    const model = await assetModelsService.create({
      manufacturer_id: manufacturerId,
      name: createName.value.trim(),
      kind: createKind.value,
    })
    await queryCache.invalidateQueries({ key: MANUFACTURERS_QUERY_KEY })
    await queryCache.invalidateQueries({ key: ASSET_MODELS_QUERY_KEY })
    showCreate.value = false
    await stamp(model.id)
  } catch (e) {
    createError.value = extractErrorMessage(e, t('asset-model-create-failed'))
  } finally {
    creating.value = false
  }
}
</script>

<template>
  <div class="flex flex-col gap-1">
    <div class="flex items-center justify-between min-h-6">
      <h3 class="text-xs font-medium text-tertiary">{{ $t('asset-model-label') }}</h3>
      <button
        v-if="modelId && editable && !picking"
        type="button"
        class="p-1 text-tertiary hover:text-status-error hover:bg-status-error-muted rounded transition-colors"
        :title="$t('asset-model-clear')"
        @click="clear"
      >
        <Icon name="close" />
      </button>
    </div>

    <!-- Linked model -->
    <div v-if="modelId && !picking" class="flex items-center gap-2 flex-wrap">
      <span class="inline-flex items-center gap-1.5 px-2 py-0.5 rounded-md text-sm font-medium bg-accent/10 text-accent">
        <Icon name="device" size="xs" />
        {{ linkedDisplay || $t('asset-model-none') }}
      </span>
      <Button v-if="editable" variant="ghost" size="sm" @click="picking = true">
        {{ $t('asset-model-change') }}
      </Button>
    </div>

    <!-- Picking -->
    <div v-else-if="picking" class="flex flex-col gap-1.5">
      <SearchableDropdown
        :model-value="selectModel"
        :options="modelOptions"
        :placeholder="$t('asset-model-search-placeholder')"
        size="sm"
        @update:model-value="(v) => chooseModel(String(v))"
      />
      <button type="button" class="self-start text-xs text-tertiary hover:text-secondary" @click="picking = false">
        {{ $t('asset-detail-action-cancel') }}
      </button>
    </div>

    <!-- No model -->
    <Button
      v-else-if="editable"
      variant="secondary"
      size="sm"
      icon="add"
      class="self-start"
      @click="picking = true"
    >
      {{ $t('asset-model-choose') }}
    </Button>
    <p v-else class="text-sm text-tertiary">{{ linkedDisplay || $t('asset-model-none') }}</p>

    <AlertMessage v-if="error" type="error" :message="error" />

    <!-- Inline create -->
    <Modal :show="showCreate" :title="$t('asset-model-create-title')" size="md" @close="showCreate = false">
      <div class="flex flex-col gap-3">
        <div class="flex flex-col gap-1.5">
          <label class="text-xs font-medium uppercase tracking-wide text-tertiary">
            {{ $t('asset-model-field-manufacturer') }}
          </label>
          <SearchableDropdown
            :model-value="createMfr"
            :options="manufacturerOptions"
            :placeholder="$t('asset-model-field-manufacturer-placeholder')"
            size="sm"
            @update:model-value="(v) => (createMfr = String(v))"
          />
          <FormInput
            v-if="createMfr === NEW_MFR"
            v-model="newMfrName"
            :placeholder="$t('asset-model-new-manufacturer-placeholder')"
            size="sm"
          />
        </div>
        <FormInput
          v-model="createName"
          :label="$t('asset-model-field-name')"
          :placeholder="$t('asset-model-field-name-placeholder')"
          size="sm"
        />
        <div class="flex flex-col gap-1.5">
          <label class="text-xs font-medium uppercase tracking-wide text-tertiary">
            {{ $t('asset-detail-field-kind') }}
          </label>
          <SearchableDropdown
            :model-value="createKind"
            :options="kindOptions"
            size="sm"
            @update:model-value="(v) => (createKind = String(v))"
          />
        </div>
        <AlertMessage v-if="createError" type="error" :message="createError" />
        <div class="flex justify-end gap-2 pt-1">
          <Button variant="secondary" :disabled="creating" @click="showCreate = false">
            {{ $t('asset-detail-action-cancel') }}
          </Button>
          <Button :disabled="!canCreate || creating" :loading="creating" @click="submitCreate">
            {{ $t('asset-model-create-submit') }}
          </Button>
        </div>
      </div>
    </Modal>
  </div>
</template>
