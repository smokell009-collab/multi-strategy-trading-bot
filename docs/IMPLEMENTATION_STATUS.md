# Estado de implementação verificado

Atualizado em 2026-10-11. A auditoria READ-ONLY inicial inspecionou `main` no baseline `539cbfcb91517c8dca215dfb6cb3d88f9d923a77`; veja [`audit-initial-2026-10-11.md`](audit-initial-2026-10-11.md). O relatório descreve aquele baseline, não presume trabalho ou chamadas externas que ocorreram depois.

## Capacidades do baseline auditado

O workspace tinha domínio e validação estrutural, parsing OHLCV, EMA 9/21 no loop paper Binance, holdout IS/OOS, motor de risco fail-closed, kill-switch, journal/idempotência para o fluxo manual Binance Spot Testnet, carteira paper persistente, dashboard local autenticado e comentário IA opcional/read-only. A auditoria não executou testes, chamada externa, autenticação, ordem Testnet ou trading real naquele momento.

## Alterações verificadas depois do baseline

**Requisito 28:** divergência preço/RSI causal com pivôs estritos, confirmação atrasada e invalidação; disponível no backtest principal. O requisito continua parcial por não haver walk-forward/rolling nem fingerprint de dataset. Além disso, o binário Polymarket dedicado permite a estratégia em paper sobre preço de outcome, sem mudar a seleção/defaults do paper Binance.

**Fase B1:** `MarketId::canonicalized()` usa trim + uppercase ASCII e rejeita identidade vazia. `RiskEngine` compara IDs canônicos sem colapsar venues diferentes; overrides são canonicalizados e colisões rejeitadas. B1 continua parcial: identidade venue/símbolo/unidade tipada, proveniência temporal e qualidade derivada até risco seguem em aberto.

**Fase C — Polymarket:** `ode-polymarket` lê Gamma, CLOB REST, Data API e market WebSocket públicos. `polymarket-watch` recebe um slug/outcome explícito e pode observar ou simular paper pUSD, reaproveitando estratégias, risco, kill-switch e SQLite. As duas estratégias rodam em contas separadas. O adaptador não implementa autenticação do usuário, signer, carteira, user channel nem ordem Polymarket. Os EUA foram informados como região; a documentação consultada classifica o serviço como close-only e o escopo permanece read-only/paper.

## Validação observada nesta implementação

- `cargo fmt --all -- --check`: passou.
- `cargo test --workspace --locked`: **79 testes passaram, 0 falhas**.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`: passou sem warnings.
- `git diff --check`: passou.
- Smoke `polymarket-watch --once`: Gamma e order book público retornaram dados; modo reportou `read-only` e `live_order_sent=false`.
- Smoke contínuo de 12 segundos: feed WebSocket conectou e publicou top-of-book; SQLite temporário persistiu duas contas paper independentes. Nenhum fill foi alegado e nenhuma ordem enviada.

Os testes externos rodaram no egress do sandbox, não no dispositivo/rede do usuário nos EUA. A validação não mede edge, performance, execução, resolução de mercado ou elegibilidade da conta.

## Restrições mantidas

Paper continua distinto de qualquer execução. O paper Binance segue EMA; seu caminho Testnet manual permanece separado. Polymarket é somente dados públicos e fills simulados locais, em pUSD. A taxa é simulada apenas quando o `feeSchedule` é suportado; desconhecida/incompatível bloqueia paper fills. As barras históricas Polymarket são OHLC sintético derivado de observações pontuais e não representam candles de BTC/ETH spot. Não há evidência de rentabilidade nem caminho de produção/live.
