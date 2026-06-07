const { invoke } = window.__TAURI__.core;

// ── Relative time helper ──────────────────────────────

function relativeTime(unixSecs) {
  if (unixSecs == null) return "Never";
  const diff = Math.max(0, Math.floor(Date.now() / 1000) - unixSecs);
  const units = [
    [31536000, "year"], [2592000, "month"], [604800, "week"],
    [86400, "day"], [3600, "hour"], [60, "minute"],
  ];
  for (const [secs, label] of units) {
    const n = Math.floor(diff / secs);
    if (n >= 1) return `${n} ${label}${n > 1 ? "s" : ""} ago`;
  }
  return "Just now";
}

// ── Screen helpers ────────────────────────────────────────────

function showScreen(id) {
  document.querySelectorAll(".screen").forEach((s) => s.classList.remove("active"));
  document.getElementById(id).classList.add("active");
  if (id === "screen-main") updateInstallPathDisplay();
}

function showTab(name) {
  document.querySelectorAll(".tab-item").forEach((t) => {
    t.classList.toggle("active", t.dataset.tab === name);
  });
  document.querySelectorAll(".tab-content").forEach((c) => {
    c.classList.toggle("active", c.id === `tab-${name}`);
  });
}

// ── Status helpers ────────────────────────────────────────────

function setStatus(msg, type = "neutral") {
  const el = document.getElementById("status-main");
  if (!msg) {
    el.classList.add("hidden");
    el.textContent = "";
    return;
  }
  el.classList.remove("hidden", "msg-neutral", "msg-error", "msg-success");
  el.classList.add(type === "error" ? "msg-error" : type === "success" ? "msg-success" : "msg-neutral");
  el.textContent = msg;
}

function setLoading(loading) {
  document.getElementById("btn-push").disabled = loading;
  document.getElementById("btn-pull").disabled = loading;
  document.getElementById("pull-input").disabled = loading;
  document.getElementById("status-main").classList.toggle("is-loading", loading);
}

let pendingInstallations = null;
let pendingAction = null; // "push" or "pull" — auto-resume after selecting

// ── Countdown timer ───────────────────────────────────────────

let countdownInterval = null;
let wasUrgent = false;

function startCountdown(seconds) {
  const countdownEl = document.getElementById("countdown");
  const timeEl = document.getElementById("countdown-time");
  wasUrgent = false;

  function tick(remaining) {
    if (remaining <= 0) {
      clearInterval(countdownInterval);
      timeEl.textContent = "Expired";
      countdownEl.classList.add("urgent");
      return;
    }
    const m = String(Math.floor(remaining / 60)).padStart(2, "0");
    const s = String(remaining % 60).padStart(2, "0");
    timeEl.textContent = `${m}:${s}`;
    const isUrgent = remaining <= 300;
    if (isUrgent && !wasUrgent) {
      countdownEl.classList.add("urgent-enter");
      countdownEl.addEventListener("animationend", () => countdownEl.classList.remove("urgent-enter"), { once: true });
    }
    wasUrgent = isUrgent;
    countdownEl.classList.toggle("urgent", isUrgent);
  }

  tick(seconds);
  countdownInterval = setInterval(() => {
    seconds--;
    tick(seconds);
  }, 1000);
}

function stopCountdown() {
  if (countdownInterval) {
    clearInterval(countdownInterval);
    countdownInterval = null;
  }
}

// ── Clipboard ─────────────────────────────────────────────────

function copyText(text) {
  navigator.clipboard.writeText(text).catch(() => {
    const ta = document.createElement("textarea");
    ta.value = text;
    ta.style.position = "fixed";
    ta.style.opacity = "0";
    document.body.appendChild(ta);
    ta.select();
    document.execCommand("copy");
    document.body.removeChild(ta);
  });
}

// ── Installation selector ─────────────────────────────

