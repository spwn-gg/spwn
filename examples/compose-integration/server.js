// Tiny zero-dependency HTTP server: a health endpoint + a greeting the agent can
// edit to see hot-reload across a session's live URL.
import { createServer } from 'node:http';

const greeting = 'hello from spwn session';

createServer((req, res) => {
  if (req.url === '/health') {
    res.writeHead(200, { 'content-type': 'text/plain' });
    res.end('ok');
    return;
  }
  res.writeHead(200, { 'content-type': 'text/plain' });
  res.end(`${greeting}\n`);
}).listen(3000, () => console.log('listening on :3000'));

export { greeting };
