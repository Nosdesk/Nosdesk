<script setup lang="ts">
/**
 * The Teams tab when it can't sign the person in on its own: why, and the one
 * thing they can do about it (agree to sign in, try again, or ask their admin).
 */
import { computed, ref } from 'vue'
import { useRouter } from 'vue-router'
import { useFluent } from 'fluent-vue'

import Button from '@/components/common/Button.vue'
import Icon from '@/components/common/Icon.vue'

import { initTeams, routeForSubPage, signInTeams, teamsProblem } from '../teams'

const { $t: t } = useFluent()
const router = useRouter()
const busy = ref(false)

const problem = computed(() => teamsProblem.value ?? 'failed')
// The full portal lives beside this page: `/` on hosted, `/portal/` self-hosted.
const portalUrl = window.location.pathname.replace(/teams\/?$/, '')

const copy = computed(() => {
  switch (problem.value) {
    case 'not-in-teams':
      return { title: t('teams-tab-not-in-teams-title'), body: t('teams-tab-not-in-teams-body') }
    case 'off':
      return { title: t('teams-tab-off-title'), body: t('teams-tab-off-body') }
    case 'unsupported':
      return { title: t('teams-tab-unsupported-title'), body: t('teams-tab-unsupported-body') }
    case 'consent':
      return { title: t('teams-tab-consent-title'), body: t('teams-tab-consent-body') }
    case 'not-set-up':
      return { title: t('teams-tab-not-set-up-title'), body: t('teams-tab-not-set-up-body') }
    default:
      return { title: t('teams-tab-failed-title'), body: t('teams-tab-failed-body') }
  }
})

async function retry(interactive: boolean): Promise<void> {
  busy.value = true
  try {
    const context = await initTeams()
    if ((await signInTeams(interactive)) === null) {
      await router.replace(routeForSubPage(context?.subPageId ?? null))
    }
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <div class="min-h-dvh flex items-center justify-center bg-app p-6">
    <div class="w-full max-w-sm flex flex-col items-center gap-4 text-center">
      <Icon name="lock" size="lg" class="text-tertiary" />
      <div class="flex flex-col gap-1">
        <h1 class="text-lg font-semibold text-primary">{{ copy.title }}</h1>
        <p class="text-sm text-secondary">{{ copy.body }}</p>
      </div>
      <Button v-if="problem === 'consent'" :loading="busy" @click="retry(true)">
        {{ t('teams-tab-consent-action') }}
      </Button>
      <Button v-else-if="problem === 'failed'" variant="secondary" :loading="busy" @click="retry(false)">
        {{ t('teams-tab-retry') }}
      </Button>
      <a
        v-if="problem !== 'off'"
        :href="portalUrl"
        target="_blank"
        rel="noopener"
        class="text-sm text-accent hover:underline"
      >
        {{ t('teams-tab-open-in-browser') }}
      </a>
    </div>
  </div>
</template>
