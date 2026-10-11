# Checklist de prontidão — Olho de Deus

> Esta checklist distingue o que está implementado, o que foi validado localmente e o que continua pendente. **Nada nesta página autoriza execução com capital real.**

## Fundação e contratos

- [x] Workspace Rust compila com `cargo test --workspace --locked`.
- [x] Formatação e Clippy são executados pelo CI.
- [x] Tipos de domínio para mercados, sinais, posições e estados de ordens estão separados.
- [x] Sinais validam identificadores, probabilidade, slippage e janela temporal antes do sizing.
- [x] Dependências de risco e domínio não dependem de modelos de IA.

## Dados e estratégia

- [x] Histórico OHLCV valida preços, OHLC, volume, ordenação, cadência e timestamps de recebimento.
- [x] Dados inválidos, atrasados ou inconsistentes bloqueiam novas entradas.
- [x] A EMA 9/21 é um módulo independente, configurável e sem acesso ao executor.
- [x] O backtest usa candles fechados, fill na abertura seguinte e custos explícitos.
- [ ] Walk-forward, separação treino/validação/teste e avaliação estatística abrangente.
- [ ] Estratégias de arbitragem, previsão, clima, memecoin, copy-trading e Smart Money.

## Risco, execução e recuperação

- [x] O motor de risco é determinístico e independente da IA.
- [x] Risco por operação, perda diária, alocação, slippage, dados e saldo são validados.
- [x] Overrides de configuração só podem apertar os limites-base.
- [x] Kill-switch persistente, atômico e fail-closed bloqueia novas entradas.
- [x] O gate de execução bloqueia kill-switch, estado de mercado/conta desconhecido e reconciliação pendente.
- [x] Intents são persistidos antes do POST e protegidos por idempotência.
- [x] Estados `unknown`, fills parciais e divergências exigem reconciliação explícita.
- [ ] Política completa de cancelamento/saída de posições durante incidentes em cada venue.

## IA, segurança e observabilidade

- [x] Comentário de IA é opt-in, manual, read-only e recebe apenas contexto público limitado.
- [x] Dashboard fica restrito a loopback e exige Bearer token.
- [x] Segredos ficam fora do repositório e dados locais ficam ignorados pelo Git.
- [x] Eventos de risco, feed e kill-switch geram auditoria e alertas deduplicados.
- [ ] Adaptadores de modelos específicos após confirmação de disponibilidade e contratos oficiais.
- [ ] Notificações externas, gestão de incidentes automatizada e alta disponibilidade.

## Liberação

- [x] Paper trading é o modo padrão.
- [x] A integração de exchange disponível é manual e restrita à Binance Spot Testnet.
- [x] Não existe host, flag ou caminho de produção no binário atual.
- [ ] Paper trading prolongado e validação fora da amostra com dados reais versionados.
- [ ] Revisão humana formal de segurança, jurisdição, permissões e operação.
- [ ] Aprovação humana explícita antes de qualquer consideração de capital real.

## Evidências mínimas por mudança

```bash
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
```

Uma tarefa só pode ser marcada como concluída quando houver teste ou evidência reproduzível, limitação documentada e nenhuma integração externa for declarada validada sem credenciais e ambiente apropriados.
