const state = {
  tab: "library",
  libraryPath: "",
  status: null,
  queue: { items: [] },
  library: { path: "", directories: [], tracks: [] },
  pendingQueueSelection: null,
  queueSelectionRequestId: 0,
};

const el = {};

document.addEventListener("DOMContentLoaded", () => {
  cacheElements();
  bindEvents();
  refreshAll();
  setInterval(refreshStatus, 2500);
});

function cacheElements() {
  for (const id of [
    "networkLine",
    "serviceState",
    "playbackState",
    "trackTitle",
    "trackMeta",
    "trackTime",
    "volumeInput",
    "volumeValue",
    "libraryPath",
    "libraryList",
    "queueCount",
    "queueList",
    "detailTitle",
    "detailArtist",
    "detailAlbum",
    "detailPath",
    "toast",
    "upButton",
    "rescanButton",
    "clearQueueButton",
    "previousButton",
    "toggleButton",
    "nextButton",
    "stopButton",
  ]) {
    el[id] = document.getElementById(id);
  }
  el.tabs = Array.from(document.querySelectorAll("[data-tab]"));
  el.views = {
    library: document.getElementById("libraryView"),
    queue: document.getElementById("queueView"),
    now: document.getElementById("nowView"),
  };
}

function bindEvents() {
  for (const button of el.tabs) {
    button.addEventListener("click", () => setTab(button.dataset.tab));
  }

  el.upButton.addEventListener("click", () => openLibrary(parentPath(state.libraryPath)));
  el.rescanButton.addEventListener("click", () => perform(async () => {
    await postJson("/api/v1/library/rescan");
    showToast("Scan started");
  }));
  el.clearQueueButton.addEventListener("click", () => perform(async () => {
    await sendRequest("/api/v1/queue", { method: "DELETE" });
    showToast("Queue cleared");
    await Promise.all([refreshStatus(), refreshQueue()]);
  }));

  el.previousButton.addEventListener("click", () => perform(() => playback("previous")));
  el.toggleButton.addEventListener("click", () => perform(() => playback("toggle")));
  el.nextButton.addEventListener("click", () => perform(() => playback("next")));
  el.stopButton.addEventListener("click", () => perform(() => playback("stop")));

  el.volumeInput.addEventListener("change", () => perform(async () => {
    const level = Number(el.volumeInput.value);
    await postJson("/api/v1/playback/volume", { level });
    await refreshStatus();
  }));
  el.volumeInput.addEventListener("input", () => {
    el.volumeValue.textContent = el.volumeInput.value;
  });
}

async function refreshAll() {
  await Promise.all([refreshStatus(), refreshLibrary(), refreshQueue()]);
}

async function refreshStatus() {
  try {
    state.status = await getJson("/api/v1/status");
    renderStatus();
    reconcilePendingQueueSelection();
    if (state.tab === "queue") {
      renderQueue();
    }
  } catch (error) {
    showToast(error.message);
  }
}

async function refreshLibrary() {
  try {
    const suffix = state.libraryPath
      ? `?path=${encodeURIComponent(state.libraryPath)}`
      : "";
    state.library = await getJson(`/api/v1/library/list${suffix}`);
    renderLibrary();
  } catch (error) {
    renderEmpty(el.libraryList, "Library unavailable");
    showToast(error.message);
  }
}

async function refreshQueue() {
  try {
    state.queue = await getJson("/api/v1/queue");
    renderQueue();
  } catch (error) {
    renderEmpty(el.queueList, "Queue unavailable");
    showToast(error.message);
  }
}

