# Relatório de auditoria — Fase A

**Data do relatório:** 2026-10-11
**Baseline auditado:** `539cbfcb91517c8dca215dfb6cb3d88f9d923a77` (`539cbfc`)
**Branch informada:** `main`
**Modo:** auditoria READ-ONLY, baseada exclusivamente nos resultados JSON fornecidos.

> **Escopo e ressalva:** este relatório consolida evidências de código, inventário de testes e documentação citados nos resultados recebidos. Nenhum teste, comando de CI, operação externa, preflight, chamada de exchange, chamada de provedor de IA ou ordem foi executado nesta auditoria. O relatório **não mede rentabilidade**, não comprova edge, não valida desempenho futuro e não autoriza inferir trading real.

## 1. Conclusão executiva

O baseline contém um núcleo local funcional de **paper trading**, contratos de domínio, validação básica de OHLCV, estratégias técnicas causais, backtest IS/OOS simples, gates de risco, persistência SQLite/JSONL, dashboard local e uma fronteira de IA somente consultiva. Também há um cliente REST/HMAC separado para **Binance Spot Testnet**, acionável somente por comandos manuais explicitamente delimitados.

As principais limitações são de **normalização e qualidade de dados**, **microestrutura**, **reconciliação de execução**, **robustez metodológica do backtest**, **observabilidade operacional** e **ausência deliberada de produção/live**. A existência de código Testnet, mocks locais ou documentação não constitui evidência de conectividade, autenticação ou ordem executada em serviço externo.

A ordem recomendada é primeiro consolidar contratos de dados e qualidade, depois integrar order book/adapters, fortalecer reconciliação e execução, evoluir o backtest, fechar observabilidade e somente então realizar exercícios externos controlados em Spot Testnet. A produção/live deve permanecer ausente até decisão e validação independentes; nada no baseline demonstra que ela exista ou esteja ativada.

## 2. Método, evidência e falhas dos agentes

- Os resultados recebidos descrevem auditorias READ-ONLY do baseline acima; em todos os blocos foi informado working tree limpo ou sem alterações.
- `failures` no conjunto consolidado veio vazio: **nenhuma falha de agente foi registrada**.
- Isso não significa que as lacunas técnicas estejam resolvidas. As lacunas abaixo são mantidas explicitamente como não verificadas ou ausentes; não foram preenchidas por conjectura.
- Documentação, nomes de testes e configuração de CI foram tratados como **evidência de existência declarada**, não como execução ou validação de runtime.
- O relatório distingue três níveis: **implementado no código**, **coberto por teste/mock encontrado** e **verificado externamente em execução**. Para o último nível, não foi encontrada evidência.

## 3. Mapa dos crates e fluxos

### 3.1 Mapa por crate e componente

| Área | Componentes observados | Papel no sistema | Limite principal observado |
|---|---|---|---|
| Domínio | `crates/ode-domain` | Wrappers de `MarketId`/`StrategyId`, `Candle`, `DataQuality`, `TradeSignal`, snapshots de mercado/carteira, estado paper e estados de ordem | Não há construtores/validação completa de `Candle`, tipo de order book ou identidade canônica de venue/símbolo |
| Eventos | `crates/ode-events` | `AuditEvent` com UUID v4, timestamp UTC RFC3339 e `details` JSON | Não há schema versionado, validação estrita, desserialização contratual ou `deny_unknown_fields` |
| Market data | `crates/ode-market-data` | Importação CSV OHLCV, parsing Decimal, validação de OHLC, ordem/cadência e percentis de latência | Sem adapter multi-venue, order book, recuperação de gaps ou propagação automática de qualidade |
| Estratégias | `crates/ode-strategy-technical` | EMA crossover e divergência preço-RSI com pivôs estritos e RSI Wilder | Divergência somente regular; sem composição/ensemble, estado persistente ou integração paper/live |
| Backtest | `crates/ode-backtest` | Split IS/OOS cronológico, warm-up, fills no próximo open, custos, sizing, stops e métricas | Sem walk-forward, rolling, regimes, significância, fingerprint de dataset/config ou short |
| Risco | `crates/ode-risk` | `RiskPolicy`, `RiskLimits`, `RiskEngine::authorize_entry`, kill-switch e sizing | Qualidade pode ser marcada `Valid` sem derivação do relatório; kill-switch bloqueia novas entradas BUY, não toda atividade |
| Execução | `crates/ode-execution` | `ExecutionSafety`, `OrderLedger`, `SqliteOrderJournal`, idempotência e guarda por mercado | Ledger em memória não integrado ao journal; reconciliação ainda não é portfolio-wide |
| Binance Testnet | `crates/ode-binance-testnet` | REST Spot Testnet, HMAC, tempo, conta, filtros, preço, ordem MARKET, consulta e cancelamento | Escopo somente Spot Testnet; mock local não prova exchange externa; cancelamento não tem comando CLI observado |
| Operações Testnet | `src/testnet_ops.rs` | Preflight, confirmação literal, ordem MARKET manual, reconciliação e bloqueios | Não é caminho automático de EMA/divergência nem trading real |
| Binário principal | `src/main.rs` | CLI, fetch de klines públicos, loop paper, PaperBroker, backtest CSV, dashboard e despacho Testnet | Feed externo está embutido no binário e não é adapter de `ode-market-data`; não há produção/live |
| Observabilidade | `crates/ode-observability` e `static/*` | SQLite/JSONL, snapshots, alertas, status, dashboard loopback autenticado | Exportação Prometheus/OpenTelemetry, notificações e atomicidade total SQLite/JSONL ausentes |
| IA consultiva | `crates/ode-model-adapter` | Chat completion OpenAI-compatible sem tools, contexto público limitado e resposta textual limitada | Nenhuma chamada externa verificada; resultado não é persistido nem integrado a risco/execução |
| Configuração/CI | `Cargo.toml`, `Cargo.lock`, `configs/*`, `.github/workflows/rust-ci.yml`, `docs/*` | Workspace Rust, exemplos, workflow de fmt/test/clippy e guias | Documentação/status pode estar stale; configurações de exemplos não constituem implementação |

