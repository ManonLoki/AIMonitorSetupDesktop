# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

Note: `AGENTS.md`, `docs/architecture.md`, and `docs/technology-stack.md` are written in Chinese and are the authoritative source for this project's rules. This file summarizes them in English; if anything here conflicts with those docs, the docs win. Read `docs/architecture.md` before making any non-trivial change.

## What this is

A Tauri 2 desktop app ("AI Monitor Setup") for discovering and configuring `AiMonitor` LAN devices. Rust is the sole business backend; React is a thin UI layer that renders state and calls Rust commands. There is no HTTP boundary between frontend and backend — the frontend never uses fetch/Axios; all backend access goes through typed Tauri `invoke`.

## Commands

```bash
pnpm install          # install JS deps
pnpm tauri dev         # run the desktop app in dev mode
pnpm dev               # vite only (rarely useful standalone; app expects Tauri)

pnpm typecheck          # tsc --noEmit
pnpm build              # typecheck + vite build
pnpm check:rust         # cargo fmt --check + cargo clippy -D warnings (src-tauri)
pnpm test:rust          # cargo test (src-tauri)
pnpm check              # typecheck + check:rust + test:rust — run before considering work done
pnpm tauri build         # full production build
```

To run a single Rust test, use cargo directly against the src-tauri manifest, e.g.:
```bash
cargo test --manifest-path src-tauri/Cargo.toml <test_name>
```

Requires Node.js 22.12+, pnpm 10.30+, Rust stable (currently validated against 1.97).

## Architecture

Strict layered flow; dependencies only point downward:

```
React page → TanStack Query hook → typed TypeScript API (features/*/api) → Tauri invoke
           → Tauri command (thin adapter) → Rust domain / application logic
```

```
src/
├── app/                       # Providers, Router, QueryClient wiring
├── features/<feature>/
│   ├── api/                   # command names + transport DTO types
│   ├── queries/               # query keys, caching/invalidation
│   └── pages/                 # Mantine page composition/interaction
└── shared/
    ├── state/                 # pure client UI Jotai atoms only
    ├── tauri/                 # generic invoke transport adapter (invokeCommand)
    └── ui/                    # cross-feature presentational components

src-tauri/src/
├── commands/                  # Tauri param/result adapters; kept very thin
├── domain/                    # business entities, rules, pure logic
├── application/                # service orchestration (e.g. MonitorService, device discovery)
└── lib.rs                     # plugin setup + command registration
```

Reference implementation to copy when adding a new feature slice: `get_system_overview` —
Rust domain `src-tauri/src/domain/system.rs` → command `src-tauri/src/commands/system.rs` →
TS API `src/features/system/api/system.ts` → query `src/features/system/queries/system.ts` →
consumed by `src/shared/ui/AppShellLayout.tsx` (renders device status in the app shell).

### State ownership

| State kind | Owner | Example |
| --- | --- | --- |
| Business facts | Rust | monitor config, status, alert rules |
| Async call results | TanStack Query | command return values, loading, cache |
| URL-expressible state | TanStack Router | current page, filters, shareable selection |
| Short-lived UI state | Jotai / local component state | theme, sidebar toggle, uncommitted input |

Never copy Rust-returned data into Jotai — that creates a second source of truth and bypasses Query's cache invalidation.

### Hard constraints (from AGENTS.md)

1. All business logic lives in the Rust backend; React is display/interaction/call-through only.
2. UI uses Mantine; routing uses TanStack Router; async Rust-command state uses TanStack Query.
3. Jotai is for client UI state only — never backend data or Query results.
4. Frontend talks to Rust only via typed Tauri `invoke` (through `shared/tauri/invoke-command.ts` → feature `api/`). No Axios/fetch/other HTTP clients in the frontend; remote network access is implemented in Rust (currently `reqwest`, used only from the Rust application layer to talk to AiMonitor device HTTP APIs; `mdns-sd` is used only from the Rust application layer for `_aimonitor._tcp.local.` discovery).
5. React components must never call raw `invoke` directly — always go through a feature's `api/` and `queries/` layers.
6. Tauri commands do transport adaptation only; business decisions belong in Rust `domain/` (or a future `application/` layer), never in the command handler.
7. Don't add unused dependencies. Any dependency change must update the lockfile and `docs/technology-stack.md` together.
8. Rust DTOs serialize with serde `camelCase` to match TypeScript. Command failures must return serializable, UI-meaningful errors — don't parse free-text errors on the frontend to infer business state.

New features should be implemented in this order: Rust domain/application logic + tests → thin `commands/` adapter → register in `lib.rs` → TypeScript DTO + call function in feature `api/` → query/mutation in feature `queries/` → page composes Mantine components and UI state only.

Any deliberate deviation from this architecture requires updating `docs/architecture.md` first with rationale — don't leave undocumented exceptions in code.

### Definition of done

Changes should pass, within their applicable scope:
```bash
pnpm build
pnpm check
pnpm tauri build
```
New business logic should get Rust unit tests first.
