<!--
  DocumentAuthorBadge: inline metadata + verification surface.

  Lives in the document metadata strip in place of the bare
  "Created by X" text. The trust signal (verified / stale) folds
  into the same affordance as a small glyph next to the author
  name; clicking the badge opens a popover with:
    - authoring metadata (created date, creator),
    - verification state (last verified, by whom),
    - actions: verify, re-verify with an interval cadence, clear.

  Three visual states drive the inline glyph:
    - never verified: no glyph (the badge looks like a normal name)
    - verified, fresh: small green check
    - verified, stale: small amber warning

  Verification mutations go through the Pinia Colada composables
  so the page-detail cache invalidation is shared with anywhere
  else in the app that reads verification state.
-->
<script setup lang="ts">
import { computed, ref } from 'vue'
import Icon from '@/components/common/Icon.vue'
import Popover from '@/components/common/Popover.vue'
import { formatDate, formatRelativeTime } from '@/utils/dateUtils'
import {
  useVerifyPageMutation,
  useUnverifyPageMutation,
} from '@/composables/usePageVerification'
import type { Page } from '@/services/documentationService'

const props = defineProps<{
  page: Page
  /** Hide the action buttons in the popover (read-only mode). */
  canVerify?: boolean
}>()

const emit = defineEmits<{
  (e: 'changed'): void
}>()

const verifyMutation = useVerifyPageMutation()
const unverifyMutation = useUnverifyPageMutation()

const triggerRef = ref<HTMLElement | null>(null)
/** Open state is two-way bindable so a sibling element (e.g.
 *  a "needs verification" chip in the document status row) can
 *  open the same popover anchored against this badge. The badge
 *  remains the canonical anchor; only the trigger location
 *  varies. */
const isOpen = defineModel<boolean>('open', { default: false })

/** Compact labels keep the cadence row to a single line in a
 *  w-72 popover. Short forms (30d / 1y / Never) read clearly in
 *  context: the row is preceded by "Re-verify every:" so the unit
 *  is unambiguous. */
const INTERVAL_OPTIONS: { label: string; value: number | null }[] = [
  { label: '30d', value: 30 },
  { label: '90d', value: 90 },
  { label: '180d', value: 180 },
  { label: '1y', value: 365 },
  { label: 'Never', value: null },
]

const state = computed<'never' | 'fresh' | 'stale'>(() => {
  if (!props.page.verified_at) return 'never'
  return props.page.is_stale ? 'stale' : 'fresh'
})

const authorName = computed(() => props.page.created_by?.name ?? 'Unknown')
const verifierName = computed(() => props.page.verified_by?.name ?? 'Someone')

const isWorking = computed(
  () =>
    verifyMutation.asyncStatus.value === 'loading' ||
    unverifyMutation.asyncStatus.value === 'loading',
)

async function verify(intervalDays: number | null) {
  await verifyMutation.mutateAsync({ pageId: props.page.id, intervalDays })
  emit('changed')
}

async function unverify() {
  await unverifyMutation.mutateAsync({ pageId: props.page.id })
  emit('changed')
}
</script>

<template>
  <button
    ref="triggerRef"
    type="button"
    class="inline-flex items-center gap-1 rounded px-1 -mx-1 hover:bg-surface-hover transition-colors text-secondary"
    :class="{ 'text-emerald-700 dark:text-emerald-400': state === 'fresh' }"
    :title="`Authored by ${authorName}${state === 'fresh' ? ` · verified ${formatRelativeTime(page.verified_at!)}` : ''}`"
    @click="isOpen = !isOpen"
  >
    <span>{{ authorName }}</span>
    <!--
      Inline glyph confirms the *fresh* state quietly next to the
      author name. The needs-attention signals (never verified,
      stale) live as a separate chip in the document status row so
      the alert reads as being about the document, not the author.
    -->
    <Icon
      v-if="state === 'fresh'"
      name="check"
      size="xs"
      class="text-emerald-600 dark:text-emerald-400"
    />
  </button>

  <Popover
    :open="isOpen"
    :anchor="{ type: 'element', element: () => triggerRef }"
    placement="bottom-start"
    :offset="6"
    role="dialog"
    aria-label="Document author and verification"
    popover-class="w-72 rounded-lg border border-default bg-surface shadow-lg p-3 text-xs"
    @close="isOpen = false"
  >
    <!-- Authoring -->
    <section class="flex flex-col gap-1.5">
      <div class="flex items-baseline justify-between gap-2">
        <span class="text-tertiary">Created</span>
        <span class="text-secondary text-right">
          {{ page.created_at ? formatDate(page.created_at) : '—' }}
        </span>
      </div>
      <div class="flex items-baseline justify-between gap-2">
        <span class="text-tertiary">Author</span>
        <span class="text-primary font-medium text-right truncate">
          {{ authorName }}
        </span>
      </div>
      <div
        v-if="page.last_edited_by && page.last_edited_by.uuid !== page.created_by?.uuid"
        class="flex items-baseline justify-between gap-2"
      >
        <span class="text-tertiary">Last edited by</span>
        <span class="text-secondary text-right truncate">
          {{ page.last_edited_by.name }}
        </span>
      </div>
    </section>

    <!-- Verification -->
    <section class="mt-3 pt-3 border-t border-subtle flex flex-col gap-1.5">
      <div class="flex items-center justify-between gap-2">
        <span class="text-tertiary">Verification</span>
        <span
          v-if="state === 'fresh'"
          class="inline-flex items-center gap-1 text-[10px] font-medium uppercase tracking-wide text-emerald-700 dark:text-emerald-400"
        >
          <Icon name="check" size="xs" />
          Verified
        </span>
        <span
          v-else-if="state === 'stale'"
          class="inline-flex items-center gap-1 text-[10px] font-medium uppercase tracking-wide text-amber-700 dark:text-amber-400"
        >
          <Icon name="warning" size="xs" />
          Stale
        </span>
        <span v-else class="text-[10px] uppercase tracking-wide text-tertiary">
          Not verified
        </span>
      </div>

      <div v-if="page.verified_at" class="flex items-baseline justify-between gap-2">
        <span class="text-tertiary">Last verified</span>
        <span class="text-secondary text-right truncate">
          {{ verifierName }} &middot; {{ formatRelativeTime(page.verified_at) }}
        </span>
      </div>

      <!-- Interval picker / actions. Always visible when canVerify
           so the cadence is obvious and one click away — no
           "first-verify-then-customise" two-step. The active
           cadence is highlighted in the chip row below, so a
           separate "Re-verify every X days" line would just be
           saying the same thing twice. -->
      <div v-if="canVerify" class="mt-2 flex flex-col gap-1.5">
        <p class="text-tertiary">
          {{ state === 'never' ? 'Mark as verified, re-verify every:' : 'Re-verify, every:' }}
        </p>
        <div class="flex flex-wrap gap-1">
          <button
            v-for="opt in INTERVAL_OPTIONS"
            :key="String(opt.value)"
            type="button"
            :disabled="isWorking"
            class="px-2 py-0.5 rounded-md border border-default bg-surface-alt text-secondary hover:text-primary hover:bg-surface-hover transition-colors disabled:opacity-50"
            :class="{
              'ring-1 ring-accent text-primary': page.verify_interval_days === opt.value,
            }"
            @click="verify(opt.value)"
          >
            {{ opt.label }}
          </button>
        </div>
        <button
          v-if="state !== 'never'"
          type="button"
          :disabled="isWorking"
          class="self-start mt-1 text-tertiary hover:text-status-error transition-colors"
          @click="unverify"
        >
          Clear verification
        </button>
      </div>
    </section>
  </Popover>
</template>
