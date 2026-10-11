# Olho de Deus — bot paper MVP

## O que já funciona

- Executável Rust assíncrono que consulta candles Spot públicos da Binance (`data-api.binance.vision`, `GET /api/v3/klines`), sem credenciais.
- Filtra candles ainda abertos e rejeita dados atrasados antes de avaliar uma entrada.
- Primeira estratégia reproduzível: cruzamento de EMA 9/21, usando somente candles fechados.
- Simulador de posição long-only com taxas e slippage configuráveis, dimensionamento por orçamento de risco, limite de alocação e stop simulado.
- Limite diário de perda bloqueia novas entradas. A estratégia pode encerrar uma posição paper por cruzamento de baixa ou stop.
- Auditoria append-only em `data/audit.jsonl` registra candles/sinais, bloqueios e preenchimentos simulados.
- Testes cobrem cruzamento, histórico insuficiente, limite de alocação, limite de perda e validação do intervalo.

## Segurança e limites do MVP

Este bot **não tem adaptador de ordens reais**, não usa API key privada e não possui uma opção para alternar para execução real. Cada preenchimento é fictício e não corresponde ao livro de ofertas nem garante que uma ordem real seria executada. O saldo é reiniciado ao iniciar o processo; posição e estado diário ainda não são persistidos. Não há backtest completo, reconciliação de conta, painel, carteira, futuros, DEX, Polymarket ou integração com IA nesta fase.

Os parâmetros de risco são hipóteses ilustrativas para teste. Mesmo paper trading não valida rentabilidade futura. O dado público pode falhar, atrasar ou divergir; por isso o bot falha fechado para novas decisões quando o fetch falha ou o candle está obsoleto.

## Requisitos

- Rust stable e Cargo (o arquivo `rust-toolchain.toml` seleciona o canal estável).
- Acesso HTTPS à API pública Spot da Binance.

## Executar uma consulta

```bash
cargo run --release -- --symbol BTCUSDT --interval 1m --once
```

## Executar continuamente em paper

```bash
cargo run --release -- --symbol BTCUSDT --interval 1m \
  --poll-seconds 15 --starting-balance 10000 \
  --risk-per-trade 0.01 --max-daily-loss 0.02 \
  --max-allocation 0.25 --stop-loss 0.02
```

Use `Ctrl+C` para encerrar. A auditoria será escrita em `data/audit.jsonl` (pasta ignorada pelo Git). Os percentuais são frações decimais: `0.01` significa 1%. Consulte `--help` para todos os argumentos.

## Verificações de desenvolvimento

```bash
cargo fmt -- --check
cargo test --locked
cargo clippy --all-targets --all-features -- -D warnings
```

## Referência de dados

A Binance documenta o endpoint, a estrutura de klines e os hosts públicos disponíveis em [Market Data Endpoints](https://developers.binance.com/docs/binance-spot-api-docs/rest-api/market-data-endpoints). O bot usa somente dados públicos de mercado; esse endpoint consulta candles e não envia ordens.
