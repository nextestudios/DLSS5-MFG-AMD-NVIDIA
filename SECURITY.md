# Segurança

O app baixa DLLs de terceiros e as coloca na pasta de jogos, então levamos segurança a sério.

- Todo download de release do GitHub é conferido pelo SHA-256 publicado pelo GitHub; se não bater, é apagado.
- Os DLLs `nvngx_*` extraídos do zip têm o CRC conferido.
- Cada release do app tem `SHA256SUMS.txt` e atestado de procedência (`gh attestation verify DLSS5-FrameGen.exe -R nextestudios/DLSS5-MFG-AMD-NVIDIA`).

## Reportar uma vulnerabilidade

Não abra issue pública. Use **Security → Report a vulnerability** neste repositório (aviso privado). Respondemos em até 7 dias.

Problemas nos mods em si (OptiScaler, MFG Unlocker, DLSS Unlocked, NeuralHub, ReShade) devem ser reportados aos projetos originais.
