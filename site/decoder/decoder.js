// The decoder page: everything here is presentation. Loading, checking and
// decoding are done by the bite decoder in ../pkg (built by `make wasm`).

const $ = (id) => document.getElementById(id);
const HUES = 8;
const MAX_FILE_BYTES = 4 * 1024 * 1024;
// A dictionary is whatever file the visitor was handed. These bound what one
// can make this page read and draw: the decoder's own limit on files per
// dictionary, and the bytes the bit view draws (eight cells each).
const MAX_FILES = 10000;
const MAX_VIEW_BYTES = 1024;

// Set once the WebAssembly module is loaded.
let loadDictionary = null;

const state = {
  dictionaries: [],
  current: null,
  vector: null, // the selected example payload, while the inputs still hold it
  view: null, // what the bit view and the table are showing
  columns: 32, // bits per row of the bit view
  // The field being shown is the one reached with the keyboard, else the one
  // under the pointer, else the one pinned by a click or a tap.
  focused: null,
  hovered: null,
  pinned: null,
};

function shownField() {
  return state.focused ?? state.hovered ?? state.pinned;
}

function element(tag, properties = {}, children = []) {
  const node = document.createElement(tag);
  for (const [key, value] of Object.entries(properties)) {
    if (key === "class") node.className = value;
    else if (key === "text") node.textContent = value;
    else if (key === "style") node.style.cssText = value;
    else node.setAttribute(key, value);
  }
  for (const child of children) node.append(child);
  return node;
}

// ---------------------------------------------------------------- dictionaries

function openDictionary(id, label, files) {
  const handle = loadDictionary(files); // throws what `bite` would print
  const entry = {
    id,
    label,
    handle,
    info: handle.info(),
    interfaces: handle.interfaces(),
    issues: handle.issues(),
    vectors: handle.vectors().filter((vector) => !vector.error),
    results: handle.runVectors(),
  };
  const messages = entry.interfaces.reduce(
    (count, iface) => count + iface.messages.filter((message) => message.defined).length,
    0,
  );
  const errors = entry.issues.filter((issue) => issue.severity === "error").length;
  const passed = entry.results.filter((result) => result.passed).length;
  // The line `bite check` ends with.
  entry.summary =
    `${entry.interfaces.length} interfaces, ${messages} messages, ${errors} errors, ` +
    `${entry.issues.length - errors} warnings, ${passed}/${entry.results.length} vectors passed`;
  return entry;
}

function choice(group, value, label, checked) {
  const input = element("input", { type: "radio", name: group, value });
  input.checked = checked;
  return element("label", { class: "choice" }, [input, element("span", { text: label })]);
}

function renderDictionaryChoices() {
  $("dictionary-choices").replaceChildren(
    ...state.dictionaries.map((entry) =>
      choice("dictionary", entry.id, entry.label, entry === state.current),
    ),
  );
}

function selectDictionary(entry) {
  state.current = entry;
  renderDictionaryChoices();
  $("dictionary-description").textContent = entry.info.description || "";
  $("dictionary-description").hidden = !entry.info.description;
  $("dictionary-summary").textContent = entry.summary;

  const failures = entry.results
    .filter((result) => !result.passed)
    .flatMap((result) =>
      result.failures.map((failure) => ({
        severity: "error",
        text: `vector ${result.name}: ${failure}`,
      })),
    );
  const lines = [...entry.issues, ...failures];
  $("dictionary-issues").replaceChildren(
    ...lines.map((issue) => element("li", { class: issue.severity, text: issue.text })),
  );
  $("dictionary-issues").hidden = lines.length === 0;

  $("interface").replaceChildren(
    ...entry.interfaces.map((iface) => element("option", { value: iface.name, text: iface.name })),
  );
  $("interface-field").hidden = entry.interfaces.length < 2;

  $("vector-choices").replaceChildren(
    ...entry.vectors.map((vector) => choice("vector", vector.name, vector.name, false)),
  );
  $("vector-set").hidden = entry.vectors.length === 0;

  if (entry.vectors.length > 0) {
    selectVector(entry.vectors[0]);
  } else {
    state.vector = null;
    $("payload").value = "";
    $("frame-id").value = "";
    interfaceChanged();
    decode();
  }
}

