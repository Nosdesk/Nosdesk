<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue'
import { RouterLink, useRoute, useRouter } from 'vue-router'
import { useQuery, useQueryCache } from '@pinia/colada'
import { useFluent } from 'fluent-vue'

import Button from '@/components/common/Button.vue'
import Icon from '@/components/common/Icon.vue'
import CommentContent from '@/components/ticketComponents/CommentContent.vue'
import TicketStatusIcon from '@/components/TicketStatusIcon.vue'
import { formatDate, formatDateTime, formatRelativeTime } from '@nosdesk/core/utils/dateUtils'
import { ticketRoute } from '@nosdesk/core/utils/ticketRoutes'

import AttachmentPicker from '../components/AttachmentPicker.vue'
import PortalLayout from '../components/PortalLayout.vue'
import ParticipantsCard from '../components/ParticipantsCard.vue'
import PortalAvatar from '../components/PortalAvatar.vue'
import ProgressTrack from '../components/ProgressTrack.vue'
import ResolutionCard from '../components/ResolutionCard.vue'
import {
  attachmentUrl,
  getMe,
  getMyTicket,
  getMyTicketByNumber,
  isClosed,
  isMerged,
  markSeen,
  replyToMyTicket,
  type PortalAttachment,
  type PortalTicket,
} from '../service'

const props = defineProps<{ number: string }>()
const { $t: t } = useFluent()
const queryCache = useQueryCache()

// The URL carries the request's number; reads, writes and live updates go by
// its id. The request list usually knows it; otherwise the server resolves
// the number, and its answer seeds the detail cache under the id.
const ticketNumber = computed(() => Number(props.number))
const resolvedIds = ref(new Map<number, number>())
const notFound = ref(false)
watch(
  ticketNumber,
  async (number) => {
    notFound.value = false
    if (resolvedIds.value.has(number)) return
    const listed = queryCache
      .getQueryData<PortalTicket[]>(['portal', 'tickets'])
      ?.find((t) => t.number === number)
    if (listed) {
      resolvedIds.value.set(number, listed.id)
      return
    }
    try {
      const found = await getMyTicketByNumber(number)
      queryCache.setQueryData(['portal', 'ticket', found.ticket.id], found)
      resolvedIds.value.set(number, found.ticket.id)
    } catch {
      if (ticketNumber.value === number) notFound.value = true
    }
  },
  { immediate: true },
)
const ticketId = computed(() => resolvedIds.value.get(ticketNumber.value) ?? 0)
const key = computed(() => ['portal', 'ticket', ticketId.value])
const detail = useQuery({
  key,
  query: () => getMyTicket(ticketId.value),
  enabled: () => ticketId.value > 0,
})
const me = useQuery({ key: ['portal', 'me'], query: getMe })

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
// Tell the server what the requester has actually looked at (only while the
// page is visible), so an email held for them while they watch is dropped.
function reportSeen(): void {
  if (document.visibilityState === 'visible' && detail.data.value) {
    void markSeen(ticketId.value).catch(() => {})
  }
}
watch(
  () => detail.data.value?.comments.length,
  (count) => {
    if (count !== undefined) reportSeen()
  },
  { immediate: true },
)

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
  reportSeen()
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

// A merged request's conversation continues on the one it was merged into:
// no reply box or "is it fixed?" here, just where it went.
const merged = computed(() => !!detail.data.value && isMerged(detail.data.value.ticket))
const canReply = computed(() => !!detail.data.value?.can_reply && !merged.value)

async function refresh(): Promise<void> {
  await queryCache.invalidateQueries({ key: key.value })
  void queryCache.invalidateQueries({ key: ['portal', 'tickets'] })
}

const reply = ref('')
const files = ref<PortalAttachment[]>([])
const sending = ref(false)
const replyFailed = ref(false)

