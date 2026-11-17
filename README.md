# plan

Schedule planning for small teams and their AI assistants. Describe a project as epics, stories and tasks with dependencies and estimates; `plan` computes several scenarios × utilizations × estimates, shows the critical path, float and risk, and lets you move things around and see the consequences immediately.

- **One calculation, two places.** The scheduler is Rust. The CLI runs it natively; the browser runs the same code as WebAssembly, so dragging a bar re-plans instantly.
- **AI friendly.** Every command speaks JSON (`--json`), edits are batched and atomic (`plan apply`), and every change is logged with who made it.
- **Plans only.** `plan` does not track status. Keep your tickets in your issue tracker and link them from task comments.

The method follows Juval Löwy, *Righting Software*, part 2 (project design): activity network, critical path, float-based coloring, multiple design options and risk metrics.

## Quick start (local)

```sh
./build.sh                        # types (Rust → TS) → WASM → TypeScript → CLI
P=target/release/plan
$P import examples/sample.toml --key demo
$P serve                          # opens the UI in the browser (a free port; --port N to choose)
$P show demo --json
```

Requirements: Rust (with the `wasm32-unknown-unknown` target), `wasm-bindgen-cli` 0.2.100, Node.js (for `tsc`).

The CLI embeds the UI (`web/index.html`, `web/ui/*.js`) and the WASM at compile time, so after changing anything under `web/` or `crates/plan-contracts`, run `./build.sh` again; `cargo build` alone keeps the old UI.

## Commands

```sh
plan epics                                   # list epics
plan epic add KEY "Name" 2026-12-01          # new epic
plan import F.toml [--key KEY]               # import a TOML plan (replaces the same key)
plan dump KEY                                # the plan as TOML (re-importable)
plan calc KEY                                # compare all scenarios × utilizations × estimates
plan show KEY [-s scenario -r 0.8 -e e]      # one schedule, task by task
plan why KEY TASK                            # what drives a task's start
plan log [KEY]                               # change history (one line per change)
plan task add KEY T1 name=… effort=1,2,4 after=A,B story=S1
plan task set KEY T1 effort=2 [-s scenario]  # -s: change only that scenario
plan task rm KEY T1
plan dep add KEY TASK PRED | dep rm KEY TASK PRED   # loops are refused
plan story set KEY S1 name=… purpose=… dod=…
plan scenario clone KEY FROM|- NEWKEY "Name" | scenario rm KEY SC
plan apply KEY ops.json                      # apply a list of edits at once; any error writes nothing ("-" for stdin)
plan export KEY out.html                     # a single self-contained HTML page (interactive, read-only)
plan serve [--port N] [--no-open]            # the UI and its HTTP API
plan push [KEY]                              # copy epics from the local SQLite to the server
plan load [PERSON]                           # people across epics: each epic's default run added up per day, days over 100%
plan mcp                                     # an MCP server on stdin/stdout for AI assistants
```

Every command accepts `--json` and returns `{"ok":true,"data":…}` or `{"ok":false,"issues":[…]}`.
`check`, `calc`, `show`, `why`, `json` and `export` also accept a path to a `.toml` file instead of an epic key.

### Edits for `plan apply`

```json
[
  {"op": "set_story", "story": {"key": "S1", "name": "Investigate", "purpose": "", "dod": ""}},
  {"op": "add_task", "task": {"id": "T1", "name": "…", "story": "S1", "deliverable": "…", "effort": [1, 2, 4], "after": ["T0"]}},
  {"op": "set_task", "key": "T1", "patch": {"effort": 2, "not_before": "2026-12-01"}},
  {"op": "link", "task": "T2", "pred": "T1"},
  {"op": "unlink", "task": "T2", "pred": "T1"},
  {"op": "delete_task", "key": "T3"},
  {"op": "clone_scenario", "from": "plan1", "key": "plan2", "name": "Plan 2"},
  {"op": "set_epic", "settings": {"deadline": "2026-12-22", "deadline_task": "CUT"}},
  {"op": "add_comment", "target": "T1", "text": "Ticket: https://example.com/issues/42"},
  {"op": "delete_comment", "id": 3}
]
```

