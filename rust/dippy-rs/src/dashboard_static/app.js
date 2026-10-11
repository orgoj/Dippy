"use strict";
// One UI for a node and a hub. A node serves /api/entries and /api/approvals
// itself; a hub lists nodes at /api/nodes and proxies each one under
// /n/<name>/. Text goes into the page with textContent only.
const token = new URLSearchParams(location.hash.slice(1)).get("token") || "";
const headers = {"X-Dippy-Token": token};
const $ = (id) => document.getElementById(id);
const form = $("filters");
const rows = $("rows");
const status = $("status");
const nodesSection = $("nodes");
const nodeList = $("node-list");
const addNode = $("add-node");
const KINDS = ["command", "redirect", "read", "edit", "web", "mcp"];
const ARMING_MS = 1000;

async function api(path, options) {
  const response = await fetch(path, {headers, cache: "no-store", ...options});
  const body = await response.json();
  return {status: response.status, body};
}

function post(path, body) {
  return api(path, {
    method: "POST",
    headers: {...headers, "Content-Type": "application/json"},
    body: JSON.stringify(body),
  });
}

function setStatus(text, error) {
  status.className = error ? "error" : "";
  status.textContent = text;
}

// ------------------------------------------------------------- sources

// Returns {single, nodes: [{name, base}]} or throws. 404 means a node.
async function sources() {
  const {status: code, body} = await api("/api/nodes");
  if (code === 404) {
    document.body.classList.add("single");
    nodesSection.hidden = true;
    return {single: true, nodes: [{name: "", base: ""}]};
  }
  if (code !== 200) throw new Error(body.error || "HTTP " + code);
  document.body.classList.remove("single");
  nodesSection.hidden = false;
  const nodes = body.map((n) => ({...n, base: "/n/" + encodeURIComponent(n.name)}));
  return {single: false, nodes};
}

// Each source's result for path: {node, ok, body | error}.
async function gather(nodes, path) {
  const results = await Promise.allSettled(nodes.map((n) => api(n.base + path)));
  return results.map((result, i) => {
    const node = nodes[i];
    if (result.status === "rejected") {
      return {node, ok: false, error: String(result.reason.message || result.reason)};
    }
    if (result.value.status !== 200) {
      return {node, ok: false, error: result.value.body.error || "HTTP " + result.value.status};
    }
    return {node, ok: true, body: result.value.body};
  });
}

function cell(row, text, className, title) {
  const td = document.createElement("td");
  td.textContent = text || "";
  if (className) td.className = className;
  if (title) td.title = title;
  row.appendChild(td);
  return td;
}

function renderNodes(nodes, errors) {
  const fragment = document.createDocumentFragment();
  for (const node of nodes) {
    const item = document.createElement("li");
    const error = errors[node.name];
    item.textContent = node.name + (error ? ": " + error : ": ok");
    item.title = node.url;
    if (error) item.className = "error";
    const remove = document.createElement("button");
    remove.type = "button";
    remove.textContent = "remove";
    remove.addEventListener("click", async () => {
      await api("/api/nodes/" + encodeURIComponent(node.name), {method: "DELETE"});
      refresh();
    });
    item.appendChild(remove);
    fragment.appendChild(item);
  }
  nodeList.replaceChildren(fragment);
}

// ------------------------------------------------------------- audit log

function query() {
  const params = new URLSearchParams();
  const data = new FormData(form);
  const decision = data.get("decision");
  if (decision === "not_allow") params.set("not_allow", "1");
  else if (decision) params.set("decision", decision);
  for (const name of ["agent", "cwd", "grep", "limit"]) {
    const value = String(data.get(name) || "").trim();
    if (value) params.set(name, value);
  }
  return params.toString();
}

function what(entry) {
  if (entry.command) return entry.command;
  if (entry.tool) return [entry.tool, entry.file_path].filter(Boolean).join(" ");
  return entry.cmd || "";
}

function renderLog(entries) {
  const fragment = document.createDocumentFragment();
  for (const entry of entries.slice().reverse()) {
    const row = document.createElement("tr");
    const time = entry.ts ? new Date(entry.ts) : null;
    cell(row, entry.node, "node");
    cell(row, time ? time.toLocaleString() : "", "time");
    cell(row, entry.decision, "d-" + entry.decision);
    cell(row, entry.agent);
    cell(row, entry.cwd, "cwd", entry.cwd);
    cell(row, what(entry), "what");
    cell(row, [entry.rule, entry.message].filter(Boolean).join(" | "));
    fragment.appendChild(row);
  }
  rows.replaceChildren(fragment);
}

