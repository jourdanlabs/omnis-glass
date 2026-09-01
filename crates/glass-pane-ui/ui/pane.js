const canvas = document.getElementById('viewport');
const ctx = canvas.getContext('2d');
const urlBar = document.getElementById('url-bar');
const badge = document.getElementById('mode-badge');
const railList = document.getElementById('rail-list');
const btnTake = document.getElementById('btn-take');
const btnRelease = document.getElementById('btn-release');

function setMode(mode) {
  badge.textContent = mode;
  badge.className = 'badge ' + mode.toLowerCase();
  const human = mode === 'HUMAN';
  urlBar.readOnly = !human;
  btnTake.hidden = human;
  btnRelease.hidden = !human;
}

window.__glassPane = {
  setMode,
  setUrl(u) { urlBar.value = u || ''; },
  drawFrame(base64Jpeg) {
    const img = new Image();
    img.onload = () => {
      canvas.width = img.width;
      canvas.height = img.height;
      ctx.drawImage(img, 0, 0);
    };
    img.src = 'data:image/jpeg;base64,' + base64Jpeg;
  },
  appendRail(entry) {
    const li = document.createElement('li');
    li.textContent = `${entry.verdict} ${entry.action} ${entry.url}`.slice(0, 120);
    railList.prepend(li);
  },
};

btnTake.onclick = () => window.ipc.postMessage('take_control');
btnRelease.onclick = () => window.ipc.postMessage('release');
urlBar.addEventListener('keydown', (e) => {
  if (e.key === 'Enter' && !urlBar.readOnly) {
    window.ipc.postMessage('navigate:' + urlBar.value);
  }
});
document.addEventListener('keydown', (e) => {
  if (e.key === 'Escape') window.ipc.postMessage('release');
});

setMode('MIRROR');
