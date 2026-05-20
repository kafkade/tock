# Tock — Architecture Design Document

**Status:** Draft v0.1

---

## 1. Executive Summary

**Tock** is a unified personal productivity engine that fuses four traditionally separate tools — task management, habit tracking, time tracking, and a focus (Pomodoro) timer — into a single end-to-end encrypted, local-first system. It is built around a methodology-neutral Rust core that exposes the same data model to a powerful CLI (with optional ratatui TUI), native Apple apps (iOS, iPadOS, macOS, watchOS) via UniFFI bindings, and a future WASM-powered web client. Synchronization is event-sourced, conflict-free under normal use, and works against either a self-hosted AGPL-3.0 server or an optional hosted service — with the on-disk vault format and protocol fully documented so users are never locked in.

The target user is the *power productivity practitioner*: developers, researchers, founders, knowledge workers, and serious productivity practitioners who have outgrown single-purpose apps and are forced to maintain manual bridges between them. Tock collapses those bridges into first-class cross-domain primitives: a Pomodoro session linked to a task automatically logs a time block *and* increments a "deep work" habit; a recurring task can be promoted to (or replaced by) a habit; projects aggregate effort, completion velocity, and supporting habits in a single view. Filtering, urgency scoring, and reporting use a single expressive query language that extends uniformly across all four domains.

What differentiates Tock is the combination of *(a)* methodology-neutral design — GTD-style views are available but not enforced, identity-based habits are encouraged but not required — *(b)* uncompromising cryptographic design (Argon2id-derived master key, per-item envelope encryption with AES-256-GCM, size-bucket padding, domain-separated AAD, SRP-6a authentication, 24-word Crockford Base32 recovery keys), and *(c)* a strict "core has zero I/O" architectural rule that makes the same Rust logic provably identical across CLI, iOS, watchOS, and web. The result is a system that respects expert workflows, refuses to leak unencrypted data to any server, and provides an honest plain-text export at any time.

---

## 2. Feature Specification

Each subsection documents *behavior*, *edge cases*, *configuration*, and *cross-domain interactions* for one domain, with CLI examples that double as executable specification.

### 2.1 Task Management

#### 2.1.1 Entity model and hierarchy

The task hierarchy is nested, with flat short-ID addressability:

```
Area  ── 1:N ──>  Project  ── 1:N ──>  Heading  ── 1:N ──>  Task  ── 1:N ──>  Checklist Item
                                  └──── 1:N ──> Task (no heading)
```

- **Area** — long-lived life domain (e.g. `work`, `health`, `family`). No completion semantics.
- **Project** — has a goal, completion state, optional deadline, optional area. Supports `dot.notation` aliasing: `work.backend.api` resolves to project `api` nested under projects via tag-style hierarchy *or* a literal project named `work.backend.api` (configurable per workspace).
- **Heading** — pure presentational grouping inside a project. No state, no dates, just an ordered label. Tasks can belong to a heading or be top-level in a project.
- **Task** — the atomic unit (see §2.1.2).
- **Checklist item** — sub-task with only `title` + `done_at`. Cannot be scheduled, tagged, or have its own checklist. Designed for "steps to complete this task," not for project decomposition (use sub-projects for that).

**Decision (opinionated):** No arbitrary sub-task nesting. Flat checklists are sufficient; arbitrary trees create UX and sync ambiguity (what does "complete parent" do?). Use projects with headings for decomposition.

#### 2.1.2 Task fields

| Field             | Type                       | Notes                                                                    |
|-------------------|----------------------------|--------------------------------------------------------------------------|
| `id`              | UUIDv7                     | Globally unique, time-ordered, sync-safe.                                |
| `sid`             | u32 (workspace-local)      | Short ID for CLI ergonomics. Recycled after `logbook purge`.             |
| `title`           | TEXT NOT NULL              | Markdown allowed.                                                        |
| `notes`           | TEXT                       | Markdown. May contain `[[wiki-links]]` to other entities (future).       |
| `status`          | ENUM                       | `pending` `started` `done` `cancelled` `someday` `inbox`.                |
| `area_id`         | UUID NULL                  |                                                                          |
| `project_id`      | UUID NULL                  |                                                                          |
| `heading_id`      | UUID NULL                  |                                                                          |
| `start_date`      | DATE NULL                  | "Don't show me before this" (deferred start).                              |
| `deadline`        | DATE NULL                  | Hard date. Distinct from `start_date`.                                   |
| `scheduled_for`   | TIMESTAMPTZ NULL           | Specific time (calendar slot). Differs from `start_date` (whole-day).    |
| `evening`         | BOOL DEFAULT 0             | "This Evening" bucket — pinned to bottom of Today.                       |
| `tags`            | TEXT[] (JSON in SQLite)    | Nested via `/`: `home/repairs/electrical`.                               |
| `priority`        | ENUM (`L`,`M`,`H`) NULL    | Feeds urgency. Optional; methodology-neutral.                            |
| `recurrence`      | RecurrenceSpec NULL        | See §2.1.6.                                                              |
| `parent_id`       | UUID NULL                  | For recurrence: points to template task.                                 |
| `depends_on`      | UUID[] (join table)        | Hard dependency. Blocks until all done.                                  |
| `annotations`     | (UUID, TIMESTAMPTZ, TEXT)[]| Append-only log per task.                                                |
| `udas`            | JSONB                      | User-defined attributes (see §2.1.5).                                    |
| `created_at`      | TIMESTAMPTZ                |                                                                          |
| `modified_at`     | TIMESTAMPTZ                |                                                                          |
| `done_at`         | TIMESTAMPTZ NULL           |                                                                          |
| `cancelled_at`    | TIMESTAMPTZ NULL           |                                                                          |
| `urgency_cache`   | REAL                       | Recomputed on write; see §2.1.4.                                         |

#### 2.1.3 GTD views (smart filters)

| View         | Filter (canonical)                                                                                      |
|--------------|---------------------------------------------------------------------------------------------------------|
| **Inbox**    | `status:inbox`                                                                                          |
| **Today**    | `status:(pending\|started) and (start_date<=today or deadline<=today or +TODAY) and not +BLOCKED`       |
| **Evening**  | `Today and evening:true`                                                                                |
| **Upcoming** | `status:(pending\|started) and (start_date>today or scheduled_for>today or deadline>today)`             |
| **Anytime**  | `status:(pending\|started) and start_date is null and scheduled_for is null and not +BLOCKED`           |
| **Someday**  | `status:someday`                                                                                        |
| **Logbook**  | `status:(done\|cancelled) order by done_at desc`                                                        |
| **Trash**    | soft-deleted (see §2.1.8)                                                                               |

Views are not hardcoded — they are seeded saved queries (`tock report` configs). Users can edit, delete, or add reports.

#### 2.1.4 Urgency scoring

Urgency scoring uses configurable weighted coefficients across cross-domain signals. Weights are user-configurable in `~/.config/Tock/config.toml`:

```
urgency =   1.0  * (deadline_factor)        // 1.0 at deadline, 0.2 a week away, 1.2 if overdue
          + 0.8  * (start_date_factor)      // 0 if not yet started, 1.0 on/after start_date
          + 0.6  * (priority_factor)        // H=1.0, M=0.65, L=0.3, none=0
          + 0.5  * (age_factor)             // capped at 365 days
          + 0.4  * (tag_factor)             // sum of user-weighted tag bumps
          + 0.4  * (project_factor)         // 1.0 if has project
          + 1.0  * (active_factor)          // 1.0 if currently being timed
          + 0.7  * (next_factor)            // 1.0 if tagged +next
          - 5.0  * (blocked_factor)         // hard demote if dependencies unmet
          - 2.0  * (waiting_factor)         // start_date in future
```

`active_factor` is the cross-domain hook: a task currently being time-tracked floats to the top of `Today`. Cache is recomputed on any write to the task or its dependencies; a background job recomputes time-sensitive factors (`deadline`, `age`) once per hour.

#### 2.1.5 UDAs (User-Defined Attributes)

**Decision:** Hybrid storage. UDAs are declared in config:

```toml
[uda.estimate]
type = "duration"
label = "Estimated effort"
default = "30m"

[uda.energy]
type = "enum"
values = ["low", "med", "high"]
```

Declared UDAs are *projected* into typed virtual columns at query time (via SQLite generated columns + `json_extract`) so filters like `energy:high` are indexable. Undeclared UDAs are still stored in `tasks.udas` JSONB and queryable via `udas.foo:bar` but unindexed. This gives schema-free extensibility without the schema-explosion of a separate `uda_values` table.

Supported UDA types: `string`, `int`, `float`, `bool`, `enum`, `duration`, `date`, `task_ref`.

#### 2.1.6 Recurrence

Two flavors:

- **Periodic** — fires on a calendar (`every monday`, `every 1st of month`, `every 90 days from start_date`). New instance materialized whether or not previous was done.
- **Chained** — next instance materializes *N* time after previous was *completed*. `recur: 7d after completion`.

```rust
enum RecurrenceSpec {
    Periodic { rrule: RRule, until: Option<Date>, count: Option<u32> },
    Chained  { interval: Duration, anchor: ChainAnchor /* Completion | Cancellation */ },
}
```

Template task (`status: 'recurring'`, never shown in normal views) owns the spec. Materialized instances inherit fields but have their own UUID, SID, urgency. Editing template offers "apply to future only" or "apply to all open instances."

**Edge case:** Editing a materialized instance creates a per-instance override stored as a sparse JSON delta on the instance; template edits do not clobber it (CRDT-style LWW per field).

#### 2.1.7 Filter / query language

A single grammar shared by `tock list`, saved reports, and the TUI search bar. EBNF (abridged):

```
query     := expr (WS expr)*
expr      := negation? (group | predicate)
negation  := '!' | 'not'
group     := '(' query (' or '|' and ' query)* ')'
predicate := field ':' value
           | '+' tag
           | '-' tag
           | '+' virtual_tag
field     := IDENT ('.' IDENT)*       // e.g. udas.energy, project.name
value     := scalar | range | regex
range     := scalar? '..' scalar?     // dates and numbers
regex     := '/' .* '/'
```

Examples:

```bash
tock list status:pending +work due.before:eow -BLOCKED
tock list project:home and (priority:H or +next) and energy:high
tock list "annotations:/postmortem/" modified:7d..
tock list udas.estimate:>=2h scheduled_for:today
```

Virtual tags (auto-derived, not stored): `+OVERDUE`, `+TODAY`, `+WEEK`, `+MONTH`, `+BLOCKED`, `+BLOCKING`, `+ACTIVE` (currently being timed), `+FOCUS` (in active Pomodoro), `+SCHEDULED`, `+UNTAGGED`, `+ANNOTATED`, `+CHILD` (recurrence instance), `+ORPHAN` (deleted project).

#### 2.1.8 Soft delete, annotations, dependencies

- **Soft delete** — `tock delete <sid>` sets `deleted_at`; entry hidden from all views except `tock trash`. `tock purge` permanently removes after 30 days (configurable).
- **Annotations** — append-only: `tock annotate <sid> "blocked on legal review"`. Each carries a timestamp and contributes to `+ANNOTATED` virtual tag. Sync as separate events (never overwritten).
- **Dependencies** — `tock modify <sid> depends:42,43`. Cycle detection on insert (DFS); rejected with `error: would create dependency cycle 42 → 43 → 42`. Dependents auto-recompute `+BLOCKED` virtual tag.

#### 2.1.9 Context filtering

A *context* is a named global filter. `tock context work` sets `+work and project.area:work` as an implicit AND on every query until `tock context none`. Stored per-device (not synced), so a phone can be in `personal` context while the desktop is in `work`.

#### 2.1.10 CLI examples (task)

```bash
# Capture to inbox (natural)
tock add "Email Sara about the Q3 contract" +work due:fri

# Capture with project + deadline + priority
tock add "Draft architecture RFC" project:work.backend due:2025-11-15 priority:H +next

# Move from inbox to project
tock modify 42 project:home.repairs heading:"Kitchen"

# Start working (also starts a time block — see §2.5)
tock start 42

# Complete (also stops timer; closes Pomodoro if linked)
tock done 42

# Today view with custom sort
tock today --sort=urgency,deadline

# Saved report
tock report next   # alias for: status:pending +next limit:10 sort:urgency-

# Annotate
tock annotate 42 "Sara replied — needs legal review first"
```

---

### 2.2 Habit Tracking

Habits are first-class entities (not just recurring tasks) with optional identity statements, cues, cravings, responses, and rewards, plus habit stacking.

#### 2.2.1 Entity model

```rust
struct Habit {
    id: Uuid,
    sid: u32,
    title: String,                       // "Read 10 pages"
    identity: Option<String>,            // "I am a reader"
    cue: Option<String>,                 // "After morning coffee"
    craving: Option<String>,             // "Quiet focused start to my day"
    response: Option<String>,            // "Open book, read 10 pages"
    reward: Option<String>,              // "Mark off, sip tea"
    direction: HabitDirection,           // Build | Break
    cadence: Cadence,                    // see below
    minimum: Minimum,                    // "start small": 2 pages, 1 minute, etc.
    stack_after: Option<Uuid>,           // habit stacking — fires nudge after this habit done
    area_id: Option<Uuid>,
    project_id: Option<Uuid>,            // optional
    tags: Vec<String>,
    level: u32,                          // progression — see §2.2.4
    xp: u32,                             // toward next level
    streak_current: u32,
    streak_best: u32,
    reminders: Vec<Reminder>,            // flexible — see §2.2.5
    accountability: Option<Accountability>, // see §2.2.7
    created_at: DateTime<Utc>,
    archived_at: Option<DateTime<Utc>>,
}

enum Cadence {
    Daily,
    WeeklyTarget { times_per_week: u8 },         // "4×/week, any day"
    SpecificDays { days: BTreeSet<Weekday> },     // Mon/Wed/Fri
    EveryNDays { n: u8, anchor: NaiveDate },
    Custom(RRule),
}

enum Minimum {
    Count(u32),          // 2 pushups
    Duration(Duration),  // 1 minute meditation
    Boolean,             // just "did it"
}

enum HabitDirection { Build, Break }
```

`Break` habits invert progression: each *avoided* day = XP. They use cues like "when I feel the urge to check Twitter" and rewards like "10 deep breaths." Streaks count days *without* the behavior. A "slip" log entry resets streak but preserves XP (no demotion on a single slip — see §2.2.4).

#### 2.2.2 Completion model

```rust
struct HabitEntry {
    id: Uuid,
    habit_id: Uuid,
    occurred_at: DateTime<Utc>,
    amount: EntryAmount,    // Count(n) | Duration(d) | Bool(true)
    notes: Option<String>,
    slip: bool,             // only for Break habits
    source: EntrySource,    // Cli | Timer | Apple | Sync
}
```

Multiple entries per day are allowed (e.g. three 1-min meditations = 3 minutes). For `Cadence::WeeklyTarget`, completion is computed across the rolling 7-day window.

#### 2.2.3 Streaks (with grace)

Default rule: a habit "is on streak" if it met its cadence for *N* consecutive periods. **Two configurable graces:**

1. **Skip days** — N user-declared days/year (default 12) that don't break a streak. `tock habit skip 7 --reason "travel"`.
2. **Freeze** — at risk of breaking? `tock habit freeze 7` consumes one of M (default 3) monthly freezes; counts as "done" for that day.

This explicitly rejects punitive streak design; behavior change requires elasticity.

#### 2.2.4 Progression / leveling

Habits accrue XP per successful period. Level thresholds are Fibonacci-scaled:

```
Level 1 → 0 XP        (Spark)
Level 2 → 5 XP        (Starter)
Level 3 → 13 XP       (Established)
Level 4 → 34 XP       (Steady)
Level 5 → 89 XP       (Anchored)
Level 6 → 233 XP      (Identity)
Level 7 → 610 XP      (Embodied)
```

When `level ≥ 5` ("Anchored"), the system suggests promoting `minimum` (the "make it harder" progression). UI surfaces: *"You've meditated 1 minute daily for 89 days. Promote minimum to 2 minutes?"*

XP decay is **soft**: missing a period costs 0 XP but pauses gain; missing > cadence × 3 triggers a "Re-spark" wizard offering to lower minimum or change cue.

#### 2.2.5 Reminders (flexible)

```rust
enum Reminder {
    At { local_time: NaiveTime, weekdays: BTreeSet<Weekday> },
    AfterHabit { habit_id: Uuid, delay: Duration },        // habit stacking
    AfterLocation { region_id: String, dwell: Duration },  // Apple only
    BeforeSleep { minutes_before_typical: u32 },           // learned from Apple Health
    AdaptiveTimeOfDay,                                      // bandit over user response
}
```

`AdaptiveTimeOfDay` runs entirely in core: a tiny ε-greedy bandit over six daypart buckets learns when nudges are accepted vs. dismissed. No remote ML.

#### 2.2.6 Habit stacking

Explicit field `stack_after: Option<Uuid>`. When the upstream habit logs an entry, a nudge is scheduled at `now + delay`. Stacks chain (A → B → C) but are validated against cycles.

#### 2.2.7 Accountability (optional)

Per-habit `Accountability` config:

```rust
struct Accountability {
    partner_pubkey: Option<X25519PublicKey>,    // E2EE share
    cadence: ReportCadence,                     // weekly summary
    auto_share: ShareFields,                    // streak only, or full
}
```

When configured, the sync layer emits an additional encrypted stream readable by the partner's public key (envelope encryption — partner cannot read other habits). Implemented as a separate "share" event type; see §6.

#### 2.2.8 CLI examples (habits)

