// Work a GitHub project board with a persona per column.
//
// Every `pollSeconds` this reads the board. Each ticket gets ONE spwn session, filed
// under the board item's id, so it keeps its worktree and conversation as it moves
// across the board. When a ticket lands in a column that has a persona, its session is
// prompted with that persona's instructions for the column. When the persona finishes,
// the ticket can move on to the next column by itself.
//
// Use it: copy this file and `spwn.d.ts` (the Workflows panel's "New workflow" writes
// one) into your project's `.spwn/workflows/`, save a GitHub token with the `project`
// and `repo` scopes in spwn's Settings, allow workflows for the project, and Run it.
//
// Everything below is yours to change: the personas, the columns, the prompts, what
// happens when a turn ends.

import type { Session, Spwn, WorkflowMeta } from "./spwn";

export const meta: WorkflowMeta = {
  name: "GitHub board",
  description: "Work tickets on a GitHub project board, a persona per column",
  keepAlive: true,
  inputs: {
    owner: { description: "User or organization that owns the project", required: true },
    project: { type: "number", description: "Project number, from its URL", required: true },
    statusField: { default: "Status", description: "The single-select field used as columns" },
    pollSeconds: { type: "number", default: 60 },
    maxConcurrent: { type: "number", default: 3, description: "Tickets worked at once" },
  },
};

interface Inputs {
  owner: string;
  project: number;
  statusField: string;
  pollSeconds: number;
  maxConcurrent: number;
}

// --- Personas: who works a ticket, and how they think -------------------------------

interface Persona {
  name: string;
  /** Agent definition to run (default: spwn's default agent). */
  agent?: string;
  preamble: string;
}

const PERSONAS = {
  architect: {
    name: "Architect",
    preamble:
      "You are a pragmatic software architect. You read the codebase before proposing " +
      "anything, prefer the smallest design that solves the problem, and call out risks.",
  },
  engineer: {
    name: "Engineer",
    preamble:
      "You are a careful senior engineer. You follow the existing conventions, keep " +
      "changes focused, and add or update tests for what you change.",
  },
  reviewer: {
    name: "Reviewer",
    preamble:
      "You are a demanding code reviewer. You look for bugs, missing tests and unclear " +
      "code, and you fix what you find rather than only listing it.",
  },
} satisfies Record<string, Persona>;

// --- Columns: which persona works a ticket in each column, and what they're asked ----

interface Column {
  persona: keyof typeof PERSONAS;
  prompt: (ticket: Ticket) => string;
  /** Move the ticket to this column when the persona's turn finishes. */
  then?: string;
}

const COLUMNS: Record<string, Column> = {
  Ready: {
    persona: "architect",
    prompt: (t) =>
      `${describe(t)}\n\nWrite an implementation plan for this ticket in PLAN.md at the ` +
      `root of the repository. Don't implement it yet.`,
    then: "In progress",
  },
  "In progress": {
    persona: "engineer",
    prompt: (t) =>
      `${describe(t)}\n\nImplement the plan in PLAN.md. Run the tests and make sure they pass.`,
    then: "In review",
  },
  "In review": {
    persona: "reviewer",
    prompt: (t) =>
      `${describe(t)}\n\nReview the changes on this branch against the ticket and PLAN.md. ` +
      `Fix any problems you find, then summarize what you checked.`,
  },
};

// --- The board -----------------------------------------------------------------------

interface Ticket {
  itemId: string;
  column: string | null;
  title: string;
  body: string;
  url: string | null;
  number: number | null;
}

interface Board {
  projectId: string;
  fieldId: string | null;
  /** Column name → single-select option id. */
  options: Record<string, string>;
  tickets: Ticket[];
}

