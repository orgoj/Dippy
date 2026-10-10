# Dashboard in dippy-rs, approvals through it (unattended cloud task)

Brief for one autonomous overnight session; nobody can start a second one.
Read it fully, then work through the steps in order without asking
questions. Read `AGENTS.md`, `rust/PORTING.md` and
`docs/plans/rust-port-report.md` first.

## Goal

1. Port `dippy dashboard` (node and hub, `src/dippy/dashboard.py`,
   `src/dippy/dashboard_static/`, contract `docs/dashboard-api.json`) to
   `dippy-rs`. After this, nothing in Dippy needs Python.
2. New setting `set approval ask|dashboard` (default `ask`). With
   `dashboard`, an `ask` of an agent hook (Claude, AGY, Gemini, Cursor,
   Codex, pi) or of `dippy run`/`run-on-server` goes to a queue on the local
   dashboard node; the user decides it in the web UI and Dippy answers the
   agent only with allow or deny. With `ask` everything behaves exactly as
   today (the agent's own prompt, or the askpass program where one is used
   today, including the `dippy-askpass` GUI).
3. The web UI shows the queue of pending requests; clicking one opens a
   popup (modal) dialog laid out like the `dippy-askpass` dialog.

Dippy catches mistakes of non-adversarial agents; it is not a security
boundary. Approval keys and protection against an agent approving itself
are out of scope (user decision): the existing dashboard token is enough.

## Non-negotiable invariants

- Fail closed: an `ask` never becomes `allow` without a user decision.
- `set approval ask` (and no setting) stays byte-identical to today: hook
  output, `dippy run` output, audit entries and the payloads sent to
  askpass programs (the unversioned hook payload in `hook.rs`, version 1 in
  `run.rs`) do not change. Every existing test and parity tool must give
  the same results after every commit. Add a test per hook mode that
  proves it. New payloads and messages apply only to dashboard waits.
- `pass` is not `ask`: an unmatched tool that Dippy leaves to the agent
  (`hook.rs` `pass`, Claude/Cursor/Codex) keeps the agent's native flow and
  never enters the queue. A config error keeps today's behaviour (no config
  means no `approval` setting).
- Reuse, never duplicate, decision logic: the web rule editor calls
  `askpass::check`, `askpass::statuses`, `askpass::decide_cause`,
  `askpass::rule_line` and `admin::add_rule` through REST.
- New settings follow the existing ones (`set key value`, kebab-case,
  parsed in `config.rs`, listed in `admin::KEYS` for `dippy config get/set`,
  documented in `docs/config.md`) and merge by membership in
  `configured_settings`, so a higher scope can restore the default. An
  unknown `approval` value is a config error.
- Never read or write the real `~/.dippy`, never start a GUI, a browser or
  a real agent; tests use a temporary `HOME`, port 0 and fake programs.
  Never run the real `tmux`/`ssh`/`herdr`.
- Every `feat` follows the repository workflow: `just bump-version X.Y.Z`
  first (patch increments), CHANGELOG section, `just check`, `just test`,
  conventional commit with the `Co-Authored-By` trailer. Commit and push
  the working branch after every step below, so an interrupted session
  leaves finished, working steps. Never force-push, never touch `main` or
  `orgoj`.

## Order, priorities and stop rules

Do the steps in this order. Each step ends green and committed before the
next starts. If a step cannot be made green after three serious attempts,
revert its uncommitted changes, record the blocker in the report, skip the
steps that depend on it (below) and continue with the next independent one.

Dependencies: 2 needs nothing; 3 needs 2; 4 needs 2 and 3; 5 needs 3;
6 needs 4; 7 needs 1, 3 and 5; 8 and 9 always run. Step 1 failing does not
block 2-6 (they extend the Rust node, which then serves only the new
routes and the UI).

