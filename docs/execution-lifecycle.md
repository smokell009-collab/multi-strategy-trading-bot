# Ciclo de execução e reconciliação

## Fluxo normativo

1. **Sinal:** a estratégia produz um `TradeSignal` com estratégia, mercado, direção, edge, slippage e janela temporal.
2. **Validação de contrato:** o domínio rejeita identificadores vazios, probabilidade fora de `[0, 1]`, timestamps inválidos e expiração invertida.
3. **Qualidade e risco:** `ode-risk` verifica kill-switch, mercado correspondente, qualidade/idade dos dados, expiração, edge, slippage, posição, saldo, perda diária e sizing.
4. **Gate de execução:** `ode-execution` bloqueia kill-switch, estado de mercado/conta desconhecido e reconciliação pendente.
5. **Intent:** o `client_order_id` e payload são persistidos antes de qualquer POST.
6. **Envio:** a venue recebe uma única tentativa; o cliente não faz retry automático de resultado incerto.
7. **Reconciliação:** a resposta é comparada com o intent por mercado, lado, quantidade, status e fill.
8. **Auditoria:** sinal, decisão, rejeição, ordem, fill, erro e configuração devem ser registráveis.

## Estados

| Estado | Pode criar nova ordem no mercado? | Ação |
|---|---:|---|
| `pending` | não | consultar/reconciliar |
| `unknown` | não | não repetir POST; consultar por `clientOrderId` |
| `open` | não | acompanhar ou cancelar quando suportado |
| `partially_filled` | não | reconciliar fill e saldo |
| `filled` | sim, se todos os demais gates passarem | atualizar posição |
| `cancelled` | sim, se todos os demais gates passarem | registrar motivo |
| `rejected` | sim, após entender a rejeição | corrigir causa sem burlar limites |

`Unknown` nunca é tratado como sucesso nem como cancelamento. Divergências de campos ou fills devem manter a ordem em estado de revisão e bloquear novo envio no mercado.

## Escopo atual

- O paper broker usa SQLite para snapshot de carteira e auditoria transacional.
- A execução externa manual é limitada à Binance Spot Testnet e a `BTCUSDT`.
- Não há execução de estratégia automática na exchange, stop remoto, futuros, DEX ou produção.
- A reconciliação de posições agregadas e cancelamento seguro durante incidentes são itens de fases futuras.
