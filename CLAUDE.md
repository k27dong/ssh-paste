# ssh-paste

Rust CLI that pushes the local clipboard to remote hosts over ssh so Claude
Code's Ctrl+V works there. Spec and plans live under docs/superpowers/
(local-only, never committed).

## Rules
- Comments: default none. Only a WHY the code cannot express, on the line it
  explains. No docstrings, no narration, in tests too.
- No conventional-commit tags anywhere (commits, PR titles, branches). Plain
  prose, first letter uppercase.
- Commits authored by k27dong only; no co-author trailers of any kind.
- `#![forbid(unsafe_code)]` stays; no unsafe in tests either.
- Flat readable modules: free functions, no getter/setter boilerplate, no deep
  private-helper nesting.
- Before any commit: cargo fmt --all -- --check && cargo clippy --all-targets
  -- -D warnings && cargo test.
- Never push without Kevin's explicit instruction.
