-- Per-segment LLM-refined text (toggle Raw | Refined in UI).
ALTER TABLE transcript_segments ADD COLUMN refined_text TEXT;
