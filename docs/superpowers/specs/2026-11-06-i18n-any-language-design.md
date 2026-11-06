# RustyPresenter in any language (I18N) — design

**Date:** November 6, 2026 · **Asked by:** Jake ("make sure that there is a plan feature for multiple languages so that you can use this project in any language you like") · **Board id:** I18N-1

## 1. Goal

Every word the operator, the phone remote and the stage screen show can be shown in the volunteer's own language. A church picks its language in Settings, or the app follows the computer's language on first start. Adding a new language is one text file that any volunteer can make with a free tool (Poedit, Weblate, or a text editor), with no Rust knowledge.

Not in this project: the words *on the slides* (lyrics, scripture, announcements are the church's own content); Bible book names and typed references in other languages (Bible library spec §8, its own task); right-to-left mirroring of the operator layout (§8).

## 2. Decisions

| # | Decision | Why |
| --- | --- | --- |
| D1 | One gettext catalog per language: `translations/<lang>/LC_MESSAGES/rustypresenter.po`, plus the template `translations/rustypresenter.pot`. | The standard translators already know; every translation tool reads it. Slint's own translation support uses exactly this layout. |
| D2 | `.slint` text uses Slint's `@tr(...)`, compiled in with `with_bundled_translations("translations")` and `DefaultTranslationContext::None`. | Native Slint; no extra crates; changes language live without restarting. No context by default, so a word like "Close" is translated once; a string that needs a different word in another place uses `@tr("context" => "...")`. |
| D3 | Rust-made text uses our own `crate::i18n` (`tr!`, `trn!`, `trc!`) over the **same** `.po` files, embedded at build time by `build.rs`. Pure Rust, no gettext/libintl. | One catalog per language for translators. libintl does not build with the MinGW toolchain; a small parser is less risk than a new dependency. |
| D4 | Rust placeholders are named: `tr!("Delete “{name}”?", name = title)`. Plurals: `trn!("{n} item", "{n} items", n)`. Slint keeps its own `{}` / `{0}` / `{n}`. | Named placeholders tell a translator what the gap holds. |
| D5 | Plural rules come from each file's `Plural-Forms:` header, evaluated by a tiny expression evaluator in `i18n::plural`. | Works for every language gettext supports (Arabic 6 forms, Polish 3, Japanese 1) with no table to maintain. |
| D6 | `Settings.ui_language: String`. `""` = follow the computer. Otherwise a code such as `es` or `pt-BR`. A code not built in falls back to English with a toast. | Saved with the other settings; backups carry it. |
| D7 | Language page in Settings (new tab **Language**, right after Look). A list: "Use the computer's language (Español)", then every built-in language by its own name with its English name under it ("Español · Spanish") and how complete it is ("98% translated"). Picking one changes the whole app at once. A link line: "Help translate RustyPresenter" opens `docs/TRANSLATING.md` on GitHub. | Easy for non-technical volunteers; the percentage is honest about partial translations. |
| D8 | The phone remote gets its words from `GET /i18n.json` (the current language's Rust catalog entries the page uses) and re-reads it when the server says the language changed. | The remote page is plain HTML/JS; no build step. |
| D9 | Machine-drafted translations ship marked `X-Reviewed: no` in their header; Settings shows "(needs review)" after the name until a native speaker sets `X-Reviewed: yes`. | Honest; invites review instead of hiding quality. |
| D10 | `cargo test` keeps everything in step: the `.pot` must match the code (`i18n_pot_is_current`), each `.po` must parse, use only msgids from the `.pot`, keep every placeholder, and have the plural count its header says. `cargo run --example i18n_extract` rewrites the `.pot` and merges new strings into every `.po` (untranslated). | A forgotten string is a failing test, not a silent English word in a Spanish app. |

## 3. Pieces

### 3.1 `src/i18n/` (new)
- `po.rs` — parse a `.po`: header, `msgctxt`, `msgid`, `msgid_plural`, `msgstr`, `msgstr[i]`, multi-line strings, C escapes (`\" \\ \n \t`), comments, `#, fuzzy` (fuzzy entries are skipped), obsolete `#~` entries skipped. Errors carry the line number.
- `plural.rs` — parse and evaluate `nplurals=N; plural=EXPR;` (`n`, integers, `! * / % + - < <= > >= == != && || ?:`, parentheses). Bad header → English rule `n != 1`.
- `mod.rs` — `Catalog { lang, native_name, english_name, reviewed, nplurals, rule, map: HashMap<(ctx, id), Vec<String>> }`; `languages() -> Vec<LanguageInfo>` (code, native name, English name, percent translated, reviewed); `set_language(code) -> Result<(), Unknown>` (also calls `slint::select_bundled_translation` when a Slint context exists — wrapped so tests without a window work); `current()`; `system_language() -> Option<String>` (`sys-locale`, matched exactly, then by base language, against the built-in list); `tr`, `trn`, `trc` functions and the three macros. Lookup is lock-free on the hot path (`ArcSwap`-style `RwLock<Arc<Catalog>>`; std only). Missing entry or empty `msgstr` → English.
- Language names come from the `.po` header (`Language:` plus `X-Native-Name:` and `X-English-Name:`).
- `build.rs` writes `$OUT_DIR/i18n_bundle.rs`: `pub static BUNDLE: &[(&str, &str)] = &[("es", include_str!(".../es/LC_MESSAGES/rustypresenter.po")), …];` and `cargo:rerun-if-changed=translations`.

### 3.2 Settings and start-up
- `Settings.ui_language` (serde default `""`), action `settings/language {code}`; the `settings` save keeps it like `favourites` (a form opened earlier cannot undo a change).
- Start-up: resolve `ui_language` (or the computer's language) before the first window is built; select it on Rust and Slint sides.
- On change: every Rust-built text the operator shows is rebuilt (the existing full view refresh), the stage and phone get a `lang` change in their next update, and a toast in the *new* language says "Language changed".

### 3.3 What gets translated
All user-visible text in `ui/**/*.slint`, `src/strings/**`, toasts, dialog text, errors shown to the operator, the stage screen's labels, the room screen's built-in text (e.g. the pre-service countdown words), and `web/remote.*`. **Not** translated: log lines, ids, file names, command names, test names, settings keys, font names, theme ids, the CCLI wording the licence requires in English, and anything the church typed.

### 3.4 Shipped languages (first set)
English (source), Spanish (`es`), Portuguese — Brazil (`pt-BR`), French (`fr`), German (`de`), Korean (`ko`), Chinese — Simplified (`zh-CN`), Vietnamese (`vi`), Filipino (`fil`), Russian (`ru`), Ukrainian (`uk`), Swahili (`sw`), Haitian Creole (`ht`). All `X-Reviewed: no` until a native speaker checks them. More are one file each.

### 3.5 Fonts
The operator uses the system UI font. Windows has Korean, Chinese and Vietnamese fonts built in. The Language page shows "Your computer may not have a font for this language." (the Bible page's `font_test_line` check) under a language whose own name cannot be shaped.

## 4. Errors
- A `.po` that fails to parse is a **build-time test failure**, never a runtime crash. At runtime a broken built-in catalog (impossible after tests) falls back to English and logs.
- A placeholder missing in a translation → that entry falls back to English (and the test fails).

## 5. Tests (TDD; each fails before its code)
- PO parsing: multi-line, escapes, plurals, context, fuzzy skipped, obsolete skipped, header fields, line numbers in errors.
- Plural: English, French (`n>1`), Russian/Ukrainian 3 forms, Polish, Arabic 6 forms, Japanese 1 form, bad header falls back.
- `tr!` named args, missing args left as `{name}`, fallback to English for missing/empty/fuzzy; `trc!` context.
- `system_language` matching: `es-MX` → `es`, `pt-BR` exact, `zh-Hans-CN` → `zh-CN`, unknown → `None`.
- Settings: `settings/language` saves, unknown code refused, plain `settings` save keeps it, old data loads with `""`.
- Catalog health: `.pot` current; every `.po` valid against it; placeholders kept; plural counts right.
- UI: the Language tab lists languages; choosing Spanish changes a known label in the operator (testing backend) and a Rust-built toast.
- Phone: `/i18n.json` returns the chosen language's strings.

## 6. Done when
- Every user-visible string goes through `@tr` or `tr!` (a test greps `.slint` files for bare `text: "…"` user text, with an allowlist for symbols like "♪", "·", "—").
- Spanish is complete and switching language changes the operator, dialogs, toasts, stage labels and phone remote without restarting.
- The other languages in §3.4 are present (machine-drafted, marked for review).
- `docs/TRANSLATING.md` tells a volunteer how to add or fix a language in 5 steps.
- Checked by eye on Windows by Jake (Spanish), plus one CJK language for fonts.

## 7. Order
I18N is built after UI-MENU and the open Bible items so the one-time string sweep catches their words too; the foundation (§3.1, §3.2) can land first.

## 8. Later
- Right-to-left operator layout (Arabic, Hebrew, Persian, Urdu): Slint 1.18 shapes RTL text but does not mirror layouts. Catalogs for these languages can be added now (text is correct); mirroring waits for Slint support.
- Bible book names and typed references in the UI language: Bible library spec §8.
- Loading a `.po` from the data folder without rebuilding (needs Slint's external translator API, not public in 1.18).
- Date and number formats per language for timers and the clock.
