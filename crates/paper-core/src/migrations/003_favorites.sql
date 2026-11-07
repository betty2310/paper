-- Migration 3: favorite papers, listed above the others in every view.
ALTER TABLE items ADD COLUMN favorite INTEGER NOT NULL DEFAULT 0;