```bash
# Guided creation
tock habit new --guided
# >> Identity: "I am a writer"
# >> Cue:       "After morning coffee"
# >> Response:  "Write 1 sentence"
# >> Reward:    "Drink tea slowly"
# >> Minimum:   1 sentence (start small!)
# >> Cadence:   daily

# Quick add
tock habit add "Read 10 pages" --identity "I am a reader" --cue "after dinner" --min 2pages --daily

# Log
tock habit log read                       # boolean / minimum
tock habit log read --amount 25pages      # over-delivery, XP still capped
tock habit done read                      # alias for log

# Slip (break habit)
tock habit add "No social media before noon" --break --daily
tock habit slip social --notes "checked twitter at 9am"

# Stacking
tock habit add "Stretch 2 min" --stack-after read --delay 10m

# Status
tock habit status
# >> read       L4 (Steady)   ████████░░  47/89 XP   streak 12d  next: today
# >> stretch    L2 (Starter)  ███░░░░░░░   3/13 XP   streak 3d   next: today
# >> social     L3 (Estd, ↓)  ████░░░░░░  18/13 XP   clean 8d    next: today

# Skip
tock habit skip read --reason "flu"

# Freeze
tock habit freeze stretch --tomorrow
```

---

### 2.3 Time Tracking

The time-tracking domain owns "blocks" — closed intervals of attention against a project/task/tag set.

#### 2.3.1 Entity model

```rust
struct TimeBlock {
    id: Uuid,
    sid: u32,
    title: String,                  // human label, may differ from task title
    start: DateTime<Utc>,
    end: Option<DateTime<Utc>>,     // None = currently running
    project_id: Option<Uuid>,
    task_id: Option<Uuid>,          // strong link; affects task urgency (+ACTIVE)
    habit_id: Option<Uuid>,         // optional: time spent on a habit
    pomodoro_id: Option<Uuid>,      // set when block originated from focus timer
    tags: Vec<String>,
    notes: Option<String>,
    source: BlockSource,            // Manual | Timer | Pomodoro | AppleAuto | Imported
    billable: bool,                 // UDA-promoted because common
    rate_cents: Option<u32>,        // optional
}
```

Only **one** block per device may have `end IS NULL` (the "active block"). Switching tasks auto-stops and starts a new block.

#### 2.3.2 Natural language CLI

Both natural and flag forms are accepted; they parse to the same AST.

```bash
# Natural
tock start "Deep work" on work/backend 2 hours ago
tock start writing for habit read
tock stop
tock stop 5 minutes ago

# Equivalent flag form
tock start --title "Deep work" --project work/backend --ago 2h
tock start --title writing --habit read
tock stop --at "-5m"

# On-the-fly creation
tock start "Customer call: Acme" on +client.acme              # creates project if missing
tock start writing --habit "Morning pages" --create-habit     # creates habit if missing
```

Date/time parser supports: `2h ago`, `yesterday 14:00`, `last monday`, `eow`, `1pm`, `-5m`, ISO 8601. Implemented in core (no `chrono-english` runtime dep — custom recursive-descent parser shared with task scheduling).

#### 2.3.3 SIDs and on-the-fly resolution

`SID` is a workspace-local u32. CLI accepts `42`, `t42` (task), `h42` (habit), `b42` (block), `p42` (project). Bare numbers resolve in this order: active block → today's tasks → all open tasks → projects. Ambiguity returns an explicit prompt.

#### 2.3.4 Reporting

```bash
tock report time today
tock report time week --by project
tock report time 2025-10-01..2025-10-31 --by tag --format csv
tock report time --task 42 --format json
tock report time --billable --by project --rate-default 12500    # cents/hr
```

Reports are pluggable Tera templates in `~/.config/Tock/reports/`. Built-in formats: `table` (default), `csv`, `json`, `markdown`, `ical`, `ledger` (plain-text accounting compatible).

Sample table output:

```
Week of 2025-10-13 — total 32h 17m

PROJECT              TIME     %        BILLABLE
work.backend.api     14h 02m  43.4%    14h 02m  ($1,752.50)
work.docs             6h 30m  20.1%     6h 30m  (  $812.50)
home.repairs          4h 15m  13.2%        —
personal.reading      3h 50m  11.9%        —    (habit: read +47xp)
(untracked)           3h 40m  11.4%
```

#### 2.3.5 Edge cases

- **Clock jumps / time zones** — all blocks stored as UTC; display in local TZ; DST-safe via `time` crate.
- **Forgot to stop** — `tock doctor` detects blocks > 8h (configurable) and prompts retroactive truncation.
- **Overlap from sync** — two devices running concurrent blocks: keep both; tag the loser `+SYNC_OVERLAP`; reporting deduplicates by `max(start, other.end)` per device with a clear annotation.
- **Backdating into another block** — refuse with `error: would overlap block b17 (10:00–11:30)`; offer `--split` to trim the existing block.

---

### 2.4 Focus Timer (Pomodoro)

#### 2.4.1 Configuration

```toml
[focus]
work_minutes = 25
short_break_minutes = 5
long_break_minutes = 15
cycles_before_long_break = 4
auto_start_breaks = true
auto_start_next_cycle = false
strict = false                     # if true, pausing aborts the cycle
sound_pack = "subtle"              # subtle | classic | silent
do_not_disturb = true              # set Apple Focus mode while running

# Cross-domain
on_complete_log_habit = "deep_work"   # habit SID to auto-log
on_complete_min_for_habit = 1         # cycles required to log habit
```

#### 2.4.2 Session model

```rust
struct FocusSession {
    id: Uuid,
    started_at: DateTime<Utc>,
    ended_at: Option<DateTime<Utc>>,
    task_id: Option<Uuid>,
    project_id: Option<Uuid>,
    planned_cycles: u32,
    completed_cycles: u32,
    interruptions: Vec<Interruption>,
    state: FocusState,
}

enum FocusState { Working, ShortBreak, LongBreak, Paused, Aborted, Completed }

struct Interruption {
    at: DateTime<Utc>,
    kind: InterruptionKind,           // Internal | External
    note: Option<String>,
}
```

#### 2.4.3 Cross-domain integration

When a cycle completes:

1. A `TimeBlock { source: Pomodoro, pomodoro_id, task_id }` is closed for the cycle's duration.
2. If `task_id` is set, the task's `udas.pomodoros` is incremented and its `urgency` recomputed.
3. If `on_complete_log_habit` is set and `completed_cycles >= on_complete_min_for_habit`, a `HabitEntry { source: Timer }` is appended.
4. If the task has a deadline within 24h and the session aborted, a soft notification suggests rescheduling.

#### 2.4.4 Statistics

```bash
tock focus stats today
tock focus stats week
# >> Completed:    18 cycles  (7h 30m focus)
# >> Aborted:       3 cycles  (longest streak: 6)
# >> Best time of day: 09:00–11:00 (12 cycles)
# >> Top task: "Draft architecture RFC" (5 cycles)
# >> Habit credit: deep_work +18xp
```

#### 2.4.5 CLI examples (focus)

```bash
tock focus start                       # uses default config, no task
tock focus start --task 42             # bound to task
tock focus start --cycles 3 --task 42  # plan a fixed number
tock focus pause
tock focus resume
tock focus skip                        # skip current break
tock focus abort                       # ends session, logs partial block
tock focus status                      # live status (also via TUI)
```

In TUI: `f` opens the focus pane with a live countdown ring and current task; `Space` pauses, `s` skips break, `a` aborts.

---

### 2.5 Cross-Domain Integration

The four domains share IDs, events, and the urgency engine. Key integration points:

| Integration                                          | Mechanism                                                              |
|------------------------------------------------------|------------------------------------------------------------------------|
| Focus cycle → Time block                             | `FocusSession` close handler writes `TimeBlock { source: Pomodoro }`.  |
| Focus cycle → Habit XP                               | `on_complete_log_habit` config; idempotent on cycle UUID.              |
| `tock start <task>` (timer) auto-starts task           | Task `status: started` if `pending`; `+ACTIVE` virtual tag; urgency↑.  |
| `tock done <task>` stops active block + focus          | If block's `task_id` matches, stop block; if focus's `task_id` matches, complete focus. |
| Habit completion can satisfy a task                  | `Task.satisfied_by_habit: Option<Uuid>`. When that habit logs ≥ minimum today, task auto-completes (recurring case). |
| Project aggregation                                  | `tock project show work.backend` lists tasks + open habits scoped to project + week's time blocks. |
| Recurring task ↔ Habit promotion                     | `tock task promote-to-habit 42` archives the recurring template and creates a habit with the same cadence; existing instances stay as-is. |

#### 2.5.1 Edge cases

- **Deleted recurring task with linked habit**: deleting the template does *not* delete the habit. Habit retains `source_task_id` for provenance.
- **Habit deleted while focus session credits it**: focus session continues; on completion, the habit credit is silently skipped (logged to `tock doctor` for visibility).
- **Time block on archived project**: allowed (you may legitimately log time to a closed project); UI shows project name struck through.
- **Sync conflict on `Task.status`**: LWW per field by `(lamport, device_id)`; the `status: done` write wins only if its causality dominates a concurrent `status: cancelled` (see §6).
- **Block ends while focus is still running** (e.g. manual `tock stop`): focus session continues but its time block is recreated on next cycle complete; the manually stopped block is preserved.

---

## 3. Data Model

### 3.1 ASCII ER diagram

```
                   ┌────────┐
                   │  Area  │
                   └───┬────┘
                       │ 1:N
                ┌──────▼──────┐
                │   Project   │◄────────┐
                └──────┬──────┘         │
                   1:N │                │ N:1 (optional)
                ┌─────▼─────┐           │
                │  Heading  │           │
                └─────┬─────┘           │
                  1:N │                 │
                ┌─────▼─────┐           │
   ┌────────┐   │   Task    │───────────┤
   │ Annot. │◄──┤           │           │
   └────────┘   └──┬──┬──┬──┘           │
                   │  │  │              │
              1:N  │  │  │ N:N (deps)   │
        ┌──────────▼┐ │  │              │
        │ Checklist │ │  │              │
        │   Item    │ │  │              │
        └───────────┘ │  │              │
                      │  │              │
                      │  └───self-join──┘
                      │
              ┌───────▼──────┐         ┌──────────┐
              │  TimeBlock   │◄────────┤  Focus   │
              │              │         │ Session  │
              └───┬──────────┘         └────┬─────┘
                  │ N:1 optional            │
              ┌───▼────┐                    │
              │ Habit  │◄───────────────────┘ (on_complete_log_habit)
              └───┬────┘
              1:N │
              ┌───▼────────┐
              │ HabitEntry │
              └────────────┘

(Tags are an N:N join over Task | Habit | TimeBlock | Project via `entity_tags`.)
```

### 3.2 SQLite schema (canonical)

> SQLite stores all timestamps as ISO 8601 TEXT in UTC (`YYYY-MM-DDTHH:MM:SS.fffZ`). Booleans as INTEGER 0/1. UUIDs as 16-byte BLOB (`uuid_v7`). Encrypted column values are wrapped per §5; this schema shows the *plaintext logical* view available to the core after decryption.

```sql
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;
PRAGMA application_id = 0x4B414644;  -- 'KAFD'
PRAGMA user_version  = 1;             -- bumped on every migration

-- ───────────── workspace meta ─────────────
CREATE TABLE workspace_meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
-- seeds: schema_version, vault_id, created_at, default_context

-- ───────────── areas / projects ─────────────
CREATE TABLE areas (
    id           BLOB PRIMARY KEY,           -- uuidv7
    name         TEXT NOT NULL,
    color        TEXT,
    sort_order   INTEGER NOT NULL DEFAULT 0,
    archived_at  TEXT,
    created_at   TEXT NOT NULL,
    modified_at  TEXT NOT NULL
);

CREATE TABLE projects (
    id            BLOB PRIMARY KEY,
    sid           INTEGER NOT NULL UNIQUE,
    area_id       BLOB REFERENCES areas(id) ON DELETE SET NULL,
    name          TEXT NOT NULL,
    notes         TEXT,
    deadline      TEXT,
    status        TEXT NOT NULL DEFAULT 'active' CHECK (status IN
                  ('active','someday','paused','done','cancelled')),
    sort_order    INTEGER NOT NULL DEFAULT 0,
    done_at       TEXT,
    cancelled_at  TEXT,
    archived_at   TEXT,
    created_at    TEXT NOT NULL,
    modified_at   TEXT NOT NULL
);
CREATE INDEX projects_status_idx ON projects(status) WHERE archived_at IS NULL;
CREATE INDEX projects_area_idx   ON projects(area_id);

CREATE TABLE headings (
    id          BLOB PRIMARY KEY,
    project_id  BLOB NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    sort_order  INTEGER NOT NULL DEFAULT 0,
    created_at  TEXT NOT NULL,
    modified_at TEXT NOT NULL
);
CREATE INDEX headings_project_idx ON headings(project_id);

-- ───────────── tasks ─────────────
CREATE TABLE tasks (
    id              BLOB PRIMARY KEY,
    sid             INTEGER NOT NULL UNIQUE,
    title           TEXT NOT NULL,
    notes           TEXT,
    status          TEXT NOT NULL CHECK (status IN
                    ('inbox','pending','started','done','cancelled','someday','recurring')),
    area_id         BLOB REFERENCES areas(id)    ON DELETE SET NULL,
    project_id      BLOB REFERENCES projects(id) ON DELETE SET NULL,
    heading_id      BLOB REFERENCES headings(id) ON DELETE SET NULL,
    parent_id       BLOB REFERENCES tasks(id)    ON DELETE SET NULL,  -- recurrence template
    start_date      TEXT,
    deadline        TEXT,
    scheduled_for   TEXT,
    evening         INTEGER NOT NULL DEFAULT 0,
    priority        TEXT CHECK (priority IN ('L','M','H')),
    recurrence      TEXT,        -- JSON (RecurrenceSpec)
    udas            TEXT NOT NULL DEFAULT '{}',  -- JSON
    urgency_cache   REAL NOT NULL DEFAULT 0.0,
    satisfied_by_habit BLOB REFERENCES habits(id) ON DELETE SET NULL,
    created_at      TEXT NOT NULL,
    modified_at     TEXT NOT NULL,
    done_at         TEXT,
    cancelled_at    TEXT,
    deleted_at      TEXT
);
CREATE INDEX tasks_status_idx       ON tasks(status) WHERE deleted_at IS NULL;
CREATE INDEX tasks_project_idx      ON tasks(project_id);
CREATE INDEX tasks_deadline_idx     ON tasks(deadline) WHERE deadline IS NOT NULL;
CREATE INDEX tasks_start_date_idx   ON tasks(start_date) WHERE start_date IS NOT NULL;
CREATE INDEX tasks_urgency_idx      ON tasks(urgency_cache DESC) WHERE status IN ('pending','started');
CREATE INDEX tasks_modified_idx     ON tasks(modified_at);

CREATE TABLE task_dependencies (
    task_id       BLOB NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    depends_on_id BLOB NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    PRIMARY KEY (task_id, depends_on_id)
);
CREATE INDEX task_deps_rev_idx ON task_dependencies(depends_on_id);

CREATE TABLE checklist_items (
    id          BLOB PRIMARY KEY,
    task_id     BLOB NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    title       TEXT NOT NULL,
    done_at     TEXT,
    sort_order  INTEGER NOT NULL DEFAULT 0,
    created_at  TEXT NOT NULL,
    modified_at TEXT NOT NULL
);
CREATE INDEX checklist_task_idx ON checklist_items(task_id);

CREATE TABLE annotations (
    id         BLOB PRIMARY KEY,
    entity_id  BLOB NOT NULL,           -- polymorphic
    entity_kind TEXT NOT NULL CHECK (entity_kind IN ('task','project','habit','block')),
    body       TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX annotations_entity_idx ON annotations(entity_id, entity_kind);

-- ───────────── tags (N:N) ─────────────
CREATE TABLE tags (
    id    BLOB PRIMARY KEY,
    name  TEXT NOT NULL UNIQUE,         -- "home/repairs/electrical"
    color TEXT
);

CREATE TABLE entity_tags (
    entity_id   BLOB NOT NULL,
    entity_kind TEXT NOT NULL CHECK (entity_kind IN ('task','project','habit','block')),
    tag_id      BLOB NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    PRIMARY KEY (entity_id, entity_kind, tag_id)
);
CREATE INDEX entity_tags_tag_idx ON entity_tags(tag_id);

-- ───────────── habits ─────────────
CREATE TABLE habits (
    id             BLOB PRIMARY KEY,
    sid            INTEGER NOT NULL UNIQUE,
    title          TEXT NOT NULL,
    identity       TEXT,
    cue            TEXT,
    craving        TEXT,
    response       TEXT,
    reward         TEXT,
    direction      TEXT NOT NULL CHECK (direction IN ('build','break')),
    cadence        TEXT NOT NULL,            -- JSON (Cadence)
    minimum        TEXT NOT NULL,            -- JSON (Minimum)
    stack_after    BLOB REFERENCES habits(id) ON DELETE SET NULL,
    stack_delay_s  INTEGER NOT NULL DEFAULT 0,
    area_id        BLOB REFERENCES areas(id)    ON DELETE SET NULL,
    project_id     BLOB REFERENCES projects(id) ON DELETE SET NULL,
    level          INTEGER NOT NULL DEFAULT 1,
    xp             INTEGER NOT NULL DEFAULT 0,
    streak_current INTEGER NOT NULL DEFAULT 0,
    streak_best    INTEGER NOT NULL DEFAULT 0,
    reminders      TEXT NOT NULL DEFAULT '[]', -- JSON (Vec<Reminder>)
    accountability TEXT,                       -- JSON (Accountability)
    source_task_id BLOB REFERENCES tasks(id) ON DELETE SET NULL,
    created_at     TEXT NOT NULL,
    modified_at    TEXT NOT NULL,
    archived_at    TEXT
);
CREATE INDEX habits_active_idx ON habits(archived_at) WHERE archived_at IS NULL;

CREATE TABLE habit_entries (
    id          BLOB PRIMARY KEY,
    habit_id    BLOB NOT NULL REFERENCES habits(id) ON DELETE CASCADE,
    occurred_at TEXT NOT NULL,
    amount      TEXT NOT NULL,        -- JSON (EntryAmount)
    notes       TEXT,
    slip        INTEGER NOT NULL DEFAULT 0,
    source      TEXT NOT NULL CHECK (source IN ('cli','timer','apple','sync','import')),
    created_at  TEXT NOT NULL
);
CREATE INDEX habit_entries_habit_idx ON habit_entries(habit_id, occurred_at DESC);

CREATE TABLE habit_skips (
    id          BLOB PRIMARY KEY,
    habit_id    BLOB NOT NULL REFERENCES habits(id) ON DELETE CASCADE,
    date        TEXT NOT NULL,
    kind        TEXT NOT NULL CHECK (kind IN ('skip','freeze')),
    reason      TEXT,
    created_at  TEXT NOT NULL,
    UNIQUE (habit_id, date)
);

-- ───────────── time tracking ─────────────
CREATE TABLE time_blocks (
    id           BLOB PRIMARY KEY,
    sid          INTEGER NOT NULL UNIQUE,
    title        TEXT NOT NULL,
    start_ts     TEXT NOT NULL,
    end_ts       TEXT,                                -- NULL = running
    project_id   BLOB REFERENCES projects(id) ON DELETE SET NULL,
    task_id      BLOB REFERENCES tasks(id)    ON DELETE SET NULL,
    habit_id     BLOB REFERENCES habits(id)   ON DELETE SET NULL,
    pomodoro_id  BLOB REFERENCES focus_sessions(id) ON DELETE SET NULL,
    notes        TEXT,
    source       TEXT NOT NULL CHECK (source IN ('manual','timer','pomodoro','apple_auto','imported')),
    billable     INTEGER NOT NULL DEFAULT 0,
    rate_cents   INTEGER,
    created_at   TEXT NOT NULL,
    modified_at  TEXT NOT NULL
);
CREATE INDEX time_blocks_start_idx    ON time_blocks(start_ts);
CREATE INDEX time_blocks_open_idx     ON time_blocks(end_ts) WHERE end_ts IS NULL;
CREATE INDEX time_blocks_task_idx     ON time_blocks(task_id);
CREATE INDEX time_blocks_project_idx  ON time_blocks(project_id);
-- Enforce single active block per device at the app layer
-- (cannot be a partial UNIQUE in SQLite without a sentinel; checked in core).

-- ───────────── focus sessions ─────────────
CREATE TABLE focus_sessions (
    id               BLOB PRIMARY KEY,
    sid              INTEGER NOT NULL UNIQUE,
    started_at       TEXT NOT NULL,
    ended_at         TEXT,
    task_id          BLOB REFERENCES tasks(id)    ON DELETE SET NULL,
    project_id       BLOB REFERENCES projects(id) ON DELETE SET NULL,
    planned_cycles   INTEGER NOT NULL,
    completed_cycles INTEGER NOT NULL DEFAULT 0,
    state            TEXT NOT NULL CHECK (state IN
                     ('working','short_break','long_break','paused','aborted','completed')),
    config_snapshot  TEXT NOT NULL,    -- JSON of focus config at start
    created_at       TEXT NOT NULL,
    modified_at      TEXT NOT NULL
);
CREATE INDEX focus_started_idx ON focus_sessions(started_at);

CREATE TABLE focus_interruptions (
    id         BLOB PRIMARY KEY,
    session_id BLOB NOT NULL REFERENCES focus_sessions(id) ON DELETE CASCADE,
    at         TEXT NOT NULL,
    kind       TEXT NOT NULL CHECK (kind IN ('internal','external')),
    note       TEXT
);

-- ───────────── reports / saved queries ─────────────
CREATE TABLE saved_reports (
    id          BLOB PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE,
    query       TEXT NOT NULL,           -- filter DSL
    sort        TEXT,
    columns     TEXT NOT NULL DEFAULT '[]',
    template    TEXT,                    -- optional Tera template name
    created_at  TEXT NOT NULL,
    modified_at TEXT NOT NULL
);

-- ───────────── sync (event log) ─────────────
-- Detailed in §6; included here for completeness.
CREATE TABLE events (
    id              BLOB PRIMARY KEY,                  -- uuidv7
    device_id       BLOB NOT NULL,
    lamport         INTEGER NOT NULL,
    vector_clock    BLOB NOT NULL,                      -- CBOR-encoded
    parent_event_id BLOB,
    entity_kind     TEXT NOT NULL,
    entity_id       BLOB NOT NULL,
    op              TEXT NOT NULL,                      -- create|update|delete|annotate|log...
    payload_ct      BLOB NOT NULL,                      -- per-event ciphertext (AES-256-GCM)
    payload_nonce   BLOB NOT NULL,
    aad             BLOB NOT NULL,
    created_at      TEXT NOT NULL
);
CREATE INDEX events_entity_idx    ON events(entity_kind, entity_id, lamport);
CREATE INDEX events_device_idx    ON events(device_id, lamport);
CREATE INDEX events_created_idx   ON events(created_at);

CREATE TABLE snapshots (
    id            BLOB PRIMARY KEY,
    upto_event_id BLOB NOT NULL,
    blob_ct       BLOB NOT NULL,
    blob_nonce    BLOB NOT NULL,
    created_at    TEXT NOT NULL
);

-- ───────────── migrations ─────────────
CREATE TABLE schema_migrations (
    version    INTEGER PRIMARY KEY,
    applied_at TEXT NOT NULL,
    checksum   TEXT NOT NULL              -- sha256 of migration SQL
);

-- ───────────── SID allocator (per-workspace counter) ─────────────
CREATE TABLE sid_counters (
    kind TEXT PRIMARY KEY,   -- 'task' | 'habit' | 'block' | 'project' | 'focus'
    next INTEGER NOT NULL
);
```