function currentInterface() {
  const name = $("interface").value;
  return state.current.interfaces.find((iface) => iface.name === name) || null;
}

function interfaceChanged() {
  const iface = currentInterface();
  $("frame-field").hidden = !iface || iface.message_type.source !== "frame_id";
}

function spaced(hex) {
  return hex.replace(/\s+/g, "").replace(/(..)(?=.)/g, "$1 ");
}

function selectVector(vector) {
  state.vector = vector;
  for (const input of document.querySelectorAll('input[name="vector"]')) {
    input.checked = input.value === vector.name;
  }
  if (state.current.interfaces.some((iface) => iface.name === vector.interface)) {
    $("interface").value = vector.interface;
  }
  $("payload").value = spaced(vector.payload_hex);
  $("frame-id").value = vector.frame_id == null ? "" : `0x${vector.frame_id.toString(16)}`;
  interfaceChanged();
  decode();
}

// The example is no longer what is being decoded once an input is edited.
function inputsEdited() {
  const vector = state.vector;
  if (vector) {
    const same =
      $("payload").value.replace(/\s+/g, "").toLowerCase() ===
        vector.payload_hex.replace(/\s+/g, "").toLowerCase() &&
      $("interface").value === vector.interface &&
      parseFrameId($("frame-id").value) === (vector.frame_id ?? undefined);
    if (!same) {
      state.vector = null;
      for (const input of document.querySelectorAll('input[name="vector"]')) input.checked = false;
    }
  }
  decode();
}

// -------------------------------------------------------------------- decoding

function parseFrameId(text) {
  const trimmed = text.trim();
  if (trimmed === "") return undefined;
  if (/^0x[0-9a-f]+$/i.test(trimmed) || /^[0-9]+$/.test(trimmed)) return Number(trimmed);
  return NaN;
}

function parseHex(text) {
  const compact = text.replace(/\s+/g, "");
  if (compact.length % 2 !== 0 || /[^0-9a-f]/i.test(compact)) return null;
  const bytes = new Uint8Array(compact.length / 2);
  for (let i = 0; i < bytes.length; i += 1) bytes[i] = parseInt(compact.substr(i * 2, 2), 16);
  return bytes;
}

function decode() {
  const entry = state.current;
  const iface = currentInterface();
  const text = $("payload").value;
  const usesFrameId = iface && iface.message_type.source === "frame_id";
  const frameId = usesFrameId ? parseFrameId($("frame-id").value) : undefined;

  let result = null;
  let error = null;
  if (!iface) {
    error = "this dictionary has no interface to decode against";
  } else if (text.trim() === "") {
    // Nothing to decode yet; not an error.
  } else if (Number.isNaN(frameId)) {
    error = "frame identifier: write it in decimal or as 0x and hexadecimal digits";
  } else {
    try {
      result = entry.handle.decode(iface.name, text, frameId);
    } catch (thrown) {
      error = thrown.message || String(thrown);
    }
  }

  const bytes = parseHex(text);
  const message =
    result && iface.messages.find((m) => m.name === result.message_type && m.defined);
  state.view = { iface, bytes, result, message: message || null };
  state.pinned = null;
  state.hovered = null;
  state.focused = null;

  $("error").textContent = error || "";
  $("error").hidden = !error;
  renderHeading(error);
  renderBits();
  renderTable();
  renderInspector();
}

