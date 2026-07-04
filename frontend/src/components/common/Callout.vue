<script setup lang="ts">
/**
 * Severity callout. One unified pattern for danger/warning/info/
 * success messages: a neutral card with a coloured 4px left
 * strip plus an optional header section with a soft tinted
 * background and a coloured icon. Heading text stays neutral
 * so the colour signal comes from the strip and the icon, not
 * from a wall of red type.
 *
 * Layout slots:
 *   - `header` (optional)  — title + subtitle area. Rendered
 *                            inside the tinted header bar.
 *   - default              — body content. No padding applied
 *                            so callers can render tables
 *                            edge-to-edge or wrap in their own
 *                            padded container.
 *
 * Props:
 *   - severity (required)  — picks the strip + icon colour.
 *   - icon     (optional)  — overrides the default icon for
 *                            the severity (warning / info /
 *                            check). Pass `null` to suppress.
 */
import { computed, useSlots } from 'vue'
import Icon from '@/components/common/Icon.vue'
import type { IconName } from '@/components/common/icons'

type Severity = 'error' | 'warning' | 'info' | 'success'

const props = withDefaults(
  defineProps<{
    severity: Severity
    icon?: IconName | null
  }>(),
  { icon: undefined },
)

const slots = useSlots()
const hasHeader = computed(() => Boolean(slots.header))

/** Per-severity Tailwind class tables. Centralised so the
 *  strip / tint / icon-tint colours stay in sync; adding a new
 *  severity is one row per table. */
const DEFAULT_ICON: Record<Severity, IconName> = {
  error: 'warning',
  warning: 'warning',
  info: 'info',
  success: 'check',
}
const STRIP_CLASS: Record<Severity, string> = {
  error: 'bg-status-error',
  warning: 'bg-status-warning',
  info: 'bg-accent',
  success: 'bg-status-success',
}
const HEADER_BG_CLASS: Record<Severity, string> = {
  error: 'bg-status-error/10',
  warning: 'bg-status-warning/10',
  info: 'bg-accent/10',
  success: 'bg-status-success/10',
}
const ICON_COLOR_CLASS: Record<Severity, string> = {
  error: 'text-status-error',
  warning: 'text-status-warning',
  info: 'text-accent',
  success: 'text-status-success',
}

const resolvedIcon = computed<IconName | null>(() => {
  if (props.icon === null) return null
  return props.icon ?? DEFAULT_ICON[props.severity]
})

const stripClass = computed(() => STRIP_CLASS[props.severity])
const headerBgClass = computed(() => HEADER_BG_CLASS[props.severity])
const iconColorClass = computed(() => ICON_COLOR_CLASS[props.severity])
</script>

<template>
  <div class="relative bg-surface border border-default rounded-lg overflow-hidden pl-1">
    <span
      aria-hidden="true"
      class="absolute left-0 top-0 bottom-0 w-1"
      :class="stripClass"
    />

    <div
      v-if="hasHeader"
      class="px-4 py-3 flex items-start gap-3"
      :class="headerBgClass"
    >
      <Icon
        v-if="resolvedIcon"
        :name="resolvedIcon"
        size="md"
        :class="iconColorClass"
        class="flex-shrink-0 mt-0.5"
      />
      <div class="flex-1 text-sm">
        <slot name="header" />
      </div>
    </div>

    <slot />
  </div>
</template>