### 3.3 UDA storage strategy

**Decision:** Hybrid — `tasks.udas TEXT (JSON)` is canonical storage; declared UDAs are projected to virtual columns via SQLite *generated columns* on demand:

```sql
ALTER TABLE tasks ADD COLUMN uda_energy TEXT
    GENERATED ALWAYS AS (json_extract(udas, '$.energy')) VIRTUAL;
CREATE INDEX tasks_uda_energy_idx ON tasks(uda_energy)
    WHERE json_extract(udas, '$.energy') IS NOT NULL;
```

Rationale: a separate EAV `uda_values` table would force JOINs on every list query (hot path). JSON-in-column gives us LWW-friendly diffing for sync and zero-cost reads for unindexed UDAs. The generated-column projection costs nothing if not declared.

### 3.4 Migration strategy

- `PRAGMA user_version` is the source of truth.
- Migrations are numbered, embedded in the `tock-core` crate via `include_str!`, and contain:
  - `up.sql` — schema changes
  - `data.rs` — optional Rust-side data backfill (re-encryption, re-derive caches)
  - `checksum` — sha256 verified at startup
- On open, core compares `user_version` to highest-known migration; missing migrations run in a single transaction.
- **Backward-compat rule:** within a major version, additive only. Destructive changes require a major bump and an export/re-import migration. The vault format header (§5.2) carries a `min_compatible_version`; older clients refuse to open newer vaults.

### 3.5 Common-query indexes (rationale)

| Query                                    | Index                                                         |
|------------------------------------------|---------------------------------------------------------------|
| `Today` view                             | `tasks_urgency_idx`, `tasks_deadline_idx`, `tasks_start_date_idx` |
| `Upcoming`                               | `tasks_start_date_idx`, `tasks_deadline_idx`                  |
| Project drill-down                       | `tasks_project_idx`, `headings_project_idx`                   |
| Time report by project                   | `time_blocks_project_idx`, `time_blocks_start_idx`            |
| Currently running                        | `time_blocks_open_idx` (partial)                              |
| Habit history                            | `habit_entries_habit_idx`                                     |
| Sync delta pull                          | `events_device_idx`, `events_created_idx`                     |
| Tag search                               | `entity_tags_tag_idx`                                         |

---

## 4. Architecture

### 4.1 Crate / workspace layout

```
Tock/
├── Cargo.toml                      # workspace
├── rust-toolchain.toml             # pinned stable, edition = "2024"
├── deny.toml                       # cargo-deny
├── crates/
│   ├── tock-core/               # PURE: zero I/O, zero net, zero async runtime
│   ├── tock-crypto/             # PURE: key hierarchy, AEAD, SRP, recovery codes
│   ├── tock-storage/            # SQLite adapter (sync rusqlite), schema, migrations
│   ├── tock-sync/               # event log, conflict res, transport trait (no I/O impl)
│   ├── tock-parse/              # filter DSL + natural-language date/time parser
│   ├── tock-cli/                # clap subcommands + ratatui TUI binary `tock`
│   ├── tock-server/             # Axum sync server (AGPL-3.0)
│   ├── tock-import/             # Things 3, Taskwarrior, CSV, JSON importers
│   ├── tock-export/             # JSON, CSV, Markdown, iCal, ledger
│   └── tock-uniffi/             # UniFFI scaffolding crate (cdylib + udl)
├── bindings/
│   └── swift/                      # generated Swift package
├── apps/
│   ├── ios/                        # SwiftUI iOS + iPadOS + watchOS + widgets
│   ├── macos/                      # SwiftUI macOS (shares iOS code)
│   └── web/                        # Next.js + WASM (tock-core compiled to wasm32)
├── docs/
└── xtask/                          # cargo-xtask: build orchestration, codegen
```

#### 4.1.1 Crate responsibilities & invariants

| Crate              | License     | I/O? | Async? | Depends on                                     |
|--------------------|-------------|------|--------|------------------------------------------------|
| `tock-core`     | Apache-2.0  | NO   | NO     | `tock-crypto`, `tock-parse`, `serde`, `time`, `uuid`, `zeroize` |
| `tock-crypto`   | Apache-2.0  | NO   | NO     | `aes-gcm`, `argon2`, `hkdf`, `x25519-dalek`, `srp`, `zeroize`, `subtle` |
| `tock-parse`    | Apache-2.0  | NO   | NO     | `winnow` (or hand-rolled), `time`              |
| `tock-storage`  | Apache-2.0  | YES (disk) | NO | `tock-core`, `rusqlite` (bundled)         |
| `tock-sync`     | Apache-2.0  | trait-only | trait-only | `tock-core`, `tock-storage`     |
| `tock-import`   | Apache-2.0  | YES  | NO     | `tock-core`, `tock-storage`              |
| `tock-export`   | Apache-2.0  | YES  | NO     | `tock-core`, `tock-storage`              |
| `tock-cli`      | Apache-2.0  | YES  | YES    | all of the above + `clap`, `ratatui`, `tokio` (single-thread) |
| `tock-server`   | AGPL-3.0    | YES  | YES    | `tock-sync`, `axum`, `sqlx`, `tokio`        |
| `tock-uniffi`   | Apache-2.0  | YES (binding shim) | NO | `tock-core`, `tock-storage`, `tock-sync`, `uniffi` |

**Mandatory `Cargo.toml` lints** in every workspace member:

```toml
[lints.rust]
unsafe_code = "forbid"
missing_docs = "warn"
rust_2024_compatibility = "warn"

[lints.clippy]
pedantic = { level = "warn", priority = -1 }
nursery  = { level = "warn", priority = -1 }
unwrap_used = "deny"
expect_used = "deny"
panic = "deny"
todo = "deny"
```

`tock-core` and `tock-crypto` carry the additional invariant enforced by CI: `cargo tree -p tock-core --edges normal` must not list `tokio`, `reqwest`, `rusqlite`, `std::fs`, `std::net`. Verified with a `xtask check-purity` job that scans dependency manifests.

### 4.2 Dependency graph (ASCII)

```
                          ┌────────────────────┐
                          │   tock-crypto   │ (pure)
                          └─────────┬──────────┘
                                    │
                          ┌─────────▼──────────┐    ┌────────────────┐
                          │    tock-core    │◄───┤ tock-parse  │ (pure)
                          └──┬──────┬──────────┘    └────────────────┘
                             │      │
                ┌────────────▼┐    ┌▼────────────────┐
                │ tock-    │    │ tock-sync    │ (trait-only IO)
                │  storage    │    └──┬──────────────┘
                └──┬──────────┘       │
                   │          ┌───────┴────────┐
                   │          │                │
            ┌──────▼──────────▼┐    ┌──────────▼─────────┐
            │  tock-cli     │    │  tock-server    │ (AGPL)
            │  (clap+ratatui)  │    │  (axum)            │
            └──────┬───────────┘    └────────────────────┘
                   │
            ┌──────▼───────────┐    ┌─────────────────────┐
            │ tock-import   │    │ tock-export      │
            └──────────────────┘    └─────────────────────┘

                   ┌──────────────────────────┐
                   │   tock-uniffi (shim)  │
                   └─────────┬────────────────┘
                             │ generates
                   ┌─────────▼────────────────┐
                   │   bindings/swift/        │
                   └─────────┬────────────────┘
                             │
        ┌────────────────────┼───────────────────────┐
        │                    │                       │
   ┌────▼─────┐         ┌────▼─────┐           ┌─────▼─────┐
   │ apps/ios │         │apps/macos│           │watchOS    │
   └──────────┘         └──────────┘           └───────────┘

   ┌──────────────────────────────────────┐
   │  apps/web  ◄── tock-core (wasm32) │
   └──────────────────────────────────────┘
```

### 4.3 Platform bindings

#### 4.3.1 UniFFI (Apple)

- Single `tock-uniffi` crate exposes a high-level facade: `Workspace`, `TaskRepo`, `HabitRepo`, `TimeRepo`, `FocusController`, `SyncClient`.
- `.udl` files generated from `#[uniffi::export]` macros (UniFFI 0.28+).
- Built as a `staticlib` + `cdylib`; xtask script lipos arm64-ios, arm64-ios-sim, arm64-macos, x86_64-macos into an `.xcframework`.
- Async surfaces use UniFFI's async support backed by `tokio` current-thread runtime owned by the shim (never by core).

#### 4.3.2 WASM (web)

- `tock-core` compiled to `wasm32-unknown-unknown` with `wasm-bindgen`.
- Storage in web is an alternate `tock-storage-web` crate using IndexedDB via `idb` crate; same trait as `rusqlite` storage.
- Sync uses `fetch` via `web-sys`.

#### 4.3.3 Feature flags

```toml
# tock-core
[features]
default = []
serde = ["dep:serde"]                # for export crates
schemars = ["dep:schemars"]          # JSON schema generation

# tock-storage
[features]
default = ["sqlite-bundled"]
sqlite-bundled = ["rusqlite/bundled-sqlcipher-vendored-openssl"]
sqlite-system  = ["rusqlite/sqlcipher"]

# tock-cli
[features]
default = ["tui", "completion"]
tui = ["dep:ratatui", "dep:crossterm"]
completion = ["clap_complete"]
```

### 4.4 Build targets and CI/CD

| Target                                | Trigger                  | Output                              |
|---------------------------------------|--------------------------|-------------------------------------|
| `cargo build --workspace`             | every PR                 | debug binaries                      |
| `cargo test --workspace`              | every PR                 | unit + integration                  |
| `cargo clippy --workspace -- -D warnings` | every PR             | lint gate                           |
| `cargo deny check`                    | every PR                 | license + advisory gate             |
| `xtask check-purity`                  | every PR                 | core/crypto have no I/O deps        |
| `xtask cli-snapshot`                  | every PR                 | clap help golden files              |
| `xtask wasm-build`                    | every PR                 | wasm pkg                            |
| `xtask xcframework`                   | tag `v*`, nightly        | Apple xcframework                   |
| `cargo dist`                          | tag `v*`                 | cli binaries (linux, mac, win)      |
| `docker build tock-server`         | tag `v*`                 | server image (AGPL notice baked in) |
| `mdbook build docs/`                  | every push to main       | docs site                           |

Matrix OSes: `ubuntu-latest`, `macos-14` (arm64), `windows-latest`. MSRV pinned in `rust-toolchain.toml`; bumped explicitly.

---

## 5. Encryption & Security Design

Tock's encryption design is adapted for an event-sourced workload.

### 5.1 Key hierarchy

```
                 ┌─────────────────────────┐
                 │  User Password (UTF-8)  │
                 └────────────┬────────────┘
                              │  Argon2id (t=3, m=64 MiB, p=1, 32-byte out)
                              │  salt = vault.kdf_salt (16 B, random per vault)
                              ▼
                 ┌─────────────────────────┐
                 │  Master Key  MK (32 B)  │
                 └────────────┬────────────┘
                              │  HKDF-SHA256, salt = vault.hkdf_salt (32 B)
              ┌───────────────┼──────────────────────────┐
              │ info="Tock/v1/mek"   │ info="Tock/v1/srp-verifier"
              ▼                         ▼
   ┌─────────────────────┐   ┌─────────────────────────────┐
   │  MEK (32 B)         │   │  SRP-6a verifier seed       │
   │  (key-wrap key)     │   │  → used in §5.6             │
   └──────────┬──────────┘   └─────────────────────────────┘
              │  AES-256-GCM (key wrap)
              │  nonce = vault.vk_wrap_nonce
              ▼
   ┌─────────────────────┐
   │  Vault Key  VK (32B)│  ← random per vault, generated at vault creation
   └──────────┬──────────┘
              │  HKDF-SHA256, salt = VK, info = "Tock/v1/item/" || entity_kind
              ▼
   ┌─────────────────────────────────────┐
   │  Domain Key  DK_kind (32 B per kind)│  e.g. DK_task, DK_habit, DK_block, DK_event
   └──────────┬──────────────────────────┘
              │  HKDF-SHA256, salt = DK_kind, info = "item/" || uuid_v7
              ▼
   ┌──────────────────────────────────┐
   │  Item Key  IK (32 B per entity)  │
   └──────────────────────────────────┘
              │  AES-256-GCM
              ▼
       (ciphertext + 12 B nonce + 16 B tag)

   ┌──────────────────────────────────┐
   │ Recovery Key (24 words,          │
   │   Crockford Base32, 256 bits)    │
   │   → HKDF-SHA256                  │
   │   → alternate wrap of VK         │ ← stored as second vk_wrap_ct in header
   └──────────────────────────────────┘
```

