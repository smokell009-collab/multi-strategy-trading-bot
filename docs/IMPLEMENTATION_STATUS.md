# Estado de implementação verificado

Atualizado em 2026-10-11. A auditoria inicial READ-ONLY foi feita sobre `main` no baseline `539cbfcb91517c8dca215dfb6cb3d88f9d923a77`; veja [`audit-initial-2026-10-11.md`](audit-initial-2026-10-11.md). O baseline não incluiu execução de testes ou chamadas externas. Este status distingue esse inventário do trabalho de acompanhamento B1 descrito abaixo.

## Capacidades observadas no baseline

O workspace Rust contém domínio e validação estrutural de sinais, parsing/validação OHLCV básica, EMA 9/21 no loop paper, backtest de holdout IS/OOS, motor de risco fail-closed, kill-switch persistente, journal SQLite/idempotência para o fluxo manual Binance Spot Testnet, paper broker persistente, dashboard local autenticado e comentário de IA opcional/read-only. Há leitura pública de klines Binance no caminho paper e um cliente separado de Binance Spot Testnet.

A presença desses caminhos no código não prova conectividade externa. No momento da auditoria não foram verificados preflight, autenticação em serviço externo, chamadas a provedores, ordens Testnet ou trading real. Não foi encontrado caminho de produção/live no escopo inspecionado.

Lacunas prioritárias no baseline: normalização canônica de market ID; timestamps/provenance/qualidade de dados ligados ao risco; order book/microestrutura; reconciliação portfolio-wide; testes de crash/timeout/concorrência; walk-forward; fingerprints/schema de relatórios; métricas operacionais e integrações Alpaca/CCXT/Polymarket/TradingView funcionais. Guias e mocks não são tratados como adapters ou integrações verificadas.

## Requisito 28 — estado conhecido

A divergência regular preço/RSI foi implementada e publicada anteriormente em `main`, com configuração off por padrão, pivôs estritos, RSI Wilder causal e seleção somente pelo backtest. EMA continua sendo a única estratégia do loop paper. A estratégia continua **parcial** porque não há walk-forward/rolling, fingerprint de dataset nem schema version de relatório. Não há evidência financeira de rentabilidade.

## Follow-up da Fase B — B1 identidade mínima de mercado

Implementado neste trabalho: `MarketId::canonicalized()` aplica trim e uppercase ASCII e rejeita vazio; `RiskEngine` compara o ID canonicalizado do sinal com o snapshot sem colapsar venues qualificadas distintas; chaves TOML de overrides de mercado também são canonicalizadas e colisões são rejeitadas. IDs bare como `BTCUSDT` continuam aceitos por compatibilidade, sem inferência de venue.

**Validação local observada:** `cargo fmt --all -- --check`; suíte completa `cargo test --workspace --locked --quiet` — 70 testes aprovados; `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`; `git diff --check`. O teste de integração externa não foi executado. B1 é apenas o primeiro incremento e não completa a Fase B.

Próximas etapas ainda abertas: identidade tipada venue/símbolo/unidade, source/receive timestamps, validação temporal estrita de candles, relatório de gaps e derivação/propagação obrigatória da qualidade até o `RiskEngine`; depois, adapter de dados/order book em fase própria.

## Restrições

Paper continua padrão; a estratégia de divergência continua backtest-only. Binance Testnet continua um caminho manual e separado; nenhuma operação externa foi executada nesta implementação B1. Trading real permanece ausente/não habilitado. Nenhum LLM controla cálculo da estratégia, limites de risco ou execução.
