(function () {
  if (window.__DRPA_RECORDER_INSTALLED__) {
    return;
  }
  window.__DRPA_RECORDER_INSTALLED__ = true;
  window.__DRPA_RECORDER_QUEUE__ = window.__DRPA_RECORDER_QUEUE__ || [];
  let counter = 0;
  const lastInputs = new WeakMap();

  function nowIso() {
    return new Date().toISOString();
  }

  function nextId() {
    counter += 1;
    return "evt_" + String(counter).padStart(6, "0");
  }

  function isSensitive(element) {
    const haystack = [
      element.type,
      element.name,
      element.id,
      element.placeholder,
      element.autocomplete,
      element.getAttribute("aria-label"),
    ].join(" ").toLowerCase();
    return /(password|passwd|pwd|token|secret|otp|captcha|verification|code)/.test(haystack);
  }

  function cssEscape(value) {
    if (window.CSS && window.CSS.escape) {
      return window.CSS.escape(value);
    }
    return String(value).replace(/[^a-zA-Z0-9_-]/g, "\\$&");
  }

  function selectorCandidates(element) {
    const selectors = [];
    const add = (kind, value, score, reason) => {
      if (!value) return;
      selectors.push({ kind, value, score, reason });
    };
    const testId =
      element.getAttribute("data-testid") ||
      element.getAttribute("data-test") ||
      element.getAttribute("data-qa");
    if (testId) add("css", `[data-testid='${testId}']`, 95, "data-testid/data-test");
    const aria = element.getAttribute("aria-label");
    if (aria) add("css", `[aria-label='${aria}']`, 88, "aria-label");
    if (element.id) add("css", `#${cssEscape(element.id)}`, 82, "id");
    if (element.name) add("css", `${element.tagName.toLowerCase()}[name='${element.name}']`, 76, "name");
    if (element.placeholder) {
      add("css", `${element.tagName.toLowerCase()}[placeholder='${element.placeholder}']`, 72, "placeholder");
    }
    const text = visibleText(element);
    if (text && text.length <= 60) add("text", text, 68, "visible text");
    add("css", shortCssPath(element), 45, "generated css path");
    return selectors;
  }

  function visibleText(element) {
    return (element.innerText || element.value || element.textContent || "").trim().replace(/\s+/g, " ");
  }

  function shortCssPath(element) {
    const parts = [];
    let current = element;
    while (current && current.nodeType === Node.ELEMENT_NODE && parts.length < 5) {
      let part = current.tagName.toLowerCase();
      if (current.id) {
        part += "#" + cssEscape(current.id);
        parts.unshift(part);
        break;
      }
      const parent = current.parentElement;
      if (parent) {
        const siblings = Array.from(parent.children).filter((item) => item.tagName === current.tagName);
        if (siblings.length > 1) {
          part += `:nth-of-type(${siblings.indexOf(current) + 1})`;
        }
      }
      parts.unshift(part);
      current = parent;
    }
    return parts.join(" > ");
  }

  function targetInfo(element) {
    const rect = element.getBoundingClientRect();
    return {
      tag: element.tagName.toLowerCase(),
      type: element.type || "",
      text: visibleText(element),
      label: associatedLabel(element),
      placeholder: element.placeholder || "",
      attributes: {
        id: element.id || "",
        name: element.name || "",
        "data-testid": element.getAttribute("data-testid") || "",
        "aria-label": element.getAttribute("aria-label") || "",
      },
      rect: {
        x: Math.round(rect.x),
        y: Math.round(rect.y),
        width: Math.round(rect.width),
        height: Math.round(rect.height),
      },
      selectors: selectorCandidates(element),
    };
  }

  function associatedLabel(element) {
    if (element.labels && element.labels.length) {
      return Array.from(element.labels)
        .map((item) => visibleText(item))
        .filter(Boolean)
        .join(" ");
    }
    return "";
  }

  function confidenceFor(target) {
    const best = (target.selectors || [])[0];
    if (!best) return "unknown";
    if (best.score >= 85) return "high";
    if (best.score >= 65) return "medium";
    return "low";
  }

  function pushEvent(type, element, extra) {
    const target = targetInfo(element);
    const event = Object.assign(
      {
        id: nextId(),
        type,
        timestamp: nowIso(),
        url: location.href,
        title: document.title,
        target,
        value: null,
        sensitive: false,
        confidence: confidenceFor(target),
        notes: [],
      },
      extra || {}
    );
    window.__DRPA_RECORDER_QUEUE__.push(event);
  }

  document.addEventListener(
    "click",
    function (event) {
      pushEvent("click", event.target, {});
    },
    true
  );

  document.addEventListener(
    "input",
    function (event) {
      const element = event.target;
      const sensitive = isSensitive(element);
      const value = sensitive
        ? { mode: "param", param_name: element.name || element.id || "secret_value", redacted: true }
        : { mode: "literal", text: element.value || "" };
      clearTimeout(lastInputs.get(element));
      lastInputs.set(
        element,
        setTimeout(function () {
          pushEvent("input", element, { value, sensitive });
        }, 350)
      );
    },
    true
  );

  document.addEventListener(
    "change",
    function (event) {
      const element = event.target;
      pushEvent("change", element, {
        value: { mode: "literal", text: element.value || "", checked: Boolean(element.checked) },
        sensitive: isSensitive(element),
      });
    },
    true
  );

  document.addEventListener(
    "submit",
    function (event) {
      pushEvent("submit", event.target, { notes: ["submit events are hints; generated scripts usually replay click/input steps"] });
    },
    true
  );
})();
