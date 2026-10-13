-- Fails while any loan has a NULL borrower (one left by a purge).
ALTER TABLE public.asset_loans
    DROP CONSTRAINT asset_loans_borrower_user_uuid_fkey,
    ADD CONSTRAINT asset_loans_borrower_user_uuid_fkey
        FOREIGN KEY (borrower_user_uuid) REFERENCES public.users(uuid) ON DELETE CASCADE;

ALTER TABLE public.asset_loans ALTER COLUMN borrower_user_uuid SET NOT NULL;
