<script setup lang="ts">
/**
 * Known-issue notices: the one live now, anything scheduled, and recent past
 * ones. Most notices are posted from the incident ticket itself; this page is
 * for general ones ("Wi-Fi maintenance on Saturday") and for tidying up.
 */
import { computed, ref } from 'vue'
import { RouterLink } from 'vue-router'
import { useQuery, useQueryCache } from '@pinia/colada'
import { useFluent } from 'fluent-vue'

import Button from '@/components/common/Button.vue'
import NoticeDialog from '@/components/notices/NoticeDialog.vue'
import { formatRelativeTime } from '@nosdesk/core/utils/dateUtils'
import { isLive, noticeService, type Notice } from '@nosdesk/core/services/noticeService'
import { numberForTicketId } from '@/composables/useTicketNumberLookup'
import { ticketPath } from '@/utils/ticketNumbers'

const { $t: t } = useFluent()
const queryCache = useQueryCache()
const KEY = ['notices']
const notices = useQuery({ key: KEY, query: () => noticeService.list() })

const dialogOpen = ref(false)
const editing = ref<Notice | null>(null)

function status(n: Notice): 'live' | 'scheduled' | 'ended' {
  if (isLive(n)) return 'live'
  return Date.parse(n.starts_at) > Date.now() ? 'scheduled' : 'ended'
}
const rows = computed(() => (notices.data.value ?? []).map((n) => ({ n, status: status(n) })))

function open(n: Notice | null): void {
  editing.value = n
  dialogOpen.value = true
}
function saved(): void {
  dialogOpen.value = false
  void queryCache.invalidateQueries({ key: KEY })
}
</script>

<template>
  <div class="flex-1">
    <div class="flex flex-col gap-6 px-4 sm:px-6 py-4 mx-auto w-full max-w-4xl">
      <div class="flex items-start gap-4">
        <div class="flex flex-col gap-2 flex-1">
          <h1 class="text-xl sm:text-2xl font-bold text-primary">{{ t('notices-title') }}</h1>
          <p class="text-secondary">{{ t('notices-description') }}</p>
        </div>
        <Button icon="add" @click="open(null)">{{ t('notices-new') }}</Button>
      </div>

      <p v-if="notices.error.value && !notices.data.value" class="text-sm text-status-error">
        {{ t('notices-load-failed') }}
      </p>
      <p v-else-if="notices.data.value && !rows.length" class="text-sm text-secondary">
        {{ t('notices-empty') }}
      </p>
      <ul v-else class="flex flex-col divide-y divide-default bg-surface border border-default rounded-xl">
        <li v-for="{ n, status: s } in rows" :key="n.id" class="flex items-center gap-4 px-4 py-3">
          <div class="flex flex-col gap-0.5 min-w-0 flex-1">
            <span class="text-sm font-medium text-primary truncate">{{ n.title }}</span>
            <span class="text-xs text-secondary">
              {{ t(`notices-status-${s}`) }} ·
              {{ s === 'ended' ? t('notices-ended', { when: formatRelativeTime(n.ends_at) }) : t('notices-ends', { when: formatRelativeTime(n.ends_at) }) }}
              <template v-if="n.incident_ticket_id && numberForTicketId(n.incident_ticket_id) !== undefined">
                ·
                <RouterLink
                  :to="ticketPath({ id: n.incident_ticket_id, number: numberForTicketId(n.incident_ticket_id) })"
                  class="text-accent hover:underline"
                >
                  #{{ numberForTicketId(n.incident_ticket_id) }}
                </RouterLink>
              </template>
            </span>
          </div>
          <Button v-if="s !== 'ended'" variant="secondary" size="sm" @click="open(n)">{{ t('notices-edit') }}</Button>
        </li>
      </ul>
    </div>

    <NoticeDialog :show="dialogOpen" :notice="editing" @close="dialogOpen = false" @saved="saved" />
  </div>
</template>
