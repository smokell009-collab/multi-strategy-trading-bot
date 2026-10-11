# Fase 6 — contratos de segurança e execução

## Objetivo

Implementar a próxima etapa incremental do mapa do PDF sem tentar criar todas as estratégias ao mesmo tempo: tornar explícitos os contratos de sinal e o gate de execução, reforçar a reconciliação e documentar o estado real do projeto.

## Entregas

- `ode-domain` valida a forma de `TradeSignal` antes do sizing: IDs, probabilidade, slippage e timestamps.
- `ode-domain` expõe helpers para distinguir estados terminais e `Unknown` que exige reconciliação.
- `ode-risk` converte sinal malformado em rejeição determinística `InvalidSignal`.
- `ode-execution` expõe `ExecutionSafety`, que bloqueia novas ordens quando kill-switch, mercado, conta ou reconciliação não são confiáveis.
- O fluxo manual Spot Testnet usa o gate antes da criação do intent e mantém a segunda leitura do kill-switch imediatamente antes do POST.
- Documentação adicionada para checklist, segurança, ameaças, risco e ciclo de execução.

## Aceite

```bash
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
```

A fase não adiciona nenhuma venue, não habilita produção, não envia ordens durante os testes e não declara novas estratégias como implementadas.

## Próxima fase recomendada

Implementar um reconciliador de posições paper/Testnet com fixtures de reinicialização, fills parciais, saldo divergente e cancelamento seguro; depois adicionar uma única estratégia mensurável além da EMA, com configuração, backtest e métricas próprias.