Notes:
- **MK and VK are never persisted.** MK exists only in memory during a session and is wiped via `Zeroize` on lock.
- **VK rotation** is supported by re-wrapping with a new MK (password change) or by re-deriving and re-wrapping all items (full rotation; rare).
- **Item Keys are deterministic** from `(VK, kind, uuid)` — no per-item key storage. This is critical for sync: a remote peer holding only VK can decrypt any item by UUID.
- **AAD discipline**: every AEAD operation includes a domain-separated AAD tag (see §5.3).

### 5.2 Vault format (`.kafvault`)

Binary, little-endian, version-prefixed. Header is fixed at 256 bytes; body is the SQLite database file encrypted at the page level by SQLCipher (the bundled feature) keyed by VK.

```
Offset  Size  Field                  Notes
──────  ────  ─────────────────────  ─────────────────────────────────────────
0x0000   4    magic                  "KAFD" (0x4B 0x41 0x46 0x44)
0x0004   2    format_version         u16 = 1
0x0006   2    min_compatible_version u16 = 1
0x0008  16    vault_id               UUIDv7
0x0018  16    kdf_salt               Argon2id salt
0x0028  32    hkdf_salt              HKDF salt
0x0048   4    argon2_t               u32 iterations (3)
0x004C   4    argon2_m_kib           u32 memory KiB (65536)
0x0050   1    argon2_p               u8 parallelism (1)
0x0051   3    _reserved
0x0054  12    vk_wrap_nonce          AES-GCM nonce (password path)
0x0060  48    vk_wrap_ct             32 B wrapped VK + 16 B tag (password)
0x0090  12    vk_recover_nonce       AES-GCM nonce (recovery path)
0x009C  48    vk_recover_ct          32 B wrapped VK + 16 B tag (recovery)
0x00CC  32    srp_verifier_hash      SHA-256 of SRP verifier (server stores v)
0x00EC  16    created_at_ts          ISO8601 (truncated, padded)
0x00FC   4    flags                  bit 0: has_recovery, bit 1: padding_enabled
0x0100   0    --- end of header (256 B) ---
0x0100   N    sqlcipher database     SQLite file, page-encrypted with VK
```

Header itself is integrity-protected: the first AES-GCM operation (unlocking VK) uses the header bytes `[0x0000, 0x0090)` as AAD, so any tampering with format/version/salts is detected.

### 5.3 Per-item encryption flow

For event-sourced sync, the unit of encryption is the **event payload**, not the row. Local storage uses SQLCipher (VK-keyed) for at-rest protection; events carry their own AEAD envelope so they can transit untrusted servers.

```
Encrypt event(entity_kind, entity_id, op, payload_json):
  1.  DK   ← HKDF(salt=VK, info="Tock/v1/item/"||entity_kind, len=32)
  2.  IK   ← HKDF(salt=DK, info="item/"||entity_id, len=32)
  3.  nonce ← random 12 B  (or deterministic from (event_id) for dedup; default random)
  4.  AAD  ← "tock|v1|"||entity_kind||"|"||entity_id||"|"||op||"|"||lamport_be||"|"||device_id
  5.  padded_pt ← pad(payload_json, bucket(len(payload_json)))     // size-bucket padding
  6.  ct, tag ← AES-256-GCM(IK, nonce, padded_pt, AAD)
  7.  emit Event { payload_ct = ct||tag, payload_nonce = nonce, aad = AAD, … }
  8.  zeroize(DK, IK, padded_pt)

Decrypt:
  1.  Recompute DK, IK from (VK, entity_kind, entity_id).
  2.  Verify AAD matches event fields (entity_kind, entity_id, op, lamport, device_id).
  3.  AES-256-GCM-Open → padded_pt.
  4.  Strip padding → payload_json.
```

Size-bucket padding rounds plaintext up to the next power-of-two bucket within `[64, 128, 256, 512, 1024, 2048, 4096, 8192]`; payloads larger than 8 KiB are padded to the next multiple of 4 KiB. Padding byte = `0x00`, with a 2-byte big-endian length prefix at the start of the plaintext so the original length is recoverable.

### 5.4 Recovery key

- **Generation**: 256 bits of CSPRNG entropy encoded as 24 words from a **Crockford Base32 wordlist** (no ambiguous chars; checksum baked in via a 4-bit CRC on the last word).
- **Derivation**: `RK_material ← decode_crockford(words)` → `RK ← HKDF-SHA256(salt=vault_id, ikm=RK_material, info="Tock/v1/recovery", len=32)`.
- **Storage**: never persisted by Tock. The vault header carries `vk_recover_ct = AES-GCM(RK, vault.vk_recover_nonce, VK, aad=header[0..0x90])`.
- **Usage**:
  1. User pastes 24 words → core validates checksum and length.
  2. Derive RK.
  3. AEAD-decrypt `vk_recover_ct` → VK.
  4. Prompt for **new password** → derive new MK and MEK → wrap VK afresh → overwrite `vk_wrap_ct`.
  5. SRP verifier is regenerated and pushed to server (proves possession of new MK).

The recovery flow never reveals the old password. The recovery key is offered at vault creation and can be regenerated (which re-wraps VK with new RK and invalidates the old).

### 5.5 Threat model

**Protect against:**

- **Server compromise / hostile sync host** — server only sees opaque events; can correlate metadata (event count, timing, sizes within buckets, device IDs) but cannot read content. SRP-6a means the server never sees the password or verifier-from-password.
- **At-rest disk theft** — vault file encrypted at SQLCipher page level by VK; VK only reachable via password (Argon2id-hardened) or recovery key.
- **Network MITM** — transport is TLS 1.3 (required); additionally, the SRP-6a session key is used to derive an authenticated channel binding so a TLS-stripping MITM still cannot impersonate.
- **Sync replay / reorder** — events carry monotonic lamport + UUIDv7; storage rejects duplicates; AAD pins payload to `(entity, op, lamport, device)`.
- **Stolen recovery key** — equivalent to stolen password (full access). Documented; users instructed to store offline.
- **Cross-item key reuse** — defeated by per-item HKDF derivation.

**Do not protect against:**

- **Endpoint compromise / malicious OS** — if the device is rooted, plaintext is observable while vault is unlocked.
- **Coercion / rubber-hose** — no plausible-deniability hidden vault in v1.
- **Traffic analysis at scale** — bucketed sizes mitigate per-event leakage but a passive adversary can still infer activity timing.
- **Side channels in third-party crypto crates** — we rely on `aes-gcm`, `argon2`, `x25519-dalek` audits and CI advisory checks.
- **Forward secrecy of historical events** — past events remain decryptable with VK; rotating VK requires full re-encryption (a `tock vault rotate` operation).

### 5.6 SRP-6a authentication

We use SRP-6a (RFC 5054) over a 4096-bit safe-prime group with SHA-256.

**Registration:**

```
Client                                                Server
──────                                                ──────
password, vault_id
salt_srp  ← random 16 B (independent of kdf_salt)
x         ← H( salt_srp || H(username || ":" || password) )
v         ← g^x  mod N
                       ──── (username, salt_srp, v) ───►
                                                       store (username, salt_srp, v)
                                                       Server NEVER sees password.
```

**Login (mutual auth, derives shared K):**

```
Client                                                  Server
──────                                                  ──────
a ← random
A ← g^a mod N
                          ──── (username, A) ───►
                                                       lookup salt_srp, v
                                                       b ← random
                                                       B ← (k*v + g^b) mod N    [k = H(N, g)]
                          ◄──── (salt_srp, B) ─────
u   ← H(A, B)
x   ← H( salt_srp || H(username || ":" || password) )
S_c ← (B − k*g^x)^(a + u*x)  mod N
K_c ← H(S_c)
M1  ← H( H(N) XOR H(g) || H(username) || salt_srp || A || B || K_c )
                          ──── M1 ───►
                                                       S_s ← (A * v^u)^b  mod N
                                                       K_s ← H(S_s)
                                                       verify M1 with K_s
                                                       M2 ← H( A || M1 || K_s )
                          ◄──── M2 ─────
verify M2
session_key ← K_c (== K_s)
```

The resulting `K` (256 bits) is used as the input keying material for an HKDF that produces:
- a **bearer token** sent with each subsequent HTTPS request (short-lived, signed by HMAC-K),
- a **channel-binding tag** included in event AAD for the session (defense-in-depth against TLS strip).

The server stores only `(username, salt_srp, v)` — never the password, never enough to derive MK or VK.

---

## 6. Sync Protocol Design

Tock syncs by replicating an **append-only event log** between devices through one or more servers (or peer-to-peer via the same transport trait). Conflict detection uses vector clocks; resolution uses LWW at field granularity for state, append-only for logs (annotations, habit entries, time blocks, focus interruptions).

### 6.1 Event schema

```rust
// crates/tock-sync/src/event.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: Uuid,                       // UUIDv7 (time-ordered)
    pub device_id: DeviceId,            // 16 B random, per device
    pub lamport: u64,                   // monotonic per device
    pub vector_clock: VectorClock,      // {device_id -> lamport seen}
    pub parent_event_id: Option<Uuid>,  // last local event id (chain integrity)
    pub entity_kind: EntityKind,        // Task | Habit | HabitEntry | TimeBlock | ...
    pub entity_id: Uuid,
    pub op: EventOp,                    // see below
    pub payload: EncryptedPayload,      // ciphertext + nonce + AAD (see §5.3)
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventOp {
    Create,
    Update { fields: Vec<FieldName> },  // names only; values inside payload
    Delete,                              // soft delete sets deleted_at
    Purge,                               // hard delete (tombstone-collected later)
    Append { sub_kind: AppendKind },     // annotations, habit entries, interruptions
    Snapshot,                            // produced by compaction
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AppendKind { Annotation, HabitEntry, TimeBlockSegment, FocusInterruption }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorClock(pub BTreeMap<DeviceId, u64>);

impl VectorClock {
    pub fn happens_before(&self, other: &Self) -> bool { /* ∀ a ≤ b and ∃ a < b */ }
    pub fn concurrent_with(&self, other: &Self) -> bool {
        !self.happens_before(other) && !other.happens_before(self) && self != other
    }
    pub fn merge(&mut self, other: &Self) { /* per-device max */ }
}
```

The plaintext **payload** for an `Update` carries only the changed fields plus their new values, as a JSON object keyed by field name — this minimizes bandwidth and gives a natural field-level merge granularity.

### 6.2 Conflict detection algorithm

For every incoming event `e` on entity `E`:

```
1. Look up entity E's current state and its "head events" — the set of last applied
   events such that no later applied event on E supersedes them.

2. For each head event h on E:
     a. If e.vector_clock dominates h.vector_clock → e supersedes h.
        Apply e; remove h from heads; add e to heads.
     b. If h.vector_clock dominates e.vector_clock → e is stale.
        Discard (already known transitively); record a no-op ack.
     c. If concurrent (neither dominates) → CONFLICT (see §6.3).

3. Update vector clock: device_clock.merge(e.vector_clock); local lamport ← max(local, e.lamport) + 1.

4. Persist e in `events` table; update materialized entity row.
```

### 6.3 Conflict resolution rules

Resolution is per-`EventOp` and per-`EntityKind`:

| Case                                                     | Rule                                                                           |
|----------------------------------------------------------|--------------------------------------------------------------------------------|
| Two concurrent `Update`s on disjoint field sets          | **Field-level merge** — apply both. No user prompt.                            |
| Two concurrent `Update`s on the same field               | **LWW by (lamport DESC, device_id ASC)** with a tie-broken deterministic order. Loser's value is preserved in `events` for forensic recovery. |
| Concurrent `Update` and `Delete`                         | **Delete wins** by default; the Update is preserved in history. Configurable: `sync.conflict.delete_vs_update = "delete" \| "update" \| "prompt"`. |
| Concurrent `Append`s                                     | **Both apply** (append-only collections are commutative).                      |
| Concurrent `Delete`s                                     | Idempotent — both apply (no-op the second).                                    |
| Concurrent `Create` with same `entity_id`                | Impossible by construction (UUIDv7 collisions are negligible); if it occurs, deterministic tie-break by `device_id`, the loser becomes an Update.  |
| `time_blocks` open-overlap across devices                | Both apply; mark both with virtual tag `+SYNC_OVERLAP` (see §2.3.5).           |
| `focus_sessions` two devices started concurrently        | The one with earlier `started_at` wins; the other is downgraded to `Aborted` with `note: "superseded by sync"`. |

A **conflict log** (queryable via `tock sync conflicts`) surfaces LWW-losers for user review without blocking sync.

### 6.4 Snapshot / compaction

- Every **1000 events globally** or **30 days**, whichever comes first, a snapshot is produced:
  1. Materialize the full entity set into a CBOR document.
  2. Encrypt with `IK ← HKDF(VK, info="Tock/v1/snapshot/" || snapshot_uuid)`; same envelope as events.
  3. Store as a `snapshots` row referencing the highest included `event.id`.
  4. After snapshot is replicated to ≥1 peer, the originating events older than the snapshot can be **tombstoned** (kept as `(id, lamport, device_id)` triples for causality but payload set to NULL).
- New devices onboarding fetch the latest snapshot first, then events after it. This bounds startup time at O(snapshots + recent events).
- Snapshots are **never authoritative for conflict detection** — they are a cache. The vector-clock state lives in the (possibly tombstoned) event metadata.

### 6.5 Device onboarding flow

```
Existing device E                    New device N                     Server S
─────────────────                    ────────────                     ──────────

1. User: `tock device pair`
   E generates ephemeral X25519 (es, ep_E)
   E displays QR encoding:
     { vault_id, server_url, ep_E, fingerprint=SHA256(ep_E)[:8] }

                                     2. User scans QR (or pastes text)
                                        N parses fields
                                        N generates ephemeral X25519 (ns, ep_N)
                                        N displays fingerprint of ep_N

3. User confirms ep_N fingerprint on E
   (out-of-band trust establishment)

                                                                  4. N --(register pubkey)--> S
                                                                     S returns device_id_N

5. E computes shared_secret = X25519(es, ep_N)
   E derives wrap_key = HKDF(shared_secret, info="Tock/v1/onboard")
   E AEAD-encrypts VK with wrap_key → onboarding_blob

6. E --(POST onboarding_blob, target=device_id_N)--> S

                                     7. N --(GET onboarding_blob)--> S
                                        N computes shared_secret = X25519(ns, ep_E)
                                        N derives wrap_key (same)
                                        N decrypts → VK

8. N prompts user for *new local* password (independent of E's):
   - derive new MK, MEK via Argon2id over the new password
   - re-wrap VK locally on N
   - store header + open empty SQLCipher DB keyed by VK

9. N pulls latest snapshot + events from S, decrypts with VK, materializes state.

10. N publishes a `Create(Device)` event so other devices learn of N.
```

Notes:
- The server never sees VK or wrap_key — only opaque blobs and X25519 public keys.
- Out-of-band confirmation of the fingerprint thwarts QR-MITM (a hostile server cannot substitute its own pubkey without the fingerprint mismatching).
- A pairing attempt without confirmation expires in **5 minutes**.
- Recovery-key onboarding is identical except step 1 is skipped — N derives VK directly from the recovery key, then registers with S.

### 6.6 Transport abstraction

```rust
// crates/tock-sync/src/transport.rs

#[async_trait]
pub trait Transport: Send + Sync {
    async fn push(&self, events: &[Event]) -> Result<PushAck>;

    async fn pull(
        &self,
        cursor: &SyncCursor,    // vector clock snapshot
        limit: usize,
    ) -> Result<PullBatch>;

    async fn fetch_snapshot(&self, id: SnapshotId) -> Result<EncryptedSnapshot>;

    async fn put_onboarding_blob(
        &self,
        target_device: DeviceId,
        blob: OnboardingBlob,
    ) -> Result<()>;

    async fn get_onboarding_blob(
        &self,
        device: DeviceId,
    ) -> Result<Option<OnboardingBlob>>;

    /// Long-poll or websocket subscription.
    async fn subscribe(&self) -> BoxStream<'static, Result<Event>>;
}

pub struct PushAck { pub accepted: usize, pub duplicates: usize, pub server_lamport: u64 }
pub struct PullBatch { pub events: Vec<Event>, pub next_cursor: SyncCursor, pub more: bool }
```

Implementations:
- `HttpTransport` (default, in `tock-cli`) — REST over HTTPS, long-poll fallback if WebSocket unavailable.
- `WebsocketTransport` — duplex, used by Apple apps and TUI live mode.
- `LocalLanTransport` — mDNS-discovered peer, X25519-authenticated, useful for offline LAN sync.
- `FileSyncTransport` — drop events to a shared folder (Syncthing / iCloud Drive); enables zero-server operation.

Every implementation lives outside `tock-core` and is injected into `tock-sync` via the trait.

### 6.7 Bandwidth and latency budgets

Working assumptions, derived from a power user generating ~200 events/day (tasks, time blocks, habit entries):

| Metric                                          | Budget                   | Notes                                        |
|-------------------------------------------------|--------------------------|----------------------------------------------|
| Average event size (post-pad, ciphertext)        | **≤ 512 B**              | 99th percentile in `2048` bucket             |
| Steady-state daily traffic per device            | **~100 KiB up + 100 KiB down** | trivially fits cellular                |
| Cold onboarding (3 yrs of history via snapshot) | **≤ 5 MiB**              | snapshot dominates                           |
| Snapshot frequency                              | every 1000 events / 30d  | bounds cold start                            |
| Sync latency (push → other device receives)     | **p50 ≤ 1.5 s, p95 ≤ 5 s** | websocket + long-poll fallback             |
| Push round-trip                                  | **p95 ≤ 400 ms** on 4G   | batches up to 64 events                      |
| Server storage per active user                  | **~50 MiB/year** worst case | dominated by tombstones; compaction reclaims |
| Maximum event batch                             | **256 events / 256 KiB** | whichever first                              |