Time gates: the required morning result is steps 2-5 (settings, node
queue, Claude/AGY/`run` client, node UI) working end to end. Reserve the
last 20 % of the session, at least one hour, for steps 8-9 and the full
verification (`just check`, `just test`, every parity tool); stop feature
work when that window starts. When behind schedule, drop step 7 (hub) first,
then the less used agents of step 6 (pi, then Cursor), never steps 8-9.

1. Port of the existing dashboard (parity).
2. Settings and the shared approval payload.
3. Node approval queue (REST).
4. Hook client for Claude, AGY and `dippy run`/`run-on-server`.
5. Web UI: queue and popup dialog (node mode).
6. Hook client for Gemini, Cursor, Codex and pi.
7. Hub: aggregated queue and forwarded answers.
8. Docs.
9. Report.

## Step 1 - port the existing dashboard (parity)

- `dippy-rs dashboard [--host] [--port] [--token-file] [--hub NODES_FILE]`
  with the Python routes, status codes, JSON bodies, token handling (file
  0600, `X-Dippy-Token`), node registry (`GET/PUT/DELETE /api/nodes`,
  atomic 0600 writes, hand edits picked up) and allowlisted proxy
  (`/n/{name}/api/entries`), with the Python opener's rules: 5 s timeout,
  no redirects followed, no environment proxy, response size cap.
- Python accepts `https` node URLs. Add one well-maintained TLS client
  crate (justify it in the report) or, if that is not possible, reject
  `https` with a clear error and record it as an intentional divergence.
- Keep the HTTP server small: std `TcpListener` plus threads or one small,
  well-maintained crate; no async runtime unless unavoidable. Justify every
  new dependency in the report.
- Static UI embedded in the binary (`include_str!`), unchanged until step 5.
- Parity script `rust/parity/dashboard_compare.py` in the style of
  `hooks_compare.py`: start the Python and the Rust node/hub on port 0 in a
  sandbox HOME with a seeded audit log, run the same requests, compare
  status and body (normalize timestamps and ports). It covers the legacy
  API only (`/api/entries`, `/api/nodes`, `/n/{name}/api/entries`, token
  and error handling), never static assets or approval routes, so it stays
  at 0 differences after step 5 changes the UI. New routes are verified by
  Rust tests and the contract test.

## Step 2 - settings and the shared approval payload

Settings (user and project scope unless noted):

- `set approval ask|dashboard`.
- `set dashboard-url URL`: **user config only** (a project `.dippy` setting
  it is a config warning and ignored), so a cloned repository cannot send
  requests to a server that approves everything. Default
  `http://127.0.0.1:<the node's default port>`.
- `set dashboard-token-file PATH`: **user config only** like
  `dashboard-url`; default `~/.dippy/dashboard-token`, the node's default
  `--token-file`. The client reads the token from it; a missing or
  unreadable file means "not accepted" (fallback, step 4).
- `set approval-timeout-message TEXT`: like `approval-wait-message`, only
  the final instruction is configurable and an empty value is rejected. The
  fixed, neutral part states that approval timed out after N seconds and
  that the requested command was not executed (see `docs/config.md`,
  "Runtime messages deliberately use neutral terms"). Default instruction:
  wait for the user and retry the same request, or continue with another
  safe solution. Used only for dashboard waits (hooks and `dippy run`);
  native askpass timeouts keep today's texts.

Payload: one versioned schema (`"version": 2`) built by one function and
used only for dashboard requests (native askpass payloads stay unchanged):
`id` (client-generated, see step 4), `command`, `cwd` (mandatory, absolute),
`tool`, `file_path`, `message`, `agent`, `server`, `decision`, `reason`,
`context_flags`, `flags`, `remote`, `causes` (with `flags`), `deadline`
(Unix time in milliseconds, UTC). The node parses it without environment
fallback (no `DIPPY_*`, no current directory): a missing or relative `cwd`,
an unknown field or a wrong type is a 400.

## Step 3 - node approval queue (REST)

The queue lives only in the node process, in memory (pending requests are
lost on restart). All routes require the node token.

