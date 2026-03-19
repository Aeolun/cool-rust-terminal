# Project Guidelines

## Commit Convention

Use semantic commits (Conventional Commits) for all git commits:

- `feat:` - New features
- `fix:` - Bug fixes
- `docs:` - Documentation changes
- `style:` - Code style changes (formatting, semicolons, etc.)
- `refactor:` - Code refactoring without functional changes
- `perf:` - Performance improvements
- `test:` - Adding or updating tests
- `build:` - Build system or dependency changes
- `ci:` - CI/CD configuration changes
- `chore:` - Other changes that don't modify src or test files

Examples:
- `feat: add dark mode support`
- `fix: resolve crash on startup`
- `ci: add GitHub Actions for releases`

## Releases

The version in `Cargo.toml` (`workspace.package.version`) must match the git tag before tagging a release. The CI release workflow builds with the Cargo version, so a mismatch will fail the build. Always bump `Cargo.toml` first, commit, then tag.
