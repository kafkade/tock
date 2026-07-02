# tock CLI — core/shared messages (en-US, fallback locale).
#
# Translator notes:
#   * Lines beginning with `#` are comments; `##` group headers, `###` file headers.
#   * `{ $var }` placeholders are substituted at runtime — keep them intact.
#   * Keep message ids unchanged; translate only the text after `=`.

## Generic

error-prefix = error: { $message }

## CalDAV

caldav-collection-removed = Removed CalDAV collection: { $url }
caldav-all-links-deleted = All links to this collection have been deleted.
caldav-collection-configured = CalDAV collection configured: { $name }
caldav-setup-url = URL: { $url }
caldav-setup-user = User: { $user }
caldav-setup-hint = Run `tock caldav sync` to start syncing.
caldav-no-collection-at = No collection configured at: { $url }
caldav-run-setup-first = Run `tock caldav setup` first.
caldav-no-collections = No CalDAV collections configured.
caldav-run-setup-to-add = Run `tock caldav setup` to add one.
caldav-syncing-collection = Syncing collection: { $name }
caldav-dry-run-push-count = Would push { $count } resource(s)
caldav-sync-op-update = update
caldav-sync-op-create = create
caldav-dry-run-note = (dry run — no changes applied)
caldav-sync-summary = { $tasks } task(s), { $blocks } time block(s), { $links } existing link(s)
caldav-sync-push-count = { $count } resource(s) to push
caldav-sync-transport-note = Note: CalDAV sync requires a network transport implementation.
caldav-sync-transport-not-wired = The sync engine is ready but HTTP transport is not yet wired.
caldav-collections-header = CalDAV collections:
# Fallback name for collections without a display name.
caldav-unnamed = (unnamed)
caldav-status-url = URL:        { $url }
caldav-status-user = User:       { $user }
caldav-status-sync-token = Sync token: { $token }
caldav-status-last-sync = Last sync:  { $time }
caldav-status-last-sync-never = Last sync:  never
caldav-status-linked = Linked:     { $count } resource(s)

## Import / Export

import-done = Imported { $count } { $count ->
    [one] task
   *[other] tasks
}
import-unsupported-format = unsupported format: { $format } (supported: json, taskwarrior, things3, csv)
export-unsupported-format = unsupported format: { $format } (supported: json, md)

## Views

views-unknown = unknown view '{ $name }'. Available: { $available }

## Agenda

agenda-header = Agenda — { $date } ({ $weekday })
agenda-empty = Nothing scheduled and no time blocks for { $date }
agenda-scheduled-heading = Scheduled tasks:
agenda-blocks-heading = Time blocks:
agenda-all-day = all-day
agenda-overlap = overlaps time block '{ $block }'
agenda-unparseable = could not understand day '{ $input }'

## Projects / Areas / Tags

project-created = Created project #{ $sid } — { $name }
project-count = { $count } { $count ->
    [one] project
   *[other] projects
}
project-archived = Archived project #{ $sid }
area-created = Created area — { $name }
area-count = { $count } { $count ->
    [one] area
   *[other] areas
}
tag-count = { $count } { $count ->
    [one] tag
   *[other] tags
}
tag-renamed = Renamed #{ $old } → #{ $new }

## Reports

report-defined = Defined report '{ $name }' — query: { $query }
report-none-saved = No saved reports. Examples:
report-example-overdue = tock report define overdue --query '+OVERDUE' --sort deadline
report-example-urgent = tock report define urgent  --query 'status:pending priority:H' --sort urgency
report-example-work = tock report define work    --query 'tag:work status:pending' --columns sid,title,deadline
# Column label in report list.
report-list-query-label = query:
# Column label in report list.
report-list-sort-label = sort:
report-count = { $count } { $count ->
    [one] report
   *[other] reports
}
report-not-found = report '{ $name }' not found
report-removed = Deleted report '{ $name }'

## Context

context-activated = Activated context { $name }
context-cleared = Cleared active context
context-defined = Defined context { $name }
context-none-defined = No contexts defined
context-removed = Removed context { $name }
# Banner shown above task listings when a context is active.
context-active-banner = [ctx: { $name }]

## Hooks

hooks-none-installed = No hooks installed

## Time tracking

