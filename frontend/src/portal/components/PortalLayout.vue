<script setup lang="ts">
/**
 * The signed-in portal's frame: a header with the workspace's logo, the
 * sections (requests, approvals when any wait, help when published), "New
 * request" and the account menu; on phones a tab bar instead. In Teams the
 * app bar already names us and signing out isn't ours to offer, so the header
 * keeps only the sections; the widget has its own header.
 */
import { computed, ref, watch } from 'vue'
import { RouterLink, useRoute, useRouter } from 'vue-router'
import { useQuery, useQueryCache } from '@pinia/colada'
import { useFluent } from 'fluent-vue'

import Button from '@/components/common/Button.vue'
import Icon from '@/components/common/Icon.vue'
import type { IconName } from '@/components/common/icons'
import MenuRow from '@/components/common/MenuItem.vue'
import ResponsiveMenu from '@/components/common/ResponsiveMenu.vue'
import LogoIcon from '@/components/icons/LogoIcon.vue'
import { useBrandingStore } from '@/stores/branding'
import { useThemeStore } from '@/stores/theme'
import { useDateStore } from '@nosdesk/core/stores/dateStore'
import { usePublicSettingsStore } from '@nosdesk/core/stores/publicSettings'

import NoticeBanner from '@/components/requester/NoticeBanner.vue'

import PortalAvatar from './PortalAvatar.vue'
import { followNotice, getMe, getNotice, listMyApprovals, signOut } from '../service'
import { embedHost } from '../embed'

withDefaults(defineProps<{ wide?: boolean }>(), { wide: false })

const { $t: t } = useFluent()
const route = useRoute()
const router = useRouter()
const branding = useBrandingStore()
const theme = useThemeStore()
const dateStore = useDateStore()

const inTeams = embedHost === 'teams'
const inWidget = embedHost === 'widget'
const logoUrl = computed(() => branding.getLogoUrl(theme.isDarkMode))

// The help centre, when the workspace publishes one.
const publicSettings = usePublicSettingsStore()
void publicSettings.load()
const helpCentre = computed(() => publicSettings.settings?.guest_public_docs_enabled === true)

const me = useQuery({ key: ['portal', 'me'], query: getMe })
// Shown only to people with requests waiting for their approval.
const approvals = useQuery({ key: ['portal', 'approvals'], query: listMyApprovals })
const waitingApprovals = computed(() => approvals.data.value?.length ?? 0)

interface Section {
  to: string
  label: string
  icon: IconName
  count?: number
}
const sections = computed<Section[]>(() => [
  { to: '/tickets', label: t('portal-nav-requests'), icon: 'inbox' as const },
  ...(waitingApprovals.value > 0
    ? [{ to: '/approvals', label: t('portal-nav-approvals-short'), icon: 'checkCircle' as const, count: waitingApprovals.value }]
    : []),
  ...(helpCentre.value ? [{ to: '/docs', label: t('portal-nav-help'), icon: 'book' as const }] : []),
])
function isActive(to: string): boolean {
  if (to === '/tickets') return route.path === '/tickets' || /^\/tickets\/\d+/.test(route.path)
  return route.path === to || route.path.startsWith(`${to}/`)
}

// The team's known-issue notice; following it adds the requester to the
// incident, which then shows in their requests.
const queryCache = useQueryCache()
const notice = useQuery({ key: ['portal', 'notice'], query: getNotice })
const following = ref(false)
async function follow(): Promise<void> {
  const current = notice.data.value?.notice
  if (!current) return
  following.value = true
  try {
    await followNotice(current.id)
    await queryCache.invalidateQueries({ key: ['portal', 'notice'] })
    void queryCache.invalidateQueries({ key: ['portal', 'tickets'] })
  } finally {
    following.value = false
  }
}
// Speak the requester's language once we know it.
watch(
  () => me.data.value?.effective_locale,
  (locale) => {
    if (locale) dateStore.setUserLocale(locale)
  },
  { immediate: true },
)

const accountButton = ref<HTMLElement | null>(null)
const accountOpen = ref(false)
const accountAnchor = computed(() => ({ type: 'element' as const, element: () => accountButton.value }))
async function onSignOut(): Promise<void> {
  accountOpen.value = false
  try {
    await signOut()
  } finally {
    void router.replace('/login')
  }
}
</script>

