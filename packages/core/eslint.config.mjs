// Boundary guard for @nosdesk/core.
//
// The package is HEADLESS so it stays portable to the Tauri mobile app: it must
// not import app code (`@/...`), must not depend on the router (a frontend-shell
// concern), and must not touch DOM/browser globals. This config enforces only
// those invariants; general TS style is covered where the code is consumed.
// `vue` and `pinia` are intentionally allowed (core owns stores/queries).
import tseslint from 'typescript-eslint'

export default tseslint.config({
  files: ['src/**/*.ts'],
  languageOptions: { parser: tseslint.parser },
  rules: {
    'no-restricted-imports': [
      'error',
      {
        paths: [
          { name: 'vue-router', message: '@nosdesk/core is headless: no router dependency.' },
        ],
        patterns: [
          { group: ['@/*'], message: '@nosdesk/core must not import app code (@/...).' },
        ],
      },
    ],
    'no-restricted-globals': [
      'error',
      { name: 'window', message: '@nosdesk/core is headless: no DOM/browser globals.' },
      { name: 'document', message: '@nosdesk/core is headless: no DOM/browser globals.' },
      { name: 'localStorage', message: '@nosdesk/core is headless: no DOM/browser globals.' },
      { name: 'sessionStorage', message: '@nosdesk/core is headless: no DOM/browser globals.' },
      { name: 'navigator', message: '@nosdesk/core is headless: no DOM/browser globals.' },
    ],
  },
})
