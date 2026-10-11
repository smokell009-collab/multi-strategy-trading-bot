# Multi-Strategy Trading Bot (Claude + MCP)

> ⚠️ **AVISO DE RISCO:** Este projeto é **estritamente educacional**. Operar no mercado financeiro envolve risco real de **perda total do capital**. **Comece SEMPRE em paper trading / testnet** e só considere capital real após semanas de testes consistentes. Nada aqui é recomendação de investimento, não há promessa de lucro, e resultados simulados **não** garantem resultados futuros. Você é o único responsável por qualquer operação executada.

## 🎯 Objetivo

Base organizada para um bot **multi-estratégia** em que o **Claude** (Claude Desktop ou Claude Code) atua como **orquestrador**: ele lê dados de mercado, avalia sinais e executa ordens por meio de servidores **MCP (Model Context Protocol)** conectados a diferentes venues — sempre sob regras rígidas de gestão de risco.

## 🧩 Estratégias e venues

| Módulo | Venue | Uso principal |
|---|---|---|
| Análise técnica | **TradingView** | Indicadores, screeners, sinais |
| Mercados de previsão | **Polymarket** | Probabilidades de eventos (Gamma/CLOB/Data APIs) |
| Ações e ETFs (EUA) | **Alpaca** | Execução em conta *paper* |
| Cripto | **CCXT** (Binance, Bybit, etc.) | Dados e execução em **testnet** |

## 🏗 Estrutura

```
multi-strategy-trading-bot/
├── README.md
├── .gitignore
├── .env.example                             # Modelo das variáveis de ambiente
├── configs/claude_desktop_config.example.json   # Exemplo de config MCP
├── docs/                                    # Guias de setup e arquitetura
├── prompts/system-prompt-orquestrador.md    # System prompt do orquestrador
├── scripts/install-mcps.sh                  # Comandos para instalar os MCPs
└── resources/links-uteis.md                 # Links oficiais e community
```

## 🚀 Ordem recomendada de setup

1. Leia [`docs/arquitetura.md`](docs/arquitetura.md)
2. Rode [`scripts/install-mcps.sh`](scripts/install-mcps.sh)
3. [Alpaca (paper)](docs/setup-alpaca.md)
4. [TradingView](docs/setup-tradingview.md)
5. [CCXT (testnet)](docs/setup-ccxt.md)
6. [Polymarket (somente leitura no início)](docs/setup-polymarket.md)
7. Copie `configs/claude_desktop_config.example.json` para o local de config do Claude e preencha as keys
8. Copie `.env.example` para `.env` e preencha as variáveis
9. Cole o conteúdo de [`prompts/system-prompt-orquestrador.md`](prompts/system-prompt-orquestrador.md) como instrução do projeto
10. Rode **semanas** em paper antes de considerar capital real

## 🔗 Links oficiais

| Recurso | Link |
|---|---|
| **TradingView MCP (oficial)** | **https://mcp.tradingview.com/mcp** |
| Model Context Protocol | https://modelcontextprotocol.io |
| Claude Desktop | https://claude.ai/download |
| Alpaca MCP Server | https://github.com/alpacahq/alpaca-mcp-server |
| Alpaca Docs | https://docs.alpaca.markets |
| Polymarket Docs | https://docs.polymarket.com |
| CCXT | https://github.com/ccxt/ccxt |

Lista completa (oficiais + community) em [`resources/links-uteis.md`](resources/links-uteis.md).

## ⚠️ Avisos importantes

- **Paper trading / testnet primeiro. Sempre.**
- Nunca faça commit de API keys (`.env` já está no `.gitignore`).
- Use keys com **permissão de saque desabilitada**.
- LLMs podem errar, alucinar dados ou interpretar mal instruções. Mantenha supervisão humana.
- Verifique a legislação do seu país (ex.: Polymarket tem restrições geográficas).
- Resultados passados ou simulados não garantem resultados futuros.

## 📜 Créditos

Criado com **Grok** + **Lovable**. Repositórios de terceiros pertencem aos seus respectivos autores.
