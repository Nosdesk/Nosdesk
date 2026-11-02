-- When "New ticket" made this blank ticket for its requester. It stays blank
-- while nothing has touched it since, that is while updated_at still equals
-- blank_at, and "New ticket" opens it instead of making another.
ALTER TABLE public.tickets ADD COLUMN blank_at timestamptz;

CREATE INDEX tickets_blank_by_requester ON public.tickets (requester_uuid)
    WHERE blank_at IS NOT NULL;
