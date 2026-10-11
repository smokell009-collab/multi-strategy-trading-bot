# Decisões de arquitetura — ODG

## ADR-028-1 — Divergência causal preço/RSI

**Status:** adotada nesta entrega.

O detector usa pivôs estritos: o preço no centro deve ser estritamente menor/maior que todas as barras vizinhas definidas por `pivot_left_bars` e `pivot_right_bars`. Empates não formam pivô. Um pivô só passa a existir após o fechamento de todas as barras à direita. O RSI Wilder é avaliado no índice do pivô e calculado exclusivamente com fechamentos até esse índice; candles posteriores confirmam o pivô, mas não recalculam seu valor retrospectivo.

Bullish exige lower-low de preço e higher-low de RSI. Bearish exige higher-high de preço e lower-high de RSI. As comparações são estritas. Nenhuma probabilidade calibrada ou score de confiança é produzido.

## ADR-028-2 — Invalidação e emissão única

**Status:** adotada nesta entrega.

O sinal só pode ser emitido na barra que confirma o segundo pivô e `evaluate` retorna `Hold` em prefixos posteriores, evitando repetição do mesmo evento. Uma quebra abaixo do mínimo do segundo pivô bullish ou acima do máximo do segundo pivô bearish marca o setup como `Invalidated`. Isso não cancela retroativamente decisões históricas conhecidas antes da quebra; a evidência futura só pode influenciar decisões posteriores.

## ADR-028-3 — Isolamento do paper

**Status:** adotada nesta entrega.

`price_rsi_divergence` é selecionável somente com `--backtest-csv --backtest-strategy price-rsi-divergence`. A configuração de exemplo mantém `enabled = false`. O loop paper não consulta esse módulo, não persiste estado de divergência e continua usando EMA 9/21. O backtest existente é long-only: um `Sell` pode fechar posição, mas não abre short.

## ADR-028-4 — Proveniência mínima de estratégia

**Status:** adotada nesta entrega.

Cada implementação de `Strategy` declara ID, versão semântica do algoritmo e parâmetros serializáveis. `BacktestReport` registra esses valores. A versão `1.0.0` identifica o contrato algorítmico inicial, não uma alegação de maturidade estatística. O relatório ainda não tem schema version, fingerprint SHA-256 do dataset/configuração ou proveniência completa de custos/fonte nesta branch; esses itens não são considerados implementados.

## Decisões futuras

Walk-forward ainda não está implementado no repositório atual. A divergência só poderá ser integrada a janelas rolling após a existência de um contrato de janela sem vazamento e testes comparáveis à execução holdout. Nenhum resultado deste módulo constitui evidência de edge, lucro ou taxa de acerto futura.
