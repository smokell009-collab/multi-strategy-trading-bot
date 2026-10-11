# Configurações versionadas

Os arquivos desta pasta são exemplos revisáveis, não um mecanismo para habilitar execução real. Nesta versão, o binário carrega explicitamente:

- `strategies.example.toml` para a configuração da EMA;
- `risk-overrides.example.toml` para restrições que só podem apertar os limites-base;
- flags do CLI para saldo paper, custos, intervalo, dashboard e caminho do kill-switch.

Não existe configuração de host de produção. O único cliente externo de ordens é fixo à Binance Spot Testnet no código e permanece manual. Não copie credenciais para estes arquivos: use variáveis de ambiente e permissões mínimas.

Ao adicionar uma nova configuração, documente o schema, valide campos desconhecidos, escreva testes de rejeição e explique se o campo altera dados, risco, execução ou observabilidade.
