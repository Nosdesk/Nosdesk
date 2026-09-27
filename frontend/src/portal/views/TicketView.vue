<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue'
import { RouterLink, useRoute, useRouter } from 'vue-router'
import { useQuery, useQueryCache } from '@pinia/colada'
import { useFluent } from 'fluent-vue'

import Button from '@/components/common/Button.vue'
import FormTextarea from '@/components/common/FormTextarea.vue'
import Icon from '@/components/common/Icon.vue'
import StatusPill from '@/components/common/StatusPill.vue'
import CommentContent from '@/components/ticketComponents/CommentContent.vue'
import { formatRelativeTime } from '@nosdesk/core/utils/dateUtils'

import AttachmentPicker from '../components/AttachmentPicker.vue'
import PortalLayout from '../components/PortalLayout.vue'
import ParticipantsCard from '../components/ParticipantsCard.vue'
import ResolutionCard from '../components/ResolutionCard.vue'
import { stateTone } from '../stateTone'
import { attachmentUrl, getMyTicket, isClosed, replyToMyTicket, type PortalAttachment } from '../service'

const props = defineProps<{ id: string }>()
const { $t: t } = useFluent()
const queryCache = useQueryCache()

const ticketId = computed(() => Number(props.id))
const key = computed(() => ['portal', 'ticket', ticketId.value])
const detail = useQuery({ key, query: () => getMyTicket(ticketId.value) })

// An answer from the resolved email, taken once and cleared from the URL so a
// reload doesn't answer again.
const route = useRoute()
const router = useRouter()
const answer = typeof route.query.answer === 'string' ? route.query.answer : null
if (answer) void router.replace({ query: {} })

// "No, I still need help": the reply box asks what's wrong and the reply
// records that answer.
const stillNeedsHelp = ref(false)
async function askWhatsWrong(): Promise<void> {
  stillNeedsHelp.value = true
  await nextTick()
  document.getElementById('portal-reply')?.focus()
}

// A reply that arrives live: count it in the tab title while the tab is
// hidden, and offer a jump instead of scrolling the reader away from where they
// are.
const commentCount = computed(() => detail.data.value?.comments.length ?? 0)
const unread = ref(0)
const showNewReply = ref(false)
const baseTitle = document.title
watch(commentCount, (count, previous) => {
  if (!previous || count <= previous) return
  if (document.visibilityState === 'hidden') {
    unread.value += count - previous
    document.title = `(${unread.value}) ${baseTitle}`
  }
  const nearBottom =
    window.innerHeight + window.scrollY >= document.documentElement.scrollHeight - 240
  if (!nearBottom) showNewReply.value = true
})
function onVisible(): void {
  if (document.visibilityState !== 'visible') return
  unread.value = 0
  document.title = baseTitle
}
document.addEventListener('visibilitychange', onVisible)
onBeforeUnmount(() => {
  document.removeEventListener('visibilitychange', onVisible)
  document.title = baseTitle
})
function jumpToLatest(): void {
  showNewReply.value = false
  document.getElementById('portal-thread-end')?.scrollIntoView({ behavior: 'smooth', block: 'end' })
}

async function refresh(): Promise<void> {
  await queryCache.invalidateQueries({ key: key.value })
  void queryCache.invalidateQueries({ key: ['portal', 'tickets'] })
}

const reply = ref('')
const files = ref<PortalAttachment[]>([])
const sending = ref(false)
const replyFailed = ref(false)

async function sendReply(): Promise<void> {
  if (!reply.value.trim() && !files.value.length) return
  sending.value = true
  replyFailed.value = false
  try {
    await replyToMyTicket(
      ticketId.value,
      reply.value.trim(),
      files.value.map((f) => f.id),
      stillNeedsHelp.value,
    )
    reply.value = ''
    files.value = []
    stillNeedsHelp.value = false
    await refresh()
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
      <span id="portal-thread-end" />
      <Button
        v-if="showNewReply"
        icon="chevronDown"
        class="fixed bottom-6 left-1/2 -translate-x-1/2 z-10 shadow-lg"
        @click="jumpToLatest"
      >
        {{ t('portal-new-reply') }}
      </Button>

      <ParticipantsCard
        :ticket-id="ticketId"
        :participants="detail.data.value.participants"
        :is-requester="detail.data.value.is_requester"
        @changed="refresh"
      />

      <p v-if="!detail.data.value.can_reply" class="text-sm text-secondary bg-surface border border-default rounded-xl p-4">
        {{ t('portal-shared-read-only', { name: detail.data.value.ticket.requested_by ?? '' }) }}
      </p>

      <ResolutionCard
        v-if="detail.data.value.is_requester && !stillNeedsHelp"
        :ticket="detail.data.value.ticket"
        :rating="detail.data.value.rating"
        :answer="answer"
        @changed="refresh"
        @still-needs-help="askWhatsWrong"
      />

      <form
        v-if="detail.data.value.can_reply"
        class="flex flex-col gap-3 bg-surface border border-default rounded-xl p-4"
        @submit.prevent="sendReply"
      >
        <FormTextarea
          id="portal-reply"
          v-model="reply"
          :label="stillNeedsHelp ? t('portal-still-label') : t('portal-reply-label')"
          :placeholder="stillNeedsHelp ? t('portal-still-placeholder') : t('portal-reply-placeholder')"
          :rows="4"
          resize="vertical"
          :disabled="sending"
        />
        <p v-if="isClosed(detail.data.value.ticket)" class="text-sm text-secondary">
          {{ t('portal-reply-reopens') }}
        </p>
        <AttachmentPicker v-model="files" :disabled="sending" />
        <p v-if="replyFailed" role="alert" class="text-sm text-status-error">{{ t('portal-reply-failed') }}</p>
        <Button
          type="submit"
          class="self-end"
          icon="send"
          :loading="sending"
          :disabled="!reply.trim() && !files.length"
        >
          {{ t('portal-reply-send') }}
        </Button>
      </form>
    </template>
  </PortalLayout>
</template>
