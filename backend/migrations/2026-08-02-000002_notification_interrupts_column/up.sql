-- Persist whether each notification interrupted (toast / desktop) vs landed
-- quietly in the bell. Previously this was computed per-send and only put on
-- the SSE payload; storing it lets the send path count a recipient's recent
-- interrupts to cap bursts (protect the interrupt channel without dropping
-- bell records). Constant DEFAULT is a metadata-only add (no row rewrite, no
-- per-row UPDATE), so it does not fire the audit trigger.
ALTER TABLE notifications
    ADD COLUMN interrupts BOOLEAN NOT NULL DEFAULT true;
