-- Mail one address was sent. The request form's confirmation caps count it
-- across every workspace by lower(recipient), and the first mail a workspace
-- sends an address is found by it; without it both scan the whole queue.
CREATE INDEX IF NOT EXISTS outbound_emails_lower_recipient_idx
    ON public.outbound_emails (lower(recipient), created_at DESC);
