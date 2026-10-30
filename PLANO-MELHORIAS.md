# Plano de melhoria — DLSS 5 + Frame Generation

## Diagnóstico (o que está fraco hoje)

| # | Problema | Impacto | Onde |
|---|---|---|---|
| 1 | Downloads não são verificados (sem hash) | Arquivo corrompido ou adulterado vai direto para o jogo | `net.rs` |
| 2 | A busca de jogos roda na abertura, travando a janela | App "congela" com HD lento ou drive de rede | `main.rs` → `games::scan` |
| 3 | O app não lembra o que instalou em cada jogo | Não mostra versão nem escolha; não dá para atualizar | `install.rs` |
| 4 | Não existe "atualizar tudo" | Mods novos saem toda semana; o usuário refaz jogo por jogo | — |
| 5 | Layout em coluna única, alto (880 px) | Não cabe em telas de notebook; a lista de jogos fica espremida | `main.rs` |
| 6 | O usuário não vê *o que* vai acontecer na imagem | "XeFG + plugin" não diz nada para quem joga | `main.rs` |
| 7 | Log ocupa espaço o tempo todo | Polui a tela | `main.rs` |
| 8 | Limite da API do GitHub (60/h sem login) | Muitas aplicações seguidas caem no cache | `net.rs` |
| 9 | Nada é testado em Windows de verdade | Registro, Xbox e NeuralHub `/S` só verificados por código | — |

## Fase 1 — feita nesta rodada

1. **Hash SHA-256 de cada download** contra o `digest` que o GitHub publica. Se não bater, o arquivo é apagado e a instalação para. (#1)
2. **Busca de jogos em segundo plano**, com botão "Recarregar". (#2)
3. **Estado por jogo** (`.gpu-mods-state.json`): placa, DLSS 5, multiplicador, versões instaladas e data. Selecionar um jogo já modado recarrega as escolhas dele. (#3)
4. **"Atualizar todos"**: reaplica as escolhas salvas em todos os jogos modados com as versões mais novas. (#4)
5. **Layout novo** (#5, #6, #7):
   - Tema escuro com a cor da marca da placa (verde NVIDIA, vermelho AMD, azul Intel).
   - Biblioteca à esquerda com busca, filtro por loja e selo "✔ modado".
   - **Pipeline da imagem**: Jogo → Upscaler → DLSS 5 → Frame Gen → Tela, montado a partir do plano real.
   - **Linha do tempo de quadros animada**: mostra 1 quadro real + N gerados e calcula o FPS final a partir de um FPS base que você ajusta.
   - Log recolhível no rodapé.

## Fase 2 — próxima

- Cache de metadados das releases por 1 h (reduz chamadas à API do GitHub). (#8)
- Escolha do nome do proxy (`dxgi`/`version`/`winmm`) quando o jogo não carrega `dxgi.dll`.
- Perfis por jogo conhecidos (ex.: jogos RE Engine precisam de `REPatch`), vindos de um JSON no repositório.
- Aviso de anti-cheat automático (detectar `EasyAntiCheat`/`BattlEye` na pasta e bloquear).

## Fase 3 — depois

- CI no GitHub Actions gerando o `.exe` assinado a cada tag.
- Teste de fumaça em VM Windows (abrir app, varrer lojas, aplicar num jogo falso).
- Atualização automática do próprio app.
