# Take-off description layout correction

The description and quantity basis now use separate lines. Both can wrap without a line limit.
The take-off grid measures each row after layout. Its minimum height is 72 px.
Column widths and the default panel width stay available. Other grids keep fixed row heights.

Files: src/shell/TakeoffPanel.tsx, src/shell/theme.css, src/ui/DataGrid.tsx, src/shell/TakeoffPanel.test.tsx.
The source already contains accepted quantity-basis changes. This correction changes presentation only.
See source-fingerprints.json for the exact review snapshot. No commit or release was made.

Verification: 22 tests pass in TakeoffPanel.test.tsx and DataGrid.test.tsx.
The tests include English and Arabic names, count limits, and unspecified area basis.
TypeScript passes. These checks do not measure browser text or prove desktop acceptance.

A native Tauri attempt used an isolated 1440×900 Xvfb display and copies of new.dxf and new.qiyas.
The inspector connection timed out. No usable native layout or screenshot result was obtained.
The old run-owned Xvfb files were removed before another attempt. All temporary app and server processes were stopped.

Review: Engineering Lead must review row measurement and the take-off layout once.
Product UX Designer must confirm the text arrangement.
After review, Qiyas Tester must repeat the existing QSM-452 native cases with the default panel in both languages.
Check that each description button has positive width. Check full area basis and count limits.
Resize the panel, edit a long name, sort, and scroll. Check row overlap and keyboard navigation.
Capture new screenshots and layout bounds. Keep desktop acceptance separate from source review.
