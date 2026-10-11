# Roadmap — Olho de Deus

## Fase 1 — bot paper funcional (MVP)

- [x] Workspace/binário Rust estável
- [x] Feed público de candles Binance Spot, validação de candle fechado e bloqueio de dado obsoleto
- [x] Estratégia EMA 9/21 para demonstração
- [x] Executor simulado, sizing por risco, teto de alocação, stop, custos e limite diário
- [x] Auditoria JSONL local e testes unitários
- [x] CI: formatação, testes e Clippy
- [ ] Persistir carteira e estado para recuperação após reinício
- [ ] Backtest/replay sem look-ahead, com custos parametrizados

## Fase 2 — domínio e governança de risco

- [ ] Tipos de domínio e eventos separados em crates
- [ ] Políticas de risco por ativo/estratégia e kill-switch persistente
- [ ] Testes de propriedade, falhas, idempotência e reconciliação

## Fase 3 — dados e estratégia

- [ ] Histórico reproduzível, qualidade/latência de feed e métricas
- [ ] Estratégias independentes, com métricas e interruptores próprios
- [ ] Validar contribuição incremental fora da amostra

## Fase 4 — observabilidade e IA

- [ ] Dashboard, alertas, auditoria persistente e autenticação
- [ ] Adaptadores de modelos com permissões limitadas; IA nunca controla os gates de risco

## Fase 5 — integração de execução (não implementada)

- [ ] Definir exchange/mercado, jurisdição, políticas e ambiente de teste
- [ ] Adaptador em sandbox/testnet, ciclo completo de ordem e reconciliação
- [ ] Revisão de segurança e autorização humana explícita antes de qualquer execução real

**Nenhuma fase futura é considerada concluída apenas por estar documentada. A execução real permanece fora do escopo do MVP e não tem caminho de ativação no binário atual.**
