<script setup lang="ts">
import { ref, watch, nextTick, computed } from 'vue'
import UserAvatar from './UserAvatar.vue'
import { useDataStore } from '@/stores/dataStore'

interface TicketDetails {
  title: string
  requester?: string
  assignee?: string
  requester_avatar?: string | null
  assignee_avatar?: string | null
  status?: string
  created?: string
}

const props = defineProps<{
  text: string
  details?: {
    title?: string
    status?: string
    requester?: string
    assignee?: string
    requester_avatar?: string | null
    assignee_avatar?: string | null
    created?: string
  }
  position?: 'top' | 'bottom' | 'left' | 'right'
  delay?: number
  disabled?: boolean
  fullWidth?: boolean
}>()

const container = ref<HTMLElement | null>(null)
const tooltipTop = ref(0)
const isHovering = ref(false)
const tooltipVisible = ref(false)
const hoverTimer = ref<number | null>(null)
const hideTimer = ref<number | null>(null)

// Use the data store for user name lookups
const dataStore = useDataStore()

// Get user name from UUID
const getDisplayName = (uuid: string | undefined) => {
  if (!uuid) return 'Unassigned'

  const uuidPattern = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i
  if (!uuidPattern.test(uuid)) return uuid

  return dataStore.getUserName(uuid) || uuid
}

// Computed properties for user names
const requesterName = computed(() => getDisplayName(props.details?.requester))
const assigneeName = computed(() => getDisplayName(props.details?.assignee))

watch(isHovering, (newValue) => {
  if (newValue) {
    nextTick(() => {
      updatePosition()
    })
  }
})

const updatePosition = () => {
  if (container.value) {
    const rect = container.value.getBoundingClientRect()
    tooltipTop.value = rect.top + (rect.height / 2)
  }
}

const handleMouseEnter = () => {
  isHovering.value = true
  
  // Clear any existing timers
  if (hoverTimer.value !== null) {
    window.clearTimeout(hoverTimer.value)
    hoverTimer.value = null
  }
  
  if (hideTimer.value !== null) {
    window.clearTimeout(hideTimer.value)
    hideTimer.value = null
  }
  
  // Show tooltip immediately
  tooltipVisible.value = true
  nextTick(() => {
    updatePosition()
  })
}

const handleMouseLeave = () => {
  isHovering.value = false
  
  // Clear any existing hover timer
  if (hoverTimer.value !== null) {
    window.clearTimeout(hoverTimer.value)
    hoverTimer.value = null
  }
  
  // Set a small delay before hiding to prevent flickering
  // when moving between elements quickly
  hideTimer.value = window.setTimeout(() => {
    tooltipVisible.value = false
  }, 50) // Small delay to prevent flickering
}

// Add a watch for tooltipVisible to ensure position is updated when tooltip becomes visible
watch(tooltipVisible, (newValue) => {
  if (newValue) {
    nextTick(() => {
      updatePosition()
    })
  }
})
</script>

<template>
  <div 
    class="relative min-w-0" 
    :class="{ 'flex-1': !fullWidth, 'w-full': fullWidth }"
    ref="container"
    @mouseenter="handleMouseEnter"
    @mouseleave="handleMouseLeave"
  >
    <slot />
    <div
      v-if="!disabled && tooltipVisible"
      class="absolute bg-surface text-primary text-xs px-3 py-2 rounded shadow-lg
             pointer-events-none z-[9999] w-[240px] transition-opacity duration-150"
      :class="{ 'opacity-0': !tooltipVisible, 'opacity-100': tooltipVisible }"
      :style="{
        position: 'fixed',
        left: 'calc(256px + 0.5rem)', // 256px is the navbar width (w-64 = 16rem = 256px)
        top: `${tooltipTop}px`,
        transform: 'translateY(-50%)'
      }"
    >
      <!-- Arrow pointing left -->
      <div
        class="absolute -left-2 top-1/2 -translate-y-1/2 w-0 h-0
               border-t-[6px] border-t-transparent
               border-r-[8px] border-r-surface
               border-b-[6px] border-b-transparent"
      ></div>

      <div class="flex flex-col gap-1">
        <div class="font-medium">{{ text }}</div>
        <div v-if="details" class="text-secondary flex flex-col gap-2 mt-1">
          <div v-if="details.status" class="flex items-center gap-2">
            <span class="text-tertiary">Status:</span>
            <span>{{ details.status }}</span>
          </div>
          <div v-if="details.requester || details.assignee" class="flex flex-col gap-1.5">
            <div v-if="details.requester" class="flex items-center gap-2">
              <UserAvatar :name="details.requester" :avatar="details.requester_avatar" :showName="false" size="xs" />
              <span class="flex flex-row gap-1 truncate">
                <span class="text-tertiary">Requester:</span>
                <span>{{ requesterName }}</span>
              </span>
            </div>
            <div v-if="details.assignee" class="flex items-center gap-2">
              <UserAvatar :name="details.assignee" :avatar="details.assignee_avatar" :showName="false" size="xs" />
              <span class="flex flex-row gap-1 truncate">
                <span class="text-tertiary">Assignee:</span>
                <span>{{ assigneeName }}</span>
              </span>
            </div>
          </div>
          <div v-if="details.created" class="text-[11px] text-tertiary">
            {{ details.created }}
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.group\/tooltip {
  isolation: isolate;
}
</style> 