# Translating RustyPresenter

RustyPresenter can show every button, message and phone-remote word in your language. Each language is one text
file. You do not need to know any programming to make or fix one.

You need [Poedit](https://poedit.net) (free). Any `.po` editor works, and so does a plain text editor.

## Add or fix a language in 5 steps

1. **Start from the right file.**
   - To fix a language that is already there, open `translations/<code>/LC_MESSAGES/rustypresenter.po`
     (for example `translations/es/LC_MESSAGES/rustypresenter.po` for Spanish) and go to step 3.
   - To add a new language, copy `translations/rustypresenter.pot` (the list of every English text).
     `<code>` is your language's code: `es`, `fr`, `ko`, or with a country when it matters, such as `pt-BR` or `zh-CN`.

2. **Open it in Poedit.** For a new language, use **File → New from POT/PO file…**, pick the copy, and choose
   your language.

3. **Translate.** Go through the list and type each text in your language.
   - Keep the parts in curly brackets exactly as they are: `{name}`, `{n}`, `{phone}`. The app puts a song title,
     a number and so on there. You may move them within the sentence.
   - Keep key names the way Windows shows them in your language (Ctrl, Enter).
   - A text you leave empty shows in English, so a half-done file is fine.
   - Some texts say where they are used, or have a note. Read it when a word could mean two things.

4. **Save it as `translations/<code>/LC_MESSAGES/rustypresenter.po`** (make the folders if they are new), and check
   the file's header. In Poedit it is under **Translation → Properties**; in a text editor it is the block at the top.
   It needs these lines:

   ```
   "Language: pt-BR\n"
   "Plural-Forms: nplurals=2; plural=(n > 1);\n"
   "X-Native-Name: Português (Brasil)\n"
   "X-English-Name: Portuguese (Brazil)\n"
   "X-Reviewed: no\n"
   ```

   - **Language**: the same code as the folder name.
   - **Plural-Forms**: how your language says "1 song, 2 songs". Poedit fills it in when you pick the language.
     Otherwise copy it from the [gettext plural list](https://www.gnu.org/software/gettext/manual/html_node/Plural-forms.html).
   - **X-Native-Name**: your language's name in your language. Settings → Language shows it.
   - **X-English-Name**: its name in English, shown under it.
   - **X-Reviewed**: `yes` once a native speaker has read the whole file, otherwise `no`.

5. **Check it.** Double-click `dev-build.cmd` (it builds the app and runs the tests; the results are in `build.log`),
   or run `cargo test --test i18n_catalogs`. The checks say if a header is missing, a `{gap}` was changed, or a
   plural text needs more forms. Then start the app with `RustyPresenter.cmd`, open **Settings → Language**, pick
   your language and look at the app. Send the `.po` file to the project (or open a pull request).

## For developers: keeping the list of texts up to date

The texts come from the code: `@tr("…")` in `ui/**/*.slint`, and `tr!`, `trn!`, `trc!` in `src/` (see the top of
`src/i18n/mod.rs`). The phone remote's words are listed in `src/i18n/remote_strings.rs`.

After adding or changing text, run this from the project folder:

```
cargo run --example i18n_extract
```

It rewrites `translations/rustypresenter.pot` and adds the new texts (untranslated) to every language's `.po`, keeping
the translations it already has. Texts the code no longer uses stay at the end of the file as `#~` lines. Commit the
changed files together with the code. Until you run it, the test `i18n_pot_is_current` fails.
