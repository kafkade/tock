# ADR-008: Four unified domains — tasks, habits, time tracking, focus

**Status:** Accepted  
**Date:** 2026-05-20

## Context

Users currently maintain separate tools for task management, habit tracking, time tracking, and focus timers (Pomodoro). This creates manual integration burden:

- Completing a Pomodoro session doesn't automatically log time to the task.
- Habit tracking is disconnected from tasks (can't mark "meditated today" as satisfying a recurring task).
- Time reports can't aggregate across tasks, habits, and focus sessions.
- Filtering and urgency scoring are siloed per tool.

Existing productivity tools either:
1. Specialize in one domain (Things 3 for tasks, Streaks for habits, Toggl for time tracking), requiring manual bridges.
2. Bolt features onto a single domain (Todoist adds "habits" as recurring tasks, losing identity-based habit design).

We need a unified data model where all four domains are first-class, share IDs and events, and support cross-domain primitives.

## Decision

**Single data model spanning four domains:**

1. **Tasks:** Hierarchical (Area → Project → Heading → Task → Checklist), with GTD-style views (Inbox, Today, Someday, Logbook), urgency scoring, dependencies, recurrence, and user-defined attributes.

2. **Habits:** First-class entities (not just recurring tasks) with identity statements, cues, cravings, responses, rewards, habit stacking, progression/leveling, streaks (with grace periods), and optional accountability sharing.

3. **Time tracking:** Time blocks linked to tasks, habits, or projects. Manual start/stop or auto-generated from Pomodoro. Supports billable tracking, hourly rates, and cross-domain aggregation.

4. **Focus (Pomodoro):** Configurable work/break cycles linked to tasks. On completion, auto-logs a time block and optionally increments a habit (e.g., "deep work").

**Cross-domain primitives:**
- Focus session → time block + habit XP.
- `tock start <task>` → task status = `started`, urgency ↑.
- `tock done <task>` → stops timer, completes focus session, logs time block.
- Habit completion can satisfy a recurring task (`Task.satisfied_by_habit: Option<Uuid>`).
- Project views aggregate tasks + habits + time blocks in one query.
- Recurring task ↔ habit promotion: `tock task promote-to-habit 42` archives the recurring template and creates a habit with the same cadence.

**Methodology-neutral:**
GTD views (Inbox, Today, Anytime, Someday) are seeded saved queries, not hardcoded. Users can delete, edit, or replace them. Identity-based habits are encouraged but not required (users can create habits without identity statements). The system never enforces a workflow; it provides primitives.

**Single query language:**
Filter DSL extends uniformly across all domains: `status:pending +work urgency.over:10 limit:10 sort:urgency-` works for tasks. `cadence:daily streak.over:7` works for habits. `billable:true since:monday` works for time blocks.

## Consequences

**Positive:**
- Eliminates manual bridges (Pomodoro → time block is automatic).
- Cross-domain aggregation (project view shows tasks, habits, and time spent).
- Unified urgency engine (active timer boosts task urgency).
- Single export format (JSON, Markdown) spans all domains.
- Users outgrowing single-domain tools can consolidate workflows.

**Negative:**
- Increased surface area vs. single-domain tools (more concepts to learn).
- Users who only need tasks may find habits/time tracking overwhelming (mitigated by progressive disclosure: features activate only when used).

**Neutral:**
- Methodology-neutral design means no built-in GTD enforcement (some users expect opinionated workflows; they can build them via contexts and reports).
- Cross-domain primitives require careful conflict resolution (e.g., focus session on task A, timer on task B → both generate time blocks; resolved via sync overlap tagging).
