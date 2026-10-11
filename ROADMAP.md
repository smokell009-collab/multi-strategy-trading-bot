# Roadmap — Olho de Deus

## Fase 1 — bot paper funcional (MVP)

- [x] Workspace/binário Rust estável.
- [x] Feed público de candles Binance Spot, filtro de candle fechado e bloqueio de dado atrasado.
- [x] Estratégia EMA 9/21 e executor paper com sizing, teto de exposição, stop, custos e bloqueio por perda.
- [x] Auditoria JSONL, testes e CI de formato, workspace e Clippy.
- [x] Replay histórico básico sem look-ahead, consolidado no crate de backtest da Fase 3.
- [x] Persistir carteira paper, posição/custos e cursor de candle para recuperação após reinício; snapshot versionado e fail-closed.

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

- [x] Dashboard local autenticado em loopback, estado paper/saúde/latência, alertas deduplicados e reconhecíveis.
- [x] Auditoria persistente e consultável em SQLite com JSONL mantido para compatibilidade.
- [x] Adaptador OpenAI-compatible opt-in, acionado manualmente, sem tool calling e com contexto público tipado; sem dependência de `ode-risk`/`ode-execution`.

**Limites:** o painel não é exposto à rede e não envia push/webhook; polling do navegador é local. O adaptador retorna apenas comentário textual e não controla sinais, risco ou execução. Execução real permanece fora de escopo.

## Fase 5 — integração de execução (não implementada)

- [x] Selecionar Binance Spot Testnet/BTCUSDT como alvo de desenvolvimento; jurisdição do operador permanece não informada e não é considerada validada.
- [ ] Implementar e testar políticas de execução específicas do ambiente de teste.
- [ ] Adaptador em sandbox/testnet, ciclo completo de ordem e reconciliação.
- [ ] Revisão de segurança e autorização humana explícita antes de qualquer execução real.

**Nenhuma fase futura é considerada concluída apenas por estar documentada. Execução real continua fora do escopo e não tem caminho de ativação no binário.**
