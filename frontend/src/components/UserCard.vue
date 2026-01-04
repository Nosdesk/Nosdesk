<script setup lang="ts">
/**
 * UserCard Component
 *
 * A simple, reusable card for displaying user information.
 * Used in contexts like DeviceView, ticket assignee display, etc.
 *
 * For editable profile settings, use UserProfileCard instead.
 */
import UserAvatar from '@/components/UserAvatar.vue'

interface UserData {
  uuid: string
  name: string
  email: string
  role: string
  avatar_url?: string | null
  avatar_thumb?: string | null
}

const props = withDefaults(defineProps<{
  user: UserData
  showRole?: boolean
  showEmail?: boolean
  avatarSize?: 'sm' | 'md' | 'lg'
  linkToProfile?: boolean
}>(), {
  showRole: true,
  showEmail: true,
  avatarSize: 'lg',
  linkToProfile: true
})

// Map size prop to UserAvatar sizes
const avatarSizeMap = {
  sm: 'md' as const,
  md: 'lg' as const,
  lg: 'xl' as const
}
</script>

<template>
  <div class="flex items-start gap-4">
    <!-- Avatar -->
    <component
      :is="linkToProfile ? 'router-link' : 'div'"
      :to="linkToProfile ? `/users/${user.uuid}` : undefined"
      class="flex-shrink-0 group"
    >
      <div class="rounded-full ring-2 ring-surface-alt group-hover:ring-accent transition-all">
        <UserAvatar
          :name="user.uuid"
          :userName="user.name"
          :avatar="user.avatar_thumb || user.avatar_url"
          :size="avatarSizeMap[avatarSize]"
          :clickable="false"
          :show-name="false"
        />
      </div>
    </component>

    <!-- User Details -->
    <div class="flex-1 min-w-0">
      <component
        :is="linkToProfile ? 'router-link' : 'span'"
        :to="linkToProfile ? `/users/${user.uuid}` : undefined"
        class="text-lg font-semibold text-primary block truncate"
        :class="{ 'hover:text-accent transition-colors': linkToProfile }"
      >
        {{ user.name }}
      </component>

      <p v-if="showEmail" class="text-sm text-secondary truncate mt-1">
        {{ user.email }}
      </p>

      <div v-if="showRole" class="mt-2">
        <span
          class="inline-flex items-center px-2.5 py-1 rounded-md text-xs font-medium"
          :class="{
            'bg-status-error-muted text-status-error': user.role === 'admin',
            'bg-accent-muted text-accent': user.role === 'technician',
            'bg-surface-alt text-secondary border border-default': user.role === 'user'
          }"
        >
          {{ user.role.charAt(0).toUpperCase() + user.role.slice(1) }}
        </span>
      </div>
    </div>
  </div>
</template>
