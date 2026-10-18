# DWG checks

- 2026-10-15: Check both opening and the first render batch. Record skipped
  records, entity counts and RSS. An open result alone does not prove that
  all drawing content is present. Keep private files and their names out of
  corpus reports. See `docs/dwg-corpus.md`.

- 2026-10-15: Embedded count rows need a record even with no geometry. Use an invisible carrier with an explicit record-only flag; never rebuild it as a counted location. Verify zero rows and metadata after two DWG/DXF saves without the project sidecar.
