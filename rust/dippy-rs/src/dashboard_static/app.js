"use strict";
// One UI for a node and a hub. A node serves /api/entries itself; a hub lists
// nodes at /api/nodes and proxies each one under /n/<name>/.
const token = new URLSearchParams(location.hash.slice(1)).get("token") || "";
const headers = {"X-Dippy-Token": token};
const form = document.getElementById("filters");
const rows = document.getElementById("rows");
const status = document.getElementById("status");
const nodesSection = document.getElementById("nodes");
const nodeList = document.getElementById("node-list");
const addNode = document.getElementById("add-node");

async function api(path, options) {
  const response = await fetch(path, {headers, cache: "no-store", ...options});
  const body = await response.json();
  return {status: response.status, body};
}

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

function cell(row, text, className, title) {
  const td = document.createElement("td");
  td.textContent = text || "";
  if (className) td.className = className;
  if (title) td.title = title;
  row.appendChild(td);
}

function what(entry) {
  if (entry.command) return entry.command;
  if (entry.tool) return [entry.tool, entry.file_path].filter(Boolean).join(" ");
  return entry.cmd || "";
}

function render(entries) {
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

// Returns [{name, base}] or throws. 404 means this server is a node.
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

// One refresh at a time: dead nodes take NODE_TIMEOUT to fail, and stacked
// polls would exhaust the browser's connections to the server.
let busy = false;

async function refresh() {
  if (busy || form.elements.pause.checked) return;
  busy = true;
  try {
    const {single, nodes} = await sources();
    const q = query();
    const results = await Promise.allSettled(
      nodes.map((n) => api(n.base + "/api/entries?" + q)));
    const entries = [];
    const errors = {};
    results.forEach((result, i) => {
      const node = nodes[i];
      if (result.status === "rejected") {
        errors[node.name] = String(result.reason.message || result.reason);
      } else if (result.value.status !== 200) {
        errors[node.name] = result.value.body.error || "HTTP " + result.value.status;
      } else {
        for (const entry of result.value.body) entries.push({...entry, node: node.name});
      }
    });
    entries.sort((a, b) => (Date.parse(a.ts) || 0) - (Date.parse(b.ts) || 0));
    const limit = Number(form.elements.limit.value) || 200;
    render(entries.slice(-limit));
    if (!single) renderNodes(nodes, errors);
    const failed = Object.keys(errors);
    if (single && failed.length) throw new Error(errors[""]);
    status.className = failed.length ? "error" : "";
    status.textContent = entries.length + " entries, updated "
      + new Date().toLocaleTimeString()
      + (failed.length ? ", unreachable: " + failed.join(", ") : "");
  } catch (error) {
    status.className = "error";
    status.textContent = String(error.message || error);
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
    status.className = "error";
    status.textContent = body.error || "HTTP " + code;
    return;
  }
  addNode.reset();
  refresh();
});

if (!token) {
  status.className = "error";
  status.textContent = "Missing token: open the URL printed by dippy dashboard.";
} else {
  form.addEventListener("input", refresh);
  form.addEventListener("submit", (event) => { event.preventDefault(); refresh(); });
  refresh();
  setInterval(refresh, 2000);
}
