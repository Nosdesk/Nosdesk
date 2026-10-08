<script setup lang="ts">
import { ticketRoute } from '@nosdesk/core/utils/ticketRoutes'
import { ref } from 'vue'
import { useRouter } from 'vue-router'
import { useQuery, useQueryCache } from '@pinia/colada'
import { useFluent } from 'fluent-vue'

import Button from '@/components/common/Button.vue'
import FormInput from '@/components/common/FormInput.vue'
import FormTextarea from '@/components/common/FormTextarea.vue'
import ArticleSuggestions from '@/components/requester/ArticleSuggestions.vue'
import RequestTypePicker from '@/components/requester/RequestTypePicker.vue'

import AttachmentPicker from '../components/AttachmentPicker.vue'
import PortalLayout from '../components/PortalLayout.vue'
import { createMyTicket, listRequestTypes, searchHelpArticles, type PortalAttachment } from '../service'

const { $t: t } = useFluent()
const router = useRouter()
const queryCache = useQueryCache()

const types = useQuery({ key: ['portal', 'request-types'], query: listRequestTypes })
const requestType = ref<number | null>(null)
const title = ref('')
const description = ref('')
const files = ref<PortalAttachment[]>([])
const submitting = ref(false)
const failed = ref(false)

async function submit(): Promise<void> {
  if (!title.value.trim()) return
  submitting.value = true
  failed.value = false
  try {
    const ticket = await createMyTicket(
      title.value.trim(),
      description.value.trim(),
      files.value.map((f) => f.id),
      requestType.value,
    )
    void queryCache.invalidateQueries({ key: ['portal', 'tickets'] })
    void router.push(ticketRoute(ticket.number))
  } catch {
    failed.value = true
    submitting.value = false
  }
}
</script>

<template>
  <PortalLayout wide>
    <header class="flex flex-col gap-1">
      <h1 class="text-2xl font-semibold text-primary">{{ t('portal-nav-new') }}</h1>
      <p class="text-sm text-secondary">{{ t('portal-new-intro') }}</p>
    </header>

    <div class="grid gap-6 lg:grid-cols-[minmax(0,1fr)_18rem] items-start">
      <form class="flex flex-col gap-5 bg-surface border border-default rounded-xl p-5 sm:p-6" @submit.prevent="submit">
        <RequestTypePicker
          v-if="types.data.value?.length"
          v-model="requestType"
          :types="types.data.value"
          :disabled="submitting"
        />
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
          :rows="7"
          resize="vertical"
          :disabled="submitting"
        />
        <AttachmentPicker v-model="files" :disabled="submitting" />
        <p v-if="failed" role="alert" class="text-sm text-status-error">{{ t('portal-new-failed') }}</p>
        <div class="flex items-center gap-3 border-t border-default -mx-5 sm:-mx-6 px-5 sm:px-6 pt-4">
          <Button type="button" variant="ghost" :disabled="submitting" @click="router.back()">
            {{ t('portal-new-cancel') }}
          </Button>
          <Button type="submit" class="ml-auto" icon="send" :loading="submitting" :disabled="!title.trim()">
            {{ t('portal-new-submit') }}
          </Button>
        </div>
      </form>

      <aside class="flex flex-col gap-4 lg:sticky lg:top-20">
        <ArticleSuggestions :subject="title" :search="searchHelpArticles" />
        <section class="flex flex-col gap-3 bg-surface border border-default rounded-xl p-4">
          <h2 class="text-sm font-semibold text-primary">{{ t('portal-new-next-title') }}</h2>
          <ol class="flex flex-col gap-3 text-sm text-secondary">
            <li v-for="(step, index) in [t('portal-new-next-1'), t('portal-new-next-2'), t('portal-new-next-3')]" :key="index" class="flex gap-3">
              <span
                class="w-5 h-5 shrink-0 rounded-full bg-surface-alt border border-default text-xs text-primary inline-flex items-center justify-center tabular-nums"
                aria-hidden="true"
              >
                {{ index + 1 }}
              </span>
              <span>{{ step }}</span>
            </li>
          </ol>
        </section>
      </aside>
    </div>
  </PortalLayout>
</template>
