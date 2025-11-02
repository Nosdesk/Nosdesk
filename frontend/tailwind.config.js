/** @type {import('tailwindcss').Config} */
export default {
  darkMode: 'selector', // Tailwind v4 uses 'selector' for .dark class
  content: [
    "./index.html",
    "./src/**/*.{vue,js,ts,jsx,tsx}",
  ],
  plugins: [
    require('@tailwindcss/typography'),
  ],
} 