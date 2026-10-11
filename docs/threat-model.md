# Modelo de ameaças

## Ativos protegidos

- saldo, posições e ordens;
- chaves e permissões de venues;
- políticas de risco e kill-switch;
- integridade da auditoria e do journal;
- decisões, parâmetros, sinais e dados históricos;
- disponibilidade do operador para intervenção.

## Fronteiras de confiança

```text
Dados externos não confiáveis
        │ validação, timestamps, origem
        ▼
Normalização → estratégias → sinais tipados
                              │
                              ▼
                    risco determinístico
                              │
                    gate de execução
                              │ journal/idempotência
                              ▼
                         venue externa

IA read-only ──► comentário/auditoria
       (não acessa risco, ordens, saldo ou credenciais)
```

## Ameaças e controles

| Ameaça | Impacto | Controle atual | Lacuna conhecida |
|---|---|---|---|
| Dado atrasado, inválido ou inconsistente | Entrada baseada em estado incorreto | Validação de OHLC/cadência/latência e bloqueio no risco | Conectores adicionais ainda não existem |
| Ordem duplicada após timeout | Exposição duplicada | Intent antes do POST, chave idempotente e journal persistente | Política varia por venue futura |
| Estado `unknown` ou fill divergente | Reconciliação incorreta | Estado não terminal, bloqueio por mercado e consulta explícita | Reconciliador de posições agregado ainda pendente |
| Kill-switch corrompido ou contornado | Novas entradas indevidas | Persistência atômica, leitura fail-closed e gate de execução | Remoção manual exige procedimento operacional |
| IA ou prompt injection autoriza risco | Operação insegura | Adaptador read-only e risco sem dependência de IA | Orquestrador multiagente ainda não implementado |
| Chave exposta em código/log | Perda financeira | `.gitignore`, variáveis de ambiente e logs sem payload assinado | Secret manager de produção pendente |
| Dashboard exposto | Vazamento/controle indevido | Loopback, Bearer token, CSP e sem rotas de ordem | Alta disponibilidade não é objetivo atual |
| Backtest com vazamento futuro | Métrica enganosa | Candles fechados, fill seguinte e holdout fixo | Walk-forward e testes estatísticos pendentes |

## Critério de falha segura

Se o estado da conta, mercado, ordem ou política não puder ser confirmado, o sistema deve preferir **não criar uma nova entrada**, registrar o motivo e pedir reconciliação ou intervenção humana. Não se deve fechar uma posição automaticamente durante um incidente sem uma política de saída compatível com a liquidez e a venue.
