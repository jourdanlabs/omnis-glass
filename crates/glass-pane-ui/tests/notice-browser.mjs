// Run with CHROME_BIN pointing to a Chromium executable.
import { mkdtempSync, readFileSync, writeFileSync, copyFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, dirname } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { spawnSync } from 'node:child_process';
const source = join(dirname(fileURLToPath(import.meta.url)), '..', 'ui');
const dir = mkdtempSync(join(tmpdir(), 'glass-notice-'));
try {
  for (const name of ['pane.css', 'pane.js']) copyFileSync(join(source, name), join(dir, name));
  const html = readFileSync(join(source, 'index.html'), 'utf8').replace('</body>', '<script src="test.js"></script></body>');
  writeFileSync(join(dir, 'index.html'), html);
  writeFileSync(join(dir, 'test.js'), `
    const notice = document.getElementById('scripts-stopped');
    const expected = "This page's scripts were stopped when the agent used another page. Typing reaches its fields, but the page's own code will not react. Reload restarts them.";
    const check = (ok, message) => { if (!ok) throw Error(message); };
    try {
      check(notice.hidden, 'initially hidden');
      window.__glassPane.setScriptsStopped(true);
      check(!notice.hidden && notice.getBoundingClientRect().height > 0 && getComputedStyle(notice).visibility === 'visible', 'visible');
      check(notice.textContent === expected, 'text');
      check(parseFloat(getComputedStyle(notice).fontSize) >= parseFloat(getComputedStyle(document.getElementById('status')).fontSize), 'font size');
      check(getComputedStyle(notice).color !== getComputedStyle(document.getElementById('status')).color, 'colour');
      for (const mode of ['MIRROR', 'HUMAN']) {
        window.__glassPane.setMode(mode);
        window.__glassPane.setStatus('refused');
        check(!notice.hidden && notice.textContent === expected, 'status or mode changed notice');
      }
      window.__glassPane.setScriptsStopped(false);
      check(notice.hidden, 'other page hides notice');
      window.__glassPane.setScriptsStopped(true);
      check(!notice.hidden && notice.textContent === expected, 'return restores notice');
      window.__glassPane.setScriptsStopped(false);
      check(notice.hidden, 'reload hides notice');
      document.body.dataset.noticeTest = 'PASS';
    } catch (e) { document.body.dataset.noticeTest = e.message; }
  `);
  const result = spawnSync(process.env.CHROME_BIN || 'chromium', ['--headless', '--disable-gpu', '--no-sandbox', '--disable-extensions', '--user-data-dir=' + join(dir, 'profile'), '--dump-dom', pathToFileURL(join(dir, 'index.html')).href], { encoding: 'utf8', timeout: 30000 });
  if (result.error) throw result.error;
  if (result.status !== 0 || !result.stdout.includes('data-notice-test="PASS"')) throw Error(`notice browser test failed: ${result.stdout}\n${result.stderr}`);
  console.log('notice browser test: PASS');
} finally { rmSync(dir, { recursive: true, force: true }); }