function renderInstallationRow(inst) {
  const row = document.createElement("div");
  row.className = "install-row";

  const info = document.createElement("div");
  info.className = "install-info";

  const badge = document.createElement("span");
  badge.className = "install-badge";
  const typeLabels = { native: "Native", "xdg-config": "XDG Config", xdg: "XDG", flatpak: "Flatpak", snap: "Snap", custom: "Custom" };
  badge.textContent = typeLabels[inst.installType] || inst.installType;

  const pathEl = document.createElement("div");
  pathEl.className = "install-path";
  pathEl.textContent = inst.path;
  pathEl.title = inst.path;

  const used = document.createElement("div");
  used.className = "install-used";
  used.textContent = `Last used ${relativeTime(inst.lastUsed)}`;

  info.appendChild(badge);
  info.appendChild(pathEl);
  info.appendChild(used);

  const btn = document.createElement("button");
  btn.className = "btn btn-secondary btn-use-install";
  btn.textContent = "Use this";
  btn.addEventListener("click", async () => {
    btn.disabled = true;
    btn.textContent = "Saving\u2026";
    try {
      await invoke("set_selected_installation_cmd", { path: inst.path });
      pendingInstallations = null;
      const action = pendingAction;
      pendingAction = null;
      hideInstallSelector();
      setStatus("");
      if (action === "push") await handlePush();
      else if (action === "pull") await handlePull();
    } catch (e) {
      document.getElementById("si-error").textContent = String(e);
      btn.disabled = false;
      btn.textContent = "Use this";
    }
  });

  row.appendChild(info);
  row.appendChild(btn);
  return row;
}

function showInstallSelector(msg, diags, isFirstTime = false) {
  showScreen("screen-select-install");
  document.getElementById("btn-push").disabled = true;
  document.getElementById("btn-pull").disabled = true;
  document.getElementById("btn-si-back").classList.toggle("hidden", isFirstTime);

  const sub = document.getElementById("si-sub");
  sub.textContent = msg ?? "Choose which Zen profile to sync.";

  const list = document.getElementById("install-list");
  list.innerHTML = "";
  if (pendingInstallations && pendingInstallations.length > 0) {
    pendingInstallations.forEach((inst) => list.appendChild(renderInstallationRow(inst)));
    document.getElementById("btn-show-custom-path").style.display = "";
    document.getElementById("custom-path-form").classList.add("hidden");
    document.getElementById("si-diagnostics").classList.add("hidden");
  } else {
    document.getElementById("btn-show-custom-path").style.display = "none";
    document.getElementById("custom-path-form").classList.remove("hidden");
    showDiagnostics(diags);
  }
}

function showDiagnostics(diags) {
  const el = document.getElementById("si-diagnostics");
  const textEl = document.getElementById("si-diagnostics-text");
  if (diags && diags.length > 0) {
    textEl.textContent = diags.join("\n");
    el.classList.remove("hidden");
  } else {
    el.classList.add("hidden");
  }
}

function hideInstallSelector() {
  pendingAction = null;
  pendingInstallations = null;
  showScreen("screen-main");
  document.getElementById("btn-push").disabled = false;
  document.getElementById("btn-pull").disabled = false;
}

async function ensureProfileReady(action) {
  try {
    const result = await invoke("ensure_profile_ready_cmd");
    if (result.type === "multipleInstallations") {
      pendingInstallations = result.value;
      pendingAction = action;
      showInstallSelector("You have multiple Zen installations. Pick which one to use.", undefined, true);
      setStatus("Multiple Zen installations found. Select one.", "neutral");
      return false;
    }
    return true;
  } catch (err) {
    const msg = String(err);
    if (msg.includes("not found") || msg.includes("no longer exists")) {
      try {
        const scanResult = await invoke("scan_zen_installations_cmd");
        pendingInstallations = scanResult.installations;
        pendingAction = action;
        if (pendingInstallations.length > 0) {
          showInstallSelector("Select which Zen installation to use.", undefined, true);
        } else {
          showInstallSelector("No Zen installation detected. Enter a custom path below.", scanResult.diagnostics, true);
        }
        setStatus("Select a Zen installation to continue.", "neutral");
      } catch (e) {
        setStatus("Error scanning installations: " + String(e), "error");
      }
      return false;
    }
    setStatus(msg, "error");
    return false;
  }
}

// ── Push flow ─────────────────────────────────────────────────

