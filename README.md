# mycqu-cli

[![Rust 2024](https://img.shields.io/badge/Rust-2024%20Edition-orange?logo=rust)](https://www.rust-lang.org/)
[![License: AGPL v3](https://img.shields.io/badge/License-AGPLv3-blue.svg)](https://www.gnu.org/licenses/agpl-3.0)
[![Agent Skills](https://img.shields.io/badge/Agent%20Skills-Ready-green?logo=openai)](https://agents.md/)
[![Platform: Windows](https://img.shields.io/badge/Platform-Windows%20(DPAPI)-0078D6?logo=windows)](https://learn.microsoft.com/en-us/windows/win32/seccrypto/cryptprotectdata)

`mycqu` is a native, agent-first command-line tool for Chongqing University (CQU, 重庆大学) campus services. Built for humans and autonomous AI coding agents (Claude Code, Cursor, Codex, OpenCode, and custom LLM workflows), it unifies CQU Unified Authentication (SSO), teaching timetables, academic records, campus cards, and dorm utilities behind a non-interactive, structured interface.

---

## Key Features

- **Agent-First Contract**: Pure structured JSON responses (`ApiResponse<T>`) across every command, predictable Unix exit codes (`0`, `2`, `3`, `4`, `5`, `6`, `7`), and strict non-interactive execution.
- **Self-Describing Schema Discovery**: Built-in 3-level Schema reflection (`mycqu schema [command]`) eliminating model hallucination of parameters, flags, and constraints.
- **Zero-Trust Auth & Challenge State Machine**: Unified Authentication (SSO) with Ruishu 6 challenge resolution, DPAPI encryption for local session persistence, and non-blocking state machine for captcha and 2FA SMS/Email verification.
- **Embedded Agent Skills**: Bundled multi/mono Agent Skills deployed directly via `mycqu skill setup` into `~/.agents/skills` and local agent tools (Claude Code, Cursor, Codex) with SHA256 provenance auditing and backup-before-touch safety.
- **Full Campus Coverage**:
  - 👤 **User**: Student/faculty identity and profile verification.
  - 📅 **Academic Calendar**: Current semester, historical terms, and teaching week resolutions.
  - 📖 **Timetable & Schedule**: Exact date-to-week mapping, period schedules, and room allocations.
  - 📊 **Scores & Ranking**: Cumulative GPA, weighted average score, and class/major/grade rank.
  - 💳 **Campus Card**: Real-time card balance and paginated consumption bills.
  - ⚡ **Dorm Utilities**: Live dorm electricity meter and remaining balance query.

---

## Getting Started

### Prerequisites

- [Rust](https://rustup.rs/) toolchain (1.89+ with 2024 edition support)
- Windows host for DPAPI credential protection
- Dependency: [rsmycqu](https://github.com/L-1124/rsmycqu) (resolved automatically via Cargo git dependency)

### Installation

Build and install `mycqu` to your local Cargo bin directory:

```bash
cargo install --path .
```

Verify the installation:

```bash
mycqu --version
```

### Initial Login

Authenticate using your CQU unified authentication ID (学工号):

```bash
mycqu auth login --auth <STUDENT_ID>
```

> [!NOTE]
> Passwords are read securely from the terminal prompt or via `--stdin-password`. Plaintext credentials are never saved to disk, logged, or included in JSON responses. The session cookie jar is encrypted using Windows Data Protection API (DPAPI).

If a graphical captcha or SMS/Email verification code is triggered, the CLI emits a structured challenge with an opaque `challenge_id` and exits cleanly with code `3` (`AUTH_REQUIRED`), allowing human or agent continuation:

```bash
# When graphical captcha is required:
mycqu auth captcha
mycqu auth verify --challenge-id <ID> --stdin-code --stdin-password

# When two-factor SMS/Email is required:
mycqu auth send-code --kind sms
mycqu auth verify --challenge-id <ID> --stdin-code --kind sms
```

Verify authentication status anytime:

```bash
mycqu auth status [--online]
```

---

## Usage Examples

All commands output structured JSON formatted for machine consumption and terminal piping:

### Academic Schedule & Timetable

```bash
# Query schedule for a specific date (auto-maps to term and teaching week)
mycqu schedule list --date 2026-10-19

# Query full timetable for week 7
mycqu schedule list --week 7

# Query all registered courses for the current semester
mycqu schedule list --term current
```

### Grades & Academic Standing

```bash
# View grades for the current active semester
mycqu score list --term current

# Query cumulative GPA and official ranking
mycqu score gpa
```

### Campus Card & Utilities

```bash
# Check instant campus card balance
mycqu card balance

# Fetch card consumption bills for a date range
mycqu card bills --start-date 2026-10-01 --end-date 2026-10-18 --page 1

# Check dorm electricity balance (e.g. Meiyuan Building 5, Room 101)
mycqu dorm balance --room 梅五-101
```

### Schema Discovery (Anti-Hallucination)

```bash
# Level 1: Full catalog overview
mycqu schema

# Level 2: Subsystem group overview (e.g. schedule commands)
mycqu schema schedule

# Level 3: Leaf command parameter constraints and confirmation policy
mycqu schema "schedule list"
```

---

## Agent Skills Integration

`mycqu` includes embedded [Agent Skills](https://agents.md/) compatible with Claude Code, Cursor, Codex, OpenCode, and the `skills` CLI.

```bash
# Preview installation targets and backup plan (no disk writes)
mycqu skill setup --dry-run

# Install skills to ~/.agents/skills and detected agent homes
mycqu skill setup --yes

# Audit installed skills against binary digests to detect drift
mycqu skill check

# Uninstall managed skills and clean state
mycqu skill uninstall --yes
```

> [!TIP]
> `mycqu skill setup` automatically establishes symlinks to detected Agent directories (e.g. `~/.cursor/skills/mycqu`, `~/.claude/skills/mycqu`) with automatic copy fallback on permission-restricted environments. Existing customizations are safely archived to `~/.mycqu/skill-backups/<timestamp>/` before modification.

---

## Output Contract & Exit Codes

Every command returns a uniform JSON envelope adhering to `ApiResponse<T>`:

```json
{
  "schema_version": 1,
  "ok": true,
  "data": { ... },
  "error": null,
  "meta": {
    "complete": true,
    "timezone": "Asia/Shanghai",
    "count": 1
  }
}
```

### Exit Codes

| Code | Constant | Meaning | Action for Agents |
|---|---|---|---|
| `0` | `EXIT_SUCCESS` | Success | Parse `data` and check `meta.complete` |
| `2` | `EXIT_INVALID_ARGUMENT` | Argument syntax error or mutual parameter conflict | Inspect `mycqu schema <command>` to adjust flags |
| `3` | `EXIT_AUTH_REQUIRED` | Not logged in, session expired, or challenge required | Inspect `error.code` & `error.challenge_id` to continue auth state machine |
| `4` | `EXIT_PERMISSION_DENIED` | Permission denied or unactivated campus service | Report service boundary to user |
| `5` | `EXIT_UPSTREAM_ERROR` | CQU system error, SSO failure, or maintenance | Report upstream maintenance |
| `6` | `EXIT_STATE_INVALID` | Storage lock contention or corrupted state | Check local permissions or retry transient locks |
| `7` | `EXIT_INCOMPLETE` | Paginated result incomplete (`complete: false`) | Retain `data` and advance page offset |