The result says how the edits moved the schedule (`impact`): the default run (first scenario, first utilization, `e`) `before` and `after` — date compared to the deadline (`target`), `finish`, `slack_to_deadline` and `critical_path` — plus `changed` and `other_runs_moved` (every other scenario × utilization × estimate whose dates moved, as `[before, after]`).

```
$ plan task set demo TF effort=20
applied 1 edit(s)
plan1 · 1.0 · e: target 2026-12-17 → 2027-01-20, finish 2026-12-22 → 2027-01-22, slack 3 → -16
  26 other run(s) moved (plan calc)
```

### MCP

`plan mcp` speaks the [Model Context Protocol](https://modelcontextprotocol.io) over stdin/stdout. It uses the same store as the CLI (the server and `token_cmd` in `~/.config/plan/config`, otherwise the local SQLite) and returns the same data as `--json`.

| Tool | Same as |
|---|---|
| `epics`, `dump`, `show`, `calc`, `why`, `check`, `log`, `load` | the commands of the same name (read only) |
| `apply` | `plan apply` (all edits or none). The history records them as `mcp` |
| `delete_epic` | `plan epic rm` (its content stays in the history) |

```sh
claude mcp add plan --scope user -- /path/to/plan mcp     # Claude Code
```

The types of every edit and every result are defined once in `crates/plan-contracts` and exported to TypeScript (`web/src/contracts.d.ts`).

## Where plans live

| Store | When | Notes |
|---|---|---|
| SQLite | default for the CLI and `plan serve` | `data/plan.db` in the repository (the binary looks two directories up), or `PLAN_DB=path` |
| Firestore | `plan serve` with `PLAN_STORE=firestore:PROJECT` | one document per epic (`epics/{key}`), one per change (`history/{id}`), `meta/state` holds the version |
| A server | the CLI, when `~/.config/plan/config` has `server=URL` | the CLI talks to the server's HTTP API; add `token_cmd=…` to print a bearer token |

All stores apply edits the same way: read → apply the edits → validate (no dependency loops, known tasks) → write the plan and the history in one transaction.

## UI

| Action | Mouse | Keys |
|---|---|---|
| Switch view | buttons at the top right | 1 Gantt · 2 Network · 3 Load · 4 Table · 5 Compare |
| Select a task | click a bar, arrow or row | ↑ ↓ |
| Move the start | drag the bar | ← → (half a day) |
| Change the duration | drag the right edge | Shift + ← → |
| Undo / redo | ↶ ↷ | ⌘Z · ⇧⌘Z |
| Save the moves | Save | ⌘S |
| Add a task / story | + Task · + Story (asks for a name first) | N |
| Link tasks | drag from the circle right of the selected bar onto the next task | — |
| Comment on a task or story | the Comments box in the side panel (URLs become links) | Enter |

Moves are re-planned at once; the original position stays as a dashed outline. **Save** writes `not_before` and the new effort or wait to the current scenario.

## How the schedule is computed

| Topic | Rule |
|---|---|
| Unit | Business half-days. Weekends, Japanese public holidays, Dec 29 – Jan 3 and `calendar.off` are skipped |
| Duration | Work: effort ÷ utilization ÷ owners, rounded up to half a day. Wait: business days as written, no one's hands |
| Estimates | `[optimistic, most likely, pessimistic]`; expected = PERT (o + 4m + p) / 6, rounded to 0.5 |
| People | One task at a time per person. Two owners progress only when both are free. Days off are skipped |
| Order | Least float first (latest start of the resource-free critical-path pass), back-filling gaps |
| Across epics | Each epic is scheduled on its own. `plan load`, the MCP tool `load` and the Load view add up every epic's default run (first scenario, first utilization, `e`) per person and day; above one person's whole day is over. Nothing is moved automatically |
| Resources | Things a task needs besides people (`needs`): an environment, a room, an approval board. A task is not placed in the off periods of what it needs and does not straddle them (a release freeze is `resources.prod`). Unlike people, any number of tasks can use a resource at once |
| Float | The largest delay of one task, from where it is placed, that does not move the end (owners' availability and resources included). Free float: without moving the next task |
| Colors | Float 0 = critical (orange); ≤ `colors.red` days low, ≤ `colors.yellow` mid, above high (blue, dark to light) |
| Risk | Criticality (weights 4·3·2·1), Fibonacci, activity (1 − ΣF / (M·N)). Healthy between 0.3 and 0.75 |
| Holidays | Computed from the rules of the Japanese holiday law (equinox approximation valid for 1980–2099) |

The Help page in the UI shows the formulas with references.

## Plan files (TOML)

`plan new FILE` writes a commented template (`examples/template.toml`); `examples/sample.toml` is a complete example.

| Place | Keys | Meaning |
|---|---|---|
| top | `title`, `description` | heading |
| | `start`, `start_half` | start date, `am` or `pm` |
| | `deadline`, `deadline_task`, `deadline_label` | deadline and the task compared with it |
| | `rates`, `estimates`, `round` | utilizations, estimate kinds (o, m, e, p), rounding |
| | `default_owner` | owner of tasks without one |
| | `owners` | who is responsible for the epic (several allowed; shown in the epic list). Tasks keep their own owners |
| | `max_task_days` | tasks with a larger expected effort (person-days) get a warning to split them; default 2, `0` turns it off |
| `[calendar]` | `holidays`, `year_end`, `off` | calendar |
| `[colors]` | `red`, `yellow` | float thresholds (days) |
| `[people.NAME]` | `leave`, `rate` | days off, a fixed utilization for this person |
| `[resources.NAME]` | `label`, `off` | what the off periods are called, `[[first, last], …]` when it cannot be used |
| `[[tasks]]` | `id`, `name`, `short`, `ticket`, `deliverable`, `group`, `note` | identity and labels |
| | `effort` or `wait` | person-days or business days; a number or a triple. Neither = milestone |
| | `owner`, `after` | owner(s), predecessors |
| | `needs`, `not_before`, `fixed`, `done`, `actual_start`, `actual_finish` | resources it needs, earliest start, fixed date, done, actual dates (drawn as grey bars; marking a task done without `actual_finish` records today) |

Older plans with `prod = true` and `calendar.freeze` still read: they become `needs = ["prod"]` and `[resources.prod]` (label "Release freeze"), and are written back in the new form.
| `[scenarios.KEY]` | `name`, `note`, `drop`, `set.ID`, `add` | alternative plans as differences from the base |

## Deploy to Cloud Run

The image builds everything from source (`Dockerfile`). `.github/workflows/ci.yml` runs the tests on every push and pull request, and on `main` builds the image, pushes it to Artifact Registry and deploys it to Cloud Run, authenticating with Workload Identity Federation (no key files).

Repository variables used by the workflow: `GCP_PROJECT`, `GCP_REGION`, `GCP_WIF_PROVIDER`, `GCP_DEPLOY_SA`, `CLOUD_RUN_SERVICE`, `ARTIFACT_REPO`.

Put Identity-Aware Proxy in front of the service to limit access to your Google account. In a project without an organization, the IAP OAuth client must be created once in the console; `deploy/iap-set-client.sh` registers it. For the CLI, impersonate a service account that has `roles/iap.httpsResourceAccessor` and use the IAP OAuth client ID as the audience:

```
server=https://YOUR-SERVICE.run.app
token_cmd=gcloud auth print-identity-token --impersonate-service-account=CLI_SA --audiences=IAP_CLIENT_ID --include-email
```

## Tests

`cargo test --release` checks, among others, that `examples/sample.toml` gives the same start, finish, float, free float and risk for all 27 runs as the original Python implementation (`tests/fixtures/sample.expected.json`), and that holidays for 2025–2030 match the Python `holidays` package.

## Layout

The code is split by volatility following *The Method* (Righting Software, part 1). See [docs/design.md](docs/design.md).

| Path | Layer | Role |
|---|---|---|
| `crates/plan-contracts` | contracts | types shared by every layer (exported to TypeScript with ts-rs) |
| `crates/plan-engines` | Engine | scenarios, estimation, scheduling, analysis. No I/O |
| `crates/plan-access` | ResourceAccess | plans (SQLite, Firestore, HTTP, TOML, JSON) and the calendar |
| `crates/plan-manager` | Manager | evaluate, what-if, edit |
| `crates/plan-cli` | Client | the `plan` command, the HTTP server (embeds the UI and the WASM) and the MCP server |
| `crates/plan-web` | Client | the WASM entry point used by the UI |
| `web/` | Client | the UI in TypeScript without a framework (`src/` → `ui/` by `tsc`) |

## License

MIT
