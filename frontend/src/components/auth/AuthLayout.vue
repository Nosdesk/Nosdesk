<!--
Full-page split-screen shell for the unauthenticated pages (login,
onboarding). Left panel holds the brand logo, the form (vertically
centred), and an optional legal/footer line; the right panel is a fixed
dark brand hero with a slow animated gradient mesh, a status pill, and a
bottom tagline. The hero is hidden under `lg`, so on mobile the form
panel is a normal single column.

The hero is intentionally dark in every theme (a deliberate brand panel),
so its text uses explicit light colours rather than the theme tokens
(which flip in light mode). The mesh blobs are tinted with the accent
token, so workspace branding carries through. Motion is gated behind
`prefers-reduced-motion`.

Slots:
  - default      the form column (header + form + secondary links)
  - logo         brand mark, top of the form panel (defaults to LogoIcon)
  - footer       small print under the form (optional)
  - pill         hero status-pill label (defaults to "Self-hosted")
  - hero-title   hero headline
  - hero-subtitle hero supporting line
-->
<script setup lang="ts">
import LogoIcon from '@/components/icons/LogoIcon.vue';
</script>

<template>
  <div class="flex h-screen overflow-hidden bg-app">
    <!-- Form panel. The section owns the scroll; the inner wrapper is
         min-h-full so short forms centre vertically while tall ones (e.g.
         onboarding with its info cards) flow from the top and scroll, all
         within the panel so the hero stays pinned to the viewport. -->
    <section class="relative w-full overflow-y-auto lg:max-w-[560px] lg:basis-[45%]">
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
    <aside class="auth-hero relative hidden flex-1 overflow-hidden lg:block">
      <div class="auth-mesh" aria-hidden="true">
        <span class="blob blob-1"></span>
        <span class="blob blob-2"></span>
        <span class="blob blob-3"></span>
      </div>

      <!-- Brand radial accent + edge fades for legibility -->
      <div class="hero-radial pointer-events-none absolute inset-0"></div>
      <div class="hero-fade-left pointer-events-none absolute inset-y-0 left-0 w-40"></div>
      <div class="hero-fade-bottom pointer-events-none absolute inset-x-0 bottom-0 h-1/2"></div>

      <!-- Status pill -->
      <div
        class="absolute right-12 top-12 z-10 flex items-center gap-2 rounded-full border border-white/10 bg-white/5 px-3 py-1.5 text-xs font-medium text-white/70 backdrop-blur-sm xl:right-16 xl:top-16"
      >
        <span class="relative flex h-2 w-2">
          <span class="absolute inline-flex h-full w-full animate-ping rounded-full bg-accent opacity-60"></span>
          <span class="relative inline-flex h-2 w-2 rounded-full bg-accent"></span>
        </span>
        <slot name="pill">Self-hosted</slot>
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
        <h2 class="text-3xl font-semibold tracking-tight text-white">
          <slot name="hero-title" />
        </h2>
        <p class="text-base font-medium tracking-tight text-white/60">
          <slot name="hero-subtitle" />
        </p>
      </div>
    </aside>
  </div>
</template>

<style scoped>
/* Fixed dark brand panel regardless of the active theme. */
.auth-hero {
  background-color: #08090a;
}

.auth-mesh {
  position: absolute;
  inset: 0;
}

.blob {
  position: absolute;
  border-radius: 9999px;
  filter: blur(70px);
  will-change: transform;
}

.blob-1 {
  width: 32rem;
  height: 32rem;
  top: -8rem;
  right: -5rem;
  background: var(--color-accent);
  opacity: 0.22;
}

.blob-2 {
  width: 28rem;
  height: 28rem;
  bottom: -6rem;
  left: -4rem;
  background: var(--color-accent);
  opacity: 0.12;
}

.blob-3 {
  width: 24rem;
  height: 24rem;
  top: 38%;
  left: 32%;
  background: #ffffff;
  opacity: 0.04;
}

/* Brand radial glow from the top-right, mirroring the dashboard hero. */
.hero-radial {
  background: radial-gradient(
    ellipse 900px 600px at 82% -10%,
    color-mix(in srgb, var(--color-accent) 18%, transparent),
    transparent 70%
  );
}

/* Seam fade so the panel edge melts into the form column. */
.hero-fade-left {
  background: linear-gradient(to right, #08090a, transparent);
}

/* Bottom fade keeps the tagline readable over the mesh. */
.hero-fade-bottom {
  background: linear-gradient(to top, #08090a, rgba(8, 9, 10, 0.4), transparent);
}

@media (prefers-reduced-motion: no-preference) {
  .blob-1 {
    animation: auth-drift-1 26s ease-in-out infinite;
  }
  .blob-2 {
    animation: auth-drift-2 32s ease-in-out infinite;
  }
  .blob-3 {
    animation: auth-drift-3 38s ease-in-out infinite;
  }
}

@keyframes auth-drift-1 {
  0%,
  100% {
    transform: translate(0, 0) scale(1);
  }
  50% {
    transform: translate(-3.5rem, 2.5rem) scale(1.12);
  }
}

@keyframes auth-drift-2 {
  0%,
  100% {
    transform: translate(0, 0) scale(1);
  }
  50% {
    transform: translate(3rem, -2rem) scale(1.08);
  }
}

@keyframes auth-drift-3 {
  0%,
  100% {
    transform: translate(0, 0) scale(1);
  }
  50% {
    transform: translate(2.5rem, 3rem) scale(1.15);
  }
}
</style>
