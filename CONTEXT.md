# LibreTexts Reader

LibreTexts Reader turns written works — textbooks, EPUBs, PDFs, articles, pasted text — into something you can listen to, entirely on your own machine.

## Language

### Content

**Library**:
The set of works a reader has imported onto their machine. There is exactly one, and it is the reader's.
_Avoid_: collection, shelf, bookshelf; never a LibreTexts subdomain or a Pressbooks network — those are Catalogs

**Document**:
A single imported work, whatever it was imported from.
_Avoid_: book, file, text, item

**Section**:
An addressable division of a Document that a reader can open and listen to as a unit.
_Avoid_: chapter, page, unit

**Paragraph**:
The smallest run of text preserved from the source, in reading order.
_Avoid_: block, node, element

**Sentence**:
The unit of listening — what gets highlighted, what advances, what a reader can seek to.
_Avoid_: utterance, chunk, segment

**Display Text**:
The form of a Paragraph a reader sees, with mathematics preserved for rendering.
_Avoid_: raw text, source text

**Speech Text**:
The form of a Paragraph a Speech Engine is given, with mathematics and notation written out in words. The same Paragraph always has both forms; neither is derived from the other at read time.
_Avoid_: normalized text, TTS text

**Figure**:
An image kept from the source and anchored to the Paragraph it followed there.
_Avoid_: image, illustration, asset

**Import**:
Turning a source into a Document. Preserves reading order and figures; it is not a reproduction of the source's layout.
_Avoid_: ingest, parse, scrape

**Source**:
Where a work comes from — OpenStax, LibreTexts, Pressbooks, or a file or URL the reader supplies.
_Avoid_: provider, publisher, origin, vendor

**Catalog**:
A browsable index of works offered by a Source, before any of them are imported. One Source may offer many; what a Source calls its own catalogs — LibreTexts libraries, Pressbooks networks — is its word, shown to the reader as-is.
_Avoid_: index, listing, store

### Speech

**Speech Engine**:
A thing that can turn a Sentence into audio. Which one is in use is the reader's choice, and every Speech Engine is interchangeable from the rest of the app's point of view.
_Avoid_: provider, TTS provider, backend, model

**Voice**:
A selectable speaking identity. Only the Speech Engine that offers a Voice knows what its identifier means.
_Avoid_: voice style, speaker, narrator

**Chunk**:
A batch of text handed to a Speech Engine in one go. A Chunk is sized for the engine, not for the reader — it is not a Sentence.
_Avoid_: batch, piece, span

**Chapter Export**:
Rendering a whole Section to an audio file the reader keeps.
_Avoid_: download, render, bounce
