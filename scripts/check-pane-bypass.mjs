#!/usr/bin/env node
// Page and input operations must pass through the observer's human command.
import { readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
const ROOT = join(import.meta.dirname, '..');
const targets = ['crates/glass-pane/src', 'crates/glass-pane-bridge/src', 'crates/glass-pane-ui/src'];
const violations = [];
function walk(dir) {
  for (const ent of readdirSync(dir, { withFileTypes: true })) {
    const file = join(dir, ent.name);
    if (ent.isDirectory()) { walk(file); continue; }
    if (!ent.name.endsWith('.rs')) continue;
    const src = readFileSync(file, 'utf8');
    const forbidden = [/\bPage::navigate\b/, /\bPage::evaluate\b/, /\bObscuraHttpClient\b/, /\b(?:reqwest|wreq)::/,
      /\b(?:call|send)\s*\(\s*"(?:Page|Input)\.[^"]+"/,
      /["']method["']\s*:\s*["'](?:Page|Input)\.[^"']+["']/];
    for (const pattern of forbidden) if (pattern.test(src)) violations.push(`${file}: ${pattern}`);
  }
}
for (const dir of targets) walk(join(ROOT, dir));
if (violations.length) { console.error('check-pane-bypass: FAIL'); for (const v of violations) console.error(v); process.exit(1); }
console.log('check-pane-bypass: OK');
