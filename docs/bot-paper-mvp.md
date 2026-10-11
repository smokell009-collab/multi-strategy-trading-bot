# Olho de Deus — bot paper, governança de risco e avaliação histórica

## O que funciona

- Consulta candles públicos Spot da Binance (`data-api.binance.vision`, `GET /api/v3/klines`), sem credenciais.
- Ingestão OHLCV, verificação de timestamps/cadência e OHLC consistente; dados atrasados ou inconsistentes bloqueiam decisões.
- Estatísticas de latência p50/p95/máxima quando os timestamps originais de recebimento estão disponíveis.
- EMA crossover isolada em `ode-strategy-technical`, com parâmetros e interruptor `enabled` em TOML.
- Paper broker long-only com sizing autorizado por `ode-risk`, custos simulados, stop e auditoria JSONL. Nenhuma ordem real é enviada.
- Backtest reproduzível a partir de CSV com fills na abertura seguinte, stop intrabar baseado em OHLC, taxas/slippage e o mesmo motor de risco.
- Relatório de holdout cronológico com métricas in-sample e fora da amostra: retorno, número de operações, entradas rejeitadas por risco, taxa de acerto, expectativa, fator de lucro, drawdown e diferença para buy-and-hold.
- Auditoria JSONL e SQLite persistente (`data/olho.sqlite3`), com APIs locais autenticadas para status, eventos e alertas.
- Dashboard local de paper trading com estado do feed, latência, posição paper e alertas reconhecíveis.
- Adaptador de comentário de IA OpenAI-compatible, opcional e manual; somente contexto público de mercado é enviado e a resposta não é executável pelo bot.

## Estratégia e switch próprio

A configuração de exemplo é [`configs/strategies.example.toml`](../configs/strategies.example.toml):

```toml
[ema_crossover]
enabled = true
fast_period = 9
slow_period = 21
```

Defina `enabled = false` para desativar a EMA sem mexer nos gates de risco ou no feed. O código da estratégia só recebe candles fechados e não controla risco nem execução. A arquitetura permite módulos independentes; **EMA é a única estratégia implementada nesta versão**.

## Backtest OOS a partir de CSV

Formato obrigatório de cabeçalho e exemplo de linhas:

```csv
open_time_ms,close_time_ms,open,high,low,close,volume,received_at_ms
0,59999,100,101,99,100.5,20,60010
60000,119999,100.5,102,100,101.7,25,120020
```

`received_at_ms` é opcional; só o informe se o instante original de recebimento for conhecido. Se faltar, as métricas de latência ficam indisponíveis (não são estimadas). O loader rejeita histórico vazio, preço não positivo, OHLC incompatível, volume negativo, ordem duplicada/desordenada e intervalos irregulares/lacunas. Use candles completos do mesmo mercado e intervalo; não junte fontes ou períodos com calendários diferentes.

```bash
cargo run --release -- \
  --backtest-csv data/BTCUSDT-1m.csv \
  --symbol BTCUSDT --interval 1m \
  --in-sample-ratio 0.7 \
  --strategy-config configs/strategies.example.toml \
  --risk-overrides configs/risk-overrides.example.toml \
  --starting-balance 10000 \
  --backtest-report data/backtest-report.json
```

O programa **não consulta a Binance** nesse modo. O relatório também é impresso no `stdout`; `--backtest-report` grava uma cópia JSON. O split é cronológico: o prefixo é in-sample e o sufixo é OOS. Cada período começa flat; no OOS, candles anteriores ao corte servem somente de warm-up para a estratégia. A decisão usa candles fechados e o fill usa apenas a abertura seguinte. Stops são avaliados contra low/high do candle corrente e assumem execução no stop, ou na abertura se houver gap contra a posição. Taxas e slippage são aplicados nos fills e sizing passa pelo motor `ode-risk` e pelos overrides configurados.

O histórico precisa ter candles suficientes em **ambos** os trechos para warm-up e trades. Com EMA padrão 9/21 e `--in-sample-ratio 0.7`, são necessários aproximadamente 80 ou mais candles regulares; o código rejeita segmentos menores em vez de produzir métricas vazias como se fossem uma validação válida.

**Limite de interpretação:** é uma única divisão temporal, com parâmetros fixos. Não há otimização/treino automático, walk-forward rolling, validação em vários regimes, testes de significância ou correção de múltiplas tentativas. A comparação com buy-and-hold é descritiva do período, não prova contribuição incremental persistente. Não escolha parâmetros olhando o OOS e depois reporte esse mesmo trecho como teste independente.

O teste end-to-end local usa dados sintéticos somente para verificar leitura, cálculo e serialização. Nenhum resultado sintético é evidência de performance. O CSV, o período escolhido, os custos e a origem dos dados devem ser guardados para reproduzir qualquer análise real.

## Execução paper e kill-switch

Consulta única do feed público:

```bash
cargo run --release -- --symbol BTCUSDT --interval 1m --once
```

Loop contínuo:

```bash
cargo run --release -- --symbol BTCUSDT --interval 1m \
  --poll-seconds 15 --starting-balance 10000 \
  --risk-per-trade 0.01 --max-daily-loss 0.02 \
  --max-allocation 0.25 --stop-loss 0.02
```

Ative o latch persistente de emergência com:

```bash
cargo run --release -- --activate-kill-switch "pausa operacional"
```

O kill-switch é salvo atomicamente em `data/KILL_SWITCH.json`, bloqueia entradas após reinício e não fecha automaticamente uma posição aberta. Arquivo inválido/ilegível falha fechado. Não há comando de desativação: somente operador autorizado deve remover o arquivo após revisar a ocorrência e decidir o rearme.

