# Security

Ripplepath is designed to be pointed at repositories you do not trust. The full model is in
[docs/SECURITY_MODEL.md](docs/SECURITY_MODEL.md).

Core rule: **Ripplepath never executes code from the analysed repository** — no builds, tests,
scripts, Git hooks, Git filters or textconv drivers. Git data is read directly from the object
database with `gix`; the `git` executable is never run against an analysed repository.

## Reporting a vulnerability

Please open a private security advisory on the GitHub repository (Security → Report a
vulnerability) rather than a public issue. Include a minimal repository or input that reproduces the
problem.
