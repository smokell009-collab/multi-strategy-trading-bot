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
├── Cargo.toml / Cargo.lock              # Workspace Rust
├── src/main.rs                          # Bot paper executável
├── crates/                              # Domínio, dados, estratégia, backtest, risco e execução
├── configs/                             # Exemplos de configuração
├── docs/                                # Guias, arquitetura e segurança
├── prompts/                             # Prompt do orquestrador
└── .github/workflows/rust-ci.yml        # CI de workspace
```

## 🚀 Ordem recomendada de setup

1. Leia [`docs/arquitetura.md`](docs/arquitetura.md)
2. Rode [`scripts/install-mcps.sh`](scripts/install-mcps.sh)
3. Consulte os guias de [Alpaca (paper)](docs/setup-alpaca.md), [TradingView](docs/setup-tradingview.md), [CCXT (testnet)](docs/setup-ccxt.md) e [Polymarket (somente leitura no início)](docs/setup-polymarket.md)
4. Copie `configs/claude_desktop_config.example.json` para o local de config do Claude e preencha as keys
5. Copie `.env.example` para `.env` e preencha as variáveis necessárias aos MCPs
6. Cole o conteúdo de [`prompts/system-prompt-orquestrador.md`](prompts/system-prompt-orquestrador.md) como instrução do projeto
7. Rode **semanas** em paper antes de considerar capital real

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

## Bot Rust executável — Fase 1

O primeiro MVP de **paper trading** consulta candles públicos da Binance, identifica cruzamentos EMA 9/21 e simula operações. Ele não envia ordens reais nem precisa de chaves privadas.

```bash
cargo run --release -- --symbol BTCUSDT --interval 1m --once
```

Para executar em loop, remova `--once`. O guia completo está em [`docs/bot-paper-mvp.md`](docs/bot-paper-mvp.md). A configuração inicial ilustrativa está em [`configs/risk.paper.example.toml`](configs/risk.paper.example.toml).

## Fase 2 — domínio e governança de risco

O workspace separa os contratos em `crates/ode-domain`, `crates/ode-events`, `crates/ode-risk` e `crates/ode-execution`. O motor valida qualidade/idade de dados, validade do sinal, edge mínimo, slippage, posição aberta, perda diária, saldo e sizing; overrides por ativo/estratégia só podem **apertar** os limites-base. O kill-switch fica persistido em `data/KILL_SWITCH.json` e bloqueia entradas também após reinício.

```bash
cargo run --release -- --activate-kill-switch "pausa operacional"
cargo test --workspace --locked
```

A ativação grava o latch e termina. Não existe flag de desativação no bot; o rearme exige revisão humana e remoção manual do arquivo por operador autorizado. `ode-execution` cobre idempotência/reconciliação em contratos testados, mas ainda não contém adaptador de exchange nem envia ordens reais. Detalhes: [`docs/bot-paper-mvp.md`](docs/bot-paper-mvp.md).

## Fase 3 — dados e avaliação da estratégia

O bot aceita histórico OHLCV em CSV, valida preços, timestamps e cadência, mede latência quando `received_at_ms` está disponível e roda um holdout cronológico com a EMA modular. O relatório separa in-sample do trecho OOS, inclui custos, sizing pelo mesmo motor de risco, drawdown e comparação com buy-and-hold. Configure o interruptor/períodos em [`configs/strategies.example.toml`](configs/strategies.example.toml).

```bash
cargo run --release -- --backtest-csv data/BTCUSDT-1m.csv \
  --symbol BTCUSDT --interval 1m --in-sample-ratio 0.7 \
  --backtest-report data/backtest-report.json
```

É um **holdout simples**, não walk-forward rolling nem otimização de parâmetros; o resultado de uma amostra não comprova vantagem estatística nem rentabilidade futura. Formato CSV, pressupostos e limitações: [`docs/bot-paper-mvp.md`](docs/bot-paper-mvp.md).

## ⚠️ Avisos importantes

- **Paper trading / testnet primeiro. Sempre.**
- Nunca faça commit de API keys (`.env` já está no `.gitignore`).
- Use keys com **permissão de saque desabilitada**.
- LLMs podem errar, alucinar dados ou interpretar mal instruções. Mantenha supervisão humana.
- Verifique a legislação do seu país (ex.: Polymarket tem restrições geográficas).
- O bot Rust atual é um MVP educacional: o backtest é um holdout simples, sem otimização ou inferência estatística; não há execução real nem promessa de lucro. Saldo/posição paper reiniciam a cada execução.
- Resultados passados ou simulados não garantem resultados futuros.

## 📜 Créditos

Criado com **Grok** + **Lovable**. Repositórios de terceiros pertencem aos respectivos autores.
