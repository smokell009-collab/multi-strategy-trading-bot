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
