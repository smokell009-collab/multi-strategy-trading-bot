# Olho de Deus — bot paper e fundação de risco

## O que funciona

- Executável Rust assíncrono que consulta candles Spot públicos da Binance (`data-api.binance.vision`, `GET /api/v3/klines`), sem credenciais.
- Filtra candles abertos e bloqueia dados atrasados antes de avaliar entrada.
- Estratégia EMA 9/21 sobre candles fechados.
- Paper broker long-only com sizing aprovado pelo motor de risco, teto de alocação, taxas/slippage simuladas e stop. Nenhuma ordem real é enviada.
- Limite de perda diária que bloqueia novas entradas e latch de emergência persistente.
- Auditoria JSONL local em `data/audit.jsonl`, com IDs únicos e payload estruturado.
- Políticas de base pelas flags do CLI e overrides por estratégia/ativo em TOML. Overrides são apenas restritivos: não podem aumentar os tetos configurados na base.
- Crates separados: `ode-domain` (tipos), `ode-events` (eventos), `ode-risk` (autorização independente) e `ode-execution` (contratos de idempotência/reconciliação).

## Kill-switch persistente

Ative o latch e encerre o comando:

```bash
cargo run --release -- --activate-kill-switch "pausa operacional"
```

O estado é gravado atomicamente em `data/KILL_SWITCH.json` e sobrevive ao reinício. No modo contínuo, o bot o consulta em cada ciclo de polling e bloqueia entradas novas; o latch não fecha automaticamente posições existentes. Se o arquivo estiver corrompido ou ilegível, o bot falha fechado e encerra, sem autorizar nova entrada. O binário não oferece comando de desativação; apenas uma pessoa autorizada deve remover o arquivo após revisão da ocorrência e decisão explícita de rearme.

Para configurar limites mais conservadores por estratégia/ativo, edite uma cópia revisada de `configs/risk-overrides.example.toml` e passe o caminho:

```bash
cargo run --release -- --risk-overrides configs/risk-overrides.example.toml --once
```

Os valores percentuais do TOML usam basis points: `100` = `1%`; `10000` = `100%`. Exemplo: reduzir risco para `50` limita a operação a 0,5%; uma política por ativo ou estratégia jamais relaxa o limite-base do CLI. Campos ausentes mantêm a base.

## Execução paper

Consulta única:

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

Use `Ctrl+C` para encerrar. A auditoria fica em `data/audit.jsonl` (pasta ignorada pelo Git). Os percentuais do CLI são frações: `0.01` significa 1%. Consulte `--help` para as opções completas.

## Desenvolvimento e testes

```bash
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Os testes incluem sizing baseado em propriedade, rejeição de dados atrasados/inválidos, persistência e corrupção do kill-switch, submissão idempotente e divergências de reconciliação. `ode-execution` ainda não conversa com venue alguma: são contratos e testes isolados, sem envio de ordens.

## Limitações e segurança

Este é um MVP educacional. O saldo, a posição e o início do período diário permanecem em memória e reiniciam a cada execução; o kill-switch e o log de auditoria são persistentes. A EMA não tem edge líquido calibrado: o sinal registra edge igual a zero e passa apenas a política-base mínima atual (zero), portanto não representa vantagem estatística validada. O stop paper avalia o fechamento do candle, não a execução intrabar. Os fills simulados não reproduzem o livro real, liquidez ou latência.

Não há backtest completo, reconciliação com conta real, painel, adaptador de execução, carteira, futuros, DEX, Polymarket ou integração de IA nesta fase. Mesmo paper trading não valida rentabilidade futura. Não use capital real com base neste projeto.

## Referência de dados

A Binance documenta o endpoint, a estrutura de klines e os hosts públicos em [Market Data Endpoints](https://developers.binance.com/docs/binance-spot-api-docs/rest-api/market-data-endpoints). O bot usa somente consulta de candles públicos.
