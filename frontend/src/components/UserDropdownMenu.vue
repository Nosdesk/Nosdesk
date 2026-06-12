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
import { computed, ref } from 'vue'
import { useRouter } from 'vue-router'
import { useFluent } from 'fluent-vue'
import { useAuthStore } from '@/stores/auth'
import UserAvatar from './UserAvatar.vue'
import Popover from './common/Popover.vue'
import MenuList, { type MenuItem } from './common/MenuList.vue'
import { ICON_REGISTRY } from './common/icons'
import BugReportModal from './BugReportModal.vue'

interface Props {
  showMenu: boolean
  buttonRef?: HTMLElement | null
}

const props = defineProps<Props>()
const emit = defineEmits<{ (e: 'close'): void }>()

const router = useRouter()
const authStore = useAuthStore()
const fluent = useFluent()

const user = computed(() => {
  if (authStore.user) {
    return {
      name: authStore.user.name,
      email: authStore.user.email,
      avatar: authStore.user.avatar_url,
    }
  }
  return {
    name: fluent.$t('user-menu-guest-name'),
    email: 'guest@example.com',
    avatar: null,
  }
})

const anchor = computed(() => ({
  type: 'element' as const,
  element: () => props.buttonRef ?? null,
}))

const items = computed<MenuItem[]>(() => {
  const out: MenuItem[] = [
    { id: 'settings', label: fluent.$t('user-menu-account'), icon: ICON_REGISTRY.account.d },
  ]
  // Tenant self-serve: owners/admins of the current workspace get a
  // Team entry. Distinct from the platform-admin console below.
  const wsRole = authStore.user?.workspace_role
  if (wsRole === 'owner' || wsRole === 'admin') {
    out.push({ id: 'team', label: fluent.$t('user-menu-team'), icon: ICON_REGISTRY.team.d })
  }
  if (authStore.isAdmin) {
    out.push({ id: 'admin', label: fluent.$t('user-menu-administration'), icon: ICON_REGISTRY.admin.d })
  }
  out.push({ id: 'report-problem', label: fluent.$t('user-menu-report-problem'), icon: ICON_REGISTRY.warning.d, divider: true })
  out.push({ id: 'logout', label: fluent.$t('user-menu-sign-out'), danger: true })
  return out
})

const bugReportOpen = ref(false)

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
    case 'team':
      router.push('/workspace/members')
      break
    case 'admin':
      router.push('/admin')
      break
    case 'report-problem':
      bugReportOpen.value = true
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
    :aria-label="$t('user-menu-aria')"
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
        :uuid="authStore.user?.uuid"
        :fallbackName="user.name"
        :fallbackAvatar="user.avatar"
        size="xl"
        :show-name="false"
        :clickable="false"
        class="flex-shrink-0"
      />
      <div class="min-w-0 flex-1">
        <div class="text-sm font-medium text-primary truncate">{{ user.name }}</div>
        <div class="text-xs text-accent mt-1">{{ $t('user-menu-view-profile') }}</div>
      </div>
    </button>

    <MenuList :items="items" @select="handleSelect" />
  </Popover>

  <BugReportModal :is-open="bugReportOpen" @close="bugReportOpen = false" />
</template>
