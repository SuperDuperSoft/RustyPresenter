# I18N — RustyPresenter in any language: implementation plan

Spec: `docs/superpowers/specs/2026-11-06-i18n-any-language-design.md`. Branch: `i18n` (off `merge-all`), local only (no GitHub: Jake's GitHub budget is spent).

Rules for every task: TDD (test first, see it fail, then code); `cargo test --offline` in the cloud sandbox recipe (STATUS.md "Build limits"); never translate ids, log lines, keys, command names or church content; keep English text byte-for-byte the same so existing tests still pass in English.

## Task 1 — `i18n` core (no UI)
Files: `src/i18n/{mod.rs,po.rs,plural.rs}`, `src/lib.rs` (`pub mod i18n;`), `build.rs` (bundle list), `Cargo.toml` (`sys-locale`, already in `Cargo.lock` through Slint).
1. Tests for `po::parse` (spec §5 list), then the parser.
2. Tests for `plural::Rule::parse/eval` (en, fr, ru, uk, pl, ar, ja, bad), then the evaluator.
3. Tests for `tr`/`trn`/`trc` + macros with an in-test catalog (`i18n::testing::with_catalog`), then the registry, `set_language`, `current`, `languages`, `system_language_from(&str)`.
4. `build.rs`: generate `i18n_bundle.rs`; `with_bundled_translations("translations")` and `with_default_translation_context(None)`; `cargo:rerun-if-changed=translations`.
5. Seed `translations/rustypresenter.pot` (empty but valid) and `translations/es/LC_MESSAGES/rustypresenter.po` (header only) so the bundle path works.

## Task 2 — extractor and catalog health tests
Files: `src/i18n/extract.rs` (pure functions: scan `.slint` for `@tr(...)`, Rust for `tr!/trn!/trc!`, build a `.pot`, merge into a `.po`), `examples/i18n_extract.rs` (writes files), `tests/i18n_catalogs.rs`.
- Tests: extraction of each `@tr` form (`"x"`, `"ctx" => "x"`, `"a" | "b" % n`, with `{}` args), Rust macro forms incl. raw strings and escaped quotes; merge keeps translations, adds new msgids empty, marks removed ones obsolete (`#~`).
- `i18n_pot_is_current`: extracted == committed `.pot` (message says "run `cargo run --example i18n_extract`").
- `every_po_is_valid`: parses, msgids ⊆ pot, placeholders equal (`{name}`, `{}`, `{0}`, `{n}`), plural count == `nplurals`.
- `slint_has_no_bare_user_text`: `text: "…"`, `title: "…"`, `label: "…"`, `placeholder-text: "…"`, `accessible-label: "…"` etc. with letters must be `@tr`; allowlist file `translations/allow-bare.txt` for symbols/proper nouns.

## Task 3 — Settings → Language and live switching
Files: `src/model.rs` (`Settings.ui_language`), `src/app.rs` (`settings/language`), `src/strings/settings.rs`, `ui/dialogs/settings.slint` (+ new `ui/dialogs/settings-language.slint`), `src/ui/dialogs/settings.rs`, start-up in `src/main.rs`/`src/ui/mod.rs`, stage/phone update payloads (`lang`).
- Tests first (spec §5 Settings + UI rows). Tab order: Look, **Language**, Songs, Bible, Video, Backups, Help, About (update index uses and tests).
- Font warning reuses `font_test_line`'s check.

## Task 4 — sweep `.slint` files to `@tr`
Split across workers by folder (`ui/operator/`, `ui/dialogs/` A–M, `ui/dialogs/` N–Z, rest of `ui/`). Only user-visible strings. Plurals in Slint use `@tr("{n} item" | "{n} items" % n)`.

## Task 5 — sweep Rust strings to `tr!`
Workers by area: `src/strings/*` (consts that are user text become `fn` returning `String`), `src/app.rs` toasts and errors shown to people, `src/ui/**`, `src/import/**`, `src/bible/**` + `src/scripture/**`, `src/automation/**`, `src/preflight.rs`, `src/bundle.rs`, `src/youtube/**`, room/stage built-in text. Tests keep passing in English.

## Task 6 — phone remote
`GET /i18n.json` (map English → current, only the strings `remote.js` uses, listed by `tr!` calls in `src/i18n/remote_strings.rs`), `remote.js` `t()` helper; re-fetch on `lang` change. Test the endpoint.

## Task 7 — translations
Run the extractor; fill Spanish completely, then pt-BR, fr, de, ko, zh-CN, vi, fil, ru, uk, sw, ht. Headers: `Language`, `Plural-Forms`, `X-Native-Name`, `X-English-Name`, `X-Reviewed: no`. Keep placeholders, keyboard key names (Ctrl, Enter) as Windows shows them in that language.

## Task 8 — docs and checks
`docs/TRANSLATING.md` (5 steps: copy the `.pot`, open in Poedit, translate, save as `translations/<code>/LC_MESSAGES/rustypresenter.po`, run tests/build). README line. Full test run; render the operator under Xvfb in Spanish and Korean and look at the PNGs; PROJECT_PLAN board + handoff.
