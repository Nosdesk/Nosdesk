-- Purging a user keeps their loans. The borrower goes NULL rather than the
-- ledger rows cascading away; purge_user returns any active loan first.
-- DDL only: no row changes, so the audit trigger does not fire.
ALTER TABLE public.asset_loans ALTER COLUMN borrower_user_uuid DROP NOT NULL;

ALTER TABLE public.asset_loans
    DROP CONSTRAINT asset_loans_borrower_user_uuid_fkey,
    ADD CONSTRAINT asset_loans_borrower_user_uuid_fkey
        FOREIGN KEY (borrower_user_uuid) REFERENCES public.users(uuid) ON DELETE SET NULL;
