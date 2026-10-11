# Segurança — Olho de Deus

## Escopo e postura

Este repositório é educacional e opera em **paper trading** por padrão. A integração externa existente é manual e limitada à **Binance Spot Testnet**. Não há caminho de produção no binário atual. Nenhum resultado de backtest, paper ou Testnet é garantia de desempenho futuro.

A regra central é:

> A IA pode propor contexto ou uma análise; somente contratos determinísticos de risco e execução podem autorizar uma entrada.

## Segredos e permissões

- Nunca faça commit de API keys, segredos, frases-semente, tokens de dashboard ou dados de conta.
- Use variáveis de ambiente ou um gerenciador de segredos em ambientes controlados.
- As chaves da Binance devem ser da Spot Testnet, sem saque e com privilégio mínimo.
- Nunca registre credenciais, headers de autenticação ou payloads assinados em logs.
- O arquivo `.env` e os dados locais em `data/` são ignorados pelo Git; confirme isso antes de um commit.
- Carteiras de leitura e execução devem ser separadas quando essa integração existir.

## Dados externos e prompt injection

Descrições de tokens, posts, páginas, nomes de mercados e metadados de contratos são **dados não confiáveis**. Eles não são instruções para o orquestrador ou para qualquer agente. O sistema deve:

1. validar tipos, limites, timestamps e origem;
2. rejeitar dados inválidos ou atrasados para novas entradas;
3. manter a resposta de IA fora do fluxo de risco e execução;
4. registrar a origem e o contexto necessário para auditoria;
5. nunca interpretar texto externo como autorização ou alteração de política.

## Controles obrigatórios

- O motor `ode-risk` não depende de LLM e deve continuar capaz de rejeitar ordens com a IA desligada.
- A configuração de risco não pode zerar uma proteção obrigatória nem afrouxar os limites-base.
- O kill-switch persistente é fail-closed. Arquivo ausente significa inativo; arquivo inválido ou ilegível é erro e deve impedir operação segura.
- O gate de execução bloqueia novas ordens quando kill-switch, mercado, conta ou reconciliação estiverem incertos.
- Uma intenção é gravada antes do POST; resultado incerto não pode gerar retry automático.
- O dashboard é local, exige Bearer token e não expõe rotas de ordem.
- A execução real depende de aprovação humana explícita e de uma fase futura ainda não implementada.

## Resposta a incidente

1. Ative o kill-switch com um motivo claro.
2. Preserve o journal SQLite, auditoria JSONL e logs; não os apague nem troque de caminho.
3. Não repita uma ordem com resposta incerta.
4. Reconcile pelo `clientOrderId` e compare quantidade, status e fills.
5. Registre o incidente, o impacto, a decisão de saída e a aprovação do operador.
6. Só remova o latch após revisão humana autorizada e verificação dos testes.

O procedimento detalhado está em [`docs/incident-response.md`](docs/incident-response.md).

## Reporte de vulnerabilidades

Não publique credenciais, dados pessoais ou detalhes exploráveis em issues públicas. Ao identificar uma vulnerabilidade, preserve uma reprodução mínima sem segredos e comunique os mantenedores por um canal privado antes de divulgar o problema.