Backpressure: if push fails, events queue locally indefinitely; `tock sync status` surfaces backlog count and oldest unsynced timestamp. There is no in-protocol size cap on the local queue — the only limit is local disk.


## 7. CLI Design

### 7.1 Complete Command Tree

The CLI follows a verb-first layout with namespaced subcommands: a small number of top-level verbs for the common path (`add`, `list`, `done`, `start`, `stop`), plus namespaced subcommands for less-frequent surfaces (`habit ...`, `vault ...`, `sync ...`).

The design rule: **the 10 most common operations are single-token; everything else is namespaced.**

```
tock
├── (default)                              # `tock` with no args = `list today`
│
├── ── Task surface ────────────────────────────────────────────────
├── add <natural...>                       # Create a task
│   ├── --project, -p <name|sid>
│   ├── --area, -a <name|sid>
│   ├── --heading, -H <name>
│   ├── --tag, -t <tag>... (repeatable)
│   ├── --due <when>                       # Deadline
│   ├── --start, --when <when>             # Start/defer date
│   ├── --evening                          # Goes to Today/Evening
│   ├── --priority, --pri <L|M|H|!>
│   ├── --estimate, -e <duration>
│   ├── --checklist <item>...              # Repeatable
│   ├── --recur <rrule|natural>
│   ├── --depends <sid>...
│   ├── --note <text>
│   └── --uda <key>=<value>...
│
├── modify, mod <filter> <changes...>      # `mod 4f2 due:friday +urgent`
├── done, complete <filter>                # Mark complete (logs time if timer running)
├── cancel <filter>                        # Mark cancelled (kept in logbook)
├── delete, rm <filter>                    # Soft delete (purgeable)
├── undo                                   # Reverse last mutation (event-sourced)
├── redo
├── list, ls [view] [filter]               # Views: today, evening, upcoming,
│                                          #        anytime, someday, inbox, logbook,
│                                          #        all, next
│   ├── --sort <field,field...>
│   ├── --group <project|area|tag|date>
│   ├── --limit, -n <N>
│   ├── --report <name>                    # Use a custom report from config
│   └── --json                             # Machine-readable output
├── next                                   # Highest-urgency actionable items
├── show, info <sid>                       # Full task detail
├── annotate <filter> <text>               # Add timestamped annotation
├── denotate <filter> <index>
├── move <filter> --to <project|area|heading>
├── tag <filter> +foo -bar                 # Add/remove tags
├── inbox                                  # Triage flow (TUI)
├── today [--add <filter>]                 # Promote to Today
├── evening <filter>                       # Move to Today/Evening
├── defer <filter> <when>                  # Set start date
│
├── ── Project / Area surface ──────────────────────────────────────
├── project
│   ├── add <name> [--area <name>] [--deadline <when>] [--note <text>]
│   ├── ls [--area <name>] [--status active|paused|done]
│   ├── show <sid>
│   ├── archive <sid>
│   ├── heading add <project> <name>
│   └── heading rm <project> <name>
├── area
│   ├── add <name>
│   ├── ls
│   └── archive <sid>
│
├── ── Habit surface ─────────────────────────────────────────────────
├── habit
│   ├── add <natural...>                   # Guided unless --no-wizard
│   │   ├── --identity <text>              # "I am a person who..."
│   │   ├── --cue <text>                   # Implementation intention
│   │   ├── --cadence <daily|weekly:Mo,We,Fr|N-per-week>
│   │   ├── --min <value>                  # "start small" floor
│   │   ├── --target <value>               # eventual goal
│   │   ├── --unit <reps|minutes|pages|ml|...>
│   │   ├── --stack-after <habit-sid>      # habit stacking
│   │   ├── --break                        # this is a break-bad-habit
│   │   ├── --reminder <when|cron>...
│   │   └── --accountable <contact|webhook>
│   ├── log <habit> [value] [--at <when>] [--note <text>]
│   ├── skip <habit> [--reason <text>]
│   ├── undo <habit>                       # Undo last log
│   ├── ls [--today] [--due] [--stack <root>]
│   ├── show <sid>
│   ├── streak [<sid>]                     # Current + longest streaks
│   ├── stack <root-sid>                   # Show the chain
│   ├── level [<sid>]                      # Show progression (XP/levels)
│   ├── archive <sid>
│   └── identity ls|add|rm                 # Manage identity statements
│
├── ── Time tracking ────────────────────────────────────────────────
├── start [natural...]                     # Start a timer; can create-on-fly
│   ├── --task, -T <sid>                   # Bind to existing task
│   ├── --project, -p <name>
│   ├── --tag, -t <tag>...
│   ├── --at <when>                        # Backdate start
│   └── --note <text>
├── stop [--at <when>] [--note <text>]
├── resume [<block-sid>]                   # Restart last (or specified) block
├── current, status                        # What's running now?
├── blocks [filter]                        # List time blocks
│   ├── --since <when> --until <when>
│   ├── --project, --task, --tag
│   └── --format table|json|csv
├── report [name] [filter]                 # Built-in: today, week, month, project
│   ├── --by <project|task|tag|day|week>
│   ├── --billable
│   └── --format table|json|csv|md
├── edit <block-sid> <changes...>          # Adjust a block post-hoc
│
├── ── Focus (Pomodoro) ────────────────────────────────────────────
├── focus
│   ├── start [<task-sid>] [--length 25m] [--break 5m] [--long 15m] [--cycles 4]
│   ├── stop                               # Abort current pomo
│   ├── pause / resume
│   ├── skip-break
│   ├── status
│   └── stats [--since <when>] [--by task|project|day]
│
├── ── Vault & crypto ──────────────────────────────────────────────
├── vault
│   ├── init [--path <dir>] [--passphrase-stdin]
│   ├── unlock [--ttl <duration>]          # Cache key in keyring/agent
│   ├── lock
│   ├── status                             # Locked? device id? sync peer count?
│   ├── rotate-key                         # Generate new content key, re-wrap
│   ├── change-passphrase
│   ├── recover --shares <file>...         # Reconstruct via Shamir shares
│   └── export-recovery --threshold 3 --shares 5 --out <dir>
│
├── ── Sync & devices ──────────────────────────────────────────────
├── sync [--once] [--dry-run] [--verbose]
├── devices
│   ├── ls
│   ├── revoke <device-id>
│   └── rename <device-id> <name>
├── onboard
│   ├── invite [--ttl 10m]                 # Print pairing code/QR on this device
│   └── accept <code>                      # On new device
├── server
│   ├── login <url>                        # For hosted service
│   ├── logout
│   └── status
│
├── ── Config, context, reports ────────────────────────────────────
├── context
│   ├── ls
│   ├── set <name>                         # Activate
│   ├── clear
│   ├── define <name> <filter>             # `define work +work -personal`
│   └── rm <name>
├── config
│   ├── get <key>
│   ├── set <key> <value>
│   ├── unset <key>
│   ├── edit                               # Open $EDITOR on config.toml
│   └── path                               # Print config path
├── report
│   ├── ls
│   ├── show <name>
│   ├── define <name>                      # Wizard
│   └── rm <name>
├── uda
│   ├── add <key> --type string|int|duration|date|enum --values <a,b,c>
│   ├── ls
│   └── rm <key>
├── hooks
│   ├── ls
│   ├── enable <name> / disable <name>
│   └── test <name> --event on-add --input <file.json>
│
├── ── Import / export ─────────────────────────────────────────────
├── import
│   ├── taskwarrior [--from <export.json>]
│   ├── things3    [--from <things3.json>]
│   ├── csv        --from <file.csv> --map <map.toml>
│   ├── json       --from <file.json>
│   └── caldav     --url <url> --user <u> [--password-stdin]
├── export
│   ├── json   [filter] [--out <file>]
│   ├── csv    [filter] [--columns ...]
│   ├── md     [filter] [--template <file>]
│   └── caldav [filter] [--url <url>]
│
├── ── Meta ────────────────────────────────────────────────────────
├── tui                                    # Launch full TUI
├── completions <bash|zsh|fish|nu|pwsh>
├── version
└── help [command]
```

#### Default filter language

A `<filter>` argument accepts:

- A **SID** (e.g. `4f2`, the first unambiguous prefix of a ULID).
- A **range**: `4f2-9aa`, `4f2,9aa,c1d`.
- **Tag expressions**: `+work -archived`.
- **Field comparisons**: `project:home`, `due.before:friday`, `urgency.over:8`.
- **Virtual tags**: `+TODAY`, `+OVERDUE`, `+BLOCKED`, `+ACTIVE` (timer), `+TAGGED`.
- **Dot-paths into UDAs**: `uda.effort:>3`.
- A bare prefix string: `groceries` → fuzzy description match.

### 7.2 Natural Language Parsing Strategy

The CLI accepts **two equivalent forms** for every mutation command:

```
tock add Buy milk tomorrow at 5pm @errands #shopping !
tock add "Buy milk" --due "tomorrow 5pm" --tag errands --project shopping --priority H
```

Both produce identical events. The parser tries the natural form first; tokens consumed by it become structured, and remaining tokens fall through to the description.

#### Grammar (EBNF-ish, restricted to the natural surface)

```ebnf
input        = (token ws)* token? ;
token        = sigil_tag | sigil_proj | sigil_area | priority_marker
             | temporal_phrase | duration_phrase | checklist_marker | word ;

sigil_tag    = "#" ident | "+" ident ;
sigil_proj   = "@@" ident_path                     (* @@home.garden *)
             | "::" ident_path ;
sigil_area   = "@" ident                           (* @work *)
priority_marker = "!" | "!!" | "!!!" ;             (* L / M / H *)

temporal_phrase = ("due"|"by"|"on"|"at"|"when"|"start"|"defer") ws nl_date
                | nl_date ;                         (* unanchored: applies to due *)
duration_phrase = "for" ws duration | "~" duration ;

nl_date = relative_date | absolute_date | weekday_date | named_date ;
relative_date = "today" | "tonight" | "tomorrow" | "yesterday"
              | "in" int unit | int unit "ago"
              | "next" (weekday | unit) | "this" (weekday | unit) ;
absolute_date = iso_date | mdy | dmy ;             (* locale-aware *)
weekday_date  = weekday [ws ("at"|"@") ws time] ;
named_date    = "eod" | "eow" | "eom" | "someday" | "anytime" | "inbox" | "evening" ;
duration      = int ("m"|"min"|"minutes"|"h"|"hr"|"hours"|"d"|"days"|"w"|"weeks") ;
```

#### Resolution rules (deterministic, in this order)

1. **Anchoring wins.** `due:friday` always beats a bare `friday` later in the line.
2. **Sigils win over words.** `#urgent` is always a tag, never a description token; backslash-escape (`\#urgent`) to put it in the description.
3. **First date is due, second is start.** `Buy milk friday from monday` → due=Fri, start=Mon. Reversed if the second is sigil-anchored.
4. **Times without dates** default to *today if still in the future*, else *tomorrow*. `at 5pm` at 3pm today → today 17:00; at 7pm → tomorrow 17:00.
5. **Weekdays** are *next occurrence*, never today even if today matches, unless explicitly `this monday`.
6. **Locale**: date order (m/d vs d/m) is read from `config.locale.date_order`; default is ISO if ambiguous.
7. **Timezone**: stored UTC; rendered in `config.locale.timezone` (default: system).
8. **Plural collisions**: `weeks` is duration; `week` after `next`/`this` is a date.

#### Ambiguity resolution

When a phrase has multiple valid parses, the CLI:

1. Picks the highest-scoring parse (score = anchored tokens + specificity).
2. Echoes the structured result on stdout:
   ```
   ✓ Added 4f2: "Buy milk"
     due: Fri 2025-03-14 17:00   project: shopping   tag: errands   priority: H
   ```
3. If two parses tie, asks (TTY only) or prefers the *less surprising* one and prints a hint:
   ```
   ✓ Added 4f2: "Call Pat about 2pm meeting"
     ⓘ "2pm" treated as description (anchored "tomorrow" used as due).
       To override: `tock mod 4f2 due:"tomorrow 2pm"`
   ```
4. Non-TTY (scripts, hooks): always picks highest-scoring parse silently; structured result available via `--json`.

#### Examples (natural ↔ flag equivalence)

| Natural                                                            | Flag form                                                                                  |
|--------------------------------------------------------------------|--------------------------------------------------------------------------------------------|
| `tock add Pay rent on the 1st !! @finance`                        | `tock add "Pay rent" --due 2025-04-01 --priority M --area finance`                        |
| `tock add Email Sam #followup due friday for 15m`                 | `tock add "Email Sam" -t followup --due fri --estimate 15m`                               |
| `tock mod 4f2 tomorrow !`                                         | `tock mod 4f2 --due tomorrow --priority L`                                                |
| `tock start writing chapter 3 @@book.draft`                       | `tock start --project book.draft --note "writing chapter 3"`                              |
| `tock habit add Meditate 10m every morning at 7 stack-after coffee` | `tock habit add Meditate --target 10 --unit minutes --cadence "daily 07:00" --stack-after coffee` |
| `tock focus start 4f2 25/5x4`                                     | `tock focus start 4f2 --length 25m --break 5m --cycles 4`                                 |

#### Errors and suggestions

Parse failures never abort silently. The CLI returns exit 2 and prints a Rust-style caret diagnostic:

```
error: could not parse temporal expression
  ┌─ stdin
  │
  │ tock add Call Sam next blursday at 5
  │                ^^^^^^^^^^^^^^ here
  │
  = note: "blursday" is not a recognized day name
  = help: did you mean "thursday"?  try: --due "next thursday 5pm"
```

Powered by a small Levenshtein dictionary over the keyword set (weekdays, named dates, units, sigils).

### 7.3 TUI Layout and Navigation

The TUI is a thin layer over the same core library used by the CLI: every action emits the same event the CLI would. The TUI is **panel-based with vim-style keys**, not a generic file-manager dual-pane. Layout adapts to terminal size; below is the ≥120×30 layout.

```
┌─ tock ──────────────────── vault: unlocked  · sync: ✓ 12s ago · ctx: work ─┐
│ ┌─ Views ────────┐ ┌─ Today  ────────────────────────────────┐ ┌─ Detail ─┐ │
│ │ ▸ Inbox    (3) │ │ ◉ 09:00  Write standup notes      ~10m │ │ Write    │ │
│ │ ● Today    (7) │ │ ◉        Review PR #482        @@work  │ │ standup  │ │
│ │ ▸ Evening  (2) │ │ ◉ 11:30  Dentist               !!      │ │ notes    │ │
│ │ ▸ Upcoming(14) │ │ ─── Evening ─────────────────────────── │ │          │ │
│ │ ▸ Anytime (31) │ │ ◯        Read chapter 4                │ │ project: │ │
│ │ ▸ Someday  (9) │ │ ◯        Plan week                     │ │   work   │ │
│ │ ▸ Logbook      │ │                                         │ │ due:     │ │
│ │ ── Projects ── │ │ ─── Habits ────────────────────────────│ │   today  │ │
│ │ ▸ Book draft   │ │ ✓ Meditate     10m  🔥 23              │ │   09:00  │ │
│ │ ▸ Home reno    │ │ ◯ Read         20p  🔥 11              │ │ urgency: │ │
│ │ ▸ Side biz     │ │ ◯ Workout      30m  🔥 4               │ │   8.7    │ │
│ │ ── Areas ───── │ │                                         │ │ tags:    │ │
│ │ ▸ @work        │ └─────────────────────────────────────────┘ │  +daily  │ │
│ │ ▸ @home        │ ┌─ Timer ────────┐ ┌─ Focus ──────────────┐ │          │ │
│ │ ▸ @health      │ │ ▶ 00:42:17     │ │ 🍅 12:34 / 25:00     │ │ depends: │ │
│ │ ── Contexts ── │ │ Review PR #482 │ │ pomo 2 / 4           │ │   none   │ │
│ │ ● work         │ └────────────────┘ └──────────────────────┘ │          │ │
│ └────────────────┘                                              └──────────┘ │
│ : (cmd)                                              ? help  q quit  ⌘K cmd │
└──────────────────────────────────────────────────────────────────────────────┘
```

#### Navigation model

- **Panels**: Sidebar (views), List (current view), Detail (selected task). Tab/`Shift-Tab` rotate focus.
- **Vim keys** within panels: `h j k l`, `gg`/`G`, `/` search, `n`/`N` next match, `:` command-line (full CLI subset).
- **Cross-cutting actions** are uppercase mnemonics: `A` add, `D` delete, `E` edit, `T` start timer, `F` focus, `H` habit log, `S` sync.
- **Hjkl across panels** when at edge: `l` from list → detail; `h` from list → sidebar.

#### Key bindings (default; overridable in `config.toml`)

| Key      | Context  | Action                                       |
|----------|----------|----------------------------------------------|
| `j`/`k`  | any list | next / previous item                         |
| `gg`/`G` | any list | top / bottom                                 |
| `Enter`  | list     | open in detail panel                         |
| `Space`  | task     | toggle complete                              |
| `c`      | task     | cancel                                       |
| `d`      | task     | delete (confirm)                             |
| `e`      | task     | edit fields inline                           |
| `t`      | task     | start/stop timer on this task                |
| `f`      | task     | start focus session on this task             |
| `a`      | any      | quick-add (modal, accepts natural language)  |
| `A`      | any      | quick-add then jump to detail                |
| `H`      | habit    | log habit (prompts for value if needed)      |
| `u`      | any      | undo last action                             |
| `U`      | any      | redo                                         |
| `/`      | list     | search/filter (live)                         |
| `:`      | any      | command palette (CLI subset)                 |
| `g t`    | any      | go to Today                                  |
| `g i`    | any      | go to Inbox                                  |
| `g p`    | any      | go to Projects                               |
| `g h`    | any      | go to Habits                                 |
| `Tab`    | any      | next panel                                   |
| `?`      | any      | help overlay                                 |
| `q`      | any      | quit                                         |

