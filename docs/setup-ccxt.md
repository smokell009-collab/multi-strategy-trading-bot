# Setup — CCXT (Binance, Bybit, etc.)

[CCXT](https://github.com/ccxt/ccxt) unifica mais de 100 exchanges de cripto.

## 1. Keys de testnet
- Binance Spot Testnet: https://testnet.binance.vision
- Bybit Testnet: https://testnet.bybit.com
Desative **saque** e, se possível, restrinja por IP.

## 2. MCP
```bash
git clone https://github.com/doggybee/mcp-server-ccxt.git external/mcp-server-ccxt
# ou via npx, conforme README:
npx -y @mcpfun/mcp-server-ccxt
```
Configure o bloco `ccxt` em `configs/claude_desktop_config.example.json` com modo sandbox.

## Teste
*"Via CCXT, mostre o preço de BTC/USDT na Binance testnet."*
