-- Square WebP thumbnails for asset media grid rendering.
ALTER TABLE public.asset_media
    ADD COLUMN thumbnail_url character varying(2048);
