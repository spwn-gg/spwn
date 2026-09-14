// Types for spwn workflows — the scripts in a project's `.spwn/workflows/`.
//
// spwn writes this file next to your workflows (from the Workflows panel's "New workflow")
// and keeps it current; don't edit it. Use it from a workflow:
//
//   TypeScript:  import type { Spwn, WorkflowMeta } from "./spwn";
//   JavaScript:  // @ts-check
//                /** @param {import("./spwn").Spwn} spwn */
//
// Types are for your editor only: spwn strips them and never type-checks at run time.

/** `export const meta` — optional. */
export interface WorkflowMeta {
  /** Shown in the Workflows panel instead of the file name. */
  name?: string;
  description?: string;
  /** Restart the workflow (with backoff) whenever it exits or throws, until stopped. */
  keepAlive?: boolean;
  /** Inputs the Run form asks for; `main` receives them as its second argument. */
  inputs?: Record<string, WorkflowInput>;
}

export interface WorkflowInput {
  /** How the Run form edits it. Default `"string"`. */
  type?: "string" | "number" | "boolean";
  /** Used when the input is left empty — including runs started with spwn. */
  default?: unknown;
  description?: string;
  /** Refuse to start without it (and without a default). */
  required?: boolean;
}

/** `export default` — the workflow itself. A keep-alive workflow is restarted when this returns. */
export type WorkflowMain<Inputs = Record<string, unknown>> = (
  spwn: Spwn,
  inputs: Inputs,
) => unknown | Promise<unknown>;

/** A session's live status, as the sidebar shows it. */
export type SessionStatus =
  | "thinking"
  | "blockedPermission"
  | "blockedQuestion"
  | "done"
  | "error"
  | "idle";

export interface Spwn {
  readonly project: { readonly id: string; readonly name: string; readonly dir: string };
  readonly workflow: { readonly name: string; readonly runId: string };

  /** Write to the run's log in the Workflows panel. `console.*` works too. */
  log(...args: unknown[]): void;
  warn(...args: unknown[]): void;
  error(...args: unknown[]): void;

  /** Resolves after `ms`; rejects (with `code: "stopped"`) as soon as the run is stopped. */
  sleep(ms: number): Promise<void>;
  /** True once the run has been asked to stop. */
  readonly stopping: boolean;
  /** Resolves when the run is stopped — for workflows that only react to events. */
  untilStopped(): Promise<void>;

  /** JSON values kept per workflow across runs and restarts (in spwn's data folder). */
  readonly state: {
    get<T = unknown>(key: string, fallback?: T): T;
    /** Setting `null` or `undefined` deletes the key. */
    set(key: string, value: unknown): void;
    delete(key: string): void;
    all(): Record<string, unknown>;
  };

  /** Run a program (no shell) in the project dir; a stopped run kills it. */
  exec(cmd: string, args?: string[], options?: ExecOptions): Promise<ExecResult>;
  /** Run a script with `sh -c`. */
  sh(script: string, options?: ExecOptions): Promise<ExecResult>;

  fetch(url: string, options?: FetchOptions): Promise<FetchResponse>;

  /** GitHub's API with the token saved in spwn's Settings (the token never reaches the script). */
  readonly github: {
    /** Resolves with `data`; throws when GitHub returns errors and no data. */
    graphql<T = any>(query: string, variables?: Record<string, unknown>): Promise<T>;
    /** `path` like `/repos/owner/repo/issues`. Resolves with the parsed JSON body. */
    rest<T = any>(method: string, path: string, body?: unknown): Promise<T>;
  };

  /** Files, relative to (and kept inside) the project dir. */
  readonly fs: {
    read(path: string): Promise<string>;
    write(path: string, content: string): Promise<void>;
    exists(path: string): Promise<boolean>;
  };

  readonly sessions: {
    /** Start an interactive agent session — in its own worktree, with the project's hooks. */
    create(options?: CreateSessionOptions): Promise<Session>;
    /** The session this workflow created with `key`, surviving restarts; null if none. */
    find(key: string, options?: { onHookPrompt?: HookPromptHandler }): Promise<Session | null>;
    /** Any session in the project by id, which this run then owns. Null if there's none. */
    get(id: string, options?: { onHookPrompt?: HookPromptHandler }): Promise<Session | null>;
    /** The sessions this workflow created. */
    list(): Promise<Session[]>;
  };

  readonly agents: {
    list(): Promise<Array<{ id: string; name: string; [key: string]: unknown }>>;
    /**
     * A one-shot, non-interactive run (like a scheduled task) in its own worktree.
     * Resolves when the agent finishes, with its final message.
     */
    run(options: string | AgentRunOptions): Promise<AgentRunResult>;
  };

  /** Listen for a session event anywhere in this project. Returns an unsubscribe function. */
  on<E extends keyof SpwnEvents>(event: E, handler: (payload: SpwnEvents[E]) => unknown): () => void;
  off<E extends keyof SpwnEvents>(event: E, handler: (payload: SpwnEvents[E]) => unknown): void;
}

