<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { useReference } from '@nosdesk/core/sync/composables'
import { assetUrl } from '@nosdesk/core/transport'
import type { User } from '@nosdesk/core/types/user'

interface Props {
  /** UUID of the user to render. When set, the user's name and
   *  avatar are sourced live from the sync pool: lazy-fetched on
   *  miss and updated automatically when a `user.updated` SSE
   *  frame arrives. This is the supported path for any consumer
   *  that has a uuid in hand. */
  uuid?: string | null
  /** Display-name fallback. Used when no uuid is supplied (guest
   *  senders, presence rows without a backing user, chrome that
   *  doesn't want a pool round-trip) or while the pool row is
   *  still resolving. */
  fallbackName?: string
  /** Avatar URL fallback. Same semantics as fallbackName. */
  fallbackAvatar?: string | null
  /** Show name text next to avatar */
  showName?: boolean
  /** Avatar size */
  size?: 'xxs' | 'xs' | 'sm' | 'md' | 'lg' | 'xl' | 'full'
  /** Whether clicking navigates to the user profile. Only
   *  meaningful when uuid is set. */
  clickable?: boolean
}

const props = withDefaults(defineProps<Props>(), {
  showName: true,
  size: 'md',
  clickable: true,
})

const router = useRouter()

// Live row from the sync pool. Null while the row hasn't loaded
// or when uuid is unset; useReference batches and dispatches the
// lazy fetch when a referenced uuid isn't already in the pool.
const liveUser = useReference<User>('user', () => props.uuid ?? null)

const displayName = computed<string>(
  () => liveUser.value?.name ?? props.fallbackName ?? '',
)

// Pick between the 48x48 `avatar_thumb` and the 200x200
// `avatar_url`. Small chips upscale the thumb cleanly; the
// xl badge in the user menu and the `full` profile shot need
// the full-res original or they noticeably blur. The fallback
// from the caller wins only if neither pool field is set.
const prefersFullRes = computed<boolean>(
  () => props.size === 'xl' || props.size === 'full',
)

const avatarUrl = computed<string | null>(() => {
  const thumb = liveUser.value?.avatar_thumb ?? null
  const full = liveUser.value?.avatar_url ?? null
  const primary = prefersFullRes.value ? full : thumb
  const secondary = prefersFullRes.value ? thumb : full
  const resolved = primary ?? secondary ?? props.fallbackAvatar ?? null
  // Resolve the stored `/uploads/...` path for the current runtime: identity on
  // web (same origin as the API), the `nosdesk-asset://` proxy scheme on mobile
  // (the webview's `tauri://localhost` origin can't load a bare API path).
  return resolved ? assetUrl(resolved) : null
})

/**
 * "Loading" state: caller passed a UUID (so they're identifying
 * a user by id) but neither a resolved name nor an avatar URL
 * has arrived yet. Without this gate we'd render the initials
 * fallback with `getBackgroundColor(uuid)`, which produces a
 * UUID-derived hue (typically purple, since hex-prefixed UUIDs
 * land in the same bucket of the HSL colour wheel) that then
 * jumps to a name-derived hue once the user resolves. The
 * skeleton state hides that flash.
 *
 * Once the row resolves the colour stays stable: it's derived
 * from the actual display name, not the uuid.
 */
const isLoading = computed<boolean>(
  () => !!props.uuid && !displayName.value && !avatarUrl.value,
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
  if (props.clickable && props.uuid) {
    router.push(`/users/${props.uuid}`)
  }
}

// Track image download: `imageLoaded` gates the fade-in over the initials
// base so there's never a transparent gap while the bytes arrive; `imageFailed`
// falls back to the initials permanently. Both reset when the URL changes.
const imageLoaded = ref(false)
const imageFailed = ref(false)

watch(avatarUrl, () => {
  imageLoaded.value = false
  imageFailed.value = false
})

// A cached image can already be `complete` before `@load` binds, which would
// otherwise strand it at opacity 0. Catch that on mount.
const onImgMount = (el: unknown) => {
  const img = el as HTMLImageElement | null
  if (img?.complete && img.naturalWidth > 0) imageLoaded.value = true
}
</script>

<template>
  <div
    class="flex items-center"
    :class="[
      { 'cursor-pointer hover:opacity-80': clickable && uuid },
      size === 'full' ? 'h-full aspect-square' : '',
      // `min-w-0` only when a name is rendered: it lets the name span's
      // `truncate` engage in a width-constrained parent. The avatar
      // itself is already `flex-shrink-0`, so it can't be squashed.
      showName ? 'gap-2 min-w-0' : ''
    ]"
    @click="navigateToProfile"
  >
    <!-- Avatar wrapper for theme effects -->
    <div class="avatar-themed rounded-full flex-shrink-0" :class="sizeClasses.base">
      <!-- Three-state crossfade. mode="out-in" prevents stacked
           overlap during the swap so the circle stays a single
           silhouette. The skeleton pulses on a neutral surface
           tone, never a UUID-derived hue, so the resolve doesn't
           flash a different colour into place. -->
      <Transition name="avatar-resolve" mode="out-in">
        <!-- Loading: skeleton pulse while the pool row resolves. Same
             pattern the rest of the app uses (TicketRowSkeleton, etc.). -->
        <div
          v-if="isLoading"
          key="loading"
          class="w-full h-full rounded-full bg-surface-alt animate-pulse"
          aria-hidden="true"
        />
        <!-- Resolved: coloured initials are the base layer; the photo (when
             there is one) fades in over them once it decodes. So the circle is
             never a transparent gap while the image downloads, and a failed
             load simply leaves the initials in place. -->
        <div v-else key="resolved" class="relative w-full h-full">
          <div
            class="absolute inset-0 rounded-full flex items-center justify-center font-medium text-white"
            :class="sizeClasses.text"
            :style="{ backgroundColor: getBackgroundColor(displayName) }"
            :title="displayName || 'User'"
          >
            {{ getInitials(displayName) }}
          </div>
          <!-- :key on the URL forces element recreation when it changes,
               bypassing the browser cache and re-running the fade. -->
          <img
            v-if="avatarUrl && !imageFailed"
            :key="`img:${avatarUrl}`"
            :ref="onImgMount"
            :src="avatarUrl"
            :alt="displayName || 'User'"
            class="absolute inset-0 w-full h-full rounded-full object-cover transition-opacity duration-200"
            :class="imageLoaded ? 'opacity-100' : 'opacity-0'"
            loading="lazy"
            @load="imageLoaded = true"
            @error="imageFailed = true"
          />
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