async function handlePush() {
  setLoading(true);
  setStatus("Making sure Zen is closed\u2026");

  try {
    const zenOpen = await invoke("is_zen_running");
    if (zenOpen) {
      setStatus("Zen Browser is still running. Quit it (\u2318Q) first, then try again.", "error");
      return;
    }

    const ready = await ensureProfileReady("push");
    if (!ready) return;

    setStatus("Encrypting and uploading\u2026");
    const syncCode = await invoke("push_profile");

    stopCountdown();
    const codeEl = document.getElementById("sync-code");
    const copyBtn = document.getElementById("btn-copy");
    codeEl.textContent = syncCode;
    codeEl.classList.remove("copied");
    copyBtn.textContent = "Copy";
    copyBtn.classList.remove("copied");
    setStatus("");
    showScreen("screen-push");
    startCountdown(3600); // 1 hour

  } catch (err) {
    const msg = String(err);
    if (msg.startsWith("MULTIPLE_INSTALLATIONS:")) {
      try {
        pendingInstallations = JSON.parse(msg.slice("MULTIPLE_INSTALLATIONS:".length));
        pendingAction = "push";
        showInstallSelector("You have multiple Zen installations. Pick which one to upload from.", undefined, true);
        setStatus("Multiple Zen installations found. Select one.", "neutral");
      } catch (e) {
        setStatus("Error parsing installations. Try rescanning.", "error");
      }
    } else if (msg.includes("profile folder not found") || msg.includes("profile path no longer exists")) {
      try {
        const scanResult = await invoke("scan_zen_installations_cmd");
        pendingInstallations = scanResult.installations;
        pendingAction = "push";
        if (pendingInstallations.length > 0) {
          showInstallSelector("Select which Zen installation to upload from.", undefined, true);
        } else {
          showInstallSelector("No Zen installation detected. Enter a custom path below.", scanResult.diagnostics, true);
        }
        setStatus("Select a Zen installation to continue.", "neutral");
      } catch (e) {
        setStatus("Error scanning installations: " + String(e), "error");
      }
    } else {
      setStatus(msg, "error");
    }
  } finally {
    setLoading(false);
  }
}

// ── Pull flow ─────────────────────────────────────────────────

async function handlePull() {
  const input = document.getElementById("pull-input");
  input.classList.remove("is-error");
  const rawCode = input.value.trim().toUpperCase();

  if (!rawCode) {
    setStatus("Enter a sync code (e.g. ZEN-A3F9B2-ABC123)", "error");
    input.classList.add("is-error");
    input.focus();
    return;
  }
  if (!/^ZEN-[A-Z0-9]{4,8}-[A-Z0-9]{4,12}$/.test(rawCode)) {
    setStatus("Invalid code. Format is ZEN-XXXXXX-YYYYYY", "error");
    input.classList.add("is-error");
    input.focus();
    return;
  }

  setLoading(true);
  setStatus("Making sure Zen is closed\u2026");

  try {
    const zenOpen = await invoke("is_zen_running");
    if (zenOpen) {
      setStatus("Zen Browser is still running. Quit it (\u2318Q) first, then try again.", "error");
      return;
    }

    const ready = await ensureProfileReady("pull");
    if (!ready) return;

    setStatus("Downloading and decrypting\u2026");
    const files = await invoke("pull_profile", { syncCode: rawCode });

    input.classList.remove("is-error");
    document.getElementById("pull-files").textContent = files.join(", ");
    setStatus("");
    showScreen("screen-pull");

  } catch (err) {
    const msg = String(err);
    if (msg.startsWith("MULTIPLE_INSTALLATIONS:")) {
      try {
        pendingInstallations = JSON.parse(msg.slice("MULTIPLE_INSTALLATIONS:".length));
        pendingAction = "pull";
        showInstallSelector("You have multiple Zen installations. Pick which one to download to.", undefined, true);
        setStatus("Multiple Zen installations found. Select one.", "neutral");
      } catch (e) {
        setStatus("Error parsing installations. Try rescanning.", "error");
      }
    } else if (msg.includes("profile folder not found") || msg.includes("profile path no longer exists")) {
      try {
        const scanResult = await invoke("scan_zen_installations_cmd");
        pendingInstallations = scanResult.installations;
        pendingAction = "pull";
        if (pendingInstallations.length > 0) {
          showInstallSelector("Select which Zen installation to download to.", undefined, true);
        } else {
          showInstallSelector("No Zen installation detected. Enter a custom path below.", scanResult.diagnostics, true);
        }
        setStatus("Select a Zen installation to continue.", "neutral");
      } catch (e) {
        setStatus("Error scanning installations: " + String(e), "error");
      }
    } else {
      setStatus(msg, "error");
      input.classList.add("is-error");
    }
  } finally {
    setLoading(false);
  }
}

