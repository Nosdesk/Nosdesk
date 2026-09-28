<script setup lang="ts">
import { computed } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { TooltipProvider } from 'reka-ui'
import { useFluent } from 'fluent-vue'

import IconButton from '@/components/common/IconButton.vue'
import { useBrandingStore } from '@/stores/branding'

import { isEmbed, postToParent } from './embed'
import { usePortalEvents } from './usePortalEvents'

// Live updates ride the portal session cookie, which an embedded page doesn't
// have.
if (!isEmbed) usePortalEvents()

// Esc closes the widget (the host page returns focus to its launcher),
// unless something inside the widget (a menu, a dialog) used it first.
if (isEmbed) {
  window.addEventListener('keydown', (event) => {
    if (event.key === 'Escape' && !event.defaultPrevented) postToParent({ type: 'nosdesk:close' })
  })
}

const { $t: t } = useFluent()
const route = useRoute()
const router = useRouter()
const branding = useBrandingStore()
const atHome = computed(() => route.name === 'embed')
</script>

<template>
  <TooltipProvider :delay-duration="300">
  <div v-if="isEmbed" class="h-dvh flex flex-col bg-app">
    <header class="flex items-center gap-1 px-2 py-2 border-b border-default bg-surface">
      <IconButton
        v-if="!atHome"
        icon="chevronLeft"
        size="sm"
        variant="ghost"
        :label="t('widget-back')"
        @click="router.back()"
      />
      <span class="flex-1 min-w-0 truncate px-1 text-sm font-semibold text-primary">{{ branding.appName }}</span>
      <IconButton icon="close" size="sm" variant="ghost" :label="t('widget-close')" @click="postToParent({ type: 'nosdesk:close' })" />
    </header>
    <main class="flex-1 min-h-0 overflow-y-auto">
      <RouterView />
    </main>
  </div>
  <RouterView v-else />
  </TooltipProvider>
</template>
