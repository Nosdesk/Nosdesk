<script setup lang="ts">
/**
 * Cloud trial reminder at the top of the content, above the view. Shown to
 * the workspace owner only, from seven days out (info), three days out
 * (warning) or once the trial is set not to continue (neutral). Renders
 * nothing at all otherwise, so the content never shifts for anyone else.
 *
 * `role="status"` announces it without taking focus. Dismissing it moves
 * focus to the page heading, since the control under focus is gone.
 */
import { computed, ref } from 'vue'
import { useFluent } from 'fluent-vue'
import Button from '@/components/common/Button.vue'
import IconButton from '@/components/common/IconButton.vue'
import Icon from '@/components/common/Icon.vue'
import { useTrialNotice } from '@/composables/useTrialNotice'
import { bannerCopy, bannerTone, isDismissed, recordDismissal } from '@/utils/trialNotice'
import { openControlPlaneBilling } from '@/services/nosdeskAccount'

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const { notice, scope } = useTrialNotice()

// Storage is not reactive; bumping this re-reads it after a dismissal.
const dismissals = ref(0)

const tone = computed(() => (notice.value ? bannerTone(notice.value) : null))

const visible = computed(() => {
  void dismissals.value
  const n = notice.value
  if (!n || !tone.value || !scope.value.workspace || !scope.value.user) return false
  return !isDismissed(n, scope.value.workspace, scope.value.user)
})

const copy = computed(() => (notice.value ? bannerCopy(notice.value, t) : null))

const frameClass = computed(() =>
  tone.value === 'warning' ? 'border-status-warning-border' : 'border-default',
)
const stripClass = computed(() =>
  tone.value === 'warning' ? 'bg-status-warning' : tone.value === 'info' ? 'bg-accent' : 'bg-strong',
)
const iconClass = computed(() =>
  tone.value === 'warning' ? 'text-status-warning' : tone.value === 'info' ? 'text-accent' : 'text-tertiary',
)

/** The page heading, or the main landmark when the header has no plain
 *  heading (an editable title). Made programmatically focusable only. */
function focusAfterDismiss() {
  const target =
    document.querySelector<HTMLElement>('header h1') ?? document.querySelector<HTMLElement>('main')
  if (!target) return
  if (!target.hasAttribute('tabindex')) target.setAttribute('tabindex', '-1')
  target.focus({ preventScroll: true })
}

function dismiss() {
  const n = notice.value
  if (!n) return
  recordDismissal(n, scope.value.workspace, scope.value.user)
  dismissals.value++
  focusAfterDismiss()
}

function manageBilling() {
  void openControlPlaneBilling()
}
</script>

<template>
  <div v-if="visible && copy" data-test="trial-reminder" class="flex-shrink-0 px-4 pt-4 sm:px-6 print:hidden">
    <section
      role="status"
      aria-labelledby="trial-reminder-title"
      :data-tone="tone"
      class="relative flex items-start gap-3 overflow-hidden rounded-xl border bg-surface py-3 pr-3 pl-4"
      :class="frameClass"
    >
      <span aria-hidden="true" class="absolute inset-y-0 left-0 w-1" :class="stripClass" />
      <Icon
        :name="tone === 'warning' ? 'warning' : 'info'"
        size="md"
        class="mt-px flex-shrink-0"
        :class="iconClass"
      />
      <div class="flex min-w-0 flex-1 flex-col gap-3 sm:flex-row sm:items-start">
        <p class="min-w-0 flex-1 pt-px text-sm text-secondary">
          <strong id="trial-reminder-title" class="font-semibold text-primary">{{ copy.title }}</strong> {{ copy.body }}
        </p>
        <div class="flex flex-shrink-0 items-center gap-1">
          <Button
            :variant="tone === 'warning' ? 'primary' : 'secondary'"
            size="sm"
            :aria-label="$t('trial-notice-manage-billing-aria')"
            :title="$t('trial-notice-opens-account')"
            data-test="trial-reminder-billing"
            @click="manageBilling"
          >
            <span class="inline-flex items-center gap-1.5">
              {{ $t('trial-notice-manage-billing') }}
              <Icon name="openExternal" size="xs" />
            </span>
          </Button>
          <IconButton
            :label="$t('trial-notice-dismiss')"
            icon="close"
            size="sm"
            :tooltip="false"
            data-test="trial-reminder-dismiss"
            @click="dismiss"
          />
        </div>
      </div>
    </section>
  </div>
</template>