Write the contract first: extend `docs/dashboard-api.json` with every route
and schema below before any router code, then a test compares the router
table with the contract so they cannot drift. Fixed representations:

- Item: `{"id", "state", "request" (the v2 payload), "statuses" (current
  decision per cause, recomputed on every read), "created", "deadline",
  "decided" (or null), "note" (or null)}`; `state` is `pending`,
  `allowed`, `denied` or `expired`; times are Unix milliseconds, UTC.
- `note`: string, at most 1000 characters. Unknown fields and wrong types
  are 400. Body limit 64 KiB.

Routes:

- `POST /api/approvals` - v2 payload. 201 with the item; the same `id`
  again returns 200 with the existing item (idempotent; a different body
  under an existing id is 409). A `deadline` in the past, or more than one
  hour ahead, is 400. The node flips the item to `expired` atomically when
  the deadline passes.
- `GET /api/approvals?state=pending|done` - list, newest first; keep the
  last 200 done items (`allowed`, `denied`, `expired`).
- `GET /api/approvals/{id}` - the item; `?wait=SECONDS` long-polls until
  the state leaves `pending` or the wait ends (server cap 10 s), then 200
  with the item in its current state. Unknown id is 404.
- `POST /api/approvals/{id}/decision` - `{"decision": "allow"|"deny",
  "note"}`. 200 with the decided item. First decision wins under a mutex;
  a decision on a decided or expired item returns 409 with the item, and
  the UI shows the final state.
- `POST /api/approvals/{id}/check` - `{"cause": INDEX, "action", "kind",
  "tags", "pattern", "target": "project"|"user"}`; `cause` indexes the
  stored request's causes (null for a request without causes). Returns
  `askpass::check` (line, outcome, broad, can_save).
- `POST /api/approvals/{id}/rule` - same body. The node recomputes the
  check from the stored request and saves exactly the checked line only
  when `Check::can_save`; 409 otherwise. Returns the file and new statuses.
  Saves are serialized in the node (`admin::add_rule` is
  read-modify-write); config is loaded per request, never cached.

Tests: malformed and unknown fields, expired at POST, wait expiry,
duplicate id, simultaneous decisions, check/save identity.

## Step 4 - hook client: Claude, AGY, `dippy run`

Client rules (shared code):

- Budget: a monotonic timer starts at the first instruction of `main`;
  the whole wait (config load, connect, POST, polling) ends at
  `askpass-timeout - 3` seconds, saturating: a timeout of 3 or less
  (including 0 and negative values, which the config accepts today) means
  no queue wait and an immediate deny with the timeout message (document
  it). The payload `deadline` is the wall-clock time of the budget's end.
- The client generates the `id` (random, 32 hex digits) before the POST.
- Connect timeout at most 1 s. Fall back to the `set approval ask`
  behaviour only when the node provably did not accept the request:
  missing token file, connection refused, connect timeout, or an explicit
  401/404 response to the POST. Every other outcome after the request body
  was sent (reset, read timeout, 5xx, malformed response) is indeterminate:
  never fall back and never send a second POST; poll `GET` with the same
  id until the budget ends, then deny with the timeout message (also when
  the node restarted and the id is 404). Test the case "server stores the
  request, then drops the POST response".
- The response of the last long-poll inside the budget is authoritative;
  no request is sent after the budget ends. Only state `allowed` allows;
  `denied`, `expired`, any error and any malformed response deny.
- Audit: the hook keeps its existing `ask` entry; the outcome adds one
  entry (`allow`/`deny`, agent, note or timeout, approval id).
  `hook-approvals.log` follows `log-hook-approvals` as today.
- A user deny carries the user's note to the agent.
- Agent hooks have their own timeouts (Claude cancels the hook and falls
  back to its own prompt; Gemini continues execution on timeout, possibly
  after 10 s by default; see `docs/hook-systems/`). Document for each agent
  the hook timeout to configure (greater than `askpass-timeout`) and what
  happens when it is too short.

