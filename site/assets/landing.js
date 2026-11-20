// The landing page's first screen: the copy button, and the decode of the
// weather-station `observation` test vector, field by field. The values come
// from the bite decoder compiled to WebAssembly (pkg/, built by `make wasm`),
// running in the visitor's browser; if it is not there, from the vector's
// expected values in dictionaries.json. Nothing here decodes anything itself.
(function () {
  "use strict";

  document.querySelectorAll("button.copy").forEach(function (b) {
    b.addEventListener("click", function () {
      var text = document.getElementById(b.dataset.copy).textContent;
      if (navigator.clipboard) navigator.clipboard.writeText(text).then(function () {
        b.textContent = "Copied"; setTimeout(function () { b.textContent = "Copy"; }, 1600);
      });
    });
  });

  var bytesEl = document.getElementById("bytes"), recordEl = document.getElementById("record");
  var fieldEl = document.getElementById("bite-field"), footEl = document.getElementById("bite-foot");
  if (!bytesEl || !window.fetch) return;
  var reduce = window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  var DICT = "weather-station", IFACE = "WX", VECTOR = "observation";

  function parseHex(text) {
    var c = text.replace(/\s+/g, ""), out = [];
    for (var i = 0; i < c.length; i += 2) out.push(parseInt(c.substr(i, 2), 16));
    return out;
  }
  function el(tag, cls, text) { var n = document.createElement(tag); if (cls) n.className = cls; if (text != null) n.textContent = text; return n; }

  // Returns { hex, fields: [{name, start, stop, unit}], values: {name: text}, live: bool }
  function viaWasm(bundle) {
    return import("../pkg/bite_wasm.js").then(function (m) {
      return m.default().then(function () {
        var entry = bundle.find(function (d) { return d.id === DICT; });
        var handle = m.loadDictionary(entry.files);
        var iface = handle.interfaces().find(function (i) { return i.name === IFACE; });
        var vector = handle.vectors().find(function (v) { return v.name === VECTOR; });
        var hex = vector.payload_hex || vector.payload;
        var result = handle.decode(IFACE, hex);
        var message = iface.messages.find(function (x) { return x.name === result.message_type && x.defined; });
        var values = {};
        Object.keys(result.fields).forEach(function (k) { if (result.fields[k] != null) values[k] = String(result.fields[k]); });
        return { hex: hex, fields: message.fields, values: values, type: result.message_type, live: true };
      });
    });
  }
  function viaVector(bundle) {
    var files = bundle.find(function (d) { return d.id === DICT; }).files;
    var vector = JSON.parse(files["vectors/" + VECTOR + ".json"]);
    var type = vector.expect.message_type, def = JSON.parse(files[IFACE + "/messages/" + type + ".json"]);
    var fields = Object.keys(def.fields).map(function (k) { var f = def.fields[k]; return { name: k, start: f.start, stop: f.stop, unit: f.unit }; });
    return { hex: vector.payload_hex, fields: fields, values: vector.expect.fields, type: type, live: false };
  }

  fetch("dictionaries.json").then(function (r) { if (!r.ok) throw new Error(r.status); return r.json(); }).then(function (bundle) {
    return viaWasm(bundle).catch(function () { return viaVector(bundle); });
  }).then(show).catch(function () {});

  function show(d) {
    var bytes = parseHex(d.hex), cells = [];
    bytesEl.textContent = "";
    bytes.forEach(function (b) {
      var cell = el("span", "byte"), bits = el("span", "bits");
      cell.appendChild(el("span", "hx", ("0" + b.toString(16)).slice(-2)));
      for (var i = 0; i < 8; i++) bits.appendChild(el("i"));
      cell.appendChild(bits); bytesEl.appendChild(cell); cells.push(cell);
    });
    footEl.textContent = d.live
      ? "The bite decoder, compiled to WebAssembly, decoding the weather-station dictionary's observation test vector in your browser."
      : "The weather-station dictionary's observation test vector and the values bite check expects of it.";
    var shown = d.fields.filter(function (f) { return d.values[f.name] != null; });
    function value(f) { return d.values[f.name] + (f.unit && f.unit !== "bytes" ? " " + f.unit : ""); }
    function paint(i, on) {
      var f = shown[i], hue = "var(--c" + (i % 8) + ")";
      for (var bit = f.start; bit < f.stop; bit++) {
        var cell = cells[Math.floor(bit / 8)];
        if (!cell) continue;
        cell.querySelectorAll(".bits i")[bit % 8].style.background = hue;
        if (on) cell.classList.add("on");
      }
    }
    function rows(n, current) {
      recordEl.textContent = "";
      for (var i = 0; i < n; i++) {
        var r = el("div", "rrow" + (i === current ? " on" : ""));
        r.appendChild(el("span", "k", shown[i].name)); r.appendChild(el("span", "v", value(shown[i])));
        recordEl.appendChild(r);
      }
    }
    if (reduce) {
      shown.forEach(function (_, i) { paint(i, false); });
      rows(shown.length, -1);
      fieldEl.textContent = d.type + ", " + bytes.length + " bytes";
      return;
    }
    var i = 0;
    (function step() {
      if (i === 0) {
        cells.forEach(function (c) { c.classList.remove("on"); c.querySelectorAll(".bits i").forEach(function (b) { b.style.background = ""; }); });
        recordEl.textContent = "";
      }
      bytesEl.classList.add("focus");
      cells.forEach(function (c) { c.classList.remove("on"); });
      paint(i, true);
      var f = shown[i];
      fieldEl.textContent = f.name + ": bits " + f.start + " to " + (f.stop - 1);
      rows(i + 1, i);
      i++;
      if (i < shown.length) { setTimeout(step, 1300); return; }
      setTimeout(function () {
        bytesEl.classList.remove("focus"); rows(shown.length, -1);
        fieldEl.textContent = d.type + ", " + bytes.length + " bytes";
        i = 0; setTimeout(step, 4000);
      }, 1300);
    })();
  }
})();
