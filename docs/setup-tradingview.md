# Setup — TradingView

O TradingView **não possui API pública oficial de dados/execução**. Os MCPs disponíveis são projetos **community** que usam dados públicos/screeners. Confira se existe um MCP oficial atualizado antes de escolher.

## Opção recomendada (community)
```bash
git clone https://github.com/atilaahmettaner/tradingview-mcp.git external/tradingview-mcp
```
Siga o README do repo para instalar (geralmente via `uv`/`uvx`) e ajuste o bloco `tradingview` em `configs/claude_desktop_config.example.json`.

## Alternativas
- Pesquise por "tradingview mcp" no GitHub e em https://github.com/modelcontextprotocol/servers
- Webhooks de alertas do TradingView: https://www.tradingview.com/support/solutions/43000529348

## Teste
Pergunte ao Claude: *"Use o TradingView para mostrar o RSI diário de AAPL."*

⚠️ Respeite os Termos de Uso do TradingView.
