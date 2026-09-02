#!/usr/bin/env node
// Agent-surface bypass guard — governed egress paths (MCP, JS fetch, CLI, HTTP clients, nav sink).
import { readFileSync, readdirSync } from 'node:fs';
import { join, relative } from 'node:path';

const ROOT = join(import.meta.dirname, '..');

function loadExemptions() {
  const agent = JSON.parse(
    readFileSync(join(ROOT, 'scripts/agent-surface-exemptions.json'), 'utf8'),
  );
  const cdp = JSON.parse(
    readFileSync(join(ROOT, 'scripts/cdp-dispatch-exemptions.json'), 'utf8'),
  );
  return {
    files: [...(agent.files || []), ...(cdp.files || [])],
    entries: [...(agent.entries || []), ...(cdp.entries || [])],
  };
}

const EXEMPT = loadExemptions();

const SCAN_ROOTS = [
  'crates/obscura-mcp/src',
  'crates/obscura-js/src/ops.rs',
  'crates/obscura-cli/src/main.rs',
  'crates/obscura-net/src/client.rs',
  'crates/obscura-net/src/wreq_client.rs',
];

const CALL_PATTERNS = [
  { name: 'navigate_with_wait', re: /\b(?:page|state\.page_mut\(\))\.navigate_with_wait\s*\(/ },
  { name: 'page.evaluate', re: /\b(?:page|state\.page_mut\(\))\.evaluate\s*\(/ },
  { name: 'StealthHttpClient::fetch', re: /StealthHttpClient::[\w:]*fetch\s*\(/ },
  { name: 'cached_request_client', re: /\bcached_request_client\s*\(/ },
];

const REDIRECT_SCAN_FILES = [
  'crates/obscura-net/src/client.rs',
  'crates/obscura-net/src/wreq_client.rs',
];

const SINK_CHECKS = [
  {
    file: 'crates/obscura-browser/src/page.rs',
    fn: 'process_pending_navigation',
    mustInclude: 'gate_browser_action',
  },
];

const GATE_MARKERS = [
  'gate_mcp_',
  'gated_navigate',
  'gated_evaluate',
  'gate_js_fetch',
  'gate_http_outbound',
  'gate_browser_action',
  'gate_redirect_destination',
  'glass_govern::',
];

function walk(dir, out = []) {
  for (const ent of readdirSync(dir, { withFileTypes: true })) {
    const p = join(dir, ent.name);
    if (ent.isDirectory()) {
      if (ent.name === 'target' || ent.name === 'vendor') continue;
      walk(p, out);
    } else if (ent.name.endsWith('.rs')) {
      out.push(p);
    }
  }
  return out;
}

function isExempt(rel, lineNo, patternName) {
  if (rel === 'crates/obscura-mcp/src/page_gate.rs') return true;
  for (const entry of EXEMPT.entries || []) {
    if (entry.file === rel && (entry.pattern === patternName || entry.symbol === patternName)) {
      if (entry.lines && !entry.lines.includes(lineNo)) continue;
      return true;
    }
  }
  for (const f of EXEMPT.files || []) {
    if (f === rel) return true;
  }
  return false;
}

function stripTestModules(src) {
  const marker = '#[cfg(test)]';
  const idx = src.indexOf(marker);
  return idx >= 0 ? src.slice(0, idx) : src;
}

function governedNear(lines, lineIdx, window = 80) {
  const start = Math.max(0, lineIdx - window);
  const slice = lines.slice(start, lineIdx + 1).join('\n');
  return GATE_MARKERS.some((m) => slice.includes(m));
}

function collectFiles() {
  const files = new Set();
  for (const root of SCAN_ROOTS) {
    const abs = join(ROOT, root);
    if (root.endsWith('.rs')) {
      files.add(abs);
      continue;
    }
    try {
      for (const f of walk(abs)) files.add(f);
    } catch {
      // optional path
    }
  }
  return [...files];
}

const violations = [];

for (const file of collectFiles()) {
  const rel = relative(ROOT, file);
  let src = readFileSync(file, 'utf8');
  if (rel === 'crates/obscura-mcp/src/lib.rs') {
    src = stripTestModules(src);
  }
  const lines = src.split('\n');
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    const lineNo = i + 1;
    for (const { name, re } of CALL_PATTERNS) {
      if (!re.test(line)) continue;
      if (isExempt(rel, lineNo, name)) continue;
      if (governedNear(lines, i)) continue;
      violations.push({ file: rel, line: lineNo, pattern: name, text: line.trim() });
    }
    if (REDIRECT_SCAN_FILES.includes(rel) && /\bcurrent_url = next_url\b/.test(line)) {
      if (!governedNear(lines, i, 40)) {
        violations.push({
          file: rel,
          line: lineNo,
          pattern: 'redirect_hop_assign',
          text: line.trim(),
        });
      }
    }
  }
}

for (const sink of SINK_CHECKS) {
  const abs = join(ROOT, sink.file);
  const src = readFileSync(abs, 'utf8');
  const fnStart = src.indexOf(`fn ${sink.fn}`);
  if (fnStart < 0) {
    violations.push({
      file: sink.file,
      line: 0,
      pattern: sink.fn,
      text: `missing function ${sink.fn}`,
    });
    continue;
  }
  const fnBody = src.slice(fnStart, fnStart + 4000);
  if (!fnBody.includes(sink.mustInclude)) {
    violations.push({
      file: sink.file,
      line: 0,
      pattern: sink.fn,
      text: `sink ${sink.fn} missing ${sink.mustInclude}`,
    });
  }
}

if (violations.length) {
  console.error('check-cdp-bypass: ungoverned agent-surface calls:');
  for (const v of violations) {
    console.error(`  ${v.file}:${v.line} [${v.pattern}] ${v.text}`);
  }
  process.exit(1);
}
console.log('check-cdp-bypass: OK');
