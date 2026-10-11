# Multi-Strategy Trading Bot (Claude + MCP)

> ⚠️ **AVISO DE RISCO:** Este projeto é educacional. Operar no mercado financeiro envolve risco real de perda total do capital. **Comece SEMPRE em paper trading / testnet.** Nada aqui é recomendação de investimento e não há promessa de lucro.

## 🎯 Objetivo

Base organizada para um bot **multi-estratégia** em que o **Claude** (Claude Desktop ou Claude Code) atua como **orquestrador**: ele lê dados de mercado, avalia sinais e executa ordens por meio de servidores **MCP (Model Context Protocol)** conectados a diferentes venues — sempre sob regras rígidas de gestão de risco.

## 🧩 Estratégias e venues

| Módulo | Venue | Uso principal |
|---|---|---|
| Análise técnica | **TradingView** | Indicadores, screeners, sinais |
| Mercados de previsão | **Polymarket** | Probabilidades de eventos (Gamma/CLOB/Data APIs) |
| Ações e ETFs (EUA) | **Alpaca** | Execução em conta **paper** |
| Cripto | **CCXT** (Binance, Bybit, etc.) | Dados e execução em **testnet** |

## 📁 Estrutura

```
multi-strategy-trading-bot/
├── README.md
├── .gitignore
├── configs/claude_desktop_config.example.json   # Exemplo de config MCP
├── docs/                                         # Guias de setup e arquitetura
├── prompts/system-prompt-orquestrador.md         # System prompt do orquestrador
├── scripts/install-mcps.sh                       # Comandos para instalar os MCPs
└── resources/links-uteis.md                      # Links oficiais e repos
```

## 🚀 Ordem recomendada de setup

1. Leia [`docs/arquitetura.md`](docs/arquitetura.md)
2. Rode [`scripts/install-mcps.sh`](scripts/install-mcps.sh)
3. [Alpaca (paper)](docs/setup-alpaca.md)
4. [TradingView](docs/setup-tradingview.md)
5. [CCXT (testnet)](docs/setup-ccxt.md)
6. [Polymarket (somente leitura no início)](docs/setup-polymarket.md)
7. Copie `configs/claude_desktop_config.example.json` para o local de config do Claude e preencha as keys
8. Cole o conteúdo de [`prompts/system-prompt-orquestrador.md`](prompts/system-prompt-orquestrador.md) como instrução do projeto
9. Rode **semanas** em paper antes de considerar capital real

## 🔗 Links oficiais

| Recurso | Link |
|---|---|
| Model Context Protocol | https://modelcontextprotocol.io |
| Claude Desktop | https://claude.ai/download |
| Alpaca MCP Server | https://github.com/alpacahq/alpaca-mcp-server |
| Alpaca Docs | https://docs.alpaca.markets |
| Polymarket Docs | https://docs.polymarket.com |
| CCXT | https://github.com/ccxt/ccxt |
| CCXT MCP (community) | https://github.com/doggybee/mcp-server-ccxt |
| TradingView MCP (community) | https://github.com/atilaahmettaner/tradingview-mcp |

Lista completa em [`resources/links-uteis.md`](resources/links-uteis.md).

## ⚠️ Avisos importantes

- **Paper trading / testnet primeiro. Sempre.**
- Nunca faça commit de API keys (`.env` já está no `.gitignore`).
- Use keys com **permissão de saque desabilitada**.
- LLMs podem errar, alucinar dados ou interpretar mal instruções. Mantenha supervisão humana.
- Verifique a legislação do seu país (ex.: Polymarket tem restrições geográficas).
- Resultados passados ou simulados não garantem resultados futuros.

## 🙏 Créditos

Criado com **Grok** + **Lovable**. Repositórios de terceiros pertencem aos seus respectivos autores.
