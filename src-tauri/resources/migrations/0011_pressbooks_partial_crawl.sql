-- Make a Pressbooks Catalog crawl resumable, and keep it honest about what a
-- withdrawn book means.
--
-- Before this, a crawl fetched every page, then deleted the Catalog's rows and
-- rewrote them in one transaction. That is why a book withdrawn from the
-- Catalog left the reader's view: the delete took it. It is also why an
-- interruption cost the whole crawl -- three hundred requests for the largest
-- bundled Catalog, with nothing kept.
--
-- Rows are now written as pages arrive, so the delete cannot happen up front:
-- an interruption would leave a reader who had a whole Catalog holding ten
-- books. Instead every row a crawl writes is stamped with that crawl's
-- identity, and only a crawl that reaches its last page deletes what it did not
-- stamp. A withdrawn book therefore still leaves the reader's view, one
-- complete crawl later, and an interrupted crawl leaves the previous listing
-- intact underneath the part it has fetched.
--
-- `crawl_stamp` is on the network rather than derived per run because a resumed
-- crawl continues a run that a previous session started: the stamp has to
-- survive the process that created it, or a resume would be unable to tell its
-- own rows from the ones it is replacing.

ALTER TABLE pressbooks_book ADD COLUMN seen_at TEXT;

ALTER TABLE pressbooks_network ADD COLUMN crawl_stamp TEXT;

-- Rows already on disk were written by the all-or-nothing crawl, so they are a
-- complete Catalog as of whenever it ran. `synced_pages` already equals
-- `total_pages` for them, which is what marks a Catalog complete, so they need
-- no stamp to be read correctly -- and giving them one would claim they belong
-- to a crawl that never happened.
