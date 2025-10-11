<!-- # src/components/UserAvatar.vue -->
<script setup lang="ts">
import { computed, ref, onMounted, watch, defineExpose, onUnmounted } from 'vue'
import { useRouter } from 'vue-router'
import userService from '@/services/userService'
import LazyImage from '@/components/LazyImage.vue'
import { useUserLookup } from '@/services/userLookupService'
import { useDataStore } from '@/stores/dataStore'

// Method to refresh a specific user's data
const refreshUser = async (userUuid: string) => {
  try {
    const userData = await dataStore.getUserByUuid(userUuid, true); // Force refresh through batching system
    if (userData) {
      // Add to lookup cache
      addUsersToCache([userData]);
    }
  } catch (err) {
    console.error('Error refreshing user data:', err);
  }
}

interface Props {
  name: string;
  userName?: string; // Optional pre-loaded user name
  showName?: boolean;
  size?: 'xs' | 'sm' | 'md' | 'lg' | 'xl' | 'full';
  avatar?: string | null;
  avatarUrl?: string | null; // Direct avatar URL from API responses
  clickable?: boolean;
}

const props = withDefaults(defineProps<Props>(), {
  showName: true,
  size: 'md',
  avatar: null,
  avatarUrl: null,
  clickable: true
})

const router = useRouter()

// User lookup service for efficient lookups
const { getUserName, getUserAvatar, lookupUser, addUsersToCache } = useUserLookup()
const dataStore = useDataStore()

// Simple ref for the element
const elementRef = ref<HTMLElement | null>(null)

// Force refresh counter for reactivity
const forceRefresh = ref(0) // Counter to force reactivity updates

// Check if a string is a UUID
const isUuid = (str: string) => {
  const uuidPattern = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i
  return uuidPattern.test(str)
}

// Fetch user data only if not available in cache and no userName prop provided
onMounted(async () => {
  if (isUuid(props.name) && !props.userName) {
    // Check if user is in cache first
    const cachedName = getUserName(props.name)
    if (!cachedName) {
      // Let the batching system handle coordination - no artificial delays needed
      if (elementRef.value) {
        await lookupUser(props.name)
        forceRefresh.value++ // Trigger reactivity update
      }
    }
  }
})

// Cleanup on unmount
onUnmounted(() => {
  // No cleanup needed since we removed periodic refresh
})

// Simple error handler - just refresh user data if needed
const handleImageError = async () => {
  if (isUuid(props.name)) {
    try {
      // Force refresh the user data in case avatar URLs have been updated
      await dataStore.getUserByUuid(props.name, true)
      forceRefresh.value++ // Trigger reactivity update
    } catch (error) {
      // Silently fail - the colored initials will be shown as fallback
    }
  }
}

// Watch for changes to props.name with debouncing
watch(() => props.name, async (newName, oldName) => {
  // Skip if name hasn't actually changed
  if (newName === oldName) return
  
  if (isUuid(newName) && !props.userName) {
    const cachedName = getUserName(newName)
    if (!cachedName) {
      // Let the batching system handle debouncing - no artificial delays needed
      if (props.name === newName && elementRef.value) {
        await lookupUser(newName)
        forceRefresh.value++ // Trigger reactivity update
      }
    }
  }
}) // Removed immediate: true to prevent burst requests on mount

// Check if the name is a UUID and get the actual name if it is
const displayName = computed(() => {
  // If we have a pre-loaded user name, use it immediately
  if (props.userName) {
    return props.userName;
  }
  
  // If the name is not a UUID, use it as-is
  if (!isUuid(props.name)) {
    return props.name;
  }
  
  // Only for UUID lookups do we need the reactive forceRefresh
  forceRefresh.value
  
  // Try to get name from lookup service cache for UUIDs
  const cachedName = getUserName(props.name);
  if (cachedName) {
    return cachedName;
  }
  
  // Fallback to UUID if no cached name available
  return props.name;
})

// Get the avatar URL either from props or from the user data if loaded by UUID
const effectiveAvatarUrl = computed(() => {
  // If an avatarUrl was directly provided as a prop (from API response), use it immediately
  if (props.avatarUrl) {
    return props.avatarUrl;
  }
  
  // If an avatar was directly provided as a prop, use it immediately
  if (props.avatar) {
    return props.avatar;
  }
  
  // Only for UUID lookups do we need the reactive forceRefresh
  forceRefresh.value
  
  // If name is a UUID, try to get avatar from lookup service (with cache busting)
  if (isUuid(props.name)) {
    const preferThumb = props.size !== 'full';
    return getUserAvatar(props.name, preferThumb);
  }

  return null;
})

const getInitials = (name: string) => {
  if (!name) {
    return '?';
  }
  
  // Split by space or hyphen to better handle formats like "User-123"
  const parts = name.split(/[\s-]+/);
  
  const initials = parts
    .filter(part => part.length > 0)
    .map(word => word.charAt(0))
    .join('')
    .toUpperCase()
    .slice(0, 2);
    
  return initials;
}

