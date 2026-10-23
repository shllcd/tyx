# AGENTS.md

## Commits

- One logical change per commit.
- Imperative subject, optional scope prefix (`router: …`); without body.

## Tests

- Tests are a development-time tool only: write and run them to verify your own changes, then delete them before committing.
- NEVER commit test files or test code unless the user explicitly asked for tests or gave permission; unanswered → no tests in the commit.

## Code is self-documenting

- No `//` comments explaining what/why — rename, extract, or tighten the signature instead.
  Exceptions: `// SAFETY:` on `unsafe`, license headers of ported code,
  lint-disable with an inline reason.
- Doc comments (`///`, `/** */`) only on public items whose signature can't express the
  contract (units, bounds, errors, complexity, ordering, ownership). Never restate the name.
- Names carry meaning: units in names (`timeout_ms`), predicates for booleans (`isPending`).
- No dead code, commented-out code, `todo!`, or `TODO` markers. Delete, don't comment.

## External changes

- Worktree changes you didn't make are the user's: NEVER edit, revert, reformat, or absorb them into your own edits.
- If they overlap your task, stop and ask how to proceed instead of working around them silently.
