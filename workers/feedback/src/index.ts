export interface Env {
  DB: D1Database;
}

type FeedbackPayload = {
  source?: unknown;
  version?: unknown;
  scenario?: unknown;
  contact?: unknown;
  tags?: unknown;
};

type ValidFeedback = {
  source: string;
  version: string | null;
  scenarioJson: string;
  contact: string | null;
  tags: string[];
  agentAddress: string | null;
};

type RateLimitResult =
  | { ok: true; ipHash: string }
  | { ok: false; retryAfterSeconds: number };

const MAX_BODY_BYTES = 20 * 1024;
const MAX_SCENARIO_BYTES = 16 * 1024;
const MAX_CONTACT_BYTES = 256;
const MAX_TAGS = 10;
const GLOBAL_RATE_LIMIT_WINDOW_SECONDS = 60;
const MAX_GLOBAL_SUBMISSIONS_PER_WINDOW = 1;
const DAILY_RATE_LIMIT_WINDOW_SECONDS = 24 * 60 * 60;
const MAX_SUBMISSIONS_PER_DAILY_KEY = 1;
const RATE_LIMIT_RETENTION_SECONDS = 8 * 24 * 60 * 60;
const textEncoder = new TextEncoder();

export default {
  async fetch(request: Request, env: Env, ctx: ExecutionContext): Promise<Response> {
    const url = new URL(request.url);

    if (request.method === "OPTIONS") {
      return new Response(null, { status: 204 });
    }

    if (request.method !== "POST" || url.pathname !== "/feedback") {
      return json({ status: "error", error: "not_found" }, 404);
    }

    if (!isJsonRequest(request)) {
      return json({ status: "error", error: "unsupported_media_type" }, 415);
    }

    const contentLength = request.headers.get("content-length");
    if (contentLength && Number(contentLength) > MAX_BODY_BYTES) {
      return json({ status: "error", error: "payload_too_large" }, 413);
    }

    let payload: FeedbackPayload;
    try {
      const body = await request.arrayBuffer();
      if (body.byteLength > MAX_BODY_BYTES) {
        return json({ status: "error", error: "payload_too_large" }, 413);
      }
      payload = JSON.parse(new TextDecoder().decode(body)) as FeedbackPayload;
    } catch {
      return json({ status: "error", error: "invalid_json" }, 400);
    }

    const validation = validatePayload(payload);
    if (!validation.ok) {
      return json({ status: "error", error: validation.error }, 400);
    }
    const feedback = validation.feedback;

    let rateLimit: RateLimitResult;
    try {
      rateLimit = await enforceRateLimits(feedback.agentAddress, request, env);
    } catch {
      return json({ status: "error", error: "internal_rate_limit_error" }, 500);
    }
    ctx.waitUntil(cleanupOldRateLimitWindows(env, Math.floor(Date.now() / 1000)));
    if (!rateLimit.ok) {
      return json(
        { status: "error", error: "rate_limited" },
        429,
        { "retry-after": String(rateLimit.retryAfterSeconds) },
      );
    }

    const id = crypto.randomUUID();
    const createdAt = new Date().toISOString();

    try {
      await env.DB.prepare(
        `INSERT INTO feedback (id, created_at, source, version, scenario_json, contact, tags_json, user_agent, cf_ray, ip_hash)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
      )
        .bind(
          id,
          createdAt,
          feedback.source,
          feedback.version,
          feedback.scenarioJson,
          feedback.contact,
          JSON.stringify(feedback.tags),
          request.headers.get("user-agent"),
          request.headers.get("cf-ray"),
          rateLimit.ipHash,
        )
        .run();
    } catch {
      return json({ status: "error", error: "internal_storage_error" }, 500);
    }

    return json({ status: "accepted", id }, 202);
  },
};

function validatePayload(
  payload: FeedbackPayload,
): { ok: true; feedback: ValidFeedback } | { ok: false; error: string } {
  if (!payload || typeof payload !== "object" || Array.isArray(payload)) {
    return { ok: false, error: "payload_must_be_object" };
  }

  if (!payload.scenario || typeof payload.scenario !== "object" || Array.isArray(payload.scenario)) {
    return { ok: false, error: "scenario_must_be_object" };
  }

  const scenarioJson = JSON.stringify(payload.scenario);
  if (textEncoder.encode(scenarioJson).byteLength > MAX_SCENARIO_BYTES) {
    return { ok: false, error: "scenario_too_large" };
  }

  const source = typeof payload.source === "string" && payload.source.trim() !== ""
    ? payload.source.trim()
    : "hyperliquid-cli";
  const version = typeof payload.version === "string" ? payload.version.trim() : null;
  let contact: string | null = null;
  if (payload.contact !== undefined) {
    if (typeof payload.contact !== "string") {
      return { ok: false, error: "invalid_contact" };
    }
    contact = payload.contact.trim();
    if (contact === "" || textEncoder.encode(contact).byteLength > MAX_CONTACT_BYTES) {
      return { ok: false, error: "invalid_contact" };
    }
  }

  const tags: string[] = [];
  if (payload.tags !== undefined) {
    if (!Array.isArray(payload.tags) || payload.tags.length > MAX_TAGS) {
      return { ok: false, error: "invalid_tags" };
    }
    for (const tag of payload.tags) {
      if (typeof tag !== "string" || !/^[a-zA-Z0-9_-]{1,64}$/.test(tag)) {
        return { ok: false, error: "invalid_tag" };
      }
      tags.push(tag.trim().toLowerCase());
    }
  }

  return {
    ok: true,
    feedback: {
      source,
      version,
      scenarioJson,
      contact,
      tags,
      agentAddress: extractAgentAddress(payload.scenario),
    },
  };
}

function extractAgentAddress(scenario: object): string | null {
  const candidate = stringField(scenario, "agent_address", "agentAddress")
    ?? stringField(scenario, "signer_address", "signerAddress")
    ?? stringField(scenario, "wallet_address", "walletAddress");
  if (candidate === null) {
    return null;
  }
  const normalized = candidate.trim().toLowerCase();
  return /^0x[a-f0-9]{40}$/.test(normalized) ? normalized : null;
}

function stringField(source: object, snake: string, camel: string): string | null {
  const record = source as Record<string, unknown>;
  const value = record[snake] ?? record[camel];
  return typeof value === "string" && value.trim() !== "" ? value : null;
}

function isJsonRequest(request: Request): boolean {
  const contentType = request.headers.get("content-type") ?? "";
  return contentType.toLowerCase().split(";", 1)[0].trim() === "application/json";
}

async function enforceRateLimits(
  agentAddress: string | null,
  request: Request,
  env: Env,
): Promise<RateLimitResult> {
  const nowSeconds = Math.floor(Date.now() / 1000);
  const ipHash = await sha256Hex(clientIp(request));
  const ipKey = `ip:${ipHash}`;
  const agentKey = agentAddress !== null ? `agent:${await sha256Hex(agentAddress)}` : null;
  const dayStart = nowSeconds - (nowSeconds % DAILY_RATE_LIMIT_WINDOW_SECONDS);
  const minuteStart = nowSeconds - (nowSeconds % GLOBAL_RATE_LIMIT_WINDOW_SECONDS);

  const ipLimit = await incrementRateLimitWindow(
    env,
    ipKey,
    dayStart,
    nowSeconds,
    MAX_SUBMISSIONS_PER_DAILY_KEY,
    DAILY_RATE_LIMIT_WINDOW_SECONDS,
  );
  if (!ipLimit.ok) {
    return ipLimit;
  }

  if (agentKey !== null) {
    const agentLimit = await incrementRateLimitWindow(
      env,
      agentKey,
      dayStart,
      nowSeconds,
      MAX_SUBMISSIONS_PER_DAILY_KEY,
      DAILY_RATE_LIMIT_WINDOW_SECONDS,
    );
    if (!agentLimit.ok) {
      await decrementRateLimitWindow(env, ipKey, dayStart);
      return agentLimit;
    }
  }

  const globalLimit = await incrementRateLimitWindow(
    env,
    "global",
    minuteStart,
    nowSeconds,
    MAX_GLOBAL_SUBMISSIONS_PER_WINDOW,
    GLOBAL_RATE_LIMIT_WINDOW_SECONDS,
  );
  if (!globalLimit.ok) {
    await decrementRateLimitWindow(env, ipKey, dayStart);
    if (agentKey !== null) {
      await decrementRateLimitWindow(env, agentKey, dayStart);
    }
    return globalLimit;
  }

  return { ok: true, ipHash };
}

async function incrementRateLimitWindow(
  env: Env,
  keyHash: string,
  windowStart: number,
  nowSeconds: number,
  maxSubmissions: number,
  windowSeconds: number,
): Promise<{ ok: true } | { ok: false; retryAfterSeconds: number }> {
  const updatedAt = new Date(nowSeconds * 1000).toISOString();
  await env.DB.prepare(
    `INSERT INTO feedback_rate_limits (ip_hash, window_start, count, updated_at)
     VALUES (?, ?, 1, ?)
     ON CONFLICT(ip_hash, window_start)
     DO UPDATE SET count = count + 1, updated_at = excluded.updated_at`,
  )
    .bind(keyHash, windowStart, updatedAt)
    .run();

  const row = await env.DB.prepare(
    `SELECT count FROM feedback_rate_limits WHERE ip_hash = ? AND window_start = ?`,
  )
    .bind(keyHash, windowStart)
    .first<{ count: number }>();

  const count = row?.count ?? maxSubmissions + 1;
  if (count > maxSubmissions) {
    return {
      ok: false,
      retryAfterSeconds: Math.max(1, windowStart + windowSeconds - nowSeconds),
    };
  }

  return { ok: true };
}

async function decrementRateLimitWindow(
  env: Env,
  keyHash: string,
  windowStart: number,
): Promise<void> {
  await env.DB.prepare(
    `UPDATE feedback_rate_limits
     SET count = MAX(count - 1, 0)
     WHERE ip_hash = ? AND window_start = ?`,
  )
    .bind(keyHash, windowStart)
    .run();
}

function clientIp(request: Request): string {
  return (
    request.headers.get("cf-connecting-ip")
    ?? request.headers.get("x-forwarded-for")?.split(",", 1)[0]?.trim()
    ?? "unknown"
  );
}

async function sha256Hex(value: string): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", textEncoder.encode(value));
  return [...new Uint8Array(digest)]
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join("");
}

async function cleanupOldRateLimitWindows(env: Env, nowSeconds: number): Promise<void> {
  const cutoff = nowSeconds - RATE_LIMIT_RETENTION_SECONDS;
  await env.DB.prepare("DELETE FROM feedback_rate_limits WHERE window_start < ?")
    .bind(cutoff)
    .run();
}

function json(body: unknown, status = 200, extraHeaders: HeadersInit = {}): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: {
      "content-type": "application/json; charset=utf-8",
      ...extraHeaders,
    },
  });
}