const getBackgroundColor = (name: string) => {
  if (!name) return 'hsl(0, 70%, 35%)';
  
  // Get the first letter and convert to uppercase
  const firstLetter = name.charAt(0).toUpperCase();
  
  // Get position in alphabet (A = 0, B = 1, etc.)
  const position = firstLetter.charCodeAt(0) - 65;
  
  // Convert position to a value between 0 and 360 (hue range)
  // We'll mod by 26 (alphabet length) first to handle non-letters
  const hue = (position % 26) * (360 / 26);
  
  // Return HSL color with fixed saturation and lightness
  return `hsl(${hue}, 70%, 35%)`;
}

const sizeClasses = computed(() => {
  // Base sizes in rem units
  const sizes = {
    xs: {
      base: 'h-[1.25rem] w-[1.25rem]', // 20px at default font size
      text: 'text-[10px]',
      responsive: 'sm:h-[1.375rem] sm:w-[1.375rem]' // 22px on small screens
    },
    sm: {
      base: 'h-[1.5rem] w-[1.5rem]', // 24px at default font size
      text: 'text-xs',
      responsive: 'sm:h-[1.75rem] sm:w-[1.75rem]' // Slightly larger on small screens
    },
    md: {
      base: 'h-[2rem] w-[2rem]', // 32px at default font size
      text: 'text-md',
      responsive: 'sm:h-[2.25rem] sm:w-[2.25rem]' // Slightly larger on small screens
    },
    lg: {
      base: 'h-[2.25rem] w-[2.25rem]', // 36px at default font size
      text: 'text-lg',
      responsive: 'sm:h-[2.5rem] sm:w-[2.5rem]' // 40px on small screens
    },
    xl: {
      base: 'h-[3.75rem] w-[3.75rem]', // 60px at default font size
      text: 'text-2xl',
      responsive: 'sm:h-[3.5rem] sm:w-[3.5rem]' // 56px on small screens
    },
    full: {
      base: 'w-full h-full min-w-full min-h-full', // Full size with minimum dimensions
      text: 'text-4xl sm:text-5xl',
      responsive: '' // No responsive changes needed
    }
  }

  return sizes[props.size] || sizes.md
})

// Computed class for the name text that scales with viewport
const nameTextClasses = computed(() => {
  const baseClasses = 'text-white transition-all duration-200'
  
  switch (props.size) {
    case 'xs':
      return `${baseClasses} text-[10px] sm:text-xs`
    case 'sm':
      return `${baseClasses} text-xs sm:text-sm`
    case 'lg':
      return `${baseClasses} text-sm` // Keeping text size consistent for header
    case 'full':
      return `${baseClasses} text-xl` // Larger text for full size
    default: // md
      return `${baseClasses} text-xs sm:text-sm`
  }
})

const navigateToProfile = () => {
  if (props.clickable && isUuid(props.name)) {
    router.push(`/users/${props.name}`)
  }
}

// Track if image failed to load
const imageFailed = ref(false)

// Reset failed state when avatar URL changes
watch(effectiveAvatarUrl, () => {
  imageFailed.value = false
})

// Export methods for external components to use
defineExpose({
  refreshUser
})
</script>

<template>
  <div 
    ref="elementRef"
    class="flex items-center"
    :class="[
      { 'cursor-pointer hover:opacity-80': clickable },
      size === 'full' ? 'h-full aspect-square' : '',
      showName ? 'gap-2' : ''
    ]"
    @click="navigateToProfile"
  >
    <!-- Avatar with image -->
    <LazyImage
      v-if="effectiveAvatarUrl && !imageFailed"
      :src="effectiveAvatarUrl"
      :alt="displayName"
      :class="[
        sizeClasses.base,
        sizeClasses.responsive,
        'rounded-full'
      ]"
      object-fit="cover"
      loading="lazy"
      @error="() => { imageFailed = true; handleImageError() }"
    />

    <!-- Avatar with initials fallback -->
    <div
      v-else
      :class="[
        sizeClasses.base,
        sizeClasses.responsive,
        sizeClasses.text
      ]"
      class="rounded-full flex items-center justify-center flex-shrink-0 font-medium text-white"
      :style="{ backgroundColor: getBackgroundColor(displayName) }"
    >
      {{ getInitials(displayName) }}
    </div>
    
    <!-- Text container with consistent spacing to prevent layout shift -->
    <span 
      v-if="showName" 
      :class="nameTextClasses"
      class="transition-all duration-500 ease-out truncate"
    >
      {{ displayName }}
    </span>
  </div>
</template>

<style scoped>
/* Ensure text doesn't cause layout shifts */
.truncate {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
</style>