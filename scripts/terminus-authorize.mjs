#!/usr/bin/env node
// OMNIS GLASS browser TERMINUS authorize — stdin JSON → stdout verdict.
// Same contract as CADUCEUS scripts/terminus-authorize.mjs; browser kind only.
// Deny logic lives here (JS), not in Rust.

import { createHash } from 'node:crypto';
import { appendFileSync, existsSync, mkdirSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
const sha256 = (s) => createHash('sha256').update(String(s)).digest('hex');

const DECISION = {
  PERMIT: 'PERMIT',
  HOLD: 'HOLD',
  REFUSE_SECRET: 'REFUSE_SECRET',
  REFUSE_CROWN: 'REFUSE_CROWN_JEWEL',
  REFUSE_HOST: 'REFUSE_HOST',
  REFUSE_LANE: 'REFUSE_LANE_ACTION',
  REFUSE_SCANNER_FAILURE: 'REFUSE_SCANNER_FAILURE',
};

// Minimal secret scanner — aligned with CADUCEUS terminus.mjs rule ids
const SECRET_RULES = [
  { id: 'private_key', re: /-----BEGIN (?:RSA |EC |DSA |OPENSSH )?PRIVATE KEY-----/ },
  { id: 'aws_access_key', re: /\b(?:AKIA|ASIA)[0-9A-Z]{16}\b/ },
  {
    id: 'bearer_secret',
    re: /\b(?:sk-[A-Za-z0-9_-]{16,64}|ghp_[A-Za-z0-9]{20,40}|xox[baprs]-[A-Za-z0-9-]{10,60})\b/,
  },
];

const CROWN_JEWELS = (process.env.OMNIS_CROWN_JEWELS || '')
  .split(',')
  .map((s) => s.trim())
  .filter(Boolean);

function scanSecrets(text) {
  const s = String(text ?? '');
  for (const rule of SECRET_RULES) {
    const m = rule.re.exec(s);
    if (m) return { refused: true, rule: rule.id, digest: sha256(m[0]) };
  }
  for (const jewel of CROWN_JEWELS) {
    if (jewel && s.includes(jewel)) {
      return { refused: true, rule: 'crown_jewel', digest: sha256(jewel) };
    }
  }
  return { refused: false };
}

function chainHead(path) {
  if (!existsSync(path)) return null;
  const lines = readFileSync(path, 'utf8').trim().split('\n').filter(Boolean);
  if (!lines.length) return null;
  try {
    return JSON.parse(lines[lines.length - 1]).sig;
  } catch {
    return null;
  }
}

function writeReceipt(fields) {
  const state = process.env.OMNIS_GLASS_STATE_DIR || join(process.env.HOME || '/tmp', '.omnis-glass');
  const ledger = process.env.TERMINUS_ACTION_LEDGER || join(state, 'terminus.actions.jsonl');
  mkdirSync(dirname(ledger), { recursive: true });
  const body = {
    receipt: 'terminus.action',
    canon_version: 3,
    ts: new Date().toISOString(),
    ...fields,
    prev: chainHead(ledger),
  };
  const canonical = JSON.stringify(body, Object.keys(body).sort());
  const sig = sha256(canonical);
  const rec = { ...body, sig };
  appendFileSync(ledger, JSON.stringify(rec) + '\n');
  return { sig, type: 'terminus.action' };
}

function payloadText(action) {
  if (action.payload == null) return '';
  if (typeof action.payload === 'string') return action.payload;
  return JSON.stringify(action.payload);
}

function authorizeBrowser(action) {
  const lane = action.lane || 'browse:research';
  const browserAction = action.action || 'navigate';
  const host = action.target?.host || '';
  const isKeyEvent = browserAction === 'type' && action.key_event === true;
  const keyReceiptFields = isKeyEvent
    ? { payload_digest: null, chars: Array.from(action.payload?.field_value ?? '').length }
    : {};

  // Lane structural checks (mirrors browser-lanes.yaml semantics)
  if (lane === 'browse:scrape') {
    const writeActions = new Set([
      'type', 'fill', 'submit', 'click-submit', 'upload', 'storage-write',
    ]);
    if (writeActions.has(browserAction)) {
      const rec = writeReceipt({
        kind: 'browser',
        action: browserAction,
        lane,
        host,
        verdict: 'REFUSE',
        decision: DECISION.REFUSE_LANE,
        reason: 'lane_read_only',
        refused: true,
        ...keyReceiptFields,
      });
      return {
        verdict: 'REFUSE',
        decision: DECISION.REFUSE_LANE,
        reason: 'lane_read_only',
        receipt: rec,
      };
    }
  }

  const text = payloadText(action);
  // Upload payloads nest scan text under payload key
  const scanText =
    action.payload && typeof action.payload === 'object' && action.payload.payload
      ? String(action.payload.payload)
      : text;
  const scan = scanSecrets(scanText);
  if (scan.refused) {
    const decision =
      scan.rule === 'crown_jewel' ? DECISION.REFUSE_CROWN : DECISION.REFUSE_SECRET;
    const rec = writeReceipt({
      kind: 'browser',
      action: browserAction,
      lane,
      host,
      payload_digest: isKeyEvent ? null : (text ? sha256(text) : null),
      ...keyReceiptFields,
      verdict: 'REFUSE',
      decision,
      reason: scan.rule,
      refused: true,
    });
    return { verdict: 'REFUSE', decision, reason: scan.rule, receipt: rec };
  }

  const rec = writeReceipt({
    kind: 'browser',
    action: browserAction,
    lane,
    host,
    payload_digest: isKeyEvent ? null : (text ? sha256(text) : null),
    ...keyReceiptFields,
    verdict: 'ALLOW',
    decision: DECISION.PERMIT,
    reason: 'permit',
    refused: false,
  });
  return { verdict: 'ALLOW', decision: DECISION.PERMIT, reason: 'permit', receipt: rec };
}

function authorizeAction(action) {
  if (action.kind !== 'browser') {
    return {
      verdict: 'REFUSE',
      decision: DECISION.REFUSE_SCANNER_FAILURE,
      reason: 'unsupported_kind',
      receipt: null,
    };
  }
  return authorizeBrowser(action);
}

const raw = readFileSync(0, 'utf8');
let action;
try {
  action = JSON.parse(raw);
} catch {
  process.stderr.write('terminus-authorize: invalid JSON on stdin\n');
  process.exit(2);
}

const out = authorizeAction(action);
process.stdout.write(JSON.stringify(out) + '\n');
if (out.verdict === 'ALLOW') process.exit(0);
if (out.verdict === 'HOLD') process.exit(2);
process.exit(1);
