// 本地透明代理：转发自定义模型请求，拦截 usage 做本地统计
// 零依赖，仅用 Node 内置模块。监听 127.0.0.1:8787
const http = require('http');
const https = require('https');
const fs = require('fs');
const path = require('path');
const { HttpsProxyAgent } = require('https-proxy-agent');

const PORT = 8787;
// 转发外部 endpoint 时复用系统代理（如 Clash 7897）；127.0.0.1 由 NO_PROXY 排除
const UPSTREAM_PROXY = process.env.HTTPS_PROXY || process.env.https_proxy || process.env.HTTP_PROXY || null;
const upstreamAgent = UPSTREAM_PROXY ? new HttpsProxyAgent(UPSTREAM_PROXY) : undefined;
const STATS_FILE = path.join(__dirname, 'usage.json');

// 按路径前缀映射到真实 endpoint（与 models.json 原 url 对应）
const TARGETS = {
  '/v1/chat/completions': 'https://api.b.ai/v1/chat/completions',
  '/compatible-mode/v1/chat/completions':
    'https://llm-061p0s7xc7be60sv.cn-beijing.maas.aliyuncs.com/compatible-mode/v1/chat/completions',
};

function loadStats() {
  try {
    return JSON.parse(fs.readFileSync(STATS_FILE, 'utf8'));
  } catch {
    return { total: { prompt_tokens: 0, completion_tokens: 0, total_tokens: 0, calls: 0 }, records: [] };
  }
}

function saveStats(s) {
  fs.writeFileSync(STATS_FILE, JSON.stringify(s, null, 2));
}

function recordUsage(model, usage) {
  const s = loadStats();
  s.total.prompt_tokens += usage.prompt_tokens || 0;
  s.total.completion_tokens += usage.completion_tokens || 0;
  s.total.total_tokens += usage.total_tokens || 0;
  s.total.calls += 1;
  s.records.push({
    ts: new Date().toISOString(),
    model,
    prompt_tokens: usage.prompt_tokens || 0,
    completion_tokens: usage.completion_tokens || 0,
    total_tokens: usage.total_tokens || 0,
    cached_tokens: usage.prompt_tokens_details ? usage.prompt_tokens_details.cached_tokens : 0,
    reasoning_tokens: usage.completion_tokens_details ? usage.completion_tokens_details.reasoning_tokens : 0,
  });
  if (s.records.length > 500) s.records = s.records.slice(-500);
  saveStats(s);
  console.log(
    `[usage] model=${model} prompt=${usage.prompt_tokens} completion=${usage.completion_tokens} ` +
      `total=${usage.total_tokens} | 累计 total_tokens=${s.total.total_tokens} calls=${s.total.calls}`
  );
}

// 从 SSE 文本里取最后一个带 usage 的 data 行
function extractUsageFromSSE(text) {
  let lastUsage = null;
  for (const line of text.split('\n')) {
    const m = line.trim();
    if (!m.startsWith('data:')) continue;
    const payload = m.slice(5).trim();
    if (payload === '[DONE]') continue;
    try {
      const obj = JSON.parse(payload);
      if (obj.usage) lastUsage = obj.usage;
    } catch {}
  }
  return lastUsage;
}

function forward(req, res, targetUrl) {
  let body = '';
  req.on('data', (c) => (body += c));
  req.on('end', () => {
    let parsed = {};
    try {
      parsed = JSON.parse(body);
    } catch {}
    const model = parsed.model || 'unknown';

    // 关键补丁：stream 模式默认不回 usage，强制注入 stream_options.include_usage
    if (parsed.stream && (!parsed.stream_options || !parsed.stream_options.include_usage)) {
      parsed.stream_options = { include_usage: true };
      body = JSON.stringify(parsed);
    }

    const u = new URL(targetUrl);
    const options = {
      method: 'POST',
      hostname: u.hostname,
      port: u.port || 443,
      path: u.pathname + u.search,
      headers: {
        'Content-Type': 'application/json',
        Authorization: req.headers['authorization'],
        'Content-Length': Buffer.byteLength(body),
      },
      agent: upstreamAgent,
    };

    const upstream = https.request(options, (upRes) => {
      res.writeHead(upRes.statusCode, upRes.headers);
      if (parsed.stream) {
        let acc = '';
        upRes.on('data', (chunk) => {
          res.write(chunk);
          acc += chunk.toString('utf8');
        });
        upRes.on('end', () => {
          res.end();
          const usage = extractUsageFromSSE(acc);
          if (usage) recordUsage(model, usage);
          else console.log('[warn] stream 响应未含 usage（已注入 include_usage 仍无，检查上游）');
        });
      } else {
        let acc = '';
        upRes.on('data', (chunk) => (acc += chunk.toString('utf8')));
        upRes.on('end', () => {
          res.end(acc);
          try {
            const obj = JSON.parse(acc);
            if (obj.usage) recordUsage(model, obj.usage);
          } catch {}
        });
      }
    });
    upstream.on('error', (e) => {
      res.writeHead(502);
      res.end('proxy upstream error: ' + e.message);
    });
    upstream.write(body);
    upstream.end();
  });
}

const server = http.createServer((req, res) => {
  if (req.method === 'GET' && req.url === '/stats') {
    res.writeHead(200, { 'Content-Type': 'application/json' });
    res.end(JSON.stringify(loadStats(), null, 2));
    return;
  }
  if (req.method === 'POST' && TARGETS[req.url]) {
    forward(req, res, TARGETS[req.url]);
    return;
  }
  res.writeHead(404);
  res.end('not found');
});

server.listen(PORT, '127.0.0.1', () => {
  console.log(`token-proxy listening on http://127.0.0.1:${PORT}`);
  console.log('targets:', Object.keys(TARGETS).join(', '));
});