function renderStatus() {
  const status = state.status;
  if (!status) {
    return;
  }

  el.serviceState.textContent = status.service?.state ?? "unknown";

  const network = status.network;
  if (network) {
    const connection = network.mode === "car"
      ? network.hotspot_ssid
      : network.active_connection;
    el.networkLine.textContent = `${network.mode} - ${connection} - ${network.ip4}`;
  } else {
    el.networkLine.textContent = "Network unavailable";
  }

  const playback = status.playback;
  if (!playback) {
    el.playbackState.textContent = "unavailable";
    el.trackTitle.textContent = "MPD unavailable";
    el.trackMeta.textContent = "-";
    el.trackTime.textContent = "0:00 / 0:00";
    el.toggleButton.textContent = "Play";
    return;
  }

  const track = playback.track;
  el.playbackState.textContent = playback.state;
  el.toggleButton.textContent = playback.state === "play" ? "Pause" : "Play";
  el.volumeInput.value = playback.volume;
  el.volumeValue.textContent = playback.volume;

  if (track) {
    const title = track.title || basename(track.uri);
    el.trackTitle.textContent = title;
    el.trackMeta.textContent = [track.artist, track.album].filter(Boolean).join(" - ") || track.uri;
    el.trackTime.textContent = `${formatSeconds(track.elapsed_s)} / ${formatSeconds(track.duration_s)}`;
    el.detailTitle.textContent = title;
    el.detailArtist.textContent = track.artist || "-";
    el.detailAlbum.textContent = track.album || "-";
    el.detailPath.textContent = track.uri;
  } else {
    el.trackTitle.textContent = "No track";
    el.trackMeta.textContent = playback.queue_length ? "Queue ready" : "Queue is empty";
    el.trackTime.textContent = "0:00 / 0:00";
    el.detailTitle.textContent = "No track";
    el.detailArtist.textContent = "-";
    el.detailAlbum.textContent = "-";
    el.detailPath.textContent = "-";
  }
}

function renderLibrary() {
  const listing = state.library;
  state.libraryPath = listing.path || "";
  el.libraryPath.textContent = state.libraryPath || "Root";
  el.upButton.disabled = !state.libraryPath;
  el.libraryList.replaceChildren();

  if (!listing.directories.length && !listing.tracks.length) {
    renderEmpty(el.libraryList, "Empty");
    return;
  }

  for (const directory of listing.directories) {
    el.libraryList.appendChild(libraryRow({
      title: directory.name,
      meta: "Directory",
      open: () => openLibrary(directory.path),
      add: () => perform(() => enqueue("directories", directory.path, directory.name)),
    }));
  }

  for (const track of listing.tracks) {
    el.libraryList.appendChild(libraryRow({
      title: track.title || track.name,
      meta: formatTrackMeta(track),
      open: null,
      add: () => perform(() => enqueue("files", track.uri, track.title || track.name)),
    }));
  }
}

function renderQueue() {
  const items = state.queue.items || [];
  el.queueCount.textContent = `${items.length} ${items.length === 1 ? "item" : "items"}`;
  el.clearQueueButton.disabled = items.length === 0;
  el.queueList.replaceChildren();

  if (!items.length) {
    renderEmpty(el.queueList, "Empty");
    return;
  }

  for (const item of items) {
    const row = document.createElement("div");
    row.className = "list-row queue-row";
    if (isCurrentQueueItem(item)) {
      row.classList.add("current");
      row.setAttribute("aria-current", "true");
    }

    const main = document.createElement("button");
    main.className = "list-row-main queue-play-button";
    main.type = "button";
    main.addEventListener("click", () => perform(() => playQueueItem(item)));

    const title = document.createElement("span");
    title.className = "row-title";
    title.textContent = `${item.position + 1}. ${item.title || item.name}`;

    const meta = document.createElement("span");
    meta.className = "row-meta";
    meta.textContent = formatTrackMeta(item) || item.uri;

    main.append(title, meta);

    const actions = document.createElement("div");
    actions.className = "queue-actions";
    actions.append(
      queueActionButton("Up", item.position === 0, () => moveQueueItem(item, item.position - 1)),
      queueActionButton("Down", item.position >= items.length - 1, () => moveQueueItem(item, item.position + 1)),
      queueActionButton("Next", !canMoveAfterCurrent(item), () => moveQueueItemNext(item)),
      queueActionButton("Del", false, () => removeQueueItem(item)),
    );

    row.append(main, actions);
    el.queueList.appendChild(row);
  }
}

