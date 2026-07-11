(function installDouyinBevyAdapter(global, hostGlobal) {
  "use strict";

  const previousAdapter = global.DouyinBevy;
  const adapterVersion = 25;
  const host = hostGlobal || global;
  const tt = global.tt || host.tt;
  if (!tt) throw new Error("Douyin Mini Game global `tt` was not found");

  if (previousAdapter) {
    if (typeof previousAdapter.disposeAdapter === "function") {
      previousAdapter.disposeAdapter();
    } else {
      try {
        const TransitionEvent = global.PageTransitionEvent;
        if (typeof global.dispatchEvent === "function" && typeof TransitionEvent === "function") {
          global.dispatchEvent(new TransitionEvent("pagehide", { persisted: false }));
        }
      } catch (_) {}
      for (const method of [
        "offShow",
        "offHide",
        "offTouchStart",
        "offTouchMove",
        "offTouchEnd",
        "offTouchCancel",
      ]) {
        try { if (typeof tt[method] === "function") tt[method](); } catch (_) {}
      }
      try { if (global.__listeners && global.__listeners.clear) global.__listeners.clear(); } catch (_) {}
      try {
        if (previousAdapter.canvas && previousAdapter.canvas.__listeners) {
          previousAdapter.canvas.__listeners.clear();
        }
      } catch (_) {}
    }
  }

  const systemInfo = tt.getSystemInfoSync ? tt.getSystemInfoSync() : {};
  const logicalWidth = Number(systemInfo.windowWidth || systemInfo.screenWidth || 640);
  const logicalHeight = Number(systemInfo.windowHeight || systemInfo.screenHeight || 360);
  const pixelRatio = Number(systemInfo.pixelRatio || 1);
  const lifecycleEvents = [];
  const storageFallback = new Map();
  const webglContexts = new WeakSet();
  const instrumentedWebglContexts = new WeakSet();
  let presentedWebglContext = null;

  function setIfMissing(target, name, value) {
    try {
      if (target[name] != null) return target[name];
    } catch (_) {}
    try { target[name] = value; } catch (_) {}
    try {
      if (target[name] != null) return target[name];
    } catch (_) {}
    try {
      Object.defineProperty(target, name, {
        configurable: true,
        writable: true,
        value,
      });
      return value;
    } catch (_) {
      return undefined;
    }
  }

  function setValue(target, name, value) {
    try { target[name] = value; } catch (_) {}
    try {
      if (target[name] === value) return value;
    } catch (_) {}
    try {
      Object.defineProperty(target, name, {
        configurable: true,
        writable: true,
        value,
      });
      return value;
    } catch (_) {
      return undefined;
    }
  }

  function objectWithDefaults(target, name, defaults) {
    let value;
    try { value = target[name]; } catch (_) {}
    if (!value) {
      value = {};
      setIfMissing(target, name, value);
    }
    for (const [key, entry] of Object.entries(defaults)) setIfMissing(value, key, entry);
    return value;
  }

  function utf8Sequence(codePoint) {
    if (codePoint <= 0x7f) return [codePoint];
    if (codePoint <= 0x7ff) {
      return [0xc0 | (codePoint >> 6), 0x80 | (codePoint & 0x3f)];
    }
    if (codePoint <= 0xffff) {
      return [
        0xe0 | (codePoint >> 12),
        0x80 | ((codePoint >> 6) & 0x3f),
        0x80 | (codePoint & 0x3f),
      ];
    }
    return [
      0xf0 | (codePoint >> 18),
      0x80 | ((codePoint >> 12) & 0x3f),
      0x80 | ((codePoint >> 6) & 0x3f),
      0x80 | (codePoint & 0x3f),
    ];
  }

  function stringCodePoint(value, index) {
    const first = value.charCodeAt(index);
    if (first >= 0xd800 && first <= 0xdbff && index + 1 < value.length) {
      const second = value.charCodeAt(index + 1);
      if (second >= 0xdc00 && second <= 0xdfff) {
        return {
          codePoint: 0x10000 + ((first - 0xd800) << 10) + (second - 0xdc00),
          read: 2,
        };
      }
    }
    if (first >= 0xd800 && first <= 0xdfff) return { codePoint: 0xfffd, read: 1 };
    return { codePoint: first, read: 1 };
  }

  class TextEncoderPolyfill {
    get encoding() { return "utf-8"; }

    encode(input) {
      const value = input === undefined ? "" : String(input);
      const output = [];
      for (let index = 0; index < value.length;) {
        const current = stringCodePoint(value, index);
        output.push(...utf8Sequence(current.codePoint));
        index += current.read;
      }
      return Uint8Array.from(output);
    }

    encodeInto(input, destination) {
      if (!(destination instanceof Uint8Array)) {
        throw new TypeError("TextEncoder.encodeInto requires a Uint8Array destination");
      }
      const value = String(input);
      let read = 0;
      let written = 0;
      while (read < value.length) {
        const current = stringCodePoint(value, read);
        const bytes = utf8Sequence(current.codePoint);
        if (written + bytes.length > destination.length) break;
        destination.set(bytes, written);
        written += bytes.length;
        read += current.read;
      }
      return { read, written };
    }
  }

  function decoderBytes(input) {
    if (input === undefined) return new Uint8Array();
    if (input instanceof Uint8Array) return input;
    if (input instanceof ArrayBuffer) return new Uint8Array(input);
    if (ArrayBuffer.isView(input)) {
      return new Uint8Array(input.buffer, input.byteOffset, input.byteLength);
    }
    throw new TypeError("TextDecoder.decode requires an ArrayBuffer or typed array");
  }

  class TextDecoderPolyfill {
    constructor(label, options) {
      const encoding = label === undefined ? "utf-8" : String(label).toLowerCase();
      if (!["utf-8", "utf8", "unicode-1-1-utf-8"].includes(encoding)) {
        throw new RangeError("Only UTF-8 is supported by this TextDecoder");
      }
      const settings = options || {};
      this.encoding = "utf-8";
      this.fatal = !!settings.fatal;
      this.ignoreBOM = !!settings.ignoreBOM;
    }

    decode(input, options) {
      if (options && options.stream) {
        throw new TypeError("Streaming TextDecoder.decode is not supported");
      }
      const bytes = decoderBytes(input);
      const codeUnits = [];
      let output = "";
      let index = 0;

      const flush = () => {
        if (codeUnits.length === 0) return;
        output += String.fromCharCode.apply(null, codeUnits);
        codeUnits.length = 0;
      };
      const invalid = () => {
        if (this.fatal) throw new TypeError("The encoded data is not valid UTF-8");
        codeUnits.push(0xfffd);
        index += 1;
      };

      while (index < bytes.length) {
        const first = bytes[index];
        let length;
        let codePoint;
        let minimum;
        if (first <= 0x7f) {
          length = 1;
          codePoint = first;
          minimum = 0;
        } else if (first >= 0xc2 && first <= 0xdf) {
          length = 2;
          codePoint = first & 0x1f;
          minimum = 0x80;
        } else if (first >= 0xe0 && first <= 0xef) {
          length = 3;
          codePoint = first & 0x0f;
          minimum = 0x800;
        } else if (first >= 0xf0 && first <= 0xf4) {
          length = 4;
          codePoint = first & 0x07;
          minimum = 0x10000;
        } else {
          invalid();
          continue;
        }

        if (index + length > bytes.length) {
          invalid();
          continue;
        }
        let valid = true;
        for (let offset = 1; offset < length; offset += 1) {
          const next = bytes[index + offset];
          if ((next & 0xc0) !== 0x80) {
            valid = false;
            break;
          }
          codePoint = (codePoint << 6) | (next & 0x3f);
        }
        if (!valid || codePoint < minimum || codePoint > 0x10ffff
          || (codePoint >= 0xd800 && codePoint <= 0xdfff)) {
          invalid();
          continue;
        }

        if (!(codePoint === 0xfeff && index === 0 && !this.ignoreBOM)) {
          if (codePoint <= 0xffff) {
            codeUnits.push(codePoint);
          } else {
            const adjusted = codePoint - 0x10000;
            codeUnits.push(0xd800 + (adjusted >> 10), 0xdc00 + (adjusted & 0x3ff));
          }
        }
        index += length;
        if (codeUnits.length >= 8192) flush();
      }
      flush();
      return output;
    }
  }

  const TextEncoderImpl = global.TextEncoder || host.TextEncoder || TextEncoderPolyfill;
  const TextDecoderImpl = global.TextDecoder || host.TextDecoder || TextDecoderPolyfill;

  class EventTargetPolyfill {
    constructor() {
      Object.defineProperty(this, "__listeners", { value: new Map(), configurable: true });
    }

    addEventListener(type, callback) {
      if (!callback) return;
      if (!this.__listeners) {
        Object.defineProperty(this, "__listeners", { value: new Map(), configurable: true });
      }
      const listeners = this.__listeners.get(type) || [];
      if (!listeners.includes(callback)) listeners.push(callback);
      this.__listeners.set(type, listeners);
    }

    removeEventListener(type, callback) {
      if (!this.__listeners) return;
      const listeners = this.__listeners.get(type);
      if (!listeners) return;
      const index = listeners.indexOf(callback);
      if (index >= 0) listeners.splice(index, 1);
    }

    dispatchEvent(event) {
      if (!event || !event.type) throw new TypeError("dispatchEvent requires an Event");
      event.target = event.target || this;
      event.currentTarget = this;
      const listeners = this.__listeners && this.__listeners.get(event.type);
      if (listeners) {
        for (const callback of listeners.slice()) {
          if (typeof callback === "function") callback.call(this, event);
          else if (callback && callback.handleEvent) callback.handleEvent(event);
          if (event.__immediateStopped) break;
        }
      }
      const handler = this["on" + event.type];
      if (!event.__immediateStopped && typeof handler === "function") handler.call(this, event);
      return !event.defaultPrevented;
    }
  }

  class EventPolyfill {
    constructor(type, init) {
      const options = init || {};
      this.type = String(type || "");
      this.bubbles = !!options.bubbles;
      this.cancelable = !!options.cancelable;
      this.composed = !!options.composed;
      this.defaultPrevented = false;
      this.target = null;
      this.currentTarget = null;
      this.timeStamp = now();
      this.isTrusted = false;
    }

    preventDefault() {
      if (this.cancelable) this.defaultPrevented = true;
    }

    stopPropagation() {
      this.__stopped = true;
    }

    stopImmediatePropagation() {
      this.__stopped = true;
      this.__immediateStopped = true;
    }

    composedPath() {
      return this.target ? [this.target] : [];
    }
  }

  class MouseEventPolyfill extends EventPolyfill {
    constructor(type, init) {
      super(type, init);
      const options = init || {};
      this.screenX = Number(options.screenX || 0);
      this.screenY = Number(options.screenY || 0);
      this.clientX = Number(options.clientX || 0);
      this.clientY = Number(options.clientY || 0);
      this.offsetX = Number(options.offsetX != null ? options.offsetX : this.clientX);
      this.offsetY = Number(options.offsetY != null ? options.offsetY : this.clientY);
      this.movementX = Number(options.movementX || 0);
      this.movementY = Number(options.movementY || 0);
      this.button = Number(options.button != null ? options.button : 0);
      this.buttons = Number(options.buttons || 0);
      this.ctrlKey = !!options.ctrlKey;
      this.shiftKey = !!options.shiftKey;
      this.altKey = !!options.altKey;
      this.metaKey = !!options.metaKey;
      this.relatedTarget = options.relatedTarget || null;
    }
  }

  class PointerEventPolyfill extends MouseEventPolyfill {
    constructor(type, init) {
      super(type, init);
      const options = init || {};
      this.pointerId = Number(options.pointerId || 0);
      this.width = Number(options.width || 1);
      this.height = Number(options.height || 1);
      this.pressure = Number(options.pressure != null ? options.pressure : 0);
      this.tangentialPressure = Number(options.tangentialPressure || 0);
      this.tiltX = Number(options.tiltX || 0);
      this.tiltY = Number(options.tiltY || 0);
      this.twist = Number(options.twist || 0);
      this.pointerType = String(options.pointerType || "mouse");
      this.isPrimary = options.isPrimary !== false;
    }

    getCoalescedEvents() {
      return [this];
    }
  }

  class KeyboardEventPolyfill extends EventPolyfill {
    constructor(type, init) {
      super(type, init);
      Object.assign(this, {
        key: "",
        code: "",
        location: 0,
        repeat: false,
        isComposing: false,
        ctrlKey: false,
        shiftKey: false,
        altKey: false,
        metaKey: false,
      }, init || {});
    }
  }

  class WheelEventPolyfill extends MouseEventPolyfill {
    constructor(type, init) {
      super(type, init);
      const options = init || {};
      this.deltaX = Number(options.deltaX || 0);
      this.deltaY = Number(options.deltaY || 0);
      this.deltaZ = Number(options.deltaZ || 0);
      this.deltaMode = Number(options.deltaMode || 0);
    }
  }
  WheelEventPolyfill.DOM_DELTA_PIXEL = 0;
  WheelEventPolyfill.DOM_DELTA_LINE = 1;
  WheelEventPolyfill.DOM_DELTA_PAGE = 2;

  class FocusEventPolyfill extends EventPolyfill {}
  class PageTransitionEventPolyfill extends EventPolyfill {
    constructor(type, init) {
      super(type, init);
      this.persisted = !!(init && init.persisted);
    }
  }

  class MessagePortPolyfill extends EventTargetPolyfill {
    constructor() {
      super();
      this.onmessage = null;
      this.otherPort = null;
      this.closed = false;
    }
    postMessage(data) {
      if (this.closed || !this.otherPort || this.otherPort.closed) return;
      const destination = this.otherPort;
      schedule(() => destination.dispatchEvent(Object.assign(new EventPolyfill("message"), { data })));
    }
    start() {}
    close() { this.closed = true; }
  }

  class MessageChannelPolyfill {
    constructor() {
      this.port1 = new MessagePortPolyfill();
      this.port2 = new MessagePortPolyfill();
      this.port1.otherPort = this.port2;
      this.port2.otherPort = this.port1;
    }
  }

  class AbortSignalPolyfill extends EventTargetPolyfill {
    constructor() {
      super();
      this.aborted = false;
      this.reason = undefined;
    }
  }

  class AbortControllerPolyfill {
    constructor() { this.signal = new AbortSignalPolyfill(); }
    abort(reason) {
      this.signal.aborted = true;
      this.signal.reason = reason;
      this.signal.dispatchEvent(new EventPolyfill("abort"));
    }
  }

  class DOMExceptionPolyfill extends Error {
    constructor(message, name) {
      super(message);
      this.name = name || "Error";
    }
  }

  class StyleDeclaration {
    constructor(owner) {
      this.__owner = owner;
      this.__values = Object.create(null);
      this.setProperty("display", "block");
      this.setProperty("box-sizing", "content-box");
      this.setProperty("writing-mode", "horizontal-tb");
      this.setProperty("width", logicalWidth + "px");
      this.setProperty("height", logicalHeight + "px");
    }

    getPropertyValue(name) {
      return this.__values[toKebab(name)] || "";
    }

    setProperty(name, value) {
      const key = toKebab(name);
      this.__values[key] = String(value);
      this[name] = String(value);
      notifyResize(this.__owner);
    }

    removeProperty(name) {
      const key = toKebab(name);
      const previous = this.__values[key] || "";
      delete this.__values[key];
      delete this[name];
      notifyResize(this.__owner);
      return previous;
    }
  }

  function stylePropertyValue(style, name) {
    if (!style) return "";
    if (typeof style.getPropertyValue === "function") {
      try {
        const value = style.getPropertyValue(name);
        if (value != null && value !== "") return String(value);
      } catch (_) {}
    }
    for (const key of [name, toCamel(name), toKebab(name)]) {
      try {
        const value = style[key];
        if (value != null && value !== "") return String(value);
      } catch (_) {}
    }
    return "";
  }

  function installCanvasStyle(canvas) {
    let style;
    try { style = canvas.style; } catch (_) {}
    if (!style) {
      style = new StyleDeclaration(canvas);
      setValue(canvas, "style", style);
      try { style = canvas.style || style; } catch (_) {}
    }

    if (typeof style.getPropertyValue !== "function") {
      setValue(style, "getPropertyValue", function getPropertyValue(name) {
        const key = toKebab(name);
        const camel = toCamel(key);
        const value = this[key] != null ? this[key] : this[camel];
        return value == null ? "" : String(value);
      });
    }
    if (typeof style.setProperty !== "function") {
      setValue(style, "setProperty", function setProperty(name, value) {
        const key = toKebab(name);
        const text = String(value);
        this[key] = text;
        this[toCamel(key)] = text;
        notifyResize(canvas);
      });
    }
    if (typeof style.removeProperty !== "function") {
      setValue(style, "removeProperty", function removeProperty(name) {
        const key = toKebab(name);
        const camel = toCamel(key);
        const previous = this[key] != null ? String(this[key]) : String(this[camel] || "");
        try { delete this[key]; } catch (_) { this[key] = ""; }
        try { delete this[camel]; } catch (_) { this[camel] = ""; }
        notifyResize(canvas);
        return previous;
      });
    }

    for (const [name, value] of [
      ["display", "block"],
      ["box-sizing", "content-box"],
      ["writing-mode", "horizontal-tb"],
      ["width", logicalWidth + "px"],
      ["height", logicalHeight + "px"],
    ]) {
      if (stylePropertyValue(style, name) === "") {
        if (typeof style.setProperty === "function") style.setProperty(name, value);
        else style[toCamel(name)] = value;
      }
    }
    return style;
  }

  class ClassList {
    constructor() {
      this.values = new Set();
    }
    add(...names) { names.forEach((name) => this.values.add(name)); }
    remove(...names) { names.forEach((name) => this.values.delete(name)); }
    contains(name) { return this.values.has(name); }
    toggle(name, force) {
      const enabled = force == null ? !this.values.has(name) : !!force;
      if (enabled) this.values.add(name); else this.values.delete(name);
      return enabled;
    }
  }

  class ElementPolyfill extends EventTargetPolyfill {
    constructor(tagName) {
      super();
      this.tagName = String(tagName || "div").toUpperCase();
      this.nodeName = this.tagName;
      this.nodeType = 1;
      this.id = "";
      this.children = [];
      this.parentElement = null;
      this.ownerDocument = null;
      this.dataset = Object.create(null);
      this.attributes = Object.create(null);
      this.classList = new ClassList();
      this.style = new StyleDeclaration(this);
    }

    appendChild(child) {
      if (!this.children.includes(child)) this.children.push(child);
      child.parentElement = this;
      return child;
    }

    removeChild(child) {
      const index = this.children.indexOf(child);
      if (index >= 0) this.children.splice(index, 1);
      child.parentElement = null;
      return child;
    }

    remove() {
      if (this.parentElement) this.parentElement.removeChild(this);
    }

    setAttribute(name, value) {
      const text = String(value);
      this.attributes[name] = text;
      if (name === "id") this.id = text;
      if (name.startsWith("data-")) this.dataset[toCamel(name.slice(5))] = text;
    }

    getAttribute(name) {
      return Object.prototype.hasOwnProperty.call(this.attributes, name)
        ? this.attributes[name]
        : null;
    }

    contains(child) {
      if (child === this) return true;
      const children = this.children || [];
      return children.some((candidate) => candidate.contains && candidate.contains(child));
    }

    getBoundingClientRect() {
      const width = cssPixels(stylePropertyValue(this.style, "width"), logicalWidth);
      const height = cssPixels(stylePropertyValue(this.style, "height"), logicalHeight);
      return { x: 0, y: 0, left: 0, top: 0, right: width, bottom: height, width, height };
    }
  }

  class HTMLElementPolyfill extends ElementPolyfill {}
  class HTMLCanvasElementPolyfill extends HTMLElementPolyfill {}
  class HTMLImageElementPolyfill extends HTMLElementPolyfill {}

  const resizeObservers = new Map();
  let activeElement = null;
  let documentRef = null;

  function adaptWebglContext(context) {
    if (!context || instrumentedWebglContexts.has(context)) return;
    instrumentedWebglContexts.add(context);
    if (!presentedWebglContext) presentedWebglContext = context;
    let drawFramebuffer = null;
    for (const method of ["drawArrays", "drawElements", "drawArraysInstanced", "drawElementsInstanced", "bindFramebuffer"]) {
      const original = context[method];
      if (typeof original !== "function") continue;
      let prototype = Object.getPrototypeOf(context);
      let nativeMethod = null;
      while (prototype && !nativeMethod) {
        const descriptor = Object.getOwnPropertyDescriptor(prototype, method);
        if (descriptor && typeof descriptor.value === "function") nativeMethod = descriptor.value;
        prototype = Object.getPrototypeOf(prototype);
      }
      context[method] = function adaptedWebglCall(...args) {
        if (method === "bindFramebuffer"
          && (args[0] === context.FRAMEBUFFER || args[0] === context.DRAW_FRAMEBUFFER)) {
          drawFramebuffer = args[1];
        }
        // Helium hooks the first draw to present the onscreen canvas. Keep that
        // hook available for wgpu's final surface draw, not an offscreen pass.
        const implementation = method.startsWith("draw") && drawFramebuffer && nativeMethod
          ? nativeMethod
          : original;
        return Reflect.apply(implementation, this, args);
      };
    }
  }

  function decorateCanvas(canvas, id) {
    if (canvas.__douyinBevyCanvas) {
      installCanvasStyle(canvas);
      const previousGetContext = canvas.getContext && canvas.getContext.bind(canvas);
      canvas.getContext = function getContext(type, options) {
        const contextOptions = /^webgl/.test(type)
          ? Object.assign({}, options || {}, {
            preserveDrawingBuffer: !!(canvas._canvas && canvas._canvas.ownerDocument),
          })
          : options;
        const context = previousGetContext ? previousGetContext(type, contextOptions) : null;
        if (context && /^webgl/.test(type)) {
          webglContexts.add(context);
          adaptWebglContext(context);
        }
        return context;
      };
      return canvas;
    }
    const eventTarget = new EventTargetPolyfill();
    const nativeGetContext = canvas.getContext && canvas.getContext.bind(canvas);
    installCanvasStyle(canvas);
    Object.defineProperties(canvas, {
      __douyinBevyCanvas: { value: true },
      __listeners: { value: eventTarget.__listeners, configurable: true },
      dataset: { value: Object.create(null), writable: true },
      attributes: { value: Object.create(null), writable: true },
      classList: { value: new ClassList(), writable: true },
      children: { value: [], writable: true },
      id: { value: id || "", writable: true },
      tagName: { value: "CANVAS" },
      nodeName: { value: "CANVAS" },
      nodeType: { value: 1 },
      ownerDocument: { get: () => documentRef },
      parentElement: { value: null, writable: true },
    });
    canvas.addEventListener = EventTargetPolyfill.prototype.addEventListener;
    canvas.removeEventListener = EventTargetPolyfill.prototype.removeEventListener;
    canvas.dispatchEvent = EventTargetPolyfill.prototype.dispatchEvent;
    canvas.setAttribute = ElementPolyfill.prototype.setAttribute;
    canvas.getAttribute = ElementPolyfill.prototype.getAttribute;
    canvas.contains = ElementPolyfill.prototype.contains;
    canvas.remove = ElementPolyfill.prototype.remove;
    canvas.getContext = function getContext(type, options) {
      const contextOptions = /^webgl/.test(type)
        ? Object.assign({}, options || {}, {
          preserveDrawingBuffer: !!(canvas._canvas && canvas._canvas.ownerDocument),
        })
        : options;
      const context = nativeGetContext ? nativeGetContext(type, contextOptions) : null;
      if (context && !context.canvas) {
        try { Object.defineProperty(context, "canvas", { value: canvas }); } catch (_) {}
      }
      if (context && /^webgl/.test(type)) {
        webglContexts.add(context);
        adaptWebglContext(context);
      }
      return context;
    };
    canvas.getBoundingClientRect = function getBoundingClientRect() {
      const style = installCanvasStyle(canvas);
      const width = cssPixels(stylePropertyValue(style, "width"), logicalWidth);
      const height = cssPixels(stylePropertyValue(style, "height"), logicalHeight);
      return { x: 0, y: 0, left: 0, top: 0, right: width, bottom: height, width, height };
    };
    Object.defineProperties(canvas, {
      clientWidth: { get: () => canvas.getBoundingClientRect().width },
      clientHeight: { get: () => canvas.getBoundingClientRect().height },
      offsetWidth: { get: () => canvas.getBoundingClientRect().width },
      offsetHeight: { get: () => canvas.getBoundingClientRect().height },
    });
    canvas.focus = function focus() {
      activeElement = canvas;
      canvas.dispatchEvent(new FocusEventPolyfill("focus"));
    };
    canvas.blur = function blur() {
      if (activeElement === canvas) activeElement = null;
      canvas.dispatchEvent(new FocusEventPolyfill("blur"));
    };
    canvas.setPointerCapture = function setPointerCapture() {};
    canvas.releasePointerCapture = function releasePointerCapture() {};
    canvas.hasPointerCapture = function hasPointerCapture() { return false; };
    canvas.requestPointerLock = function requestPointerLock() {};
    return canvas;
  }

  const mainCanvas = decorateCanvas(tt.createCanvas(), "bevy-canvas");
  mainCanvas.width = Math.max(1, Math.round(logicalWidth * pixelRatio));
  mainCanvas.height = Math.max(1, Math.round(logicalHeight * pixelRatio));
  let devtoolsMirror = null;
  if (mainCanvas._canvas && mainCanvas._canvas.ownerDocument) {
    // Helium DevTools does not composite wgpu's WebGL surface. Mirror it into
    // the simulator DOM; real devices do not expose `_canvas.ownerDocument`.
    const nativeCanvas = mainCanvas._canvas;
    const mirrorCanvas = nativeCanvas.ownerDocument.createElement("canvas");
    mirrorCanvas.width = mainCanvas.width;
    mirrorCanvas.height = mainCanvas.height;
    mirrorCanvas.style.position = "absolute";
    mirrorCanvas.style.inset = "0";
    mirrorCanvas.style.width = "100%";
    mirrorCanvas.style.height = "100%";
    mirrorCanvas.style.pointerEvents = "none";
    mirrorCanvas.style.zIndex = "1";
    if (nativeCanvas.parentElement) nativeCanvas.parentElement.appendChild(mirrorCanvas);
    devtoolsMirror = {
      canvas: mirrorCanvas,
      context: mirrorCanvas.getContext("2d", { alpha: false }),
      width: mirrorCanvas.width,
      height: mirrorCanvas.height,
      pixels: new Uint8Array(mirrorCanvas.width * mirrorCanvas.height * 4),
      image: null,
    };
    devtoolsMirror.image = devtoolsMirror.context.createImageData(
      devtoolsMirror.width,
      devtoolsMirror.height,
    );
  }

  class DocumentPolyfill extends EventTargetPolyfill {
    constructor() {
      super();
      this.visibilityState = "visible";
      this.hidden = false;
      this.currentScript = null;
      this.fullscreenElement = null;
      this.webkitFullscreenElement = null;
      this.documentElement = new HTMLElementPolyfill("html");
      this.body = new HTMLElementPolyfill("body");
      this.documentElement.ownerDocument = this;
      this.body.ownerDocument = this;
      this.documentElement.appendChild(this.body);
      this.body.appendChild(mainCanvas);
    }

    createElement(tagName) {
      if (String(tagName).toLowerCase() === "canvas") {
        return decorateCanvas(tt.createCanvas(), "");
      }
      const element = new HTMLElementPolyfill(tagName);
      element.ownerDocument = this;
      return element;
    }

    querySelector(selector) {
      if (selector === "#bevy-canvas" || selector === "canvas") return mainCanvas;
      if (selector === "#loading") return null;
      return null;
    }

    getElementById(id) {
      return id === "bevy-canvas" ? mainCanvas : null;
    }

    contains(node) {
      return node === mainCanvas || this.documentElement.contains(node);
    }

    exitFullscreen() {
      this.fullscreenElement = null;
      return Promise.resolve();
    }

    webkitExitFullscreen() {
      return this.exitFullscreen();
    }

    exitPointerLock() {}
  }

  documentRef = new DocumentPolyfill();
  Object.defineProperty(documentRef, "activeElement", { get: () => activeElement });

  class ResizeObserverPolyfill {
    constructor(callback) {
      this.callback = callback;
      this.targets = new Set();
    }
    observe(target) {
      this.targets.add(target);
      const observers = resizeObservers.get(target) || new Set();
      observers.add(this);
      resizeObservers.set(target, observers);
      schedule(() => this.notify(target));
    }
    unobserve(target) {
      this.targets.delete(target);
      const observers = resizeObservers.get(target);
      if (observers) observers.delete(this);
    }
    disconnect() {
      for (const target of this.targets) this.unobserve(target);
    }
    notify(target) {
      if (!this.targets.has(target)) return;
      const rect = target.getBoundingClientRect();
      this.callback([{ target, contentRect: rect }], this);
    }
  }
  class ResizeObserverEntryPolyfill {}

  class IntersectionObserverPolyfill {
    constructor(callback) {
      this.callback = callback;
      this.targets = new Set();
    }
    observe(target) {
      this.targets.add(target);
      schedule(() => this.callback([{ target, isIntersecting: true, intersectionRatio: 1 }], this));
    }
    unobserve(target) { this.targets.delete(target); }
    disconnect() { this.targets.clear(); }
  }
  class IntersectionObserverEntryPolyfill {}

  class MediaQueryListPolyfill extends EventTargetPolyfill {
    constructor(media) {
      super();
      this.media = media;
      this.matches = media.includes("resolution:") || media.includes("pointer: coarse");
    }
    addListener(callback) { this.addEventListener("change", callback); }
    removeListener(callback) { this.removeEventListener("change", callback); }
  }

  class HeadersPolyfill {
    constructor(values) { this.values = values || Object.create(null); }
    get(name) { return this.values[String(name).toLowerCase()] || null; }
  }

  class ResponsePolyfill {
    constructor(body, init) {
      const options = init || {};
      this.bodyBytes = toUint8Array(body || new Uint8Array());
      this.status = options.status == null ? 200 : options.status;
      this.statusText = options.statusText || "";
      this.ok = this.status >= 200 && this.status < 300;
      this.type = "basic";
      this.url = options.url || "";
      this.headers = new HeadersPolyfill(options.headers);
    }
    arrayBuffer() {
      const bytes = this.bodyBytes;
      return Promise.resolve(bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength));
    }
    text() {
      return this.arrayBuffer().then((buffer) => new TextDecoder().decode(buffer));
    }
    json() { return this.text().then(JSON.parse); }
    clone() { return new ResponsePolyfill(this.bodyBytes.slice(), { status: this.status, url: this.url }); }
  }

  function notifyResize(target) {
    const observers = resizeObservers.get(target);
    if (!observers) return;
    for (const observer of observers) schedule(() => observer.notify(target));
  }

  function schedule(callback) {
    if (typeof global.queueMicrotask === "function") global.queueMicrotask(callback);
    else Promise.resolve().then(callback);
  }

  function now() {
    return global.performance && global.performance.now ? global.performance.now() : Date.now();
  }

  function toCamel(value) {
    return value.replace(/-([a-z])/g, (_, letter) => letter.toUpperCase());
  }

  function toKebab(value) {
    return String(value).replace(/[A-Z]/g, (letter) => "-" + letter.toLowerCase()).toLowerCase();
  }

  function cssPixels(value, fallback) {
    if (!value || value === "auto" || value.endsWith("%")) return fallback;
    const parsed = Number.parseFloat(value);
    return Number.isFinite(parsed) ? parsed : fallback;
  }

  function toUint8Array(value) {
    if (value instanceof Uint8Array) return value;
    if (value instanceof ArrayBuffer) return new Uint8Array(value);
    if (value && value.buffer instanceof ArrayBuffer) {
      return new Uint8Array(value.buffer, value.byteOffset || 0, value.byteLength);
    }
    if (typeof value === "string") return new TextEncoder().encode(value);
    return new Uint8Array(value || []);
  }

  function normalizePackagePath(path) {
    let value = String(path && path.url ? path.url : path || "");
    value = value.replace(/^https?:\/\/[^/]+\//, "").replace(/^file:\/\//, "");
    value = value.split("?")[0].split("#")[0];
    while (value.startsWith("./")) value = value.slice(2);
    return value.startsWith("/") ? value.slice(1) : value;
  }

  function readPackageFile(path) {
    const filePath = normalizePackagePath(path);
    return new Promise((resolve, reject) => {
      let fileSystem;
      try {
        fileSystem = tt.getFileSystemManager();
      } catch (error) {
        reject(error);
        return;
      }
      fileSystem.readFile({
        filePath,
        success(result) { resolve(toUint8Array(result.data)); },
        fail(error) { reject(new Error("Unable to read package file `" + filePath + "`: " + JSON.stringify(error))); },
      });
    });
  }

  function fetchPolyfill(path) {
    const url = normalizePackagePath(path);
    return readPackageFile(url).then(
      (bytes) => new ResponsePolyfill(bytes, {
        status: 200,
        url,
        headers: { "content-type": url.endsWith(".wasm") ? "application/wasm" : "application/octet-stream" },
      }),
      () => new ResponsePolyfill(new Uint8Array(), { status: 404, url }),
    );
  }

  function defineInstanceClass(name, predicate, fallbackClass, replaceExisting) {
    if (global[name] && !replaceExisting) return;
    const klass = fallbackClass || class {};
    Object.defineProperty(klass, Symbol.hasInstance, { value: predicate });
    setValue(global, name, klass);
  }

  // Helium exposes a browser Window constructor, but its sandbox global is not
  // an instance of it. web_sys::window() relies on this exact instanceof check.
  defineInstanceClass("Window", (value) => value === global, undefined, true);
  defineInstanceClass("HTMLElement", (value) => !!(value && (value.__douyinBevyCanvas || value instanceof HTMLElementPolyfill)), HTMLElementPolyfill);
  defineInstanceClass("HTMLCanvasElement", (value) => !!(value && value.__douyinBevyCanvas), HTMLCanvasElementPolyfill);
  defineInstanceClass("HTMLImageElement", (value) => !!(value && value.__douyinBevyImage), HTMLImageElementPolyfill);
  defineInstanceClass("WebGLRenderingContext", (value) => !!(value && webglContexts.has(value)));
  defineInstanceClass("WebGL2RenderingContext", (value) => !!(value && webglContexts.has(value)));

  const runtimeGlobals = {
    window: global,
    self: global,
    document: documentRef,
    EventTarget: global.EventTarget || EventTargetPolyfill,
    Event: EventPolyfill,
    MouseEvent: MouseEventPolyfill,
    PointerEvent: PointerEventPolyfill,
    KeyboardEvent: KeyboardEventPolyfill,
    WheelEvent: WheelEventPolyfill,
    FocusEvent: FocusEventPolyfill,
    PageTransitionEvent: PageTransitionEventPolyfill,
    ResizeObserver: ResizeObserverPolyfill,
    ResizeObserverEntry: ResizeObserverEntryPolyfill,
    IntersectionObserver: IntersectionObserverPolyfill,
    IntersectionObserverEntry: IntersectionObserverEntryPolyfill,
    MessageChannel: global.MessageChannel || MessageChannelPolyfill,
    MessagePort: global.MessagePort || MessagePortPolyfill,
    AbortController: global.AbortController || AbortControllerPolyfill,
    AbortSignal: global.AbortSignal || AbortSignalPolyfill,
    DOMException: global.DOMException || DOMExceptionPolyfill,
    Response: ResponsePolyfill,
    Headers: HeadersPolyfill,
    TextEncoder: TextEncoderImpl,
    TextDecoder: TextDecoderImpl,
    fetch: fetchPolyfill,
    innerWidth: logicalWidth,
    innerHeight: logicalHeight,
    devicePixelRatio: pixelRatio,
    isSecureContext: true,
  };
  for (const [name, value] of Object.entries(runtimeGlobals)) setValue(global, name, value);
  if (host !== global) {
    setIfMissing(host, "TextEncoder", TextEncoderImpl);
    setIfMissing(host, "TextDecoder", TextDecoderImpl);
  }

  if (!global.performance) global.performance = { now: () => Date.now() };
  if (!global.performance.mark) global.performance.mark = function mark() {};
  if (!global.performance.measure) global.performance.measure = function measure() {};
  if (!global.setTimeout) {
    let nextTimer = 1;
    const timers = new Map();
    global.setTimeout = (callback, delay) => {
      const id = nextTimer++;
      const start = Date.now();
      const tick = () => {
        if (!timers.has(id)) return;
        if (Date.now() - start >= Number(delay || 0)) {
          timers.delete(id);
          callback();
        } else {
          timers.set(id, global.requestAnimationFrame(tick));
        }
      };
      timers.set(id, global.requestAnimationFrame(tick));
      return id;
    };
    global.clearTimeout = (id) => {
      const frame = timers.get(id);
      timers.delete(id);
      if (frame != null && global.cancelAnimationFrame) global.cancelAnimationFrame(frame);
    };
  }
  if (!global.requestAnimationFrame) {
    global.requestAnimationFrame = (callback) => setTimeout(() => callback(now()), 16);
    global.cancelAnimationFrame = (handle) => clearTimeout(handle);
  }
  let animationFrameCount = 0;
  const hostRequestAnimationFrame = global.requestAnimationFrame.bind(global);
  const hostCancelAnimationFrame = global.cancelAnimationFrame
    ? global.cancelAnimationFrame.bind(global)
    : null;
  setValue(global, "requestAnimationFrame", (callback) => {
    return hostRequestAnimationFrame((timestamp) => {
      animationFrameCount += 1;
      callback(timestamp);
      if (presentedWebglContext && devtoolsMirror && devtoolsMirror.context) {
        const context = presentedWebglContext;
        context.bindFramebuffer(context.FRAMEBUFFER, null);
        context.finish();
        if (animationFrameCount % 4 === 0) {
          const { width, height, pixels, image } = devtoolsMirror;
          context.readPixels(0, 0, width, height, context.RGBA, context.UNSIGNED_BYTE, pixels);
          const rowBytes = width * 4;
          for (let row = 0; row < height; row += 1) {
            const sourceOffset = (height - row - 1) * rowBytes;
            image.data.set(pixels.subarray(sourceOffset, sourceOffset + rowBytes), row * rowBytes);
          }
          devtoolsMirror.context.putImageData(image, 0, 0);
        }
      }
    });
  });
  setValue(global, "cancelAnimationFrame", (handle) => {
    if (hostCancelAnimationFrame) hostCancelAnimationFrame(handle);
  });
  // Helium's native queueMicrotask throws "Illegal invocation" when wasm-bindgen
  // calls it as a free function. A Promise job has the same microtask semantics.
  setValue(global, "queueMicrotask", (callback) => Promise.resolve().then(callback));

  objectWithDefaults(global, "navigator", {
    userAgent: "Mozilla/5.0 DouyinMiniGame AppleWebKit Chrome",
    platform: systemInfo.platform || "douyin",
    language: systemInfo.language || "zh-CN",
    maxTouchPoints: 10,
    hardwareConcurrency: 4,
    getGamepads: () => [],
  });
  objectWithDefaults(global, "screen", {
    width: Number(systemInfo.screenWidth || logicalWidth),
    height: Number(systemInfo.screenHeight || logicalHeight),
    orientation: { lock: () => Promise.resolve(), unlock: () => {} },
  });
  objectWithDefaults(global, "location", {
    href: "game.js",
    origin: "",
    protocol: "file:",
    pathname: "/game.js",
  });
  global.matchMedia = (query) => new MediaQueryListPolyfill(String(query));
  global.getComputedStyle = (element) => element.style;

  if (!global.crypto || !global.crypto.getRandomValues) {
    global.crypto = global.crypto || {};
    global.crypto.getRandomValues = function getRandomValues(array) {
      for (let index = 0; index < array.length; index += 1) {
        array[index] = Math.floor(Math.random() * 256);
      }
      return array;
    };
  }

  if (!global.Image) {
    global.Image = function Image() {
      const image = tt.createImage ? tt.createImage() : new HTMLImageElementPolyfill("img");
      image.__douyinBevyImage = true;
      return image;
    };
  }
  if (!global.createImageBitmap) global.createImageBitmap = (image) => Promise.resolve(image);

  function dispatchTouch(type, sourceEvent) {
    const changedTouches = sourceEvent.changedTouches || sourceEvent.touches || [];
    const activeTouches = sourceEvent.touches || changedTouches;
    mainCanvas.dispatchEvent(new EventPolyfill(type === "pointerdown" ? "touchstart" : type === "pointerup" ? "touchend" : type === "pointercancel" ? "touchcancel" : "touchmove", { cancelable: true }));
    for (let index = 0; index < changedTouches.length; index += 1) {
      const touch = changedTouches[index];
      const x = Number(touch.screenX != null ? touch.screenX : touch.clientX || 0);
      const y = Number(touch.screenY != null ? touch.screenY : touch.clientY || 0);
      const identifier = Number(touch.identifier != null ? touch.identifier : index);
      const isPrimary = activeTouches.length === 0 || identifier === Number(activeTouches[0].identifier || 0);
      mainCanvas.dispatchEvent(new PointerEventPolyfill(type, {
        bubbles: true,
        cancelable: true,
        pointerId: identifier + 1,
        pointerType: "touch",
        isPrimary,
        screenX: x,
        screenY: y,
        clientX: x,
        clientY: y,
        offsetX: x,
        offsetY: y,
        pressure: type === "pointerup" || type === "pointercancel" ? 0 : Number(touch.force || 0.5),
        button: type === "pointermove" ? -1 : 0,
        buttons: type === "pointerup" || type === "pointercancel" ? 0 : 1,
      }));
    }
  }

  const platformCallbacks = {
    touchStart: (event) => dispatchTouch("pointerdown", event),
    touchMove: (event) => dispatchTouch("pointermove", event),
    touchEnd: (event) => dispatchTouch("pointerup", event),
    touchCancel: (event) => dispatchTouch("pointercancel", event),
    show: () => {
    lifecycleEvents.push(1);
    documentRef.visibilityState = "visible";
    documentRef.hidden = false;
    documentRef.dispatchEvent(new EventPolyfill("visibilitychange"));
      global.dispatchEvent(new PageTransitionEventPolyfill("pageshow", { persisted: true }));
    },
    hide: () => {
      lifecycleEvents.push(2);
      documentRef.visibilityState = "hidden";
      documentRef.hidden = true;
      documentRef.dispatchEvent(new EventPolyfill("visibilitychange"));
      global.dispatchEvent(new PageTransitionEventPolyfill("pagehide", { persisted: true }));
    },
  };

  if (tt.onTouchStart) tt.onTouchStart(platformCallbacks.touchStart);
  if (tt.onTouchMove) tt.onTouchMove(platformCallbacks.touchMove);
  if (tt.onTouchEnd) tt.onTouchEnd(platformCallbacks.touchEnd);
  if (tt.onTouchCancel) tt.onTouchCancel(platformCallbacks.touchCancel);
  if (tt.onShow) tt.onShow(platformCallbacks.show);
  if (tt.onHide) tt.onHide(platformCallbacks.hide);

  const globalEvents = new EventTargetPolyfill();
  global.addEventListener = EventTargetPolyfill.prototype.addEventListener;
  global.removeEventListener = EventTargetPolyfill.prototype.removeEventListener;
  global.dispatchEvent = EventTargetPolyfill.prototype.dispatchEvent;
  if (!global.__listeners) Object.defineProperty(global, "__listeners", { value: globalEvents.__listeners });

  let gameGeneration = 0;
  function prepareReload() {
    if (gameGeneration > 0) {
      EventTargetPolyfill.prototype.dispatchEvent.call(
        global,
        new PageTransitionEventPolyfill("pagehide", { persisted: false }),
      );
      try { global.__listeners.clear(); } catch (_) {}
      try { documentRef.__listeners.clear(); } catch (_) {}
      try { mainCanvas.__listeners.clear(); } catch (_) {}
    }
    gameGeneration += 1;
  }

  function disposeAdapter() {
    prepareReload();
    try {
      if (devtoolsMirror && devtoolsMirror.canvas) devtoolsMirror.canvas.remove();
    } catch (_) {}
    for (const [method, callback] of [
      ["offShow", platformCallbacks.show],
      ["offHide", platformCallbacks.hide],
      ["offTouchStart", platformCallbacks.touchStart],
      ["offTouchMove", platformCallbacks.touchMove],
      ["offTouchEnd", platformCallbacks.touchEnd],
      ["offTouchCancel", platformCallbacks.touchCancel],
    ]) {
      try { if (typeof tt[method] === "function") tt[method](callback); } catch (_) {}
    }
  }

  const api = {
    adapterVersion,
    installed: true,
    available: () => true,
    systemInfo,
    canvas: mainCanvas,
    prepareReload,
    disposeAdapter,
    readPackageFile,
    systemInfoJson: () => JSON.stringify(systemInfo),
    touchCapable: () => true,
    devicePixelRatio: () => pixelRatio,
    gameCanvasClientWidth: () => mainCanvas.clientWidth,
    toggleFullscreen: () => {},
    exitFullscreen: () => {},
    markGameReady: () => mainCanvas.focus(),
    setPreferredFramesPerSecond(fps) {
      if (tt.setPreferredFramesPerSecond) tt.setPreferredFramesPerSecond(Math.max(1, Math.min(60, Number(fps) || 60)));
    },
    takeLifecycleEvent: () => lifecycleEvents.shift() || 0,
    storageGet(key) {
      try {
        const value = tt.getStorageSync ? tt.getStorageSync(key) : storageFallback.get(key);
        return value == null ? "" : String(value);
      } catch (_) {
        return storageFallback.get(key) || "";
      }
    },
    storageSet(key, value) {
      storageFallback.set(key, String(value));
      if (tt.setStorageSync) tt.setStorageSync(key, value);
    },
    storageRemove(key) {
      storageFallback.delete(key);
      if (tt.removeStorageSync) tt.removeStorageSync(key);
    },
  };

  setValue(global, "DouyinBevy", api);
  if (host !== global) setValue(host, "DouyinBevy", api);
  if (typeof module !== "undefined" && module.exports) module.exports = api;
})(
  typeof globalThis !== "undefined"
    ? globalThis
    : (typeof GameGlobal !== "undefined" ? GameGlobal : this),
  typeof GameGlobal !== "undefined"
    ? GameGlobal
    : (typeof globalThis !== "undefined" ? globalThis : this),
);
