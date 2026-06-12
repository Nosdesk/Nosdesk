<!--
Segmented-pill control — a small group of mutually-exclusive options
rendered as connected pills, for choosing one value from a short set
(group-by axis, view mode, etc.). The on-brand alternative to a
dropdown when there are only a handful of options.

Visual + interaction match the asset / tickets tab strips
(AssetViewTabs, TicketsViewTabs): a rounded `bg-surface-alt` track with
the active pill on `bg-surface shadow-sm`. `role="radiogroup"` with
`aria-checked` on each option; left/right (and up/down) arrows move the
selection, matching native radio semantics.

  <SegmentedControl v-model="axis" :options="axisOptions" aria-label="Group by" />
-->
<script setup lang="ts" generic="T extends string">
const props = withDefaults(
  defineProps<{
    modelValue: T
    options: { value: T; label: string }[]
    /** Accessible name for the group (e.g. "Group by"). */
    ariaLabel?: string
    /** `sm` tightens the pill height for dense toolbars. */
    size?: 'sm' | 'md'
  }>(),
  { ariaLabel: undefined, size: 'md' },
)

const emit = defineEmits<{
  (e: 'update:modelValue', value: T): void
}>()

function select(value: T): void {
  if (value !== props.modelValue) emit('update:modelValue', value)
}

// Arrow keys cycle the selection like a native radio group.
function onKeydown(event: KeyboardEvent): void {
  const { key } = event
  if (key !== 'ArrowLeft' && key !== 'ArrowRight' && key !== 'ArrowUp' && key !== 'ArrowDown') {
    return
  }
  event.preventDefault()
  const idx = props.options.findIndex((o) => o.value === props.modelValue)
  if (idx === -1) return
  const dir = key === 'ArrowRight' || key === 'ArrowDown' ? 1 : -1
  const next = props.options[(idx + dir + props.options.length) % props.options.length]
  select(next.value)
}
</script>

<template>
  <div
    role="radiogroup"
    :aria-label="ariaLabel"
    class="inline-flex items-center gap-0.5 rounded-md bg-surface-alt p-0.5"
    @keydown="onKeydown"
  >
    <button
      v-for="opt in options"
      :key="opt.value"
      type="button"
      role="radio"
      :aria-checked="opt.value === modelValue"
      :tabindex="opt.value === modelValue ? 0 : -1"
      class="inline-flex items-center justify-center rounded font-medium whitespace-nowrap shrink-0 transition-colors focus:outline-none focus-visible:ring-1 focus-visible:ring-accent"
      :class="[
        size === 'sm' ? 'h-6 px-2 text-xs' : 'h-7 px-2.5 text-sm',
        opt.value === modelValue
          ? 'bg-surface text-primary shadow-sm'
          : 'text-secondary hover:text-primary hover:bg-surface/60',
      ]"
      @click="select(opt.value)"
    >
      {{ opt.label }}
    </button>
  </div>
</template>
