<script setup lang="ts">
import { computed, ref } from 'vue'
import { useFluent } from 'fluent-vue'
import SectionCard from '@/components/common/SectionCard.vue'
import SearchableDropdown, { type DropdownOption } from '@/components/common/SearchableDropdown.vue'
import FormInput from '@/components/common/FormInput.vue'
import Button from '@/components/common/Button.vue'
import Icon from '@/components/common/Icon.vue'
import { useColorFilter } from '@/composables/useColorFilter'
import { useToastStore } from '@nosdesk/core/stores/toast'
import { extractErrorMessage } from '@/utils/errors'
import { useAssetGroupsStore } from '@/stores/assetGroups'
import { createAssetGroup, setAssetGroupsForAsset } from '@/services/assetGroupService'
import type { AssetGroup } from '@nosdesk/core/types/asset'

const props = defineProps<{
  assetId: number
  groups: AssetGroup[]
  editable: boolean
}>()

const emit = defineEmits<{ (e: 'update:groups', groups: AssetGroup[]): void }>()

const fluent = useFluent()
const t = (k: string, args?: Record<string, string | number>) => fluent.$t(k, args)
const toast = useToastStore()
const { colorFilterStyle } = useColorFilter()
const store = useAssetGroupsStore()
void store.load()

const saving = ref(false)
const pendingAdd = ref('')

const assignedIds = computed(() => new Set(props.groups.map((g) => g.id)))

// Active groups not already on this asset, for the add picker.
const addOptions = computed<DropdownOption[]>(() =>
  store.active
    .filter((g) => !assignedIds.value.has(g.id))
    .map((g) => ({ value: String(g.id), label: g.name })),
)

async function persist(ids: number[]) {
  if (saving.value) return
  saving.value = true
  try {
    // The PUT returns the resulting refs, so the parent renders exactly what
    // was saved without reconstructing rows client-side.
    const saved = await setAssetGroupsForAsset(props.assetId, ids)
    emit('update:groups', saved)
  } catch (err) {
    toast.error(extractErrorMessage(err, t('asset-detail-groups-save-error')))
  } finally {
    saving.value = false
  }
}

function onAdd(value: string) {
  const id = Number(value)
  pendingAdd.value = ''
  if (!Number.isFinite(id) || assignedIds.value.has(id)) return
  void persist([...props.groups.map((g) => g.id), id])
}

function removeGroup(id: number) {
  void persist(props.groups.filter((g) => g.id !== id).map((g) => g.id))
}

// Inline create: agents organize inventory without leaving the asset.
const creating = ref(false)
const newName = ref('')
const createBusy = ref(false)

function startCreate() {
  creating.value = true
  newName.value = ''
}
function cancelCreate() {
  creating.value = false
  newName.value = ''
}
async function confirmCreate() {
  const name = newName.value.trim()
  if (!name || createBusy.value) return
  createBusy.value = true
  try {
    const created = await createAssetGroup({ name })
    await store.load(true) // surface the new group in the picker + list facet
    cancelCreate()
    await persist([...props.groups.map((g) => g.id), created.id])
  } catch (err) {
    toast.error(extractErrorMessage(err, t('asset-detail-groups-create-error')))
  } finally {
    createBusy.value = false
  }
}
</script>

<template>
  <SectionCard v-if="editable || groups.length > 0" content-padding="p-4">
    <template #title>{{ t('asset-detail-groups-title') }}</template>

    <div class="flex flex-col gap-3">
      <div v-if="groups.length > 0" class="flex flex-wrap gap-2">
        <span
          v-for="group in groups"
          :key="group.id"
          class="inline-flex items-center gap-2 px-3 py-1.5 bg-surface-alt rounded-lg border border-default"
        >
          <span
            class="w-3 h-3 rounded-full flex-shrink-0"
            :style="{ backgroundColor: group.color || '#6b7280', ...colorFilterStyle }"
          ></span>
          <span class="text-sm text-primary">{{ group.name }}</span>
          <button
            v-if="editable"
            type="button"
            class="text-tertiary hover:text-primary transition-colors disabled:opacity-50"
            :disabled="saving"
            :aria-label="t('asset-detail-groups-remove', { name: group.name })"
            @click="removeGroup(group.id)"
          >
            <Icon name="close" class="w-3.5 h-3.5" />
          </button>
        </span>
      </div>
      <p v-else class="text-sm text-tertiary">{{ t('asset-detail-groups-empty') }}</p>

      <template v-if="editable">
        <!-- Inline create: name + confirm/cancel. -->
        <div v-if="creating" class="flex items-center gap-2">
          <FormInput
            v-model="newName"
            :placeholder="t('asset-detail-groups-create-placeholder')"
            class="flex-1"
            @keyup.enter="confirmCreate"
          />
          <Button size="sm" :loading="createBusy" :disabled="!newName.trim()" @click="confirmCreate">
            {{ t('asset-detail-groups-create-confirm') }}
          </Button>
          <button
            type="button"
            class="p-1.5 text-tertiary hover:text-primary transition-colors"
            :aria-label="t('common-cancel')"
            @click="cancelCreate"
          >
            <Icon name="close" class="w-4 h-4" />
          </button>
        </div>
        <template v-else>
          <SearchableDropdown
            :model-value="pendingAdd"
            :options="addOptions"
            :placeholder="t('asset-detail-groups-add-placeholder')"
            :disabled="saving"
            @update:model-value="onAdd"
          />
          <button
            type="button"
            class="self-start text-sm text-accent hover:underline"
            @click="startCreate"
          >
            {{ t('asset-detail-groups-create-new') }}
          </button>
        </template>
      </template>
    </div>
  </SectionCard>
</template>
