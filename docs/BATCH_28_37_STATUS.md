# Status do lote de requisitos 28–37

## Base observada

Este status descreve a branch `main` realmente disponível nesta sessão, não as capacidades listadas como “confirmadas” no PDF de continuidade. `/home/ubuntu/odg` e o status histórico não existiam; a cópia ativa está em `/home/ubuntu/workspace/repo`. Recursos avançados não localizados não foram presumidos.

## Requisitos

| Req. | Tema | Estado observado | Próxima evidência necessária |
|---|---|---|---|
| 28 | Divergência preço-RSI | **Parcial — commit `817f2ce` publicado em `main`.** Núcleo determinístico, configuração off por padrão, testes causais, seletor de holdout e proveniência de estratégia foram implementados e validados localmente. Walk-forward não existe no baseline. | Implementar e testar integração com walk-forward quando essa infraestrutura existir; depois revisar critérios e métricas. |
| 29 | Momentum | Não iniciado; ROC, volatilidade realizada e volume relativo ainda não são estratégia implementada. | Contrato OHLCV explícito, limites, configuração e teste de backtest sem inventar spread/profundidade/DHS. |
| 30 | Order Blocks | Não iniciado. | Aprovar/documentar definição v1 antes do código e testar timestamps de confirmação/invalidação. |
| 31 | Fair Value Gaps | Não iniciado. | Estado monotônico em prefixos causais, sem confundir gap de mercado com lacuna de cadência. |
| 32 | Break of Structure | Não iniciado. | Pivôs estritos confirmados, rompimento por fechamento e emissão única. |
| 33 | Change of Character | Não iniciado. | Estado bullish/bearish/unknown, transição confirmada e desligada por padrão. |
| 34 | Fake Moves | Não iniciado. | Avaliação tipada, evidência/severidade/janela; sem chamar heurística de fraude ou liquidez. |
| 35 | Alpaca Paper | Não iniciado como adapter Rust funcional; guias não comprovam integração. | REST isolado, host fixo paper, HTTPS, mock local, journal separado e conta/credenciais fornecidas por canal seguro para validação externa. |
| 36 | Alpaca Live | Não implementado; nenhuma ordem live será enviada. | Apenas guards/preflight read-only podem ser considerados em fase separada; live permanece bloqueado. |
| 37 | CCXT/venues | Fronteira Rust normalizada não localizada; documentação de setup não é adapter. | Tipos, transporte injetável, fixtures, adapters independentes e namespace venue/account antes de qualquer ordem. |

## Requisito 28 — detalhes desta entrega

A divergência é selecionável exclusivamente pelo backtest. A config de exemplo usa `enabled = false`, e EMA segue como default e única estratégia do loop paper. O detector usa pivôs estritos, RSI Wilder no prefixo até o pivô, confirmação com `right_bars`, lower-low/higher-low bullish, higher-high/lower-high bearish, invalidação no extremo do segundo pivô, e nenhuma probabilidade artificial. O holdout registra ID, versão semântica do algoritmo e parâmetros. O teste de integração do holdout passou e a CLI foi exercitada localmente com CSV sintético; nenhum resultado financeiro foi inferido. Os gates locais passaram: formato, 64 testes, Clippy com `-D warnings` e `git diff --check`. A integração walk-forward fica pendente porque não há implementação walk-forward nesta branch.

## Estado do commit/push

O commit `817f2ce` (`feat: add causal price-rsi divergence backtest`) foi publicado em `main`. Os gates locais passaram: formatação, 64 testes, Clippy com `-D warnings` e `git diff --check`. O GitHub Actions do commit de código estava **queued** na consulta: [workflow Rust CI](https://github.com/smokell009-collab/multi-strategy-trading-bot/actions/runs/38135998438). Não marcar `DONE_VERIFIED`: a integração walk-forward permanece pendente.
