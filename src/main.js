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
      setStatus("Zen is still open. Please close it and try again.", "error");
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
      setStatus("Zen is still open. Please close it and try again.", "error");
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