function renderHeading(error) {
  const { iface, bytes, result, message } = state.view;
  const title = $("result-title");
  const sub = $("result-sub");
  sub.replaceChildren();
  if (!result) {
    title.textContent = error ? "Not decoded" : "Paste a payload";
    sub.textContent = error
      ? ""
      : "Hexadecimal digits, with or without spaces, for one message of this interface.";
    return;
  }
  title.textContent = result.message_type;
  const parts = [iface.name];
  if (iface.port != null) parts.push(`port ${iface.port}`);
  parts.push(`${bytes.length} bytes`);
  if (message && message.description) parts.push(message.description);
  if (!message) {
    const named = iface.messages.some((m) => m.name === result.message_type);
    parts.push(
      named
        ? "the dictionary names this message and has no file for it, so no field is decoded"
        : "no message of this interface has this identifier, so no field is decoded",
    );
  }
  sub.append(parts.join(" · "));
  if (state.vector) {
    const outcome = state.current.results.find((r) => r.name === state.vector.name);
    if (outcome) {
      sub.append(
        " ",
        element("span", {
          class: outcome.passed ? "badge" : "badge crit",
          text: outcome.passed
            ? "matches the values its test vector expects"
            : "differs from its test vector",
        }),
      );
    }
  }
}

// -------------------------------------------------------------------- bit view

function bitAt(bytes, index, order) {
  const byte = bytes[index >> 3];
  return order === "lsb0" ? (byte >> (index & 7)) & 1 : (byte >> (7 - (index & 7))) & 1;
}

// A field's place on the grid below the bits: fields that overlap go on
// separate rows.
function assignLanes(fields) {
  const ends = [];
  return fields.map((field) => {
    let lane = ends.findIndex((end) => end <= field.start);
    if (lane === -1) lane = ends.length;
    ends[lane] = field.stop;
    return lane;
  });
}

function renderBits() {
  const container = $("bits");
  const { iface, bytes, message } = state.view;
  $("view").hidden = !bytes || bytes.length === 0;
  if (!bytes || bytes.length === 0) {
    container.replaceChildren();
    return;
  }
  const order = iface.bit_order;
  const fields = message ? message.fields : [];
  const lanes = assignLanes(fields);
  const payloadBits = bytes.length * 8;
  const wantedBytes = Math.max(bytes.length, Math.ceil((message ? message.bit_length : 0) / 8));
  const totalBytes = Math.min(wantedBytes, MAX_VIEW_BYTES);
  const totalBits = totalBytes * 8;

  const owner = new Array(totalBits).fill(-1);
  fields.forEach((field, index) => {
    for (let bit = field.start; bit < field.stop && bit < totalBits; bit += 1) {
      if (owner[bit] === -1) owner[bit] = index;
    }
  });
  const disc = iface.message_type;
  const isDisc = (bit) => disc.source === "payload" && bit >= disc.start && bit < disc.stop;

  const perRow = state.columns;
  const bytesPerRow = perRow / 8;
  const template = new Array(bytesPerRow).fill("repeat(8, minmax(0, 1fr))").join(" 6px ");
  // Display position within a row to grid column: a 6px track follows each byte.
  const column = (position) => Math.floor(position / 8) * 9 + (position % 8) + 1;

  const rows = [];
  let anyMissing = false;
  for (let firstByte = 0; firstByte < totalBytes; firstByte += bytesPerRow) {
    const row = element("div", { class: "bit-row", style: `grid-template-columns: ${template}` });
    const count = Math.min(bytesPerRow, totalBytes - firstByte) * 8;
    // Bits are drawn most significant first within each byte, whichever way
    // the interface numbers them.
    const bitIndex = (position) => {
      const byte = firstByte + Math.floor(position / 8);
      const within = position % 8;
      return byte * 8 + (order === "lsb0" ? 7 - within : within);
    };

    for (let b = 0; b < count / 8; b += 1) {
      const index = firstByte + b;
      const present = index < bytes.length;
      row.append(
        element("div", { class: "byte-head", style: `grid-column: ${b * 9 + 1} / span 8` }, [
          element("span", { class: "offset", text: `byte ${index}` }),
          element("span", {
            class: "hex",
            text: present ? bytes[index].toString(16).padStart(2, "0") : "—",
          }),
        ]),
      );
    }

    for (let position = 0; position < count; position += 1) {
      const bit = bitIndex(position);
      const field = owner[bit];
      const present = bit < payloadBits;
      const value = present ? bitAt(bytes, bit, order) : null;
      const classes = ["bit"];
      if (field === -1) classes.push("none");
      if (!present) {
        classes.push("miss");
        anyMissing = true;
      } else if (value === 0) classes.push("zero");
      if (isDisc(bit)) classes.push("disc");
      const before = position > 0 ? owner[bitIndex(position - 1)] : -2;
      const after = position < count - 1 ? owner[bitIndex(position + 1)] : -2;
      if (field !== -1 && before !== field) classes.push("first");
      if (field !== -1 && after !== field) classes.push("last");
      const cell = element("span", {
        class: classes.join(" "),
        style: `grid-column: ${column(position)}` + (field === -1 ? "" : `; --c: var(--c${field % HUES})`),
        text: present ? String(value) : "·",
        "data-bit": bit,
      });
      if (field !== -1) cell.dataset.f = field;
      row.append(cell);
    }

    fields.forEach((field, index) => {
      let run = null;
      const flush = () => {
        if (!run) return;
        const length = run.to - run.from + 1;
        const segment = element("button", {
          type: "button",
          tabindex: "-1",
          "aria-hidden": "true",
          class: length < 3 ? "seg tight" : "seg",
          style:
            `grid-column: ${column(run.from)} / ${column(run.to) + 1}; ` +
            `grid-row: ${3 + lanes[index]}; --c: var(--c${index % HUES})`,
          title: field.name,
          text: field.name,
        });
        segment.dataset.f = index;
        row.append(segment);
        run = null;
      };
      for (let position = 0; position < count; position += 1) {
        const bit = bitIndex(position);
        if (bit >= field.start && bit < field.stop) {
          if (run) run.to = position;
          else run = { from: position, to: position };
        } else flush();
      }
      flush();
    });
    rows.push(row);
  }
  container.replaceChildren(...rows);
  container.setAttribute(
    "aria-label",
    `The payload bit by bit: ${bytes.length} bytes` +
      (message ? `, ${fields.length} fields. The table below lists each field and its bits.` : "."),
  );

  const note = $("bits-note");
  note.replaceChildren();
  if (wantedBytes > totalBytes) {
    note.append(
      element("span", {
        class: "numbering",
        text: `The first ${totalBytes} of ${wantedBytes} bytes are drawn. The table lists every field.`,
      }),
    );
  }
  if (disc.source === "payload") {
    note.append(
      element("span", { class: "key" }, [
        element("i", { class: "disc" }),
        `bits [${disc.start}, ${disc.stop}) identify the message`,
      ]),
    );
  }
  if (anyMissing) {
    note.append(
      element("span", { class: "key" }, [
        element("i", { class: "miss" }),
        "in the message, beyond the end of this payload",
      ]),
    );
  }
  note.append(
    element("span", {
      class: "numbering",
      text:
        order === "lsb0"
          ? "Bit numbering lsb0: bit 0 is the rightmost bit of byte 0, so a field that crosses a byte continues at the right of the next byte."
          : "Bit numbering msb0: bit 0 is the leftmost bit of byte 0.",
    }),
  );
}