function libraryRow({ title, meta, open, add }) {
  const row = document.createElement("div");
  row.className = open ? "list-row library-row openable" : "list-row library-row";
  if (open) {
    row.tabIndex = 0;
    row.setAttribute("role", "button");
    row.setAttribute("aria-label", `Open ${title}`);
    row.addEventListener("click", open);
    row.addEventListener("keydown", (event) => {
      if (event.target !== row) {
        return;
      }
      if (event.key === "Enter" || event.key === " ") {
        event.preventDefault();
        open();
      }
    });
  }

  const main = document.createElement("div");
  main.className = "list-row-main";

  const titleEl = document.createElement("span");
  titleEl.className = "row-title";
  titleEl.textContent = title;

  const metaEl = document.createElement("span");
  metaEl.className = "row-meta";
  metaEl.textContent = meta || "-";

  main.append(titleEl, metaEl);

  const addButton = document.createElement("button");
  addButton.className = "add-button";
  addButton.type = "button";
  addButton.textContent = "Add";
  addButton.addEventListener("click", (event) => {
    event.stopPropagation();
    add();
  });

  row.append(main, addButton);
  return row;
}

async function openLibrary(path) {
  state.libraryPath = path;
  await refreshLibrary();
}

async function enqueue(kind, path, label) {
  await postJson(`/api/v1/queue/${kind}`, { path });
  showToast(`Added ${label}`);
  await Promise.all([refreshStatus(), refreshQueue()]);
}

async function playQueueItem(item) {
  const requestId = ++state.queueSelectionRequestId;
  const selection = {
    ...queueSelectionFor(item),
    requestId,
  };
  state.pendingQueueSelection = selection;
  renderQueue();
  scheduleQueueSelectionReconcile(requestId);

  try {
    await postJson("/api/v1/queue/play", queuePlayBody(selection));
    if (isLatestQueueSelection(requestId)) {
      showToast(`Playing ${item.title || item.name}`);
      await refreshStatus();
      await refreshQueue();
    }
  } catch (error) {
    if (isLatestQueueSelection(requestId)) {
      state.pendingQueueSelection = null;
      renderQueue();
      await refreshStatus();
    }
    throw error;
  }
}

async function removeQueueItem(item) {
  await postJson("/api/v1/queue/remove", queueItemTargetBody(item));
  showToast(`Removed ${item.title || item.name}`);
  clearPendingSelectionFor(item);
  await Promise.all([refreshStatus(), refreshQueue()]);
}

async function moveQueueItem(item, toPosition) {
  if (toPosition < 0) {
    return;
  }
  await postJson("/api/v1/queue/move", {
    ...queueItemTargetBody(item),
    to_position: toPosition,
  });
  showToast(`Moved ${item.title || item.name}`);
  await Promise.all([refreshStatus(), refreshQueue()]);
}

async function moveQueueItemNext(item) {
  await postJson("/api/v1/queue/play-next", queueItemTargetBody(item));
  showToast(`Queued next ${item.title || item.name}`);
  await Promise.all([refreshStatus(), refreshQueue()]);
}

async function playback(action) {
  await postJson(`/api/v1/playback/${action}`);
  await refreshStatus();
}

async function perform(task) {
  try {
    await task();
  } catch (error) {
    showToast(error.message);
  }
}

function setTab(tab) {
  state.tab = tab;
  for (const button of el.tabs) {
    button.classList.toggle("active", button.dataset.tab === tab);
  }
  for (const [name, view] of Object.entries(el.views)) {
    view.classList.toggle("active", name === tab);
  }
  if (tab === "queue") {
    refreshQueue();
  }
}

async function getJson(path) {
  const response = await sendRequest(path);
  return response.json();
}

async function postJson(path, body) {
  const options = { method: "POST" };
  if (body !== undefined) {
    options.headers = { "content-type": "application/json" };
    options.body = JSON.stringify(body);
  }
  return sendRequest(path, options);
}

async function sendRequest(path, options = {}) {
  const response = await fetch(path, options);
  if (response.ok) {
    return response;
  }
  let message = `${options.method || "GET"} ${path} failed`;
  try {
    const payload = await response.json();
    if (payload.error) {
      message = payload.error;
    }
  } catch (_error) {
    message = `${message}: ${response.status}`;
  }
  throw new Error(message);
}

function renderEmpty(container, label) {
  container.replaceChildren();
  const empty = document.createElement("div");
  empty.className = "empty";
  empty.textContent = label;
  container.appendChild(empty);
}

function formatTrackMeta(track) {
  const bits = [];
  if (track.artist) {
    bits.push(track.artist);
  }
  if (track.album) {
    bits.push(track.album);
  }
  if (track.duration_s !== undefined && track.duration_s !== null) {
    bits.push(formatSeconds(track.duration_s));
  }
  return bits.join(" - ");
}

