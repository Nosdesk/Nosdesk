<script setup lang="ts">
/**
 * Where a request stands, as the steps it goes through: sent, approved (only
 * when an approval is involved), being worked on, resolved. The step it's on
 * is marked; a declined approval or a closed request ends the track there.
 */
import { computed } from 'vue'
import { useFluent } from 'fluent-vue'

import Icon from '@/components/common/Icon.vue'

import type { PortalTicket } from '../service'

const props = defineProps<{ ticket: PortalTicket }>()
const { $t: t } = useFluent()

type StepState = 'done' | 'current' | 'upcoming' | 'stopped'
interface Step {
  key: string
  label: string
  state: StepState
}

const steps = computed<Step[]>(() => {
  const category = props.ticket.state?.category
  const approval = props.ticket.approval_state
  const finished = category === 'done'
  const cancelled = category === 'cancelled'
  const working = category === 'active' || category === 'in_review'
  const out: Step[] = [{ key: 'sent', label: t('portal-track-sent'), state: 'done' }]

  if (approval) {
    const decided = approval === 'approved' || approval === 'skipped'
    out.push({
      key: 'approval',
      label:
        approval === 'declined'
          ? t('portal-track-declined')
          : decided
            ? t('portal-track-approved')
            : t('portal-track-approval'),
      state: approval === 'declined' ? 'stopped' : decided ? 'done' : 'current',
    })
    if (approval === 'declined' || approval === 'pending') {
      out.push({ key: 'work', label: t('portal-track-working'), state: 'upcoming' })
      out.push({ key: 'resolved', label: t('portal-track-resolved'), state: 'upcoming' })
      return out.slice(0, approval === 'declined' ? 2 : 4)
    }
  }

  if (cancelled) {
    out.push({ key: 'closed', label: t('portal-track-closed'), state: 'stopped' })
    return out
  }
  if (category === 'merged') {
    out.push({ key: 'merged', label: t('portal-track-merged'), state: 'stopped' })
    return out
  }
  // Waiting in the queue: the team has it but nobody has started.
  if (!working && !finished) out[out.length - 1].state = 'current'
  out.push({
    key: 'work',
    label: working && props.ticket.state ? props.ticket.state.name : t('portal-track-working'),
    state: finished ? 'done' : working ? 'current' : 'upcoming',
  })
  out.push({
    key: 'resolved',
    label: t('portal-track-resolved'),
    state: finished ? 'current' : 'upcoming',
  })
  return out
})

const summary = computed(() => {
  const current = steps.value.find((s) => s.state === 'current' || s.state === 'stopped')
  return current ? current.label : ''
})
</script>

<template>
  <ol class="flex items-start w-full" :aria-label="t('portal-track-label', { step: summary })">
    <li
      v-for="(step, index) in steps"
      :key="step.key"
      class="flex-1 flex flex-col items-center gap-2 min-w-0 relative"
      :aria-current="step.state === 'current' ? 'step' : undefined"
    >
      <!-- The line into this step, drawn behind the marker. -->
      <span
        v-if="index > 0"
        class="absolute top-[11px] h-0.5 rounded-full"
        :class="step.state === 'upcoming' ? 'bg-[var(--color-border-default)]' : step.state === 'stopped' ? 'bg-status-error' : 'bg-accent'"
        style="left: calc(-50% + 18px); right: calc(50% + 18px)"
        aria-hidden="true"
      />
      <span
        class="relative z-[1] w-6 h-6 rounded-full flex items-center justify-center shrink-0"
        :class="{
          'bg-accent text-white': step.state === 'done',
          'bg-app ring-2 ring-accent': step.state === 'current',
          'bg-app border-2 border-default': step.state === 'upcoming',
          'bg-status-error text-white': step.state === 'stopped',
        }"
        aria-hidden="true"
      >
        <Icon v-if="step.state === 'done'" name="check" size="xs" />
        <Icon v-else-if="step.state === 'stopped'" name="close" size="xs" />
        <span v-else-if="step.state === 'current'" class="w-2 h-2 rounded-full bg-accent" />
      </span>
      <span
        class="text-xs text-center leading-snug px-1 max-w-full"
        :class="step.state === 'current' || step.state === 'stopped' ? 'text-primary font-medium' : step.state === 'done' ? 'text-secondary' : 'text-tertiary'"
      >
        {{ step.label }}
      </span>
    </li>
  </ol>
</template>
