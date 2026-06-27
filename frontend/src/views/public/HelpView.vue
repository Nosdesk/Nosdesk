<template>
  <PublicLayout content-class="max-w-lg mx-auto w-full">
    <!-- No loader: the settings call is brief and the help cards render
         optimistically. The disabled notice only appears once the flag
         resolves and is confirmed off. -->
    <FeatureDisabledNotice
      v-if="!loading && !enabled"
      :title="t('help-disabled-title')"
      :message="t('help-disabled-message')"
    />

    <template v-else>
      <div class="flex flex-col gap-1 text-center">
        <h1 class="text-xl sm:text-2xl font-bold text-primary">{{ t('help-heading') }}</h1>
        <p class="text-sm text-secondary">{{ t('help-tagline') }}</p>
      </div>

      <div class="grid gap-3 sm:grid-cols-2">
        <RouterLink
          v-for="card in cards"
          :key="card.to"
          :to="card.to"
          class="group bg-surface border border-default rounded-xl shadow-sm p-4 hover:border-accent transition-colors flex items-start gap-3"
        >
          <div
            class="shrink-0 w-9 h-9 rounded-lg flex items-center justify-center"
            :class="card.iconBg"
          >
            <component :is="card.icon" class="w-4 h-4" :class="card.iconColor" />
          </div>
          <div class="flex-1 min-w-0 flex flex-col gap-0.5">
            <div class="text-primary font-semibold text-sm group-hover:text-accent transition-colors">
              {{ card.title }}
            </div>
            <p class="text-xs text-secondary">{{ card.description }}</p>
          </div>
        </RouterLink>
      </div>
    </template>
  </PublicLayout>
</template>

<script setup lang="ts">
import { h, ref, computed, onMounted } from 'vue';
import { RouterLink } from 'vue-router';
import { useFluent } from 'fluent-vue';
import PublicLayout from './PublicLayout.vue';
import FeatureDisabledNotice from './FeatureDisabledNotice.vue';
import { usePublicSettingsStore } from '@nosdesk/core/stores/publicSettings';

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

const store = usePublicSettingsStore();
const loading = ref(true);
const enabled = computed(() => store.settings?.guest_help_page_enabled === true);

const TicketIcon = () =>
  h('svg', { fill: 'none', stroke: 'currentColor', viewBox: '0 0 24 24' }, [
    h('path', {
      'stroke-linecap': 'round',
      'stroke-linejoin': 'round',
      'stroke-width': 2,
      d: 'M15 5v2m0 4v2m0 4v2M5 5a2 2 0 00-2 2v3a2 2 0 110 4v3a2 2 0 002 2h14a2 2 0 002-2v-3a2 2 0 110-4V7a2 2 0 00-2-2H5z'
    })
  ]);

const DocIcon = () =>
  h('svg', { fill: 'none', stroke: 'currentColor', viewBox: '0 0 24 24' }, [
    h('path', {
      'stroke-linecap': 'round',
      'stroke-linejoin': 'round',
      'stroke-width': 2,
      d: 'M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z'
    })
  ]);

const KeyIcon = () =>
  h('svg', { fill: 'none', stroke: 'currentColor', viewBox: '0 0 24 24' }, [
    h('path', {
      'stroke-linecap': 'round',
      'stroke-linejoin': 'round',
      'stroke-width': 2,
      d: 'M15 7a2 2 0 012 2m4 0a6 6 0 01-7.743 5.743L11 17H9v2H7v2H4a1 1 0 01-1-1v-2.586a1 1 0 01.293-.707l5.964-5.964A6 6 0 1121 9z'
    })
  ]);

const SignInIcon = () =>
  h('svg', { fill: 'none', stroke: 'currentColor', viewBox: '0 0 24 24' }, [
    h('path', {
      'stroke-linecap': 'round',
      'stroke-linejoin': 'round',
      'stroke-width': 2,
      d: 'M11 16l-4-4m0 0l4-4m-4 4h14m-5 4v1a3 3 0 01-3 3H6a3 3 0 01-3-3V7a3 3 0 013-3h7a3 3 0 013 3v1'
    })
  ]);

const cards = computed(() => {
  const list: Array<{
    to: string;
    title: string;
    description: string;
    icon: ReturnType<typeof h>;
    iconBg: string;
    iconColor: string;
  }> = [];

  if (store.settings?.guest_tickets_enabled) {
    list.push({
      to: '/submit-ticket',
      title: t('help-card-submit-title'),
      description: t('help-card-submit-desc'),
      icon: TicketIcon(),
      iconBg: 'bg-accent-muted',
      iconColor: 'text-accent'
    });
  }
  if (store.settings?.guest_public_docs_enabled) {
    list.push({
      to: '/docs',
      title: t('help-card-docs-title'),
      description: t('help-card-docs-desc'),
      icon: DocIcon(),
      iconBg: 'bg-status-info-muted',
      iconColor: 'text-status-info'
    });
  }
  list.push({
    to: '/reset-password',
    title: t('help-card-reset-title'),
    description: t('help-card-reset-desc'),
    icon: KeyIcon(),
    iconBg: 'bg-status-warning-muted',
    iconColor: 'text-status-warning'
  });
  list.push({
    to: '/login',
    title: t('help-card-signin-title'),
    description: t('help-card-signin-desc'),
    icon: SignInIcon(),
    iconBg: 'bg-surface-alt',
    iconColor: 'text-secondary'
  });
  return list;
});

onMounted(async () => {
  await store.load();
  loading.value = false;
});
</script>
