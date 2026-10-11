# Interface language

Choose **Menu > Edit > Preferences…** (Command-comma on macOS, Ctrl-comma elsewhere) **> Interface language** and select **Auto**, **English**, **日本語**, **简体中文**, **繁體中文**, **Русский**, **Български**, **Čeština**, **Português (Brasil)**, **Deutsch**, **Español**, **Français**, **తెలుగు**, **Magyar**, **Українська**, **Italiano**, **العربية**, **Polski**, **Türkçe**, **Bahasa Indonesia**, **한국어**, **हिन्दी**, **বাংলা** or **اردو**. The change applies immediately and persists between launches. Command ids, document contents and file names are unchanged.

Displayed keyboard shortcuts use the same active interface language. For example, German shows `Strg+K` and `Strg+Umschalt+S` in the toolbar, menus, command palette, tooltips and Keyboard shortcuts dialog; macOS keeps its modifier symbols (`⌘`, `⇧`, `⌃`). Key names have their own `key` catalog context, so the Delete key can read `Entf` while the command remains `Löschen`. A missing key-name entry falls back to the English key name. Actual bindings and machine-facing command ids remain stable.

**Auto** (the default) uses the **primary** system language. Windows and Apple platforms query the native display-language preference; Linux/BSD use the first locale returned by `sys-locale` (`LANGUAGE`, then `LC_ALL`, `LC_MESSAGES`, `LANG`). Browser builds and the mobile webview use `navigator.languages[0]`. A missing or unsupported primary language uses English, even when a later preferred language is supported: `sv-SE, de-AT` uses English, while `de-AT, en-US` uses German. Windows and Apple detection do not use locale environment variables in place of the display language. Detection is cached for the process. A manually selected language is stored and continues to override Auto.

Region variants use their existing language catalog: `de-AT` → German, `fr-CA` → French, `es-MX` → Spanish, `pt-PT` → Brazilian Portuguese, `hi-IN` → Hindi, `bn-BD`/`bn-IN` → Bengali, `tr-TR` → Turkish, `id-ID` → Indonesian, `ur-PK`/`ur-IN` → Urdu, and `ko-KR` → Korean. Existing Chinese script/region matching is unchanged: explicit Hans/Hant wins; otherwise Taiwan, Hong Kong and Macau use Traditional Chinese, and other Chinese regions use Simplified Chinese.

The control channel exposes the setting through `ui.set`:

```json
{"method":"ui.set","params":{"key":"language","value":"ja"}}
```

The value is `auto` or a registered code (`en`, `ja`, `zh-hans`, `zh-hant`, `cs`, `pt-br`, `de`, `es`, `fr`, `ru`, `bg`, `te`, `hu`, `uk`, `it`, `ar`, `pl`, `tr`, `id`, `ko`, `hi`, `bn`, `ur`), case-insensitive. `ui.state` reports the saved value. Unknown values return an error without changing the current setting. Preferences saved before this setting existed follow the primary system language.

Dialogs, panels, menus, notices and history labels go through the catalog (`tl!`). Japanese, Traditional Chinese, Simplified Chinese, Russian, Bulgarian, German, Spanish, French, Telugu, Hungarian, Ukrainian, Italian, Brazilian Portuguese, Turkish and Arabic translate them (about 2,000–2,200 entries each); Czech covers the menus so far. Untranslated labels use English. Error details that come from the engine or the operating system are shown as they are, inside a translated frame ("操作「…」に失敗しました: …"). The command palette matches the translated label, the English label and the command id. Vertical Japanese PDF rendering is an existing viewer feature; this change does not add vertical text editing. Per-language coverage, script support and what each language still needs are in [localization-parity.md](localization-parity.md).

