// Render PNG exports from the SVG sources. Run from the repository root:
//   node brand/tools/render-png.js && python brand/tools/favicon-ico.py
const { chromium } = require('playwright');
const fs = require('fs'); const path = require('path');
const B = path.resolve(__dirname, '..');
const jobs = [
  ...[1024, 512, 192, 180, 167, 152, 128, 120, 64, 48].map(s => ['app-icon/app-icon-1024.svg', `app-icon/png/app-icon-${s}.png`, s, s]),
  ...[512, 256, 128, 64, 32, 16].map(s => ['app-icon/app-icon-rounded.svg', `app-icon/png/app-icon-rounded-${s}.png`, s, s]),
  ...[48, 32, 16].map(s => ['app-icon/favicon.svg', `app-icon/png/favicon-${s}.png`, s, s]),
  ['logo/amanfold-logo.svg', 'logo/png/amanfold-logo.png', null, 192],
  ['logo/amanfold-logo-reversed.svg', 'logo/png/amanfold-logo-reversed.png', null, 192],
  ['logo/amanfold-logo-arabic.svg', 'logo/png/amanfold-logo-arabic.png', null, 192],
  ['logo/amanfold-logo-stacked.svg', 'logo/png/amanfold-logo-stacked.png', null, 600],
  ['logo/amanfold-mark.svg', 'logo/png/amanfold-mark-512.png', 512, 512],
];
(async () => {
  const b = await chromium.launch(); const p = await b.newPage();
  for (const [src, out, w0, h] of jobs) {
    const svg = fs.readFileSync(path.join(B, src), 'utf8');
    const m = svg.match(/viewBox="0 0 ([\d.]+) ([\d.]+)"/);
    const w = w0 || Math.round(h * (+m[1]) / (+m[2]));
    await p.setViewportSize({ width: w, height: h });
    await p.setContent(`<style>*{margin:0}svg{display:block;width:${w}px;height:${h}px}</style>${svg}`);
    fs.mkdirSync(path.dirname(path.join(B, out)), { recursive: true });
    await p.screenshot({ path: path.join(B, out), omitBackground: true });
  }
  await b.close(); console.log(`rendered ${jobs.length} PNGs`);
})();