time-stopped = Stopped #{ $sid } — { $title }
time-started = Started #{ $sid } — { $title } ({ $time })
time-stopped-duration = Stopped #{ $sid } — { $title } ({ $duration })
time-no-timer = No timer running
time-resumed = Resumed #{ $sid } — { $title }
time-current-running = #{ $sid } — { $title } (running { $duration })
# Column headers for time blocks table.
time-blocks-col-sid = SID
time-blocks-col-title = Title
time-blocks-col-started = Started
time-blocks-col-duration = Duration
# Shown when a time block has no end time yet.
time-running = running
time-blocks-count = { $count } { $count ->
    [one] block
   *[other] blocks
}
time-report-header = Time report: { $period }
# Column headers for time report table.
time-report-col-title = Title
time-report-col-duration = Duration
time-report-col-total = Total
time-block-updated = Updated block #{ $sid } — { $title }

## Focus

focus-already-active = Focus session #{ $sid } is already active ({ $state })
focus-notify-started-title = Focus started
focus-notify-started-body = 🍅 Work for { $minutes } minutes
focus-started = Focus #{ $sid } started — { $minutes } min work × { $cycles } cycles
focus-none-active = No active focus session
focus-notify-complete-title = Focus complete!
focus-notify-complete-body = 🎉 { $cycles } cycles done
focus-complete = Focus #{ $sid } complete! { $cycles } cycles
focus-notify-pomodoro-done-title = Pomodoro done!
focus-notify-pomodoro-done-body = Take a { $minutes } min break
focus-cycle-done = Cycle { $completed }/{ $planned } done — { $minutes } min { $state } break
focus-skip-break = Skipped break — working (cycle { $cycle })
focus-paused = Focus #{ $sid } paused
focus-resumed = Focus #{ $sid } resumed
focus-aborted = Focus #{ $sid } aborted after { $cycles } cycles
focus-status-line = Focus #{ $sid } — { $state } ({ $completed }/{ $planned })
focus-status-elapsed = Elapsed: { $duration }
focus-status-config = Config: { $work } work / { $short } short / { $long } long
focus-stats-header = Focus stats: { $period }
focus-stats-completed = Completed: { $cycles } cycles ({ $sessions } sessions)
focus-stats-aborted = Aborted: { $sessions } sessions
focus-stats-time = Focus time: { $hours }h { $minutes }m
focus-history-header = Focus history for task #{ $sid } — { $title }
focus-history-sessions = Focus sessions ({ $count }):
focus-history-blocks = Time blocks ({ $count }):

## Habits

habit-slip-logged = Slip logged for #{ $sid } — { $title } ({ $streak } { $label })
habit-reminders-cleared = Cleared all reminders for habit #{ $sid }
habit-no-reminders = No reminders set for habit #{ $sid }
habit-reminder-added = Added reminder at { $time } for habit #{ $sid }
habit-remind-usage = Specify --at <HH:MM>, --list, or --clear
habit-created = Created habit #{ $sid } — { $title }
# Column headers for habit list table.
habit-col-sid = SID
habit-col-dir = Dir
habit-col-level = Level
habit-col-streak = Streak
habit-col-title = Title
habit-col-identity = Identity
# Level abbreviation shown in compact table column.
habit-level-display = L{ $level } ({ $name })
# Days-streak abbreviation shown in compact table column.
habit-streak-display = { $days }d
habit-count = { $count } { $count ->
    [one] habit
   *[other] habits
}
habit-not-found = habit #{ $sid } not found
habit-show-direction = Direction: { $value }
habit-show-identity = Identity: { $value }
habit-show-cue = Cue: { $value }
habit-show-craving = Craving: { $value }
habit-show-response = Response: { $value }
habit-show-reward = Reward: { $value }
habit-show-cadence = Cadence: { $value }
habit-show-minimum = Minimum: { $value }
habit-show-level = Level: L{ $level } ({ $name })
habit-show-xp = XP: { $value }
habit-show-streaks = Streaks: current { $current }d / best { $best }d
# Label for the stacking field.
habit-stacking-none = none
habit-show-stacking = Stacking: { $value }
habit-show-area = Area: { $value }
habit-show-project = Project: { $value }
habit-show-created = Created: { $value }
habit-show-modified = Modified: { $value }
habit-show-archived = Archived: { $value }
habit-show-entries = Entries: { $count } (next: { $next })
habit-show-stacked-none = Stacked children: none
habit-show-stacked-header = Stacked children:
# Shown when a habit entry was logged as a slip.
habit-logged-slip = Logged slip
# Shown when a habit entry was logged normally.
habit-logged-habit = Logged habit
habit-log-result = { $outcome } #{ $sid } — streak { $streak }d, { $xp }
habit-skipped = Skipped habit #{ $sid } on { $date } — streak { $streak }d
habit-frozen = Froze habit #{ $sid } on { $date } — streak { $streak }d
habit-backfilled = Backfilled habit #{ $sid } on { $date } — streak { $streak }d, { $xp }
habit-streak-current = Current streak: { $days }d
habit-streak-best = Best streak: { $days }d
habit-entries-none = Recent entries: none
habit-entries-header = Recent entries:
# Suffix for entries marked as a slip.
habit-slip-label = slip
habit-modified = Modified habit #{ $sid } — { $title }
habit-archived = Archived habit #{ $sid }
habit-none-active = No active habits

