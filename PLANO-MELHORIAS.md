# Plano de melhoria — DLSS 5 + Frame Generation

Todas as fases estão concluídas (versão 1.1.0).

## Diagnóstico inicial

| # | Problema | Resolvido em |
|---|---|---|
| 1 | Downloads sem verificação de hash | Fase 1 |
| 2 | Busca de jogos travava a janela | Fase 1 |
| 3 | O app não lembrava o que instalou em cada jogo | Fase 1 |
| 4 | Não existia "atualizar tudo" | Fase 1 |
| 5 | Layout em coluna única, sem biblioteca | Fase 1 + biblioteca de capas |
| 6 | O usuário não via o que acontece na imagem | Fase 1 |
| 7 | Log ocupava a tela | Fase 1 |
| 8 | Limite da API do GitHub (60/h) | Fase 2 |
| 9 | Nada testado em Windows de verdade | Fase 3 |

## Fase 1 — ✔ concluída
- SHA-256 de todo download conferido contra o GitHub.
- Busca de jogos em segundo plano.
- Estado por jogo (`.gpu-mods-state.json`) e **Atualizar todos**.
- Interface nova: biblioteca de capas por loja, tela do jogo, caminho da imagem, linha do tempo de quadros, progresso flutuante.

## Fase 2 — ✔ concluída
- **Cache de 1 h** das informações de release (`%LOCALAPPDATA%\GpuMods\cache\meta`): aplicar em vários jogos seguidos não gasta o limite da API.
- **Escolha do proxy** na tela do jogo (Automático/`dxgi.dll`, `version.dll`, `winmm.dll`, `winhttp.dll`), salva por jogo e mantida no "Atualizar todos". Só nomes da lista são aceitos.
- **Perfis por jogo** em [`profiles.json`](profiles.json): proxy, anti-cheat e dicas. O app baixa a versão da `main` (cache de 1 dia) e usa a cópia embutida offline. Proxies fora da lista são descartados.
- **Anti-cheat**: Easy Anti-Cheat, BattlEye e EA Javelin detectados pelos arquivos do jogo (e pelos perfis). O aplicar fica bloqueado até o usuário confirmar que vai jogar offline; o "⚡ Aplicar" rápido nunca passa direto.

## Fase 3 — ✔ concluída
- **Teste de fumaça no Windows**: `DLSS5-FrameGen.exe --smoke-test relatorio.txt` roda sem janela (placa, lojas, perfis, todos os planos e um aplicar/remover real). O CI e o workflow de release rodam o `.exe` final assim antes de publicar.
- **Assinatura**: o workflow de release assina o `.exe` com `signtool` quando os segredos `WINDOWS_PFX_BASE64` e `WINDOWS_PFX_PASSWORD` existem (veja [RELEASING.md](RELEASING.md)). Sem certificado, a release sai sem assinatura, como antes.
- **Atualização automática**: o app avisa quando há release nova e se atualiza com um clique (hash conferido, troca o próprio `.exe` e reabre).

## O que depende de fora do código
- **Certificado de assinatura**: a etapa está pronta; falta cadastrar um certificado (comercial, ou gratuito para open source via SignPath Foundation) nos segredos do repositório.
- **Teste com placas e jogos reais**: o CI não tem GPU; os relatos dos usuários (modelo de issue) alimentam os perfis.
