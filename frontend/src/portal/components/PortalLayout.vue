<script setup lang="ts">
import { computed, watch } from 'vue'
import { RouterLink, useRouter } from 'vue-router'
import { useQuery } from '@pinia/colada'
import { useFluent } from 'fluent-vue'

import Button from '@/components/common/Button.vue'
import LogoIcon from '@/components/icons/LogoIcon.vue'
import { useBrandingStore } from '@/stores/branding'
import { useThemeStore } from '@/stores/theme'
import { useDateStore } from '@nosdesk/core/stores/dateStore'

import { getMe, signOut } from '../service'

const { $t: t } = useFluent()
const router = useRouter()
const branding = useBrandingStore()
const theme = useThemeStore()
const dateStore = useDateStore()

const logoUrl = computed(() => branding.getLogoUrl(theme.isDarkMode))

const me = useQuery({ key: ['portal', 'me'], query: getMe })
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
    <header class="w-full border-b border-default bg-surface">
      <div class="max-w-3xl mx-auto px-4 h-14 flex items-center gap-4">
        <RouterLink to="/tickets" class="flex items-center gap-2 min-w-0" :aria-label="branding.appName">
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
        </nav>
        <div class="ml-auto flex items-center gap-2">
          <Button size="sm" icon="add" @click="router.push('/tickets/new')">
            {{ t('portal-nav-new') }}
          </Button>
          <span v-if="me.data.value" class="hidden sm:inline text-sm text-secondary truncate max-w-[12rem]">
            {{ me.data.value.name }}
          </span>
          <Button variant="ghost" size="sm" @click="onSignOut">{{ t('portal-sign-out') }}</Button>
        </div>
      </div>
    </header>
    <main class="w-full max-w-3xl mx-auto px-4 py-6 flex flex-col gap-4">
      <slot />
    </main>
  </div>
</template>
