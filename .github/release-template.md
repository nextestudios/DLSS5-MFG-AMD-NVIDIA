## DLSS5-FrameGen {{VERSION}}

{{CHANGES}}

### Download

| Arquivo | Para quê |
|---|---|
| **`DLSS5-FrameGen.exe`** | O app. Roda direto, sem instalar. |
| `DLSS5-FrameGen-{{VERSION}}-windows-x64.zip` | O app com README, licença e CHANGELOG. |
| `SHA256SUMS.txt` | Hashes para conferir os arquivos. |

**Requisitos:** Windows 10/11 x64 · DirectX 12 · placa NVIDIA RTX/GTX, AMD RX 6000+ ou Intel Arc · internet na primeira aplicação.

### Verificar o download

```
DLSS5-FrameGen.exe  {{SHA_EXE}}
```

No PowerShell: `Get-FileHash .\DLSS5-FrameGen.exe` · Procedência do build (GitHub CLI): `gh attestation verify DLSS5-FrameGen.exe -R nextestudios/DLSS5-MFG-AMD-NVIDIA`

### Como atualizar

Baixe o novo `.exe` e substitua o antigo. Depois clique em **Atualizar todos** para levar as versões mais novas dos mods a todos os jogos já modados.

### Avisos

- O `.exe` não é assinado: o SmartScreen pode avisar na primeira vez (**Mais informações → Executar assim mesmo**).
- Não use em jogos online com anti-cheat (risco de banimento).
- Os mods (OptiScaler, MFG Unlocker, DLSS Unlocked, NeuralHub, ReShade) são baixados das fontes originais na hora de aplicar; cada um tem a própria licença.

**Mudanças completas:** {{COMPARE}}