async function refreshLog(nodes, errors) {
  const results = await gather(nodes, "/api/entries?" + query());
  const entries = [];
  for (const r of results) {
    if (!r.ok) errors[r.node.name] = r.error;
    else for (const entry of r.body) entries.push({...entry, node: r.node.name});
  }
  entries.sort((a, b) => (Date.parse(a.ts) || 0) - (Date.parse(b.ts) || 0));
  const limit = Number(form.elements.limit.value) || 200;
  renderLog(entries.slice(-limit));
  return entries.length + " entries";
}

// ------------------------------------------------------------- approvals

let items = [];          // [{...item, node, base}], newest first
const seen = new Set();  // ids already notified

function subject(request) {
  if (request.command) return request.command;
  return [request.tool, request.file_path].filter(Boolean).join(" ");
}

function seconds(ms) {
  const s = Math.max(0, Math.round(ms / 1000));
  return s >= 60 ? Math.floor(s / 60) + "m " + (s % 60) + "s" : s + "s";
}

function renderQueue() {
  const now = Date.now();
  const pending = items.filter((i) => i.state === "pending");
  const fragment = document.createDocumentFragment();
  for (const item of pending) {
    const row = document.createElement("tr");
    cell(row, item.node, "node");
    cell(row, item.request.agent);
    cell(row, item.request.cwd, "cwd", item.request.cwd);
    cell(row, subject(item.request), "what");
    cell(row, seconds(now - item.created), "time");
    const left = item.deadline - now;
    cell(row, seconds(left), left < 10000 ? "left urgent" : "left");
    row.addEventListener("click", () => openDialog(item));
    fragment.appendChild(row);
  }
  $("queue").replaceChildren(fragment);
  $("queue-empty").hidden = pending.length > 0;
  const done = document.createDocumentFragment();
  for (const item of items.filter((i) => i.state !== "pending").slice(0, 50)) {
    const row = document.createElement("tr");
    cell(row, item.node, "node");
    cell(row, item.state, "s-" + item.state);
    cell(row, item.request.agent);
    cell(row, subject(item.request), "what");
    cell(row, item.decided ? new Date(item.decided).toLocaleTimeString() : "", "time");
    cell(row, item.note);
    done.appendChild(row);
  }
  $("done").replaceChildren(done);
  $("pending-count").textContent = pending.length ? "(" + pending.length + ")" : "";
  document.title = (pending.length ? "(" + pending.length + ") " : "") + "Dippy Dashboard";
}

function notify(fresh) {
  if (!$("notify").checked || !("Notification" in window)) return;
  if (Notification.permission !== "granted") return;
  for (const item of fresh) {
    new Notification("Dippy: approval requested", {
      body: (item.request.agent || "") + ": " + subject(item.request),
      tag: item.id,
    });
  }
}

async function refreshApprovals(nodes, errors) {
  const results = await gather(nodes, "/api/approvals");
  const next = [];
  for (const r of results) {
    if (!r.ok) {
      // A node without the queue (Python Dippy) answers 404: not an error.
      if (r.error !== "not found") errors[r.node.name] = r.error;
      continue;
    }
    for (const item of r.body) next.push({...item, node: r.node.name, base: r.node.base});
  }
  next.sort((a, b) => b.created - a.created);
  const fresh = next.filter((i) => i.state === "pending" && !seen.has(i.id));
  for (const item of fresh) seen.add(item.id);
  items = next;
  renderQueue();
  notify(fresh);
  if (dialogItem) {
    const current = items.find((i) => i.id === dialogItem.id && i.node === dialogItem.node);
    if (current) updateDialogState(current);
  }
  return items.filter((i) => i.state === "pending").length + " pending";
}

// ------------------------------------------------------------- dialog

const dialog = $("dialog");
let dialogItem = null;
let selected = null;     // cause index or null
let armedAt = 0;         // decisions are ignored before this time
let checkSeq = 0;

function arm() {
  armedAt = Date.now() + ARMING_MS;
}

function armed() {
  return armedAt !== 0 && Date.now() >= armedAt && document.hasFocus();
}

function itemPath(item, suffix) {
  return item.base + "/api/approvals/" + encodeURIComponent(item.id) + (suffix || "");
}

// Rule tags from context flags: all but the structural `@` ones.
function tagsOf(flags) {
  return (flags || []).filter((f) => !f.startsWith("@")).join(",");
}

function prefill(request, index) {
  const cause = index === null ? null : request.causes[index];
  let kind = "command";
  let pattern = request.command || request.file_path || "";
  if (cause && KINDS.includes(cause.kind)) {
    kind = cause.kind;
    pattern = cause.target;
  }
  $("d-kind").value = kind;
  // A prefilled rule never introduces an environment reference.
  $("d-pattern").value = pattern.split("${").join("$${");
  $("d-tags").value = cause ? tagsOf(cause.flags) : tagsOf(request.flags);
}

