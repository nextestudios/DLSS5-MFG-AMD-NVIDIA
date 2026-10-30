# Changelog

Todas as mudanças relevantes deste projeto ficam aqui.
O formato segue o [Keep a Changelog](https://keepachangelog.com/pt-BR/1.1.0/) e as versões seguem o [Versionamento Semântico](https://semver.org/lang/pt-BR/) (veja [RELEASING.md](RELEASING.md)).

## [Unreleased]

## [1.3.0] - 2026-10-30

### Adicionado
- **Instalador** `DLSS5-FrameGen-Setup.exe` (Inno Setup, em português): atalho no Menu Iniciar e opcional na área de trabalho, desinstalação pelo Windows. Instala por usuário em `%LOCALAPPDATA%\Programs\DLSS5-FrameGen`, sem Administrador, para a atualização automática continuar funcionando.
- **Versão portátil** `DLSS5-FrameGen-vX.Y.Z-portable-win-x64.zip`: com `portable.txt` ao lado do `.exe`, cache, jogos adicionados e configurações ficam na pasta `GpuMods` ao lado dele.
- O CI e a release geram o instalador, instalam em silêncio, rodam o teste de fumaça no app instalado e desinstalam; a versão portátil também é testada.

## [1.2.0] - 2026-10-30

### Corrigido
- **DLSS 5 na AMD:** "NeuralHub não encontrado depois da instalação". O app procurava o `neuralhub.exe` em pastas fixas; agora usa o registro de desinstalação do Windows, que diz onde o instalador colocou o app. Se a instalação silenciosa não deixar o NeuralHub no lugar, o instalador normal abre.
- **Capas não apareciam no Windows:** o caminho `file://C:\...` era lido como pasta de rede. As capas agora vêm da API oficial da loja Steam (`IStoreBrowseService/GetItems`), que também traz as capas novas que a Steam guarda em pastas com hash.

### Alterado
- **Downloads mais rápidos:** arquivos grandes do GitHub são baixados em 8 partes ao mesmo tempo, com progresso em MB no cartão de instalação. No teste, os DLLs do DLSS 5 (367 MB) levaram 8,7 s; uma conexão só leva 13 s.
- **O app se atualiza sozinho:** quando sai uma release nova, ele baixa, confere o SHA-256 e reabre, sem clique. Confere ao abrir e a cada 6 horas. A opção **Atualizar o app sozinho** na barra lateral desliga.
- Layout da biblioteca: espaço igual entre as linhas, sem o selo da loja repetido em cada capa, status do mod ("✔ DLSS 5 4x") dentro da capa, botão DLSS 5 com ✔ no lugar do "●", botões da barra lateral alinhados e caminho da pasta em uma linha.

## [1.1.0] - 2026-10-30

### Adicionado
- **Biblioteca de jogos com capas**, no estilo do NeuralHub: barra lateral com as lojas (Todos, Modados, Steam, Epic, Xbox…, Adicionados) e grade de capas reais da Steam. Jogos de outras lojas ganham a capa quando existem na Steam com o mesmo nome; os demais ganham uma capa gerada.
- **Aplicar com um clique:** passe o mouse na capa e clique em ⚡ Aplicar (usa o preset do topo). Clique direito para abrir, remover mods ou abrir a pasta.
- **Tela do jogo:** fundo e logo do jogo, recursos, caminho da imagem, linha do tempo de quadros e o botão grande de aplicar.
- **Adicionar jogo por pasta ou arrastando** a pasta/o `.exe` para a janela; o jogo fica salvo na biblioteca.
- Progresso da instalação num cartão flutuante, com o log completo opcional.
- Teste que renderiza a interface sem tela e gera os prints do README.
- **Escolha do proxy** por jogo ("Carregar como": `dxgi.dll`, `version.dll`, `winmm.dll`, `winhttp.dll`), mantida no "Atualizar todos".
- **Perfis por jogo** (`profiles.json`, baixado do repositório com cache de 1 dia): proxy, anti-cheat e dicas.
- **Bloqueio de anti-cheat**: Easy Anti-Cheat, BattlEye e EA Javelin detectados pelos arquivos do jogo; aplicar só depois de confirmar jogo offline.
- **Atualização automática do app** com hash conferido.
- Modo `--smoke-test`: o `.exe` final é testado num Windows no CI e antes de cada release.
- Assinatura do `.exe` no workflow de release quando há certificado configurado.
- 50 testes automáticos (antes 15): todas as combinações de placa × DLSS 5 × multiplicador, download parcial de zip contra um servidor HTTP local, backup/remoção, lojas e interface.
- Teste de ponta a ponta com os pacotes reais (instala, reinstala e remove de um jogo falso) e workflow semanal que avisa quando um mod muda o formato das releases.
- Versão do app no topo da janela.
- Padrão de releases: validação de versão e CHANGELOG, zip com documentação, `SHA256SUMS.txt`, atestado de procedência do build e notas com modelo fixo.
- Script `scripts/release.sh`, `RELEASING.md`, `CONTRIBUTING.md`, `SECURITY.md` e modelo de pull request.

### Corrigido
- Placas virtuais (Microsoft Basic Render Driver em VMs) eram classificadas como Intel Arc; agora o app pede para escolher o modelo. Achado pelo teste de fumaça no Windows.
- Seletor de placa sem nome para leitores de tela.
- Placas profissionais NVIDIA (RTX 2000/4000/6000 Ada e RTX PRO Blackwell) eram classificadas como GTX ou RTX 20/30.

### Alterado
- Chamadas à API do GitHub usam `GITHUB_TOKEN` quando ele existe (CI) e ficam em cache por 1 hora.

## [1.0.0] - 2026-10-29

Primeira versão pública.

### Adicionado
- Detecção da placa de vídeo (NVIDIA RTX 20–50 e GTX, AMD RX 6000–9000, Intel Arc), escolhendo a placa dedicada em notebooks.
- Biblioteca de jogos automática: Steam, Epic, GOG, Ubisoft, EA, Xbox e Rockstar, com busca e filtro por loja.
- DLSS 5 (Neural Rendering) em NVIDIA RTX via DLSS Unlocked e em AMD via NeuralHub, instalado e atualizado em modo silencioso.
- Download automático dos `nvngx_dlssnr/dlss/dlssd/dlssg.dll` mais novos, só dos arquivos necessários (download parcial do zip).
- Frame Generation de 2x a 6x: MFG Unlocker em NVIDIA; XeSS-FG + MFG Unlocker no OptiScaler 10 em AMD e Intel; FSR FG 2x.
- Sempre a última release de cada componente, com cache para uso offline.
- Backup dos arquivos do jogo, remoção completa e "Atualizar todos".
- Interface com a cor da marca da placa, caminho da imagem e linha do tempo animada de quadros reais × gerados.

### Segurança
- SHA-256 de todo download conferido contra o GitHub.

[Unreleased]: https://github.com/nextestudios/DLSS5-MFG-AMD-NVIDIA/compare/v1.3.0...HEAD
[1.3.0]: https://github.com/nextestudios/DLSS5-MFG-AMD-NVIDIA/compare/v1.2.0...v1.3.0
[1.2.0]: https://github.com/nextestudios/DLSS5-MFG-AMD-NVIDIA/compare/v1.1.0...v1.2.0
[1.1.0]: https://github.com/nextestudios/DLSS5-MFG-AMD-NVIDIA/compare/v1.0.0...v1.1.0
[1.0.0]: https://github.com/nextestudios/DLSS5-MFG-AMD-NVIDIA/releases/tag/v1.0.0
