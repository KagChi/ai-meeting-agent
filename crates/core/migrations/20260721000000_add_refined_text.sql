-- Persist LLM-refined full transcript on each transcript version.
ALTER TABLE transcript_versions ADD COLUMN refined_text TEXT;
