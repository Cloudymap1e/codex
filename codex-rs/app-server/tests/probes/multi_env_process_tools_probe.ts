#!/usr/bin/env node
/*
 * App-server probe for the model-facing multi-environment process-tool surface.
 *
 * Example:
 *   cd <repo>
 *   source scripts/test-remote-env.sh
 *   node --experimental-strip-types \
 *     codex-rs/app-server/tests/probes/multi_env_process_tools_probe.ts \
 *     --app-server-bin codex-rs/target/debug/codex-app-server
 *   codex_remote_env_cleanup
 */

import { createServer } from "node:http";
import { mkdtemp, mkdir, writeFile } from "node:fs/promises";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { dirname, isAbsolute, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(scriptDir, "../../../..");

const args = new Map();
for (let index = 2; index < process.argv.length; index += 1) {
  const arg = process.argv[index];
  if (!arg.startsWith("--")) {
    throw new Error(`unexpected positional argument: ${arg}`);
  }
  const value = process.argv[index + 1];
  if (value == null || value.startsWith("--")) {
    throw new Error(`missing value for ${arg}`);
  }
  args.set(arg, value);
  index += 1;
}

function resolveCliPath(value) {
  if (value == null) {
    return undefined;
  }
  return isAbsolute(value) ? value : resolve(process.cwd(), value);
}

const appServerCommand =
  resolveCliPath(args.get("--app-server-bin")) ??
  resolveCliPath(process.env.CODEX_APP_SERVER_BIN) ??
  join(repoRoot, "codex-rs/target/debug/codex-app-server");
const appServerArgs = ["--disable-plugin-startup-tasks-for-tests"];

function sse(events) {
  return events
    .map((event) => `event: ${event.type}\ndata: ${JSON.stringify(event)}\n`)
    .join("\n");
}

function assistantDone(id) {
  return sse([
    { type: "response.created", response: { id } },
    {
      type: "response.output_item.done",
      item: {
        type: "message",
        role: "assistant",
        id: `${id}-msg`,
        content: [{ type: "output_text", text: "done" }],
      },
    },
    {
      type: "response.completed",
      response: {
        id,
        usage: {
          input_tokens: 0,
          input_tokens_details: null,
          output_tokens: 0,
          output_tokens_details: null,
          total_tokens: 0,
        },
      },
    },
  ]);
}

function remoteExecCall(id, markerPath) {
  const argumentsJson = JSON.stringify({
    cmd: `cat ${markerPath}`,
    environment_id: "remote",
    shell: "/bin/sh",
    yield_time_ms: 1000,
    max_output_tokens: 2000,
  });
  return sse([
    { type: "response.created", response: { id } },
    {
      type: "response.output_item.done",
      item: {
        type: "function_call",
        call_id: "call_remote_env",
        name: "exec_command",
        arguments: argumentsJson,
      },
    },
    {
      type: "response.completed",
      response: {
        id,
        usage: {
          input_tokens: 0,
          input_tokens_details: null,
          output_tokens: 0,
          output_tokens_details: null,
          total_tokens: 0,
        },
      },
    },
  ]);
}

async function startMockResponsesServer(responses) {
  const requests = [];
  let responseIndex = 0;
  const server = createServer((req, res) => {
    if (req.method !== "POST" || !req.url?.endsWith("/responses")) {
      res.writeHead(404).end();
      return;
    }

    const chunks = [];
    req.on("data", (chunk) => chunks.push(Buffer.from(chunk)));
    req.on("end", () => {
      requests.push(Buffer.concat(chunks).toString("utf8"));
      const body = responses[Math.min(responseIndex, responses.length - 1)];
      responseIndex += 1;
      res.writeHead(200, { "content-type": "text/event-stream" });
      res.end(body);
    });
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  const address = server.address();
  if (address == null || typeof address === "string") {
    throw new Error("mock responses server did not bind a TCP address");
  }
  return {
    url: `http://127.0.0.1:${address.port}`,
    requests,
    close: () => server.close(),
  };
}

class AppServerClient {
  nextId = 0;
  buffer = "";
  pending = [];
  child;

  constructor(codexHome, remoteExecServerUrl) {
    this.child = spawn(appServerCommand, appServerArgs, {
      cwd: codexHome,
      env: {
        ...process.env,
        CODEX_HOME: codexHome,
        CODEX_APP_SERVER_MANAGED_CONFIG_PATH: join(codexHome, "managed_config.toml"),
        CODEX_EXEC_SERVER_URL: remoteExecServerUrl,
        RUST_LOG: "warn",
      },
      stdio: ["pipe", "pipe", "inherit"],
    });
    this.child.stdout.setEncoding("utf8");
    this.child.stdout.on("data", (chunk) => {
      this.buffer += chunk;
      for (;;) {
        const newline = this.buffer.indexOf("\n");
        if (newline === -1) {
          break;
        }
        const line = this.buffer.slice(0, newline).trim();
        this.buffer = this.buffer.slice(newline + 1);
        if (line.length > 0) {
          this.pending.push(JSON.parse(line));
        }
      }
    });
  }

  async initialize() {
    const response = await this.request("initialize", {
      clientInfo: {
        name: "multi-env-process-tools-probe",
        title: null,
        version: "0.1.0",
      },
      capabilities: {
        experimentalApi: true,
        optOutNotificationMethods: [],
      },
    });
    this.notify("initialized");
    return response;
  }

  request(method, params) {
    const id = this.nextId;
    this.nextId += 1;
    this.write({ jsonrpc: "2.0", id, method, params });
    return this.waitFor((message) => message.id === id && "result" in message);
  }

  notify(method, params) {
    this.write(
      params == null ? { jsonrpc: "2.0", method } : { jsonrpc: "2.0", method, params },
    );
  }

  async waitFor(predicate) {
    const deadline = Date.now() + 20_000;
    for (;;) {
      const index = this.pending.findIndex(predicate);
      if (index !== -1) {
        return this.pending.splice(index, 1)[0];
      }
      if (Date.now() > deadline) {
        throw new Error(
          `timed out waiting for app-server message; pending=${JSON.stringify(this.pending)}`,
        );
      }
      await new Promise((resolve) => setTimeout(resolve, 25));
    }
  }

  async waitForNotification(method) {
    return this.waitFor((message) => message.method === method);
  }

  async close() {
    this.child.kill();
    await once(this.child, "exit").catch(() => undefined);
  }

  write(message) {
    this.child.stdin.write(`${JSON.stringify(message)}\n`);
  }
}

function assertContains(haystack, needle, label) {
  if (!haystack.includes(needle)) {
    throw new Error(`${label} missing ${needle}`);
  }
}

function assertLacks(haystack, needle, label) {
  if (haystack.includes(needle)) {
    throw new Error(`${label} unexpectedly included ${needle}`);
  }
}

function userInput(text) {
  return [{ type: "text", text, text_elements: [] }];
}

async function runTurn(client, workspace, environmentIds, text) {
  const threadStart = await client.request("thread/start", {
    model: "mock-model",
    cwd: workspace,
    environments: environmentIds.map((environmentId) => ({
      environmentId,
      cwd: environmentId === "remote" ? "/tmp" : workspace,
    })),
  });
  const thread = threadStart.result.thread;
  await client.request("turn/start", {
    threadId: thread.id,
    input: userInput(text),
    cwd: workspace,
    model: "mock-model",
  });
  await client.waitForNotification("turn/completed");
}

async function main() {
  const tmp = await mkdtemp(join(process.env.TMPDIR ?? "/tmp", "codex-multi-env-probe-"));
  const codexHome = join(tmp, "codex_home");
  const workspace = join(tmp, "workspace");
  await mkdir(codexHome);
  await mkdir(workspace);

  const marker = `codex-remote-routing-${Date.now()}`;
  const markerPath = `/tmp/${marker}.txt`;
  const remoteHarnessReady =
    process.env.CODEX_TEST_REMOTE_ENV != null && process.env.CODEX_TEST_REMOTE_EXEC_SERVER_URL != null;
  const mock = await startMockResponsesServer([
    assistantDone("single"),
    assistantDone("multi"),
    ...(remoteHarnessReady
      ? [remoteExecCall("remote-call", markerPath), assistantDone("remote-done")]
      : []),
  ]);
  await writeFile(
    join(codexHome, "config.toml"),
    `
model = "mock-model"
approval_policy = "never"
sandbox_mode = "read-only"
model_provider = "mock_provider"

[features]
unified_exec = true

[model_providers.mock_provider]
name = "Mock provider for probe"
base_url = "${mock.url}/v1"
wire_api = "responses"
request_max_retries = 0
stream_max_retries = 0
supports_websockets = false
`,
  );

  if (remoteHarnessReady) {
    await new Promise((resolvePromise, reject) => {
      const child = spawn("docker", [
        "exec",
        process.env.CODEX_TEST_REMOTE_ENV,
        "sh",
        "-lc",
        `printf '%s\\n' '${marker}' > ${markerPath}`,
      ]);
      child.on("exit", (code) =>
        code === 0 ? resolvePromise() : reject(new Error(`docker exec exited ${code}`)),
      );
      child.on("error", reject);
    });
  }

  const client = new AppServerClient(
    codexHome,
    process.env.CODEX_TEST_REMOTE_EXEC_SERVER_URL ?? "http://127.0.0.1:1",
  );
  try {
    await client.initialize();

    await runTurn(client, workspace, ["local"], "single selected environment");
    const singleBody = mock.requests.at(-1) ?? "";
    assertContains(singleBody, "<cwd>", "single-env model request");
    assertLacks(singleBody, "<environments>", "single-env model request");
    assertLacks(singleBody, "environment_id", "single-env model request");

    await runTurn(client, workspace, ["local", "remote"], "multi selected environments");
    const multiBody = mock.requests.at(-1) ?? "";
    assertContains(multiBody, "<environments>", "multi-env model request");
    assertContains(multiBody, '<environment id=\\"local\\" primary=\\"true\\">', "multi-env model request");
    assertContains(multiBody, '<environment id=\\"remote\\" primary=\\"false\\">', "multi-env model request");
    assertContains(multiBody, "environment_id", "multi-env model request");

    if (remoteHarnessReady) {
      await runTurn(client, workspace, ["local", "remote"], "route explicit remote exec_command");
      const toolResultRequest = mock.requests.at(-1) ?? "";
      assertContains(toolResultRequest, marker, "explicit remote environment_id routing result");
    } else {
      console.log("remote routing assertion skipped: source scripts/test-remote-env.sh to enable it");
    }

    console.log("multi-env process tool app-server probe passed");
  } finally {
    mock.close();
    await client.close();
  }
}

await main();
