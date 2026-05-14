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
};

const MAX_BODY_BYTES = 20 * 1024;
const MAX_SCENARIO_BYTES = 16 * 1024;
const MAX_CONTACT_BYTES = 256;
const MAX_TAGS = 10;
const textEncoder = new TextEncoder();

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);

    if (request.method === "OPTIONS") {
      return new Response(null, { status: 204, headers: corsHeaders() });
    }

    if (request.method !== "POST" || url.pathname !== "/feedback") {
      return json({ status: "error", error: "not_found" }, 404);
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

    const id = crypto.randomUUID();
    const createdAt = new Date().toISOString();

    try {
      await env.DB.prepare(
        `INSERT INTO feedback (id, created_at, source, version, scenario_json, contact, tags_json, user_agent, cf_ray)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)`,
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
    },
  };
}

function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: {
      "content-type": "application/json; charset=utf-8",
      ...corsHeaders(),
    },
  });
}

function corsHeaders(): HeadersInit {
  return {
    "access-control-allow-origin": "*",
    "access-control-allow-methods": "POST, OPTIONS",
    "access-control-allow-headers": "content-type, authorization",
    "access-control-max-age": "86400",
  };
}
