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

**Limite:** nesta fase, `ode-execution` define contratos; o adaptador posterior só pode alcançar Spot Testnet, nunca a produção.

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

## Fase 5 — execução restrita à Binance Spot Testnet

- [x] Selecionar Binance Spot Testnet/BTCUSDT como alvo de desenvolvimento; jurisdição do operador permanece não informada e não é considerada validada.
- [x] Cliente REST HMAC com host de Testnet fixo, relógio sincronizado, HTTPS obrigatório, sem redirects e sem possibilidade de trocar para produção.
- [x] Validar permissões da conta, estado do símbolo e filtros de quantidade/notional antes de permitir uma ordem.
- [x] `ode-execution` persiste intenção antes do POST, protege a idempotência, serializa uma ordem ativa por símbolo e guarda a âncora de risco diária UTC.
- [x] Comandos explícitos de preflight somente leitura, ordem MARKET manual com confirmação textual, consulta/reconciliação e nenhuma repetição automática após resultado incerto.
- [ ] Revisão de segurança e autorização humana antes de qualquer consideração de execução em produção; produção não faz parte desta fase.

**Fase 5 implementa apenas ordens de teste. Execução real continua fora do escopo e não existe host, flag ou caminho de ativação de produção no binário. A jurisdição do operador segue não informada.**
