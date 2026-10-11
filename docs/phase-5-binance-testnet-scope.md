# Fase 5 — escopo Binance Spot Testnet

## Decisão de escopo

A Fase 5 deste repositório será implementada para **Binance Spot Testnet**, reutilizando BTCUSDT como par padrão. O host da Testnet será constante no código, distinto do host de produção; nenhuma variável de ambiente poderá trocar o host para produção. O caminho de execução real fica fora desta fase.

A jurisdição do operador não foi informada e não é considerada validada por este documento. Como o escopo é exclusivamente ambiente de teste, a implementação não infere permissão legal para qualquer operação real. Antes de considerar uma venue real, o operador terá de definir jurisdição, permissões e controles, revisar o código e autorizar explicitamente a mudança.

As chaves de Testnet deverão ser geradas pelo próprio operador no portal da Binance Spot Test Network e fornecidas apenas por variáveis de ambiente (`BINANCE_API_KEY` e `BINANCE_SECRET`). Nunca devem ser copiadas para issues, commits ou logs. Nenhuma chave ou ordem da conta do usuário foi usada para implementar ou validar esta fase.

## Contratos técnicos verificados

A base REST documentada para Spot Testnet é `https://testnet.binance.vision/api`. A API documenta chamadas assinadas com `timestamp` e `signature`; para HMAC-SHA256, a assinatura é hexadecimal. O limite documentado de `recvWindow` é 60.000 ms; a documentação recomenda usar um valor pequeno, de até 5.000 ms. Endpoints de trading e dados privados também dependem da API key e das permissões correspondentes.[1] [3]

O teste unitário local usa o vetor ilustrativo oficial `symbol=LTCBTC&side=BUY&type=LIMIT&timeInForce=GTC&quantity=1&price=0.1&recvWindow=5000&timestamp=1499827319559` e compara o HMAC-SHA256 hexadecimal com o resultado publicado; são credenciais de exemplo da documentação, não chaves de usuário.[3]

O fluxo de ordem deve preservar o `newClientOrderId` e consultar a ordem por esse ID quando uma resposta ficar incerta. A documentação da API alerta que um timeout pode significar estado de execução desconhecido; por isso a implementação não pode repetir cegamente um POST de ordem. O estado deve ser consultado e reconciliado antes de qualquer nova ação.[1]

Ordens de mercado são validadas pelos filtros do símbolo, em especial `MARKET_LOT_SIZE`/`LOT_SIZE` e os filtros `MIN_NOTIONAL`/`NOTIONAL` aplicáveis ao mercado. A quantidade precisa respeitar mínimos, máximos e `stepSize`; notional é quantidade multiplicada pelo preço de referência conforme as regras da exchange.[2]

## Limite obrigatório

Os testes automatizados podem usar servidor HTTP local/mocks. Uma consulta pública de horário ou `exchangeInfo` na Testnet é somente leitura. Nenhuma ordem deve ser enviada à Testnet durante a implementação sem pedido explícito; nenhuma ordem real pode ser enviada em qualquer etapa desta fase.

## Implementação entregue

- `ode-binance-testnet` usa URL-base constante, HTTPS obrigatório fora dos testes, redirects desativados, sincronização de relógio e HMAC-SHA256 validado pelo vetor oficial. O adapter oferece preflight de conta, regras de símbolo, preço médio, ordem MARKET, consulta por `clientOrderId` e cancelamento.
- A CLI expõe `testnet-preflight`, `testnet-place-market-order` e `testnet-reconcile-order`; para o POST é exigida a opção literal `--confirm-testnet-order BINANCE-SPOT-TESTNET`. O modo paper continua sendo o padrão.
- A CLI restringe a Fase 5 a `BTCUSDT`; não há agregação de limites ou de exposição entre mercados.
- Entradas de compra passam pelo `ode-risk`, saldo livre, kill-switch, limites configurados, âncora diária UTC persistente e filtros Spot. Vendas manuais são limitadas ao saldo base livre.
- O journal SQLite persiste o intent antes do POST, usa ID idempotente, bloqueia atomicamente ordens não terminais concorrentes por símbolo e não repete automaticamente chamadas após timeout/resultado incerto. O operador deve reconciliar primeiro.
- A validação desta implementação foi feita por testes unitários e servidor mock local. **Não foram fornecidas chaves do usuário e nenhuma ordem de Testnet ou produção foi enviada.**
- Como verificação de conectividade sem autenticação, esta sandbox consultou apenas `GET /api/v3/time`; a Binance respondeu HTTP 451 com mensagem de restrição de localização. Isso descreve o endereço/ambiente desta sandbox, **não** determina a elegibilidade, jurisdição ou acesso do operador. O preflight autenticado não foi executado; se e onde o operador puder usar o serviço, deve testar a própria conta e observar os termos locais.

Esta entrega não conecta sinais da EMA a ordens, não instala stop remoto, não oferece um loop autônomo de entrada/saída e não contém host/flag de produção. Uma ordem MARKET de Testnet permanece uma operação externa de teste, não uma simulação local; use somente credenciais próprias de Testnet e revise quantidades antes de executar.

## Referências oficiais

[1]: https://developers.binance.com/en/docs/products/spot/testnet/rest-api "Binance Spot Testnet REST API — base URL, segurança, endpoints de ordem e resposta"
[2]: https://developers.binance.com/en/docs/products/spot/filters "Binance Spot Filters — regras de preço, quantidade e notional"
[3]: https://developers.binance.com/en/docs/products/spot/rest-api "Binance Spot REST API — request security, signing example and timing rules"