#### CLI ↔ TUI mapping

Every TUI keystroke is implemented as `core::dispatch(Command)` against the same command enum as the CLI parser. `:` opens a command line that accepts the *exact* CLI grammar minus the leading `tock`:

```
: add Buy milk tomorrow #shopping
: mod 4f2 due:friday
: report week --by project
```

This guarantees feature parity and means automation scripts and TUI muscle memory transfer.

### 7.4 Configuration

**Format**: TOML.
**Location**:
- Linux: `$XDG_CONFIG_HOME/tock/config.toml` (default `~/.config/tock/config.toml`).
- macOS: `~/Library/Application Support/tock/config.toml` (but `~/.config/tock/config.toml` honored if present, for parity with dotfiles).
- Windows: `%APPDATA%\tock\config.toml`.

Vault data is **separate** (`vault.path` in config, default `~/.tock/vault/` on Unix-like systems).

#### Full config schema with defaults

```toml
# ─── General ──────────────────────────────────────────────────────────────
[general]
default_view       = "today"          # what `tock` with no args shows
confirm_destructive = true            # ask before delete/purge
editor             = ""               # falls back to $EDITOR, then $VISUAL
pager              = ""               # falls back to $PAGER, then "less -FRX"
color              = "auto"           # auto|always|never
unicode            = "auto"           # auto|always|never (sigils/icons)

[locale]
timezone    = "system"
date_order  = "auto"                  # auto|ymd|dmy|mdy
week_starts = "monday"                # monday|sunday|saturday
time_format = "24h"                   # 24h|12h
language    = "en"                    # for natural language parser

# ─── Vault & crypto ───────────────────────────────────────────────────────
[vault]
path             = "~/.tock/vault"
unlock_ttl       = "1h"
keyring          = "auto"             # auto|secret-service|keychain|wincred|none
passphrase_kdf   = "argon2id"
argon2_memory_mb = 256
argon2_iters     = 3
argon2_parallel  = 1

# ─── Sync ─────────────────────────────────────────────────────────────────
[sync]
enabled        = false
mode           = "self-hosted"        # self-hosted|hosted|none
server_url     = ""                   # e.g. https://sync.example.com
auto_interval  = "5m"
on_mutation    = true                 # push immediately after local change
conflict_policy = "last-writer-wins"  # or "manual"; per-field overrides below

# ─── Tasks ────────────────────────────────────────────────────────────────
[tasks]
sid_min_length     = 3                # short-id prefix length minimum
sid_alphabet       = "crockford"      # crockford|hex|words
default_priority   = "none"
auto_today_at      = "04:00"          # local time when "today" rolls over
evening_starts_at  = "18:00"
inbox_age_warn     = "7d"             # nag if inbox items older than this

# ─── Urgency coefficients ────────────────────────────────────────────────
[urgency]
next        = 15.0
due         = 12.0
blocking    = 8.0
priority_H  = 6.0
priority_M  = 3.9
priority_L  = 1.8
active      = 4.0                     # timer running on this task
age         = 2.0                     # per coefficient-year
annotations = 1.0                     # per annotation
tags        = 1.0                     # per tag
project     = 1.0                     # any project assignment
scheduled   = 5.0
blocked     = -5.0
[urgency.tags]                        # per-tag overrides
urgent  = 4.0
someday = -3.0
waiting = -2.0
[urgency.uda]                         # per-UDA scaling
effort = 0.5

# ─── Habits ───────────────────────────────────────────────────────────────
[habits]
streak_grace_days = 1                 # 1 missed day doesn't break streak
level_curve       = "fibonacci"       # linear|fibonacci
reminder_channel  = "notify"          # notify|email|webhook|none
identity_required = false             # prompt for identity on add?

# ─── Time tracking ────────────────────────────────────────────────────────
[time]
round_to          = "1m"              # round block durations
allow_overlap     = false
idle_detection    = true              # nudge if no input for >5m while timer runs
idle_threshold    = "5m"
auto_stop_at      = "23:59"           # safety cutoff
billable_default  = false

# ─── Focus / Pomodoro ─────────────────────────────────────────────────────
[focus]
length         = "25m"
short_break    = "5m"
long_break     = "15m"
cycles         = 4
auto_start_break = true
auto_start_pomo  = false
notify_sound     = "default"
dnd_integration  = true               # toggle macOS Focus / Linux DnD

# ─── Contexts ─────────────────────────────────────────────────────────────
[contexts]
active = "work"                       # currently active context (or "")

[contexts.work]
filter = "+work -archived"
[contexts.personal]
filter = "-work -client"
[contexts.deep]
filter = "+focus urgency.over:10"

# ─── Custom reports ───────────────────────────────────────────────────────
[reports.standup]
description = "Yesterday + today"
filter      = "completed.after:yesterday or (+TODAY)"
columns     = ["sid", "description", "project", "completed_at"]
sort        = ["completed_at-", "urgency-"]
group_by    = "project"

[reports.billing]
description = "Billable hours this week"
filter      = "+billable started.this_week"
columns     = ["task", "project", "duration", "rate", "total"]
format      = "csv"

# ─── User-defined attributes ──────────────────────────────────────────────
[uda.effort]
type   = "int"
label  = "Effort (1-5)"
range  = [1, 5]

[uda.client]
type   = "enum"
label  = "Client"
values = ["acme", "globex", "initech"]

[uda.energy]
type   = "enum"
values = ["low", "medium", "high"]

# ─── Hooks ────────────────────────────────────────────────────────────────
[hooks]
dir     = "~/.config/tock/hooks"
enabled = ["on-add-tagger.sh", "on-complete-zapier.py"]
timeout = "5s"
```

### 7.5 Hook Scripts API

Hooks are executable scripts in `hooks.dir`. Naming convention:

```
hooks/
  on-add-<name>          # before task creation (can mutate or veto)
  after-add-<name>       # after task created
  on-modify-<name>
  on-complete-<name>
  on-delete-<name>
  on-habit-log-<name>
  on-timer-start-<name>
  on-timer-stop-<name>
  on-focus-start-<name>
  on-focus-end-<name>
  on-sync-<name>
  on-launch-<name>       # on `tock` startup
  on-exit-<name>
```

Multiple hooks per event are run alphabetically. `on-*` are *blocking* and can mutate or veto; `after-*` are *fire-and-forget*.

#### Protocol

- **Input**: JSON on stdin. For `on-modify`, both `before` and `after` objects.
- **Output**: JSON on stdout (mutated object, or empty for no change).
- **Errors**: messages on stderr (always logged).
- **Exit codes**:
  - `0` = success (use stdout if non-empty, else no change).
  - `1` = success but with warning (printed to user).
  - `2` = veto (abort the operation, print stderr to user).
  - anything else = treat as failure; user sees a warning; operation proceeds only for `after-*` hooks.
- **Timeout**: `hooks.timeout` (default 5s). Killed and logged on overrun.
- **Environment**: `TOCK_EVENT`, `TOCK_VAULT`, `TOCK_DEVICE_ID`, plus a freshly-minted scoped read-only API token for `tock --hook-token`.

#### Example: auto-tag based on project

```bash
#!/usr/bin/env bash
# hooks/on-add-tagger.sh
set -euo pipefail
jq '
  if (.project // "") | startswith("client.") then
    .tags = ((.tags // []) + ["billable"]) | .tags |= unique
  else . end
'
```

#### Example: Slack notification on completion (Python)

```python
#!/usr/bin/env python3
# hooks/after-complete-slack.py
import json, os, sys, urllib.request
task = json.load(sys.stdin)
if "work" not in (task.get("tags") or []):
    sys.exit(0)
webhook = os.environ["SLACK_WEBHOOK"]
payload = {"text": f"✓ {task['description']} ({task.get('project','—')})"}
urllib.request.urlopen(webhook, json.dumps(payload).encode())
```

#### Example: veto adding tasks without a project on weekdays

```python
#!/usr/bin/env python3
# hooks/on-add-require-project.py
import json, sys, datetime
task = json.load(sys.stdin)
if datetime.datetime.now().weekday() < 5 and not task.get("project"):
    print("Weekday tasks must be in a project. Pass --project or add it via natural lang.", file=sys.stderr)
    sys.exit(2)
print(json.dumps(task))
```

---

## 8. Apple Platform Design

### 8.1 iOS App Architecture

The iOS app is a **thin SwiftUI shell over the Rust core via UniFFI**. No business logic in Swift; the Swift layer is presentation, navigation, system integration, and bridging to platform APIs.

```
apps/ios/
  App/
    TockApp.swift              # @main, sets up CoreActor
    AppDelegate.swift             # background tasks, notification routing
  Core/
    CoreActor.swift               # global actor wrapping UniFFI handle
    Models/                       # @Observable Swift mirrors of core types
    Subscriptions.swift           # AsyncStream → SwiftUI bridge
  Features/
    Today/
    Inbox/
    Projects/
    Habits/
    Timer/
    Focus/
    Settings/
  Widgets/                        # WidgetKit targets
  ShareExtension/
  Intents/                        # App Intents
  Watch/ -> ../watchOS/
```

#### Data flow

```
   ┌─────────────────────────────┐
   │      SwiftUI Views          │
   │  @Observable view models    │
   └───────────────┬─────────────┘
                   │ async calls
                   ▼
   ┌─────────────────────────────┐
   │   CoreActor (global actor)  │   ← serializes all core calls,
   │   - holds Core handle       │     owns the unlocked vault key
   │   - AsyncStream<Event>      │
   └───────────────┬─────────────┘
                   │ UniFFI FFI
                   ▼
   ┌─────────────────────────────┐
   │      tock-core (Rust)      │   ← pure, no I/O outside the
   │  SQLite vault on disk       │     storage trait it owns
   └─────────────────────────────┘
```

A single `@globalActor CoreActor` owns the UniFFI handle. All mutations go through it; reads are also serialized but cached in `@Observable` view-model state so SwiftUI re-renders are immediate after the actor confirms. The core emits an `EventStream` (UniFFI callback interface → Swift `AsyncStream<CoreEvent>`); the view-model layer subscribes and invalidates derived state, so multiple windows (iPad split view, widgets-on-keyboard) stay coherent.

```swift
@globalActor actor CoreActor {
    static let shared = CoreActor()
    private let core: Core  // UniFFI handle

    func add(_ command: AddCommand) async throws -> Task { try core.add(command) }
    func subscribe() -> AsyncStream<CoreEvent> { /* wrap UniFFI callback */ }
}

@Observable @MainActor
final class TodayViewModel {
    var tasks: [Task] = []
    func load() async {
        tasks = await CoreActor.shared.list(view: .today)
        for await event in await CoreActor.shared.subscribe() where event.affects(.today) {
            tasks = await CoreActor.shared.list(view: .today)
        }
    }
}
```

#### Navigation

- `TabView` at the root with five tabs: **Today · Inbox · Projects · Habits · Timer**.
- Each tab hosts a `NavigationStack` for drill-down (project → task → annotation).
- iPad replaces TabView with `NavigationSplitView` (see §8.2).
- Sheet for quick-add (invoked from FAB, Siri, share extension, widget tap).

#### Background refresh

- **BGAppRefreshTask** registered for periodic (≥15 min) sync attempts.
- **BGProcessingTask** for heavier rebuilds (urgency recompute, recurrence rollover) when on power + WiFi.
- **Push notifications** (silent) from sync server (Phase 5) trigger immediate pull on hosted-sync users.
- **Local notifications** are scheduled by core when a task/habit/focus reminder is set; the notification's `userInfo` carries an opaque action token so tapping → opens the relevant detail; long-press → "Done" action without opening app (handled by App Intent).

### 8.2 iPadOS Adaptations

```
┌────────────────────────────────────────────────────────────────────────────┐
│  ┌── Sidebar ─────┐ ┌── Content ─────────────────┐ ┌── Detail ───────────┐ │
│  │  Today      7  │ │ ◯ Write standup notes   ⓘ │ │  Write standup…     │ │
│  │  Inbox      3  │ │ ◯ Review PR #482        ⓘ │ │  ───                │ │
│  │  Upcoming  14  │ │ ◯ Dentist               ⓘ │ │  project: work      │ │
│  │  Anytime   31  │ │                            │ │  due: Today 09:00   │ │
│  │  Someday    9  │ │  Evening                   │ │  urgency: 8.7       │ │
│  │  Logbook       │ │ ◯ Read chapter 4        ⓘ │ │  notes ▼            │ │
│  │  ─── Projects  │ │ ◯ Plan week             ⓘ │ │  checklist ▼        │ │
│  │  Book draft    │ │                            │ │  annotations ▼      │ │
│  │  Home reno     │ │  Habits                    │ │  time blocks ▼      │ │
│  │  ─── Areas     │ │ ✓ Meditate  🔥23           │ │                     │ │
│  │  work          │ │ ◯ Read      🔥11           │ │  [▶ Start Timer]    │ │
│  │  home          │ │ ◯ Workout   🔥 4           │ │  [🍅 Focus 25m]     │ │
│  │  ─── Tags ─    │ │                            │ │  [Mark Done]        │ │
│  └────────────────┘ └────────────────────────────┘ └─────────────────────┘ │
└────────────────────────────────────────────────────────────────────────────┘
```

- `NavigationSplitView` with three columns; collapses to two on Slide Over.
- **Drag and drop**: reorder within a project; drag task → sidebar project/area to move; drag task → Today/Evening; drag task → habit list creates a "habit-from-task" stack (asks for cadence).
- **Multi-select**: long-press or two-finger pan; bulk modify via toolbar.
- **Keyboard shortcuts** (Smart Keyboard / Magic Keyboard) — see §8.3, same set.
- **Pencil**: handwriting auto-converts in description fields via Scribble; sketch attachments stored as note blocks (rendered as PNG in vault).
- **Stage Manager**: each window is a separate top-level state; CoreActor stays singleton, all windows observe the same event stream.

### 8.3 macOS Adaptations

The macOS app is **dual-mode**: a full window for browsing and a menu bar item for quick capture and status.

- **Full window** (`WindowGroup`): same `NavigationSplitView` as iPadOS, with proper macOS toolbar (segmented control for views, search field, add button).
- **Menu bar app** (`MenuBarExtra`): shows current timer + next task; click expands a compact popover with today list, quick-add field, focus controls. Always-on, regardless of main window.
- **Quick entry**: global hotkey (default `⌃⌥Space`, configurable) opens a borderless `NSPanel` floating window that accepts a single natural-language line. `⌘Return` submits, `Esc` cancels. Powered by the same parser as the CLI.
- **Window management**: `defaultSize(width: 1100, height: 720)`, `commands { CommandGroup(...) }` for File/Edit/View/Task/Habit/Timer menus, restorable state.

#### macOS keyboard shortcuts

| Shortcut       | Action                           |
|----------------|----------------------------------|
| `⌘N`           | New task (sheet)                 |
| `⌃⌥Space`      | Global quick entry               |
| `⌘1`–`⌘5`      | Switch tab (Today/Inbox/...)     |
| `⌘F`           | Search                           |
| `⌘K`           | Command palette                  |
| `Space`        | Toggle complete on selection     |
| `⌘D`           | Defer (date picker popover)      |
| `⌘E`           | Evening                          |
| `⌘T`           | Start/stop timer on selection    |
| `⌘⇧F`          | Start focus session              |
| `⌘Z` / `⌘⇧Z`   | Undo / redo                      |
| `⌘,`           | Settings                         |
| `⌘⌥L`          | Lock vault                       |

### 8.4 watchOS App

The Watch app is a **constrained companion**, not a mirror. It supports the high-frequency actions a user wants on the wrist: glance, tick, time.

#### Capabilities

- **View today list** (up to 20 items, paginated by digital crown).
- **Mark task complete** (tap + haptic confirmation).
- **Start/stop timer** on selected task or via "Quick Timer" (no task binding).
- **Start/stop focus session** (configurable default length).
- **Log a habit** (tap the habit chip; numeric habits open a quick stepper).
- **View streaks and today's habit progress.**

#### What lives where

| Data                       | On Watch | Source           |
|----------------------------|----------|------------------|
| Today list (top 20)        | Cached   | iPhone sync      |
| All habits + today status  | Cached   | iPhone sync      |
| Active timer state         | Live     | iPhone or local  |
| Focus session state        | Live     | iPhone or local  |
| Full vault                 | ✗        | iPhone only      |
| Projects, areas, search    | ✗        | iPhone only      |

The Watch never holds the full vault. It maintains a small **read replica** of the "actionable surface" (today + habits) and a **write-ahead log of intents** when the iPhone is unreachable. Intents are signed with a Watch-specific device key (provisioned during pairing via `WatchConnectivity`), replayed when the phone reconnects, and counter-signed by the phone before merging into the main event log.

#### Companion sync

- **Foreground**: `WCSession.sendMessage` for immediate intent dispatch.
- **Background**: `transferUserInfo` for queued intents; `updateApplicationContext` for today-list snapshot.
- **Standalone (LTE Watch, phone off)**: intents queued locally, sync via hosted server if `[sync.watch_direct]` enabled (Phase 7).

#### Complications (all families)

| Family                          | Content                                            |
|---------------------------------|----------------------------------------------------|
| `.circularSmall`                | Habit ring (today completion %)                    |
| `.modularSmall` / `.graphicCircular` | Next task icon + count, or 🍅 if focusing      |
| `.modularLarge` / `.graphicRectangular` | Next 3 tasks, OR active timer countdown      |
| `.utilitarianSmall`             | Active timer mm:ss (else habit %)                  |
| `.utilitarianLarge`             | Current task description                           |
| `.graphicCorner`                | Habit ring                                         |
| `.graphicBezel`                 | Next task title around watch face                  |
| `.graphicExtraLarge` (Ultra)    | Today's tasks + habit ring + timer                 |