Os percentuais de risco do CLI são frações (`0.01` = 1%). Overrides TOML usam basis points (`100` = 1%); overrides por estratégia/ativo só podem apertar limites-base. A auditoria fica em `data/audit.jsonl`, em pasta ignorada pelo Git.

## Dashboard local, alertas e auditoria SQLite

O dashboard só é iniciado com `--dashboard`, em loop paper contínuo (não use `--once` nem backtest). Gere um token fora do repositório; o mínimo é 32 caracteres ASCII sem espaços:

```bash
export ODE_DASHBOARD_TOKEN="$(openssl rand -hex 32)"
cargo run --release -- --dashboard --symbol BTCUSDT --interval 1m \
  --audit-db data/olho.sqlite3
```

Abra `http://127.0.0.1:8787` e informe o token. O bind é validado como loopback no CLI e no crate do servidor. A página não carrega scripts/CDNs externos; APIs exigem `Authorization: Bearer`, não há CORS habilitado e respostas usam `no-store`, CSP e `X-Content-Type-Options`. O token nunca é colocado na URL ou no log e fica em `sessionStorage` somente para a aba atual. Não publique o servidor em `0.0.0.0` nem encaminhe a porta.

O painel consulta status, eventos e alertas a cada 10 segundos. Alertas de feed são deduplicados, contam ocorrências e resolvem quando o feed volta a saudável; reconhecimento não equivale a resolução. As rotas de API são:

- `GET /api/status`: último snapshot paper e saúde do feed.
- `GET /api/events?limit=50&event_type=market_data_error&since=<RFC3339>`: auditoria consultável; `limit` é limitado a 500.
- `GET /api/alerts?limit=50&active_only=true`: alertas recentes/ativos.
- `POST /api/alerts/{id}/ack`: reconhece um alerta, sem afetar o bot.
- `GET /api/capabilities`: informa se o comentário de IA foi habilitado.
- `POST /api/advisory`: solicita comentário somente se a IA foi habilitada, o feed está saudável e o snapshot tem até 60 segundos; há intervalo mínimo de 30 segundos.

Exemplo de consulta sem expor o token em URL:

```bash
curl -H "Authorization: Bearer $ODE_DASHBOARD_TOKEN" \
  "http://127.0.0.1:8787/api/events?limit=50"
```

O SQLite usa WAL e sincronização full; a tabela de eventos é append-only pela API do bot. O arquivo SQLite, seus sidecars e a auditoria JSONL ficam em `data/`, ignorada pelo Git. A carteira paper, saldo e posição ainda não são restaurados após reinício; o banco atual persiste auditoria/status, não um ledger de execução.

## Comentário de IA (opt-in; sem autoridade operacional)

Não há chamada a modelo por padrão. Para habilitar o botão de comentário, exporte credencial diretamente no ambiente (nunca a versione) e inicie o dashboard com `--enable-ai-advisory`:

```bash
export ODE_DASHBOARD_TOKEN="$(openssl rand -hex 32)"
export ODE_MODEL_API_KEY="<chave do seu provedor>"
# ODE_MODEL_BASE_URL padrão: https://api.openai.com/v1
# ODE_MODEL_NAME padrão: gpt-4o-mini
cargo run --release -- --dashboard --enable-ai-advisory \
  --symbol BTCUSDT --interval 1m
```

O botão só envia uma solicitação após ação manual autenticada. O adaptador aceita apenas mercado, intervalo, último preço público, último sinal, estado do feed e timestamp do candle; não recebe portfólio, saldo, ordens ou credenciais. Fora de loopback, a URL do provedor precisa usar HTTPS; redirects HTTP são desabilitados. Não há tool calling. A resposta é texto de comentário, limitada em tamanho, exibida como texto (não HTML), registrada no log apenas como metadados (modelo/provedor/tamanho) e não entra em `ode-risk`/`ode-execution`. Trate qualquer comentário como potencialmente incorreto; ele não é recomendação nem sinal executável.

## Arquitetura e testes

Crates: `ode-domain` (tipos), `ode-market-data` (CSV, integridade e latência), `ode-strategy-technical` (EMA modular), `ode-backtest` (replay/holdout), `ode-events` (auditoria), `ode-risk` (autorização independente), `ode-execution` (contratos de idempotência/reconciliação, ainda sem adaptador de venue), `ode-observability` (SQLite/API/dashboard) e `ode-model-adapter` (comentário read-only opt-in).

```bash
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

## Limitações e segurança

Saldo e posição paper reiniciam a cada execução; o latch e a auditoria são persistentes. O sinal EMA usa edge esperado zero, não calibrado, e não representa vantagem estatística. Fills históricos e stops não reproduzem a fila de ordens, liquidez, taxas reais, funding ou latência de mercado. A métrica de recebimento só é tão confiável quanto os timestamps fornecidos. O limite de perda do processo usa o capital de início como referência; não é um ledger diário persistente.

Não há execução real, adaptador de exchange, carteira persistente, futuros, DEX, integração Polymarket, notificações externas ou IA autônoma nesta fase. O comentário de IA opcional não determina sinais nem risco. Mesmo um OOS histórico não valida rentabilidade futura. Não use capital real com base neste projeto.

## Referência do feed

A Binance documenta endpoint, estrutura de klines e hosts públicos em [Market Data Endpoints](https://developers.binance.com/docs/binance-spot-api-docs/rest-api/market-data-endpoints). O bot usa somente consulta de candles públicos.
