# Issues
Thanks for raising a bug or feature request!
If you're raising a bug or issue, please use the following template:

```md
**Songbird version:** (version)

**Rust version (`rustc -V`):** (version)

**Serenity/Twilight version:** (version)

**Output of `ffmpeg -version`, `youtube-dl --version` (if relevant):**
...

**Description:**
...

**Steps to reproduce:**
...
```

Additionally, tag your issue at the same time you create it.

If you're requesting a feature, explain how it will be useful to users or improve the library.
If you want to implement it yourself, please include a rough explanation of *how* you'll be going about writing it.

# Pull Requests
Thanks for considering adding new features or fixing bugs in Songbird!
Generally, we ask that PRs have a description that answers the following, under headers or in prose:

* The type of change being made.
* A high-level description of the changes.
* Steps taken to test the new feature/fix.

Your PR should also readily compile, pass all tests, and undergo automated formatting *before* it is opened.
The simplest way to check that your PR is ready is to install [cargo make], and run the following command:
```sh
cargo make ready
```

Merged PRs will be squashed into the repository under a single headline: try to tag your PR correctly, and title it with a single short sentence in the imperative mood to make your work easier to merge.
*"Driver: Fix missing track state events"* is a good example: it explains what code was modified, the problem that was solved, and would place the description of *how* the problem was solved in the commit/PR body.

If you're adding new features or utilities, please open an issue and/or speak with us on Discord to make sure that you aren't duplicating work, and are in line with the overall system architecture.

At a high level, focus on making new features as clean and usable as possible.
This extends to directing users away from footguns and functions with surprising effects, at the API level or by documentation.
Changes that affect or invalidate large areas of the library API will make a lot of users' lives that much harder when new breaking releases are published, so need deeper justification.
Focus on making sure that new feature additions are general to as many use-cases as possible: for instance, adding some queue-specific state to every audio track forces other users to pay for that additional overhead even when they aren't using this feature.

## Breaking changes
Breaking changes (in API or API semantics) must be made to target the `"next"` branch.
Commits here will be released in the next breaking semantic version (i.e., 0.1.7 -> 0.2.0, 1.3.2 -> 2.0.0).

Bugfixes and new features which do not break semantic versioning should target `"current"`.
Patches will be folded into more incremental patch updates (i.e., 1.3.2 -> 1.3.3) while new features will trigger minor updates (i.e., 1.3.2 -> 1.4.0).

## Documentation and naming
Doc-comments, comments, and item names should be written in British English where possible.
All items (`structs`, `enums`, `fn`s, etc.) must be documented in full sentences; these are user-facing
Error conditions, reasons to prefer one method over another, and potential use risks should be explained to help library users write the best code they can.

Code comments should be written similarly – this requirement is not as stringent, but focus on clarity and conciseness.
Try to focus on explaining *why/what* more confusing code exists/does, rather than *how* it performs that task, to try to prevent comments from aging as the codebase evolves.

## Testing
Pull requests must not break existing tests, examples, or common feature subsets.
Where possible, new features should include new tests (particularly around event or input handling).

These steps are included in `cargo make ready`.

## Linting
Songbird's linting pipeline requires that you have nightly Rust installed.
Your code must be formatted using `cargo +nightly fmt --all`, and must not add any more Clippy warnings than the base repository already has (as extra lints are added to Clippy over time).

These commands are included in `cargo make ready`.

[cargo make]: https://github.com/sagiegurari/cargo-make
