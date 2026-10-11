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
