<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useRouter } from 'vue-router'

interface Props {
  /** User UUID (for navigation) or display name */
  name: string
  /** Display name - if provided, used instead of name for display */
  userName?: string
  /** Show name text next to avatar */
  showName?: boolean
  /** Avatar size */
  size?: 'xxs' | 'xs' | 'sm' | 'md' | 'lg' | 'xl' | 'full'
  /** Avatar image URL */
  avatar?: string | null
  /** Whether clicking navigates to user profile */
  clickable?: boolean
}

const props = withDefaults(defineProps<Props>(), {
  showName: true,
  size: 'md',
  avatar: null,
  clickable: true
})

const router = useRouter()

// Check if string is a UUID
const isUuid = (str: string) => {
  const uuidPattern = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i
  return uuidPattern.test(str)
}

// Display name: prefer userName prop, fall back to name if it's not a UUID
const displayName = computed(() => {
  if (props.userName) return props.userName
  if (!isUuid(props.name)) return props.name
  return '' // Empty for UUID without userName - will show '?' initials
})

// Generate initials from name
const getInitials = (name: string) => {
  if (!name) return '?'

  const parts = name.split(/[\s-]+/)
  return parts
    .filter(part => part.length > 0)
    .map(word => word.charAt(0))
    .join('')
    .toUpperCase()
    .slice(0, 2) || '?'
}

// Generate consistent color from name
const getBackgroundColor = (name: string) => {
  if (!name) return 'hsl(200, 70%, 35%)'

  const firstLetter = name.charAt(0).toUpperCase()
  const position = firstLetter.charCodeAt(0) - 65
  const hue = Math.abs(position % 26) * (360 / 26)

  return `hsl(${hue}, 70%, 35%)`
}

// Size classes
const sizeClasses = computed(() => {
  const sizes = {
    xxs: { base: 'h-4 w-4', text: 'text-[8px]' },
    xs: { base: 'h-5 w-5', text: 'text-[10px]' },
    sm: { base: 'h-6 w-6', text: 'text-xs' },
    md: { base: 'h-8 w-8', text: 'text-sm' },
    lg: { base: 'h-9 w-9', text: 'text-base' },
    xl: { base: 'h-14 w-14', text: 'text-2xl' },
    full: { base: 'w-full h-full', text: 'text-4xl' }
  }
  return sizes[props.size] || sizes.md
})

const nameTextClasses = computed(() => {
  const base = 'text-primary truncate'
  switch (props.size) {
    case 'xs': return `${base} text-[10px]`
    case 'sm': return `${base} text-xs`
    case 'lg': return `${base} text-sm`
    case 'full': return `${base} text-xl`
    default: return `${base} text-sm`
  }
})

const navigateToProfile = () => {
  if (props.clickable && isUuid(props.name)) {
    router.push(`/users/${props.name}`)
  }
}

// Track image load failure
const imageFailed = ref(false)

watch(() => props.avatar, () => {
  imageFailed.value = false
})
</script>

<template>
  <div
    class="flex items-center"
    :class="[
      { 'cursor-pointer hover:opacity-80': clickable && isUuid(name) },
      size === 'full' ? 'h-full aspect-square' : '',
      showName ? 'gap-2' : ''
    ]"
    @click="navigateToProfile"
  >
    <!-- Avatar wrapper for theme effects -->
    <div class="avatar-themed rounded-full flex-shrink-0" :class="sizeClasses.base">
      <!-- Avatar with image -->
      <!-- :key forces element recreation when URL changes, bypassing browser cache -->
      <img
        v-if="avatar && !imageFailed"
        :key="avatar"
        :src="avatar"
        :alt="displayName || 'User'"
        :title="displayName || 'User'"
        class="w-full h-full rounded-full object-cover"
        loading="lazy"
        @error="imageFailed = true"
      />

      <!-- Avatar with initials fallback -->
      <div
        v-else
        :class="sizeClasses.text"
        class="w-full h-full rounded-full flex items-center justify-center font-medium text-white"
        :style="{ backgroundColor: getBackgroundColor(displayName || name) }"
        :title="displayName || 'User'"
      >
        {{ getInitials(displayName) }}
      </div>
    </div>

    <!-- Name text -->
    <span v-if="showName && displayName" :class="nameTextClasses">
      {{ displayName }}
    </span>
  </div>
</template>
