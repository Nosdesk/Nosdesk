<!--
Full-page split-screen shell for the unauthenticated pages (login,
onboarding). Left panel holds the brand logo, the form (vertically
centred), and an optional legal/footer line; the right panel is a fixed
dark brand hero with a WebGL liquid-glass backdrop (AuthHeroCanvas), a
status pill, and a bottom tagline. The hero is hidden under `lg`, so on
mobile the form panel is a normal single column.

The hero follows the active theme: a dark brand panel in dark mode, a
light panel in light mode. Its base colour is shared with the WebGL canvas
via the `--hero-base` custom property so the CSS edge fades and the shader
panel match; the canvas re-composites its accent beams for a light base
(see AuthHeroCanvas). Text and the status pill use theme tokens, which now
read correctly because the panel polarity tracks the theme. The canvas
palette is driven by the accent token, so workspace branding carries
through. Motion is gated behind `prefers-reduced-motion`.

Slots:
  - default      the form column (header + form + secondary links)
  - logo         brand mark, top of the form panel (defaults to LogoIcon)
  - footer       small print under the form (optional)
  - pill         hero status-pill label (defaults to "Self-hosted")
  - hero-title   hero headline
  - hero-subtitle hero supporting line
-->
<script setup lang="ts">
import { computed } from 'vue';
import { useThemeStore } from '@/stores/theme';
import LogoIcon from '@/components/icons/LogoIcon.vue';
import AuthHeroCanvas from '@/components/auth/AuthHeroCanvas.vue';

withDefaults(
  defineProps<{
    /** Widen the form panel for content-heavy flows (e.g. MFA setup,
     *  whose QR + recovery-code grids need more room than a login form). */
    wide?: boolean;
  }>(),
  { wide: false },
);

// Hero panel base, shared with the canvas (which uses the same #f5f6f8 in
// its light-mode re-composite) and the CSS edge fades below. Dark mode
// keeps the near-black brand panel; light mode uses a light panel.
const themeStore = useThemeStore();
const heroStyle = computed(() => ({
  '--hero-base': themeStore.isDarkMode ? '#08090a' : '#f5f6f8',
}));
</script>

<template>
  <div class="flex h-screen overflow-hidden bg-app">
    <!-- Form panel. The section owns the scroll; the inner wrapper is
         min-h-full so short forms centre vertically while tall ones (e.g.
         onboarding with its info cards) flow from the top and scroll, all
         within the panel so the hero stays pinned to the viewport. -->
    <section
      class="relative w-full overflow-y-auto"
      :class="wide ? 'lg:max-w-[760px] lg:basis-[55%]' : 'lg:max-w-[560px] lg:basis-[45%]'"
    >
      <div
        class="flex min-h-full flex-col gap-10 px-6 py-10 sm:px-10 lg:px-16 lg:py-14"
      >
        <div class="w-fit">
          <slot name="logo">
            <LogoIcon class="h-9 w-auto text-accent" :aria-label="$t('nav-logo-alt')" />
          </slot>
        </div>

        <div class="flex flex-1 flex-col justify-center">
          <div class="w-full">
            <slot />
          </div>
        </div>

        <p v-if="$slots.footer" class="text-xs leading-relaxed text-tertiary">
          <slot name="footer" />
        </p>
      </div>
    </section>

    <!-- Brand hero (desktop only) -->
    <aside class="auth-hero relative hidden flex-1 overflow-hidden lg:block" :style="heroStyle">
      <!-- Drop the N when a view fills the hero itself (onboarding's
           getting-started column), keeping the lit backdrop; show it
           otherwise (login, MFA setup). -->
      <AuthHeroCanvas :show-logo="!$slots['hero-content']" />

      <!-- Edge fades for legibility -->
      <div class="hero-fade-left pointer-events-none absolute inset-y-0 left-0 w-40"></div>
      <div class="hero-fade-bottom pointer-events-none absolute inset-x-0 bottom-0 h-1/2"></div>

      <!-- Status pill (only when a view supplies a label) -->
      <div
        v-if="$slots.pill"
        class="absolute right-12 top-12 z-10 flex items-center gap-2 rounded-full border border-default bg-surface/60 px-3 py-1.5 text-xs font-medium text-secondary backdrop-blur-sm xl:right-16 xl:top-16"
      >
        <span class="relative flex h-2 w-2">
          <span class="absolute inline-flex h-full w-full animate-ping rounded-full bg-accent opacity-60"></span>
          <span class="relative inline-flex h-2 w-2 rounded-full bg-accent"></span>
        </span>
        <slot name="pill" />
      </div>

      <!-- Vertically-centred hero content (e.g. onboarding's
           getting-started column). Used instead of the bottom tagline. -->
      <div
        v-if="$slots['hero-content']"
        class="absolute inset-0 z-10 flex flex-col justify-center px-12 xl:px-16"
      >
        <div class="max-w-md">
          <slot name="hero-content" />
        </div>
      </div>

      <!-- Bottom brand tagline (login). Only rendered when supplied, so a
           view can opt into centred hero content instead. -->
      <div
        v-if="$slots['hero-title']"
        class="absolute inset-x-0 bottom-0 z-10 flex flex-col gap-3 p-12 xl:p-16"
      >
        <h2 class="text-3xl font-semibold tracking-tight text-primary">
          <slot name="hero-title" />
        </h2>
        <p class="text-base font-medium tracking-tight text-secondary">
          <slot name="hero-subtitle" />
        </p>
      </div>
    </aside>
  </div>
</template>

<style scoped>
/* Brand panel base, theme-driven via --hero-base (set inline in script) and
   shared with the WebGL canvas so the CSS fades and the shader panel match.
   Falls back to the dark brand colour if the variable is ever missing. */
.auth-hero {
  background-color: var(--hero-base, #08090a);
}

/* Seam fade so the panel edge melts into the form column. */
.hero-fade-left {
  background: linear-gradient(to right, var(--hero-base, #08090a), transparent);
}

/* Bottom fade keeps the tagline readable over the canvas. */
.hero-fade-bottom {
  background: linear-gradient(
    to top,
    var(--hero-base, #08090a),
    color-mix(in srgb, var(--hero-base, #08090a) 40%, transparent),
    transparent
  );
}
</style>
