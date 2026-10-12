# Custom syntax highlighting

Raven Code loads user grammars written in Sublime Text's `.sublime-syntax`
format at startup. This adds syntax coloring and file-extension matching; it
does not add completion, diagnostics, or language-server support by itself.

Choose **Help → Open User Syntaxes Folder…**, place one or more
`.sublime-syntax` files in that folder (subfolders are also scanned), and
restart Raven Code. The grammar's `file_extensions` field selects files, and
its scopes use Raven Code's current theme colors. A malformed grammar is
skipped and reported in the startup notice.

Example grammar for `.raven` files:

```yaml
%YAML 1.2
---
name: Raven Sample
file_extensions: [raven]
scope: source.raven
contexts:
  main:
    - match: '\b(fn|let|return)\b'
      scope: keyword.control.raven
    - match: '"'
      push: string
  string:
    - meta_scope: string.quoted.double.raven
    - match: '"'
      pop: true
```

Language Server Protocol settings remain separate: configure an LSP server in
`raven-code.toml` if you also want language intelligence for that language.
