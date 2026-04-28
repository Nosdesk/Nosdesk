import type { Config } from 'tailwindcss'
import typography from '@tailwindcss/typography'

export default {
  darkMode: 'selector', // Tailwind v4 uses 'selector' for .dark class
  content: [
    './index.html',
    './src/**/*.{vue,js,ts,jsx,tsx}',
  ],
  theme: {
    extend: {
      // NOTE: Tailwind 4 ignores this entire `theme.extend` block,
      // configuration moved to CSS-side `@theme` directives in
      // `src/assets/main.css`. The z-index scale lives there as
      // `--z-*` custom properties, with hand-written utilities
      // (`.z-header`, `.z-overlay`, etc.) in `@layer utilities`
      // because Tailwind 4 has no `@theme` namespace for z-index.
    },
  },
  plugins: [typography],
} satisfies Config