### 3.2 Fluxo paper

1. `src/main.rs` valida argumentos e carrega estratégia/política de risco.
2. Abre `AuditStore` e restaura ou cria o `PaperBroker`.
3. Consulta klines públicas em `https://data-api.binance.vision/api/v3/klines`.
4. Descarta candles ainda abertos, ordena por `open_time_ms` e executa a validação OHLCV/cadência.
5. Aplica freshness de até dois intervalos; o último candle recebe `received_at_ms` derivado de `Utc::now()` após o fetch.
6. O loop padrão instancia `EmaCrossover`; a entrada passa por `RiskEngine`.
7. O `PaperBroker` simula fills, custos, stops, posição e estado diário.
8. Eventos, alertas e snapshot paper são persistidos; o dashboard local expõe status, eventos, alertas e advisory.

**Não há POST de ordem nesse fluxo.** O caminho normal é paper e não foi encontrado indício de que qualquer operação externa tenha sido ativada.

### 3.3 Fluxo backtest

1. `--backtest-csv` abre dados locais em `src/main.rs`.
2. `run_out_of_sample` valida dados/configuração e separa temporalmente IS/OOS, usando prefixo anterior somente para warm-up.
3. A estratégia selecionada pode ser EMA ou divergência preço-RSI.
4. A simulação usa candles disponíveis até o índice atual, preenche no próximo open, aplica risco, custos, slippage, stop e fechamento ao fim do período.
5. O relatório inclui métricas de retorno, trades, rejeições, win rate, expectativa, drawdown, buy-and-hold, diferença relativa, ID, versão, parâmetros e cortes.

O backtest é local e não consulta exchange na rota CSV. Seus resultados não são prova de rentabilidade ou de execução real.

### 3.4 Fluxo Testnet manual

1. A CLI valida escopo, modo e confirmação literal `BINANCE-SPOT-TESTNET`.
2. O cliente conecta ao host fixo `https://testnet.binance.vision/api`, sincroniza `/v3/time` e consulta conta/filtros/preço.
3. `run_market_order` registra a intenção antes do POST, aplica guard/idempotência e realiza nova autorização imediatamente antes da ordem BUY.
4. Resposta incerta é convertida em `Unknown`, bloqueando novas ordens até consulta/reconciliação explícita.
5. `run_reconcile` consulta por `clientOrderId` e atualiza journal conforme status/fill.

Esse fluxo é **manual, de Spot Testnet e não automático por estratégia**. Não há evidência de execução contra a Binance Testnet.

### 3.5 Fluxo de dashboard e IA

- Dashboard é local, autenticado por Bearer, limitado a loopback e sem rota de execução de ordens.
- `/api/advisory` é opt-in, manual, condicionado a feed saudável e snapshot recente, sem tools e sem encaminhamento para estratégia, risco ou execução.
- O adaptador usa `ODE_MODEL_API_KEY`, base padrão `https://api.openai.com/v1` ou `ODE_MODEL_BASE_URL`; a política exige HTTPS fora de loopback e desabilita redirects.
- O resultado é truncado/limpo como texto; somente metadados `provider/model/text_chars` são auditados.

## 4. Funcionalidades completas no limite auditado

“Completa” aqui significa funcionalidade observável no código dentro do escopo, não integração externa nem garantia operacional ampla.

### 4.1 Domínio, dados e qualidade básica

