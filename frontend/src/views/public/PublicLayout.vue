<template>
  <div class="min-h-screen w-full flex flex-col items-center bg-app py-8 px-4 sm:px-6 gap-6">
    <!-- Brand (logo only — LogoIcon already contains the wordmark) -->
    <RouterLink
      to="/"
      class="flex items-center justify-center"
      :aria-label="`${appName} home`"
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
        :aria-label="`${appName} Logo`"
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
      aria-label="Public navigation"
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
import LogoIcon from '@/components/icons/LogoIcon.vue';
import { useBrandingStore } from '@/stores/branding';
import { useThemeStore } from '@/stores/theme';
import { usePublicSettingsStore } from '@/stores/publicSettings';

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
    links.push({ to: '/docs', label: 'Documentation' });
  }
  if (publicSettings.settings?.guest_help_page_enabled) {
    links.push({ to: '/help', label: 'Help' });
  }
  return links;
});

onMounted(() => {
  if (!brandingStore.isLoaded) {
    brandingStore.loadBranding();
  }
});
</script>
