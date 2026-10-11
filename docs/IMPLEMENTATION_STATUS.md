# Estado de implementação verificado

Atualizado para a branch `main` baseada no commit `e3da4ae` e para a implementação local do requisito 28. A cópia acessível nesta sessão é `/home/ubuntu/workspace/repo`; `/home/ubuntu/odg` não existe. O PDF de continuidade descreve commits e capacidades que não estão presentes neste histórico. Este documento privilegia código e testes observáveis, não a lista presumida do PDF.

## Funcionalidades presentes antes do requisito 28

O workspace Rust contém domínio e validação de sinais, motor de risco fail-closed, kill-switch persistente, EMA 9/21 usada no paper, validação OHLCV, holdout temporal no backtest, Binance Spot Testnet manual com journal e reconciliação, persistência paper, dashboard local e comentário de IA opcional/read-only. Os testes do baseline passaram antes desta mudança.

Não foram encontrados nesta branch: RSI/MACD/Bollinger como estratégias; walk-forward; comparação de estratégias em janelas rolling; DHS Signal Quality Score; proveniência de dataset/configuração com fingerprint SHA-256; versões de schema de relatório; integrações Alpaca funcionais; adapters Rust CCXT/Binance/Bybit normalizados. Guias de configuração ou traits, se existirem, não contam como integração validada.

## Requisito 28 — estado desta entrega

Implementado localmente no crate técnico: divergência regular preço/RSI determinística, config TOML com `enabled = false`, pivôs estritos confirmados por barras à direita, RSI Wilder calculado apenas até o candle do pivô, sinal emitido somente no candle de confirmação, invalidação por rompimento do extremo do segundo pivô e análise de estado sem probabilidade estimada. O seletor `--backtest-strategy price-rsi-divergence` existe somente com `--backtest-csv`; o loop paper continua instanciando EMA.

O relatório do backtest registra ID, versão semântica do algoritmo e parâmetros serializados. A integração de holdout passou em teste automatizado e em uma execução CLI local sintética que verificou estratégia/versão/parâmetros; essa execução não é evidência financeira. A validação mais recente passou em `cargo fmt --all -- --check`, **64 testes** do workspace e Clippy com `-D warnings`. Dataset fingerprint e schema version ainda não existem nesta branch. Walk-forward ainda não existe, portanto o requisito 28 permanece **parcial**.

## Restrições operacionais

O modo paper continua padrão e EMA continua sendo a única estratégia conectada ao loop paper. Selecionar divergência exige flag explícita e exige backtest. Nenhuma ordem real ou Testnet foi enviada por esta implementação. Nenhum LLM participa do cálculo da divergência ou da autorização de risco.

## Evidência requerida para atualizar o estado

Executar `cargo fmt --all -- --check`, `cargo test --workspace --locked`, `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` e `git diff --check`. Só depois registrar o número real de testes e o resultado observado; não marcar o requisito como concluído enquanto walk-forward permanecer ausente.
