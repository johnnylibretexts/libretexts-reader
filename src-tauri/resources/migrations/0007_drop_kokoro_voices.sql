-- Kokoro is removed. The voices table held only Kokoro voice embeddings
-- (55 .bin files from the Kokoro-82M ONNX repo); Supertonic's ten voice
-- styles are a static list and were never stored here.
DROP TABLE IF EXISTS voices;

-- model_precision and model_downloaded described the Kokoro ONNX file.
DELETE FROM settings WHERE key IN ('model_precision', 'model_downloaded');

-- Settings values are JSON, so a string's stored text includes its quotes.
UPDATE settings
   SET value = '"supertonic"'
 WHERE key = 'tts_provider'
   AND value = '"kokoro"';

-- A stored Kokoro voice id would otherwise be silently swapped for M1 by
-- playback_voice_style on every sentence: working audio, permanently wrong
-- setting, no error. Anything that is not one of Supertonic's ten styles is
-- rewritten to the default.
UPDATE settings
   SET value = '"M1"'
 WHERE key = 'default_voice_id'
   AND value NOT IN ('"M1"', '"M2"', '"M3"', '"M4"', '"M5"',
                     '"F1"', '"F2"', '"F3"', '"F4"', '"F5"');
