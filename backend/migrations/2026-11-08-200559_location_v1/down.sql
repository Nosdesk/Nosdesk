DROP TABLE IF EXISTS public.asset_locations;
DROP TABLE IF EXISTS public.ticket_locations;

ALTER TABLE public.user_addresses
    DROP CONSTRAINT IF EXISTS user_addresses_geocode_precision_check,
    DROP CONSTRAINT IF EXISTS user_addresses_coordinates_check,
    DROP COLUMN IF EXISTS geocoded_hash,
    DROP COLUMN IF EXISTS geocoded_at,
    DROP COLUMN IF EXISTS geocode_place_confirmed,
    DROP COLUMN IF EXISTS geocode_source,
    DROP COLUMN IF EXISTS geocode_precision,
    DROP COLUMN IF EXISTS longitude,
    DROP COLUMN IF EXISTS latitude;

-- Postgres does not drop enum values; 'ticket_location' and 'asset_location'
-- stay, unused.
