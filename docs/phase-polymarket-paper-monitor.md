# Fase Polymarket — feed público e paper pUSD

**Estado:** implementação e validação local concluídas; CI remoto pendente de publicação/verificação.

## Objetivo confirmado

A direção do usuário é reaproveitar as estratégias e os contratos atuais para observar mercados Polymarket relacionados a cripto e simular operações cotadas em pUSD. A interpretação implementada é **shares YES/NO de mercados de previsão**, não spot BTC/ETH/USD. O binário recebe um slug e outcome explícitos; não faz discovery amplo nem escolhe um contrato.

A jurisdição informada é Estados Unidos. A documentação oficial consultada declara a região como close-only para abertura de posições. A implementação, portanto, é read-only/paper; não acrescenta ordem, signing, carteira, execução Polymarket ou rotina para contornar geoblock.

## Componentes entregues

- `crates/ode-polymarket`: client HTTP anônimo, Gamma V1/V2, snapshot CLOB, histórico Data API, parser de snapshots/deltas/trades e WebSocket público com subscription inicial, heartbeat `PING` a cada 10 s e reconexão exponencial até 30 s.
- `src/bin/polymarket_watch.rs` (`polymarket-watch`): seleção de outcome, histórico candles sintético, agregação de trade prints, candles fechados, simulação contra profundidade, limites de risco, kill-switch e persistência.
- `configs/polymarket-strategies.example.toml`: EMA 9/21 e divergência preço/RSI habilitadas para este modo sem alterar a configuração/defaults da binária principal.
- Estratégias rodam em contas pUSD isoladas e persistentes. `both` compara cada estratégia; não agrega saldos ou sinais em um ensemble.
- Fees: usa Gamma `feesEnabled` e `feeSchedule`; a fórmula oficial `C × rate × p × (1-p)` só é aplicada quando `exponent=1`. Fee desconhecida/incompatível bloqueia paper fills, não o monitor.
- SQLite reutiliza `AuditStore` e `PaperPortfolioState`; cada fill e snapshot da carteira são confirmados juntos.

## Segurança/ausências deliberadas

O crate não depende de nem referencia endpoints de autenticação/ordens. Não há private key, API secret, user WebSocket, signing code, POST de ordem, wallet state ou endpoint configurável. `--once` faz GET e encerra. O modo contínuo usa apenas o canal público de mercado. O kill-switch compartilhado bloqueia novas entradas paper; posições virtuais podem ser encerradas em simulação.

O risco esperado permanece zero bps por ausência de estimativa independente de edge; o motor protege estrutura/tamanho/freshness, não demonstra vantagem. A simulação é mercado-agressiva sobre o book atual, mas não reproduz prioridade de fila, latência, impacto futuro, resolução/claim nem disponibilidade contínua.

## Construção de candles

- Histórico oficial Data API expõe observações de preço e `resolution_seconds`, não OHLC completo no endpoint usado. Cada ponto vira OHLC sintético igual ao preço e volume zero.
- Eventos `last_trade_price` compõem OHLC/volume por intervalo para candles live.
- Histórico com resolução diferente, intervalo incompleto, ponto futuro ou lacuna é excluído; somente sufixo contínuo é usado. Candles de período sem trades não são fabricados.
- EMA e divergência usam a interface de estratégia existente, mas estão aplicadas a probabilidades/preços de outcome em pUSD. A divergência em extrema de OHLC sintético é exploratória e mais fraca que um feed real de barras.

## Evidências desta revisão

- O endpoint Gamma público foi consultado e retornou um mercado binário V1 com dois IDs CLOB, status ativo, fee schedule e min tick/order.
- O endpoint CLOB público retornou book com top-of-book e níveis.
- `polymarket-watch --once` foi executado com sucesso e imprimiu `mode=read-only`, best bid/ask pUSD e `live_order_sent=false`.
- A execução contínua por 12 s conectou ao WebSocket público, recebeu snapshot válido e top-of-book, atravessou o intervalo do heartbeat e encerrou por timeout com `live_order_sent=false`.
- SQLite temporário persistiu as duas carteiras paper independentes (EMA e divergência); o smoke não gerou/alegou fill.
- As chamadas externas verificadas foram somente GET REST ou subscription no canal público de dados, sem conta/signer.
- `cargo fmt --all -- --check`, `cargo test --workspace --locked` (79 testes, 0 falhas), `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` e `git diff --check` passaram localmente.
- O teste ocorreu a partir do egress do sandbox da sessão (geolocalização detectada como fora dos EUA); não comprova conectividade para o dispositivo/rede do usuário nos Estados Unidos.

## Comandos

```bash
cargo run --release --bin polymarket-watch -- \
  --slug 'SLUG-DO-MERCADO' --outcome yes --once

cargo run --release --bin polymarket-watch -- \
  --slug 'SLUG-DO-MERCADO' --outcome yes --interval 1m --strategy both
```

Persistência padrão: `data/polymarket-paper.sqlite3`; latch compartilhado: `data/KILL_SWITCH.json`. Cada account key inclui mercado, outcome, intervalo e strategy ID. No reinício, estado persistido prevalece sobre `--starting-balance-pusd`.

## Validação ainda pendente

O CI remoto deve ser consultado após o commit. Os smokes exercitam apenas APIs públicas no egress do sandbox, não execução, rentabilidade ou elegibilidade geográfica do usuário/dispositivo.

## Fontes oficiais

- [Market data overview](https://docs.polymarket.com/market-data/overview)
- [Prices, order books e history](https://docs.polymarket.com/market-data/prices-order-books)
- [Realtime/WebSocket](https://docs.polymarket.com/market-data/realtime-data)
- [Market details e fee schedule](https://docs.polymarket.com/market-data/market-details)
- [Fees](https://docs.polymarket.com/trading/fees)
- [Geoblock](https://docs.polymarket.com/api-reference/geoblock)
- [CLOB V2 migration](https://docs.polymarket.com/v2-migration)
