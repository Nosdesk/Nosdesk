<script setup lang="ts">
import { computed, ref } from 'vue'
import { RouterLink } from 'vue-router'
import { useQuery, useQueryCache } from '@pinia/colada'
import { useFluent } from 'fluent-vue'

import Button from '@/components/common/Button.vue'
import FormTextarea from '@/components/common/FormTextarea.vue'
import Icon from '@/components/common/Icon.vue'
import StatusPill from '@/components/common/StatusPill.vue'
import CommentContent from '@/components/ticketComponents/CommentContent.vue'
import { formatRelativeTime } from '@nosdesk/core/utils/dateUtils'

import PortalLayout from '../components/PortalLayout.vue'
import { stateTone } from '../stateTone'
import { attachmentUrl, getMyTicket, replyToMyTicket } from '../service'

const props = defineProps<{ id: string }>()
const { $t: t } = useFluent()
const queryCache = useQueryCache()

const ticketId = computed(() => Number(props.id))
const key = computed(() => ['portal', 'ticket', ticketId.value])
const detail = useQuery({ key, query: () => getMyTicket(ticketId.value) })

const reply = ref('')
const sending = ref(false)
const replyFailed = ref(false)

async function sendReply(): Promise<void> {
  if (!reply.value.trim()) return
  sending.value = true
  replyFailed.value = false
  try {
    await replyToMyTicket(ticketId.value, reply.value.trim())
    reply.value = ''
    await queryCache.invalidateQueries({ key: key.value })
    void queryCache.invalidateQueries({ key: ['portal', 'tickets'] })
  } catch {
    replyFailed.value = true
  } finally {
    sending.value = false
  }
}
</script>

<template>
  <PortalLayout>
    <RouterLink to="/tickets" class="self-start inline-flex items-center gap-1 text-sm text-secondary hover:text-primary">
      <Icon name="chevronLeft" />
      {{ t('portal-back-to-requests') }}
    </RouterLink>

    <p v-if="detail.error.value && !detail.data.value" class="text-sm text-status-error">
      {{ t('portal-request-load-failed') }}
    </p>

    <template v-else-if="detail.data.value">
      <div class="flex flex-col gap-2">
        <div class="flex items-start gap-3">
          <h1 class="text-xl font-semibold text-primary flex-1 min-w-0">{{ detail.data.value.ticket.title }}</h1>
          <StatusPill
            v-if="detail.data.value.ticket.state"
            size="sm"
            :label="detail.data.value.ticket.state.name"
            :tone="stateTone(detail.data.value.ticket.state.category)"
          />
        </div>
        <p class="text-xs text-tertiary">
          {{ t('portal-request-number', { id: detail.data.value.ticket.id }) }} ·
          {{ t('portal-opened', { when: formatRelativeTime(detail.data.value.ticket.created) }) }}
        </p>
      </div>

      <ol class="flex flex-col gap-3">
        <li
          v-for="comment in detail.data.value.comments"
          :key="comment.id"
          class="bg-surface border rounded-xl p-4 flex flex-col gap-2"
          :class="comment.author.is_staff ? 'border-accent/40' : 'border-default'"
        >
          <div class="flex items-center gap-2 text-sm">
            <span class="font-medium text-primary">
              {{ comment.author.is_you ? t('portal-thread-you') : comment.author.name }}
            </span>
            <span v-if="comment.author.is_staff" class="text-xs text-accent">{{ t('portal-thread-staff') }}</span>
            <time class="ml-auto text-xs text-tertiary" :datetime="comment.created_at">
              {{ formatRelativeTime(comment.created_at) }}
            </time>
          </div>
          <CommentContent
            :content="comment.content"
            :content-format="comment.content_format"
            :render-kind="comment.render_kind"
            :new-content="comment.new_content"
            :quoted-content="comment.quoted_content"
          />
          <ul v-if="comment.attachments.length" class="flex flex-wrap gap-2" :aria-label="t('portal-attachments')">
            <li v-for="file in comment.attachments" :key="file.id">
              <a
                :href="attachmentUrl(ticketId, file.id)"
                class="inline-flex items-center gap-1.5 text-xs px-2 py-1 rounded-md bg-surface-alt border border-default text-secondary hover:text-primary"
              >
                <Icon name="paperclip" />
                {{ file.name }}
              </a>
            </li>
          </ul>
        </li>
      </ol>

      <form class="flex flex-col gap-3 bg-surface border border-default rounded-xl p-4" @submit.prevent="sendReply">
        <FormTextarea
          v-model="reply"
          :label="t('portal-reply-label')"
          :placeholder="t('portal-reply-placeholder')"
          :rows="4"
          resize="vertical"
          :disabled="sending"
        />
        <p v-if="replyFailed" role="alert" class="text-sm text-status-error">{{ t('portal-reply-failed') }}</p>
        <Button type="submit" class="self-end" icon="send" :loading="sending" :disabled="!reply.trim()">
          {{ t('portal-reply-send') }}
        </Button>
      </form>
    </template>
  </PortalLayout>
</template>