Updates use `WidgetKit` (watchOS 9+) timelines refreshed by the companion app on mutation, with `reloadAllTimelines()` triggered by the iPhone's CoreActor event stream forwarded over `WatchConnectivity`.

### 8.5 Widgets (WidgetKit)

Widgets share a common Swift package `WidgetCore` that owns a read-only UniFFI handle into the same vault (group container `group.com.tock.app`). Widgets do **not** mutate; tap actions deep-link or invoke App Intents.

| Size                   | Content                                                              |
|------------------------|----------------------------------------------------------------------|
| `.systemSmall`         | Active timer countdown + task title; OR if idle, next task + due.   |
| `.systemMedium`        | Today list (3–4 items, checkbox interactive via App Intent).         |
| `.systemLarge`         | Today list (6 items) + habit ring strip + active timer footer.       |
| `.systemExtraLarge`    | (iPadOS) Two-column: Today + Inbox; habits row; week timer summary.  |
| `.accessoryCircular`   | Habit completion ring or task count badge.                           |
| `.accessoryRectangular`| Next task + due time.                                                |
| `.accessoryInline`     | "3 due · 🍅 12:34" status line.                                      |

- **Interactive widgets** (iOS 17+): checkboxes call `CompleteTaskIntent`, habit chips call `LogHabitIntent`. State refresh via `WidgetCenter.shared.reloadTimelines(ofKind:)` once the core event arrives.
- **Lock screen widgets**: the accessory family items above; rotation suppressed if vault is locked (show "🔒 Tap to unlock").
- **Standby mode** (iOS 17+): `.systemLarge` is registered as a Standby-eligible widget with night-mode tinting.

### 8.6 Siri Shortcuts (App Intents)

All intents are defined in Swift via `AppIntent` and delegate to `CoreActor`. They are **donation-rich**: every CLI action triggered via the app donates an intent so Siri learns user phrasing.

| Intent                          | Parameters                                            | Example phrase                                    |
|---------------------------------|-------------------------------------------------------|---------------------------------------------------|
| `AddTaskIntent`                 | `description`, `project?`, `due?`, `tags?`            | "Add task buy milk tomorrow to Tock"             |
| `CompleteTaskIntent`            | `task` (entity)                                       | "Mark dentist done in Tock"                      |
| `ShowTodayIntent`               | —                                                     | "Show today in Tock"                             |
| `StartTimerIntent`              | `task?`, `note?`                                      | "Start timer on review PR"                        |
| `StopTimerIntent`               | —                                                     | "Stop the timer"                                  |
| `StartFocusIntent`              | `task?`, `length?`                                    | "Focus for 25 minutes on writing"                 |
| `LogHabitIntent`                | `habit` (entity), `value?`                            | "Log meditation 10 minutes"                       |
| `ShowHabitStreakIntent`         | `habit?`                                              | "What's my reading streak?"                       |
| `CaptureToInboxIntent`          | `text`                                                | "Capture think about Q4 plan to Tock inbox"      |
| `OpenViewIntent`                | `view` (enum)                                         | "Open inbox in Tock"                             |
| `RunReportIntent`               | `report` (entity)                                     | "Run standup report"                              |

`AppEntity` types (`Task`, `Habit`, `Project`, `Report`) make tasks/habits first-class in the Shortcuts app — users can build flows like "When I arrive at work → Open work context → Show Today → Start focus 50 minutes on top task." Entity queries are paged from the core.

#### Automation triggers (Focus Filter)

Register a **Focus Filter** so when the user enables their "Deep Work" iOS Focus, the app activates the matching `context` and hides everything outside it. Implemented via `SetFocusFilterIntent`.

### 8.7 Share Extension

Quick capture from any app: Safari page, mail message, photo, plain text.

#### Extracted metadata

| Source           | Extracted                                          | Mapped to task field             |
|------------------|----------------------------------------------------|----------------------------------|
| URL              | URL, page title (via `LPMetadataProvider`)         | description = title, note = URL  |
| Selected text    | Text                                               | description (first line), note (rest) |
| Image            | UIImage (saved into vault as attachment block)     | description = prompt, attachment |
| File             | File ref                                           | description = filename, attachment |
| Mail message     | Subject, sender, message URL (`message:` URL)      | description = subject, note = sender + link |
| Map location     | Coordinate + place name                            | description = place, location UDA |

The extension UI is a single SwiftUI sheet:

```
┌─ Add to Tock ─────────────────────────────┐
│ ▢ Read: "How to design CRDTs"             │
│   <description, editable, prefilled>      │
│                                            │
│ project ▾  inbox          tags ▾  reading │
│ due ▾      none           !  none         │
│                                            │
│ url: https://example.com/crdts            │
│                                            │
│             [ Cancel ]   [ Add to Today ▾]│
└────────────────────────────────────────────┘
```

The split button defaults to "Add to Inbox"; long-press reveals Today / Evening / Someday / pick project. The extension speaks to the **same vault** as the main app via app group; if the vault is locked it shows a lock prompt that defers to the host app via universal link.

---

## 9. Import/Export & Interoperability

### 9.1 Canonical JSON Format

All import/export, hook I/O, and the `--json` flag emit the same canonical schema. Version is required and read on import for migration.

```jsonc
{
  "$schema": "https://tock.dev/schemas/tock/v1.json",
  "version": 1,
  "exported_at": "2025-03-13T17:45:00Z",
  "vault_id": "vlt_01HQZ...",
  "tasks": [
    {
      "id":           "tsk_01HQZABCXYZ...",      // ULID
      "sid":          "4f2",                      // short id (display only)
      "description":  "Write standup notes",
      "status":       "pending",                  // pending|completed|cancelled|deleted
      "area_id":      "area_01H...",
      "project_id":   "prj_01H...",
      "heading":      "Monday morning",           // text, lives under project
      "parent_id":    null,                       // for subtasks
      "tags":         ["work", "writing"],
      "priority":     "M",                        // null|L|M|H
      "start_date":   "2025-03-13",               // when actionable
      "due_date":     "2025-03-13T09:00:00Z",
      "scheduled_at": null,
      "today":        true,
      "evening":      false,
      "completed_at": null,
      "estimate_sec": 600,
      "actual_sec":   0,                          // sum of linked time blocks
      "checklist":    [
        {"id":"chk_...","text":"Review yesterday","done":true},
        {"id":"chk_...","text":"Draft summary","done":false}
      ],
      "annotations":  [
        {"id":"ann_...","at":"2025-03-12T18:00:00Z","text":"Sam asked to include metrics"}
      ],
      "depends_on":   ["tsk_01H..."],
      "blocks":       [],
      "recurrence":   {
        "rule":      "FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR",
        "anchor":    "due",
        "next_after": "complete"                 // complete|due — when to roll
      },
      "urgency":      8.7,                        // computed, included for export
      "uda":          { "effort": 3, "client": "acme" },
      "created_at":   "2025-03-10T12:00:00Z",
      "updated_at":   "2025-03-12T18:00:00Z"
    }
  ],
  "projects": [
    {
      "id":"prj_...", "sid":"p1a", "name":"Book draft",
      "area_id":"area_...", "status":"active",
      "deadline":"2025-06-01", "note":"…",
      "created_at":"...", "updated_at":"..."
    }
  ],
  "areas":   [ { "id":"area_...", "name":"work", "created_at":"..." } ],
  "habits":  [
    {
      "id":"hab_...", "sid":"h1m",
      "name":"Meditate",
      "identity":"I am someone who centers daily",
      "cue":"After morning coffee",
      "stack_after":"hab_coffee_id",
      "is_break_bad": false,
      "cadence": { "kind":"daily", "times":["07:00"] },
      "min": 1, "target": 10, "unit":"minutes",
      "level": 4, "xp": 1180,
      "streak_current": 23, "streak_longest": 41,
      "reminders":[{"kind":"local","at":"07:00"}],
      "accountable":[{"kind":"webhook","url":"https://…"}],
      "created_at":"...", "updated_at":"..."
    }
  ],
  "habit_logs": [
    {"id":"hlg_...","habit_id":"hab_...","at":"2025-03-13T07:12:00Z","value":10,"note":null,"skipped":false}
  ],
  "time_blocks": [
    {
      "id":"tbk_...", "sid":"b03",
      "task_id":"tsk_...", "project_id":"prj_...",
      "started_at":"2025-03-13T08:01:00Z",
      "ended_at":  "2025-03-13T08:43:00Z",
      "duration_sec": 2520,
      "billable": false,
      "tags":["work"],
      "note":null,
      "source":"manual"                            // manual|focus|imported
    }
  ],
  "focus_sessions": [
    {
      "id":"foc_...", "task_id":"tsk_...",
      "started_at":"...", "ended_at":"...",
      "planned_cycles":4, "completed_cycles":3,
      "length_sec":1500, "short_break_sec":300, "long_break_sec":900,
      "interruptions":1
    }
  ],
  "contexts":[{"name":"work","filter":"+work -archived"}],
  "udas":[{"key":"effort","type":"int","label":"Effort"}],
  "reports":[ /* report definitions */ ],
  "deleted":[{"kind":"task","id":"tsk_...","deleted_at":"..."}]
}
```

**Versioning**: integer `version`. Migrations are pure functions `migrate_v{n}_to_v{n+1}(json) -> json` chained at import. Unknown fields are preserved (forward-compat for downgrades inside the same major version). UDA values are typed; unknown UDA keys are imported as strings and surfaced for user mapping.

### 9.2 Taskwarrior Import/Export

#### Field mapping

| Taskwarrior          | App                          | Notes                                              |
|----------------------|------------------------------|----------------------------------------------------|
| `uuid`               | external id `taskwarrior:<uuid>` mapped → new ULID | bidirectional sidecar table  |
| `description`        | `description`                | exact                                              |
| `status` pending     | `status: pending`            |                                                    |
| `status` completed   | `status: completed`          | `end` → `completed_at`                             |
| `status` deleted     | `status: deleted`            |                                                    |
| `status` waiting     | `status: pending` + `+waiting` virtual tag + `start_date = wait` | TW "wait" → defer date |
| `status` recurring   | parent template, not exported as a task; spawns instances | recurrence preserved |
| `project` (dot)      | `project` with dot-notation preserved | `home.garden` → project tree path     |
| `tags`               | `tags`                       |                                                    |
| `priority` H/M/L     | `priority` H/M/L             |                                                    |
| `due`                | `due_date`                   |                                                    |
| `scheduled`          | `scheduled_at`               |                                                    |
| `wait`               | `start_date`                 | "defer until"                                      |
| `entry`              | `created_at`                 |                                                    |
| `modified`           | `updated_at`                 |                                                    |
| `start`              | open time-block of duration `now - start` | see §below                            |
| `end`                | `completed_at` or block end  |                                                    |
| `depends`            | `depends_on`                 | uuids translated to ULIDs                          |
| `annotations`        | `annotations`                | entry → at                                         |
| `urgency`            | recomputed (config-driven)   | TW value not preserved                             |
| `recur` + `until`    | `recurrence.rule` (translated to RRULE) | TW `weekly`/`monthly` → RRULE          |
| UDAs                 | `uda.<key>` (type preserved via `.type` declaration in TW config) |                              |

#### What is lost (and how it's surfaced)

| Lost                                    | Why                                  | Mitigation                          |
|-----------------------------------------|--------------------------------------|-------------------------------------|
| Per-task urgency override               | App computes from coefficients       | UDA-based `priority_boost`          |
| Hook scripts                            | Hook ABI differs                     | Rewrite using §7.5 protocol         |
| `imask` / template UUIDs                | App uses different recurrence model  | Instances generated fresh           |
| Aliases for reports                     | App uses TOML reports                | Auto-translate `report.X.*` keys    |

#### CLI

```
tock import taskwarrior                            # uses `task export` if in PATH
tock import taskwarrior --from ~/tw-export.json
tock export taskwarrior --out ~/tw-import.json     # produces TW-compatible JSON
```

