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

defineProps<{
  title: string
  isCollapsed: boolean
  // Icon type: 'clock' for recent items, 'book' for documentation
  icon?: 'clock' | 'book'
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
    class="flex flex-col overflow-hidden transition-opacity duration-200"
    :class="[isCollapsed ? 'opacity-90 hover:opacity-100' : '']"
    @update:open="emit('toggle')"
  >
    <h3 class="flex-shrink-0 m-0">
      <CollapsibleTrigger as-child>
        <button
          type="button"
          :aria-controls="contentId"
          class="group flex w-full items-center justify-between py-1.5 px-3 cursor-pointer transition-colors duration-200 text-left focus:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-accent"
          :class="isCollapsed ? 'hover:bg-surface-hover' : 'bg-surface-hover/40'"
        >
          <span class="text-xs font-medium text-secondary uppercase tracking-wider flex items-center gap-1.5">
            <!-- Clock icon for recent tickets -->
            <svg v-if="icon === 'clock'" class="w-3 h-3 text-accent" fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden="true">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 8v4l3 3m6-3a9 9 0 11-18 0 9 9 0 0118 0z" />
            </svg>
            <!-- Book icon for documentation -->
            <svg v-else-if="icon === 'book'" class="w-3 h-3 text-accent" fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden="true">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 6.253v13m0-13C10.832 5.477 9.246 5 7.5 5S4.168 5.477 3 6.253v13C4.168 18.477 5.754 18 7.5 18s3.332.477 4.5 1.253m0-13C13.168 5.477 14.754 5 16.5 5c1.747 0 3.332.477 4.5 1.253v13C19.832 18.477 18.247 18 16.5 18c-1.746 0-3.332.477-4.5 1.253" />
            </svg>
            <!-- Default dot fallback -->
            <span v-else class="w-2 h-2 rounded-full bg-accent"></span>
            {{ title }}
          </span>
          <span
            class="text-tertiary group-hover:text-primary transition-colors duration-200 bg-surface-hover rounded p-0.5 inline-flex"
            aria-hidden="true"
          >
            <svg
              xmlns="http://www.w3.org/2000/svg"
              class="h-3 w-3 transition-transform duration-200"
              :class="{ 'rotate-180': !isCollapsed }"
              fill="none"
              viewBox="0 0 24 24"
              stroke="currentColor"
            >
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 9l-7 7-7-7" />
            </svg>
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
