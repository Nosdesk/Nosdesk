<script setup lang="ts">
/**
 * The widget's first screen: help articles and a request form for visitors,
 * or a pointer to the portal when the workspace only helps signed-in people.
 */
import { computed, onMounted, ref } from 'vue'
import { RouterLink } from 'vue-router'
import { useFluent } from 'fluent-vue'
import axios from 'axios'

import Icon from '@/components/common/Icon.vue'
import { usePublicSettingsStore } from '@nosdesk/core/stores/publicSettings'

import { embedInit } from '../embed'

const { $t: t } = useFluent()
const publicSettings = usePublicSettingsStore()
void publicSettings.load()

const info = ref<{ enabled: boolean; allow_anonymous: boolean } | null>(null)
onMounted(async () => {
  // Opened on its own (not framed): nobody will introduce themselves.
  if (window.parent !== window) await embedInit()
  try {
    const { data } = await axios.get('/api/portal/auth/widget')
    info.value = data
  } catch {
    info.value = { enabled: false, allow_anonymous: false }
  }
})

const helpCentre = computed(() => publicSettings.settings?.guest_public_docs_enabled === true)
const requestForm = computed(() => publicSettings.settings?.guest_tickets_enabled === true)
// The full portal lives beside this page: `/` on hosted, `/portal/` self-hosted.
const portalUrl = window.location.pathname.replace(/widget\/?$/, '')
</script>

<template>
  <div class="flex flex-col gap-5 p-5">
    <div class="flex flex-col gap-1">
      <h1 class="text-lg font-semibold text-primary">{{ t('widget-title') }}</h1>
      <p v-if="info && !info.allow_anonymous" class="text-sm text-secondary">{{ t('widget-sign-in-needed') }}</p>
    </div>

    <nav v-if="info?.allow_anonymous" class="flex flex-col gap-2">
      <RouterLink
        v-if="helpCentre"
        to="/docs"
        class="flex items-center gap-3 rounded-xl border border-default bg-surface px-4 py-3 hover:bg-surface-hover"
      >
        <Icon name="book" size="sm" class="text-accent" />
        <span class="flex flex-col">
          <span class="text-sm font-medium text-primary">{{ t('widget-help-articles') }}</span>
          <span class="text-xs text-secondary">{{ t('widget-help-articles-hint') }}</span>
        </span>
      </RouterLink>
      <RouterLink
        v-if="requestForm"
        to="/submit-ticket"
        class="flex items-center gap-3 rounded-xl border border-default bg-surface px-4 py-3 hover:bg-surface-hover"
      >
        <Icon name="comment" size="sm" class="text-accent" />
        <span class="flex flex-col">
          <span class="text-sm font-medium text-primary">{{ t('widget-new-request') }}</span>
          <span class="text-xs text-secondary">{{ t('widget-new-request-hint') }}</span>
        </span>
      </RouterLink>
    </nav>

    <a :href="portalUrl" target="_blank" rel="noopener" class="text-sm text-accent hover:underline">
      {{ t('widget-open-portal') }}
    </a>
  </div>
</template>