function ruleText(rule) {
  if (!rule) return "built-in classification";
  let text = rule.directive + " " + rule.pattern;
  if (rule.file) text += " (" + rule.file + (rule.line ? ":" + rule.line : "") + ")";
  return text;
}

function renderCauses(item) {
  const list = document.createDocumentFragment();
  item.request.causes.forEach((cause, index) => {
    const li = document.createElement("li");
    if (index === selected) li.className = "selected";
    const head = document.createElement("span");
    head.textContent = "[" + (item.statuses[index] || "?") + "] " + cause.kind + ": " + cause.target;
    head.className = "d-" + (item.statuses[index] || "ask");
    const rule = document.createElement("span");
    rule.className = "rule";
    const flags = (cause.flags || []).length ? "  flags: " + cause.flags.join(", ") : "";
    rule.textContent = ruleText(cause.rule) + flags;
    li.append(head, rule);
    li.addEventListener("click", () => {
      selected = index;
      prefill(item.request, index);
      renderCauses(dialogItem);
      check();
    });
    list.appendChild(li);
  });
  $("d-causes").replaceChildren(list);
  const allAllowed = item.request.causes.length > 0
    && item.statuses.length === item.request.causes.length
    && item.statuses.every((s) => s === "allow");
  // Suggest, never click.
  $("d-suggest").hidden = !allAllowed || item.state !== "pending";
  $("d-approve").classList.toggle("suggested", allAllowed);
}

function updateDialogState(item) {
  dialogItem = {...dialogItem, ...item};
  renderCauses(dialogItem);
  const pending = item.state === "pending";
  $("d-approve").disabled = !pending;
  $("d-deny").disabled = !pending;
  $("d-note").disabled = !pending;
  $("d-left").textContent = pending ? seconds(item.deadline - Date.now()) + " left" : "";
  if (!pending) {
    $("d-result").className = "check s-" + item.state;
    $("d-result").textContent = "Final state: " + item.state + (item.note ? " (" + item.note + ")" : "");
  }
}

function openDialog(item) {
  dialogItem = item;
  const request = item.request;
  selected = request.causes.length ? 0 : null;
  $("d-title").textContent = (item.node ? item.node + ": " : "") + (request.agent || "")
    + " asks" + (request.server ? " to run on " + request.server : "");
  $("d-request").value = subject(request) || request.tool || "";
  $("d-meta").textContent = "cwd " + request.cwd
    + (request.tool ? "  tool " + request.tool : "") + "  reason: " + request.reason;
  $("d-flags").textContent = (request.context_flags || []).length
    ? "context flags: " + request.context_flags.join(", ") : "";
  // A request without command and causes (an unmatched tool) has no rule.
  const editable = Boolean(request.command) || request.causes.length > 0;
  $("d-editor").disabled = !editable;
  $("d-action").value = "allow";
  prefill(request, selected);
  $("d-check").textContent = editable ? "" : "No rule applies to this request.";
  $("d-note").value = "";
  $("d-result").textContent = "";
  updateDialogState(item);
  if (!dialog.open) dialog.showModal();
  arm();
  if (editable) check();
}

function candidate() {
  const target = document.querySelector("input[name=d-target]:checked").value;
  return {
    cause: selected,
    action: $("d-action").value,
    kind: $("d-kind").value,
    tags: $("d-tags").value,
    pattern: $("d-pattern").value,
    target,
  };
}

async function check() {
  if (!dialogItem || $("d-editor").disabled) return;
  const seq = ++checkSeq;
  const {status: code, body} = await post(itemPath(dialogItem, "/check"), candidate());
  if (seq !== checkSeq) return;
  const out = $("d-check");
  $("d-save").disabled = !(code === 200 && body.can_save);
  if (code !== 200) {
    out.className = "check error";
    out.textContent = body.error || "HTTP " + code;
    return;
  }
  out.className = "check";
  if (body.outcome) {
    out.textContent = body.line + "  ->  this part: " + body.outcome.cause
      + ", whole request: " + body.outcome.request
      + (body.broad ? "  (broad pattern!)" : "") + "  in " + body.file;
  } else {
    out.className = "check error";
    out.textContent = body.line + "  ->  " + body.error;
  }
}

