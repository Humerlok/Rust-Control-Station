# Rust Control Station — Beta

Versão beta para testes. Pode ter erros: **não use em voo real sem validar antes.**

## Baixar

Abra a página de Releases do repositório e baixe o arquivo do seu sistema:

https://github.com/humerlok/rust-control-station/releases

| Sistema | Arquivo |
|---|---|
| Windows | `*-setup.exe` (ou `.msi`), x64 ou arm64 |
| Debian / Ubuntu | `.deb` |
| Fedora | `.rpm` |
| Outras distros Linux | `.AppImage` |

Cada versão tem instaladores **x86_64** (PCs comuns) e **ARM64** (Raspberry Pi, Jetson, Windows em ARM). O nome do arquivo indica a arquitetura: `x64`/`amd64`/`x86_64` ou `arm64`/`aarch64`/`ARM64`.

## Instalar

**Windows:** execute o `.exe`. O SmartScreen vai avisar "app desconhecido", porque o instalador não é assinado. Clique em *Mais informações* e depois em *Executar assim mesmo*. O WebView2 já vem no Windows 10/11 atualizado.

**Debian / Ubuntu:**
```bash
sudo apt install ./Rust.Control.Station_*.deb
```

**Fedora:**
```bash
sudo dnf install ./Rust.Control.Station-*.rpm
```

**AppImage:**
```bash
chmod +x Rust.Control.Station_*.AppImage
./Rust.Control.Station_*.AppImage
```

**Linux, porta serial:** para conectar por USB, adicione seu usuário ao grupo da serial e faça logout/login:
```bash
sudo usermod -aG dialout $USER      # Debian/Ubuntu
sudo usermod -aG dialout,lock $USER # Fedora
```

## Testar sem drone

Em *Connect*, escolha o tipo **Replay (log)**. Ele precisa de um arquivo `.BIN` do ArduPilot, que não vem no instalador.

## O que testar

- Conexão (serial, TCP, UDP) e tela Flight Data
- Parâmetros: baixar, editar, salvar e carregar `.param`
- Calibração, Extended Tuning e MAVLink Inspector
- Planejamento de missão: enviar e baixar
- MAVFTP e gravação de firmware (**só testado com bootloader falso, não grave em placa importante**)

## Novidades desta versão

- **Salvar arquivo**: Download no MAVFTP, *Save file* no Flight Plan e *Save to file* em Parameters abrem uma janela para escolher onde salvar.
- **MAVFTP**: os atalhos em *Places* agora mostram só as pastas que existem no veículo.
- **Quick View**: as opções do Mission Planner, agrupadas por categoria (inclui o 2º GPS: `GPS2 satellites`, `GPS2 fix type`, `GPS2 HDOP`...).
- **HUD**: modo de voo em destaque; fica vermelho por 2 s quando muda.
- **Mensagens**: alertas, erros críticos e PreArm aparecem numa faixa colorida na parte de baixo do mapa (vermelho, laranja, amarelo).
- **Vários veículos/componentes**: o seletor ao lado da conexão lista todos os SYSID/componentes (ex.: `1-1 QUADROTOR`, `1-154 GIMBAL`), e um aviso aparece quando há mais de um SYSID.

## Reportar problemas

Abra uma issue em https://github.com/humerlok/rust-control-station/issues com:
- versão (nome do arquivo baixado) e sistema operacional
- passos para reproduzir
- placa/firmware usados e o que apareceu na tela

## Gerar uma nova versão (mantenedor)

1. Ajuste `version` em `rust/Cargo.toml`, `rust/desktop/src-tauri/tauri.conf.json` e `rust/desktop/ui/package.json`.
2. Faça commit e envie a tag:
   ```bash
   git tag beta-0.1.0
   git push origin beta-0.1.0
   ```
3. Acompanhe na aba *Actions*. Em cerca de 15–25 min os instaladores aparecem em *Releases*, marcados como pré-lançamento.

Para só testar o build sem publicar, use *Actions → Beta release → Run workflow*. Os arquivos ficam como artefatos da execução.
