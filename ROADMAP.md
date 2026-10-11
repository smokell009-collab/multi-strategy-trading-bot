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

## Fase 6 — contratos de segurança e execução

- [x] Validar estruturalmente sinais antes do sizing, sem depender de LLM.
- [x] Centralizar helpers para estados terminais e `Unknown` que exige reconciliação.
- [x] Expor gate determinístico que bloqueia kill-switch, mercado/conta incertos e reconciliação pendente.
- [x] Integrar o gate ao fluxo manual da Binance Spot Testnet sem ampliar o escopo de venue.
- [x] Documentar checklist de prontidão, segurança, ameaças, política de risco e ciclo de execução.
- [ ] Implementar reconciliador agregado de posições com fixtures de reinicialização, divergência e cancelamento seguro.

**Fase 6 reforça contratos e governança; não adiciona estratégias, não envia ordens durante os testes e não cria caminho de produção.** Critérios e evidências: [`CHECKLIST.md`](CHECKLIST.md) e [`docs/phase-6-safety-contracts.md`](docs/phase-6-safety-contracts.md).

## Lote 28–37 — primeira fatia do requisito 28

- [x] Núcleo de divergência preço/RSI com pivôs estritos e confirmação atrasada no crate técnico.
- [x] Configuração TOML `price_rsi_divergence` desativada por padrão.
- [x] Seletor exclusivo do backtest; o modo paper continua usando apenas EMA.
- [x] Testes de bullish/bearish, histórico insuficiente, dados inválidos, empate, look-ahead, invalidação e não repetição.
- [x] O relatório grava ID, versão e parâmetros da estratégia selecionada.
- [ ] Integração com walk-forward: infraestrutura não localizada nesta branch e não será simulada como concluída.

Estado e decisões reais: [`docs/BATCH_28_37_STATUS.md`](docs/BATCH_28_37_STATUS.md), [`docs/IMPLEMENTATION_STATUS.md`](docs/IMPLEMENTATION_STATUS.md) e [`docs/ARCHITECTURE_DECISIONS.md`](docs/ARCHITECTURE_DECISIONS.md). Este requisito permanece parcial até validação completa e integração walk-forward.

## Fase A — auditoria inicial do sistema

- [x] Mapear crates, fluxos paper/backtest/Testnet/dashboard e classificar funcionalidades completas, parciais, ausentes e integrações sem confundir mocks com serviços externos.
- [x] Inventariar riscos e testes existentes sem afirmar que foram executados durante a auditoria.
- [x] Registrar lacunas e plano incremental em [`docs/audit-initial-2026-10-11.md`](docs/audit-initial-2026-10-11.md).

## Fase B1 — identidade canônica mínima de mercado

- [x] Normalizar `MarketId` na fronteira de risco por trim e ASCII uppercase; rejeitar vazio.
- [x] Comparar signal/snapshot sem colapsar venues qualificadas diferentes.
- [x] Aplicar overrides por chave de mercado canônica e cobrir os casos com testes.
- [ ] Definir identidade tipada completa venue/símbolo/unidade; separar source/receive timestamps; tornar estrita a validação temporal OHLCV; registrar gaps e propagar qualidade derivada até o risco.

Escopo e limitações do incremento: [`docs/phase-b1-market-identity.md`](docs/phase-b1-market-identity.md). B1 não cria adapters, aliases, order book, integração externa ou negociação real.

## Fase C — Polymarket read-only e paper em pUSD

- [x] Adapter público Gamma/CLOB/Data API e market WebSocket: metadados, IDs V1/V2 por outcome, book snapshots/deltas, prints e histórico.
- [x] Binário `polymarket-watch` separado do fluxo Binance; market slug e outcome selecionados explicitamente.
- [x] Reutilizar EMA e divergência preço/RSI, motor de risco, kill-switch e auditoria SQLite em carteiras paper pUSD separadas.
- [x] Simulação de fills por profundidade/taxas conhecidas; bloquear novos fills quando fee schedule não é suportada.
- [ ] Validar acessibilidade no dispositivo/rede/região do operador e avaliar a qualidade de sinais com dados históricos independentes; nenhum edge está comprovado.
- [ ] Qualquer escopo além de leitura/paper exigiria nova revisão de jurisdição e autorização; não há execução Polymarket implementada.

**Limite:** candles técnicos são aproximações do preço de probability/outcome do Polymarket, não preços OHLC spot de BTC/ETH. Fills não modelam prioridade de fila/latência, não há probabilidade independente e não há ordem assinada. A jurisdição informada é EUA e permanece close-only segundo a documentação consultada; não se habilita nova posição.
