---
name: publish-desktop-builds
description: Build and publish AIMonitor desktop installers for macOS ARM64 and Windows x64 from a Mac, using Tauri and cargo-xwin, while cleaning and repopulating the repository-root publish directory. Use when asked to release, package, rebuild, or verify this project's desktop distribution artifacts.
---

# Publish Desktop Builds

Publish both supported desktop installers through the repository's validated release
script. Keep release logic in that script rather than duplicating shell commands in a
prompt.

## Workflow

1. Read `AGENTS.md`, `docs/architecture.md`, and `docs/technology-stack.md`.
2. Inspect `git status --short`. Preserve unrelated user changes.
3. From the repository root, run:

   ```bash
   pnpm release:desktop
   ```

   The script validates prerequisites, cleans `publish/`, builds the unsigned macOS
   ARM64 DMG, cross-compiles the unsigned Windows x64 NSIS installer with
   `cargo-xwin`, validates both application binary architectures, and copies only the
   two installers into `publish/`.

4. Require a zero exit status. Do not report success from partial Tauri output.
5. Verify `publish/` contains exactly:

   - `AIMonitor_<version>_aarch64.dmg`
   - `AIMonitor_<version>_x64-setup.exe`

6. Report the absolute artifact paths, sizes, and checksums. Mention that the artifacts
   are unsigned because the release command uses `--no-sign`.

## Guardrails

- Run only on macOS. Do not replace `cargo-xwin` with plain Cargo for the Windows
  target.
- Do not copy raw binaries, `.app` directories, dependency files, logs, checksums, or
  unrelated bundle formats into `publish/`.
- Do not skip the release script's cleanup or architecture checks.
- Do not install missing tools automatically. Report the exact missing prerequisite
  shown by the script.
- Do not delete anything outside the repository-root `publish/` directory.
