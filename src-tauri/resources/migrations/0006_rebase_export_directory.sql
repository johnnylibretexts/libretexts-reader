-- The product name changed from "Johnny Reader" to "LibreTexts Reader", which
-- changes the default export directory. The chosen directory is persisted in
-- settings (JSON-encoded), so an existing install would keep pointing at the
-- old path. Matches on "/Johnny Reader" so a custom path chosen by the user is
-- left untouched. Idempotent forward, inert once rebased.

UPDATE settings
   SET value = replace(value, '/Johnny Reader', '/LibreTexts Reader')
 WHERE key = 'export_directory'
   AND value LIKE '%/Johnny Reader%';
