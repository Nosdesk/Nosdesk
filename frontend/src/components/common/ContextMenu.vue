<script setup lang="ts">
import { ref, onMounted, onUnmounted, nextTick, computed } from 'vue'

export interface MenuItem {
  id: string
  label: string
  icon?: string
  danger?: boolean
  divider?: boolean
}

const props = defineProps<{
  items: MenuItem[]
  x: number
  y: number
}>()

const emit = defineEmits<{
  select: [id: string]
  close: []
}>()

const menuRef = ref<HTMLElement | null>(null)
const adjustedX = ref(props.x)
const adjustedY = ref(props.y)

const handleMousedown = (event: MouseEvent) => {
  if (menuRef.value && !menuRef.value.contains(event.target as Node)) {
    emit('close')
  }
}

const handleKeydown = (event: KeyboardEvent) => {
  if (event.key === 'Escape') {
    emit('close')
  }
}

const handleContextMenu = (event: MouseEvent) => {
  event.preventDefault()
}

const handleSelect = (id: string) => {
  emit('select', id)
  emit('close')
}

onMounted(async () => {
  document.addEventListener('mousedown', handleMousedown)
  document.addEventListener('keydown', handleKeydown)
  document.addEventListener('contextmenu', handleContextMenu)

  await nextTick()

  // Adjust position if menu overflows viewport
  if (menuRef.value) {
    const rect = menuRef.value.getBoundingClientRect()
    const vw = window.innerWidth
    const vh = window.innerHeight

    if (props.x + rect.width > vw) {
      adjustedX.value = props.x - rect.width
    }
    if (props.y + rect.height > vh) {
      adjustedY.value = props.y - rect.height
    }
  }

  // Focus the menu for keyboard accessibility
  menuRef.value?.focus()
})

onUnmounted(() => {
  document.removeEventListener('mousedown', handleMousedown)
  document.removeEventListener('keydown', handleKeydown)
  document.removeEventListener('contextmenu', handleContextMenu)
})
</script>

<template>
  <Teleport to="body">
    <div
      ref="menuRef"
      role="menu"
      tabindex="-1"
      class="fixed bg-surface border border-default rounded-lg shadow-lg py-1 z-[100] min-w-[160px] outline-none"
      :style="{ left: `${adjustedX}px`, top: `${adjustedY}px` }"
    >
      <template v-for="item in items" :key="item.id">
        <div v-if="item.divider" class="my-1 border-t border-subtle"></div>
        <button
          role="menuitem"
          class="w-full px-3 py-1.5 text-xs text-left flex items-center gap-2 transition-colors"
          :class="item.danger
            ? 'text-red-500 hover:text-red-600 hover:bg-red-50 dark:hover:bg-red-950/30'
            : 'text-secondary hover:text-primary hover:bg-surface-hover'"
          @click="handleSelect(item.id)"
        >
          <svg
            v-if="item.icon"
            class="w-3.5 h-3.5 flex-shrink-0"
            fill="none"
            viewBox="0 0 24 24"
            stroke="currentColor"
            stroke-width="2"
          >
            <path stroke-linecap="round" stroke-linejoin="round" :d="item.icon" />
          </svg>
          <span>{{ item.label }}</span>
        </button>
      </template>
    </div>
  </Teleport>
</template>
