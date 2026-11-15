/**
 * A rule's `trigger_config` as the editor's form holds it, and back. An
 * updated trigger names the fields whose change runs it (none named: any
 * change); a replied trigger names whose reply runs it. Field names and a
 * reply kind are all it holds: no ids.
 */
import type { RuleReplyKind, RuleTriggerConfig, RuleTriggerKind } from '@nosdesk/core/types/rule'

export interface TriggerSettings {
  changedFields: string[]
  replyKind: RuleReplyKind
}

const REPLY_KINDS: RuleReplyKind[] = ['customer', 'agent', 'any']

export const DEFAULT_REPLY_KIND: RuleReplyKind = 'customer'

export function readTriggerConfig(config: RuleTriggerConfig | null | undefined): TriggerSettings {
  const fields = config?.changed_fields_include
  const reply = config?.reply_kind
  return {
    changedFields: Array.isArray(fields) ? fields.filter((f): f is string => typeof f === 'string') : [],
    replyKind: reply && REPLY_KINDS.includes(reply) ? reply : DEFAULT_REPLY_KIND,
  }
}

export function triggerConfigFor(kind: RuleTriggerKind, settings: TriggerSettings): RuleTriggerConfig {
  if (kind === 'ticket_updated' && settings.changedFields.length > 0) {
    return { changed_fields_include: [...settings.changedFields] }
  }
  if (kind === 'ticket_replied') return { reply_kind: settings.replyKind }
  return {}
}