// ----------------------------------------------------------------------- table

function formatValue(value) {
  return value === null || value === undefined ? null : String(value);
}

function describeKind(field, order) {
  let text = field.type;
  if (field.scale != null) text += ` × ${field.scale}`;
  if (field.offset != null) text += field.offset < 0 ? ` − ${-field.offset}` : ` + ${field.offset}`;
  if (field.byte_order === "little" && order === "msb0") text += ", little-endian";
  if (field.labelled) text += ", labelled";
  return text;
}

function renderTable() {
  const { iface, result, message } = state.view;
  const body = $("fields").tBodies[0];
  const fields = message ? message.fields : [];
  $("fields").hidden = fields.length === 0;
  const note = $("no-fields");
  note.hidden = !(result && fields.length === 0);
  note.textContent = note.hidden ? "" : "No fields: the bits above are shown as they arrived.";

  body.replaceChildren(
    ...fields.map((field, index) => {
      const value = formatValue(result.fields[field.name]);
      const button = element(
        "button",
        { type: "button", class: "field-button", "aria-pressed": "false" },
        [element("span", { class: "swatch" }), element("span", { text: field.name })],
      );
      button.dataset.f = index;
      const row = element("tr", { style: `--c: var(--c${index % HUES})` }, [
        element("td", {}, [button]),
        element("td", { class: value === null ? "value null" : "value" }, [
          value === null ? "beyond the payload" : value,
          // Shown in place of the unit column where the table is narrow.
          ...(value !== null && field.unit
            ? [" ", element("span", { class: "unit-inline", "aria-hidden": "true", text: field.unit })]
            : []),
        ]),
        element("td", { class: "unit", text: field.unit || "" }),
        element("td", { class: "bitrange", text: `[${field.start}, ${field.stop})` }),
        element("td", { class: "kind", text: describeKind(field, iface.bit_order) }),
      ]);
      row.dataset.f = index;
      return row;
    }),
  );
}

