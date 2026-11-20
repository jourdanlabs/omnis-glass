const canvas = document.getElementById('viewport');
const ctx = canvas.getContext('2d');
const urlBar = document.getElementById('url-bar');
const badge = document.getElementById('mode-badge');
const railList = document.getElementById('rail-list');
const btnTake = document.getElementById('btn-take');
const btnRelease = document.getElementById('btn-release');
const controls = [...document.querySelectorAll('.human-control')];
let human = false;
let mirrorOnly = false;
let deviceWidth = 1, deviceHeight = 1;

function setMode(mode) {
  badge.textContent = mode;
  badge.className = 'badge ' + mode.toLowerCase();
  human = mode === 'HUMAN' || mode === 'LOCKED';
  urlBar.readOnly = !human || mirrorOnly;
  btnTake.hidden = human || mirrorOnly;
  btnRelease.hidden = !human || mirrorOnly;
  for (const button of controls) button.disabled = !human || mirrorOnly;
}
window.__glassPane = {
  setMode,
  setMirrorOnly(value) { mirrorOnly = value; for (const button of controls) button.hidden = value; setMode(badge.textContent); },
  setUrl(value) { urlBar.value = value || ''; },
  setScriptsStopped(value) { document.getElementById('scripts-stopped').hidden = !value; },
  setStatus(value) { document.getElementById('status').textContent = value || ''; },
  setCapture(value) { document.getElementById('capture').textContent = value ? 'Last capture: ' + value : ''; },
  drawFrame(frame) {
    deviceWidth = frame.metadata.deviceWidth;
    deviceHeight = frame.metadata.deviceHeight;
    const img = new Image();
    img.onload = () => { canvas.width = img.width; canvas.height = img.height; ctx.drawImage(img, 0, 0); };
    img.src = 'data:image/jpeg;base64,' + frame.data;
  },
  appendRail(entry) {
    const li = document.createElement('li');
    li.textContent = `${entry.verdict} ${entry.action} ${entry.url}`.slice(0, 120);
    railList.prepend(li);
  },
};
function paneIpc(command) {
  const token = window.__GLASS_PANE_AUTH_TOKEN;
  if (!token || !window.ipc) return;
  window.ipc.postMessage('auth:' + token + '|' + JSON.stringify(command));
}
btnTake.onclick = () => paneIpc({type: 'take'});
btnRelease.onclick = () => paneIpc({type: 'release'});
for (const name of ['back', 'forward', 'reload']) document.getElementById('btn-' + name).onclick = () => paneIpc({type: name});
urlBar.addEventListener('keydown', (e) => {
  if (e.key === 'Enter' && !urlBar.readOnly) paneIpc({type: 'navigate', url: urlBar.value});
});
document.addEventListener('keydown', (e) => { if (e.key === 'Escape') paneIpc({type: 'release'}); });
function position(e) {
  const rect = canvas.getBoundingClientRect();
  const scale = Math.min(rect.width / deviceWidth, rect.height / deviceHeight);
  return {x: (e.clientX - rect.left - (rect.width - deviceWidth * scale) / 2) / scale,
          y: (e.clientY - rect.top - (rect.height - deviceHeight * scale) / 2) / scale};
}
let pendingMove = null, moveTimer = null, lastMoveAt = -Infinity;
function flushMove() {
  if (moveTimer !== null) clearTimeout(moveTimer);
  moveTimer = null;
  if (!pendingMove || !human || mirrorOnly) { pendingMove = null; return; }
  const latest = pendingMove; pendingMove = null; lastMoveAt = Date.now();
  paneIpc({type:'mouse', params:latest});
}
function queueMove(params) {
  pendingMove = params;
  if (moveTimer !== null) return;
  const delay = Math.max(0, 30 - (Date.now() - lastMoveAt));
  moveTimer = setTimeout(flushMove, delay);
}
for (const [dom, cdp] of [['mousedown','mousePressed'], ['mouseup','mouseReleased'], ['mousemove','mouseMoved'], ['wheel','mouseWheel']]) {
  canvas.addEventListener(dom, (e) => {
    if (!human || mirrorOnly) { if (dom !== 'mousemove') window.__glassPane.setStatus('take control first'); return; }
    const button = ['left', 'middle', 'right'][e.button] || 'none';
    const params = {type: cdp, ...position(e), button, buttons: e.buttons, modifiers: (e.altKey ? 1 : 0) | (e.ctrlKey ? 2 : 0) | (e.metaKey ? 4 : 0) | (e.shiftKey ? 8 : 0)};
    if (dom === 'wheel') { params.deltaX = e.deltaX; params.deltaY = e.deltaY; }
    if (dom === 'mousedown' || dom === 'mouseup') params.clickCount = e.detail || 1;
    if (dom === 'mousemove') queueMove(params);
    else { if (dom === 'mousedown' || dom === 'mouseup') flushMove(); paneIpc({type:'mouse', params}); }
    e.preventDefault(); canvas.focus();
  });
}
for (const [dom, cdp] of [['keydown','keyDown'], ['keyup','keyUp']]) canvas.addEventListener(dom, (e) => {
  if (e.key === 'Escape') return;
  if (!human || mirrorOnly) { window.__glassPane.setStatus('take control first'); return; }
  const params = {type: cdp, key: e.key, code: e.code, windowsVirtualKeyCode: e.keyCode, modifiers: (e.altKey ? 1 : 0) | (e.ctrlKey ? 2 : 0) | (e.metaKey ? 4 : 0) | (e.shiftKey ? 8 : 0)};
  if (dom === 'keydown' && e.key.length === 1 && !e.ctrlKey && !e.metaKey) params.text = e.key;
  paneIpc({type:'key', params}); e.preventDefault();
});
setMode('MIRROR');

paneIpc({type: "ready"});
