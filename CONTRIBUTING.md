# Contributing to LIVI

## Issues

Open an issue before you start on a pull request, and describe the problem or the change there. Pull requests without a related issue are not reviewed.

A bug report has to be reproducible. Include the LIVI version and build, the hardware (board, display, dongle), the phone and its OS version, the connection (wired or wireless), the steps that lead to the problem, what you expected, what happened and the logs (see [DEBUGGING.md](DEBUGGING.md)). Reports that cannot be reproduced cannot be reviewed and are closed.

## LLMs and generated content

LLMs may be used as a tool, but you are the author of what you submit and you stand behind it. You have reviewed every line, you understand it and you can explain it.

- AI tools are not listed as commit author or in `Co-authored-by`. To disclose their use, say so in the pull request description or add an `Assisted-by:` trailer.
- Issues, pull requests and comments that were evidently generated and not proofread, or consist of filler text, are closed without discussion.

## Contributor License Agreement

Contributions are accepted only under the [Contributor License Agreement](CLA.md). Every commit author and co-author accepts it once by posting this comment on the pull request:

```
I have read the CLA Document and I hereby sign the CLA
```

The `CLA` check records the signature and turns green once everyone has signed. Commits need an author email that is linked to a GitHub account, and `Co-authored-by` trailers have to use the co-author's GitHub email. If `CLA.md` changes, it has to be accepted again. If you contribute on behalf of an employer or another legal entity, contact hello@f-io.dev first.

## Translations

Each language is one file in `native/crates/livi-i18n/locales/`. Start from a copy of `en.json` and translate every entry, including the `navigation` section. Keep placeholders such as `{{n}}` as they are. A file is either empty or complete. A language shows up in the language list once its file is filled.

Please, only translate into a language you speak natively or fluently.

## Pull requests

- Target `main`, one topic per pull request.
- Commit subjects follow `type(scope): summary`, e.g. `fix(ui): keep the window size on restart`.
- Run `git config core.hooksPath .githooks` once. The hook formats staged Rust files and adds the license header.
- Before pushing, run in `native/`: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings` and `cargo test --workspace --locked`.
- Run `dprint check` for YAML, Markdown, JSON and TOML.