// ------------------------------------------------------------------- highlight

function rawBits(field) {
  const { iface, bytes } = state.view;
  const width = field.stop - field.start;
  if (field.stop > bytes.length * 8) return null;
  const digits = [];
  for (let i = 0; i < width; i += 1) {
    // Most significant bit first.
    const bit = iface.bit_order === "lsb0" ? field.stop - 1 - i : field.start + i;
    digits.push(bitAt(bytes, bit, iface.bit_order));
  }
  const binary = digits.join("");
  const hex = BigInt(`0b${binary}`).toString(16);
  if (width > 32) return `0x${hex.padStart(Math.ceil(width / 4), "0")}`;
  // Groups of four, counted from the least significant bit.
  const grouped = binary.replace(/\B(?=(?:.{4})+$)/g, " ");
  return `${grouped} = 0x${hex}`;
}

function renderInspector() {
  const inspector = $("inspector");
  const { result, message } = state.view;
  const index = shownField();
  if (index == null || !message) {
    inspector.className = "inspector idle";
    inspector.style.removeProperty("--c");
    inspector.textContent =
      message && message.fields.length > 0
        ? "Point at a field or a bit, or tap it, to see which bits the field is read from."
        : "";
    return;
  }
  const field = message.fields[index];
  const value = formatValue(result.fields[field.name]);
  const width = field.stop - field.start;
  inspector.className = "inspector on";
  inspector.style.setProperty("--c", `var(--c${index % HUES})`);
  const raw = rawBits(field);
  inspector.replaceChildren(
    element("span", { class: "name", text: field.name }),
    ` = ${value === null ? "nothing: the payload ends before this field" : value}` +
      (value !== null && field.unit ? ` ${field.unit}` : ""),
    element("span", {
      class: "raw",
      text:
        `bits [${field.start}, ${field.stop}) · ${width} ${width === 1 ? "bit" : "bits"} · ` +
        describeKind(field, state.view.iface.bit_order) +
        (raw ? ` · raw ${raw}` : ""),
    }),
    ...(field.description ? [element("span", { class: "raw", text: field.description })] : []),
  );
}

function applyHighlight() {
  const index = shownField();
  const { message } = state.view;
  const field = index != null && message ? message.fields[index] : null;
  const container = $("bits");
  container.classList.toggle("focus", field !== null);
  for (const cell of container.querySelectorAll(".bit")) {
    const bit = Number(cell.dataset.bit);
    const on = field !== null && bit >= field.start && bit < field.stop;
    cell.classList.toggle("on", on);
    // A bit two fields share takes the colour of the one being shown.
    if (cell.dataset.f !== undefined) {
      cell.style.setProperty("--c", `var(--c${(on ? index : Number(cell.dataset.f)) % HUES})`);
    }
  }
  for (const segment of container.querySelectorAll(".seg")) {
    segment.classList.toggle("on", field !== null && Number(segment.dataset.f) === index);
  }
  for (const row of $("fields").tBodies[0].rows) {
    const on = field !== null && Number(row.dataset.f) === index;
    row.classList.toggle("on", on);
    row.querySelector(".field-button").setAttribute(
      "aria-pressed",
      String(state.pinned !== null && Number(row.dataset.f) === state.pinned),
    );
  }
  renderInspector();
}

function fieldOf(target) {
  const holder = target instanceof Element ? target.closest("[data-f]") : null;
  return holder ? Number(holder.dataset.f) : null;
}