The Ukrainian catalog has 2,189 entries, written from the meaning of the English labels using Ukrainian computing vocabulary. Commands use infinitives; fixed-count messages use count-neutral wording. Plural-aware messages use the three [CLDR Ukrainian integer forms](https://www.unicode.org/cldr/charts/48/supplemental/language_plural_rules.html#uk): `1 сторінка`, `2 сторінки`, `5 сторінок`; 11–14 take the third form, while 21 and 22 return to the first and second. Calendar dates use genitive month names (`9 жовтня 2026`), while headings use standalone names (`Жовтень 2026`). Home recommendation cards grow to fit translated text; the toolbar search hint truncates before the shortcut, with the full label available on hover.

Japanese interface text uses BIZ UDPGothic from [craft-fonts](https://github.com/storytold/craft-fonts), an optional build input that every release includes (`CRAFT_FONTS_DIR`; see the README). A build made without it has no Japanese face, so Japanese labels show replacement boxes. Simplified and Traditional Chinese are drawn with the craft-fonts `Hans` and `Hant` face, Droid Sans Fallback (Apache-2.0, in the web build too). In those languages it comes before the Japanese faces, so shared ideographs take Chinese shapes and a line never mixes faces with different baselines; in other languages it follows them and only draws what they lack, such as a Chinese file name. A build without craft-fonts shows every Chinese character as a replacement box. Russian, Bulgarian, Czech, Brazilian Portuguese, German, Spanish, French, Hungarian, Italian and Turkish need only the bundled Latin faces (tested in `tests/fonts.rs`). Hungarian uses the same one/other plural rule as English: 1 is singular, and every other count — including 0 — is the other form. After a numeral the noun stays singular (`1 oldal`, `2 oldal`). Telugu is drawn with a craft-fonts `Telu` face (Noto Sans Telugu, kept in the web build too), whose conjuncts egui shapes; a build without one shows Telugu labels as replacement boxes; the combined preview includes the existing Telugu font contribution.

The Brazilian Portuguese catalog has 2,179 entries: the ~1,826-entry gist André Cardoso posted in [#288](https://github.com/storytold/pdfcraft/issues/288), the labels added since, and a review of the existing wording. Portuguese, like French, treats 0 and 1 as singular (`1 página`, `2 páginas`).

## Hindi, Bengali, Indonesian, Urdu and Korean

Each new catalog contains all 2,241 `(context, source)` keys in the current catalog union, including command labels, tool labels, UI literals, keyboard names and plural entries. They are original translations of the English labels; native-speaker review is still welcome. Indonesian and Korean use one plural form. Hindi and Bengali use the singular form for 0 and 1; Urdu uses it for 1 only. Calendar month and weekday names are available in the same languages. This changes the interface, not OCR language models or the characters that a particular PDF text-editing operation can write.

CI and release builds pin the [combined font snapshot `9e89a05`](https://github.com/lukashold/craft-fonts/tree/9e89a056b92666d14ba70cd1c0811d9cfb6777fe), preserving the authors and licenses of [craft-fonts #19](https://github.com/storytold/craft-fonts/pull/19), [#17](https://github.com/storytold/craft-fonts/pull/17) and [#10](https://github.com/storytold/craft-fonts/pull/10). It supplies Noto Sans Devanagari, Noto Sans Bengali and NanumGothic; existing Noto Sans Arabic supplies Urdu. All are OFL fonts with licenses in the font repository. The Regular script faces are retained in the web/mobile subset. No font file is copied into PdfCraft, and no system font is needed for these labels.

Tests check every catalog character in all four UI font families, both the desktop stack and the web/mobile subset. Separate egui rendering tests compare joined Devanagari/Bengali consonants and Urdu words against deliberately unjoined forms. A rendered script sample was also visually checked with `PDFCRAFT_SYSTEM_FONTS=0`. These checks establish glyph coverage and contextual shaping; they do not constitute native-speaker review or fix the existing RTL layout limitations below.

Urdu uses the same logical-order catalog and single-line display conversion as Arabic. The font is Naskh, not Nastaliq. Layout mirroring, wrapped RTL paragraphs and editable mixed-direction text remain incomplete for both Arabic and Urdu.

Existing catalogs remain available. Turkish is already proposed separately in [#1057](https://github.com/storytold/pdfcraft/pull/1057), and Chinese face/script-selection improvements in [#879](https://github.com/storytold/pdfcraft/pull/879), with [craft-fonts #17](https://github.com/storytold/craft-fonts/pull/17). This preview integrates their fixed revisions without proposing duplicate feature PRs; the Telugu font contribution remains [craft-fonts #10](https://github.com/storytold/craft-fonts/pull/10). Two Touch control labels are also translated in each newly combined catalog.

### Combined preview inputs

The desktop preview starts at Preview .7 `0f7861474fce4ef437ee3d015e328a378d6651e2`. It imports the desktop language/font delta from the independently tested Mobile + Touch language tree `e49b2225a6faaeaf6fb8eb11b164b6cbdebcf78c`, retaining the scan/OCR, historical recognition, touch and Hot Folder features. The native mobile host remains a separate change. All 35 additional preview labels are translated in the five new catalogs; Turkish also covers the remaining preview keys.
It integrates only the CJK contribution at `faa5fb77978c00ac323b4f49d92f9f232d68f94d`
and Turkish at `6eae87fec234d7eb897382a354e6d3109ae720ea`, plus the five-language
contribution based on upstream `bbbe60015c571ebfc39708ee07777b89e267122a`.
The newer CJK head `3f2629e164c3bbf438cc92e3218ec1c8766f0425` rebases the same
font/runtime changes onto a later upstream; unrelated upstream changes are not imported.
The combined font commit above retains three original parents: PR #19 at
`3ed348d5faf558cf77e4f978d927cf07d0c9acb3`, PR #17 at
`2caca391ea8cdd8b3be14a53be8662746ee50385`, and Telugu PR #10 at
`9446ad4d0ba4d7d672342896c54ea2c099dfaaef`. Original license files and font bytes
are unchanged. Droid's Apache notice and the OFL texts are included in package notices.

## Arabic and right-to-left text

The Arabic catalog has 2,190 entries in Modern Standard Arabic with Western digits. Counts use the six [CLDR Arabic forms](https://www.unicode.org/cldr/charts/48/supplemental/language_plural_rules.html#ar) (`0`, `1`, `2`, `3–10`, `11–99`, the rest), and dates use the Arabic names of the Gregorian months.

egui joins the letters of an Arabic word and runs them right to left, but it places the words of a line left to right in the order they are written. So catalog entries are written in logical (typing) order, and `i18n` puts each one into display order when the catalog loads (`bidi::display_rtl`): the line reads right to left, Latin words and numbers inside it read left to right, `{name}` placeholders and keyboard shortcuts stay whole, spaces at either end stay where the code concatenates, and a short caption keeps its colon on the right, beside the widget it captions. Lookups (`tl!`, `tr`, `trn`) therefore return display-order text for Arabic; `i18n::fmt` inserts values as they are. The command search matches a query of several Arabic words word by word.

What Arabic and Urdu do not have yet:

- **The layout is not mirrored.** Panels, menus, toolbars and alignment are as in the left-to-right languages, with Arabic labels in them.
- **A wrapped paragraph breaks at the wrong end.** A label too long for its line continues on the next line with its *first* words, so wrapped descriptions and tooltips read from the last line up. Single-line labels are right.
- **Text you type into a field** (search, comments, form values) has joined letters but its words run left to right while you edit it. What is stored is correct.
- Labels built by joining translated pieces in code (a list such as `A · B · C`) keep the code's left-to-right order.

These need bidirectional support in the text layout itself, not in the catalog.

Arabic glyphs come from the craft-fonts `Arab` face (Noto Sans Arabic, kept in the web build too). A build without craft-fonts shows Arabic as replacement boxes on the web; on desktop it falls back to one installed font (below). The craft-fonts revision pinned for CI and releases (`.github/workflows/`) has the Arabic face. That revision also holds Noto Sans CJK SC, which `crates/fonts/build.rs` never embeds (`AGENTS.md` §1.1 bars it); Chinese uses Droid Sans Fallback instead.

File names and document titles in Arabic or Hebrew are drawn with a craft-fonts `Arab` face when the build input has one; on desktop, a character no embedded face has is drawn with one font already installed on the machine (`PDFCRAFT_SYSTEM_FONTS=0` turns this off). In a Simplified or Traditional Chinese interface built without a Chinese face (no craft-fonts, as in a source build or Flathub's), that installed font is a Chinese one: PingFang or Hiragino Sans GB on macOS, Microsoft YaHei or JhengHei on Windows, WenQuanYi or Droid Sans Fallback on Linux ([#882](https://github.com/storytold/pdfcraft/issues/882)). It is read at runtime and never embedded; Source Han and Noto CJK are not among the candidates (`AGENTS.md` §1.1). The tab strip, the recent-files list and Properties show such names in display order; other places (bookmarks, comments, search results) have the glyphs but may order mixed-direction text wrongly.

Japanese, Simplified Chinese, French, German and Brazilian Portuguese coverage tests, following PhotoCraft's complete-catalog approach, check every registered command, All tools label and `tl!("literal")` in the UI source. Simplified Chinese also checks direct `i18n::t` calls and multiline literals, plus the About tabs, contributor name modes, sort options, table headers and model columns. Generated history labels, contributor summaries and diagnostics are tested with user-supplied names and error details. Catalog coverage does not guarantee glyph coverage, so with craft-fonts `chinese_labels_have_glyphs` also checks that every character of both Chinese catalogs is drawn on desktop and on the web, and that each one the Latin faces lack comes from the Chinese face (none falls through to a Japanese face or a replacement box); it fails, rather than skips, when the build input has no allowed `Hans` or `Hant` face ([#499](https://github.com/storytold/pdfcraft/issues/499)).

German labels keep nouns capitalized, including tool names inserted into notices. The certificate viewer uses the `certificate` context for `Subject` (the certificate holder), while document properties keep the plain translation (the topic). Search hints reserve room for the shortcut, and Home cards limit their title and description to the available rows; hovering shows the full text when it is shortened.

## How translations work

PdfCraft uses the same system as PhotoCraft (`crates/ui-egui/src/i18n/`):

- Strings in the code stay in English and serve as lookup keys. At draw time, `tl!("Save")` returns the text in the current language, and falls back to English when there is no entry.
- Each language has a catalog, `crates/ui-egui/src/i18n/<code>.tsv`, with one entry per line: `context<TAB>English<TAB>translation`. An empty context marks a plain string. `@id` keys an entry by command id (for example `file.saveAs`), so one menu item can read differently from another with the same English label. `@plural` holds plural forms. Any other context disambiguates an English word with several meanings (`tl_ctx!` in UI code, for example the signature pad's "Type" button against the "Type" column). The header of `ja.tsv` documents the format and escapes.
- Placeholders (`{name}`) must appear in both columns. A trailing `…` must be kept. `i18n::fmt` fills placeholders in one pass, so a value such as a file name containing `{n}` is inserted as written.
- Catalogs are validated strictly: unknown escapes, unknown `@` contexts, placeholder or ellipsis mismatches, wrong plural form counts and duplicates are errors. The tests require the bundled catalogs to have none; at run time a bad line is skipped (and logged) and that string shows in English.
- Catalogs are assets: each `.tsv` has an `ATTRIBUTION.toml` entry with `kind = "translation"` and its SHA-256, checked by `cargo xtask assets`.
- Translations are clean-room: write them from the meaning of the English text, never from another product's string tables.

## Adding a language

1. Copy the header of `ja.tsv` into `xx.tsv` and translate entries.
2. Add one row to `LANGUAGES` in `crates/ui-egui/src/i18n/mod.rs`: code, native name, catalog and plural rule.
3. Add an `ATTRIBUTION.toml` entry for the catalog (`kind = "translation"`), then run `cargo xtask assets --write` and `cargo xtask assets`.

The Preferences dropdown, the system-language match and the catalog tests (format, duplicates, placeholders, plural forms, command ids) then pick it up.
