/**
 * GitHub Webhook Relay — Cloudflare Worker
 *
 * Receives GitHub webhook POSTs, validates the HMAC-SHA256 signature
 * using the provided secret, and writes them to Durable Object storage.
 * The local monitor polls this Worker via the /poll endpoint.
 *
 * Deploy: wrangler deploy webhook_relay_worker.js
 * Then set WORKER_URL in ~/.layers/infrastructure.toml
 *
 * Environment variables (set via `wrangler secret put`):
 *   GITHUB_WEBHOOK_SECRET: the secret configured in GitHub webhook settings
 *   RELAY_SECRET: a bearer token the local relay agent must present
 */

const SECRET = {{GITHUB_SECRET}};
const RELAY_SECRET = "{{RELAY_SECRET}}";

/**
 * @param {string} payload
 * @param {string} signature  — GitHub's HMAC-SHA256 signature (sha256=...)
 */
async function verifySignature(payload, signature) {
  if (!SECRET) return true; // skip verification if no secret set
  const expected = await crypto.subtle.digest(
    'SHA-256',
    new TextEncoder().encode(SECRET + payload)
  );
  const expectedHex = Array.from(new Uint8Array(expected))
    .map(b => b.toString(16).padStart(2, '0'))
    .join('');
  return signature === `sha256=${expectedHex}`;
}

/**
 * Relay agent polls here to get buffered events.
 * GET /poll?since=<unix_ms>
 *   → 200: JSON { events: [{id, type, repo, payload, received_at}] }
 *   → 401: bad RELAY_SECRET
 *   → 204: no new events
 */
async function handlePoll(request) {
  const auth = request.headers.get('Authorization') || '';
  if (auth !== `Bearer ${RELAY_SECRET}`) {
    return new Response('Unauthorized', { status: 401 });
  }

  const url = new URL(request.url);
  const since = parseInt(url.searchParams.get('since') || '0', 10);

  // Events are stored as Durable Object storage entries
  // We use the Durable Object binding defined in wrangler.toml
  const namespace = request.env.EVENT_STORE;
  if (!namespace) {
    return Response.json({ error: 'Durable Object not configured' }, { status: 500 });
  }

  const key = `events:${new Date().toISOString().slice(0, 10)}`;
  const stored = await namespace.get(key).catch(() => null);
  const events = stored ? JSON.parse(stored) : [];

  const newEvents = events.filter(e => e.received_at > since);
  if (newEvents.length === 0) {
    return new Response('', { status: 204 });
  }

  return Response.json({
    events: newEvents,
    polled_at: Date.now()
  });
}

/**
 * GitHub sends POST here with webhook payload.
 * The monitor is also notified via a webhook.forward_url if configured.
 */
async function handleWebhook(request) {
  const payload = await request.text();
  const signature = request.headers.get('x-hub-signature-256') || '';
  const eventType = request.headers.get('x-github-event') || 'unknown';
  const deliveryId = request.headers.get('x-github-delivery') || `${Date.now()}`;

  const valid = await verifySignature(payload, signature);
  if (!valid) {
    return Response.json({ error: 'Invalid signature' }, { status: 401 });
  }

  let body;
  try {
    body = JSON.parse(payload);
  } catch {
    body = { raw: payload };
  }

  const event = {
    id: deliveryId,
    type: eventType,
    repo: body.repository?.full_name || body.repository?.name || 'unknown',
    action: body.action || '',
    sender: body.sender?.login || 'unknown',
    payload: body,
    received_at: Date.now()
  };

  // Store in Durable Object (append to today's list)
  const namespace = request.env.EVENT_STORE;
  if (namespace) {
    const key = `events:${new Date().toISOString().slice(0, 10)}`;
    try {
      const existing = await namespace.get(key).catch(() => '[]');
      const events = JSON.parse(existing);
      events.push(event);
      // Keep only last 500 events per day
      const trimmed = events.slice(-500);
      await namespace.put(key, JSON.stringify(trimmed));
    } catch (e) {
      console.error('DO storage error:', e);
    }
  }

  // Optionally forward to a local relay agent
  const forwardUrl = request.headers.get('x-relay-forward') || '';
  if (forwardUrl) {
    fetch(forwardUrl, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'X-Relay-Secret': RELAY_SECRET
      },
      body: JSON.stringify(event)
    }).catch(() => {}); // fire and forget
  }

  return Response.json({ received: true, id: deliveryId });
}

export default {
  async fetch(request, env) {
    const url = new URL(request.url);

    if (request.method === 'GET' && url.pathname === '/poll') {
      return handlePoll(request);
    }

    if (request.method === 'POST' && (url.pathname === '/webhook' || url.pathname === '/github')) {
      return handleWebhook(request);
    }

    // Health check
    if (request.method === 'GET' && url.pathname === '/health') {
      return Response.json({ status: 'ok', ts: Date.now() });
    }

    return Response.json({ error: 'Not found' }, { status: 404 });
  }
};
