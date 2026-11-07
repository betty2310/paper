-- Migration 2: citation keys are unique without regard to case, because
-- BibTeX matches \cite keys case-insensitively. The column from migration 1
-- was never filled, so no existing row can clash.

CREATE UNIQUE INDEX items_citation_key ON items(citation_key COLLATE NOCASE)
  WHERE citation_key IS NOT NULL;
