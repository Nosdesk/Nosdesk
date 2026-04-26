<script setup lang="ts">
/**
 * Header user-profile dropdown. Two parts inside one popover:
 * a clickable user-info card (avatar, name, "View Profile") at
 * the top, and a `<MenuList>` of standard account actions
 * below it. The split is intentional — the user card is rich
 * content (avatar + multi-line text) that doesn't fit
 * `MenuList`'s gutter-aligned action-row format.
 *
 * Positioning, dismiss, focus, and the fade-scale transition
 * delegate to `<Popover>` — same primitive every other dropdown
 * in the app uses.
 */
import { computed } from 'vue'
import { useRouter } from 'vue-router'
import { useAuthStore } from '@/stores/auth'
import UserAvatar from './UserAvatar.vue'
import Popover from './common/Popover.vue'
import MenuList, { type MenuItem } from './common/MenuList.vue'
import { ICON_REGISTRY } from './common/icons'

interface Props {
  showMenu: boolean
  buttonRef?: HTMLElement | null
}

const props = defineProps<Props>()
const emit = defineEmits<{ (e: 'close'): void }>()

const router = useRouter()
const authStore = useAuthStore()

const user = computed(() => {
  if (authStore.user) {
    return {
      name: authStore.user.name,
      email: authStore.user.email,
      avatar: authStore.user.avatar_url,
    }
  }
  return { name: 'Guest', email: 'guest@example.com', avatar: null }
})

const anchor = computed(() => ({
  type: 'element' as const,
  element: () => props.buttonRef ?? null,
}))

const items = computed<MenuItem[]>(() => {
  const out: MenuItem[] = [
    { id: 'settings', label: 'Settings', icon: ICON_REGISTRY.permissions.d },
  ]
  if (authStore.user?.role === 'admin') {
    out.push({ id: 'admin', label: 'Administration', icon: ICON_REGISTRY.permissions.d })
  }
  out.push({ id: 'logout', label: 'Sign out', danger: true, divider: true })
  return out
})

function handleProfileClick() {
  if (authStore.user) router.push(`/users/${authStore.user.uuid}`)
  emit('close')
}

function handleSelect(id: string) {
  emit('close')
  switch (id) {
    case 'settings':
      router.push('/profile/settings')
      break
    case 'admin':
      router.push('/admin')
      break
    case 'logout':
      try {
        authStore.logout()
      } catch (error) {
        console.error('Logout failed:', error)
      }
      break
  }
}
</script>

<template>
  <Popover
    :open="showMenu"
    :anchor="anchor"
    placement="bottom-end"
    react-to-scroll="reposition"
    :auto-focus="false"
    role="menu"
    aria-label="User menu"
    popover-class="bg-surface border border-default rounded-lg shadow-lg py-1 min-w-[12rem]"
    @close="emit('close')"
  >
    <!-- User info card. Click navigates to the profile page;
         the visual treatment doubles as the "you are signed in
         as X" affordance. -->
    <button
      type="button"
      class="w-full px-4 py-3 border-b border-default hover:bg-surface-hover cursor-pointer flex items-center gap-3 min-w-0 text-left"
      @click="handleProfileClick"
    >
      <UserAvatar
        :name="user.name"
        :avatar="user.avatar"
        size="xl"
        :show-name="false"
        :clickable="false"
        class="flex-shrink-0"
      />
      <div class="min-w-0 flex-1">
        <div class="text-sm font-medium text-primary truncate">{{ user.name }}</div>
        <div class="text-xs text-accent mt-1">View Profile</div>
      </div>
    </button>

    <MenuList :items="items" @select="handleSelect" />
  </Popover>
</template>