<template>
  <div class="min-h-dvh w-full flex flex-col bg-app">
    <header v-if="!inWidget" class="sticky top-0 z-20 w-full border-b border-default bg-surface/95 backdrop-blur">
      <div
        class="mx-auto px-4 h-14 flex items-center gap-2 sm:gap-6"
        :class="wide ? 'max-w-6xl' : 'max-w-4xl'"
      >
        <RouterLink
          v-if="!inTeams"
          to="/tickets"
          class="flex items-center min-w-0 shrink-0 rounded-md focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
          :aria-label="branding.appName"
        >
          <img v-if="logoUrl" :src="logoUrl" :alt="branding.appName" class="h-6 max-w-[140px] object-contain" />
          <LogoIcon v-else class="h-6 text-accent" />
        </RouterLink>

        <nav
          class="items-center gap-1 text-sm"
          :class="inTeams ? 'flex' : 'hidden sm:flex'"
          :aria-label="t('portal-nav-label')"
        >
          <RouterLink
            v-for="section in sections"
            :key="section.to"
            :to="section.to"
            class="inline-flex items-center gap-2 px-2.5 py-1.5 rounded-md transition-colors"
            :class="isActive(section.to) ? 'bg-surface-hover text-primary font-medium' : 'text-secondary hover:text-primary hover:bg-surface-hover'"
            :aria-current="isActive(section.to) ? 'page' : undefined"
          >
            {{ section.label }}
            <span
              v-if="section.count"
              class="min-w-5 h-5 px-1.5 rounded-full bg-accent text-white text-xs font-medium inline-flex items-center justify-center"
            >
              {{ section.count }}
            </span>
          </RouterLink>
        </nav>

        <div class="ml-auto flex items-center gap-2">
          <!-- A wrapper carries the display toggle: Button's own inline-flex
               would beat `hidden`. Phones use the tab bar's instead. -->
          <div :class="inTeams ? 'flex' : 'hidden sm:flex'">
            <Button size="sm" icon="add" @click="router.push('/tickets/new')">
              {{ t('portal-nav-new') }}
            </Button>
          </div>
          <button
            v-if="!inTeams"
            ref="accountButton"
            type="button"
            class="rounded-full focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
            :aria-label="t('portal-account-menu')"
            :aria-expanded="accountOpen"
            @click="accountOpen = !accountOpen"
          >
            <PortalAvatar :name="me.data.value?.name ?? ''" size="md" />
          </button>
        </div>
      </div>
    </header>

    <ResponsiveMenu
      v-if="!inTeams && !inWidget"
      :open="accountOpen"
      :anchor="accountAnchor"
      placement="bottom-end"
      :auto-focus="false"
      role="menu"
      :aria-label="t('portal-account-menu')"
      popover-class="bg-surface border border-default rounded-lg shadow-lg py-1 min-w-[14rem]"
      @close="accountOpen = false"
    >
      <div class="flex items-center gap-3 px-4 py-3 border-b border-default">
        <PortalAvatar :name="me.data.value?.name ?? ''" size="lg" />
        <div class="flex flex-col min-w-0">
          <span class="text-sm font-medium text-primary truncate">{{ me.data.value?.name }}</span>
          <span v-if="me.data.value?.email" class="text-xs text-secondary truncate">{{ me.data.value.email }}</span>
        </div>
      </div>
      <MenuRow tone="danger" @click="onSignOut">{{ t('portal-sign-out') }}</MenuRow>
    </ResponsiveMenu>

    <main
      class="w-full mx-auto px-4 py-6 sm:py-8 flex flex-col gap-6"
      :class="[wide ? 'max-w-6xl' : 'max-w-4xl', !inTeams && !inWidget ? 'pb-24 sm:pb-8' : '']"
    >
      <NoticeBanner
        v-if="notice.data.value?.notice"
        :notice="notice.data.value.notice"
        :following="notice.data.value.following"
        can-follow
        :busy="following"
        @follow="follow"
      />
      <slot />
    </main>

    <!-- Phones: the sections and "New request" as a tab bar, like the agent app. -->
    <nav
      v-if="!inTeams && !inWidget"
      class="fixed bottom-0 inset-x-0 z-20 sm:hidden bg-surface-alt border-t border-default pb-[env(safe-area-inset-bottom)]"
      :aria-label="t('portal-nav-label')"
    >
      <div class="flex items-stretch h-14">
        <RouterLink
          v-for="section in sections.slice(0, 1)"
          :key="section.to"
          :to="section.to"
          class="flex-1 flex flex-col items-center justify-center gap-0.5 text-[0.6875rem]"
          :class="isActive(section.to) ? 'text-accent' : 'text-secondary'"
          :aria-current="isActive(section.to) ? 'page' : undefined"
        >
          <Icon :name="section.icon" size="md" />
          {{ section.label }}
        </RouterLink>
        <RouterLink
          to="/tickets/new"
          class="flex-1 flex flex-col items-center justify-center gap-0.5 text-[0.6875rem]"
          :class="route.path === '/tickets/new' ? 'text-accent' : 'text-secondary'"
        >
          <Icon name="add" size="md" />
          {{ t('portal-nav-new') }}
        </RouterLink>
        <RouterLink
          v-for="section in sections.slice(1)"
          :key="section.to"
          :to="section.to"
          class="relative flex-1 flex flex-col items-center justify-center gap-0.5 text-[0.6875rem]"
          :class="isActive(section.to) ? 'text-accent' : 'text-secondary'"
          :aria-current="isActive(section.to) ? 'page' : undefined"
        >
          <Icon :name="section.icon" size="md" />
          {{ section.label }}
          <span
            v-if="section.count"
            class="absolute top-1.5 left-1/2 ml-2 min-w-4 h-4 px-1 rounded-full bg-accent text-white text-[0.625rem] font-medium inline-flex items-center justify-center"
          >
            {{ section.count }}
          </span>
        </RouterLink>
      </div>
    </nav>
  </div>
</template>