const describe = (t: Ticket) =>
  [`Ticket${t.number ? ` #${t.number}` : ""}: ${t.title}`, t.url, "", t.body || "(no description)"]
    .filter((line) => line !== null)
    .join("\n");

const BOARD_QUERY = `
  query Board($owner: String!, $number: Int!, $field: String!, $cursor: String) {
    repositoryOwner(login: $owner) {
      ... on ProjectOwner {
        projectV2(number: $number) {
          id
          field(name: $field) {
            ... on ProjectV2SingleSelectField { id options { id name } }
          }
          items(first: 100, after: $cursor) {
            pageInfo { hasNextPage endCursor }
            nodes {
              id
              fieldValueByName(name: $field) {
                ... on ProjectV2ItemFieldSingleSelectValue { name }
              }
              content {
                ... on Issue { title body url number }
                ... on PullRequest { title body url number }
                ... on DraftIssue { title body }
              }
            }
          }
        }
      }
    }
  }`;

async function readBoard(spwn: Spwn, inputs: Inputs): Promise<Board> {
  const board: Board = { projectId: "", fieldId: null, options: {}, tickets: [] };
  let cursor: string | null = null;
  do {
    const data: any = await spwn.github.graphql(BOARD_QUERY, {
      owner: inputs.owner,
      number: Number(inputs.project),
      field: inputs.statusField,
      cursor,
    });
    const project = data?.repositoryOwner?.projectV2;
    if (!project) throw new Error(`no project #${inputs.project} for ${inputs.owner}`);
    board.projectId = project.id;
    board.fieldId = project.field?.id ?? null;
    for (const o of project.field?.options ?? []) board.options[o.name] = o.id;
    for (const item of project.items.nodes) {
      if (!item?.content) continue;
      board.tickets.push({
        itemId: item.id,
        column: item.fieldValueByName?.name ?? null,
        title: item.content.title ?? "(untitled)",
        body: item.content.body ?? "",
        url: item.content.url ?? null,
        number: item.content.number ?? null,
      });
    }
    cursor = project.items.pageInfo.hasNextPage ? project.items.pageInfo.endCursor : null;
  } while (cursor);
  return board;
}

async function moveTo(spwn: Spwn, board: Board, ticket: Ticket, column: string) {
  const optionId = board.options[column];
  if (!board.fieldId || !optionId) {
    spwn.warn(`can't move "${ticket.title}": the board has no "${column}" column`);
    return;
  }
  await spwn.github.graphql(
    `mutation Move($project: ID!, $item: ID!, $field: ID!, $option: String!) {
       updateProjectV2ItemFieldValue(input: {
         projectId: $project, itemId: $item, fieldId: $field,
         value: { singleSelectOptionId: $option }
       }) { projectV2Item { id } }
     }`,
    { project: board.projectId, item: ticket.itemId, field: board.fieldId, option: optionId },
  );
  spwn.log(`"${ticket.title}" → ${column}`);
}

// --- Working a ticket ------------------------------------------------------------------

async function work(spwn: Spwn, board: Board, ticket: Ticket, column: Column) {
  const persona: Persona = PERSONAS[column.persona];
  const prompt = `${persona.preamble}\n\n${column.prompt(ticket)}`;

  let session: Session | null = await spwn.sessions.find(ticket.itemId);
  if (session?.awaitingTurn) {
    // A run stopped while this prompt was out: collect its reply rather than send it twice.
    spwn.log(`"${ticket.title}" is in ${ticket.column}: resuming ${persona.name}'s turn`);
  } else if (session) {
    spwn.log(`"${ticket.title}" is in ${ticket.column}: prompting its session as ${persona.name}`);
    await session.send(prompt);
  } else {
    spwn.log(`"${ticket.title}" is in ${ticket.column}: starting a session as ${persona.name}`);
    session = await spwn.sessions.create({
      key: ticket.itemId,
      title: ticket.number ? `#${ticket.number} ${ticket.title}` : ticket.title,
      agent: persona.agent,
      prompt,
    });
  }

  const turn = await session.waitForTurn();
  if (turn.blocked) {
    // The agent wants permission or an answer. Leave it for a person; the ticket isn't
    // handed on until someone moves it.
    spwn.warn(`"${ticket.title}": ${persona.name} is waiting for you in "${session.title}"`);
    return;
  }
  spwn.log(`"${ticket.title}": ${persona.name} finished`);
  if (column.then) await moveTo(spwn, board, ticket, column.then);
}

export default async function main(spwn: Spwn, inputs: Inputs) {
  // Item id → the column its session was last prompted for, so a ticket is worked once
  // per column — across polls, restarts, and spwn restarts.
  const handled = spwn.state.get<Record<string, string>>("handled", {});
  const inFlight = new Set<string>();

  while (!spwn.stopping) {
    try {
      const board = await readBoard(spwn, inputs);
      for (const ticket of board.tickets) {
        const column = ticket.column ? COLUMNS[ticket.column] : undefined;
        if (!column || handled[ticket.itemId] === ticket.column) continue;
        if (inFlight.has(ticket.itemId) || inFlight.size >= inputs.maxConcurrent) continue;

        inFlight.add(ticket.itemId);
        work(spwn, board, ticket, column)
          .then(() => {
            handled[ticket.itemId] = ticket.column!;
            spwn.state.set("handled", handled);
          })
          .catch((e) => spwn.error(`"${ticket.title}" failed:`, e))
          .finally(() => inFlight.delete(ticket.itemId));
      }
    } catch (e) {
      spwn.error("couldn't read the board:", e);
    }
    await spwn.sleep(inputs.pollSeconds * 1000);
  }
}
