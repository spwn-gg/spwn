// The `spwn` API handed to a workflow's `main(spwn, inputs)`, plus `console` and timers.
//
// Everything here wraps two host functions: `__host(op, json)` → Promise<json> and
// `__hostSync(op, json)` → json, each answering `{"ok": value}` or `{"err": message}`.
// The reference for this API is `spwn.d.ts`; keep the two in step.
"use strict";
(() => {
  const host = globalThis.__host;
  const hostSync = globalThis.__hostSync;
  delete globalThis.__host;
  delete globalThis.__hostSync;

  const unwrap = (json) => {
    const r = JSON.parse(json);
    if (r && Object.prototype.hasOwnProperty.call(r, "err")) {
      const e = new Error(r.err);
      if (r.code) e.code = r.code;
      throw e;
    }
    return r.ok;
  };
  const call = async (op, args) => unwrap(await host(op, JSON.stringify(args ?? {})));
  const callSync = (op, args) => unwrap(hostSync(op, JSON.stringify(args ?? {})));

  // --- console -------------------------------------------------------------------

  const show = (v) => {
    if (typeof v === "string") return v;
    if (v instanceof Error) {
      const head = `${v.name}: ${v.message}`;
      return v.stack ? `${head}\n${v.stack.trimEnd()}` : head;
    }
    if (v === undefined) return "undefined";
    if (typeof v === "function") return `[Function ${v.name || "anonymous"}]`;
    try {
      return JSON.stringify(v, null, 2) ?? String(v);
    } catch {
      return String(v);
    }
  };
  const logAt = (level) => (...args) => {
    callSync("log", { level, msg: args.map(show).join(" ") });
  };
  globalThis.console = {
    log: logAt("info"),
    info: logAt("info"),
    debug: logAt("debug"),
    warn: logAt("warn"),
    error: logAt("error"),
  };

  // --- timers: ride on the host's sleep, so they end with the run ----------------

  let timerSeq = 0;
  const liveTimers = new Set();
  const schedule = (fn, ms, args, repeat) => {
    const id = ++timerSeq;
    liveTimers.add(id);
    const tick = () =>
      call("sleep", { ms: Math.max(0, Number(ms) || 0) }).then(
        () => {
          if (!liveTimers.has(id)) return;
          if (!repeat) liveTimers.delete(id);
          try {
            fn(...args);
          } catch (e) {
            console.error("timer callback failed:", e);
          }
          if (repeat && liveTimers.has(id)) tick();
        },
        () => liveTimers.delete(id),
      );
    tick();
    return id;
  };
  globalThis.setTimeout = (fn, ms, ...args) => schedule(fn, ms, args, false);
  globalThis.setInterval = (fn, ms, ...args) => schedule(fn, ms, args, true);
  globalThis.clearTimeout = globalThis.clearInterval = (id) => {
    liveTimers.delete(id);
  };
  if (typeof globalThis.queueMicrotask !== "function") {
    globalThis.queueMicrotask = (fn) => {
      Promise.resolve().then(fn);
    };
  }

  // --- events + hook prompts: one pump reads both from the host ------------------

  const listeners = new Map(); // event → Set<fn>
  const promptHandlers = new Map(); // handler id → fn
  let handlerSeq = 0;
  let pumping = false;

  const answerPrompt = async (ev) => {
    let answer = null;
    const fn = promptHandlers.get(ev.handler);
    try {
      if (fn) answer = await fn({ ...ev.prompt, event: ev.event, session: ev.terminalId });
    } catch (e) {
      console.error("onHookPrompt handler failed:", e);
    }
    if (Array.isArray(answer)) answer = answer.join(",");
    await call("prompt.answer", { id: ev.id, answer: answer == null ? null : String(answer) }).catch(() => {});
  };

  const pump = async () => {
    if (pumping) return;
    pumping = true;
    for (;;) {
      let ev;
      try {
        ev = await call("events.next");
      } catch {
        return; // the run is ending
      }
      if (ev.type === "hookPrompt") {
        answerPrompt(ev);
        continue;
      }
      for (const fn of listeners.get(ev.event) ?? []) {
        Promise.resolve()
          .then(() => fn(ev.payload))
          .catch((e) => console.error(`spwn.on("${ev.event}") handler failed:`, e));
      }
    }
  };

  const addPromptHandler = (fn) => {
    if (typeof fn !== "function") return undefined;
    const id = `h${++handlerSeq}`;
    promptHandlers.set(id, fn);
    pump();
    return id;
  };

  // --- sessions ------------------------------------------------------------------

  // Where each session's transcript stood before its last prompt, so `waitForTurn`
  // waits for the reply to that prompt and not an earlier turn.
  const marks = new WeakMap();

  class Session {
    constructor(rec) {
      this._update(rec);
    }
    _update(rec) {
      // `status` is a method (the live value), so the record's snapshot of it is dropped.
      const { mark, status: _snapshot, ...fields } = rec;
      Object.assign(this, fields);
      if (mark !== undefined) marks.set(this, mark);
      return this;
    }
    async refresh() {
      const rec = await call("session.get", { id: this.id });
      if (!rec) throw new Error(`session ${this.id} no longer exists`);
      return this._update(rec);
    }
    status() {
      return call("session.status", { id: this.id });
    }
    async send(text, { submit = true } = {}) {
      marks.set(this, await call("session.send", { id: this.id, text: String(text), submit }));
    }
    async waitForTurn({ timeoutMs } = {}) {
      const r = await call("session.waitForTurn", { id: this.id, since: marks.get(this) ?? null, timeoutMs });
      if (r.turnUuid) marks.set(this, r.turnUuid);
      return r;
    }
    async prompt(text, opts = {}) {
      await this.send(text);
      return this.waitForTurn(opts);
    }
    waitForStatus(status, { timeoutMs } = {}) {
      return call("session.waitForStatus", { id: this.id, status: [].concat(status), timeoutMs });
    }
    screen() {
      return call("session.screen", { id: this.id });
    }
    // Not `key()`: `key` is the property the workflow filed the session under.
    press(key) {
      return call("session.key", { id: this.id, key });
    }
    interrupt() {
      return call("session.interrupt", { id: this.id });
    }
    transcript() {
      return call("session.transcript", { id: this.id });
    }
    lastMessage() {
      return call("session.lastMessage", { id: this.id });
    }
    delete() {
      return call("session.delete", { id: this.id });
    }
  }

  const sessions = {
    async create(opts = {}) {
      const { onHookPrompt, ...rest } = opts;
      return new Session(await call("sessions.create", { ...rest, promptHandler: addPromptHandler(onHookPrompt) }));
    },
    async find(key, { onHookPrompt } = {}) {
      const rec = await call("sessions.find", { key: String(key), promptHandler: addPromptHandler(onHookPrompt) });
      return rec ? new Session(rec) : null;
    },
    async get(id, { onHookPrompt } = {}) {
      const rec = await call("session.get", { id, claim: true, promptHandler: addPromptHandler(onHookPrompt) });
      return rec ? new Session(rec) : null;
    },
    async list() {
      return (await call("sessions.list")).map((rec) => new Session(rec));
    },
  };

  // --- the API ---------------------------------------------------------------------

  const project = callSync("project");

  const exec = (cmd, args = [], opts = {}) => call("exec", { ...opts, cmd, args: args.map(String) });

  const spwn = {
    project: Object.freeze({ id: project.id, name: project.name, dir: project.dir }),
    workflow: Object.freeze({ name: project.workflow, runId: project.runId }),

    log: logAt("info"),
    warn: logAt("warn"),
    error: logAt("error"),

    sleep: (ms) => call("sleep", { ms }),
    get stopping() {
      return callSync("stopping");
    },
    untilStopped: () => call("untilStopped"),

    state: Object.freeze({
      get(key, fallback) {
        const v = callSync("state.get", { key: String(key) });
        return v === null || v === undefined ? fallback : v;
      },
      set(key, value) {
        callSync("state.set", { key: String(key), value: value === undefined ? null : value });
      },
      delete(key) {
        callSync("state.delete", { key: String(key) });
      },
      all() {
        return callSync("state.all");
      },
    }),

    exec,
    sh: (script, opts = {}) => exec("sh", ["-c", script], opts),

    async fetch(url, opts = {}) {
      const r = await call("fetch", { ...opts, url: String(url) });
      return { ...r, text: async () => r.body, json: async () => JSON.parse(r.body) };
    },

    github: Object.freeze({
      graphql: (query, variables = {}) => call("github.graphql", { query, variables }),
      rest: (method, path, body) => call("github.rest", { method, path, body }),
    }),

    fs: Object.freeze({
      read: (path) => call("fs.read", { path }),
      write: (path, content) => call("fs.write", { path, content: String(content) }),
      exists: (path) => call("fs.exists", { path }),
    }),

    sessions: Object.freeze(sessions),

    agents: Object.freeze({
      list: () => call("agents.list"),
      run: (opts) => {
        const { onHookPrompt, ...rest } = typeof opts === "string" ? { prompt: opts } : opts;
        return call("agents.run", { ...rest, promptHandler: addPromptHandler(onHookPrompt) });
      },
    }),

    on(event, fn) {
      if (!listeners.has(event)) {
        listeners.set(event, new Set());
        callSync("events.subscribe", { events: [...listeners.keys()] });
      }
      listeners.get(event).add(fn);
      pump();
      return () => spwn.off(event, fn);
    },
    off(event, fn) {
      listeners.get(event)?.delete(fn);
    },
  };

  Object.freeze(spwn);
  globalThis.__takeSpwnApi = () => {
    delete globalThis.__takeSpwnApi;
    return spwn;
  };
})();