- `TradeSignal::validate_shape` cobre o contrato documentado: IDs não vazios após trim, probabilidade entre 0 e 1, slippage até 10000 bps, timestamps não negativos e expiry não anterior ao sinal.
- A separação entre forma e freshness é explícita: idade relativa ao relógio é gate externo.
- Importação CSV OHLCV funciona para séries não vazias, positivas, ordenadas e com intervalo fixo.
- `validate_candles` verifica timestamps básicos, OHLC positivo/coerente, volume não negativo, ordem/cadência e percentis de latência quando há `received_at_ms`.
- `OrderStatus::is_terminal` e `requires_reconciliation` distinguem estados terminais de `Unknown` que exige reconciliação.
- `AuditEvent::new` gera ID único e timestamp UTC local; `AuditStore` persiste eventos.

### 4.2 Estratégias e backtest

- EMA crossover é causal no caminho implementado: usa somente o slice recebido e é chamado em prefixos de candles fechados; o fill ocorre na abertura seguinte.
- Divergência preço-RSI regular usa RSI Wilder causal, pivôs confirmados por barras à direita, lower-low/higher-low bullish, higher-high/lower-high bearish, rejeição de empate e emissão única após confirmação.
- Configuração TOML usa defaults, `deny_unknown_fields` e validação de períodos/estratégias.
- Backtest possui split IS/OOS único, warm-up, sizing por risco, stop, taxas/slippage, fechamento no fim e autorização de entradas pelo risco.
- Métricas básicas e relatório estruturado existem para o recorte simulado.

### 4.3 Risco, execução local e paper

- `RiskEngine::authorize_entry` encadeia kill-switch, shape, direção/mercado, qualidade, idade/futuro, timestamps/expiração, edge, slippage, posição, preço/equity/carteira, perda diária, sizing, alocação e caixa.
- `KillSwitch::activate` usa arquivo temporário, `create_new`, sync de arquivo/diretório e rename atômico; estado inválido falha fechado.
- `SqliteOrderJournal` persiste intenção antes do POST, impõe uma ordem não terminal por mercado, atualiza fills monotonicamente, rejeita downgrade terminal e conserva âncora diária UTC.
- PaperBroker restaura estado, posição, cursor e saldo sem sobrescrever o persistido; eventos de fills e rejeições são auditados.
- Dashboard local tem autenticação, loopback, headers de segurança/CSP/no-store e renderização textual segura.

### 4.4 Integrações implementadas em código, com escopo limitado

- Leitura de klines públicos da Binance no loop paper.
- Cliente separado de Binance Spot Testnet com HMAC, sincronização de relógio, account/exchangeInfo/avgPrice/order/query/cancel e filtros.
- Adaptador OpenAI-compatible somente consultivo, sem tools e sem encaminhamento a execução.

Essas três capacidades são completas **como caminhos de código dentro do escopo**, mas nenhuma foi verificada por execução externa nesta auditoria.

## 5. Funcionalidades parciais

### 5.1 Identidade, normalização e tempo

- `Trim::All` e `Decimal::from_str` fazem normalização sintática, mas `MarketId`/`StrategyId` preservam diferenças de whitespace, caixa e aliases.
- Não existe política canônica de venue, símbolo, moeda, unidade, escala, timezone/epoch ou provenance.
- `Candle` só exige `open_time_ms >= 0` e `close_time_ms >= open_time_ms`; não valida duração, alinhamento ao intervalo, futuro/clock skew ou monotonicidade de recebimento.
- No live fetch, somente o último candle recebe `received_at_ms=now`; a latência calculada é idade do último fechamento no instante da consulta, não latência de rede/recebimento de cada barra.

### 5.2 Qualidade e gaps

- `DataQualityReport` expõe latências, mas `gap_count` permanece zero e qualquer lacuna/cadência irregular causa erro imediato.
- Não há produtor em `ode-market-data` para `Delayed`, `Invalid` ou `Inconsistent`.
- Consumidores constroem `MarketSnapshot` com `DataQuality::Valid` sem derivá-lo obrigatoriamente do relatório, provenance ou profundidade do feed.
- A qualidade detectada não é propagada automaticamente até `RiskEngine`; uma validação que rejeita gaps não deixa um relatório de lacunas recuperável.

### 5.3 Estratégia e backtest

- Divergência só é regular e compara pivôs consecutivos; não há divergência oculta, matching não consecutivo, score/confiança ou probabilidade.
- Invalidação de divergência é diagnóstica: `analyze` pode informar `Invalidated`, mas `evaluate` retorna `Hold`; a simulação não transforma o rompimento do segundo pivô em saída automática.
- Backtest é uma divisão IS/OOS única com parâmetros fixos; não há walk-forward/rolling, regimes, treino/otimização ou significância.
- EMA usa o primeiro close como seed, sem bootstrap SMA nem teste golden de valores numéricos.
- O sistema é long-only; `Sell` fecha posição e não abre short.
- Métricas não incluem exposição, turnover, duração, volatilidade/Sharpe, custos separados ou curva intrabar completa; drawdown usa marcação em closes e stops.
- Relatório tem proveniência mínima: sem fingerprint SHA-256 de dataset/config e sem schema version.

### 5.4 Risco, journal e execução