function parentPath(path) {
  if (!path) {
    return "";
  }
  const parts = path.split("/").filter(Boolean);
  parts.pop();
  return parts.join("/");
}

function basename(path) {
  const parts = path.split("/").filter(Boolean);
  return parts[parts.length - 1] || path;
}

function isCurrentQueueItem(item) {
  if (queueSelectionMatchesItem(state.pendingQueueSelection, item)) {
    return true;
  }

  const playback = state.status?.playback;
  if (!playback) {
    return false;
  }
  if (playback.queue_id !== undefined && playback.queue_id !== null && item.id !== undefined && item.id !== null) {
    return Number(playback.queue_id) === Number(item.id);
  }
  if (playback.queue_position !== undefined && playback.queue_position !== null) {
    return Number(playback.queue_position) === Number(item.position);
  }
  return playback.track?.uri === item.uri;
}

function queueSelectionFor(item) {
  return {
    id: item.id ?? null,
    position: item.position,
    uri: item.uri,
  };
}

function queuePlayBody(selection) {
  const body = { position: selection.position };
  if (selection.id !== null) {
    body.id = selection.id;
  }
  return body;
}

function queueItemTargetBody(item) {
  const body = { position: item.position };
  if (item.id !== undefined && item.id !== null) {
    body.id = item.id;
  }
  return body;
}

function queueActionButton(label, disabled, action) {
  const button = document.createElement("button");
  button.className = label === "Del" ? "queue-action danger" : "queue-action";
  button.type = "button";
  button.textContent = label;
  button.disabled = disabled;
  button.addEventListener("click", (event) => {
    event.stopPropagation();
    if (!button.disabled) {
      perform(action);
    }
  });
  return button;
}

function canMoveAfterCurrent(item) {
  const playback = state.status?.playback;
  if (!playback || playback.queue_position === undefined || playback.queue_position === null) {
    return false;
  }
  return !isCurrentQueueItem(item);
}

function clearPendingSelectionFor(item) {
  if (queueSelectionMatchesItem(state.pendingQueueSelection, item)) {
    state.pendingQueueSelection = null;
  }
}

function queueSelectionMatchesItem(selection, item) {
  if (!selection) {
    return false;
  }
  if (selection.id !== null && item.id !== undefined && item.id !== null) {
    return Number(selection.id) === Number(item.id);
  }
  if (selection.position !== undefined && selection.position !== null) {
    return Number(selection.position) === Number(item.position);
  }
  return selection.uri === item.uri;
}

function reconcilePendingQueueSelection() {
  const selection = state.pendingQueueSelection;
  const playback = state.status?.playback;
  if (!selection || !playback) {
    return;
  }
  if (queueSelectionMatchesPlayback(selection, playback)) {
    state.pendingQueueSelection = null;
  }
}

function queueSelectionMatchesPlayback(selection, playback) {
  if (
    selection.id !== null &&
    playback.queue_id !== undefined &&
    playback.queue_id !== null
  ) {
    return Number(selection.id) === Number(playback.queue_id);
  }
  if (
    selection.position !== undefined &&
    selection.position !== null &&
    playback.queue_position !== undefined &&
    playback.queue_position !== null
  ) {
    return Number(selection.position) === Number(playback.queue_position);
  }
  return playback.track?.uri === selection.uri;
}

function isLatestQueueSelection(requestId) {
  return state.pendingQueueSelection?.requestId === requestId;
}

function scheduleQueueSelectionReconcile(requestId) {
  window.setTimeout(async () => {
    if (!isLatestQueueSelection(requestId)) {
      return;
    }
    await refreshStatus();
    if (isLatestQueueSelection(requestId)) {
      state.pendingQueueSelection = null;
      renderQueue();
    }
  }, 1800);
}

function formatSeconds(value) {
  const total = Math.max(0, Number(value || 0));
  const minutes = Math.floor(total / 60);
  const seconds = Math.floor(total % 60).toString().padStart(2, "0");
  return `${minutes}:${seconds}`;
}

let toastTimer = null;

function showToast(message) {
  el.toast.textContent = message;
  el.toast.classList.add("visible");
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => {
    el.toast.classList.remove("visible");
  }, 2200);
}
