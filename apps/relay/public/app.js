/**
 * The mobile companion (§55–§58).
 *
 * Plain modules, no framework and no build step. Not minimalism for its own
 * sake: this page is opened on a phone, sometimes on a bad connection, to
 * answer one question — does anything need me? A framework bundle would be
 * more bytes than the entire application logic, for a screen that is three
 * lists and two buttons.
 *
 * What the companion is deliberately **not**: it is not a small Mission
 * Control, it does not stream a terminal, and it cannot run arbitrary things.
 * §56 is "watch, and answer when asked"; anything more is a remote shell with
 * a friendlier name.
 */

import { freshness } from "/freshness.js";

const KEY = "jarvis.pairing";

/** How often the phone asks the relay for news, while the page is visible. */
const POLL_MS = 15_000;

const el = document.getElementById("app");

function loadPairing() {
  try {
    const raw = localStorage.getItem(KEY);
    return raw ? JSON.parse(raw) : null;
  } catch {
    // A private window, or storage the browser refuses. Pairing simply has to
    // be done again — better than a blank page.
    return null;
  }
}

function savePairing(pairing) {
  try {
    localStorage.setItem(KEY, JSON.stringify(pairing));
  } catch {
    // Nothing to do; the session keeps working until the tab closes.
  }
}

function clearPairing() {
  try {
    localStorage.removeItem(KEY);
  } catch {}
}

// ---- Rendering --------------------------------------------------------------

const STATUS_LABEL = {
  running: "Executando",
  verifying: "Verificando",
  waiting: "Aguardando",
  blocked: "Bloqueada",
  failed: "Falhou",
};

function escape(text) {
  const node = document.createElement("span");
  node.textContent = text ?? "";
  return node.innerHTML;
}

function renderPairing(error) {
  el.innerHTML = `
    <header><h1>J.A.R.V.I.S.</h1></header>
    <p class="fresh">Conecte este aparelho ao seu desktop.</p>
    <section>
      <h2>Código de pareamento</h2>
      <input id="code" inputmode="latin" autocapitalize="characters" autocomplete="off"
             spellcheck="false" maxlength="7" placeholder="ABC123" aria-label="Código de pareamento">
      <div class="row"><button class="primary wide" id="claim">Conectar</button></div>
      ${error ? `<p class="error">${escape(error)}</p>` : ""}
      <p class="hint">Abra <strong>Configurações → Celular</strong> no J.A.R.V.I.S.
      e toque em <em>Conectar um aparelho</em>. O código vale por 5 minutos.</p>
    </section>`;

  const input = document.getElementById("code");
  const claim = document.getElementById("claim");
  input.focus();
  const submit = () => void claimCode(input.value);
  claim.addEventListener("click", submit);
  input.addEventListener("keydown", (event) => {
    if (event.key === "Enter") submit();
  });
}

function renderState(state) {
  const { snapshot, error } = state;
  const fresh = freshness(snapshot);
  const projects = snapshot?.projects ?? [];
  const approvals = snapshot?.approvals ?? [];

  const approvalCards = approvals
    .map(
      (approval) => `
      <div class="card">
        <div class="card__project">${escape(approval.projectName)}</div>
        <div class="card__title">${escape(approval.operation)}</div>
        <code>${escape(approval.summary)}</code>
        <div class="row">
          <button class="primary" data-approve="${escape(approval.id)}">Permitir uma vez</button>
        </div>
      </div>`,
    )
    .join("");

  const projectCards = projects
    .map(
      (project) => `
      <div class="card">
        <div class="card__title">${escape(project.name)}</div>
        <div class="card__meta">${project.activeSessions} sessão(ões) ativa(s)</div>
        ${project.attention
          .map(
            (mission) => `
          <div style="margin-top:10px">
            <span class="status" data-s="${escape(mission.status)}">${
              STATUS_LABEL[mission.status] ?? escape(mission.status)
            }</span>
            <span>${escape(mission.title)}</span>
            ${mission.reason ? `<div class="card__meta">${escape(mission.reason)}</div>` : ""}
          </div>`,
          )
          .join("")}
      </div>`,
    )
    .join("");

  el.innerHTML = `
    <header>
      <h1>J.A.R.V.I.S.</h1>
      <span class="device">${escape(snapshot?.deviceName ?? "")}</span>
    </header>
    <p class="fresh" ${fresh.stale ? "data-stale" : ""}>${escape(fresh.text)}</p>

    <section>
      <h2>Precisa de você</h2>
      ${
        approvals.length
          ? approvalCards
          : `<p class="empty">Nada aguardando aprovação.</p>`
      }
    </section>

    <section>
      <h2>Em andamento</h2>
      ${projectCards || `<p class="empty">Nenhum agente trabalhando agora.</p>`}
    </section>

    ${error ? `<p class="error">${escape(error)}</p>` : ""}
    <div class="row"><button id="unpair">Desconectar este aparelho</button></div>`;

  for (const button of el.querySelectorAll("[data-approve]")) {
    button.addEventListener("click", () => void approve(button.dataset.approve, button));
  }
  document.getElementById("unpair").addEventListener("click", () => {
    clearPairing();
    renderPairing(null);
  });
}