async function saveRule() {
  if (!dialogItem) return;
  const {status: code, body} = await post(itemPath(dialogItem, "/rule"), candidate());
  const out = $("d-check");
  if (code !== 200) {
    out.className = "check error";
    out.textContent = "Not saved: " + (body.error || (body.line ? body.line : "HTTP " + code));
    return;
  }
  out.className = "check";
  out.textContent = "Saved " + body.line + " to " + body.file;
  updateDialogState({...dialogItem, statuses: body.statuses});
  check();
}

async function decide(decision) {
  if (!dialogItem || !armed() || dialogItem.state !== "pending") return;
  const note = $("d-note").value.trim();
  const {status: code, body} = await post(itemPath(dialogItem, "/decision"),
    {decision, note: note || null});
  if (code === 200 || code === 409) {
    updateDialogState(body);
    if (code === 200) dialog.close();
  } else {
    $("d-result").className = "check error";
    $("d-result").textContent = body.error || "HTTP " + code;
  }
  refresh();
}

for (const id of ["d-action", "d-kind", "d-tags", "d-pattern"]) {
  $(id).addEventListener("input", check);
}
for (const radio of document.querySelectorAll("input[name=d-target]")) {
  radio.addEventListener("change", check);
}
$("d-save").addEventListener("click", saveRule);
$("d-approve").addEventListener("click", () => decide("allow"));
$("d-deny").addEventListener("click", () => decide("deny"));
$("d-close").addEventListener("click", () => dialog.close());
dialog.addEventListener("close", () => { dialogItem = null; armedAt = 0; });
// Esc would close the dialog: it denies instead (only when armed).
dialog.addEventListener("cancel", (event) => {
  event.preventDefault();
  decide("deny");
});
// Keyboard approval only with Ctrl+Enter; nothing else decides.
dialog.addEventListener("keydown", (event) => {
  if (event.key === "Enter" && event.ctrlKey) {
    event.preventDefault();
    decide("allow");
  }
});
// Misclick protection: re-arm after the window regains focus.
window.addEventListener("focus", () => { if (dialog.open) arm(); });
window.addEventListener("blur", () => { if (dialog.open) armedAt = Number.MAX_SAFE_INTEGER; });

// ------------------------------------------------------------- views

let view = "approvals";
for (const button of document.querySelectorAll("#views button")) {
  button.addEventListener("click", () => {
    view = button.dataset.view;
    for (const b of document.querySelectorAll("#views button")) {
      b.classList.toggle("active", b === button);
    }
    $("view-approvals").hidden = view !== "approvals";
    $("view-log").hidden = view !== "log";
    refresh();
  });
}
$("notify").addEventListener("change", () => {
  if ($("notify").checked && "Notification" in window && Notification.permission === "default") {
    Notification.requestPermission();
  }
});

// One refresh at a time: dead nodes take NODE_TIMEOUT to fail, and stacked
// polls would exhaust the browser's connections to the server.
let busy = false;

async function refresh() {
  if (busy) return;
  busy = true;
  try {
    const {single, nodes} = await sources();
    const errors = {};
    // The queue is always polled (title count, notifications); the log only
    // while shown and not paused. The hub is never long-polled.
    let text = await refreshApprovals(nodes, errors);
    if (view === "log" && !form.elements.pause.checked) {
      text += ", " + await refreshLog(nodes, errors);
    }
    if (!single) renderNodes(nodes, errors);
    const failed = Object.keys(errors);
    if (single && failed.length) throw new Error(errors[""]);
    setStatus(text + ", updated " + new Date().toLocaleTimeString()
      + (failed.length ? ", unreachable: " + failed.join(", ") : ""), failed.length > 0);
  } catch (error) {
    setStatus(String(error.message || error), true);
  } finally {
    busy = false;
  }
}

addNode.addEventListener("submit", async (event) => {
  event.preventDefault();
  const data = new FormData(addNode);
  const {status: code, body} = await api(
    "/api/nodes/" + encodeURIComponent(String(data.get("name")).trim()), {
      method: "PUT",
      headers: {...headers, "Content-Type": "application/json"},
      body: JSON.stringify({url: String(data.get("url")).trim(),
                            token: String(data.get("token")).trim()}),
    });
  if (code !== 200) {
    setStatus(body.error || "HTTP " + code, true);
    return;
  }
  addNode.reset();
  refresh();
});

if (!token) {
  setStatus("Missing token: open the URL printed by dippy dashboard.", true);
} else {
  form.addEventListener("input", refresh);
  form.addEventListener("submit", (event) => { event.preventDefault(); refresh(); });
  refresh();
  setInterval(refresh, 2000);
  // Countdowns between polls.
  setInterval(() => {
    renderQueue();
    if (dialogItem && dialogItem.state === "pending") {
      $("d-left").textContent = seconds(dialogItem.deadline - Date.now()) + " left";
    }
  }, 1000);
}
