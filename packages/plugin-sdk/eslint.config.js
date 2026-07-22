// @nosdesk/plugin-sdk runs as browser/iframe code (DOM globals like `window`,
// `MessageEvent`, `HTMLElement` are expected: it is the iframe-side bridge), so
// unlike headless @nosdesk/core it does not restrict DOM access. Basic TS lint.
import tseslint from 'typescript-eslint';

export default tseslint.config({
  files: ['src/**/*.ts'],
  languageOptions: { parser: tseslint.parser },
  extends: [tseslint.configs.recommended],
});
