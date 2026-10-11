# Fase B1 — identidade canônica mínima de mercado

**Estado:** implementado e validado localmente; não representa integração com venues nem conclusão da Fase B.

## Contrato

`MarketId::canonicalized()` retorna uma cópia normalizada aplicando somente `trim()` nas extremidades e conversão para ASCII maiúsculo. Identificador vazio após a normalização retorna `MarketIdError::Empty`. A função não altera o valor armazenado pelo construtor `From`, não inventa aliases, não converte símbolos e não infere venues.

Quando há venue conhecida, a forma recomendada é explícita e qualificada, por exemplo `BINANCE:BTCUSDT`. `BINANCE:BTCUSDT`, `binance:btcusdt` e o mesmo valor com espaços externos canonicalizam à mesma chave; `COINBASE:BTCUSDT` permanece distinto. O identificador legado `BTCUSDT` continua válido por compatibilidade, mas não contém informação de venue e não deve ser misturado entre fontes distintas.

## Aplicação

- `TradeSignal::validate_shape()` rejeita `market_id` vazio após canonicalização.
- `RiskEngine::authorize_entry()` compara as formas canonicalizadas do sinal e do snapshot. Uma identidade vazia no sinal ou no snapshot falha fechado.
- Overrides por mercado são consultados pela identidade canônica. Exemplo TOML para chave qualificada:

```toml
[markets."BINANCE:BTCUSDT"]
max_slippage_bps = 3
```

Os overrides existentes como `[markets.BTCUSDT]` permanecem válidos para o identificador legado.

## Limites explícitos

Este incremento não altera adapters, símbolos, configuração de estratégia, backtest, paper, Testnet ou execução; não valida se a venue/símbolo existe, nem normaliza unidades, contratos ou aliases. `MarketId` ainda é um identificador em string e construtores/serialização não canonicalizam automaticamente. A regra aplica-se nas fronteiras de validação de sinal e autorização de risco para não quebrar silenciosamente os formatos persistidos/configurados.

Próximos passos de Fase B, separados deste B1: modelo estruturado de identidade venue/símbolo/unidade, timestamps fonte/recebimento, validação temporal de candles, relatório de gaps e propagação de qualidade derivada até o risco. Nenhum resultado de performance financeira é relacionado a esta mudança.

## Validação local observada

`cargo fmt --all -- --check`, `cargo test --workspace --locked --quiet` (**70 testes aprovados**), `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` e `git diff --check` passaram após a implementação. Nenhum teste ou serviço externo foi chamado; o resultado não mede rentabilidade.
