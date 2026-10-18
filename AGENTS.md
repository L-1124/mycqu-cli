# AGENTS.md

## Project Overview

`mycqu-cli` (`mycqu`) is a native Rust command-line tool for Chongqing University (CQU) campus services, designed specifically for AI coding agents, automation workflows, and terminal users.

- **SDK & Protocol**: Built on `rsmycqu` (campus SSO protocols, Ruishu 6 challenge resolution, and mycqu microservices).
- **Architecture**: Non-interactive, machine-readable JSON output envelope (`ApiResponse`), strict process exit codes (`0`, `2`, `3`, `4`, `5`, `6`, `7`), self-describing schema reflection (`mycqu schema`), Windows DPAPI encrypted session storage, and offline embedded Agent Skills (`rust-embed`).
- **Core Capabilities**: Auth & session persistence, student profile, academic calendar & terms, schedule/timetables, score & GPA ranking, course catalogs, campus card balance/bills, dorm electricity, and Agent Skill distribution.

## Setup & Build Commands

- **Prerequisites**: Rust 1.89+ (2024 edition). The `rsmycqu` dependency is automatically fetched via Git (`https://github.com/L-1124/rsmycqu`).
- **Build**: `cargo build`
- **Release Build**: `cargo build --release`
- **Run Directly**: `cargo run -- [subcommand] [flags]`
- **Check Compilation**: `cargo check`
- **Clippy Linting**: `cargo clippy --all-targets`

## Testing Instructions

- **Run All Tests**: `cargo test`
- **Run Library Unit Tests**: `cargo test --lib`
- **Run Contract Integration Tests**: `cargo test --test contract_test`
- **Run Focused Test**: `cargo test <test_function_name> -- --nocapture`

### Contract Invariants
- `tests/contract_test.rs` validates synchronization between `src/catalog.json` and Clap command definitions:
  - Every CLI flag in Clap must exist in `catalog.json`'s `input_schema.properties` (and vice versa).
  - All commands must specify `output_schema`, `requires_auth`, `has_side_effects`, and `possible_errors`.
  - Any command with `has_side_effects: true` must define a `confirmation` metadata block.
  - Adding or renaming subcommands/flags requires updating `src/catalog.json` before tests will pass.

## Agent Invocation & Output Contract

Every command emits structured JSON to stdout adhering to `ApiResponse<T>`:

```json
{
  "schema_version": 1,
  "ok": true,
  "data": { ... },
  "error": null,
  "meta": {
    "complete": true,
    "timezone": "Asia/Shanghai",
    "count": 10
  }
}
```

### Exit Codes
- `0` (`EXIT_SUCCESS`): Command executed successfully. Check `data` and `meta.complete`.
- `2` (`EXIT_INVALID_ARGUMENT`): Parameter error or mutual exclusion conflict. Query `mycqu schema <command>` to fix flags.
- `3` (`EXIT_AUTH_REQUIRED`): Session unauthenticated or challenge required (`CAPTCHA_REQUIRED`, `SECOND_FACTOR_REQUIRED`, `INVALID_CREDENTIALS`).
- `4` (`EXIT_PERMISSION_DENIED`): Resource permission denied or service unauthorized (e.g. unactivated campus card).
- `5` (`EXIT_UPSTREAM_ERROR`): CQU upstream system failure or network maintenance.
- `6` (`EXIT_STATE_INVALID`): Local storage lock contention, corrupted state, or platform storage failure.
- `7` (`EXIT_INCOMPLETE`): `ok: true`, but `complete: false` (e.g. bill paging limits). Retain `data` and continue paging.

## Schema Discovery (Anti-Hallucination)

Agents must query Schema definitions rather than guessing flags or invoking `--help`:

```bash
# Level 1: Full command catalog
mycqu schema

# Level 2: Group-level overview (e.g. all schedule commands)
mycqu schema schedule

# Level 3: Leaf command parameter and safety contract
mycqu schema "schedule list"
```

## Authentication & Challenge State Machine

Authentication never blocks interactively. When authentication interruptions occur (exit code `3`):

1. **Captcha (`CAPTCHA_REQUIRED`)**:
   - Extract `error.challenge_id`.
   - Retrieve captcha image: `mycqu auth captcha` (returns `data.captcha_path`).
   - Prompt user for captcha string and password, then submit:
     `mycqu auth verify --challenge-id <ID> --stdin-code --stdin-password`
2. **2FA (`SECOND_FACTOR_REQUIRED`)**:
   - Inspect `data.masked_phone` or `data.masked_email`.
   - Send code after user approval: `mycqu auth send-code --kind sms` (or `mail`).
   - Submit code: `mycqu auth verify --challenge-id <ID> --stdin-code --kind sms`
3. **Password Retry (`INVALID_CREDENTIALS`)**:
   - Submit revised password: `mycqu auth verify --challenge-id <ID> --stdin-password` (no verification code requested).

## Skill Management Engine

`mycqu` bundles offline Agent Skills embedded into the binary:
- `skills/multi/mycqu`: Primary campus query skill (`SKILL.md` under 200 lines, Golden Routes, and references).
- `skills/multi/cqu-reverse`: Ruishu 6 pure calculation & SSO reverse engineering guide.
- `skills/mono/mycqu`: Single-entrypoint fallback layout (`metadata.internal: true`).

Commands:
- `mycqu skill list`: Inspect embedded and installed skills across detected agent homes.
- `mycqu skill setup --dry-run`: Preview installation, symlink/copy targets, and backup plans.
- `mycqu skill setup --yes`: Atomically install canonical skills to `~/.agents/skills` and link to detected agent homes.
- `mycqu skill check`: Verify SHA256 directory digests against `~/.mycqu/skills-state.json`.
- `mycqu skill uninstall --yes`: Safely remove managed skills and links.
