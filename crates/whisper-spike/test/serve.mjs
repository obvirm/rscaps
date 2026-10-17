// Static server for the whisper-spike standalone test (repo root).
// COOP/COEP required by onnxruntime-web threads. Usage:
//   node crates/whisper-spike/test/serve.mjs [port]
// Then open: http://127.0.0.1:8901/crates/whisper-spike/test/
import http from 'node:http';
import fs from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..', '..');
const port = Number(process.argv[2] || 8901);
const MIME = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8',
  '.wasm': 'application/wasm',
  '.json': 'application/json; charset=utf-8',
  '.onnx': 'application/octet-stream',
  '.ts': 'text/plain; charset=utf-8',
};

const server = http.createServer(async (req, res) => {
  try {
    const url = new URL(req.url, 'http://x');
    let p = path.normalize(path.join(root, decodeURIComponent(url.pathname)));
    if (!p.startsWith(root)) { res.writeHead(403); res.end('forbidden'); return; }
    try {
      const st = await fs.stat(p);
      if (st.isDirectory()) p = path.join(p, 'index.html');
    } catch { /* fall through to 404 */ }
    const data = await fs.readFile(p);
    res.writeHead(200, {
      'Content-Type': MIME[path.extname(p).toLowerCase()] || 'application/octet-stream',
      'Cross-Origin-Opener-Policy': 'same-origin',
      'Cross-Origin-Embedder-Policy': 'require-corp',
      'Cache-Control': 'no-store',
      'Access-Control-Allow-Origin': '*',
    });
    res.end(data);
  } catch {
    res.writeHead(404); res.end('not found');
  }
});
server.listen(port, '127.0.0.1', () => {
  console.log('whisper test at http://127.0.0.1:' + port + '/crates/whisper-spike/test/');
});