- Reconcilição é parcial: há consulta por `clientOrderId`, mas não há comparação portfolio-wide de ordens locais contra snapshot/lista da venue, nem barreira persistente para divergência de `exchange_order_id` já armazenado.
- `OrderLedger` em memória tem garantias diferentes e não está integrado ao journal SQLite; `update_local` não impõe toda a monotonicidade/terminalidade.
- `decode_order` não revalida todas as invariantes semânticas de registros persistidos; a proteção depende em parte do chamador.
- Kill-switch é gate de novas entradas BUY; SELL pode seguir e há janela entre rechecagem e POST.
- `ExecutionSafety` no fluxo Testnet recebe estados conhecidos/sem reconciliação pendente após chamadas específicas, mas não implementa reconciliação agregada da conta.
- Edge está presente, porém sinais EMA/pedido manual observados fixam `expected_net_edge_bps=0`; override com edge mínimo positivo bloqueia essas entradas e não há cálculo de edge líquido pela estratégia.
- Há incompatibilidade efetiva para intervalo `1d`: CLI aceita o intervalo, enquanto `risk_limits` calcula idade máxima de dois dias e `RiskLimits::validate` rejeita valor acima de um dia.
- Não foram encontrados testes de concorrência multi-processo, crash entre intent e POST ou timeout após ordem potencialmente criada.

### 5.5 Observabilidade e IA

- `commit_paper_event` combina evento/alerta/snapshot em transação, mas `append_event` grava evento e depois chama `raise_alert` em outra conexão; podem divergir em falha intermediária.
- SQLite e JSONL não são atômicos entre si; JSONL é fixo em `data/audit.jsonl`, embora `--audit-db` seja configurável.
- Métricas live normalmente têm uma amostra local; o painel não exibe todos os campos de qualidade/latência.
- Há somente quatro categorias de alertas, sem notificações externas, resolução manual geral, charts ou filtros interativos implementados na UI.
- O adaptador de IA é funcional como fronteira, mas não há mock HTTP do provedor, validação semântica de saída ou persistência do texto advisory.
- O endpoint HTTPS configurável pode apontar para host incorreto; a política valida transporte, não allowlist de provedor.

### 5.6 Documentação, adapters e CI

- Guias de Alpaca, CCXT/Bybit, Polymarket e TradingView, URLs, exemplos e scripts não constituem adapters funcionais nem prova de chamadas.
- Há inconsistência documental sobre TradingView oficial versus MCP community.
- `docs/IMPLEMENTATION_STATUS.md` referencia commit histórico diferente do baseline e afirma contagens/status não usados como prova.
- CI declara fmt, `cargo test --workspace --locked` e Clippy; nenhum foi executado nesta auditoria. O workflow não faz testes externos, Testnet, MCP ou setup guides.

## 6. Funcionalidades ausentes

### 6.1 Dados e mercado

- Modelo/parser/validador de order book: bids/asks, níveis Decimal, sequência, snapshots/deltas, crossed book, checksum, gaps/out-of-order, recuperação e profundidade.
- Contrato canônico de normalização com venue, símbolo, unidade, escala decimal, timezone/epoch, source timestamp, receive timestamp e provenance.
- Validação estrita de duração/alinhamento/futuro/clock skew e qualidade por barra.
- Relatório de gaps que registre lacunas sem ocultá-las e recuperação de gaps.
- Adapter normalizado de market data para múltiplas venues dentro de `ode-market-data`.
- Fixtures para fetch HTTP, timestamps negativos/futuros, received_at semântico, duração inválida, normalização, qualidade derivada e order book.

### 6.2 Estratégia e pesquisa local

- Walk-forward/rolling e avaliação em múltiplos regimes.
- Short, contabilidade de short e semântica de saída por invalidação.
- Métricas estatísticas/robustez, intervalos de incerteza e controle de múltiplas tentativas.
- Fingerprint SHA-256 de dataset/config, schema version e proveniência completa no relatório.
- Valores golden determinísticos de EMA/RSI contra séries-ouro e contrato HTTP/CSV com fixtures mais amplas.
- Integração de divergência ao paper/live; atualmente ela é backtest-only.

### 6.3 Execução e integração externa

- Reconciliação portfolio-wide/por conta, incluindo ordens ausentes, desconhecidas, divergentes e `exchange_order_id` reutilizado/trocado.
- Barreira persistente antes de novas ordens quando houver terminalidade divergente ou estado desconhecido.
- Testes reais de concorrência, crash, restart e timeout após POST.
- Integração de `OrderLedger` ao journal persistente ou remoção da API não usada.
- Comando CLI de cancelamento e stop-loss remoto; embora `cancel_order` exista na biblioteca, não há subcomando observado.
- Qualquer host, credencial, flag ou caminho de produção/mainnet.
- Adapters funcionais para Alpaca, CCXT/Bybit, Polymarket ou TradingView dentro do repositório.
- Integração automática de EMA/divergência com ordens.

### 6.4 Observabilidade e IA

