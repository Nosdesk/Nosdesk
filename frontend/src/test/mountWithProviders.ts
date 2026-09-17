/**
 * Component-test harness: mounts a component under the same providers the
 * app installs in `App.vue` and `main.ts`, so a primitive that reads the
 * fluent plugin, Reka's ConfigProvider or the TooltipProvider renders as it
 * does in the app. Translations resolve to the message id (no catalogue is
 * loaded), which keeps assertions readable and locale-free.
 */
import { defineComponent, h, type Component } from 'vue'
import { mount, type VueWrapper } from '@vue/test-utils'
import { FluentBundle } from '@fluent/bundle'
import { createFluentVue } from 'fluent-vue'
import { ConfigProvider, TooltipProvider } from 'reka-ui'

export function mountWithProviders(
  component: Component,
  props: Record<string, unknown> = {},
  slots: Record<string, () => unknown> = {},
): VueWrapper {
  const fluent = createFluentVue({ bundles: [new FluentBundle('en-US')] })
  const Host = defineComponent({
    setup() {
      return () =>
        h(ConfigProvider, {}, () =>
          h(TooltipProvider, { delayDuration: 0 }, () => h(component, props, slots)),
        )
    },
  })
  return mount(Host, { attachTo: document.body, global: { plugins: [fluent] } })
}
