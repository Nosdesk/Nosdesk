<!--
Sidebar section with a disclosure header, on Reka's Collapsible. The
whole header is the trigger (one button, named by the title, with
`aria-expanded` and `aria-controls` from Reka); the old inner icon
button inside a clickable div was a nested control with no accessible
relationship to the panel. Content unmounts while collapsed, as before.

The parent controls the state (`isCollapsed` + `toggle`) and the outer
height; the root element is reachable through the instance's `$el` for
the resizable layout in Navbar.
-->
<script setup lang="ts">
import { useId } from 'vue'
import { CollapsibleContent, CollapsibleRoot, CollapsibleTrigger } from 'reka-ui'
import Icon from '@/components/common/Icon.vue'

defineProps<{
  title: string
  isCollapsed: boolean
}>()

const emit = defineEmits<{
  (e: 'toggle'): void
}>()

// Reka assigns the content id when the content mounts, after the
// trigger has rendered, and the trigger does not re-render for it; an
// id of our own keeps aria-controls right from the first paint.
const contentId = useId()
</script>

<template>
  <CollapsibleRoot
    :open="!isCollapsed"
    class="flex flex-col overflow-hidden"
    @update:open="emit('toggle')"
  >
    <h3 class="flex-shrink-0 m-0">
      <CollapsibleTrigger as-child>
        <button
          type="button"
          :aria-controls="contentId"
          class="group flex w-full h-7 items-center justify-between px-2.5 rounded-md cursor-pointer transition-colors duration-200 text-left hover:bg-surface-hover focus:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-accent"
        >
          <span class="text-3xs font-semibold text-tertiary tracking-wide uppercase select-none group-hover:text-secondary">
            {{ title }}
          </span>
          <span class="text-tertiary group-hover:text-primary transition-colors duration-200 inline-flex" aria-hidden="true">
            <Icon name="chevronRight" size="xs" class="transition-transform duration-200" :class="{ 'rotate-90': !isCollapsed }" />
          </span>
        </button>
      </CollapsibleTrigger>
    </h3>

    <!-- Content wrapper; the parent controls height and flex behaviour. -->
    <CollapsibleContent as-child>
      <div :id="contentId" class="overflow-y-auto bg-surface/60 flex-1 min-h-0">
        <slot />
      </div>
    </CollapsibleContent>
  </CollapsibleRoot>
</template>
