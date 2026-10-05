<script setup lang="ts">
/**
 * The starter catalogue: ready-made rules an admin adds as drafts and
 * adjusts in the editor before they go live. Each lists its steps in plain
 * words. A starter whose name is already taken in the workspace shows as
 * added.
 */
import { computed, ref } from 'vue'
import { useFluent } from 'fluent-vue'
import { useRouter } from 'vue-router'
import { useQuery, useQueryCache } from '@pinia/colada'

import Modal from '@/components/Modal.vue'
import AlertMessage from '@/components/common/AlertMessage.vue'
import Button from '@/components/common/Button.vue'
import Icon from '@/components/common/Icon.vue'
import { useRuleStepText } from '@/composables/useRuleStepText'
import { extractErrorMessage } from '@/utils/errors'
import rulesService from '@nosdesk/core/services/rulesService'
import { useDateStore } from '@nosdesk/core/stores/dateStore'
import { useToastStore } from '@nosdesk/core/stores/toast'
import type { Rule, StarterRule } from '@nosdesk/core/types/rule'

const props = defineProps<{
  show: boolean
  /** The workspace's rules, to mark the starters already added. */
  rules: Rule[]
}>()
const emit = defineEmits<{ close: [] }>()

const { $t: t } = useFluent()
const router = useRouter()
const toast = useToastStore()
const queryCache = useQueryCache()
const dateStore = useDateStore()

// Names and descriptions come back in the app's language.
const startersQuery = useQuery({
  key: () => ['rules', 'starters', dateStore.locale],
  query: () => rulesService.starterCatalog(dateStore.locale),
  enabled: () => props.show,
  staleTime: 60 * 60 * 1000,
})
const starters = computed(() => startersQuery.data.value ?? [])
const stepText = useRuleStepText({ enabled: () => props.show, userUuids: () => [] })
const steps = (starter: StarterRule) =>
  starter.actions.map((a) => stepText.planned(a)).filter((s): s is string => !!s)

const takenNames = computed(() => new Set(props.rules.map((r) => r.name.trim().toLowerCase())))
const isAdded = (starter: StarterRule) => takenNames.value.has(starter.name.trim().toLowerCase())

const adding = ref<string | null>(null)
const error = ref('')

async function add(starter: StarterRule) {
  adding.value = starter.id
  error.value = ''
  try {
    const created = await rulesService.create({
      name: starter.name,
      description: starter.description || null,
      trigger_kind: 'manual',
      conditions: [],
      actions: starter.actions,
    })
    await queryCache.invalidateQueries({ key: ['rules'] })
    toast.success(t('admin-rules-starters-toast', { name: created.name }))
    emit('close')
    await router.push({ name: 'admin-rules-edit', params: { id: created.id } })
  } catch (err) {
    error.value = extractErrorMessage(err, t('admin-rules-starters-error'))
  } finally {
    adding.value = null
  }
}
</script>

<template>
  <Modal
    :show="show"
    :title="t('admin-rules-starters-title')"
    :description="t('admin-rules-starters-description')"
    size="md"
    @close="emit('close')"
  >
    <div class="flex flex-col gap-3">
      <AlertMessage v-if="error" type="error" :message="error" />
      <AlertMessage v-if="startersQuery.error.value" type="error" :message="t('admin-rules-starters-error-load')" />
      <ul class="flex flex-col gap-2">
        <li
          v-for="starter in starters"
          :key="starter.id"
          class="flex flex-col gap-3 rounded-lg border border-default bg-surface px-4 py-3 sm:flex-row sm:items-start sm:justify-between"
        >
          <div class="flex flex-col gap-1 min-w-0">
            <p class="text-sm font-medium text-primary">{{ starter.name }}</p>
            <p v-if="starter.description" class="text-xs text-secondary">{{ starter.description }}</p>
            <ul class="flex flex-col gap-0.5 pl-4 list-disc text-xs text-tertiary">
              <li v-for="(step, i) in steps(starter)" :key="i">{{ step }}</li>
            </ul>
          </div>
          <span
            v-if="isAdded(starter)"
            class="inline-flex flex-shrink-0 items-center gap-1 text-xs text-secondary"
          >
            <Icon name="check" size="xs" />
            {{ t('admin-rules-starters-added') }}
          </span>
          <Button
            v-else
            class="flex-shrink-0"
            variant="secondary"
            size="sm"
            icon="add"
            :loading="adding === starter.id"
            :disabled="adding !== null"
            @click="add(starter)"
          >
            {{ t('admin-rules-starters-add') }}
          </Button>
        </li>
      </ul>
    </div>
  </Modal>
</template>