// ── Event listeners ───────────────────────────────────────────

document.getElementById("btn-push").addEventListener("click", handlePush);
document.getElementById("btn-pull").addEventListener("click", handlePull);

document.getElementById("pull-input").addEventListener("keydown", (e) => {
  if (e.key === "Enter") handlePull();
});

// Tab switching
document.querySelectorAll(".tab-item").forEach((tab) => {
  tab.addEventListener("click", async () => {
    const name = tab.dataset.tab;
    showTab(name);
    if (name === "sync") await loadSyncStatus();
    updateInstallPathDisplay();
  });
});

// Copy sync code
function doCopy() {
  const code = document.getElementById("sync-code").textContent;
  copyText(code);

  const btn = document.getElementById("btn-copy");
  const codeEl = document.getElementById("sync-code");
  btn.textContent = "Copied!";
  btn.classList.add("copied");
  codeEl.classList.remove("copied");
  void codeEl.offsetWidth;
  codeEl.classList.add("copied");
  setTimeout(() => {
    btn.textContent = "Copy";
    btn.classList.remove("copied");
    codeEl.classList.remove("copied");
  }, 2000);
}

document.getElementById("sync-code").addEventListener("click", doCopy);
document.getElementById("btn-copy").addEventListener("click", doCopy);

document.getElementById("btn-push-done").addEventListener("click", () => {
  stopCountdown();
  showScreen("screen-main");
  showTab("sync");
});

document.getElementById("btn-pull-done").addEventListener("click", () => {
  showScreen("screen-main");
  showTab("sync");
  const pullInput = document.getElementById("pull-input");
  pullInput.value = "";
  pullInput.classList.remove("is-error");
  setStatus("");
});

// Auto-format pull input into ZEN-XXXXXX-YYYYYY as user types
document.getElementById("pull-input").addEventListener("input", (e) => {
  e.target.classList.remove("is-error");
  setStatus("");
  let raw = e.target.value.toUpperCase().replace(/[^A-Z0-9]/g, "");
  if (raw.startsWith("ZEN")) raw = raw.slice(3);
  let out = "ZEN";
  if (raw.length > 0) out += "-" + raw.slice(0, 6);
  if (raw.length > 6) out += "-" + raw.slice(6, 18);
  e.target.value = out.slice(0, 28);

  if (/^ZEN-[A-Z0-9]{4,8}-[A-Z0-9]{4,12}$/.test(e.target.value)) {
    e.target.classList.remove("code-valid-flash");
    void e.target.offsetWidth;
    e.target.classList.add("code-valid-flash");
    e.target.addEventListener("animationend", () => e.target.classList.remove("code-valid-flash"), { once: true });
  }
});

// ── Installation selector events ───────────────────────────────

document.getElementById("btn-change-installation").addEventListener("click", async (e) => {
  e.preventDefault();
  pendingAction = null;
  const scanResult = await invoke("scan_zen_installations_cmd");
  pendingInstallations = scanResult.installations;
  showInstallSelector(null, scanResult.diagnostics);
  document.getElementById("si-error").textContent = "";
});

document.getElementById("btn-si-back").addEventListener("click", () => {
  pendingAction = null;
  pendingInstallations = null;
  hideInstallSelector();
});

document.getElementById("btn-si-refresh").addEventListener("click", async () => {
  const diagEl = document.getElementById("si-diagnostics");
  diagEl.classList.add("hidden");
  document.getElementById("si-error").textContent = "";
  const scanResult = await invoke("scan_zen_installations_cmd");
  pendingInstallations = scanResult.installations;
  showInstallSelector(pendingInstallations.length > 0
    ? "Pick which Zen installation to use."
    : "No Zen installation detected. Enter a custom path below.",
    scanResult.diagnostics);
});

