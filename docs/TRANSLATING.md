# Translating RustyPresenter

RustyPresenter can show every button, message and phone-remote word in your language. You do not need to know any
programming to fix a translation or add a new language, and you do not need any other program.

## The easy way: in the app, in 3 steps

1. **Open the translations.** In RustyPresenter, click **Settings → Translations…** in the menu (or open
   **Settings → Language** and click **Edit this translation…**). To start a new language, click
   **Add a language…**, pick yours from the list (or tick "My language is not in the list"), and choose to start
   from nothing or from a copy of a close language.

2. **Type your words.** The list shows each English text beside your language. Click one and type in the box below.
   - Use the search box, or the buttons **Untranslated**, **Needs review** and **Changed by me**, to find texts.
   - Keep the parts in curly brackets exactly as they are: `{name}`, `{n}`. A red note tells you if one is missing
     or changed; that text stays in English until it is fixed.
   - Texts that change with a number ("1 song, 2 songs") have one box for each kind of number, labelled with
     examples ("For 1…", "For 2, 3, 4…").
   - Your change shows in the whole app (and on the phone remote) a moment after you stop typing, and is saved by
     itself. **Reset this entry to built-in** undoes it.

3. **Share it.** When a native speaker has read it all, click **Mark as reviewed**. Click **Export…** to save one
   `.po` file, and send it to the project so other churches get it too. **Import…** loads a file someone sent you.

Your changes are kept in the data folder, in `translations/<code>.po` (for example `translations/es.po`), and win
over the translation built into the program. A file there for a language the program does not have adds that
language to **Settings → Language**. A file that cannot be read is skipped, and the app says so.

## The advanced way: Poedit and pull requests

You need [Poedit](https://poedit.net) (free). Any `.po` editor works, and so does a plain text editor. This changes
the translations built into the program, so it needs a rebuild.

### Add or fix a language in 5 steps

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
   - Texts marked `song section heading` (Verse, Chorus, Bridge…) are the words people type on their own line in
     song lyrics. The app then knows that part is a chorus, a verse and so on. Give the word your churches write;
     for more than one, put `|` between them (`Coro|Estribillo`). English headings always work too.

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

Translations edited in the app (`src/i18n/user.rs`, `src/ui/dialogs/translation_editor.rs`) reach the `@tr` text of
the `.slint` files through a small hook in a patched copy of Slint's core: see `patches/i-slint-core/PATCHED.md`.
Note that only texts already in `rustypresenter.pot` can be edited in the app, so run the extractor before a release.