Round-trip is **lossy in one direction** (TW → app preserves more; app → TW loses checklists, habits, time blocks, headings, identity, etc., which simply don't exist in TW). The app stores TW UUIDs in a sidecar so re-importing the same TW file is idempotent.

### 9.3 Things 3 Import

Things 3 doesn't have an official export, but offers AppleScript and a JSON URL scheme. We support two paths:

1. **AppleScript-driven** (macOS only): `tock import things3` runs a bundled `.scpt` that walks Areas → Projects → Tasks → Headings → Checklist items → To-dos, emitting our canonical JSON.
2. **Manual file**: `tock import things3 --from things3.json` accepts the JSON URL-scheme dump or third-party export tools (e.g. `things-cli export`).

#### Hierarchy

```
Things 3                       App
─────────────────────────────────────────────
Area              ─────────▶   Area
Project           ─────────▶   Project (area inherited)
Heading           ─────────▶   Project heading (text on tasks within)
To-Do             ─────────▶   Task
Checklist item    ─────────▶   Checklist entry
Tag (nested)      ─────────▶   Tag (flat with "/" preserved: "work/email")
```

#### Field mapping

| Things 3            | App                            | Notes                                  |
|---------------------|--------------------------------|----------------------------------------|
| `title`             | `description`                  |                                        |
| `notes`             | `note` (first annotation)      |                                        |
| `when` = today      | `today: true`                  |                                        |
| `when` = evening    | `today: true, evening: true`   |                                        |
| `when` = `<date>`   | `start_date`                   | Things "when" is start, not due        |
| `deadline`          | `due_date`                     |                                        |
| `tags`              | `tags` (nested → "/" path)     |                                        |
| `checklistItems`    | `checklist`                    |                                        |
| `status` open       | `pending`                      |                                        |
| `status` completed  | `completed`, `completed_at`    |                                        |
| `status` canceled   | `cancelled`                    |                                        |
| repeating task      | `recurrence.rule` (translated) | Things "every 2 weeks on Mon" → RRULE  |
| Trash               | not imported                   | `--include-trash` opt-in               |
| Logbook             | imported with `completed_at`   |                                        |

Headings become text grouping on the project (`heading` field) — preserved in detail view, projects list, and exports. Things' "Someday" maps to our `Someday` view via virtual filter (`start_date is null and project.status = 'paused' or +someday`); a `+someday` tag is added on import for clarity.

### 9.4 CSV Import/Export

#### Auto-detection

The importer reads the first row as a header, then matches column names against a built-in dictionary (case-insensitive, punctuation-insensitive):

```
description, title, task, name           → description
due, due date, deadline, due_date        → due_date
start, start date, defer, scheduled      → start_date
project, list                            → project
area, folder, category                   → area
tags, labels, categories                 → tags (split by , ; |)
priority, pri                            → priority (mapping H/M/L/!/!!/!!! or 1-3)
status, state, completed                 → status
notes, note, description2                → note
estimate, eta, duration                  → estimate_sec (parsed)
```

Unmatched columns are imported as UDAs (`uda.<column_name>`) with `string` type; the user is prompted to retype them post-import (`tock uda retype <key> --type int`).

#### CLI / config

```
tock import csv --from tasks.csv                        # auto-detect
tock import csv --from tasks.csv --map csv-map.toml     # explicit mapping
tock export csv --columns sid,description,due,project,urgency \
                 --filter '+TODAY' --out today.csv
```

`csv-map.toml`:

```toml
[columns]
"Task Name"   = "description"
"Due"         = { field = "due_date", format = "%m/%d/%Y" }
"List"        = "project"
"Tags"        = { field = "tags", split = "," }
"Effort"      = { uda = "effort", type = "int" }
[defaults]
status = "pending"
area   = "imported"
```

### 9.5 CalDAV Integration

CalDAV is treated as a **secondary, optional** sync surface alongside the primary E2EE sync (which CalDAV servers can't participate in because of E2EE). Used mainly for **calendar visibility** (time blocks on a work calendar) and **interop with iOS Reminders / macOS Calendar**.

#### Mapping

| iCalendar          | App                                                            |
|--------------------|----------------------------------------------------------------|
| `VTODO`            | Task                                                           |
| `SUMMARY`          | `description`                                                  |
| `DESCRIPTION`      | `note` / first annotation                                      |
| `DUE`              | `due_date`                                                     |
| `DTSTART` (VTODO)  | `start_date`                                                   |
| `PRIORITY` 1–4     | `H`                                                            |
| `PRIORITY` 5       | `M`                                                            |
| `PRIORITY` 6–9     | `L`                                                            |
| `STATUS`           | maps to pending/in-process/completed/cancelled                 |
| `PERCENT-COMPLETE` | derived from checklist completion (export); ignored on import unless `=100` |
| `CATEGORIES`       | tags                                                           |
| `RELATED-TO`       | `depends_on` (RELTYPE=DEPENDS-ON)                              |
| `RRULE`            | `recurrence.rule`                                              |
| `X-APP-PROJECT`    | `project` (custom prop)                                        |
| `X-APP-UDA-*`      | UDAs (custom props)                                            |
| `VEVENT`           | Time block (`DTSTART`/`DTEND`) or focus session                |
| `CATEGORIES=focus` | Focus session                                                  |
| `X-APP-TASK-ID`    | `task_id` linkage                                              |

#### Two-way sync behavior

- One CalDAV collection per "channel" the user configures (e.g. `Work Tasks`, `Personal Tasks`, `Time Blocks`).
- Maintains a sidecar `caldav_links` table: `(local_id, collection, href, etag, last_pushed_at, last_pulled_at)`.
- Sync loop: pull (changed `Etag`) → resolve → push (PUT with `If-Match: <etag>`).
- App-level fields without iCal equivalents (habits, headings, identity, checklist items beyond first) are stored in `X-APP-*` props; foreign clients ignore them; we round-trip them.

#### Conflict handling between CalDAV and primary E2EE sync

- The **primary sync (event log)** is the source of truth; CalDAV is a projection.
- On conflict (CalDAV mutation arrives for an entity that mutated locally first): we apply the standard CRDT merge to the local entity, then **re-push** the merged state to CalDAV with `If-Match`. If `412 Precondition Failed`, pull again and repeat (bounded to 3 attempts; then surface to user).
- Tasks **deleted** on CalDAV are *not* deleted in the app — they are *unlinked* and a notification is shown ("Calendar removed Task X. Delete here too?"). Prevents foreign-client mistakes from wiping data.

### 9.6 Markdown Export

Markdown is for humans: standups, weekly reviews, shareable summaries. Templated with [Tera](https://tera.netlify.app/) so users can customize.

#### Default task-list template

```markdown
# Today — {{ today | date(format="%A, %B %-d %Y") }}

{% for group_name, items in tasks | group_by(attribute="project") -%}
## {{ group_name | default(value="(no project)") }}

{% for t in items -%}
- [{% if t.status == "completed" %}x{% else %} {% endif %}] **{{ t.description }}**
  {%- if t.due_date %} _due {{ t.due_date | date(format="%a %H:%M") }}_{% endif %}
  {%- if t.tags %} {% for tag in t.tags %}`#{{ tag }}`{% endfor %}{% endif %}
  {%- if t.estimate_sec %} ⏱ {{ t.estimate_sec | duration }}{% endif %}
{% endfor %}
{% endfor %}
```

#### Habit report template

```markdown
# Habits — Week of {{ week_start | date(format="%b %-d") }}

| Habit       | Identity                              | M  T  W  T  F  S  S | Streak |
|-------------|---------------------------------------|---------------------|--------|
{% for h in habits -%}
| {{ h.name }} | {{ h.identity | truncate(length=30) }} | {{ h.week_marks }} | 🔥 {{ h.streak_current }} |
{% endfor %}
```

#### Time report template

```markdown
# Time — {{ range_label }}

**Total:** {{ total | duration }}  ·  **Billable:** {{ billable | duration }}

| Project        | Task                       | Duration |
|----------------|----------------------------|----------|
{% for row in rows -%}
| {{ row.project }} | {{ row.task }} | {{ row.duration | duration }} |
{% endfor %}
```

CLI:

```
tock export md --filter '+TODAY' --template ~/.config/tock/templates/standup.md
tock report week --format md > weekly-review.md
tock habit ls --format md > habits.md
```

---

## 10. Development Roadmap

Eight phases. Each ships a usable artifact to keep dogfooding pressure high. Estimated complexity is calibrated for a **1–2 person team**; "Very High" means months, not weeks. **The roadmap is sequenced so that the core team can dogfood from Phase 1 onward, and other people can dogfood from Phase 2.**

### Phase 0 — Foundation
**Complexity: High** · Depends on: nothing

- Monorepo scaffolded (`crates/tock-core`, `-cli`, `-server`; `bindings/swift`; `apps/`).
- Workspace `Cargo.toml`, `rust-toolchain.toml` pinned, `cargo deny`, `cargo clippy -D warnings`, `cargo fmt --check` in CI.
- CI/CD: GitHub Actions for Linux/macOS/Windows, code coverage, release artifacts (cargo-dist), signed binaries for macOS, Homebrew tap, Nix flake.
- Crypto primitives wrapped (XChaCha20-Poly1305, Argon2id, X25519, Ed25519) behind `crypto::` module with property tests.
- Vault: SQLite + sqlcipher OR SQLite + app-layer encryption (decision in part 1 §3); migration framework.
- Event log skeleton (append-only, signed, deterministic IDs).
- `tracing`-based logging with redaction filter for vault data.
- **Testable**: `tock vault init/unlock/lock/status` works; passes property tests; round-trips encrypted writes.

### Phase 1 — CLI Task Manager MVP
**Complexity: Medium** · Depends on: Phase 0

- Task CRUD: `add`, `mod`, `done`, `cancel`, `delete`, `undo`, `redo`, `ls`, `show`, `annotate`.
- Projects, areas, headings, tags (flat first; nested in Phase 4).
- Natural-language parser v1: dates, durations, sigils, priority markers (see §7.2).
- Filter language: tags, fields, virtual tags `+TODAY` `+OVERDUE`, dot paths.
- Built-in views: inbox, today, upcoming, anytime, someday, logbook.
- Output: human (color tables) + `--json`.
- Shell completions (`bash`, `zsh`, `fish`, `nu`, `pwsh`).
- Import: JSON (own format) for testing.
- **Testable**: a single user can fully manage their tasks from the CLI for the everyday basics.

### Phase 2 — Time Tracking + Focus Timer
**Complexity: Medium** · Depends on: Phase 1

- `start`, `stop`, `resume`, `current`, `blocks`, `report`, `edit`.
- On-the-fly task creation from `start`.
- Block storage with conflict checks (`allow_overlap` config).
- Built-in reports: today, week, month, by project/task/tag.
- Focus: `focus start/stop/pause/resume/skip-break/status/stats`.
- Notifications: cross-platform (`notify-rust` on Linux, NSUserNotification on macOS, toast on Windows).
- Idle detection (Phase 2.5 stretch; basic in-terminal-only first).
- **Testable**: complete a full Pomodoro day end-to-end; produce a weekly time report.

### Phase 3 — Habit Tracking
**Complexity: Medium** · Depends on: Phase 1

- Habit CRUD with guided creation wizard (`habit add` without `--no-wizard`).
- Identity statements, cues, habit stacking (parent/child links).
- Streaks (with `streak_grace_days`), longest streak, level/XP curve.
- Cadences: daily, weekly (specific days), N-per-week, custom RRULE.
- Logging: numeric values, units, skip with reason, undo.
- Break-bad-habit mode (inverted scoring).
- Reminders: local notifications.
- **Testable**: track a full set of daily habits from the CLI, including identity statements and stacking.

### Phase 4 — Advanced Task Features
**Complexity: High** · Depends on: Phases 1–3

- UDAs (declared in config; typed; in queries; in reports).
- Hook scripts API (§7.5): on-add, on-modify, on-complete, on-delete, on-habit-log, on-timer-*, on-focus-*, on-launch, on-exit.
- Urgency engine (fully configurable coefficients).
- Custom reports (TOML-defined; `report.<name>.*`).
- Dependencies + `+BLOCKED`/`+BLOCKING` virtual tags.
- Recurrence: RRULE, instance generation strategies (on-due vs on-complete).
- Contexts (filter-as-context).
- Nested tags.
- TUI v1 (`ratatui`): all of §7.3.
- **Testable**: power-user task management with UDAs, hooks, custom reports, dependencies, recurrence, and contexts.

### Phase 5 — Sync + Server
**Complexity: Very High** · Depends on: Phases 0–4

- Event log → wire format → sync protocol (E2EE; details in part 1 §5).
- Conflict resolution (CRDT-ish per-field LWW with concurrent-edit promotion to annotation).
- Device pairing flow (`onboard invite` / `accept`, QR + 6-word phrase).
- Device revocation, key rotation, recovery shares.
- Self-hosted server (`tock-server`, Axum, AGPL-3.0): receives encrypted blobs, append-only log per vault, never sees plaintext.
- Container image, Helm chart, systemd unit.
- Hosted service skeleton (same binary, billing layer, S3-compatible blob backend).
- **Testable**: two CLI installs sync the same vault; offline edits merge cleanly; server can't decrypt.

### Phase 6 — iOS / iPadOS App
**Complexity: Very High** · Depends on: Phases 1–5

- UniFFI bindings + Swift packaging.
- SwiftUI app: Today, Inbox, Projects, Habits, Timer, Settings.
- `NavigationSplitView` on iPadOS.
- Vault unlock via biometrics (LAContext) + keychain-cached key.
- Quick-add sheet, share extension, App Intents (§8.6), interactive widgets (§8.5).
- BGAppRefreshTask + push-triggered sync for hosted-sync users.
- TestFlight beta.
- **Testable**: full iPhone/iPad app; Tock App Store submission ready.

### Phase 7 — macOS + watchOS + Polish
**Complexity: High** · Depends on: Phase 6

- macOS native app (full window + `MenuBarExtra` + global hotkey).
- watchOS companion (§8.4) + complications (all families).
- CalDAV bidirectional sync (§9.5).
- Imports: Taskwarrior (§9.2), Things 3 (§9.3), CSV (§9.4).
- Markdown export with Tera templates (§9.6).
- Onboarding flows in apps (first-launch wizard, identity-habit picker).
- Performance pass: cold-start, large-vault queries, sync throughput.
- Accessibility audit (VoiceOver, Dynamic Type, color contrast).
- Localization framework (strings extracted; English ships; community translations welcomed).
- 1.0 release.
- **Testable**: full ecosystem; ready for general public launch.

---

## 11. Monetization Analysis

The bar to clear: **must sustain a 1–2 person team indefinitely** *and* **must not betray the Tock ethos** (privacy-first, AGPL server, open core, no dark patterns).

### Model A — Fully open source, no monetization

- **Sustainability**: poor. Donations rarely cover a single FTE; OSS burnout common. Server hosting costs are recurring even if code is free; someone pays.
- **Community**: maximal. Easiest for contributors; widest adoption.
- **Adoption friction**: lowest.
- **Ethos**: perfect.
- **Competitive position**: niche, technical, no growth budget — comparable to other community-driven open-source productivity projects.

**Verdict**: ethically pure, financially fragile. Acceptable only if maintainers explicitly want a hobby project. Not viable as a "go big" plan.

### Model B — Open core + paid hosted sync (self-host free, hosted paid)

- **Sustainability**: solid. Recurring revenue, low support burden (sync is the only paid surface), aligns user value with payment. With 500 paid users at $4–6/mo = ~$30k ARR; 2,000 = ~$120k ARR. Realistic for a niche productivity tool with strong distribution.
- **Community**: strong. CLI is free, server is AGPL → self-hosters and corporate users contribute; hosted is the convenience tier.
- **Adoption friction**: low. Everything works without paying; payment is opt-in for hosted convenience.
- **Ethos**: excellent. AGPL server keeps the field level. Hosted = pay for hardware, not for code.
- **Competitive position**: directly serves the convenience-sync market segment without surrendering privacy or openness.

**Verdict**: best alignment, proven model (Standard Notes, Bitwarden, Obsidian Sync), strong moat.

### Model C — Open core + paid native apps (CLI free, iOS/macOS paid)

- **Sustainability**: spiky one-time revenue. Some established native-app businesses sustain a team via this model, but typically after a long brand head start, premium pricing, and exclusive platform focus. Replicating it as a new entrant is risky: each platform release is a revenue event, not a stream.
- **Community**: medium. CLI users get value; Apple users feel gated.
- **Adoption friction**: medium-high on Apple. Charging $30 up front for a new app with no reputation is a steep ask.
- **Ethos**: acceptable but slightly off — paying for the *interface* not the *service* is harder to justify in 2025 when the app builds against AGPL-licensed components a competitor can rebrand.
- **Competitive position**: competes head-to-head in a well-established native-app market segment. Hard.

**Verdict**: workable for Apple-only operators with brand recognition; bad fit for a brand-new ecosystem entrant.

### Model D — Open core + paid sync + paid apps

- **Sustainability**: best on paper, worst on perception. Double-charging looks greedy especially when the CLI is free.
- **Community**: bifurcated.
- **Adoption friction**: highest. "Pay $30 for the app and $5/mo to sync it?"
- **Ethos**: shaky. Two paywalls on a privacy-first OSS project invites criticism.
- **Competitive position**: muddled.

**Verdict**: only viable if both the apps and the sync are exceptional; not a starting position.

### Recommendation: **Model B**, with a small twist

**Open core + paid hosted sync.** Specifically:

1. **All code Apache-2.0** (core, CLI, Swift bindings, iOS/macOS/watchOS apps).
2. **Server AGPL-3.0** so a forked SaaS competitor must publish their modifications. This is the strongest defensive license without becoming user-hostile.
3. **Apple apps free on the App Store as Tock** — distribution and discovery matter more than App Store revenue. Free apps with paid hosted sync convert; paid apps with a free CLI confuse.
4. **Hosted sync tiers**:
   - **Free**: single device or local-network sync only; no hosted relay.
   - **Personal $4/mo or $36/yr**: unlimited devices, hosted relay, encrypted backups, 7-day point-in-time restore.
   - **Family $7/mo**: up to 6 vaults under one bill.
   - **Pro $9/mo**: priority support, CalDAV proxy, longer backup retention.
   - **Self-hosted**: free forever, AGPL server.
5. **No feature gating on free CLI**. The only paywalled thing is "use our servers." Self-hosting is a first-class path, not a punishment.
6. **No tracking, no analytics opt-in by default** — paying users are paying *for* privacy, not despite it.

**Why this wins**:

- It mirrors the proven Bitwarden / Standard Notes / Obsidian playbook in a category that genuinely lacks an open-source-first option.
- It rewards the technical-user community (which is the Tock core audience — see the broader kafkade ecosystem) with a free, no-strings tool.
- It captures the "I want it to just work" segment via hosted sync, without compromising the principled offering.
- It positions cleanly as a privacy-first, open-source-first option in a category dominated by closed and cloud-only incumbents.

---

## 12. Naming Decision

The name is now locked to **`tock`** for the CLI and **Tock** for user-facing surfaces.

### Decision summary

- **Selected name:** `tock`
- **Why it won:** short, rhythmic, time-evocative, easy to type, and a strong fit for a fast CLI-first product.
- **Product framing:** it suggests cadence and momentum through the familiar "tick-tock" association without sounding like a generic clock app.

### Known conflicts

- **crates.io:** `tock` already exists as a digital clock crate with modest usage.
- **App Store:** **Tock** is already used for restaurant reservations.
- **Assessment:** neither conflict is in the same product domain as this app, so the name remains workable.

### Packaging and distribution strategy

- Publish crates under names such as `tock-core`, `tock-cli`, and `tock-server`.
- If the root crate name is needed, prefer `tock-app` or `tock-cli` while keeping the installed binary name as **`tock`**.
- Ship Apple clients under the display name **Tock**.
- Use an App Store listing such as **Tock — Tasks & Habits** for clarity and differentiation.

### CLI guidance

- All command examples in this architecture now use `tock`.
- Recommend `tt` as the documented power-user shell alias in the README.

### Revisit policy

The team should proceed with **tock** as the working product name, but may revisit it before launch if a clearly better option surfaces.


---

## Acknowledgments & Prior Art

Tock stands on the shoulders of decades of thinking, research, and craft from the
personal-productivity community. We want to acknowledge — explicitly and gratefully —
the ideas and ecosystems that shaped this design:

- **Getting Things Done (GTD)**, David Allen — the foundational vocabulary
  (inbox, next actions, areas, projects, contexts, weekly review) that underpins
  Tock's view system and triage flow.
- **Atomic Habits**, James Clear — the framing of habits as identity-shaping
  practices, the cue/craving/response/reward loop, "make it obvious / attractive /
  easy / satisfying," habit stacking, and the "start small" minimum. These ideas
  inform the structure of Tock's habit model without prescribing a particular
  methodology.
- **Taskwarrior** — the proof that a CLI-first, query-language-driven task tool can
  serve serious knowledge workers; the inspiration for urgency-as-coefficients,
  virtual tags, user-defined attributes, and hook-driven extensibility. Tock's
  import path (§9.2) is designed to make Taskwarrior users feel at home.
- **The broader open-source time-tracking community** — including projects that
  pioneered ergonomic CLI time-block tracking — for showing how natural-language
  start/stop commands can disappear into a developer's workflow.
- **The Pomodoro Technique®**, Francesco Cirillo — the original 25/5 cadence and
  the principle that protected focus + scheduled breaks beat heroic concentration.
- **Standard Notes, Bitwarden, and Obsidian** — for demonstrating that an
  open-source-first, end-to-end-encrypted product can be commercially sustainable
  through honest hosted-sync pricing without compromising the principled offering.

Tock is not a clone of any of these. It is a synthesis: a unified data model and
sync substrate that lets the ideas above compose with each other instead of
living in four disconnected apps. Where prior art applies, we cite it; where Tock
diverges, the divergence is intentional and documented.

---

## Ecosystem

Tock is one of several focused, privacy-first tools in the **kafkade** family.
Each tool is independently useful, but they share an architectural spine:

- A pure-Rust core with the **"core has zero I/O"** invariant.
- An **end-to-end encrypted vault** (Argon2id → MK → VK → per-item keys, AES-256-GCM
  envelope encryption with size-bucket padding and domain-separated AAD).
- An **event-sourced sync protocol** with vector-clock conflict detection,
  field-level LWW merge, and append-only logs for commutative collections.
- A **CLI-first surface** with a ratatui TUI option, UniFFI bindings for native
  Apple apps, and a WASM path for the web.
- **AGPL-3.0 server, Apache-2.0 everything else**, with a documented vault format
  and protocol so users are never locked in.

### Siblings

- **ldgr** — a personal-finance ledger and budgeting tool. Plain-text-friendly,
  double-entry-capable, with the same vault format and the same crypto/sync
  guarantees. Tock and ldgr can coexist in the same configuration tree and share
  device-pairing flows; they do not share data, but they share trust.
- **pildora** — a medication and supplement tracker with adherence logging,
  reminder scheduling, and refill tracking. Highly sensitive data; protected by
  the same end-to-end encryption design, with a deliberately minimal surface
  area appropriate to medical-adjacent information.

The intent is that a user who chooses one kafkade tool can adopt the others with
near-zero ceremony: the same recovery key flow, the same device-onboarding QR
exchange, the same self-hosted-or-hosted choice for sync, the same export-at-any-time
guarantee. The tools are deliberately separate binaries — there is no
"super-app" — but they are designed to feel like one ecosystem.

