<!-- DocumentIconSelector.vue - Professional Notion-style icon picker -->
<script setup lang="ts">
import { ref, watch, computed, nextTick } from 'vue'
import { useFluent } from 'fluent-vue'
import Emoji from '@/components/common/Emoji.vue'
import Popover from '@/components/common/Popover.vue'
import DocumentIconPickerPanel from '@/components/DocumentIconPickerPanel.vue'

const { $t } = useFluent()

interface Props {
  initialIcon?: string
  size?: 'sm' | 'md' | 'lg'
}

const props = withDefaults(defineProps<Props>(), {
  initialIcon: '📄',
  size: 'md',
})

const emit = defineEmits(['update:icon'])

const currentIcon = ref(props.initialIcon)
const showDropdown = ref(false)
const triggerRef = ref<HTMLElement | null>(null)

const anchor = computed(() => ({
  type: 'element' as const,
  element: () => triggerRef.value,
}))

watch(() => props.initialIcon, (newIcon) => {
  if (newIcon !== currentIcon.value) {
    currentIcon.value = newIcon
  }
})

const sizeClasses = computed(() => {
  switch (props.size) {
    case 'sm': return 'w-7 h-7'
    case 'lg': return 'w-12 h-12'
    default: return 'w-9 h-9'
  }
})

const emojiTriggerSize = computed((): 'md' | 'lg' | 'xl' => {
  switch (props.size) {
    case 'sm': return 'md'
    case 'lg': return 'xl'
    default: return 'lg'
  }
})

function onIconSelected(icon: string) {
  currentIcon.value = icon
  emit('update:icon', icon)
}

function onPanelSelect() {
  showDropdown.value = false
}

function toggleDropdown() {
  showDropdown.value = !showDropdown.value
  if (showDropdown.value) {
    nextTick()
  }
}

function closeDropdown() {
  showDropdown.value = false
}
</script>

<template>
  <div class="inline-block">
    <button
      ref="triggerRef"
      type="button"
      @click="toggleDropdown"
      class="flex items-center justify-center rounded-lg transition-all duration-150 hover:bg-surface-hover active:scale-95 focus:outline-none focus:ring-2 focus:ring-accent/50"
      :class="sizeClasses"
      :aria-label="$t('doc-icon-selector-trigger-aria')"
      :aria-expanded="showDropdown"
      aria-haspopup="dialog"
    >
      <Emoji :emoji="currentIcon" :size="emojiTriggerSize" />
    </button>

    <Popover
      :open="showDropdown"
      :anchor="anchor"
      placement="bottom-start"
      react-to-scroll="reposition"
      :offset="8"
      :auto-focus="false"
      role="dialog"
      popover-class="w-80 overflow-hidden"
      @close="closeDropdown"
    >
      <DocumentIconPickerPanel
        v-model="currentIcon"
        :active="showDropdown"
        @update:model-value="onIconSelected"
        @select="onPanelSelect"
      />
    </Popover>
  </div>
</template>
