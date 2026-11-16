// @vitest-environment node
/**
 * The file-download lint rule: a raw `download` link, in a template or built
 * in a script, is an error; saving through the platform isn't.
 */
import { describe, it } from 'vitest'
import { RuleTester } from 'eslint'
import tseslint from 'typescript-eslint'
import pluginVue from 'eslint-plugin-vue'
import rule from '../../../scripts/eslint-rules/file-download.mjs'

// The frontend reaches vue-eslint-parser through eslint-plugin-vue.
const vueParser = pluginVue.configs['flat/base'].find((c) => c.languageOptions?.parser)!.languageOptions!.parser

RuleTester.describe = describe
RuleTester.it = it

const error = { messageId: 'rawDownload' }
const sfc = (template: string) => ({
  code: `<template>${template}</template>`,
  filename: 'Example.vue',
})

new RuleTester({
  languageOptions: { parser: tseslint.parser, ecmaVersion: 'latest', sourceType: 'module' },
}).run('file-download (script)', rule, {
  valid: [
    "await saveFile(blob, 'report.csv')",
    "link.href = url",
    "const { download } = useThing(); download()",
    "el.setAttribute('href', url)",
  ],
  invalid: [
    { code: "link.download = 'report.csv'", errors: [error] },
    { code: "link['download'] = name", errors: [error] },
    { code: "a.setAttribute('download', name)", errors: [error] },
  ],
})

new RuleTester({
  languageOptions: {
    parser: vueParser,
    parserOptions: { parser: tseslint.parser },
    ecmaVersion: 'latest',
    sourceType: 'module',
  },
}).run('file-download (template)', rule, {
  valid: [
    sfc('<DownloadLink :href="file.url" :filename="file.name"><Icon name="download" /></DownloadLink>'),
    sfc('<a :href="url" target="_blank">Docs</a>'),
    sfc('<Button @click="download">Download</Button>'),
  ],
  invalid: [
    { ...sfc('<a :href="url" download>Save</a>'), errors: [error] },
    { ...sfc('<a :href="url" :download="name">Save</a>'), errors: [error] },
    { ...sfc('<LinkButton :href="src" v-bind:download="filename">Save</LinkButton>'), errors: [error] },
  ],
})
