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
| Cripto | Binance Spot Testnet (Rust) + conectores MCP/CCXT existentes | Dados públicos e ordens manuais somente em **testnet** |

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

A ativação grava o latch e termina. Não existe flag de desativação no bot; o rearme exige revisão humana e remoção manual do arquivo por operador autorizado. `ode-execution` cobre idempotência/reconciliação; o adaptador adicionado na Fase 5 é fixo à Spot Testnet e não envia ordens de produção. Detalhes: [`docs/bot-paper-mvp.md`](docs/bot-paper-mvp.md).

## Fase 3 — dados e avaliação da estratégia

O bot aceita histórico OHLCV em CSV, valida preços, timestamps e cadência, mede latência quando `received_at_ms` está disponível e roda um holdout cronológico com a EMA modular. O relatório separa in-sample do trecho OOS, inclui custos, sizing pelo mesmo motor de risco, drawdown e comparação com buy-and-hold. Configure o interruptor/períodos em [`configs/strategies.example.toml`](configs/strategies.example.toml).

```bash
cargo run --release -- --backtest-csv data/BTCUSDT-1m.csv \
  --symbol BTCUSDT --interval 1m --in-sample-ratio 0.7 \
  --backtest-report data/backtest-report.json
```

É um **holdout simples**, não walk-forward rolling nem otimização de parâmetros; o resultado de uma amostra não comprova vantagem estatística nem rentabilidade futura. Formato CSV, pressupostos e limitações: [`docs/bot-paper-mvp.md`](docs/bot-paper-mvp.md).

## Fase 4 — observabilidade local e comentário de IA opcional

O loop paper persiste eventos em SQLite (`data/olho.sqlite3`) e mantém a cópia JSONL. Para abrir o dashboard, gere um token local de pelo menos 32 caracteres e inicie o bot em loop contínuo:

```bash
export ODE_DASHBOARD_TOKEN="$(openssl rand -hex 32)"
cargo run --release -- --dashboard --symbol BTCUSDT --interval 1m
```

Acesse `http://127.0.0.1:8787` e informe o token. O servidor só aceita bind em loopback; todas as APIs de dados exigem `Authorization: Bearer ...`. O painel exibe status paper, latência, alertas (com reconhecimento) e eventos recentes. Exemplo de consulta:

```bash
curl -H "Authorization: Bearer $ODE_DASHBOARD_TOKEN" \
  "http://127.0.0.1:8787/api/events?limit=50&event_type=market_data_error"
```

O adaptador de modelo é desligado por padrão. Para habilitá-lo, configure `ODE_MODEL_API_KEY` (e, se necessário, `ODE_MODEL_BASE_URL`/`ODE_MODEL_NAME`) e acrescente `--enable-ai-advisory`. A chamada só acontece quando solicitada manualmente na interface autenticada; o provedor recebe exclusivamente mercado, intervalo, preço, último sinal e saúde do feed. Não recebe saldo, posição, credenciais ou ordens; não tem ferramentas nem rota para controlar risco/execução. O resultado é texto consultivo e não é executado pelo bot. Consulte [o guia de operação](docs/bot-paper-mvp.md) para limites e segurança.

## Fase 5 — execução manual somente na Binance Spot Testnet

O executável Rust oferece três subcomandos, separados do modo paper: `testnet-preflight` (somente leitura), `testnet-place-market-order` (ordem `MARKET` após gates e confirmação textual exata) e `testnet-reconcile-order` (consulta de uma ordem já journaled). Esta fase aceita somente `BTCUSDT`; o host é constante (`testnet.binance.vision`) e nenhum argumento ou variável pode trocar o destino para produção. O código não possui caminho de execução real.

Configure **chaves criadas no portal Spot Testnet**, sem saque e com permissões mínimas. O binário Rust não carrega `.env` automaticamente; solicite as credenciais sem ecoá-las no terminal:

```bash
read -r -s -p "Binance Spot Testnet API key: " BINANCE_API_KEY; export BINANCE_API_KEY; echo
read -r -s -p "Binance Spot Testnet secret: " BINANCE_SECRET; export BINANCE_SECRET; echo
cargo run --release -- --symbol BTCUSDT testnet-preflight
```

> Nesta sessão, a consulta pública sem autenticação ao horário retornou HTTP 451 por restrição de localização aplicada à sandbox. Isso não determina a elegibilidade do operador nem prova que sua conta/ambiente terá o mesmo resultado; o preflight autenticado não foi executado.

Somente depois de revisar o resultado do preflight, um exemplo de envio de ordem **à Testnet** (operação externa em ambiente de teste, não simulação local) é:

```bash
cargo run --release -- --symbol BTCUSDT \
  --risk-per-trade 0.01 --max-daily-loss 0.02 --max-allocation 0.25 \
  testnet-place-market-order --side buy --quantity 0.0001 \
  --confirm-testnet-order BINANCE-SPOT-TESTNET
```