// Cmd/Ctrl+Enter sends, as in the agent app.
function onReplyKeydown(event: KeyboardEvent): void {
  if (event.key === 'Enter' && (event.metaKey || event.ctrlKey)) {
    event.preventDefault()
    void sendReply()
  }
}

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
  <PortalLayout wide>
    <RouterLink
      to="/tickets"
      class="self-start -mb-2 inline-flex items-center gap-1 text-sm text-secondary hover:text-primary rounded focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
    >
      <Icon name="chevronLeft" />
      {{ t('portal-back-to-requests') }}
    </RouterLink>

    <p v-if="notFound || (detail.error.value && !detail.data.value)" class="text-sm text-status-error">
      {{ t('portal-request-load-failed') }}
    </p>

    <template v-else-if="detail.data.value">
      <header class="flex flex-col gap-2">
        <h1 class="text-2xl font-semibold text-primary text-balance">{{ detail.data.value.ticket.title }}</h1>
        <div class="flex flex-wrap items-center gap-x-4 gap-y-1 text-sm text-secondary">
          <span v-if="detail.data.value.ticket.state" class="inline-flex items-center gap-1.5 text-primary">
            <TicketStatusIcon :category="detail.data.value.ticket.state.category" class="w-4 h-4" />
            {{ detail.data.value.ticket.state.name }}
          </span>
          <span class="tabular-nums">{{ t('portal-request-number', { id: detail.data.value.ticket.number }) }}</span>
          <time :datetime="detail.data.value.ticket.created" :title="formatDateTime(detail.data.value.ticket.created)">
            {{ t('portal-opened', { when: formatRelativeTime(detail.data.value.ticket.created) }) }}
          </time>
        </div>
      </header>

      <section class="flex flex-col gap-3 bg-surface border border-default rounded-xl px-4 sm:px-6 py-5">
        <ProgressTrack :ticket="detail.data.value.ticket" />
        <p
          v-if="detail.data.value.ticket.approval_state === 'pending'"
          class="text-sm text-secondary text-center"
        >
          {{ t('portal-approval-waiting') }}
        </p>
      </section>

      <div class="grid gap-6 lg:grid-cols-[minmax(0,1fr)_18rem] items-start">
        <div class="flex flex-col gap-4 min-w-0">
          <h2 class="sr-only">{{ t('portal-thread-title') }}</h2>
          <ol class="flex flex-col">
            <li
              v-for="(comment, index) in detail.data.value.comments"
              :key="comment.id"
              class="relative flex gap-3 pb-5"
            >
              <!-- The thread's spine, joining one message to the next. -->
              <span
                v-if="index < detail.data.value.comments.length - 1 || canReply"
                class="absolute left-4 top-10 bottom-0 w-px bg-[var(--color-border-default)]"
                aria-hidden="true"
              />
              <PortalAvatar :name="comment.author.name" :src="comment.author.avatar_url" />
              <div class="flex flex-col gap-1.5 flex-1 min-w-0">
                <div class="flex flex-wrap items-baseline gap-x-2 text-sm">
                  <span class="font-medium text-primary">
                    {{ comment.author.is_you ? t('portal-thread-you') : comment.author.name }}
                  </span>
                  <span
                    v-if="comment.author.is_staff"
                    class="text-xs font-medium text-accent"
                  >
                    {{ t('portal-thread-staff') }}
                  </span>
                  <time
                    class="text-xs text-tertiary"
                    :datetime="comment.created_at"
                    :title="formatDateTime(comment.created_at)"
                  >
                    {{ formatRelativeTime(comment.created_at) }}
                  </time>
                </div>
                <div
                  class="rounded-xl px-4 py-3 flex flex-col gap-3 border"
                  :class="comment.author.is_you ? 'bg-accent/5 border-accent/20' : 'bg-surface border-default'"
                >
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
                        class="inline-flex items-center gap-1.5 text-xs px-2.5 py-1.5 rounded-lg bg-app border border-default text-secondary hover:text-primary"
                      >
                        <Icon name="paperclip" />
                        {{ file.name }}
                      </a>
                    </li>
                  </ul>
                </div>
              </div>
            </li>
          </ol>
          <span id="portal-thread-end" />
          <Button
            v-if="showNewReply"
            icon="chevronDown"
            class="fixed bottom-20 sm:bottom-6 left-1/2 -translate-x-1/2 z-10 shadow-lg"
            @click="jumpToLatest"
          >
            {{ t('portal-new-reply') }}
          </Button>

          <p
            v-if="merged"
            class="flex flex-col gap-2 text-sm text-secondary bg-surface border border-default rounded-xl p-4"
          >
            <template v-if="detail.data.value.merged_into">
              {{ t('portal-merged-into', { number: detail.data.value.merged_into }) }}
              <RouterLink
                :to="ticketRoute(detail.data.value.merged_into)"
                class="self-start font-medium text-accent hover:underline rounded focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
              >
                {{ t('portal-merged-open', { number: detail.data.value.merged_into }) }}
              </RouterLink>
            </template>
            <template v-else>
              {{ t('portal-merged-hidden') }}
              <RouterLink
                to="/tickets/new"
                class="self-start font-medium text-accent hover:underline rounded focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
              >
                {{ t('portal-nav-new') }}
              </RouterLink>
            </template>
          </p>

          <p
            v-else-if="!detail.data.value.can_reply"
            class="text-sm text-secondary bg-surface border border-default rounded-xl p-4"
          >
            {{ t('portal-shared-read-only', { name: detail.data.value.ticket.requested_by ?? '' }) }}
          </p>

          <form
            v-else
            class="flex gap-3"
            @submit.prevent="sendReply"
          >
            <PortalAvatar :name="me.data.value?.name ?? ''" />
            <div class="flex flex-col gap-3 flex-1 min-w-0 bg-surface border border-default rounded-xl p-3 focus-within:border-accent/60 transition-colors">
              <label for="portal-reply" class="sr-only">
                {{ stillNeedsHelp ? t('portal-still-label') : t('portal-reply-label') }}
              </label>
              <textarea
                id="portal-reply"
                v-model="reply"
                rows="3"
                class="w-full resize-y bg-transparent text-sm text-primary placeholder:text-tertiary focus:outline-none px-1"
                :placeholder="stillNeedsHelp ? t('portal-still-placeholder') : t('portal-reply-placeholder')"
                :disabled="sending"
                @keydown="onReplyKeydown"
              />
              <p v-if="isClosed(detail.data.value.ticket)" class="text-xs text-secondary px-1">
                {{ t('portal-reply-reopens') }}
              </p>
              <p v-if="replyFailed" role="alert" class="text-sm text-status-error px-1">{{ t('portal-reply-failed') }}</p>
              <div class="flex flex-wrap items-center gap-2">
                <AttachmentPicker v-model="files" compact :disabled="sending" class="flex-1 min-w-0" />
                <Button
                  type="submit"
                  size="sm"
                  icon="send"
                  class="ml-auto"
                  :loading="sending"
                  :disabled="!reply.trim() && !files.length"
                >
                  {{ t('portal-reply-send') }}
                </Button>
              </div>
            </div>
          </form>
        </div>

        <aside class="flex flex-col gap-4 lg:sticky lg:top-20">
          <ResolutionCard
            v-if="detail.data.value.is_requester && !stillNeedsHelp && !merged"
            :ticket="detail.data.value.ticket"
            :rating="detail.data.value.rating"
            :answer="answer"
            @changed="refresh"
            @still-needs-help="askWhatsWrong"
          />
          <ParticipantsCard
            :ticket-id="ticketId"
            :participants="detail.data.value.participants"
            :is-requester="detail.data.value.is_requester"
            @changed="refresh"
          />
          <section class="flex flex-col gap-3 bg-surface border border-default rounded-xl p-4">
            <h2 class="text-sm font-semibold text-primary">{{ t('portal-details-title') }}</h2>
            <dl class="grid grid-cols-[auto_1fr] gap-x-4 gap-y-2 text-sm">
              <dt class="text-secondary">{{ t('portal-details-number') }}</dt>
              <dd class="text-primary tabular-nums text-right">#{{ detail.data.value.ticket.number }}</dd>
              <dt class="text-secondary">{{ t('portal-details-opened') }}</dt>
              <dd class="text-primary text-right">{{ formatDate(detail.data.value.ticket.created) }}</dd>
              <dt class="text-secondary">{{ t('portal-details-updated') }}</dt>
              <dd class="text-primary text-right">{{ formatRelativeTime(detail.data.value.ticket.modified) }}</dd>
              <template v-if="detail.data.value.ticket.requested_by">
                <dt class="text-secondary">{{ t('portal-details-requester') }}</dt>
                <dd class="text-primary text-right truncate">{{ detail.data.value.ticket.requested_by }}</dd>
              </template>
            </dl>
          </section>
        </aside>
      </div>
    </template>
  </PortalLayout>
</template>
