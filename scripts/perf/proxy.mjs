// A timing proxy in front of the dev or preview server: node proxy.mjs <listen> <target> <logfile>
// Logs when the web view asks for each file, and injects marks (head, DOMContentLoaded, load, first paint,
// ribbon visible) into index.html. Harness only; the app is untouched.
import http from 'http'; import net from 'net'; import fs from 'fs';
const [listen, target, logfile] = process.argv.slice(2).map((x, i) => (i < 2 ? +x : x));
const log = (m) => fs.appendFileSync(logfile, `${Date.now()} ${m}\n`);
const markJs = `(function(){var m=function(n){try{fetch('/__mark/'+n,{keepalive:true})}catch(e){}};m('head');addEventListener('DOMContentLoaded',function(){m('dcl')});addEventListener('load',function(){m('load')});try{new PerformanceObserver(function(l){l.getEntries().forEach(function(e){m(e.name+'-'+Math.round(e.startTime))})}).observe({type:'paint',buffered:true})}catch(e){}var p=function(){if(document.querySelector('[role=tab]')){requestAnimationFrame(function(){requestAnimationFrame(function(){m('ready-'+Math.round(performance.now()))})})}else{requestAnimationFrame(p)}};p()})()`;
const inject = '<script src="/__mark.js"></script>';
const server = http.createServer((req, res) => {
  if (req.url === '/__mark.js') { log('MARKJS'); res.writeHead(200, { 'content-type': 'text/javascript' }); return res.end(markJs); }
  if (req.url.startsWith('/__mark/')) { log('MARK ' + req.url.slice(8)); res.writeHead(204); return res.end(); }
  log('GET ' + req.url.split('?')[0]);
  const up = http.request({ host: '::1', port: target, path: req.url, method: req.method, headers: (() => { const h = { ...req.headers, 'accept-encoding': 'identity' }; if (req.url === '/' || req.url.startsWith('/?')) { delete h['if-none-match']; delete h['if-modified-since']; } return h; })() }, (r) => {
    if ((r.headers['content-type'] || '').includes('text/html')) {
      let body = ''; r.on('data', (d) => (body += d)); r.on('end', () => { body = body.replace('<head>', '<head>' + inject); const h = { ...r.headers }; delete h['content-length']; res.writeHead(r.statusCode, h); res.end(body); });
    } else { res.writeHead(r.statusCode, r.headers); r.pipe(res); }
  });
  up.on('error', () => { res.writeHead(502); res.end(); }); req.pipe(up);
});
server.on('upgrade', (req, sock, head) => { const up = net.connect(target, '::1', () => { up.write(`${req.method} ${req.url} HTTP/1.1\r\n` + Object.entries(req.headers).map(([k, v]) => `${k}: ${v}`).join('\r\n') + '\r\n\r\n'); up.write(head); sock.pipe(up); up.pipe(sock); }); up.on('error', () => sock.destroy()); });
server.listen(listen, '127.0.0.1', () => log('PROXY up'));
