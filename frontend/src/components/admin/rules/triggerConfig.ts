/**
 * A rule's `trigger_config` as the editor's form holds it, and back. An
 * updated trigger names the fields whose change runs it (none named: any
 * change); a replied trigger names whose reply runs it; a plugin event
 * trigger names the plugin and the event of its that runs it. Field names,
 * a reply kind, a plugin's uuid and an event name are all it holds.
 */
import type { RuleReplyKind, RuleTriggerConfig, RuleTriggerKind } from '@nosdesk/core/types/rule'

export interface TriggerSettings {
  changedFields: string[]
  replyKind: RuleReplyKind
  /** The plugin a plugin event rule runs on; empty until one is picked. */
  pluginUuid: string
  /** The plugin's event that runs it; empty until one is picked. */
  pluginEvent: string
}

const REPLY_KINDS: RuleReplyKind[] = ['customer', 'agent', 'any']

export const DEFAULT_REPLY_KIND: RuleReplyKind = 'customer'

export function readTriggerConfig(config: RuleTriggerConfig | null | undefined): TriggerSettings {
  const fields = config?.changed_fields_include
  const reply = config?.reply_kind
  return {
    changedFields: Array.isArray(fields) ? fields.filter((f): f is string => typeof f === 'string') : [],
    replyKind: reply && REPLY_KINDS.includes(reply) ? reply : DEFAULT_REPLY_KIND,
    pluginUuid: typeof config?.plugin_uuid === 'string' ? config.plugin_uuid : '',
    pluginEvent: typeof config?.event_type === 'string' ? config.event_type : '',
  }
}

export function triggerConfigFor(kind: RuleTriggerKind, settings: TriggerSettings): RuleTriggerConfig {
  if (kind === 'ticket_updated' && settings.changedFields.length > 0) {
    return { changed_fields_include: [...settings.changedFields] }
  }
  if (kind === 'ticket_replied') return { reply_kind: settings.replyKind }
  if (kind === 'plugin_event') {
    return {
      ...(settings.pluginUuid ? { plugin_uuid: settings.pluginUuid } : {}),
      ...(settings.pluginEvent ? { event_type: settings.pluginEvent } : {}),
    }
  }
  return {}
}

/** What the trigger still needs before the rule can be saved, as a message
 *  key: a plugin event rule names a plugin and one of its events. */
export function triggerProblem(kind: RuleTriggerKind, settings: TriggerSettings): string | null {
  if (kind === 'plugin_event' && (!settings.pluginUuid || !settings.pluginEvent)) {
    return 'admin-rule-editor-plugin-incomplete'
  }
  return null
}
