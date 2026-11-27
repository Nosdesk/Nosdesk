import type { Comment } from '@nosdesk/core/types/comment'

// Helpdesks an import brings replies from, by the id the import records.
const IMPORT_SOURCES: Record<string, string> = {
  zendesk: 'Zendesk',
  freshdesk: 'Freshdesk',
  freshservice: 'Freshservice',
}

/**
 * Who wrote an imported reply at the source, when the name shown isn't
 * theirs: a reply credited to the import's placeholder author, or one whose
 * account goes by another name here. Null otherwise, and for any reply that
 * wasn't imported. Never their address.
 */
export function importedAuthor(
  comment: Pick<Comment, 'channel_metadata'> & { user?: { name?: string } | null },
): { name: string; source: string } | null {
  const imported = comment.channel_metadata?.import
  const name = typeof imported?.author_name === 'string' ? imported.author_name.trim() : ''
  if (!name || name === (comment.user?.name ?? '').trim()) return null
  const id = typeof imported?.source === 'string' ? imported.source : ''
  return { name, source: IMPORT_SOURCES[id] ?? id }
}
