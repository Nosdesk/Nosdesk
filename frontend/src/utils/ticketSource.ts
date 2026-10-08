/**
 * The Fluent key naming the channel a ticket came in through, for the
 * ticket's Source row. `null` for a provider without a name yet, which the
 * row shows as is.
 */
export function ticketSourceLabelKey(provider: string): string | null {
  switch (provider) {
    case 'email_forward':
      return 'ticket-detail-source-email-forward'
    case 'email_managed':
      return 'ticket-detail-source-email-managed'
    case 'slack':
      return 'ticket-detail-source-slack'
    case 'teams':
      return 'ticket-detail-source-teams'
    default:
      // email_imap, email_smtp and any later mail provider.
      return provider.startsWith('email_') ? 'ticket-detail-source-email' : null
  }
}