- Prometheus/OpenTelemetry, séries históricas, uptime, erros e latência da própria API/IA.
- Exportação/notificações externas, escalonamento e filtros/charts interativos.
- Persistência consultável do texto/resultado de advisory.
- Integração da IA com estratégia, risco ou execução — atualmente ausente de forma intencional.
- Teste de integração do Router completo, polling do painel, reinício do dashboard e mock HTTP do provedor.
- Invariante contábil completa entre cash, quantity, entry, fees e patrimônio.

## 7. Integrações: verificadas por código versus documentação/mocks

### 7.1 Resultado geral

> **Integrações externas reais verificadas nesta auditoria: nenhuma.**

A auditoria verificou apenas presença/configuração no código e testes locais encontrados. Não houve prova de conectividade, autenticação, disponibilidade, resposta atual, permissão de conta ou ordem criada em host externo.

| Integração/caminho | O que o código permite ou declara | Evidência encontrada | O que não foi verificado |
|---|---|---|---|
| Binance data API | GET público de klines fechados para o loop paper | URL fixa, parâmetros, ordenação e validação no código | Conectividade, resposta, latência real e disponibilidade do host |
| Binance Spot Testnet | REST privado com HMAC; account, filtros, preço, POST/consulta/cancelamento | Cliente com host fixo e comandos manuais delimitados | Preflight, autenticação real, ordem enviada, status real ou reconciliação real |
| Ordem Testnet | MARKET manual com intenção persistida e `clientOrderId` | Mock HTTP local verifica POST, header, assinatura e ID | Exchange externa e execução em conta Testnet |
| HMAC Binance | Assinatura e vetor criptográfico | Teste local do vetor oficial | Transporte e aceitação pela Binance |
| OpenAI-compatible | Chat completion sem tools, contexto público e advisory manual | Código, política de URL e testes unitários de payload/URL | Chamada ao provedor, credencial, resposta ou disponibilidade |
| Dashboard | Servidor local loopback com Bearer | Código e testes de política/handler | Exposição externa — que é proibida pelo limite observado |
| Alpaca, CCXT/Bybit, Polymarket, TradingView | Hosts, exemplos, guias ou scripts | Documentação/configuração | Adapter, chamada, autenticação, modo somente leitura ou integração funcional |

### 7.2 Documentação, mocks e testes locais não equivalem a integração real

- O servidor em `127.0.0.1` prova construção de requisição assinada contra um mock local, não a Binance externa.
- Fixtures sintéticos de candles, bullish/bearish history, SQLite temporário e CSV em memória provam somente os cenários locais codificados.
- `docs/phase-5-binance-testnet-scope.md`, `docs/phase-6-safety-contracts.md`, README e setup guides são contexto e instrução, não logs de execução.
- A existência de `POST /v3/order` na biblioteca não significa que a ordem tenha sido ativada ou enviada.
- A existência de `ALPACA_PAPER_TRADE=true`, `SANDBOX=true` ou chaves em exemplos não demonstra enforcement nem integração.

## 8. Riscos técnicos e operacionais

### Dados e decisões

1. **Identidade não canônica:** IDs semanticamente iguais com caixa/whitespace diferentes podem não coincidir em comparações exatas do `RiskEngine`.
2. **Qualidade mascarada:** snapshots marcados `DataQuality::Valid` podem não refletir atraso, inconsistência, provenance ou profundidade reais.
3. **Latência mal interpretada:** `now - close` do último candle pode parecer p50/p95, mas não mede recebimento de rede nem as demais barras.
4. **Gaps ocultos:** cadência irregular é rejeitada, mas `gap_count` permanece zero; a ausência fica sem diagnóstico estruturado.
5. **Sem microestrutura:** close/avgPrice não demonstra spread, liquidez, profundidade ou slippage executável; sizing/risco não são avaliação de order book.
6. **Eventos sem contrato forte:** JSON livre, relógio local e ausência de schema/proveniência podem gerar drift e evolução silenciosa.

### Backtest e estratégia

7. **Causalidade dependente do chamador:** `Strategy::evaluate(&[Candle])` não declara que candles estão fechados; a segurança depende de cada chamador.
8. **Invalidação sem saída:** divergência invalidada pode não fechar posição bullish porque o caminho de simulação mantém `Hold`.
9. **Generalização limitada:** split IS/OOS único, sem walk-forward, regimes ou incerteza, não demonstra performance futura.
10. **Métricas incompletas:** drawdown em closes/stops e ausência de exposição/turnover/volatilidade não formam curva financeira operacional completa.
11. **Fixtures sintéticos:** não provam desempenho, dados de exchange ou execução.

### Execução e Testnet

