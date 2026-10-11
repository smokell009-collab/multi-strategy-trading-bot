# Resposta a incidentes

## Quando acionar

Acione este procedimento diante de dados atrasados persistentes, erro de API/RPC, divergência de saldo ou posição, ordem em `unknown`, fill incompatível, credencial suspeita, alerta crítico ou comportamento que não possa ser explicado pela auditoria.

## Passos

1. Pare novas entradas ativando o latch:

   ```bash
   cargo run --release -- --activate-kill-switch "motivo do incidente"
   ```

2. Preserve `data/KILL_SWITCH.json`, o journal `data/testnet-orders.sqlite3`, o banco de auditoria e a auditoria JSONL. Não altere o caminho nem apague sidecars SQLite.
3. Identifique o último `client_order_id` e não repita o comando de ordem se o resultado do POST for incerto.
4. Reconcile pela consulta explícita da Testnet:

   ```bash
   cargo run --release -- --symbol BTCUSDT \
     testnet-reconcile-order --client-order-id "SEU_CLIENT_ORDER_ID"
   ```

5. Compare intent e venue: símbolo, lado, quantidade solicitada, quantidade executada, status, saldo e posição.
6. Registre no evento de incidente a linha do tempo, impacto, evidências, decisões e operador responsável.
7. Corrija a causa fora do fluxo de negociação e rode a suíte completa.
8. Somente um operador autorizado pode remover manualmente o latch após revisar a reconciliação e aprovar o rearme.

## Critério de encerramento

Um incidente não está encerrado apenas porque o alerta foi reconhecido. O estado da conta e das ordens precisa estar reconciliado, a causa precisa estar documentada, os testes precisam passar e os limites operacionais devem ser revalidados.

O procedimento não fecha automaticamente posições. Saídas durante incidentes exigem uma política específica para a venue e liquidez disponível; não devem ser improvisadas como atalho para liberar novas entradas.
