# ADR-009: Natural language CLI with dual-mode parsing

**Status:** Accepted  
**Date:** 2026-05-20

## Context

Task management CLIs traditionally require rigid syntax:

```bash
taskwarrior add "Deploy backend" project:work due:friday priority:H
todo.txt add "(A) Deploy backend +work @deploy due:2026-05-23"
```

This is precise but hostile to new users and slow for quick capture. Natural language input is fast:

```bash
tock add Deploy backend to work due friday priority high
```

However, pure natural-language parsing is ambiguous (does "to work" mean `project:work` or is "to work" part of the title?). We need a system that accepts natural language by default but provides an escape hatch for precision.

## Decision

**Dual-mode parsing:**

1. **Natural language as primary interface:**
   ```bash
   tock add Deploy backend to work due friday pri high
   tock add Call dentist tomorrow at 2pm
   tock add Review PR this evening +urgent
   ```
   The parser extracts intent using keyword detection, date parsing, and heuristics:
   - `to <name>` → `--project <name>`
   - `due <when>` → `--due <when>`
   - `at <time>` → `--scheduled-for <datetime>`
   - `tomorrow`, `friday`, `next week` → date arithmetic
   - `pri high`, `priority h`, `!` → `--priority H`
   - `+tag` → append to tags
   - Remaining words → task title

2. **Traditional flags as escape hatch:**
   ```bash
   tock add "Deploy backend to work" --project backend --due friday --priority H
   ```
   Quotes disambiguate title from parameters. Flags override natural language extraction.

3. **Filter DSL for queries:**
   ```bash
   tock list "status:pending +work urgency.over:10 limit:10 sort:urgency-"
   ```
   Filtering uses a structured DSL (field operators: `:`, `.over:`, `.under:`, `.between:`, logical `and`/`or`, tag syntax `+tag` / `-tag`). This is not natural language—it's a query language users learn once.

**Date/time parsing:**
Natural date parsing via `tock-parse`: `tomorrow`, `friday`, `next monday`, `in 3 days`, `2026-05-23`, `may 23`, `23 may`. Relative dates anchored to "now" at parse time.

**Scripting support:**
Scripts can use flags exclusively, avoiding ambiguity:
```bash
tock add --title "Deploy backend" --project work --due 2026-05-23 --priority H
```

## Consequences

**Positive:**
- Fast interactive use (natural language is faster to type than flags).
- Gentle learning curve (new users can start with English-like syntax).
- Unambiguous fallback (flags for edge cases, scripts, and precision).
- Filter DSL is learnable and composable (similar to SQL `WHERE` clauses).

**Negative:**
- Parser heuristics may misinterpret intent (e.g., "to work" as title vs. project). Mitigated by confirmation prompts in interactive mode, `--dry-run`, and undo.
- Dual-mode parsing increases implementation complexity (two parsers to maintain).
- Natural language parsing is locale-dependent (v1 supports English only; internationalization deferred to v2).

**Neutral:**
- Users who prefer flags can ignore natural language (both modes are first-class).
- Filter DSL is not natural language (no attempt to parse "show me high priority tasks due this week"—users must learn the DSL syntax).
