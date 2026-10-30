# Contribuindo

1. Abra uma issue antes de mudanças grandes.
2. Crie uma branch a partir da `main` (`feat/...`, `fix/...`).
3. Antes do PR:
   ```sh
   cargo clippy --all-targets -- -D warnings
   cargo test                      # rápido, sem internet
   cargo test -- --ignored         # baixa os pacotes reais e aplica num jogo falso
   ```
4. Toda lógica nova vem com teste. Regras de "o que instalar" ficam em `src/plan.rs` (puro, sem I/O) e são cobertas pelo teste `invariants_for_every_combination`.
5. Adicione uma linha em `CHANGELOG.md` na seção `[Unreleased]`.

## Perfis de jogos

Sabe que um jogo precisa de outro proxy, tem anti-cheat ou tem uma pegadinha? Edite [`profiles.json`](profiles.json):

```json
{ "names": ["Nome do Jogo"], "steam_appid": 123, "exes": ["Jogo.exe"], "proxy": "version.dll", "anticheat": "BattlEye", "notes": ["Dica curta."] }
```

`proxy` aceita só `dxgi.dll`, `version.dll`, `winmm.dll` ou `winhttp.dll`. O teste `embedded_profiles_parse_and_are_safe` confere o arquivo.

Releases: veja [RELEASING.md](RELEASING.md).