document.getElementById("btn-show-custom-path").addEventListener("click", (e) => {
  e.preventDefault();
  document.getElementById("custom-path-form").classList.remove("hidden");
  document.getElementById("btn-show-custom-path").style.display = "none";
});

document.getElementById("input-custom-path").addEventListener("input", () => {
  const val = document.getElementById("input-custom-path").value.trim();
  document.getElementById("btn-validate-custom").disabled = !val;
});

document.getElementById("btn-validate-custom").addEventListener("click", async () => {
  const path = document.getElementById("input-custom-path").value.trim();
  if (!path) return;
  const btn = document.getElementById("btn-validate-custom");
  const errEl = document.getElementById("custom-path-error");
  btn.disabled = true;
  btn.textContent = "Validating\u2026";
  errEl.textContent = "";
  try {
    const result = await invoke("validate_custom_path_cmd", { path });
    if (result) {
      pendingInstallations = [result];
      const list = document.getElementById("install-list");
      list.innerHTML = "";
      list.appendChild(renderInstallationRow(result));
      document.getElementById("custom-path-form").classList.add("hidden");
    } else {
      errEl.textContent = "No Zen profile found at this path.";
    }
  } catch (e) {
    errEl.textContent = String(e);
  } finally {
    btn.disabled = false;
    btn.textContent = "Validate";
  }
});

async function updateInstallPathDisplay() {
  const el = document.getElementById("change-installation-link");
  const pathEl = document.getElementById("current-install-path");
  try {
    const path = await invoke("get_selected_installation_cmd");
    if (path) {
      pathEl.textContent = path.length > 50 ? "\u2026" + path.slice(-47) : path;
      pathEl.title = path;
      el.classList.remove("hidden");
    } else {
      el.classList.add("hidden");
    }
  } catch (_) {
    el.classList.add("hidden");
  }
}

// ── Sync tab ──────────────────────────────────────────────────────────────────

const syncDisconnected = document.getElementById('sync-disconnected');
const syncConnected    = document.getElementById('sync-connected');
const syncRollback     = document.getElementById('sync-rollback');

function showSyncState(state) {
  syncDisconnected.style.display = state === 'disconnected' ? '' : 'none';
  syncConnected.style.display    = state === 'connected'    ? '' : 'none';
  syncRollback.style.display     = state === 'rollback'     ? '' : 'none';
  if (state === 'disconnected') {
    const btn = document.getElementById('btn-connect-github');
    btn.disabled = false;
    btn.textContent = 'Connect GitHub';
  }
}

async function loadSyncStatus() {
  try {
    const status = await invoke('get_sync_status_cmd');
    if (status.connected) {
      document.getElementById('sync-account-label').textContent = status.username;
      document.getElementById('sync-repo-label').textContent    = `${status.username}/zync-sync`;
      document.getElementById('input-machine-name').value       = status.machineName || '';
      if (status.lastSynced) {
        const d = new Date(status.lastSynced * 1000);
        const from = status.lastSyncedFrom ? ` from ${status.lastSyncedFrom}` : '';
        document.getElementById('sync-last-synced').textContent =
          `Last synced: ${d.toLocaleString()}${from}`;
      } else {
        document.getElementById('sync-last-synced').textContent = 'Not yet synced this session';
      }
      showSyncState('connected');
    } else {
      showSyncState('disconnected');
    }
    updateInstallPathDisplay();
  } catch (e) {
    document.getElementById('sync-connect-error').textContent = String(e);
    showSyncState('disconnected');
  }
}

document.getElementById('btn-connect-github').addEventListener('click', async () => {
  const btn = document.getElementById('btn-connect-github');
  const err = document.getElementById('sync-connect-error');
  document.getElementById('sync-connect-error').textContent = '';
  btn.disabled = true;
  btn.textContent = 'Connecting…';
  err.textContent = '';
  try {
    await invoke('connect_github_cmd');
    await loadSyncStatus();
  } catch (e) {
    err.textContent = String(e);
    btn.disabled = false;
    btn.textContent = 'Connect GitHub';
  }
});

