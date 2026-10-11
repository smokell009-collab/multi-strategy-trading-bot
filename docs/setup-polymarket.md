# Polymarket — monitor somente leitura e simulação paper

## Escopo

O binário `polymarket-watch` faz consulta pública de mercado, outcome, histórico, book e trades e pode simular sinais/fills localmente em pUSD. Ele não tem autenticação de usuário, chave privada, signer, user WebSocket, API key, endpoint POST de ordem nem código de execução Polymarket. Não precisa de arquivo `.env` nem de saldo/carteira.

“Cripto em dólar” neste modo significa **mercados de previsão relacionados a BTC/ETH/cripto**, onde cada share YES/NO tem preço probabilístico em pUSD. Não é negociação spot de BTC/USD, compra de criptomoeda ou stablecoin. O operador precisa fornecer o slug específico; não há seleção automática de mercado.

A documentação consultada classifica os Estados Unidos como **close-only** para o Polymarket. Por isso o projeto mantém essa integração em leitura e simulação local: nenhuma nova posição é aberta na venue. Disponibilidade das APIs públicas pode variar pela rede/região do dispositivo onde o programa rodar.

## Serviços públicos integrados

| Serviço | Endpoint público | Uso implementado |
|---|---|---|
| Gamma | `https://gamma-api.polymarket.com/markets/slug/{slug}` | Pergunta, condition, status, outcomes, fee schedule e restrições de mercado |
| CLOB REST | `https://clob.polymarket.com/book?token_id=...` | Snapshot do book e top-of-book |
| Data API | `https://data-api.polymarket.com/v2/prices-history` | Histórico de preços para aquecer as estratégias |
| CLOB WebSocket | `wss://ws-subscriptions-clob.polymarket.com/ws/market` | Snapshots/deltas de book e last trade; `PING` texto a cada 10 s |

O parser escolhe IDs conforme versão: `clobTokenIds` em mercados V1 e `positionIds` em V2, e confere os rótulos YES/NO. Não usa Gamma para enviar ordens nem endpoint autenticado.

## Instalação e comandos

Requisitos: Rust/Cargo do workspace. Exemplo de snapshot REST único:

```bash
cargo run --release --bin polymarket-watch -- \
  --slug 'SLUG-EXATO-DO-MERCADO' --outcome yes --once
```

Monitor contínuo com as duas estratégias em contas paper independentes:

```bash
cargo run --release --bin polymarket-watch -- \
  --slug 'SLUG-EXATO-DO-MERCADO' --outcome yes \
  --interval 1m --strategy both
```

Opções principais:

- `--outcome yes|no`: outcome acompanhado.
- `--interval 1m|5m|15m|30m|1h|4h`: cadência das candles sintéticas.
- `--strategy both|ema-crossover|price-rsi-divergence`: seleção explícita; `both` é o padrão e separa saldos paper por estratégia.
- `--strategy-config configs/polymarket-strategies.example.toml`: exemplo habilita EMA 9/21 e divergência RSI.
- `--risk-overrides configs/risk-overrides.example.toml`: overrides só podem apertar os limites-base.
- `--starting-balance-pusd 1000`: saldo virtual por estratégia; snapshot existente prevalece após reinício.
- `--audit-db data/polymarket-paper.sqlite3`: eventos e carteira paper persistente.
- `--kill-switch-file data/KILL_SWITCH.json`: latch já compartilhado pelo projeto; bloqueia novas entradas, sem apagar posição paper aberta.
- `--max-runtime-seconds N`: encerra após N segundos; útil para smoke test do WebSocket.

Use Ctrl+C para encerrar. O comando `--once` só lê REST e encerra; o modo contínuo assina apenas o canal de mercado público. A auditoria e os saldos paper não são apagados automaticamente. O estado de cada conta é separado por slug, outcome, intervalo e estratégia.

## Como os sinais e fills paper funcionam

1. Carrega série de preço do token YES/NO e aceita apenas pontos com resolução/cadência pedida, candles fechados e sufixo temporal contínuo.
2. Converte os pontos de histórico em candles sintéticas com `open=high=low=close` e volume zero; candles live agregam `last_trade_price` por período. Não preenche lacunas.
3. Avalia EMA crossover e/ou preço-RSI em histórico fechado; o feed é preço do outcome binário, não o preço spot do ativo que aparece no texto do mercado.
4. Emite uma conta paper isolada por estratégia. BUY passa pelo `RiskEngine`, pela qualidade/freshness do book, pelo kill-switch, pelo limite de slippage, pelo saldo e pelo mínimo do mercado. SELL e stop simulam venda contra bids.
5. Fills paper consomem profundidade disponível até o limite de slippage e descontam a fee curve documentada quando Gamma fornece fee schedule suportado (expoente 1). Fee schedule ausente/incompatível bloqueia os fills; o monitor segue apenas lendo.
6. Eventos de fill e snapshot da conta são gravados na mesma transação SQLite. O `stdout` é JSONL para facilitar observação.

Cada estratégia recebe saldo próprio e pode simular operações simultâneas no mesmo outcome; `--strategy both` **não** é carteira combinada nem ensemble. `expected_net_edge_bps` não é estimado pela estratégia; a autorização usa o piso zero do motor. Não interprete sinais ou saldo paper como recomendação.

## Limites e jurisdição

- O programa usa somente feed público de mercado; não consulta posições privadas, saldo de usuário, geoblock da conta ou credenciais.
- O acesso às APIs públicas foi testado a partir do ambiente sandbox desta implementação; isso não prova acesso no dispositivo/rede do usuário, particularmente nos EUA.
- O adaptador não contém qualquer caminho de ordens Polymarket. O feed read-only não autoriza o usuário a negociar; restrições do serviço e leis locais prevalecem.
- Uma resolução de outcome pode acontecer sem sinal técnico útil; estratégia técnica sobre probabilidades não é previsão independente.
- Candles de histórico são observações pontuais convertidas para OHLC sintético. Divergência pode ser especialmente sensível a esse proxy e à falta de extremos intrabar.
- Simulação consome book atual de forma instantânea, sem fila, latência, impacto, partial fill real, resolução/claim, interrupções prolongadas ou liquidez futura.
- Não há walk-forward para esse feed, medição de edge, validação estatística, seleção automática de contratos ou garantia de rentabilidade.
- O método de taxa suportado segue a fórmula oficial `C × feeRate × p × (1-p)` para expoente 1. Outros expoentes são fail-closed, sem extrapolação.

## Fontes oficiais

- [Introdução e APIs de dados](https://docs.polymarket.com/market-data/overview)
- [Preços, books e histórico](https://docs.polymarket.com/market-data/prices-order-books)
- [Dados realtime e WebSocket](https://docs.polymarket.com/market-data/realtime-data)
- [Detalhes de mercado e `feeSchedule`](https://docs.polymarket.com/market-data/market-details)
- [Fórmula de taxas](https://docs.polymarket.com/trading/fees)
- [Restrições geográficas](https://docs.polymarket.com/api-reference/geoblock)
- [Migração CLOB V2](https://docs.polymarket.com/v2-migration)

Veja também a nota de implementação e evidências em [`phase-polymarket-paper-monitor.md`](phase-polymarket-paper-monitor.md).
