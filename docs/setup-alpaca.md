# Setup — Alpaca (Paper Trading)

1. Crie conta em https://app.alpaca.markets/signup
2. No painel, selecione **Paper Trading** e gere **API Key** e **Secret**.
3. Instale o MCP oficial:
```bash
git clone https://github.com/alpacahq/alpaca-mcp-server.git external/alpaca-mcp-server
# ou, conforme o README oficial:
uvx alpaca-mcp-server
```
4. Preencha `ALPACA_API_KEY`, `ALPACA_SECRET_KEY` e mantenha `ALPACA_PAPER_TRADE=true`.
5. Reinicie o Claude Desktop.

## Teste
*"Mostre o saldo da minha conta Alpaca paper."*

Docs: https://docs.alpaca.markets · Paper: https://docs.alpaca.markets/docs/paper-trading
