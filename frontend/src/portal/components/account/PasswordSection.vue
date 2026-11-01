<script setup lang="ts">
/** Change the password the person signs in with. Their other sessions end. */
import { computed, ref } from 'vue'
import { useFluent } from 'fluent-vue'

import Button from '@/components/common/Button.vue'
import FormInput from '@/components/common/FormInput.vue'
import SectionCard from '@/components/common/SectionCard.vue'
import { useAccountApi } from '@/composables/useAccountApi'
import { extractErrorMessage } from '@/utils/errors'

const emit = defineEmits<{
  (e: 'success', message: string): void
  (e: 'error', message: string): void
}>()

const { $t: t } = useFluent()
const account = useAccountApi()

const current = ref('')
const next = ref('')
const confirm = ref('')
const saving = ref(false)

const mismatch = computed(() => confirm.value !== '' && confirm.value !== next.value)
const valid = computed(
  () => current.value !== '' && next.value.length >= 8 && next.value.length <= 128 && next.value === confirm.value,
)

async function change(): Promise<void> {
  if (!valid.value) return
  saving.value = true
  try {
    await account.password.change(current.value, next.value)
    current.value = ''
    next.value = ''
    confirm.value = ''
    emit('success', t('settings-security-success-changed'))
  } catch (e) {
    emit('error', extractErrorMessage(e, t('settings-security-error-change-failed')))
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <SectionCard content-padding="p-4 sm:p-6">
    <template #title>{{ t('settings-security-title') }}</template>
    <form class="flex flex-col gap-4" @submit.prevent="change">
      <FormInput
        v-model="current"
        type="password"
        autocomplete="current-password"
        size="sm"
        :label="t('settings-security-label-current')"
        required
        :disabled="saving"
      />
      <div class="grid grid-cols-1 sm:grid-cols-2 gap-4">
        <FormInput
          v-model="next"
          type="password"
          autocomplete="new-password"
          size="sm"
          :label="t('settings-security-label-new')"
          :description="t('settings-security-hint-length')"
          required
          :disabled="saving"
        />
        <FormInput
          v-model="confirm"
          type="password"
          autocomplete="new-password"
          size="sm"
          :label="t('settings-security-label-confirm')"
          :error="mismatch ? t('settings-security-error-mismatch') : undefined"
          required
          :disabled="saving"
        />
      </div>
      <div class="flex justify-end">
        <Button type="submit" :disabled="!valid" :loading="saving">
          {{ t('settings-security-submit-change') }}
        </Button>
      </div>
    </form>
  </SectionCard>
</template>