O motor de risco limita **entradas de compra**; o tamanho é validado com preço de referência, saldo livre, taxas/slippage configurados, kill-switch, âncora diária UTC e filtros `LOT_SIZE`/`MARKET_LOT_SIZE`/notional da exchange. Os limites e a âncora são específicos de BTCUSDT, não agregam outros ativos/mercados. Venda manual serve para reduzir posição. O subcomando não acompanha estratégia, não instala stop na venue e não é um bot autônomo de produção. Estado de ordens e âncora diária ficam em `data/testnet-orders.sqlite3` por padrão.

Se a resposta do POST ou a consulta ficar incerta, **não execute o comando de ordem novamente**. Use o `client_order_id` mostrado no resultado para reconciliar:

```bash
cargo run --release -- --symbol BTCUSDT \
  testnet-reconcile-order --client-order-id "SEU_CLIENT_ORDER_ID"
```

O journal grava a intenção antes do POST, impede reenvio idempotente, bloqueia novas ordens no mesmo símbolo enquanto houver estado `pending/open/partially_filled/unknown` e serializa essa exclusão mútua no SQLite. **Não apague nem troque o arquivo do journal** enquanto houver ordem pendente/incerta; isso removeria a proteção local contra duplicação. O cliente não faz retry automático. O desenvolvimento foi validado com mocks; nenhuma ordem Testnet ou real foi enviada durante esta implementação. Veja o [escopo e as limitações da Fase 5](docs/phase-5-binance-testnet-scope.md).

## Fase 6 — contratos de segurança e execução

O domínio agora valida a forma de cada `TradeSignal` antes do sizing, incluindo IDs, probabilidade, slippage e janela temporal. O motor de risco transforma sinais malformados em rejeições determinísticas e continua independente de qualquer IA. O crate de execução expõe um gate reutilizável que bloqueia novas ordens quando o kill-switch, o estado do mercado, o estado da conta ou a reconciliação não são confiáveis. O caminho manual da Spot Testnet usa esse gate e mantém a verificação final do kill-switch imediatamente antes do POST.

Esta fase também registra a prontidão real do projeto em [`CHECKLIST.md`](CHECKLIST.md), a política de segurança em [`SECURITY.md`](SECURITY.md), o [modelo de ameaças](docs/threat-model.md), a [política de risco](docs/risk-policy.md) e o [ciclo de execução](docs/execution-lifecycle.md). Ela não adiciona produção, não habilita novas venues e não declara estratégias futuras como implementadas.

## Requisito 28 — divergência preço/RSI (somente backtest)

O detector usa pivôs estritos e RSI Wilder conhecido até o candle do pivô; só confirma depois do fechamento de `pivot_right_bars`. Empates não contam, lower-low/higher-low gera candidato bullish e higher-high/lower-high gera bearish. A saída não inclui probabilidade inventada; o rompimento do extremo do segundo pivô marca o setup como invalidado. A configuração em [`configs/strategies.example.toml`](configs/strategies.example.toml) vem com `enabled = false`.

Para selecioná-la explicitamente:

```bash
cargo run --release -- \
  --backtest-csv data/BTCUSDT-1m.csv \
  --backtest-strategy price-rsi-divergence \
  --strategy-config configs/strategies.example.toml \
  --backtest-report data/divergence-report.json
```

Edite `enabled = true` na seção `[price_rsi_divergence]` somente para essa análise. O seletor só é aceito junto de `--backtest-csv`; o loop paper permanece conectado à EMA 9/21. O relatório registra ID, versão algorítmica e parâmetros serializados. Esta branch não contém walk-forward nem fingerprint do dataset; portanto a integração completa do requisito 28 continua pendente e os resultados do holdout não demonstram edge ou rentabilidade futura. Veja [`docs/BATCH_28_37_STATUS.md`](docs/BATCH_28_37_STATUS.md).

## ⚠️ Avisos importantes

- **Paper trading / testnet primeiro. Sempre.**
- Nunca faça commit de API keys (`.env` já está no `.gitignore`).
- Use keys com **permissão de saque desabilitada**.
- LLMs podem errar, alucinar dados ou interpretar mal instruções. Mantenha supervisão humana.
- Verifique a legislação do seu país (ex.: Polymarket tem restrições geográficas).
- O bot Rust atual é um MVP educacional: o backtest é um holdout simples, sem otimização ou inferência estatística; não há execução em produção nem promessa de lucro. A execução de exchange disponível é manual e restrita à Binance Spot Testnet. A carteira paper e o cursor de candles são persistidos no SQLite e restaurados após reinício. O dashboard é local e não envia notificações externas.
- Resultados passados ou simulados não garantem resultados futuros.

## 📜 Créditos

Criado com **Grok** + **Lovable**. Repositórios de terceiros pertencem aos respectivos autores.
