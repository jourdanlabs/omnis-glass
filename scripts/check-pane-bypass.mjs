#!/usr/bin/env node
// Pane bypass guard — no direct Page/HTTP from pane crates.

import { readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';

const ROOT = join(import.meta.dirname, '..');
const TARGETS = [
  'crates/glass-pane/src',
  'crates/glass-pane-bridge/src',
  'crates/glass-pane-ui/src',
];

const FORBIDDEN = [
  /\bPage::navigate/,
  /\bPage::evaluate/,
  /\bObscuraHttpClient::/,
];

const violations = [];
for (const rel of TARGETS) {
  walk(join(ROOT, rel));
}

function walk(dir) {
  for (const ent of readdirSync(dir, { withFileTypes: true })) {
    const p = join(dir, ent.name);
    if (ent.isDirectory()) walk(p);
    else if (ent.name.endsWith('.rs')) {
      const src = readFileSync(p, 'utf8');
      if (!src.includes('gate_browser_action') && !src.includes('gate_pane_navigate') && p.includes('glass-pane-bridge')) {
        if (!src.includes('cdp_client') && !src.includes('pane_auth')) {
          // bridge must not call Page directly — checked via FORBIDDEN
        }
      }
      for (const re of FORBIDDEN) {
        if (re.test(src)) violations.push({ file: p.replace(ROOT + '/', ''), re: re.toString() });
      }
      if (p.includes('session.rs') && !src.includes('gate_pane_navigate')) {
        violations.push({ file: p.replace(ROOT + '/', ''), re: 'missing gate_pane_navigate' });
      }
    }
  }
}

if (violations.length) {
  console.error('check-pane-bypass: FAIL');
  for (const v of violations) console.error(`  ${v.file} (${v.re})`);
  process.exit(1);
}
console.log('check-pane-bypass: OK');
