<script setup lang="ts">
import { ref } from 'vue'
import { useRouter } from 'vue-router'
import { useQueryCache } from '@pinia/colada'
import { useFluent } from 'fluent-vue'

import Button from '@/components/common/Button.vue'
import FormInput from '@/components/common/FormInput.vue'
import FormTextarea from '@/components/common/FormTextarea.vue'

import PortalLayout from '../components/PortalLayout.vue'
import { createMyTicket } from '../service'

const { $t: t } = useFluent()
const router = useRouter()
const queryCache = useQueryCache()

const title = ref('')
const description = ref('')
const submitting = ref(false)
const failed = ref(false)

async function submit(): Promise<void> {
  if (!title.value.trim()) return
  submitting.value = true
  failed.value = false
  try {
    const ticket = await createMyTicket(title.value.trim(), description.value.trim())
    void queryCache.invalidateQueries({ key: ['portal', 'tickets'] })
    void router.push(`/tickets/${ticket.id}`)
  } catch {
    failed.value = true
    submitting.value = false
  }
}
</script>

<template>
  <PortalLayout>
    <h1 class="text-xl font-semibold text-primary">{{ t('portal-nav-new') }}</h1>
    <form class="flex flex-col gap-4 bg-surface border border-default rounded-xl p-5" @submit.prevent="submit">
      <FormInput
        v-model="title"
        :label="t('portal-new-subject-label')"
        :placeholder="t('portal-new-subject-placeholder')"
        required
        :disabled="submitting"
      />
      <FormTextarea
        v-model="description"
        :label="t('portal-new-description-label')"
        :placeholder="t('portal-new-description-placeholder')"
        :rows="6"
        resize="vertical"
        :disabled="submitting"
      />
      <p v-if="failed" role="alert" class="text-sm text-status-error">{{ t('portal-new-failed') }}</p>
      <Button type="submit" class="self-end" :loading="submitting" :disabled="!title.trim()">
        {{ t('portal-new-submit') }}
      </Button>
    </form>
  </PortalLayout>
</template>
