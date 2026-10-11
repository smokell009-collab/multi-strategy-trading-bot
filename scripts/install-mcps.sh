#!/usr/bin/env bash
# Instala/clona os MCPs recomendados em ./external (ignorado pelo git).
# Revise cada repositório antes de usar. Comece SEMPRE em paper/testnet.
set -euo pipefail
mkdir -p external && cd external

echo ">> Pré-requisitos: git, Node.js 18+, Python 3.10+ e uv (https://docs.astral.sh/uv/)"

clone() { [ -d "$2" ] && echo "   $2 já existe, pulando" || git clone "$1" "$2"; }

echo ">> Alpaca (oficial)";      clone https://github.com/alpacahq/alpaca-mcp-server.git alpaca-mcp-server
echo ">> TradingView (community)"; clone https://github.com/atilaahmettaner/tradingview-mcp.git tradingview-mcp
echo ">> CCXT (community)";      clone https://github.com/doggybee/mcp-server-ccxt.git mcp-server-ccxt
echo ">> Polymarket (community)"; clone https://github.com/demwick/polymarket-agent-mcp.git polymarket-agent-mcp

echo ""
echo ">> Próximos passos:"
echo "   1. Siga o README de cada pasta em ./external para instalar dependências"
echo "   2. Copie configs/claude_desktop_config.example.json para o local do Claude e preencha as keys"
echo "   3. Reinicie o Claude Desktop"
