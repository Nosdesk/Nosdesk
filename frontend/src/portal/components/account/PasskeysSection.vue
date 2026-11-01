<script setup lang="ts">
/** The person's passkeys: add one with the browser's prompt, or remove one with the password. */
import { onMounted, ref } from 'vue'
import { useFluent } from 'fluent-vue'

import Button from '@/components/common/Button.vue'
import FormInput from '@/components/common/FormInput.vue'
import SectionCard from '@/components/common/SectionCard.vue'
import { usePasskeys } from '@/composables/usePasskeys'
import { formatDate } from '@nosdesk/core/utils/dateUtils'

const emit = defineEmits<{
  (e: 'success', message: string): void
  (e: 'error', message: string): void
}>()

const { $t: t } = useFluent()
const keys = usePasskeys()
onMounted(() => void keys.loadPasskeys())

function report(ok: boolean): void {
  if (ok && keys.successMessage.value) emit('success', keys.successMessage.value)
  else if (!ok && keys.error.value) emit('error', keys.error.value)
}

const adding = ref(false)
const name = ref('')
async function add(): Promise<void> {
  const ok = await keys.registerPasskey(name.value.trim() || undefined)
  report(ok)
  if (ok) {
    adding.value = false
    name.value = ''
  }
}

const removing = ref<string | null>(null)
const password = ref('')
async function remove(): Promise<void> {
  if (!removing.value || !password.value) return
  const ok = await keys.deletePasskey(removing.value, password.value)
  report(ok)
  password.value = ''
  if (ok) removing.value = null
}
</script>

<template>
  <SectionCard content-padding="p-4 sm:p-6">
    <template #title>{{ t('settings-passkey-section-title') }}</template>
    <template v-if="keys.isSupported.value && !adding" #headerActions>
      <Button size="sm" variant="secondary" icon="add" :disabled="!keys.canAddPasskey.value" @click="adding = true">
        {{ t('settings-passkey-add-button') }}
      </Button>
    </template>

    <div class="flex flex-col gap-4">
      <p v-if="!keys.isSupported.value" class="text-sm text-secondary">
        {{ t('settings-passkey-unsupported-description') }}
      </p>

      <form v-if="adding" class="flex flex-col sm:flex-row sm:items-end gap-3" @submit.prevent="add">
        <FormInput
          v-model="name"
          size="sm"
          class="flex-1"
          :label="t('settings-passkey-add-modal-name-label')"
          :placeholder="t('settings-passkey-add-modal-name-placeholder')"
        />
        <div class="flex gap-2">
          <Button type="submit" :loading="keys.registering.value">{{ t('settings-passkey-add-modal-create') }}</Button>
          <Button variant="secondary" @click="adding = false">{{ t('settings-passkey-modal-cancel') }}</Button>
        </div>
      </form>

      <div v-if="keys.isSupported.value && !keys.hasPasskeys.value && !keys.loading.value" class="flex flex-col gap-1">
        <p class="text-sm font-medium text-primary">{{ t('settings-passkey-empty-title') }}</p>
        <p class="text-sm text-secondary">{{ t('settings-passkey-empty-self-description') }}</p>
      </div>

      <ul v-if="keys.hasPasskeys.value" class="flex flex-col gap-2">
        <li
          v-for="key in keys.passkeys.value"
          :key="key.id"
          class="flex flex-col gap-3 rounded-lg bg-surface-alt px-4 py-3"
        >
          <div class="flex items-center gap-3">
            <div class="flex flex-col min-w-0 flex-1">
              <span class="text-sm font-medium text-primary truncate">
                {{ key.name }}
                <span v-if="key.backup_eligible" class="ml-1 text-xs font-normal text-tertiary">
                  {{ t('settings-passkey-synced-badge') }}
                </span>
              </span>
              <span class="text-xs text-secondary">
                {{
                  key.last_used_at
                    ? t('settings-passkey-last-used', { date: formatDate(key.last_used_at) })
                    : t('settings-passkey-never-used')
                }}
              </span>
            </div>
            <Button
              v-if="removing !== key.id"
              size="sm"
              variant="ghost-danger"
              @click="(removing = key.id), (password = '')"
            >
              {{ t('settings-passkey-delete-modal-confirm') }}
            </Button>
          </div>
          <form v-if="removing === key.id" class="flex flex-col sm:flex-row sm:items-end gap-3" @submit.prevent="remove">
            <FormInput
              v-model="password"
              type="password"
              autocomplete="current-password"
              size="sm"
              class="flex-1"
              :label="t('settings-passkey-delete-modal-password-label')"
              required
            />
            <div class="flex gap-2">
              <Button type="submit" variant="danger" :disabled="!password" :loading="keys.loading.value">
                {{ t('settings-passkey-delete-modal-confirm') }}
              </Button>
              <Button variant="secondary" @click="removing = null">{{ t('settings-passkey-modal-cancel') }}</Button>
            </div>
          </form>
        </li>
      </ul>
    </div>
  </SectionCard>
</template>
