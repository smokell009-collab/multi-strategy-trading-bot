const tokenInput = document.querySelector('#token');
const connection = document.querySelector('#connection');
const tokenKey = 'ode-dashboard-token';
let token = sessionStorage.getItem(tokenKey) || '';
tokenInput.value = token;

async function api(path, options = {}) {
  const headers = new Headers(options.headers || {});
  headers.set('Authorization', `Bearer ${token}`);
  if (options.body !== undefined) headers.set('Content-Type', 'application/json');
  const response = await fetch(path, { ...options, headers });

  if (response.status === 401) {
    sessionStorage.removeItem(tokenKey);
    token = '';
    tokenInput.value = '';
    throw new Error('Token inválido ou sessão expirada.');
  }
  if (!response.ok) {
    let message = `HTTP ${response.status}`;
    try {
      const body = await response.json();
      if (body.error) message = body.error;
    } catch {}
    throw new Error(message);
  }
  return response.json();
}

function fmt(value) {
  return value === null || value === undefined ? '—' : String(value);
}

function setConnection(ok, message) {
  connection.textContent = message;
  connection.className = `badge ${ok ? 'ok' : 'bad'}`;
}

function date(value) {
  if (!value) return '—';
  const numeric = Number(value);
  const instant = Number.isFinite(numeric) ? new Date(numeric) : new Date(value);
  return instant.toLocaleString('pt-BR');
}

function renderStatus(status) {
  if (!status) {
    document.querySelector('#feed').textContent = 'Aguardando';
    return;
  }
  document.querySelector('#market').textContent = status.symbol || '—';
  document.querySelector('#interval').textContent = status.interval || '—';
  const feed = document.querySelector('#feed');
  feed.textContent = status.feed_state || '—';
  feed.className = `status ${status.feed_state || ''}`;
  document.querySelector('#updated').textContent = `Atualizado: ${date(status.updated_at)}`;
  document.querySelector('#price').textContent = fmt(status.last_price);
  document.querySelector('#candle-time').textContent = `Candle: ${date(status.last_candle_close_ms)}`;
  document.querySelector('#equity').textContent = fmt(status.equity);
  document.querySelector('#cash').textContent = `Caixa: ${fmt(status.cash)}`;
  document.querySelector('#position').textContent = status.has_position ? 'Aberta' : 'Sem posição';
  document.querySelector('#quantity').textContent = `Quantidade: ${fmt(status.quantity)}`;
  document.querySelector('#signal').textContent = fmt(status.last_signal);
  document.querySelector('#kill-switch').textContent = status.kill_switch_active
    ? 'KILL-SWITCH ATIVO'
    : 'Kill-switch inativo';
  document.querySelector('#latency').textContent =
    `${fmt(status.latency_p50_ms)} / ${fmt(status.latency_p95_ms)}`;
}

function renderEvents(items) {
  const body = document.querySelector('#events');
  body.replaceChildren();
  if (!items.length) {
    const row = document.createElement('tr');
    const cell = document.createElement('td');
    cell.colSpan = 4;
    cell.className = 'empty';
    cell.textContent = 'Nenhum evento registrado.';
    row.append(cell);
    body.append(row);
    return;
  }
  for (const item of items) {
    const row = document.createElement('tr');
    const values = [date(item.occurred_at), item.event_type, item.market_id, JSON.stringify(item.details)];
    values.forEach((value, index) => {
      const cell = document.createElement('td');
      cell.textContent = fmt(value);
      if (index === 3) cell.className = 'details';
      row.append(cell);
    });
    body.append(row);
  }
}

function renderAlerts(items) {
  const root = document.querySelector('#alerts');
  root.replaceChildren();
  if (!items.length) {
    root.textContent = 'Nenhum alerta ativo.';
    root.className = 'empty';
    return;
  }
  root.className = '';
  for (const item of items) {
    const row = document.createElement('article');
    row.className = `alert ${item.severity}`;

    const severity = document.createElement('div');
    severity.className = 'severity';
    severity.textContent = item.severity.toUpperCase();

    const content = document.createElement('div');
    const title = document.createElement('strong');
    title.textContent = item.title;
    const metadata = document.createElement('small');
    metadata.textContent = `${item.market_id} · ${item.occurrence_count} ocorrência(s) · último: ${date(item.last_seen_at)}`;
    content.append(title, metadata);

    const acknowledge = document.createElement('button');
    acknowledge.className = 'secondary';
    acknowledge.textContent = item.acknowledged_at ? 'Reconhecido' : 'Reconhecer';
    acknowledge.disabled = Boolean(item.acknowledged_at) || item.status !== 'active';
    acknowledge.addEventListener('click', async () => {
      try {
        await api(`/api/alerts/${item.id}/ack`, { method: 'POST', body: '{}' });
        await refresh();
      } catch (error) {
        setConnection(false, error.message);
      }
    });

    row.append(severity, content, acknowledge);
    root.append(row);
  }
}

async function refresh() {
  if (!token) {
    setConnection(false, 'Desconectado');
    return;
  }
  try {
    const [status, events, alerts, capabilities] = await Promise.all([
      api('/api/status'),
      api('/api/events?limit=50'),
      api('/api/alerts?limit=50&active_only=true'),
      api('/api/capabilities'),
    ]);
    renderStatus(status);
    renderEvents(events);
    renderAlerts(alerts);
    document.querySelector('#advisory-panel').classList.toggle(
      'hidden',
      !capabilities.ai_advisory_enabled,
    );
    setConnection(true, 'Autenticado · leitura');
  } catch (error) {
    setConnection(false, error.message);
  }
}

document.querySelector('#connect').addEventListener('click', async () => {
  token = tokenInput.value.trim();
  if (token) sessionStorage.setItem(tokenKey, token);
  else sessionStorage.removeItem(tokenKey);
  await refresh();
});

document.querySelector('#disconnect').addEventListener('click', () => {
  sessionStorage.removeItem(tokenKey);
  token = '';
  tokenInput.value = '';
  setConnection(false, 'Desconectado');
});

document.querySelector('#refresh').addEventListener('click', refresh);

document.querySelector('#analyze').addEventListener('click', async (event) => {
  const button = event.currentTarget;
  button.disabled = true;
  const output = document.querySelector('#advisory-result');
  output.textContent = 'Consultando o provedor configurado…';
  try {
    const result = await api('/api/advisory', { method: 'POST', body: '{}' });
    output.textContent = `${result.model}\n\n${result.text}\n\nComentário consultivo; não é sinal nem ordem.`;
  } catch (error) {
    output.textContent = `Não foi possível gerar comentário: ${error.message}`;
  } finally {
    button.disabled = false;
  }
});

if (token) refresh();
setInterval(refresh, 10_000);
