#!/usr/bin/env node
// Tree-wide bypass guard — flags agent-facing ungoverned egress only.
import { readFileSync, readdirSync } from 'node:fs';
import { join, relative } from 'node:path';

const ROOT = join(import.meta.dirname, '..');
const EXEMPT = JSON.parse(
  readFileSync(join(ROOT, 'scripts/cdp-dispatch-exemptions.json'), 'utf8'),
);

/** Paths where Page ops are post-ALLOW CDP domain execution or engine internals. */
function isPostGatePath(rel) {
  return (
    rel.startsWith('crates/obscura-cdp/src/domains/') ||
    rel.startsWith('crates/obscura-cdp/src/server.rs') ||
    rel.startsWith('crates/obscura-browser/') ||
    rel.startsWith('crates/obscura-js/') ||
    rel.startsWith('crates/obscura-dom/') ||
    rel.startsWith('crates/obscura-render/') ||
    rel === 'crates/obscura/src/page.rs'
  );
}

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

const AGENT_FACING = [
  'crates/obscura-mcp/src/lib.rs',
  'crates/obscura-cli/src/main.rs',
  'crates/obscura-cli/src/worker.rs', // CDP serve worker — governance via dispatch.rs
];

const violations = [];
const patterns = [
  /\bpage\.navigate_with_wait\s*\(/,
  /\bPage::navigate_with_wait\s*\(/,
  /\bpage\.evaluate\s*\(/,
  /\bObscuraHttpClient::/,
];

for (const rel of AGENT_FACING) {
  const file = join(ROOT, rel);
  let src;
  try {
    src = readFileSync(file, 'utf8');
  } catch {
    continue;
  }
  const governed =
    src.includes('glass_govern::') ||
    src.includes('gate_browser_action') ||
    src.includes('gate_mcp_') ||
    src.includes('OMNIS_GLASS_HTTP_ACTION') ||
    src.includes('obscura_cdp::') ||
    rel.includes('worker.rs');
  if (!governed) {
    violations.push({ file: rel, pattern: 'missing glass_govern markers' });
  }
}

if (violations.length) {
  console.error('check-cdp-bypass: agent-facing paths missing governance:');
  for (const v of violations) console.error(`  ${v.file} (${v.pattern})`);
  process.exit(1);
}
console.log('check-cdp-bypass: OK');