## UDA

uda-created = Created UDA '{ $key }' ({ $type_name })
uda-count = { $count } { $count ->
    [one] UDA definition
   *[other] UDA definitions
}
uda-removed = Removed UDA '{ $key }'

## Dependencies

depend-added = Task #{ $sid } now depends on #{ $on }
depend-removed = Task #{ $sid } no longer depends on #{ $from }

## Annotations

annotate-added = Annotated task #{ $sid }
annotate-removed = Removed annotation [{ $index }] from task #{ $sid }
annotate-empty = Annotation text cannot be empty
annotate-index-not-found = task #{ $sid } has no annotation at index { $index }
task-show-annotations = Annotations:

## Tasks

task-modified = Modified task #{ $sid } — { $title }
task-auto-stopped-focus = (auto-stopped focus session #{ $sid })
task-auto-stopped-timer = (auto-stopped timer #{ $sid })
task-completed = Completed task #{ $sid } — { $title }
task-cancelled = Cancelled task #{ $sid } — { $title }
task-deleted = Deleted task #{ $sid }
task-not-found = task #{ $sid } not found
task-add-cancelled-by-hook = Add cancelled by hook
task-created = Created task #{ $sid } — { $title }
task-scheduled = Scheduled task #{ $sid } for { $slot } — { $title }
task-unscheduled = Cleared schedule for task #{ $sid } — { $title }
schedule-unparseable = could not understand schedule slot '{ $input }'

# Checklist items
checklist-added = Added checklist item to task #{ $sid } — { $title }
checklist-checked = Checked checklist item on task #{ $sid } — { $title }
checklist-unchecked = Unchecked checklist item on task #{ $sid } — { $title }
checklist-removed = Removed checklist item from task #{ $sid } — { $title }
checklist-reordered = Moved checklist item { $from } → { $to }
checklist-empty = Task #{ $sid } has no checklist items
checklist-header = Checklist for task #{ $sid } ({ $done }/{ $total })

# Undo / redo
undo-done = Undid: { $action }
redo-done = Redid: { $action }
undo-empty = Nothing to undo
redo-empty = Nothing to redo
task-urgency-header = Urgency for task #{ $sid } — { $title }
# Technical labels for urgency breakdown table.
task-urgency-weight-label = weight
task-urgency-factor-label = factor
task-urgency-contribution-label = contribution
task-urgency-total-label = total
task-count = { $count } { $count ->
    [one] task
   *[other] tasks
}
task-show-recurs = Recurs:   { $value }
task-show-depends = Depends:  { $value }
task-show-blocking = Blocking: { $value }

## TUI (interactive terminal interface)

# Title of the left-hand sidebar pane listing smart views and projects.
tui-sidebar-title = Views / Projects
# Title bar of the detail pane when showing task details.
tui-pane-detail = Detail
# Title bar of the detail pane when showing the keyboard-shortcut help overlay.
tui-pane-help = Help
# Shown in the detail pane when no task is selected.
tui-no-task-selected = No task selected

# Help overlay — header and section titles.
tui-help-title = Keyboard Shortcuts
tui-help-section-navigation = Navigation
tui-help-section-actions = Actions
tui-help-section-general = General

# Help overlay — descriptions for each shortcut (the key glyphs stay fixed).
tui-help-switch-panes = Switch panes
tui-help-jump-pane = Jump to pane
tui-help-move-down = Move down
tui-help-move-up = Move up
tui-help-go-top = Go to top
tui-help-go-bottom = Go to bottom
tui-help-select-expand = Select / expand
tui-help-mark-done = Mark task done
tui-help-delete-task = Delete task
tui-help-refresh = Refresh
tui-help-toggle-help = Toggle this help
tui-help-quit = Quit

# Detail pane field labels.
tui-detail-status = Status
tui-detail-urgency = Urgency
tui-detail-project = Project
tui-detail-priority = Priority
tui-detail-start = Start
tui-detail-deadline = Deadline
# Marker shown when a task is flagged for the evening.
tui-detail-evening = Evening: yes
tui-detail-tags = Tags
tui-detail-notes = Notes
tui-detail-checklist = Checklist

# Bottom status bar default hint (compact key legend).
tui-status-hint = Tab/←→: panes · j/k: move · Enter: select · d: done · x: delete · r: refresh · ?: help · q: quit
# Status bar message after loading a view's tasks.
tui-loaded-tasks = Loaded { $count } { $count ->
    [one] task
   *[other] tasks
} from { $source }
# Status bar message when opening a task's detail view.
tui-viewing-task = Viewing task #{ $sid }
# Status bar message after completing a task.
tui-completed-task = Completed task #{ $sid } — { $title }
# Status bar message after deleting a task.
tui-deleted-task = Deleted task #{ $sid }

# Clap help text (command/argument descriptions for --help output).
# Keys are auto-derived from the command path: help-cli[-<subcommand>...]-about
# and help-cli-<path>-arg-<arg-id>. Regenerate the English block with:
#   cargo test -p tock-cli generate_clap_help_catalog -- --ignored --nocapture

# help-cli
help-cli-about = Command-line interface for tock — unified task / habit / time / focus engine.
help-cli-arg-vault = Path to the vault file
help-cli-arg-password = Vault password (reads from `TOCK_PASSWORD` env var; prompts if absent)
help-cli-arg-log_format = Log format: human (default) or json
help-cli-arg-format = Output format: table (default), compact, json
help-cli-arg-lang = Language for messages (BCP-47, e.g. `en-US`); overrides `TOCK_LANG`

# help-cli-add
help-cli-add-about = Add a new task. Supports sigils: #tag, !H/M/L, due:YYYY-MM-DD
help-cli-add-arg-recur = Recurrence: daily, weekly, monthly, yearly, every-3d, every-2w
help-cli-add-arg-words = Task description (may include sigils)

# help-cli-modify
help-cli-modify-about = Modify an existing task
help-cli-modify-arg-sid = Task SID to modify
help-cli-modify-arg-args = Fields and values to change (e.g. title:"New title" !M #newtag)

# help-cli-done
help-cli-done-about = Mark task(s) as done
help-cli-done-arg-sids = Task SID(s)

# help-cli-cancel
help-cli-cancel-about = Cancel task(s)
help-cli-cancel-arg-sids = Task SID(s)

# help-cli-delete
help-cli-delete-about = Soft-delete task(s)
help-cli-delete-arg-sids = Task SID(s)

# help-cli-depend
help-cli-depend-about = Add a dependency: task <sid> depends on <dep-sid>
help-cli-depend-arg-sid = Task SID
help-cli-depend-arg-on = Dependency SID

# help-cli-undepend
help-cli-undepend-about = Remove a dependency
help-cli-undepend-arg-sid = Task SID
help-cli-undepend-arg-from = Dependency SID

# help-cli-schedule
help-cli-schedule-about = Schedule a task for a calendar slot (day or day+time you plan to work on it)
help-cli-schedule-arg-sid = Task SID
help-cli-schedule-arg-when = When to schedule it (e.g. tomorrow, friday 9am, 2026-06-01T14:30)

# help-cli-unschedule
help-cli-unschedule-about = Clear a task's scheduled slot
help-cli-unschedule-arg-sid = Task SID

# help-cli-list
help-cli-list-about = List tasks
help-cli-list-arg-filter = Filter expression (e.g. status:pending, project:myproj)
help-cli-list-arg-json = Output as JSON

# help-cli-show
help-cli-show-about = Show task details
help-cli-show-arg-sid = Task SID
help-cli-show-arg-json = Output as JSON

# help-cli-urgency
help-cli-urgency-about = Explain urgency score for a task
help-cli-urgency-arg-sid = Task SID

# help-cli-project
help-cli-project-about = Project management

# help-cli-project-add
help-cli-project-add-about = Create a new project
help-cli-project-add-arg-name = Project name

# help-cli-project-list
help-cli-project-list-about = List projects
help-cli-project-list-arg-all = Include archived projects

# help-cli-project-archive
help-cli-project-archive-about = Archive a project
help-cli-project-archive-arg-sid = Project SID

# help-cli-area
help-cli-area-about = Area management

# help-cli-area-add
help-cli-area-add-about = Create a new area
help-cli-area-add-arg-name = Area name

# help-cli-area-list
help-cli-area-list-about = List areas
help-cli-area-list-arg-all = Include archived areas

# help-cli-tag
help-cli-tag-about = Tag management

# help-cli-tag-list
help-cli-tag-list-about = List all tags

# help-cli-tag-rename
help-cli-tag-rename-about = Rename a tag (updates all tagged entities)
help-cli-tag-rename-arg-old = Current tag name
help-cli-tag-rename-arg-new = New tag name

# help-cli-report
help-cli-report-about = Saved custom reports

# help-cli-report-define
help-cli-report-define-about = Define a new report
help-cli-report-define-arg-name = Report name
help-cli-report-define-arg-query = Filter query (for example `status:pending tag:work`)
help-cli-report-define-arg-sort = Sort by field (`urgency`, `deadline`, `created`, `sid`)
help-cli-report-define-arg-columns = Columns to display (`sid,priority,title,deadline,tags`)

# help-cli-report-list
help-cli-report-list-about = List all saved reports

# help-cli-report-show
help-cli-report-show-about = Run a saved report
help-cli-report-show-arg-name = Report name
help-cli-report-show-arg-json = Output as JSON

# help-cli-report-rm
help-cli-report-rm-about = Delete a report
help-cli-report-rm-arg-name = Report name

# help-cli-context
help-cli-context-about = Named filter contexts

# help-cli-context-set
help-cli-context-set-about = Activate a context
help-cli-context-set-arg-name = Context name

# help-cli-context-clear
help-cli-context-clear-about = Clear the active context

# help-cli-context-define
help-cli-context-define-about = Define a new context
help-cli-context-define-arg-name = Context name
help-cli-context-define-arg-filter = Filter expression

# help-cli-context-list
help-cli-context-list-about = List all contexts

# help-cli-context-rm
help-cli-context-rm-about = Delete a context
help-cli-context-rm-arg-name = Context name

# help-cli-view
help-cli-view-about = Show a built-in view (inbox, today, upcoming, anytime, someday, logbook)
help-cli-view-arg-name = View name
help-cli-view-arg-json = Output as JSON

# help-cli-time
help-cli-time-about = Time tracking

# help-cli-time-start
help-cli-time-start-about = Start timing. Creates a block immediately
help-cli-time-start-arg-words = Description words, or a task SID followed by optional text

# help-cli-time-stop
help-cli-time-stop-about = Stop the current timer

# help-cli-time-resume
help-cli-time-resume-about = Resume the most recently stopped timer

# help-cli-time-current
help-cli-time-current-about = Show the currently running timer

# help-cli-time-blocks
help-cli-time-blocks-about = List time blocks
help-cli-time-blocks-arg-period = Period: today, week, month, all
help-cli-time-blocks-arg-json = Output as JSON

# help-cli-time-report
help-cli-time-report-about = Time report summary
help-cli-time-report-arg-period = Period: today, week, month
help-cli-time-report-arg-json = Output as JSON

# help-cli-time-edit
help-cli-time-edit-about = Edit a time block
help-cli-time-edit-arg-sid = Time block SID
help-cli-time-edit-arg-title = New title
help-cli-time-edit-arg-notes = New notes
help-cli-time-edit-arg-start = New start time (ISO 8601)
help-cli-time-edit-arg-end = New end time (ISO 8601)
help-cli-time-edit-arg-task = Link to task SID
help-cli-time-edit-arg-billable = Mark as billable

# help-cli-focus
help-cli-focus-about = Focus timer

# help-cli-focus-start
help-cli-focus-start-about = Start a focus session (default: 4 cycles of 25/5/15)
help-cli-focus-start-arg-task = Task SID to focus on (optional)
help-cli-focus-start-arg-cycles = Number of Pomodoro cycles (default: 4)
help-cli-focus-start-arg-work = Work interval in minutes (default: 25)
help-cli-focus-start-arg-short_break = Short break in minutes (default: 5)
help-cli-focus-start-arg-long_break = Long break in minutes (default: 15)

# help-cli-focus-done
help-cli-focus-done-about = Complete the current work interval (mark a pomodoro done)

# help-cli-focus-skip-break
help-cli-focus-skip-break-about = Skip the current break and start working

# help-cli-focus-pause
help-cli-focus-pause-about = Pause the current session

# help-cli-focus-resume
help-cli-focus-resume-about = Resume a paused session

# help-cli-focus-stop
help-cli-focus-stop-about = Abort the current session

# help-cli-focus-status
help-cli-focus-status-about = Show the current focus session status

# help-cli-focus-stats
help-cli-focus-stats-about = Show focus stats for a period
help-cli-focus-stats-arg-period = Period: today, week, month (default: today)

# help-cli-focus-history
help-cli-focus-history-about = Show focus session history for a task
help-cli-focus-history-arg-task = Task SID

# help-cli-habit
help-cli-habit-about = Habit tracking

# help-cli-habit-add
help-cli-habit-add-about = Add a new habit
help-cli-habit-add-arg-title = Habit title
help-cli-habit-add-arg-identity = Identity statement (e.g. "I am a reader")
help-cli-habit-add-arg-cue = Cue (e.g. "After morning coffee")
help-cli-habit-add-arg-response = Response (e.g. "Read 10 pages")
help-cli-habit-add-arg-reward = Reward (e.g. "Enjoy tea")
help-cli-habit-add-arg-direction = Direction: build (default) or break
help-cli-habit-add-arg-cadence = Cadence: daily (default), or JSON
help-cli-habit-add-arg-minimum = Minimum: "boolean" (default), or JSON
help-cli-habit-add-arg-stack_after = Stack after habit SID
help-cli-habit-add-arg-stack_delay = Delay in seconds after stacked habit completes

# help-cli-habit-list
help-cli-habit-list-about = List habits
help-cli-habit-list-arg-all = Include archived habits

# help-cli-habit-show
help-cli-habit-show-about = Show habit details and status
help-cli-habit-show-arg-sid = Habit SID

# help-cli-habit-log
help-cli-habit-log-about = Log a habit completion
help-cli-habit-log-arg-sid = Habit SID
help-cli-habit-log-arg-amount = Amount (e.g. "10", "true", "15min"). Default: "true"
help-cli-habit-log-arg-notes = Notes
help-cli-habit-log-arg-slip = Mark as a slip (for break habits)

# help-cli-habit-skip
help-cli-habit-skip-about = Skip a habit for a date without breaking the streak
help-cli-habit-skip-arg-sid = Habit SID
help-cli-habit-skip-arg-date = Date to skip in ISO format. Defaults to today
help-cli-habit-skip-arg-reason = Reason for skipping

# help-cli-habit-freeze
help-cli-habit-freeze-about = Freeze a habit for a date
help-cli-habit-freeze-arg-sid = Habit SID
help-cli-habit-freeze-arg-date = Date to freeze in ISO format. Defaults to today

# help-cli-habit-backfill
help-cli-habit-backfill-about = Log a habit for a past date
help-cli-habit-backfill-arg-sid = Habit SID
help-cli-habit-backfill-arg-date = Date in ISO `YYYY-MM-DD` format
help-cli-habit-backfill-arg-amount = Amount

# help-cli-habit-streaks
help-cli-habit-streaks-about = Show streak history for a habit
help-cli-habit-streaks-arg-sid = Habit SID

# help-cli-habit-modify
help-cli-habit-modify-about = Modify a habit
help-cli-habit-modify-arg-sid = Habit SID
help-cli-habit-modify-arg-title = New title
help-cli-habit-modify-arg-identity = New identity statement
help-cli-habit-modify-arg-cue = New cue
help-cli-habit-modify-arg-response = New response
help-cli-habit-modify-arg-reward = New reward
help-cli-habit-modify-arg-stack_after = Stack after habit SID (0 to clear)

# help-cli-habit-archive
help-cli-habit-archive-about = Archive a habit
help-cli-habit-archive-arg-sid = Habit SID

# help-cli-habit-status
help-cli-habit-status-about = Show habit status overview

# help-cli-habit-slip
help-cli-habit-slip-about = Log a slip for a break-bad-habit (convenience for `log --slip`)
help-cli-habit-slip-arg-sid = Habit SID
help-cli-habit-slip-arg-notes = Notes about the slip

# help-cli-habit-remind
help-cli-habit-remind-about = Add or manage reminders for a habit
help-cli-habit-remind-arg-sid = Habit SID
help-cli-habit-remind-arg-at = Time of day (HH:MM, e.g. "07:00"). Adds a reminder
help-cli-habit-remind-arg-days = Days this reminder applies to (comma-separated, e.g. "monday,wednesday")
help-cli-habit-remind-arg-clear = Clear all reminders for this habit
help-cli-habit-remind-arg-list = List current reminders

# help-cli-hooks
help-cli-hooks-about = Hook script management

# help-cli-hooks-list
help-cli-hooks-list-about = List installed hook scripts

# help-cli-hooks-path
help-cli-hooks-path-about = Show the hooks directory path

# help-cli-uda
help-cli-uda-about = User-defined attribute management

# help-cli-uda-add
help-cli-uda-add-about = Define a new UDA
help-cli-uda-add-arg-key = Attribute key name
help-cli-uda-add-arg-type = Type: string, number, date, boolean
help-cli-uda-add-arg-label = Human-readable label
help-cli-uda-add-arg-default = Default value

# help-cli-uda-list
help-cli-uda-list-about = List all defined UDAs

# help-cli-uda-rm
help-cli-uda-rm-about = Remove a UDA definition
help-cli-uda-rm-arg-key = Key to remove

# help-cli-views
help-cli-views-about = List available views

# help-cli-agenda
help-cli-agenda-about = Show the day's agenda: scheduled tasks interleaved with time blocks
help-cli-agenda-arg-when = Day to show (natural-language date). Defaults to today

# help-cli-tui
help-cli-tui-about = Launch the interactive terminal user interface

# help-cli-completions
help-cli-completions-about = Generate shell completion scripts
help-cli-completions-arg-shell = Shell to generate completions for: bash, zsh, fish, elvish, powershell

# help-cli-caldav
help-cli-caldav-about = `CalDAV` bidirectional sync

# help-cli-caldav-setup
help-cli-caldav-setup-about = Set up a `CalDAV` collection for syncing
help-cli-caldav-setup-arg-url = `CalDAV` server base URL (e.g. <https://cal.example.com/dav/>)
help-cli-caldav-setup-arg-user = Username for authentication
help-cli-caldav-setup-arg-collection = Collection URL (full path to the calendar collection)
help-cli-caldav-setup-arg-name = Display name for this collection
help-cli-caldav-setup-arg-password_stdin = Read password from stdin instead of prompting

# help-cli-caldav-sync
help-cli-caldav-sync-about = Sync with a configured `CalDAV` collection
help-cli-caldav-sync-arg-collection = Collection URL to sync (syncs all if omitted)
help-cli-caldav-sync-arg-dry_run = Dry run — show what would change without applying

# help-cli-caldav-status
help-cli-caldav-status-about = Show `CalDAV` sync status and configured collections

# help-cli-caldav-remove
help-cli-caldav-remove-about = Remove a `CalDAV` collection configuration and unlink all resources
help-cli-caldav-remove-arg-url = Collection URL to remove

# help-cli-export
help-cli-export-about = Export data to a file
help-cli-export-arg-format = Format: 'json' or 'md'
help-cli-export-arg-out = Output file path (stdout if omitted)
help-cli-export-arg-builtin = Built-in Markdown template: task-list, habit-report, time-report
help-cli-export-arg-template = Path to a custom Tera template file (Markdown export only)
help-cli-export-arg-filter = Task filter expressions (Markdown export only)

# help-cli-import
help-cli-import-about = Import data from a file
help-cli-import-arg-format = Format: 'json', 'taskwarrior', 'things3', or 'csv'
help-cli-import-arg-file = Input file path
help-cli-import-arg-map = Column mapping TOML file (CSV only)
help-cli-import-arg-include-trash = Include trashed items (Things 3 only)
