# Como lançar uma versão

## Versionamento

O projeto segue [SemVer](https://semver.org/lang/pt-BR/) (`MAIOR.MENOR.CORREÇÃO`), com estas regras para o app:

| Muda | Quando |
|---|---|
| **MAIOR** (2.0.0) | Muda o que fica na pasta do jogo de forma incompatível (manifesto, estado, nomes de arquivo) ou o app deixa de suportar uma placa/loja. |
| **MENOR** (1.1.0) | Recurso novo: placa, loja, mod, multiplicador, tela. |
| **CORREÇÃO** (1.0.1) | Correção de bug, ajuste para um mod que mudou o formato da release, texto. |
| **Pré-release** (1.1.0-rc.1) | Versão para teste. Sai marcada como *pre-release*, não aparece como "Latest" e usa as notas de `[Unreleased]` (o CHANGELOG só é datado na versão final). |

## Passo a passo

1. Toda mudança entra por pull request com uma linha em `CHANGELOG.md`, na seção `[Unreleased]`, em **Adicionado / Alterado / Corrigido / Removido / Segurança**. O CI bloqueia PR sem CHANGELOG.
2. Com a `main` atualizada e o CI verde:
   ```sh
   scripts/release.sh 1.1.0          # ou 1.1.0-rc.1 (pré-release pode sair de qualquer branch)
   git push && git push origin v1.1.0
   ```
   O script confere a versão, a árvore limpa e a seção `[Unreleased]`, atualiza `Cargo.toml`/`Cargo.lock`, data a seção do CHANGELOG, roda os testes, commita `release: v1.1.0` e cria a tag.
3. A tag dispara `.github/workflows/release.yml`, que:
   - confere se a tag bate com o `Cargo.toml` e se o CHANGELOG tem a seção;
   - roda clippy (`-D warnings`) e todos os testes no Windows;
   - roda o `.exe` final em modo `--smoke-test` (aplicar/remover real num jogo falso);
   - assina o `.exe`, se houver certificado;
   - gera o instalador `DLSS5-FrameGen-Setup.exe` (Inno Setup, [`installer/`](installer/)), instala, roda o teste de fumaça no app instalado e desinstala; testa também o modo portátil;
   - gera `DLSS5-FrameGen.exe`, `DLSS5-FrameGen-vX.Y.Z-portable-win-x64.zip` (com `portable.txt`, README, LICENSE, CHANGELOG) e `SHA256SUMS.txt`;
   - cria o atestado de procedência do build (`gh attestation verify`);
   - publica a release com as notas no modelo `.github/release-template.md`.

## Assinatura do `.exe`

O workflow assina o `.exe` com `signtool` quando estes segredos existem em **Settings → Secrets and variables → Actions**:

| Segredo | Conteúdo |
|---|---|
| `WINDOWS_PFX_BASE64` | O certificado `.pfx` em Base64 (`[Convert]::ToBase64String([IO.File]::ReadAllBytes("cert.pfx"))`) |
| `WINDOWS_PFX_PASSWORD` | A senha do `.pfx` |

Sem eles a release sai sem assinatura (e o SmartScreen avisa na primeira execução). Projetos open source podem conseguir certificado gratuito pela SignPath Foundation.

## O que toda release contém

- Título `DLSS5-FrameGen vX.Y.Z`
- Mudanças da versão (do CHANGELOG)
- Tabela de downloads e requisitos
- SHA-256 do `.exe` e como verificar (hash e procedência)
- Como atualizar e avisos (SmartScreen, anti-cheat, licenças dos mods)
- Link para a comparação com a versão anterior

## Se algo der errado

- **Workflow falhou antes de publicar:** corrija, apague a tag (`git push --delete origin vX.Y.Z && git tag -d vX.Y.Z`) e rode o script de novo.
- **Release publicada com problema:** não reescreva a tag; lance uma CORREÇÃO (`X.Y.Z+1`).
- **"Fontes externas" falhou** (workflow semanal): um mod mudou o nome ou o formato da release. Ajuste o `pick` em `install.rs`/`net.rs` e lance uma CORREÇÃO.