document.getElementById('btn-disconnect').addEventListener('click', async () => {
  document.getElementById('sync-main-error').textContent = '';
  try {
    await invoke('disconnect_github_cmd');
    showSyncState('disconnected');
  } catch (e) {
    document.getElementById('sync-main-error').textContent = String(e);
  }
});

document.getElementById('input-machine-name').addEventListener('change', async (ev) => {
  document.getElementById('sync-main-error').textContent = '';
  try {
    await invoke('set_machine_name_cmd', { name: ev.target.value });
  } catch (e) {
    document.getElementById('sync-main-error').textContent = String(e);
  }
});

document.getElementById('btn-restore-version').addEventListener('click', async () => {
  const err = document.getElementById('sync-main-error');
  err.textContent = '';
  try {
    const snapshots = await invoke('get_snapshots_cmd');
    renderSnapshotList(snapshots);
    showSyncState('rollback');
  } catch (e) {
    err.textContent = String(e);
  }
});

document.getElementById('btn-back-rollback').addEventListener('click', () => {
  showSyncState('connected');
});

function renderSnapshotList(snapshots) {
  const list = document.getElementById('snapshot-list');
  list.innerHTML = '';
  if (snapshots.length === 0) {
    const empty = document.createElement('p');
    empty.style.color = 'var(--text-muted, #888)';
    empty.style.fontSize = '13px';
    empty.textContent = 'No snapshots yet.';
    list.appendChild(empty);
    return;
  }
  snapshots.forEach(snap => {
    const row = document.createElement('div');
    row.className = 'snapshot-row';

    const dot = document.createElement('span');
    dot.className = snap.isCurrent ? 'snapshot-dot' : 'snapshot-dot hidden';
    row.appendChild(dot);

    const info = document.createElement('div');
    info.className = 'snapshot-info';
    const machine = document.createElement('div');
    machine.className = 'snapshot-machine';
    machine.textContent = snap.machineName;
    const date = document.createElement('div');
    date.className = 'snapshot-date';
    const d = new Date(snap.pushedAt);
    date.textContent = isNaN(d.getTime()) ? snap.pushedAt : d.toLocaleString();
    const size = document.createElement('div');
    size.className = 'snapshot-size';
    size.textContent = snap.sizeMb != null ? `${snap.sizeMb.toFixed(1)} MB` : '';
    info.appendChild(machine);
    info.appendChild(date);
    info.appendChild(size);
    row.appendChild(info);

    if (!snap.isCurrent) {
      const btn = document.createElement('button');
      btn.className = 'btn btn-secondary btn-restore';
      btn.textContent = 'Restore';
      btn.addEventListener('click', async () => {
        btn.disabled = true;
        btn.textContent = 'Restoring…';
        document.getElementById('rollback-error').textContent = '';
        try {
          await invoke('restore_snapshot_cmd', { slot: snap.slot });
          showSyncState('connected');
          await loadSyncStatus();
        } catch (e) {
          const msg = String(e);
          if (msg.includes("profile folder not found") || msg.includes("profile path no longer exists")) {
            document.getElementById('rollback-error').textContent =
              "No Zen installation selected. Use the 'Change' link below to select one, then try again.";
          } else {
            document.getElementById('rollback-error').textContent = msg;
          }
          btn.disabled = false;
          btn.textContent = 'Restore';
        }
      });
      row.appendChild(btn);
    }

    list.appendChild(row);
  });
}

// Listen for sync updates from daemon
window.__TAURI__.event.listen('sync-updated', () => {
  if (syncConnected.style.display !== 'none') {
    loadSyncStatus();
  }
  const orbit = document.querySelector('.sync-orbit');
  if (orbit) {
    orbit.classList.add('is-syncing');
    setTimeout(() => orbit.classList.remove('is-syncing'), 3000);
  }
});

// ── Update dialog ─────────────────────────────────────────

const { listen } = window.__TAURI__.event;