export interface CreateSessionOptions {
  title?: string;
  /** Your id for this session (a ticket id, say) — find it again with `sessions.find`. */
  key?: string;
  /** Agent definition id (default: Settings' default agent). */
  agent?: string;
  /** Sent once the agent is ready; then `waitForTurn()` waits for the reply. */
  prompt?: string;
  /** A permission mode from the agent's definition, applied at launch. */
  permissionMode?: string;
  /** How long to wait for the agent to be ready for `prompt`. Default 90s. */
  readyTimeoutMs?: number;
  /** Answers `spwn prompt` questions from this session's hooks. Without it they're declined. */
  onHookPrompt?: HookPromptHandler;
}

export interface Session {
  /** spwn's id for the session (what `sessions.get` takes). */
  readonly id: string;
  readonly title: string;
  readonly kind: "agent" | "shell";
  readonly agent: string | null;
  /** The workflow that created it, if one did. */
  readonly workflow: string | null;
  readonly key: string | null;
  readonly branch: string | null;
  readonly baseBranch: string | null;
  /** Its worktree. */
  readonly cwd: string;
  /** The agent's own conversation id. */
  readonly sessionId: string | null;
  /**
   * A prompt was submitted and `waitForTurn` hasn't returned its reply yet — kept across
   * runs, so after a stop or restart call `waitForTurn()` instead of sending it again.
   * Always false for sessions no workflow created.
   */
  readonly awaitingTurn: boolean;

  status(): Promise<SessionStatus>;
  /** Type `text` into the agent and submit it (unless `submit: false`). */
  send(text: string, options?: { submit?: boolean }): Promise<void>;
  /**
   * Wait for the reply to the last `send` (or the create `prompt`), including its commit.
   * Returns early with `blocked: true` if the agent stops to ask for permission or input.
   */
  waitForTurn(options?: { timeoutMs?: number }): Promise<TurnResult>;
  /** `send` then `waitForTurn`. */
  prompt(text: string, options?: { timeoutMs?: number }): Promise<TurnResult>;
  waitForStatus(status: SessionStatus | SessionStatus[], options?: { timeoutMs?: number }): Promise<SessionStatus>;
  /** The agent's screen as text. */
  screen(): Promise<string>;
  /** Press a key (tmux names: `Enter`, `Escape`, `Up`, `C-c`, `BTab`, …). */
  press(key: string): Promise<void>;
  interrupt(): Promise<void>;
  transcript(): Promise<TranscriptTurn[]>;
  /** The agent's reply to the latest prompt. */
  lastMessage(): Promise<string | null>;
  /** Delete the session, its worktree and its branch (like the sidebar's ×). */
  delete(): Promise<void>;
  refresh(): Promise<this>;
}

export type TurnResult =
  | { blocked?: undefined; turnUuid: string; text: string | null; status: SessionStatus }
  | { blocked: true; status: SessionStatus; screen: string };

export interface HookPrompt {
  /** The hook event that asked (`session-created`, …). */
  event: string;
  /** The session's id. */
  session: string;
  question: string;
  header?: string | null;
  multiSelect: boolean;
  options: Array<{ label: string; description?: string | null }>;
}

/** Return an option's label (or several, for `multiSelect`); `null` declines. */
export type HookPromptHandler = (
  prompt: HookPrompt,
) => string | string[] | null | undefined | Promise<string | string[] | null | undefined>;

export interface AgentRunOptions {
  prompt: string;
  agent?: string;
  title?: string;
  key?: string;
  onHookPrompt?: HookPromptHandler;
}

export interface AgentRunResult {
  ok: boolean;
  error: string | null;
  /** The run's session, which stays in the sidebar to read. */
  sessionId: string;
  text: string | null;
}

export interface TranscriptTurn {
  uuid: string;
  parentUuid: string | null;
  role: "user" | "assistant";
  timestamp: string | null;
  model: string | null;
  blocks: Array<{
    kind: "text" | "thinking" | "toolUse" | "toolResult";
    text: string | null;
    name: string | null;
    isError: boolean | null;
    id: string | null;
  }>;
}

export interface ExecOptions {
  /** Relative to the project dir. */
  cwd?: string;
  env?: Record<string, string>;
  /** Written to stdin. */
  input?: string;
  /** Default 10 minutes. */
  timeoutMs?: number;
}

export interface ExecResult {
  /** Null when the process was killed by a signal. */
  code: number | null;
  ok: boolean;
  stdout: string;
  stderr: string;
}

export interface FetchOptions {
  method?: string;
  headers?: Record<string, string>;
  body?: string;
  /** Sent as the JSON body (sets Content-Type). */
  json?: unknown;
  /** Default 60s. */
  timeoutMs?: number;
}

export interface FetchResponse {
  status: number;
  ok: boolean;
  headers: Record<string, string>;
  body: string;
  text(): Promise<string>;
  json<T = any>(): Promise<T>;
}

/** A session lifecycle event — its hooks, if any, have already run. */
export interface SessionEvent {
  event: "session-created" | "session-ready" | "session-turn" | "session-deleted";
  projectId: string;
  /** The session's id (`Session.id`). */
  terminalId: string;
  sessionId?: string | null;
  /** Set for `session-turn`. */
  turnUuid?: string | null;
  branch?: string | null;
  worktree?: string | null;
  workflow?: { name: string; key?: string | null } | null;
}

export interface SpwnEvents {
  "session-created": SessionEvent;
  "session-ready": SessionEvent;
  "session-turn": SessionEvent;
  "session-deleted": SessionEvent;
  /** A session's live status changed. */
  status: { terminalId: string; status: SessionStatus };
}
