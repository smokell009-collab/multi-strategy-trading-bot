# Roadmap — Olho de Deus

## Fase 1 — bot paper funcional (MVP)

- [x] Workspace/binário Rust estável.
- [x] Feed público de candles Binance Spot, filtro de candle fechado e bloqueio de dado atrasado.
- [x] Estratégia EMA 9/21 e executor paper com sizing, teto de exposição, stop, custos e bloqueio por perda.
- [x] Auditoria JSONL, testes e CI de formato, workspace e Clippy.
- [x] Replay histórico básico sem look-ahead, consolidado no crate de backtest da Fase 3.
- [ ] Persistir carteira e estado para recuperação após reinício.

## Fase 2 — domínio e governança de risco

- [x] Tipos de domínio e eventos (`ode-domain`, `ode-events`).
- [x] Motor `ode-risk` com política por estratégia/mercado; overrides não afrouxam a base.
- [x] Kill-switch persistente, atômico e fail-closed.
- [x] Contratos de idempotência/reconciliação em `ode-execution`, com testes.
- [x] Testes de propriedade para sizing e testes de falha, latch e dados inválidos.
- [x] CI executa todos os crates.

**Limite:** `ode-execution` ainda não possui adaptador de exchange e não envia ordens reais.

## Fase 3 — dados e estratégia

- [x] Histórico OHLCV reproduzível por CSV, com validação de preços, OHLC, ordenação e cadência; latência p50/p95/máxima quando o horário original de recebimento existe.
- [x] EMA extraída para módulo independente, parâmetros e switch próprios; a avaliação é pura e usa apenas candles fechados.
- [x] Métricas por execução/estratégia e holdout cronológico OOS com custos, risco, drawdown e benchmark buy-and-hold.

**Limite:** há uma estratégia (EMA) e uma divisão in-sample/OOS fixa. Não há otimização, walk-forward rolling ou comprovação estatística de edge; OOS só é independente se não for usado para escolher os parâmetros.

## Fase 4 — observabilidade e IA

- [ ] Dashboard, alertas, auditoria persistente consultável e autenticação.
- [ ] Adaptadores de modelos com permissões limitadas; IA nunca controla os gates de risco.

## Fase 5 — integração de execução (não implementada)

- [ ] Definir exchange/mercado, jurisdição, políticas e ambiente de teste.
- [ ] Adaptador em sandbox/testnet, ciclo completo de ordem e reconciliação.
- [ ] Revisão de segurança e autorização humana explícita antes de qualquer execução real.

**Nenhuma fase futura é considerada concluída apenas por estar documentada. Execução real continua fora do escopo e não tem caminho de ativação no binário.**
