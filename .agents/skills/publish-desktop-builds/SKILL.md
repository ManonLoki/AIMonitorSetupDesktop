---
name: publish-desktop-builds
description: Build and publish a signed and notarized AIMonitor macOS Universal installer plus an intentionally unsigned Windows x64 installer from a Mac, using Tauri and cargo-xwin, while cleaning and repopulating the repository-root publish directory. Use when asked to release, package, rebuild, or verify this project's desktop distribution artifacts.
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

   The script validates prerequisites, builds and notarizes the signed macOS
   Universal DMG, cross-compiles the intentionally unsigned Windows x64 application
   and NSIS installer with `cargo-xwin --no-sign`, validates both application binary
   architectures, then cleans and repopulates `publish/`.

4. Require a zero exit status. Do not report success from partial Tauri output.
5. Verify `publish/` contains exactly:

   - `AIMonitorSetup-macOS-universal-v<version>.dmg`
   - `AIMonitorSetup-Windows-x64-v<version>-setup.exe`
   - `AIMonitorSetup-SHA256SUMS.txt`

6. Report the absolute artifact paths, sizes, checksums, macOS notarization result,
   Windows x64 architecture result, and that the Windows artifact is intentionally
   unsigned by repository policy.

## Guardrails

- Run only on macOS. Do not replace `cargo-xwin` with plain Cargo for the Windows
  target.
- Windows Authenticode signing is intentionally not required. Keep the explicit
  `--no-sign` build flag, and do not require a Windows certificate, signing password,
  `signCommand`, or `osslsigncode`. Treat the unsigned installer as the expected
  release artifact, not as a fallback.
- Do not copy raw binaries, `.app` directories, dependency files, logs, checksums, or
  unrelated bundle formats into `publish/`.
- Do not skip the release script's cleanup or architecture checks.
- Do not install missing tools automatically. Report the exact missing prerequisite
  shown by the script.
- Do not delete anything outside the repository-root `publish/` directory.
