#!/usr/bin/env bash
# ============================================================
# Instala/clona os MCPs recomendados em ./external (ignorado pelo git).
#
# Observações importantes:
# - O MCP OFICIAL da TradingView é REMOTO (https://mcp.tradingview.com/mcp)
#   e NÃO precisa de instalação local — por isso não é clonado aqui.
# - Revise o código de cada repositório antes de usar.
# - Comece SEMPRE em paper trading / testnet.
# ============================================================
set -euo pipefail
mkdir -p external && cd external

echo ">> Pré-requisitos: git, Node.js 18+, Python 3.10+ e uv (https://docs.astral.sh/uv/)"

# Clona apenas se a pasta ainda não existir
clone() { [ -d "$2" ] && echo "   $2 já existe, pulando" || git clone "$1" "$2"; }

echo ">> Alpaca (oficial)";       clone https://github.com/alpacahq/alpaca-mcp-server.git alpaca-mcp-server
echo ">> CCXT (community)";       clone https://github.com/doggybee/mcp-server-ccxt.git mcp-server-ccxt
echo ">> Polymarket (community)"; clone https://github.com/demwick/polymarket-agent-mcp.git polymarket-agent-mcp

echo ""
echo ">> Instalação concluída. Próximos passos:"
echo "   1. Siga o README de cada pasta em ./external para instalar dependências"
echo "   2. Copie configs/claude_desktop_config.example.json para o local de config do Claude"
echo "      e preencha suas chaves (o MCP da TradingView já está pronto, é só a URL)"
echo "   3. Copie .env.example para .env e preencha as variáveis"
echo "   4. Reinicie o Claude Desktop e teste em paper trading / testnet"