Claude: queue where Dippy returns `ask`; reply with the mode's
allow/deny JSON. AGY: the queue replaces the askpass call when
`approval dashboard`. `dippy run`/`run-on-server`: the queue replaces the
askpass call; the `STATUS: Approval required ...` line stays.

## Step 5 - web UI (node mode)

- One JS file for node and hub as today (text via `textContent` only, no
  `innerHTML`). New "Approvals" view first, audit log second.
- Queue of pending requests (agent, cwd, command or tool name, age,
  countdown to the deadline); pending count in the page title; optional
  browser notification.
- Clicking a request opens a popup dialog with the sections of
  `askpass_gui.rs`: REQUEST (whole command, read-only), WHY IT ASKED
  (selectable causes, current decision, rule, context flags), ADD A RULE
  (action, kind, tags prefilled from the cause without `@` flags, pattern,
  project/user target, live check on input, Save rule enabled only when
  allowed), Approve / Deny with an optional note. A request without
  command and causes (e.g. an unmatched tool) shows the tool name and a
  disabled rule editor.
- Misclick protection as in the GUI (`askpass::Arming`,
  `askpass::key_decision`): decision buttons and keys are ignored for 1 s
  after the popup opens or the window regains focus; keyboard approval only
  with Ctrl+Enter, Esc denies.
- A saved rule refreshes statuses; when every cause is allowed by rules
  the UI suggests Approve but never clicks it.

## Step 6 - hook client: Gemini, Cursor, Codex, pi

- Gemini: queue where Dippy asks today. Its hook timeout and "continue on
  timeout" behaviour must be documented with the required setting.
- Cursor: queue where Dippy asks today.
- Codex: PreToolUse allow is silent and does not approve; the real allow
  exists only in `PermissionRequest` (`hook.rs`). Queue only on
  `PermissionRequest`; PreToolUse keeps today's behaviour (a deny still
  blocks there). Document that commands Codex runs without a
  `PermissionRequest` are not queued.
- pi: `dippy --pi` returns `{action: ask}` and the extension
  (`pi-extension/dippy-extension.ts`, in this repository) prompts. With
  `approval dashboard`, `dippy --pi` waits for the queue and returns
  allow/deny; check that the extension waits long enough and has no own
  timeout shorter than the deadline (change it if needed).

## Step 7 - hub

The hub is only an aggregator: no queue of its own. It shows the queues of
all nodes in one list with a node column and forwards the user's answers
(decision, check, rule) to the owning node through its allowlisted proxy,
which now forwards `POST` bodies (64 KiB cap) for exactly the approval
routes. The UI never long-polls through the hub (the 5 s proxy timeout);
it polls the lists.

## Step 8 - docs

`docs/config.md` (settings, payload version 2, queue behaviour, fallback,
timeouts per agent, `dashboard-url` and `dashboard-token-file` user scope),
README fork section, a
systemd user unit example for the node (documented, not installed),
`docs/dashboard-api.json`.

## Step 9 - report

Update `docs/plans/rust-port-report.md`: steps done or skipped with
reasons, parity numbers, new dependencies and why, intentional
divergences, next step (the switch: install Rust as `dippy`, remove the
Python package).

## Out of scope

Approval keys, per-uid isolation, TLS on the server side, networking beyond
`http(s)://host:port`, persistence of pending approvals across node
restarts, changes to Python Dippy (the Python dashboard stays as is; it is
only the parity oracle), `dippy-statusline`, the notifier, the switch.

## Definition of done

- `cargo test`, clippy, fmt, `just check`, `just test` green after every
  commit; all existing parity tools unchanged (`rust/parity/run` 0 unsafe;
  `hook`, `log`, `config`, `hooks`, `pi`, `doctor`, `run` compare as before)
  plus `dashboard_compare.py` 0 differences on the legacy API (when step 1
  was done).
- Report updated; everything committed and pushed to the working branch.
