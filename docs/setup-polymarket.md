# Setup — Polymarket

⚠️ Polymarket tem **restrições geográficas**. Verifique a legalidade no seu país. Comece em **modo somente leitura**.

## APIs oficiais
| API | Base URL | Uso |
|---|---|---|
| Gamma | https://gamma-api.polymarket.com | Mercados, eventos, metadados |
| CLOB | https://clob.polymarket.com | Order book, preços, ordens |
| Data | https://data-api.polymarket.com | Posições, trades, histórico |

Docs: https://docs.polymarket.com · SDK Python: https://github.com/Polymarket/py-clob-client

## MCPs (community)
```bash
git clone https://github.com/demwick/polymarket-agent-mcp.git external/polymarket-agent-mcp
```
Siga o README do repo. Revise o código antes de fornecer qualquer chave privada de carteira.
Mais opções: busque "polymarket mcp" no GitHub.

## Segurança
- Nunca use a carteira principal; use uma carteira dedicada com saldo mínimo.
- Chave privada somente em variável de ambiente.