function renderMarkdown(md) {
  const escaped = md
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");

  const lines = escaped.split("\n");
  const out = [];
  let inList = false;

  for (const raw of lines) {
    const line = raw.trimEnd();

    const hMatch = line.match(/^(#{1,3})\s+(.+)/);
    if (hMatch) {
      if (inList) { out.push("</ul>"); inList = false; }
      const level = Math.min(hMatch[1].length + 2, 6); // h3–h5
      out.push(`<h${level}>${inline(hMatch[2])}</h${level}>`);
      continue;
    }

    const liMatch = line.match(/^[-*]\s+(.+)/);
    if (liMatch) {
      if (!inList) { out.push("<ul>"); inList = true; }
      out.push(`<li>${inline(liMatch[1])}</li>`);
      continue;
    }

    if (inList) { out.push("</ul>"); inList = false; }
    if (line === "") { out.push("<br>"); continue; }
    out.push(`<p>${inline(line)}</p>`);
  }

  if (inList) out.push("</ul>");
  return out.join("");
}

function inline(s) {
  return s
    .replace(/\*\*(.+?)\*\*/g, "<strong>$1</strong>")
    .replace(/\*(.+?)\*/g, "<em>$1</em>")
    .replace(/`(.+?)`/g, "<code>$1</code>");
}

async function showUpdateDialog(version, notes) {
  document.getElementById("update-version-number").textContent = version;
  try {
    const current = await window.__TAURI__.app.getVersion();
    document.getElementById("update-current-version").textContent = current;
  } catch (_) {
    document.getElementById("update-current-version").textContent = "";
  }
  document.getElementById("update-notes").innerHTML = notes
    ? renderMarkdown(notes)
    : "<p>No release notes provided.</p>";
  document.getElementById("update-error").classList.add("hidden");
  document.getElementById("btn-update-install").disabled = false;
  document.getElementById("btn-update-install").textContent = "Install & Restart";
  document.getElementById("update-dialog").classList.remove("hidden");
  document.getElementById("btn-update-install").focus();
}

function hideUpdateDialog() {
  document.getElementById("update-dialog").classList.add("hidden");
}

document.getElementById("btn-update-later").addEventListener("click", hideUpdateDialog);

document.addEventListener("keydown", (e) => {
  if (e.key === "Escape" && !document.getElementById("update-dialog").classList.contains("hidden")) {
    hideUpdateDialog();
  }
});

document.getElementById("btn-update-install").addEventListener("click", async () => {
  const btn = document.getElementById("btn-update-install");
  const errEl = document.getElementById("update-error");
  btn.disabled = true;
  btn.textContent = "Downloading…";
  errEl.classList.add("hidden");
  try {
    await invoke("install_update");
    // App restarts — code below never runs
  } catch (err) {
    btn.disabled = false;
    btn.textContent = "Install & Restart";
    errEl.textContent = "Update failed — download manually at github.com/jessewallace/zync/releases";
    errEl.classList.remove("hidden");
  }
});

async function initUpdateListener() {
  await listen("update-available", (event) => {
    const { version, notes } = event.payload;
    showUpdateDialog(version, notes); // async, fire-and-forget is fine here
  });
}

// ── Init ──────────────────────────────────────────────────────

async function init() {
  await initUpdateListener();
  console.log(
    "%cZync",
    "font-size:20px;font-weight:800;color:#f76f53;font-family:'Anybody',system-ui,sans-serif;font-stretch:88%",
    "\nSync your Zen Browser profile between machines.\nBuilt with Tauri · Rust · vanilla JS."
  );

  // If no profile is saved and multiple Zen installations exist,
  // prompt the user to pick one right away
  let choseScreen = false;
  try {
    const savedPath = await invoke("get_selected_installation_cmd");
    if (!savedPath) {
      const scanResult = await invoke("scan_zen_installations_cmd");
      if (scanResult.installations.length > 1) {
        pendingInstallations = scanResult.installations;
        showInstallSelector("You have multiple Zen installations. Select which one Zync should use.", undefined, true);
        choseScreen = true;
      }
    }
  } catch (_) {
    // Silently ignore — user will be prompted on first action
  }

  if (!choseScreen) {
    showScreen("screen-main");
    showTab("sync");
  }
}

document.addEventListener("DOMContentLoaded", init);