12. **Reconciliação incompleta:** ordem local terminal divergente ou `exchange_order_id` inconsistente pode não bloquear nova submissão.
13. **Janela kill-switch/escopo:** kill-switch não é emergency stop global; bloqueia novas entradas BUY, mas SELL pode continuar e a checagem não é transacional com POST.
14. **Crash/timeout:** não há teste encontrado para crash entre intent e POST, timeout após ordem criada ou corrida multi-processo.
15. **Incompatibilidade `1d`:** aceitação na CLI conflita com limite efetivo do `RiskLimits` e pode impedir inicialização.
16. **Operação externa manual:** o comando Testnet pode enviar ordem à Spot Testnet se credenciais/confirmação forem fornecidas; isso continua sendo operação externa de teste, não trading real.

### Persistência, observabilidade e IA

17. **Divergência SQLite/JSONL:** falha entre commits pode deixar auditorias diferentes; evento e alerta também podem divergir por conexões separadas.
18. **Baixa detecção operacional:** sem métricas externas/notificações, falhas dependem de polling/dashboard e arquivos locais.
19. **Endpoint de IA incorreto:** `ODE_MODEL_BASE_URL` aceita HTTPS válido sem allowlist, podendo enviar API key a destino configurado incorretamente.
20. **Modelo não confiável:** mesmo com fronteira sem tools, a resposta é externa e textual; segurança depende de manter a não execução e a supervisão humana.
21. **Documentação stale:** referências a commits/status históricos e descrição inconsistente de TradingView podem induzir a capacidades não comprovadas.
22. **CI sem fixação total:** workflow usa referências de actions/toolchain que podem derivar; não houve execução para validar o estado atual.

## 9. Testes encontrados — inventário sem alegação de execução

> **Declaração obrigatória:** nenhum dos testes abaixo foi executado nesta auditoria. Os nomes foram apenas localizados por inspeção. Não há afirmação de aprovação, cobertura efetiva ou resultado verde.

### Domínio, eventos e market data

- `crates/ode-domain/src/lib.rs`: `signal_shape_accepts_bounded_probability_and_order_status_helpers_are_explicit`; `signal_shape_rejects_invalid_probability_and_expiry`.
- `crates/ode-events/src/lib.rs`: `audit_events_have_unique_ids_and_structured_details`.
- `crates/ode-market-data/src/lib.rs`: `imports_ohlcv_and_reports_latency_percentiles`; `rejects_gaps_bad_ohlc_and_negative_volume`; `historical_csv_can_omit_unknown_receive_times`.

### Estratégias e backtest

- `crates/ode-strategy-technical/src/lib.rs`: `crossover_uses_only_supplied_closed_history`; `disabled_strategy_never_emits_entry_or_exit`; `invalid_periods_are_rejected`; `missing_divergence_section_defaults_to_disabled_and_invalid_parameters_fail_closed`.
- `crates/ode-strategy-technical/src/divergence.rs`: `divergence_is_disabled_by_default_and_parameters_are_validated`; `insufficient_history_invalid_candles_and_tied_pivots_fail_closed`; `bullish_and_bearish_divergences_wait_for_right_bars_and_use_only_closed_prefixes`; `confirmed_divergence_is_not_repeated_and_break_of_second_pivot_invalidates_it`.
- `crates/ode-backtest/src/lib.rs`: `holdout_report_separates_in_sample_and_oos_metrics`; `next_open_fills_and_costs_are_included`; `invalid_history_and_relaxed_risk_config_fail_closed`; `price_rsi_divergence_runs_only_when_explicitly_selected_and_records_parameters`.
- `src/main.rs`: `divergence_selector_is_backtest_only_and_ema_remains_the_default`; `crossover_buy_is_detected_without_lookahead`; `insufficient_history_never_signals`.

### Risco, execução e journal

- `crates/ode-risk/src/lib.rs`: `kill_switch_blocks_entry_before_any_other_check`; `stale_or_low_quality_market_data_fails_closed`; `malformed_signal_is_rejected_before_sizing`; `policy_applies_strategy_then_market_overrides`; `policy_cannot_disable_mandatory_risk_limits`; `overrides_cannot_relax_base_limits`; `persisted_kill_switch_survives_reopening_and_corruption_fails_closed`; property `authorized_size_never_exceeds_risk_or_allocation_caps`.
- `crates/ode-execution/src/lib.rs`: `duplicate_submission_is_idempotent_and_does_not_add_a_second_order`; `reusing_idempotency_key_with_different_payload_is_rejected`; `reconciliation_detects_missing_unknown_and_divergent_orders`; `venue_status_and_fill_divergence_are_reported`; `duplicate_venue_ids_are_reported_as_anomalies`; `execution_safety_blocks_each_uncertain_state_and_allows_ready_state`.
- `crates/ode-execution/src/sqlite_journal.rs`: `journal_persists_intent_and_rejects_duplicate_payload_conflicts`; `market_guard_blocks_active_orders_and_releases_after_terminal_state`; `journal_updates_fills_monotonically_and_rejects_terminal_downgrades`; `daily_risk_session_is_stable_until_utc_date_changes`.
- `src/testnet_ops.rs`: `generated_client_order_ids_fit_venue_constraints`; `command_validation_requires_exact_testnet_confirmation`; `testnet_commands_refuse_paper_and_backtest_modes`; `testnet_scope_rejects_symbols_without_portfolio_wide_risk`; `unresolved_order_blocks_new_submission_until_terminal_reconciliation`.
- `src/main.rs`: `paper_risk_sizing_respects_allocation_cap`; `paper_position_and_processed_candle_are_restored_after_restart`; `daily_loss_gate_blocks_new_entries`; `persistent_kill_switch_blocks_new_entries`; `unsupported_interval_is_rejected`; `dashboard_and_ai_require_safe_opt_in_configuration`.