function show(source, index) {
  if (state[source] === index) return;
  state[source] = index;
  applyHighlight();
}

function togglePin(index) {
  state.pinned = index === null || state.pinned === index ? null : index;
  state.hovered = null;
  applyHighlight();
}

function wireHighlight() {
  for (const region of [$("bits"), $("fields").tBodies[0]]) {
    region.addEventListener("pointermove", (event) => {
      if (event.pointerType !== "touch") show("hovered", fieldOf(event.target));
    });
    region.addEventListener("pointerleave", () => show("hovered", null));
    region.addEventListener("click", (event) => togglePin(fieldOf(event.target)));
  }
  const body = $("fields").tBodies[0];
  // Only focus that arrived by keyboard: a clicked button keeps focus, and
  // should not hold the highlight against the pointer.
  body.addEventListener("focusin", (event) =>
    show("focused", event.target.matches(":focus-visible") ? fieldOf(event.target) : null),
  );
  body.addEventListener("focusout", () => show("focused", null));
  document.addEventListener("keydown", (event) => {
    if (event.key === "Escape" && state.pinned !== null) togglePin(null);
  });

  new ResizeObserver((entries) => {
    const width = entries[0].contentRect.width;
    if (width === 0) return;
    const columns = width >= 640 ? 32 : width >= 288 ? 16 : 8;
    if (columns !== state.columns) {
      state.columns = columns;
      if (state.view) {
        renderBits();
        applyHighlight();
      }
    }
  }).observe($("view"));
}

// -------------------------------------------------------------- your own files

function readEntry(entry, prefix, out) {
  return new Promise((resolve) => {
    if (entry.isFile) {
      entry.file(
        (file) => {
          out.push({ path: prefix + entry.name, file });
          resolve();
        },
        () => resolve(),
      );
    } else if (entry.isDirectory) {
      const reader = entry.createReader();
      const children = [];
      const more = () =>
        reader.readEntries(
          (batch) => {
            if (batch.length === 0) {
              Promise.all(
                children.map((child) => readEntry(child, `${prefix}${entry.name}/`, out)),
              ).then(resolve);
            } else {
              children.push(...batch);
              more();
            }
          },
          () => resolve(),
        );
      more();
    } else resolve();
  });
}

async function droppedFiles(transfer) {
  const entries = [...(transfer.items || [])]
    .map((item) => (item.webkitGetAsEntry ? item.webkitGetAsEntry() : null))
    .filter(Boolean);
  if (entries.length === 0) {
    return [...transfer.files].map((file) => ({ path: file.name, file }));
  }
  const out = [];
  await Promise.all(entries.map((entry) => readEntry(entry, "", out)));
  return out;
}

