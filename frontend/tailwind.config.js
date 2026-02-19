/** @type {import('tailwindcss').Config} */
export default {
  darkMode: 'selector', // Tailwind v4 uses 'selector' for .dark class
  content: [
    "./index.html",
    "./src/**/*.{vue,js,ts,jsx,tsx}",
  ],
  theme: {
    extend: {
      zIndex: {
        'header':   '100',   // Site header bar
        'backdrop':  '200',   // Modal/bottom-sheet backdrop overlays
        'overlay':  '300',   // Modals, global search, toasts, notifications, tooltips
        'effect':   '400',   // Visual theme effects (CRT, snowfall)
        'cursor':   '500',   // Cursor effects (scanlines)
      },
    },
  },
  plugins: [
    require('@tailwindcss/typography'),
  ],
}
