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

/**
 * "Loading" state: caller passed a UUID (so they're identifying
 * a user by id) but neither the resolved name nor an avatar URL
 * has arrived yet. Without this gate we'd render the initials
 * fallback with `getBackgroundColor(uuid)` — which produces a
 * UUID-derived hue (typically purple, since hex-prefixed UUIDs
 * land in the same bucket of the HSL colour wheel) that then
 * jumps to a name-derived hue once the user resolves. The
 * skeleton state hides that flash.
 *
 * Once `userName` arrives the colour stays stable: it's derived
 * from the actual display name, not the uuid.
 */
const isLoading = computed<boolean>(
  () => isUuid(props.name) && !props.userName && !props.avatar,
)

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
// Text sizes use rem units (or Tailwind scale tokens) so they scale
// proportionally with the document root rather than being fixed pixels.
const sizeClasses = computed(() => {
  const sizes = {
    xxs: { base: 'h-4 w-4', text: 'text-[0.5rem]' },
    xs: { base: 'h-5 w-5', text: 'text-[0.5625rem]' },
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
    case 'xs': return `${base} text-[0.625rem]`
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
      <!-- Three-state crossfade. mode="out-in" prevents stacked
           overlap during the swap so the circle stays a single
           silhouette. The skeleton pulses on a neutral surface
           tone — never a UUID-derived hue — so the resolve
           doesn't flash a different colour into place. -->
      <Transition name="avatar-resolve" mode="out-in">
        <!-- Loading: skeleton pulse. Same pattern the rest of
             the app uses (TicketRowSkeleton, etc.). -->
        <div
          v-if="isLoading"
          key="loading"
          class="w-full h-full rounded-full bg-surface-alt animate-pulse"
          aria-hidden="true"
        />
        <!-- Resolved + has image. :key on the URL forces element
             recreation when URL changes, bypassing browser cache. -->
        <img
          v-else-if="avatar && !imageFailed"
          :key="`img:${avatar}`"
          :src="avatar"
          :alt="displayName || 'User'"
          :title="displayName || 'User'"
          class="w-full h-full rounded-full object-cover"
          loading="lazy"
          @error="imageFailed = true"
        />
        <!-- Resolved without image: coloured initials fallback. -->
        <div
          v-else
          key="initials"
          :class="sizeClasses.text"
          class="w-full h-full rounded-full flex items-center justify-center font-medium text-white"
          :style="{ backgroundColor: getBackgroundColor(displayName || name) }"
          :title="displayName || 'User'"
        >
          {{ getInitials(displayName) }}
        </div>
      </Transition>
    </div>

    <!-- Name text. While loading, render a width-matched
         skeleton bar instead of the eventual name so the row
         doesn't shift width when the user resolves. -->
    <span v-if="showName && displayName" :class="nameTextClasses">
      {{ displayName }}
    </span>
    <span
      v-else-if="showName && isLoading"
      class="h-3 w-20 rounded bg-surface-alt animate-pulse"
      aria-hidden="true"
    />
  </div>
</template>

<style scoped>
/* Crossfade between loading skeleton, image, and initials so
   the circle morphs in place rather than snap-replacing. Short
   enough to feel responsive (the data is usually local cache);
   long enough that a quick resolve doesn't look like a glitch. */
.avatar-resolve-enter-active,
.avatar-resolve-leave-active {
  transition: opacity 160ms ease-out;
}
.avatar-resolve-enter-from,
.avatar-resolve-leave-to {
  opacity: 0;
}

@media (prefers-reduced-motion: reduce) {
  .avatar-resolve-enter-active,
  .avatar-resolve-leave-active {
    transition: opacity 80ms linear;
  }
}
</style>
