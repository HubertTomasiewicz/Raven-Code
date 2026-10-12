This project intentionally contains markers with the SAME detection rank
(pom.xml + build.gradle). Raven Pico reports both candidates and refuses to
guess: `raven-pico detect` exits with code 5 (ambiguous project) and asks
for an explicit choice via `--kind maven|gradle` or `[project] kind` in
raven-pico.toml.

When markers have different ranks (e.g. Cargo.toml + pyproject.toml), the
higher-ranked one becomes primary and the other is listed as an alternative
candidate — detect succeeds with a note.
