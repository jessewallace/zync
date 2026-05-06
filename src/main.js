const { invoke } = window.__TAURI__.core;

// ── Screen helpers ────────────────────────────────────────────

function showScreen(id) {
  document.querySelectorAll(".screen").forEach((s) => s.classList.remove("active"));
  document.getElementById(id).classList.add("active");
}

function setStatus(msg, type = "") {
  const el = document.getElementById("status-main");
  el.textContent = msg;
  el.className = "status" + (type ? ` ${type}` : "");
}

function setLoading(loading) {
  document.getElementById("btn-push").disabled = loading;
  document.getElementById("btn-pull").disabled = loading;
  document.getElementById("pull-input").disabled = loading;
}

// ── Countdown timer ───────────────────────────────────────────

let countdownInterval = null;

function startCountdown(seconds) {
  const el = document.getElementById("countdown");

  function tick(remaining) {
    if (remaining <= 0) {
      clearInterval(countdownInterval);
      el.textContent = "Expired";
      el.classList.add("urgent");
      return;
    }
    const m = String(Math.floor(remaining / 60)).padStart(2, "0");
    const s = String(remaining % 60).padStart(2, "0");
    el.textContent = `Expires in ${m}:${s}`;
    el.classList.toggle("urgent", remaining <= 120);
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
    // Fallback for environments where clipboard API isn't available
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
  setStatus("Checking if Zen is running…", "loading");

  try {
    const zenOpen = await invoke("is_zen_running");
    if (zenOpen) {
      setStatus("Zen Browser is still running. Quit it (⌘Q) first, then try again.", "error");
      return;
    }

    setStatus("Detecting profile…", "loading");
    const profilePath = await invoke("detect_profile_path");
    console.log("Profile path:", profilePath);

    setStatus("Pushing profile…", "loading");
    const syncCode = await invoke("push_profile");

    stopCountdown();
    document.getElementById("sync-code").textContent = syncCode;
    document.getElementById("sync-code").classList.remove("copied");
    document.getElementById("btn-copy").classList.remove("copied");
    document.getElementById("btn-copy").textContent = "Copy";
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
    setStatus("Enter a sync code (e.g. ZEN-4829)", "warning");
    input.focus();
    return;
  }
  if (!/^ZEN-[A-Z0-9]{4,8}-[A-Z0-9]{4,12}$/.test(rawCode)) {
    setStatus("Invalid code. Format is ZEN-XXXXXX-YYYYYY", "error");
    input.focus();
    return;
  }

  setLoading(true);
  setStatus("Checking if Zen is running…", "loading");

  try {
    const zenOpen = await invoke("is_zen_running");
    if (zenOpen) {
      setStatus("Zen Browser is still running. Quit it (⌘Q) first, then try again.", "error");
      return;
    }

    setStatus("Pulling profile…", "loading");
    const files = await invoke("pull_profile", { syncCode: rawCode });

    document.getElementById("pull-files").textContent = files.join("\n");
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

// Copy on click or button
function doCopy() {
  const code = document.getElementById("sync-code").textContent;
  copyText(code);

  const btn = document.getElementById("btn-copy");
  const codeEl = document.getElementById("sync-code");
  btn.textContent = "Copied!";
  btn.classList.add("copied");
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
  setStatus("");
});

document.getElementById("btn-pull-done").addEventListener("click", () => {
  showScreen("screen-main");
  document.getElementById("pull-input").value = "";
  setStatus("");
});

// Auto-format pull input into ZEN-XXXXXX-YYYYYY as user types.
document.getElementById("pull-input").addEventListener("input", (e) => {
  let raw = e.target.value.toUpperCase().replace(/[^A-Z0-9]/g, "");
  // Strip the ZEN prefix if the user typed it so we work with just the payload
  if (raw.startsWith("ZEN")) raw = raw.slice(3);
  // Reconstruct with dashes
  let out = "ZEN";
  if (raw.length > 0) out += "-" + raw.slice(0, 6);   // key half
  if (raw.length > 6) out += "-" + raw.slice(6, 18);  // file-id half (variable)
  e.target.value = out.slice(0, 20);
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

// ── Settings screen ───────────────────────────────────────────

async function loadSettingsScreen() {
  // Clear any previous action status
  document.getElementById("status-settings").textContent = "";
  document.getElementById("status-settings").className = "status";

  const paired = await invoke("get_pairing_status_cmd");
  const pairingEl = document.getElementById("pairing-status");
  if (paired) {
    pairingEl.textContent = "Paired! Automatic sync is active.";
    pairingEl.className = "pairing-status paired";
    try {
      const passphrase = await invoke("get_passphrase_cmd");
      document.getElementById("passphrase-input").value = passphrase ?? "";
    } catch (_) {
      document.getElementById("passphrase-input").value = "";
    }
  } else {
    pairingEl.textContent = "";
    pairingEl.className = "pairing-status unpaired";
    document.getElementById("passphrase-input").value = "";
  }
}

async function handleSavePassphrase() {
  const passphrase = document.getElementById("passphrase-input").value.trim();
  if (!passphrase) {
    document.getElementById("status-settings").textContent = "Enter a passphrase first.";
    document.getElementById("status-settings").className = "status error";
    return;
  }
  if (passphrase.length < 8) {
    document.getElementById("status-settings").textContent = "Passphrase must be at least 8 characters.";
    document.getElementById("status-settings").className = "status error";
    return;
  }
  try {
    await invoke("save_passphrase_cmd", { passphrase });
    await invoke("set_auto_push_cmd", { enabled: document.getElementById("toggle-auto-push").checked });
    await invoke("set_auto_pull_cmd", { enabled: document.getElementById("toggle-auto-pull").checked });
    document.getElementById("status-settings").textContent = "Paired! Automatic sync is now active.";
    document.getElementById("status-settings").className = "status success";
    await loadSettingsScreen();
  } catch (err) {
    document.getElementById("status-settings").textContent = String(err);
    document.getElementById("status-settings").className = "status error";
  }
}

async function handleForgetPassphrase() {
  try {
    await invoke("clear_passphrase_cmd");
    document.getElementById("passphrase-input").value = "";
    document.getElementById("status-settings").textContent = "Passphrase cleared. Automatic sync disabled.";
    document.getElementById("status-settings").className = "status";
    await loadSettingsScreen();
  } catch (err) {
    document.getElementById("status-settings").textContent = String(err);
    document.getElementById("status-settings").className = "status error";
  }
}

document.getElementById("btn-save-passphrase").addEventListener("click", handleSavePassphrase);
document.getElementById("btn-forget-passphrase").addEventListener("click", handleForgetPassphrase);
document.getElementById("btn-settings-done").addEventListener("click", () => {
  showScreen("screen-main");
});
document.getElementById("btn-open-settings").addEventListener("click", async () => {
  await loadSettingsScreen();
  showScreen("screen-settings");
});
document.getElementById("btn-generate").addEventListener("click", () => {
  document.getElementById("passphrase-input").value = generatePassphrase();
});

// ── First-run detection ───────────────────────────────────────

async function init() {
  const paired = await invoke("get_pairing_status_cmd");
  if (!paired) {
    await loadSettingsScreen();
    showScreen("screen-settings");
  } else {
    showScreen("screen-main");
  }
}

document.addEventListener("DOMContentLoaded", init);
