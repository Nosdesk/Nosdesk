<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { RouterLink, useRouter } from 'vue-router'
import { useQuery, useQueryCache } from '@pinia/colada'
import { useFluent } from 'fluent-vue'

import Button from '@/components/common/Button.vue'
import LogoIcon from '@/components/icons/LogoIcon.vue'
import { useBrandingStore } from '@/stores/branding'
import { useThemeStore } from '@/stores/theme'
import { useDateStore } from '@nosdesk/core/stores/dateStore'
import { usePublicSettingsStore } from '@nosdesk/core/stores/publicSettings'

import NoticeBanner from '@/components/requester/NoticeBanner.vue'

import { followNotice, getMe, getNotice, listMyApprovals, signOut } from '../service'
import { embedHost } from '../embed'

const { $t: t } = useFluent()
const router = useRouter()
const branding = useBrandingStore()
const theme = useThemeStore()
const dateStore = useDateStore()

const logoUrl = computed(() => branding.getLogoUrl(theme.isDarkMode))

// The help centre, when the workspace publishes one.
const publicSettings = usePublicSettingsStore()
void publicSettings.load()
const helpCentre = computed(() => publicSettings.settings?.guest_public_docs_enabled === true)

const me = useQuery({ key: ['portal', 'me'], query: getMe })
// Shown only to people with requests waiting for their approval.
const approvals = useQuery({ key: ['portal', 'approvals'], query: listMyApprovals })
const waitingApprovals = computed(() => approvals.data.value?.length ?? 0)

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

async function onSignOut(): Promise<void> {
  try {
    await signOut()
  } finally {
    void router.replace('/login')
  }
}
</script>

<template>
  <div class="min-h-dvh w-full flex flex-col bg-app">
    <!-- The widget has its own header; in Teams the app bar already names us
         and signing out isn't ours to do. -->
    <header v-if="embedHost !== 'widget'" class="w-full border-b border-default bg-surface">
      <div class="max-w-3xl mx-auto px-4 h-14 flex items-center gap-4">
        <RouterLink
          v-if="embedHost !== 'teams'"
          to="/tickets"
          class="flex items-center gap-2 min-w-0"
          :aria-label="branding.appName"
        >
          <img v-if="logoUrl" :src="logoUrl" :alt="branding.appName" class="h-7 max-w-[160px] object-contain" />
          <LogoIcon v-else class="h-7 text-accent" />
        </RouterLink>
        <nav class="flex items-center gap-1 text-sm">
          <RouterLink
            to="/tickets"
            class="px-2 py-1 rounded-md text-secondary hover:text-primary hover:bg-surface-hover"
            active-class="text-primary font-medium"
          >
            {{ t('portal-nav-requests') }}
          </RouterLink>
          <RouterLink
            v-if="waitingApprovals > 0"
            to="/approvals"
            class="px-2 py-1 rounded-md text-secondary hover:text-primary hover:bg-surface-hover"
            active-class="text-primary font-medium"
          >
            {{ t('portal-nav-approvals', { count: waitingApprovals }) }}
          </RouterLink>
          <RouterLink
            v-if="helpCentre"
            to="/docs"
            class="px-2 py-1 rounded-md text-secondary hover:text-primary hover:bg-surface-hover"
            active-class="text-primary font-medium"
          >
            {{ t('portal-nav-help') }}
          </RouterLink>
        </nav>
        <div class="ml-auto flex items-center gap-2">
          <Button size="sm" icon="add" @click="router.push('/tickets/new')">
            {{ t('portal-nav-new') }}
          </Button>
          <template v-if="embedHost !== 'teams'">
            <span v-if="me.data.value" class="hidden sm:inline text-sm text-secondary truncate max-w-[12rem]">
              {{ me.data.value.name }}
            </span>
            <Button variant="ghost" size="sm" @click="onSignOut">{{ t('portal-sign-out') }}</Button>
          </template>
        </div>
      </div>
    </header>
    <main class="w-full max-w-3xl mx-auto px-4 py-6 flex flex-col gap-4">
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
  </div>
</template>
