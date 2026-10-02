@AGENTS.md

# Workflow: test first, implementation delegated

Every behaviour change follows this cycle. The main agent owns the tests and
never writes the code that makes them pass; the `implementador-tdd` subagent
(`.claude/agents/implementador-tdd.md`, kept out of version control) owns
only the green step.

1. **Interface first.** Types, parameter structs and signatures with a
   `todo!()` body, so the tests compile. A test that does not compile is not
   a useful red: the red must be a runtime failure for the expected reason.
2. **Tests.** Unit tests for pure logic (argv building, `RepoPath`
   validation) and integration tests against real jj (colocated repo plus a
   temporary bare remote, server in process behind an rmcp client). An
   integration test fails when jj is not on `PATH`; it is never skipped.
3. **Watch them fail.** Run them and check each fails for the right reason
   (the `todo!()` or the assertion), not a fixture error. Note the names of
   the red tests.
4. **Freeze the tests.** Record the test files of the slice (a checksum is
   enough). From here until green nobody edits them, the main agent
   included.
5. **Delegate green** to `implementador-tdd`, giving it the slice, the
   implementation files it may touch, the frozen test files, the expected red
   tests and the dependencies it may add.
6. **Review.** Check the frozen files are unchanged, that the code does not
   merely game the test, and run `just check` yourself, reading the
   `test result` counts.
7. **Commit** with jj: a conventional message in English, one logical change
   per commit, no AI attribution.

If a test turns out to be wrong, the subagent stops and reports instead of
editing it. The main agent decides; a changed test starts a new round (fix,
watch it fail, freeze, delegate).

Not every change is a TDD slice: CI workflows, docs and the changelog are
edited directly and validated with their own tools (`actionlint` for
workflows).

# Decisions: measure first, ask only when it is not clear

Non-trivial choices belong to the maintainer, but asking is the last step,
not the first.

1. **Trivial choices** (naming, local structure, a conventional default):
   make them and move on.
2. **Non-trivial choices:** before asking, try the alternatives and measure
   them (a prototype, a test, a benchmark, an end-to-end run against real
   jj).
3. **One is clearly better on the evidence:** use it without asking, and
   record the evidence in docs/decisions.md.
4. **Not clear:** ask, with the pros and cons of each option and the
   measurements behind them. The maintainer chooses.