### Binance Testnet, observabilidade e IA

- `crates/ode-binance-testnet/src/lib.rs`: `hmac_sha256_matches_binance_documentation_vector`; `symbol_filters_reject_bad_step_size_and_notional`; `order_statuses_map_without_treating_unknown_as_pending`; `signed_market_order_sends_header_and_reconciliable_client_id_to_mock`.
- `crates/ode-model-adapter/src/lib.rs`: `external_plain_http_is_rejected_but_loopback_is_allowed`; `request_contains_only_public_market_context_and_no_tools`; `output_is_bounded_and_trimmed`.
- `crates/ode-observability/src/lib.rs`: `paper_state_survives_reopening_and_unknown_versions_fail_closed`; `paper_event_and_snapshot_commit_together`; `sqlite_store_persists_events_alerts_status_and_acknowledgements`; `repeated_alerts_are_deduplicated_and_counted`; `bearer_auth_is_exact_and_required`; `dashboard_refuses_non_loopback_bind_and_weak_token`; `status_handler_requires_bearer_and_returns_persisted_snapshot`.

### CI declarada

`.github/workflows/rust-ci.yml` declara:

- `cargo fmt --all -- --check`;
- `cargo test --workspace --locked`;
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`.

Nenhum desses comandos foi executado nesta auditoria. A CI também não prova integração externa, Testnet, MCP, credenciais ou setup guides.

## 10. Limitações da auditoria

- Não houve pesquisa adicional, acesso a fontes externas ou uso de resultados além do JSON fornecido.
- Não houve checkout/alteração de código, execução de testes, execução de CI, chamada HTTP, acesso a exchange, uso de credenciais ou envio de ordem.
- Não foi possível confirmar comportamento em runtime, disponibilidade dos hosts, compatibilidade atual das APIs, latência, permissões, rate limits, geo-restrições ou estado de contas.
- Não foram inferidos requisitos ausentes nem completadas lacunas com convenções de mercado.
- O inventário de arquivos e testes está limitado ao escopo listado nos resultados dos agentes.
- A ausência de um arquivo, host ou caminho foi afirmada somente dentro do escopo inspecionado; não é garantia sobre código fora dele.
- Nenhum número de rentabilidade, edge, Sharpe, retorno futuro ou segurança financeira foi calculado ou validado.
- A documentação pode estar stale e, portanto, não foi usada como prova superior à implementação observada.

## 11. Ordem incremental recomendada para as Fases B–H

A ordem abaixo é uma recomendação de dependências técnicas; não representa execução realizada nem compromisso de habilitar produção.

### Fase B — contrato canônico de dados e qualidade

**Objetivo:** remover ambiguidade na identidade e tornar qualidade/provenance obrigatórias antes de decisões.

1. Definir identidade canônica de mercado com venue, símbolo, unidade, moeda/escala e política explícita de normalização.
2. Separar source timestamp, receive timestamp, epoch/timezone e provenance.
3. Tornar validação de candle estrita: duração, alinhamento, futuro/clock skew e consistência por barra.
4. Fazer `DataQualityReport` derivar `DataQuality`, registrar gaps e propagar o relatório obrigatoriamente a `MarketSnapshot` e `RiskEngine`.
5. Adicionar fixtures e testes locais para os cenários citados, sem rede.

**Primeiro incremento pequeno recomendado — B1:** implementar somente a identidade canônica mínima e seus limites de comparação, sem networking e sem order book:

- adicionar uma função/tipo puro para canonicalizar `MarketId` na fronteira de `TradeSignal` e `MarketSnapshot` (trim, política de caixa e rejeição de vazio; venue/aliases devem ser explícitos, não inferidos);
- usar a forma canônica na comparação do `RiskEngine`;
- adicionar testes para whitespace, caixa, vazio e não colisão de venues;
- não alterar estratégia, execução, Testnet ou produção neste incremento.

Esse B1 é deliberadamente pequeno e cria uma base verificável para os demais contratos de B.

### Fase C — adapters de market data e microestrutura

1. Extrair o fetch de klines de `src/main.rs` para um adapter injetável de `ode-market-data`.
2. Criar modelo de order book com níveis Decimal, invariantes bid/ask, sequência, snapshot/delta, checksum e recuperação de gap.
3. Adicionar fixtures HTTP locais para klines, order book, timestamps, qualidade e falhas.
4. Derivar latência e qualidade a partir de timestamps de origem/recebimento reais do adapter.
5. Manter venues limitadas e explicitamente identificadas; não adicionar live por padrão.

### Fase D — execução segura e reconciliação persistente

1. Implementar reconciliação portfolio-wide/por conta e barreira persistente para estados ausentes, desconhecidos e divergentes.
2. Comparar e proteger `exchange_order_id`, `client_order_id`, quantidade, lado, mercado, status e fills.
3. Integrar `OrderLedger` ao journal SQLite ou eliminar a duplicidade de API.
4. Testar concorrência multi-processo, crash entre intent/POST, timeout ambíguo, restart e resposta divergente.
5. Tornar explícito o escopo do kill-switch: bloqueio de BUY versus emergency stop de toda atividade; corrigir ou documentar SELL e a janela pré-POST.
6. Resolver a incompatibilidade do intervalo `1d` antes de ampliar operações.

### Fase E — metodologia de estratégias e backtest

1. Implementar walk-forward rolling/anchored, com treino/teste claramente separados e múltiplos regimes.
2. Definir semântica de saída por invalidação e declarar formalmente long-only ou implementar short com contabilidade completa.
3. Adicionar schema version, fingerprints de dataset/config/parâmetros/custos e provenance de fonte.
4. Adicionar testes golden determinísticos para EMA/RSI e fixtures de contrato CSV.
5. Expandir métricas para curva de equity, exposição, turnover, duração, custos separados e drawdown documentado.
6. Manter a divergência fora do paper/live até revisão e aprovação operacional explícita.

### Fase F — observabilidade e resiliência operacional

1. Tornar política de persistência SQLite/JSONL explícita, configurável e reconciliável.
2. Testar falhas entre conexões, escrita de JSONL, reinício e recuperação de snapshots.
3. Adicionar métricas operacionais, séries históricas e exportação escolhida (por exemplo, Prometheus/OpenTelemetry, se aprovada).
4. Implementar notificações/escalonamento e resolução de alertas com trilha auditável.
5. Criar teste HTTP local do Router completo, incluindo eventos, alertas, ack, capabilities e advisory.
6. Persistir advisory somente se houver decisão explícita de governança; mantê-lo fora do risco/execução.

### Fase G — integração externa controlada, ainda sem produção

1. Escolher uma única venue e um único ambiente; manter Binance Spot Testnet como limite já declarado.
2. Usar transporte injetável, fixtures de respostas, preflight read-only e credenciais segregadas.
3. Executar, se autorizado separadamente, conectividade e operações mínimas em Testnet com logs e reconciliação.
4. Registrar claramente diferença entre mock local, conectividade real, preflight e ordem criada.
5. Não ligar EMA/divergência automaticamente a ordens até aprovação operacional independente.
6. Não adicionar host, credencial ou caminho mainnet/live nesta fase.

### Fase H — validação independente e decisão de release

1. Reexecutar a suíte local e CI em um baseline fixado, registrando comandos, versões e resultados.
2. Fazer revisão independente de segurança, dados, execução, persistência, documentação e rollback.
3. Confirmar que todos os estados desconhecidos/divergentes falham fechado e que a reconciliação é operacionalmente utilizável.
4. Validar runbooks, kill-switch, backup/restore, observabilidade e resposta a incidentes.
5. Reconciliar documentação/status com o commit auditado e separar explicitamente capacidades, mocks e integrações verificadas.
6. Tomar decisão formal sobre escopo posterior. **A Fase H não deve ser interpretada como autorização automática de produção/live.**

## 12. Lacunas abertas prioritárias

1. Identidade canônica de mercado e estratégia.
2. Provenance, source/receive timestamps, qualidade por barra e propagação até risco.
3. Candle validation estrita e relatório real de gaps.
4. Order book e adapter normalizado multi-venue.
5. Reconciliação portfolio-wide, `exchange_order_id` e barreira persistente.
6. Testes de crash, timeout, concorrência, restart e respostas ambíguas.
7. Correção da semântica de `1d`.
8. Walk-forward, regimes, fingerprints, schema version e testes golden.
9. Definição de invalidação/saída e escopo long-only versus short.
10. Atomicidade/reconciliação SQLite–JSONL e métricas operacionais.
11. Teste HTTP local do Router e mock do provedor de IA.
12. Adapters externos além do cliente específico Spot Testnet — atualmente ausentes.
13. Qualquer evidência de conectividade ou operação externa real — atualmente não verificada.
14. Qualquer caminho de produção/live — ausente no escopo auditado e não deve ser inferido.

## 13. Declaração final

Este documento é uma consolidação de auditoria inicial da **Fase A** no baseline `539cbfcb91517c8dca215dfb6cb3d88f9d923a77`. Ele descreve o que os resultados JSON sustentam, registra o que é parcial ou ausente e preserva explicitamente as incertezas. **Não mede rentabilidade, não valida edge, não comprova trading real e não afirma execução de testes ou operações externas.**