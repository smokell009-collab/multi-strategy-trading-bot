# Política de risco

## Princípios

- A IA pode sugerir; `ode-risk` decide se a entrada é elegível.
- A política é aplicada antes do sizing e não pode ser desabilitada por configuração.
- Dados atrasados, inválidos ou inconsistentes não autorizam novas entradas.
- A ausência de certeza sobre conta, mercado, ordem ou saldo deve bloquear novas entradas.
- O modo padrão é paper; a única venue externa atual é Spot Testnet manual.

## Limites-base atuais

Os valores vêm das flags do binário e são validados no código. As hipóteses iniciais documentadas são:

| Controle | Exemplo padrão | Observação |
|---|---:|---|
| risco por operação | 1% | limite de sizing até o stop e custos |
| perda diária | 2% | em relação ao equity inicial do dia UTC |
| alocação por posição | 25% | específica ao MVP atual |
| stop simulado | 2% | paper e sizing |
| taxa | 10 bps | parâmetro simulável |
| slippage | 5 bps | também limitado pelo sinal |

Esses números não são recomendação de investimento e não comprovam adequação para qualquer conta.

## Overrides

`configs/risk-overrides.example.toml` permite restringir por estratégia ou mercado. A implementação aplica `min` a risco, alocação, slippage e idade de dados; aplica `max` a taxa e edge mínimo. Portanto, um override não pode afrouxar o limite-base nem zerar proteção obrigatória.

## Kill-switch

O arquivo persistente `data/KILL_SWITCH.json` bloqueia entradas antes do feed e sobrevive a reinícios. A ativação é atômica e exige motivo não vazio. Arquivo ilegível ou JSON inválido falha fechado. Não existe comando de desativação: o rearme requer inspeção humana, reconciliação e remoção manual por operador autorizado.

## Limitações

A política atual cobre um par por vez no MVP. Não agrega correlação, exposição multi-ativo, margem, funding, gás, risco de contrato ou liquidação. Esses controles são pré-requisitos para estratégias e venues futuras, não funcionalidades implícitas.
