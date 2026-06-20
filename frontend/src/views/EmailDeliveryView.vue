<script setup lang="ts">
/**
 * Email delivery — the consolidated outbound-email admin page.
 *
 * One page, two sub-tabs:
 *   - Setup: sending identity (hosted-aware) + sending domain (DKIM/DNS) + test.
 *   - Activity: the outbound queue + suppression list.
 *
 * The sub-sections are the existing standalone views rendered with `embedded`
 * (header/page-chrome suppressed) so they compose under one header. The
 * standalone routes are retired in favour of this page + `?tab=`.
 */
import { computed } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { useFluent } from 'fluent-vue';

import EmailSettingsView from './EmailSettingsView.vue';
import EmailSendingDomainView from './EmailSendingDomainView.vue';
import EmailQueueView from './admin/EmailQueueView.vue';
import EmailSuppressionsView from './admin/EmailSuppressionsView.vue';

const fluent = useFluent();
const t = (key: string) => fluent.$t(key);

const route = useRoute();
const router = useRouter();

type Tab = 'setup' | 'activity';
const tab = computed<Tab>(() => (route.query.tab === 'activity' ? 'activity' : 'setup'));

function selectTab(next: Tab) {
  if (next === tab.value) return;
  // 'setup' is the default, so drop the query param for a clean URL.
  router.replace({ query: { ...route.query, tab: next === 'setup' ? undefined : next } });
}

const tabs: { id: Tab; labelKey: string }[] = [
  { id: 'setup', labelKey: 'admin-email-delivery-tab-setup' },
  { id: 'activity', labelKey: 'admin-email-delivery-tab-activity' },
];
</script>

<template>
  <div class="flex-1">
    <div class="flex flex-col gap-6 px-4 sm:px-6 py-4 mx-auto w-full max-w-8xl">
      <header class="flex flex-col gap-1">
        <h1 class="text-xl sm:text-2xl font-bold text-primary">{{ t('admin-email-delivery-title') }}</h1>
        <p class="text-secondary">{{ t('admin-email-delivery-description') }}</p>
      </header>

      <nav class="flex gap-1 border-b border-default" role="tablist">
        <button
          v-for="tb in tabs"
          :key="tb.id"
          type="button"
          role="tab"
          :aria-selected="tab === tb.id"
          class="px-3 py-2 text-sm font-medium border-b-2 -mb-px transition-colors"
          :class="tab === tb.id ? 'border-accent text-primary' : 'border-transparent text-secondary hover:text-primary'"
          @click="selectTab(tb.id)"
        >
          {{ t(tb.labelKey) }}
        </button>
      </nav>

      <div v-if="tab === 'setup'" class="flex flex-col gap-6">
        <EmailSettingsView embedded />
        <EmailSendingDomainView embedded />
      </div>
      <div v-else class="flex flex-col gap-8">
        <section class="flex flex-col gap-3">
          <h2 class="text-base font-semibold text-primary">{{ t('admin-email-queue-title') }}</h2>
          <EmailQueueView embedded />
        </section>
        <section class="flex flex-col gap-3">
          <h2 class="text-base font-semibold text-primary">{{ t('admin-suppressions-title') }}</h2>
          <EmailSuppressionsView embedded />
        </section>
      </div>
    </div>
  </div>
</template>
