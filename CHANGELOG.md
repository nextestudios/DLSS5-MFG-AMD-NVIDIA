# Changelog

## [1.0.0] - 2026-10-29

Primeira versão pública.

### Recursos
- Detecta a placa de vídeo (NVIDIA RTX 20–50 e GTX, AMD RX 6000–9000, Intel Arc) e escolhe a placa dedicada em notebooks.
- Biblioteca de jogos automática: Steam, Epic, GOG, Ubisoft, EA, Xbox e Rockstar, com busca e filtro por loja.
- DLSS 5 (Neural Rendering) em NVIDIA RTX via DLSS Unlocked e em AMD via NeuralHub, instalado e atualizado em modo silencioso.
- Download automático dos `nvngx_dlssnr/dlss/dlssd/dlssg.dll` mais novos, só dos arquivos necessários (download parcial do zip).
- Frame Generation de 2x a 6x: MFG Unlocker em NVIDIA; XeSS-FG + MFG Unlocker no OptiScaler 10 em AMD e Intel; FSR FG 2x.
- Sempre a última release de cada componente, com cache para uso offline.
- SHA-256 de todo download conferido contra o GitHub.
- Backup dos arquivos do jogo, remoção completa e "Atualizar todos".
- Interface nova: cor da marca da placa, caminho da imagem e linha do tempo animada de quadros reais × gerados.
