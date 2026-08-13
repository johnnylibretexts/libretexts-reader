-- The bundle identifier changed from dev.johnnyrobot.reader to
-- dev.johnnylibretexts.reader, which moves the app-data directory. Image and
-- cover paths are persisted absolute (see content/images.rs), so the stored
-- rows must be rebased or every figure silently fails to render.
-- Guarded by LIKE on the old prefix: idempotent forward, inert once rebased.

UPDATE section_images
   SET local_path = replace(local_path,
       'dev.johnnyrobot.reader', 'dev.johnnylibretexts.reader')
 WHERE local_path LIKE '%dev.johnnyrobot.reader%';

UPDATE documents
   SET cover_image_path = replace(cover_image_path,
       'dev.johnnyrobot.reader', 'dev.johnnylibretexts.reader')
 WHERE cover_image_path LIKE '%dev.johnnyrobot.reader%';
