# Arquitetura

```
                    ┌─────────────────────────┐
                    │        Usuário           │
                    │ (supervisão e aprovação) │
                    └───────────┬────────────┘
                                 │
                    ┌────────────▼────────────┐
                    │   Claude (Orquestrador)  │
                    │ system-prompt + regras   │
                    │ de risco                 │
                    └────────────┬────────────┘
                                 │ MCP (stdio)
     ┌────────────┬───────────┼───────────┬───────────────┐
     ▼              ▼            ▼             ▼
┌──────────┐  ┌──────────┐  ┌──────────┐  ┌────────────┐
│TradingView│ │  Alpaca  │  │   CCXT   │  │ Polymarket │
│ análise  │  │ ações    │  │ cripto   │  │ previsão   │
│ (leitura)│  │ (paper)  │  │(testnet) │  │ (leitura)  │
└──────────┘  └────┬─────┘  └────┬─────┘  └─────┬──────┘
                   ▼             ▼              ▼
              Alpaca API   Binance/Bybit   Gamma/CLOB/Data APIs
```

## Camadas
1. **Cérebro (Claude):** interpreta dados, aplica estratégia e regras de risco.
2. **Ferramentas (MCPs):** processos locais que expõem APIs como ferramentas.
3. **Venues:** corretoras/exchanges/mercados reais (em paper/testnet por padrão).
4. **Humano no loop:** aprova ordens e monitora.

## Princípios
- Separação de leitura (análise) e escrita (execução).
- Keys apenas em variáveis de ambiente, nunca no repositório.
- Falha segura: sem dado confiável → sem ordem.

## Fase 4 — observabilidade e fronteira de IA

```text
Feed público → estratégia → ode-risk → executor paper
                         ├→ eventos → ode-observability → SQLite + JSONL
                         └→ snapshot paper → API local (loopback + Bearer) → dashboard

Usuário autenticado → POST manual → ode-model-adapter → provedor HTTPS
                                   ↑ contexto público limitado
                         texto consultivo sem retorno ao fluxo de risco/execução
```

O crate `ode-observability` não implementa execução de ordens. O dashboard é opt-in, aceita apenas endereços loopback e autentica todas as APIs de dados com token Bearer; páginas usam CSP estrita, sem CDN, e não habilitam CORS. SQLite registra eventos e status; o JSONL existente continua disponível.

`ode-model-adapter` não depende de `ode-risk` nem de `ode-execution`; o contrato de entrada só contém mercado, intervalo, preço público, sinal observado, saúde do feed e timestamp. O adaptador não declara tools/funções e devolve texto limitado, que o dashboard renderiza como texto. A chamada exige habilitação explícita e clique autenticado; nenhuma chamada é feita em background, e nada da resposta é interpretado como ordem ou override de risco.

## Recuperação da carteira paper

`ode-observability` mantém um snapshot SQLite versionado por estratégia/par/intervalo. Ele inclui caixa, posição, preço/stop, custos, âncora diária UTC e último candle tratado. O estado e o evento de fill são confirmados numa única transação; snapshots incompatíveis ou inválidos interrompem a inicialização, sem reiniciar silenciosamente a carteira.


## Fase 5 — execução manual Spot Testnet

```text
CLI testnet-* ──> preflight / ordem MARKET manual / reconcile
                    │
                    ├──> ode-risk (entrada buy) + filtros Spot + kill-switch
                    ├──> ode-execution: SQLite intent/idempotência/lock por símbolo
                    └──> ode-binance-testnet ──HTTPS/HMAC──> testnet.binance.vision

Produção: sem host, credencial, flag ou caminho de execução no binário.
```

`ode-binance-testnet` tem URL-base constante para Spot Testnet, desativa redirects e sincroniza horário antes das requisições assinadas. A intenção é persistida antes do POST; resultado incerto vira estado não terminal e bloqueia nova ordem até reconciliação explícita por `clientOrderId`. O guard de mercado é transacional para processos concorrentes. Só há ordens `MARKET` manuais; não há integração automática da EMA, stop remoto ou execução em produção. Os testes usam mocks e não submetem ordens à exchange.


## Fase 6 — contratos de segurança

```text
estratégia
   │ TradeSignal::validate_shape
   ▼
ode-risk ── qualidade, idade, expiração, edge, slippage, sizing
   │
   ▼
ExecutionSafety ── kill-switch / mercado / conta / reconciliação
   │
   ▼
intent persistido ──> POST único ──> resposta da venue
                                      │
                         campos, status e fills comparados
                                      ▼
                          terminal ou revisão obrigatória
```

`TradeSignal::validate_shape` verifica a forma do contrato antes que o sizing seja calculado. A idade do sinal e do mercado continua sendo responsabilidade contextual do `ode-risk`. `ExecutionSafety` não envia ordens: ele é um gate puro que impede novas intenções quando qualquer estado crítico é incerto. O resultado `Unknown` nunca é terminal e exige reconciliação explícita antes de outra ordem no mercado.

## Fase C — monitor Polymarket somente leitura e paper

```text
Gamma + Data API (GET) ──> mercado/outcome + histórico de preço
CLOB REST (GET) ─────────> snapshot book ─┐
CLOB market WebSocket ───> book delta/trades ─> candle outcome (pUSD) ─> EMA / divergência
                                                                   │
                                                                   ▼
                                                        ode-risk + kill-switch
                                                                   │
                                                                   ▼
                                                   fill paper contra níveis do book
                                                                   │
                                                                   ▼
                                                    AuditStore SQLite / JSONL stdout

Autenticação de usuário/signing/POST de ordem: inexistentes neste caminho.
```

O crate `ode-polymarket` é somente leitura; o binário `polymarket-watch` é separado do `src/main.rs`, da Binance Spot Testnet e de seus segredos. Ele simula long-only shares YES/NO com contas persistentes independentes em pUSD, uma por estratégia/mercado/outcome/intervalo. O histórico em pontos vira candles OHLC sintéticos; trades públicos compõem candles live. Essas barras descrevem probabilidade de outcome, não preço spot do ativo citado. A simulação usa taxa conhecida e liquidez visível no book, sem prioridade de fila, latência futura ou execução na venue. Nos Estados Unidos, o escopo deliberado permanece leitura/paper; não há caminho de nova posição no Polymarket.
