const { invoke } = window.__TAURI__.core;

// ── Screen helpers ────────────────────────────────────────────

function showScreen(id) {
  document.querySelectorAll(".screen").forEach((s) => s.classList.remove("active"));
  document.getElementById(id).classList.add("active");
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

function setPairMsg(msg, type = "neutral") {
  const el = document.getElementById("pair-msg");
  el.classList.remove("msg-neutral", "msg-error", "msg-success");
  el.classList.add(type === "error" ? "msg-error" : type === "success" ? "msg-success" : "msg-neutral");
  el.textContent = msg;
}

function setLoading(loading) {
  document.getElementById("btn-push").disabled = loading;
  document.getElementById("btn-pull").disabled = loading;
  document.getElementById("pull-input").disabled = loading;
  document.getElementById("status-main").classList.toggle("is-loading", loading);
}

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

// ── Push flow ─────────────────────────────────────────────────

async function handlePush() {
  setLoading(true);
  setStatus("Making sure Zen is closed…");

  try {
    const zenOpen = await invoke("is_zen_running");
    if (zenOpen) {
      setStatus("Zen Browser is still running. Quit it (⌘Q) first, then try again.", "error");
      return;
    }

    setStatus("Finding your profile…");
    await invoke("detect_profile_path");

    setStatus("Encrypting and uploading…");
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
    setStatus(String(err), "error");
  } finally {
    setLoading(false);
  }
}

// ── Pull flow ─────────────────────────────────────────────────

async function handlePull() {
  const input = document.getElementById("pull-input");
  const rawCode = input.value.trim().toUpperCase();

  if (!rawCode) {
    setStatus("Enter a sync code (e.g. ZEN-A3F9B2-ABC123)", "error");
    input.focus();
    return;
  }
  if (!/^ZEN-[A-Z0-9]{4,8}-[A-Z0-9]{4,12}$/.test(rawCode)) {
    setStatus("Invalid code. Format is ZEN-XXXXXX-YYYYYY", "error");
    input.focus();
    return;
  }

  setLoading(true);
  setStatus("Making sure Zen is closed…");

  try {
    const zenOpen = await invoke("is_zen_running");
    if (zenOpen) {
      setStatus("Zen Browser is still running. Quit it (⌘Q) first, then try again.", "error");
      return;
    }

    setStatus("Downloading and decrypting…");
    const files = await invoke("pull_profile", { syncCode: rawCode });

    document.getElementById("pull-files").textContent = files.join(", ");
    setStatus("");
    showScreen("screen-pull");

  } catch (err) {
    setStatus(String(err), "error");
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
    if (name === "pair") await loadPairTab();
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
  showTab("pull");
});

document.getElementById("btn-pull-done").addEventListener("click", () => {
  showScreen("screen-main");
  showTab("pull");
  document.getElementById("pull-input").value = "";
  setStatus("");
});

// Auto-format pull input into ZEN-XXXXXX-YYYYYY as user types
document.getElementById("pull-input").addEventListener("input", (e) => {
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

// ── Passphrase generator ──────────────────────────────────────

const WORDS = [
  "amber","arctic","atlas","azure","birch","blaze","bloom","brave","brook","cedar",
  "chill","cliff","cloud","coral","crane","crisp","delta","drift","dusk","eagle",
  "ember","epoch","fern","flame","fleet","flora","frost","gale","glen","grove",
  "haven","holly","ivory","jade","karma","knoll","lilac","lunar","maple","mist",
  "nexus","noble","ocean","onyx","opal","orbit","pearl","pine","prism","quest",
  "raven","reed","ridge","river","sage","scout","solar","steel","stone","swift",
  "thorn","tide","tiger","tundra","ultra","vault","viper","vivid","walnut","wheat",
  "wren","xenon","yield","zenith","zinc","acorn","bison","bluff","cobalt","crest",
];

function generatePassphrase() {
  const pick = () => WORDS[Math.floor(Math.random() * WORDS.length)];
  const num = Math.floor(Math.random() * 90) + 10;
  return `${pick()}-${pick()}-${pick()}-${num}`;
}

// ── Pair tab ──────────────────────────────────────────────────

async function loadPairTab() {
  const paired = await invoke("get_pairing_status_cmd");
  if (paired) {
    setPairMsg("Paired! Automatic sync is active.", "success");
    try {
      const passphrase = await invoke("get_passphrase_cmd");
      document.getElementById("passphrase-input").value = passphrase ?? "";
    } catch (_) {
      document.getElementById("passphrase-input").value = "";
    }
  } else {
    setPairMsg("Enter the same passphrase on each machine to enable automatic sync.");
    document.getElementById("passphrase-input").value = "";
  }
}

async function handleSavePassphrase() {
  const passphrase = document.getElementById("passphrase-input").value.trim();
  if (!passphrase) {
    setPairMsg("Enter a passphrase first.", "error");
    return;
  }
  if (passphrase.length < 8) {
    setPairMsg("Passphrase must be at least 8 characters.", "error");
    return;
  }
  try {
    await invoke("save_passphrase_cmd", { passphrase });
    await invoke("set_auto_push_cmd", { enabled: document.getElementById("toggle-auto-push").checked });
    await invoke("set_auto_pull_cmd", { enabled: document.getElementById("toggle-auto-pull").checked });
    await loadPairTab();
    const pairMsg = document.getElementById("pair-msg");
    pairMsg.classList.remove("pair-bounce");
    void pairMsg.offsetWidth;
    pairMsg.classList.add("pair-bounce");
    pairMsg.addEventListener("animationend", () => pairMsg.classList.remove("pair-bounce"), { once: true });
  } catch (err) {
    setPairMsg(String(err), "error");
  }
}

async function handleForgetPassphrase() {
  try {
    await invoke("clear_passphrase_cmd");
    document.getElementById("passphrase-input").value = "";
    await loadPairTab();
  } catch (err) {
    setPairMsg(String(err), "error");
  }
}

document.getElementById("btn-save-passphrase").addEventListener("click", handleSavePassphrase);
document.getElementById("btn-forget-passphrase").addEventListener("click", handleForgetPassphrase);
document.getElementById("btn-generate").addEventListener("click", () => {
  document.getElementById("passphrase-input").value = generatePassphrase();
  const btn = document.getElementById("btn-generate");
  btn.classList.remove("spinning");
  void btn.offsetWidth;
  btn.classList.add("spinning");
  btn.addEventListener("animationend", () => btn.classList.remove("spinning"), { once: true });
});

// ── Update dialog ─────────────────────────────────────────

const { listen } = window.__TAURI__.event;

async function showUpdateDialog(version, notes) {
  document.getElementById("update-version-number").textContent = version;
  try {
    const current = await window.__TAURI__.app.getVersion();
    document.getElementById("update-current-version").textContent = current;
  } catch (_) {
    document.getElementById("update-current-version").textContent = "";
  }
  document.getElementById("update-notes").textContent = notes || "No release notes provided.";
  document.getElementById("update-error").classList.add("hidden");
  document.getElementById("btn-update-install").disabled = false;
  document.getElementById("btn-update-install").textContent = "Install & Restart";
  document.getElementById("update-dialog").classList.remove("hidden");
}

function hideUpdateDialog() {
  document.getElementById("update-dialog").classList.add("hidden");
}

document.getElementById("btn-update-later").addEventListener("click", hideUpdateDialog);

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
    "font-size:20px;font-weight:700;color:#f76f53;font-family:'Bricolage Grotesque',system-ui,sans-serif",
    "\nSync your Zen Browser profile between machines.\nBuilt with Tauri · Rust · vanilla JS."
  );
  const paired = await invoke("get_pairing_status_cmd");
  showScreen("screen-main");
  if (!paired) {
    await loadPairTab();
    showTab("pair");
  } else {
    showTab("pull");
  }
}

document.addEventListener("DOMContentLoaded", init);
