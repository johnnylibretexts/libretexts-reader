# Importers do not share a common entry shape

The five importers — OpenStax, LibreTexts, EPUB, PDF, article — have five different entry signatures: different names, different arities, two different progress-callback types, three async and two sync. They converge only at `DocumentBuilder`. This reads like an obvious refactor waiting to happen, and it is deliberately left alone.

The formats differ in kind, not in detail. PDF reconstructs headings from font-size ratios and rebuilds paragraphs from line geometry, with no HTML anywhere in its path. LibreTexts needs two entirely separate table-of-contents strategies depending on whether the Deki API returns 401/403 for a given book. A trait wide enough to cover all five would be almost entirely optional parameters, and would explain nothing about what any importer actually does.

What genuinely *is* shared — walking HTML into paragraphs, images, anchors and captions — is factored into one module that takes a per-source skip rule. That is where the duplication was, and that is the only place it's worth removing.

Revisit if a sixth importer arrives that is a near-copy of an existing one, or if progress reporting is unified across all six import paths and the callback shapes have to agree anyway.
