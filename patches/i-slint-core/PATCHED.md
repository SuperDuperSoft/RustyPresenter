# i-slint-core 1.18.1, patched

The published crate, unchanged except `translations.rs`, which gets a run-time translation hook:

- `pub type TranslationOverride = fn(&str, Option<&str>, i32) -> Option<String>` and
  `pub fn set_translation_override(Option<TranslationOverride>)` (with the `std` feature, which Slint turns on).
- `translate_from_bundle` and `translate_from_bundle_with_plural` (what the generated code calls for every `@tr`
  when the `.slint` files are built with bundled translations) ask that function first, with the English text
  (`strs[0]`), the English plural and the number. When it returns `Some`, that text is shown, formatted with the same
  arguments; otherwise the bundle is used as before. They still read the language property first, so
  `mark_all_translations_dirty()` (already public) makes every `@tr` ask again.

Why: Slint 1.18 only shows translations compiled into the program. RustyPresenter lets volunteers edit a translation
or add a language inside the app (Settings → Translations…, `src/i18n/mod.rs`), and the change must show at once
without a rebuild. The app sets the hook in `i18n::sync_slint` and marks the translations dirty after each edit.

The rest is a byte-for-byte copy of the crates.io 1.18.1 package (only its `Cargo.lock`, `.cargo-checksum.json` and
`.cargo_vcs_info.json` left out), so it builds offline from this folder; `Cargo.lock` pins it as a path package at
exactly 1.18.1, the version every `i-slint-*` 1.18.1 crate requires with `=1.18.1`. No new crates. The licence is the
crate's own (`GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0`, texts in
`LICENSES/`), the same terms as the rest of Slint that the app already uses; the copyright headers are kept.

To check the copy: `git diff --no-index <cargo registry>/i-slint-core-1.18.1 patches/i-slint-core` shows only
`translations.rs` (and this file).

Used through `[patch.crates-io]` in the top-level `Cargo.toml`. When moving to a newer Slint: copy that release's
crate here and add the hook again, or drop this folder and that entry if Slint can load translations at run time by
then (follow slint-ui/slint discussions on "runtime translations"); then `i18n::sync_slint` must use that instead.
