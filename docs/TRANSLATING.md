# Translating tock

tock's command-line interface is localized with [Project Fluent](https://projectfluent.org/),
wired up through the [`i18n-embed`](https://crates.io/crates/i18n-embed) stack.
English (`en-US`) is the source locale and always ships complete; every other
locale is a community translation of it.

This guide explains how to add or update a translation. No Rust knowledge is
required to translate — you only edit `.ftl` text files.

## Where the strings live

```text
crates/tock-cli/
├── i18n.toml              # fallback_language = "en-US"
└── i18n/
    └── en-US/
        └── tock-cli.ftl   # the source catalog (English)
```

Each locale is a directory named with a [BCP-47](https://www.rfc-editor.org/info/bcp47)
tag (`en-US`, `es-ES`, `fr`, `de`, `pt-BR`, …) containing a single
`tock-cli.ftl` file. The filename **must** be `tock-cli.ftl` (it matches the
crate name; the build relies on this).

## Adding a new language

1. Copy the English catalog into a new locale directory, e.g. for Spanish:

   ```sh
   mkdir -p crates/tock-cli/i18n/es-ES
   cp crates/tock-cli/i18n/en-US/tock-cli.ftl crates/tock-cli/i18n/es-ES/tock-cli.ftl
   ```

2. Translate the text in `es-ES/tock-cli.ftl` (see rules below).
3. Validate (see [Validating](#validating)).
4. Try it: `cargo run -p tock-cli -- --lang es-ES list` (or set `TOCK_LANG=es-ES`).
5. Open a pull request.

The app picks a locale in this order: the `--lang` flag, the `TOCK_LANG`
environment variable, your operating-system locale (`LANG` / `LC_*`), and
finally `en-US`. Any string missing from a translation automatically falls back
to English, so a partial translation is still usable.

## The `.ftl` format

A catalog is a list of `id = value` messages:

```ftl
## CalDAV

# Shown after `tock caldav remove <url>` succeeds.
caldav-collection-removed = Removed CalDAV collection: { $url }
caldav-all-links-deleted = All links to this collection have been deleted.
```

Rules for translators:

- **Never change the part to the left of `=`** (the message id). Translate only
  the text to the right.
- **Keep every `{ $placeholder }` intact.** These are filled in at runtime with
  values like task ids, counts, dates, or names. You may move a placeholder to
  wherever it reads naturally in your language, but do not rename or delete it.
- **Lines starting with `#` are comments.** `#` is a note for translators (often
  describing where/when a string appears), `##` is a group header, `###` is a
  file header. Comments are not shown to users and do not need translating, but
  please keep the `#` translator notes — they are there to help you.
- **Indented lines continue the previous message** (used for multi-line text and
  plurals); preserve the indentation.

### Plurals

Counts use Fluent's `select` syntax so each language can use its own plural
rules. English has two forms (`one`, `other`):

```ftl
import-done = Imported { $count } { $count ->
    [one] task
   *[other] tasks
}
```

Translate the words after `[one]` / `[other]`, and add or remove plural
categories as your language requires (`zero`, `two`, `few`, `many`, `other`).
The `*` marks the **default** category and must always be present. See the
[CLDR plural rules](https://www.unicode.org/cldr/charts/latest/supplemental/language_plural_rules.html)
for your language.

### Whitespace and alignment

Leading/trailing spaces used purely for terminal alignment live in the Rust code,
not in the catalog — so you don't need to worry about lining columns up. Fluent
trims surrounding whitespace from message values.

## Validating

Before opening a PR, run the catalog checker:

```sh
cargo xtask i18n-check
```

It verifies that:

- every `.ftl` file parses, and
- each locale defines **exactly** the same set of message ids as `en-US` — no
  missing strings and no stray/renamed ids.

The same check runs in CI. If you add a brand-new string, add it to `en-US`
first (and to every other locale, or it will be reported as missing — falling
back to English at runtime is fine for users, but `i18n-check` enforces id
parity so drift is visible).

Also run the build to exercise the compile-time message check used by the code:

```sh
cargo build -p tock-cli
```

## For developers: adding new user-facing strings

UI strings are emitted with the `tr!` macro:

```rust
println!("{}", tr!("task-added", sid = task.sid));
```

`tr!` wraps `i18n_embed_fl::fl!`, which checks **at compile time** that the id
exists in `crates/tock-cli/i18n/en-US/tock-cli.ftl`. So:

1. Add the message to `en-US/tock-cli.ftl` (with a `#` context comment and,
   under the right `## Domain` header).
2. Use `tr!("your-id", name = value, …)` at the call site. Each interpolated
   value is passed as a named argument matching a `{ $name }` placeholder.
3. Keep machine-readable output (JSON via `--format json`) and `tracing` logs in
   English — only localize human-facing prose.
4. Run `cargo xtask i18n-check` and `cargo build -p tock-cli`.

Id naming convention: `domain-action`, kebab-case (e.g. `focus-session-started`,
`error-unsupported-format`). Reuse an existing id when the same English text
appears more than once.
