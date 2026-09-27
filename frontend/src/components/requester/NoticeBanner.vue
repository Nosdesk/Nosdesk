<script setup lang="ts">
/**
 * The team's known-issue notice, on the portal and the public pages. Rendered
 * with the page, so it's a status region rather than an alert (screen readers
 * don't announce alerts already present at load). Dismissing hides this
 * version only: an edit shows it again.
 */
import { computed, ref } from 'vue'
import { useFluent } from 'fluent-vue'

import Button from '@/components/common/Button.vue'
import Icon from '@/components/common/Icon.vue'
import type { PublicNotice } from '@nosdesk/core/services/noticeService'

const props = defineProps<{
  notice: PublicNotice
  /** Portal only: whether the requester follows the issue. */
  following?: boolean
  /** Portal only: offer "Follow this issue". */
  canFollow?: boolean
  busy?: boolean
}>()
const emit = defineEmits<{ follow: [] }>()
const { $t: t } = useFluent()

const key = computed(() => `nosdesk-notice-dismissed:${props.notice.id}:${props.notice.updated_at}`)
function readDismissed(): boolean {
  try {
    return window.localStorage.getItem(key.value) === '1'
  } catch {
    return false
  }
}
const dismissed = ref(readDismissed())
function dismiss(): void {
  dismissed.value = true
  try {
    window.localStorage.setItem(key.value, '1')
  } catch {
    // Private mode: it stays hidden for this page view only.
  }
}

const tone = computed(
  () =>
    ({
      info: 'border-status-info-border bg-status-info-muted',
      degraded: 'border-status-warning-border bg-status-warning-muted',
      outage: 'border-status-error-border bg-status-error-muted',
    })[props.notice.severity],
)
const iconTone = computed(
  () =>
    ({
      info: 'text-status-info',
      degraded: 'text-status-warning',
      outage: 'text-status-error',
    })[props.notice.severity],
)
</script>

<template>
  <section
    v-if="!dismissed"
    role="status"
    class="w-full flex items-start gap-3 rounded-xl border px-4 py-3"
    :class="tone"
  >
    <Icon name="info" class="mt-0.5 shrink-0" :class="iconTone" />
    <div class="flex flex-col gap-1 min-w-0 flex-1">
      <p class="text-sm font-semibold text-primary">{{ notice.title }}</p>
      <p v-if="notice.body" class="text-sm text-secondary whitespace-pre-line">{{ notice.body }}</p>
      <div v-if="canFollow && notice.followable" class="pt-1">
        <p v-if="following" class="text-sm text-secondary">{{ t('notice-following') }}</p>
        <Button v-else size="sm" variant="secondary" :loading="busy" @click="emit('follow')">
          {{ t('notice-follow') }}
        </Button>
      </div>
    </div>
    <button
      type="button"
      class="shrink-0 rounded-md p-1 text-tertiary hover:text-primary hover:bg-surface-hover"
      :aria-label="t('notice-dismiss')"
      @click="dismiss"
    >
      <Icon name="close" size="sm" />
    </button>
  </section>
</template>
