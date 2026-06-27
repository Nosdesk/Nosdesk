<template>
  <!-- min-h-dvh (dynamic viewport), not min-h-screen (100vh): the static vh
       unit can fall short of the layout viewport on a fresh load, leaving a
       grey gap below short pages (the html app-bg showing through). dvh tracks
       the visible area. Covers every guest/public page (submit-ticket, status,
       docs, help) since they all wrap this layout. -->
  <div class="min-h-dvh w-full flex flex-col items-center bg-app py-8 px-4 sm:px-6 gap-6">
    <!-- Brand (logo only, LogoIcon already contains the wordmark) -->
    <RouterLink
      to="/"
      class="flex items-center justify-center"
      :aria-label="t('public-layout-home-aria', { appName })"
    >
      <img
        v-if="customLogoUrl"
        :src="customLogoUrl"
        :alt="appName"
        class="h-10 max-w-[240px] object-contain"
      />
      <LogoIcon
        v-else
        class="h-10 text-accent"
        :aria-label="t('public-layout-logo-aria', { appName })"
      />
    </RouterLink>

    <!-- Page content (pages control their own width via contentClass) -->
    <div class="w-full flex flex-col gap-6" :class="contentClass">
      <slot />
    </div>

    <!-- Compact inline footer nav -->
    <nav
      v-if="footerLinks.length"
      class="flex items-center justify-center flex-wrap gap-x-6 gap-y-2 text-xs text-tertiary"
      :aria-label="t('public-layout-nav-aria')"
    >
      <RouterLink
        v-for="link in footerLinks"
        :key="link.to"
        :to="link.to"
        class="hover:text-secondary transition-colors"
      >{{ link.label }}</RouterLink>
    </nav>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted } from 'vue';
import { RouterLink } from 'vue-router';
import { useFluent } from 'fluent-vue';
import LogoIcon from '@/components/icons/LogoIcon.vue';
import { useBrandingStore } from '@/stores/branding';
import { useThemeStore } from '@/stores/theme';
import { usePublicSettingsStore } from '@nosdesk/core/stores/publicSettings';

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

withDefaults(
  defineProps<{
    /**
     * Tailwind max-width utility for the content column. Defaults to
     * max-w-md (448px) to match the application's auth-page aesthetic
     * (LoginView, AcceptInvitationView, MFASetupView).
     */
    contentClass?: string;
  }>(),
  { contentClass: 'max-w-md mx-auto' }
);

const brandingStore = useBrandingStore();
const themeStore = useThemeStore();
const publicSettings = usePublicSettingsStore();

const appName = computed(() => brandingStore.appName);
const customLogoUrl = computed(() =>
  brandingStore.getLogoUrl(themeStore.isDarkMode)
);

const footerLinks = computed(() => {
  // Sign-in is surfaced contextually on each page (submit form: "Already
  // have an account?", status page: "Need to reply? Sign in", etc.), so
  // keeping it in the footer too duplicates the affordance. Only
  // cross-page navigation lives here.
  const links: Array<{ to: string; label: string }> = [];
  if (publicSettings.settings?.guest_public_docs_enabled) {
    links.push({ to: '/docs', label: t('public-layout-docs-link') });
  }
  if (publicSettings.settings?.guest_help_page_enabled) {
    links.push({ to: '/help', label: t('public-layout-help-link') });
  }
  return links;
});

onMounted(() => {
  if (!brandingStore.isLoaded) {
    brandingStore.loadBranding();
  }
});
</script>