// ---- Talking to the relay ---------------------------------------------------

async function claimCode(raw) {
  const code = (raw ?? "").trim().toUpperCase().replace(/[\s-]/g, "");
  if (code.length !== 6) return renderPairing("O código tem 6 caracteres.");

  try {
    const response = await fetch("/api/pair", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ code }),
    });
    if (!response.ok) {
      // One message for every failure, matching what the relay tells us: it
      // deliberately does not distinguish "wrong", "expired" and "already
      // used", because doing so would help someone map which codes exist.
      return renderPairing("Código inválido ou expirado. Peça um novo no desktop.");
    }
    const pairing = await response.json();
    savePairing(pairing);
    void refresh();
  } catch {
    renderPairing("Não foi possível conectar. Verifique sua internet.");
  }
}

let state = { snapshot: null, error: null };

async function refresh() {
  const pairing = loadPairing();
  if (!pairing) return renderPairing(null);

  try {
    const response = await fetch(`/api/device?mailbox=${encodeURIComponent(pairing.mailboxId)}`, {
      headers: { authorization: `Bearer ${pairing.deviceToken}` },
    });
    if (response.status === 401) {
      // The desktop unpaired, or the mailbox is gone. Say so rather than
      // showing an empty screen that looks like "nothing is happening".
      clearPairing();
      return renderPairing("Este aparelho foi desconectado no desktop.");
    }
    if (!response.ok) throw new Error(String(response.status));

    const body = await response.json();
    state = { snapshot: body.snapshot, error: null };
  } catch {
    // Keep whatever was last known and say the connection failed. Blanking the
    // screen on a dropped request would lose information the person may still
    // want — and the freshness line already says how old it is.
    state = { ...state, error: "Sem conexão com o relay." };
  }
  renderState(state);
}

async function approve(approvalId, button) {
  const pairing = loadPairing();
  if (!pairing) return;

  button.disabled = true;
  button.textContent = "Enviando…";
  try {
    await fetch(`/api/device?mailbox=${encodeURIComponent(pairing.mailboxId)}`, {
      method: "POST",
      headers: {
        "content-type": "application/json",
        authorization: `Bearer ${pairing.deviceToken}`,
      },
      // A per-approval id, so a retry after a dropped response is recognised
      // as the same instruction rather than approving twice. The relay keys
      // on this.
      body: JSON.stringify({
        kind: "approve",
        id: `approve-${approvalId}`,
        approvalId,
        decision: "allow",
      }),
    });
    button.textContent = "Enviado";
  } catch {
    button.disabled = false;
    button.textContent = "Tentar de novo";
  }
}

// ---- Lifecycle --------------------------------------------------------------

void refresh();

let timer = setInterval(() => void refresh(), POLL_MS);

// Stop polling when the page is not being looked at. A companion draining a
// phone's battery in a pocket is a companion people uninstall.
document.addEventListener("visibilitychange", () => {
  clearInterval(timer);
  if (document.visibilityState === "visible") {
    void refresh();
    timer = setInterval(() => void refresh(), POLL_MS);
  }
});
