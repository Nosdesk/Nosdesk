<script setup lang="ts">
/**
 * Settings > Appearance > Text size. One instrument, read top to bottom:
 *
 * 1. The heading, with the size in force as a number on its line.
 * 2. A sample: a ticket row and two lines of a reply, at that size.
 * 3. The slider, directly under what it changes.
 * 4. In the apps, who sets the size: "Use device text size", on by default,
 *    and one sentence that says what the device is doing.
 *
 * The slider is never disabled. While the device sets the size the thumb is
 * hollow and sits where the device has put the text; moving it turns the
 * switch off, in view, and the size becomes the person's own. Turning the
 * switch back on returns to the device. So there is no dead control and no
 * third "reset". A browser has no device size to follow, so there the
 * setting is the slider and its sentence.
 *
 * Bold Text is not a setting here. The app follows the device's, and the
 * sentence mentions it only while it is on.
 */
import { computed, useId } from 'vue'
import { useFluent } from 'fluent-vue'
import { SwitchRoot, SwitchThumb } from 'reka-ui'
import TicketsCardRows from '@/components/views/TicketsCardRows.vue'
import TextSizeSlider from '@/components/settings/TextSizeSlider.vue'
import { deviceText } from '@/platform/deviceText'
import {
  canFollowDevice,
  chooseTextSize,
  followsDevice,
  setFollowDeviceText,
  textSizePercent,
  textSizePlatform,
  textSizeStops,
} from '@/platform/textSize'
import { nearestStop } from '@/utils/textScale'
import { todayIn } from '@nosdesk/core/utils/ticketDates'
import type { CardData } from '@nosdesk/core/sync/views/types'

const fluent = useFluent()
const id = useId()

const platform = textSizePlatform()
const showSwitch = canFollowDevice()
const percent = textSizePercent
const stops = textSizeStops()
const stop = computed(() => Math.min(nearestStop(percent.value), stops.length - 1))
const bold = computed(() => platform !== 'web' && !!deviceText.value?.bold)

/** What sets the size, in one sentence. */
const status = computed(() => {
  const key = !followsDevice.value
    ? showSwitch || percent.value !== 100
      ? 'settings-text-size-own'
      : 'settings-text-size-device-default'
    : percent.value >= 200
      ? 'settings-text-size-device-capped'
      : percent.value > 100
        ? 'settings-text-size-device-larger'
        : 'settings-text-size-device-default'
  return bold.value ? `${key}-bold` : key
})

const today = todayIn()
const minutesAgo = (minutes: number) => new Date(Date.now() - minutes * 60_000).toISOString()
/** The sample row: shown, not pressed. */
const sample = computed<CardData>(() => ({
  id: -1,
  number: 4792,
  title: fluent.$t('settings-text-size-sample-title'),
  workflow_state: { id: -1, name: fluent.$t('settings-text-size-sample-state'), category: 'active', color: 'blue' },
  priority: 'high',
  due_date: `${today}T00:00:00`,
  due_time: '14:30:00',
  estimate_minutes: 120,
  created_at: minutesAgo(60 * 24 * 3),
  updated_at: minutesAgo(38),
  last_activity_at: minutesAgo(38),
}) as CardData)
</script>

<template>
  <section class="flex flex-col gap-3" data-text-size>
    <div class="flex items-baseline justify-between gap-3">
      <h3 class="text-sm font-medium text-primary">{{ $t('settings-text-size-heading') }}</h3>
      <span class="text-sm whitespace-nowrap text-secondary tabular-nums" data-text-size-value>
        {{ $t('settings-text-size-value', { percent }) }}
      </span>
    </div>

    <div class="overflow-hidden rounded-lg border border-default bg-surface" aria-hidden="true" inert>
      <TicketsCardRows :cards="[sample]" :today="today" />
      <div class="border-t border-subtle px-4 py-3">
        <p class="line-clamp-2 text-base leading-relaxed text-primary">{{ $t('settings-text-size-sample-reply') }}</p>
      </div>
    </div>

    <TextSizeSlider
      :stops="stops"
      :model-value="stop"
      :following="followsDevice"
      :label="$t('settings-text-size-heading')"
      :value-text="$t('settings-text-size-value', { percent })"
      :described-by="`${id}-status`"
      @update:model-value="(index) => chooseTextSize(stops[index])"
    />

    <div class="flex flex-col gap-1">
      <div v-if="showSwitch" class="flex items-center justify-between gap-4">
        <label :for="id" class="min-w-0 cursor-pointer text-sm font-medium text-primary">
          {{ $t('settings-text-size-device-label') }}
        </label>
        <SwitchRoot
          :id="id"
          :model-value="followsDevice"
          :aria-describedby="`${id}-status`"
          class="relative inline-flex h-6 w-11 shrink-0 cursor-pointer items-center rounded-full border-2 border-strong bg-surface-alt transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-offset-2 focus-visible:ring-offset-surface data-[state=checked]:border-accent data-[state=checked]:bg-accent"
          @update:model-value="setFollowDeviceText"
        >
          <SwitchThumb class="pointer-events-none inline-block h-5 w-5 translate-x-0 rounded-full bg-white shadow-lg transition-transform data-[state=checked]:translate-x-5" />
        </SwitchRoot>
      </div>
      <!-- Under both, at the full width, so at 200% it is not squeezed into
           the column the switch leaves. -->
      <p :id="`${id}-status`" class="text-xs text-tertiary" aria-live="polite" data-text-size-status>
        {{ $t(status, { platform }) }}
      </p>
    </div>
  </section>
</template>