function isObject(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

// Files chosen one by one arrive without their folders. Put them where the
// dictionary layout wants them, judging each by what it contains.
function inferLayout(texts) {
  const laid = {};
  const messages = [];
  let metadata = null;
  let interfaceName = null;
  for (const [name, text] of Object.entries(texts)) {
    let json = null;
    try {
      json = JSON.parse(text);
    } catch {
      // Left for the decoder to report, as a message file.
    }
    if (name === "dictionary.json") laid[name] = text;
    else if (isObject(json) && typeof json.payload_hex === "string") {
      laid[`vectors/${name}`] = text;
      if (typeof json.interface === "string") interfaceName ??= json.interface;
    } else if (name === "metadata.json" || (isObject(json) && isObject(json.message_type))) {
      if (metadata !== null) {
        throw new Error(
          "These files hold more than one interface, and without their folders the page cannot tell which messages belong to which. Choose the dictionary folder instead.",
        );
      }
      metadata = text;
    } else messages.push([name, text]);
  }
  if (metadata === null) {
    throw new Error(
      "None of these files is an interface's metadata.json. Choose the dictionary folder, or include that file.",
    );
  }
  const iface = interfaceName || "INTERFACE";
  laid[`${iface}/metadata.json`] = metadata;
  for (const [name, text] of messages) laid[`${iface}/messages/${name}`] = text;
  return laid;
}

async function loadOwn(list) {
  const status = $("own-status");
  status.hidden = false;
  status.className = "notice";
  const picked = list.filter(({ path }) => path.toLowerCase().endsWith(".json"));
  try {
    if (picked.length === 0) throw new Error("No .json files among what was chosen.");
    if (picked.length > MAX_FILES) {
      throw new Error(
        `${picked.length} .json files were chosen; a dictionary holds at most ${MAX_FILES}.`,
      );
    }
    const texts = {};
    for (const { path, file } of picked) {
      if (file.size > MAX_FILE_BYTES) continue;
      texts[path] = await file.text();
    }
    const flat = Object.keys(texts).every((path) => !path.includes("/"));
    const files = flat ? inferLayout(texts) : texts;
    const first = Object.keys(texts)[0];
    const folder = flat ? null : first.split("/")[0];
    const entry = openDictionary("own", folder ? `${folder} (yours)` : "your files", files);
    if (!entry.info.description) {
      entry.info.description = "Loaded from your files, in this page only.";
    }
    state.dictionaries = state.dictionaries.filter((other) => other.id !== "own");
    state.dictionaries.push(entry);
    selectDictionary(entry);
    status.className = "notice good";
    status.textContent = `Read ${picked.length} files in this page. ${entry.summary}.`;
    $("controls").scrollIntoView({ behavior: "smooth", block: "start" });
  } catch (error) {
    status.className = "notice crit";
    status.textContent = error.message || String(error);
  }
}

function wireOwn() {
  const drop = $("drop");
  for (const type of ["dragenter", "dragover"]) {
    drop.addEventListener(type, (event) => {
      event.preventDefault();
      drop.classList.add("over");
    });
  }
  drop.addEventListener("dragleave", (event) => {
    if (!drop.contains(event.relatedTarget)) drop.classList.remove("over");
  });
  drop.addEventListener("drop", async (event) => {
    event.preventDefault();
    drop.classList.remove("over");
    loadOwn(await droppedFiles(event.dataTransfer));
  });
  for (const id of ["pick-folder", "pick-files"]) {
    $(id).addEventListener("change", (event) => {
      const files = [...event.target.files].map((file) => ({
        path: file.webkitRelativePath || file.name,
        file,
      }));
      event.target.value = "";
      if (files.length > 0) loadOwn(files);
    });
  }
}

// ----------------------------------------------------------------------- start

function wireControls() {
  $("dictionary-choices").addEventListener("change", (event) => {
    const entry = state.dictionaries.find((other) => other.id === event.target.value);
    if (entry) selectDictionary(entry);
  });
  $("vector-choices").addEventListener("change", (event) => {
    const vector = state.current.vectors.find((other) => other.name === event.target.value);
    if (vector) selectVector(vector);
  });
  $("interface").addEventListener("change", () => {
    interfaceChanged();
    inputsEdited();
  });
  $("payload").addEventListener("input", inputsEdited);
  $("frame-id").addEventListener("input", inputsEdited);
  $("controls").addEventListener("submit", (event) => event.preventDefault());
}

async function start() {
  const startup = $("startup");
  try {
    // Imported here, not at the top, so a missing build is reported on the page.
    const decoder = await import("../pkg/bite_wasm.js");
    await decoder.default();
    loadDictionary = decoder.loadDictionary;
    const response = await fetch("../dictionaries.json");
    if (!response.ok) throw new Error(`dictionaries.json: HTTP ${response.status}`);
    const bundle = await response.json();
    state.dictionaries = bundle.map((item) => openDictionary(item.id, item.id, item.files));
    wireControls();
    wireHighlight();
    wireOwn();
    $("app").hidden = false;
    startup.hidden = true;
    // Open on the dictionary with the most examples to try.
    selectDictionary(
      state.dictionaries.reduce((best, entry) =>
        entry.vectors.length > best.vectors.length ? entry : best,
      ),
    );
  } catch (error) {
    startup.className = "notice crit";
    startup.textContent =
      `The decoder did not load (${error.message || error}). ` +
      "This page needs the files `make wasm` builds, served over HTTP.";
  }
}

start();
