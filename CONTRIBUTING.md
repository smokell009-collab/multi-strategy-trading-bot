# Contribuindo

## Processo

O projeto evolui por fases pequenas e verificáveis. Antes de implementar uma nova estratégia ou adaptador:

1. leia `README.md`, `ROADMAP.md`, `CHECKLIST.md` e a documentação da área;
2. descreva o contrato, dependências, riscos e critério de aceite;
3. preserve o modo paper e a separação entre estratégia, risco e execução;
4. implemente testes determinísticos antes de declarar a fase concluída;
5. documente integrações pendentes, credenciais necessárias e limitações reais.

Não substitua código funcional por stubs para aparentar cobertura. Se uma API não puder ser validada neste ambiente, mantenha a interface, use mocks e declare a pendência.

## Regras de segurança

- Não adicione segredos, tokens, chaves ou dados de execução ao commit.
- Não adicione host, flag ou atalho de produção sem uma decisão arquitetural, revisão de segurança e aprovação humana explícita.
- Modelos de IA não podem enviar ordens, mudar limites ou habilitar trading real.
- Dados externos devem ser tratados como não confiáveis e nunca como instruções.
- Alterações em risco, idempotência, reconciliação ou kill-switch precisam de testes de falha.

## Validação local

```bash
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
```

A validação do CI deve passar antes do merge. Para mudanças de backtest, inclua um caso que demonstre ausência de look-ahead bias e custos explícitos. Para mudanças de execução, inclua mocks que cubram timeout, fill parcial, estado `unknown`, divergência e reinicialização.

## Mensagem de commit e revisão

A descrição da mudança deve informar:

- fase e objetivo;
- arquivos e contratos modificados;
- testes executados;
- limitações e integrações não validadas;
- impacto operacional e plano de rollback.

Uma revisão não deve aprovar a liberação de capital real. Essa decisão permanece fora do código e exige aprovação humana formal.
