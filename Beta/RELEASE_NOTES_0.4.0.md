# Rust Control Station beta 0.4.0

Novidade principal: **SpeedLOG**, o visualizador de logs do ArduPilot, agora dentro do RCS em **Tools > SpeedLOG**, com download de logs direto do drone conectado.

## Novo: Tools > SpeedLOG

Visualizador offline de logs DataFlash (`.bin`). Nada é enviado para a internet e nada é plotado até você escolher.

- **Abrir log**: do disco (botão *Open log…*) ou **direto do drone** (botão *Vehicle logs…*, com o drone conectado).
- **Gráficos**: marque campos na barra lateral para plotá-los. Cada painel tem um eixo Y e até 8 traços. Instâncias aparecem separadas (`IMU[0]`, `IMU[1]`).
- **Zoom** arrastando no gráfico: o eixo do tempo é compartilhado entre os painéis. Duplo clique volta para o log inteiro.
- **Cursor sincronizado** entre os painéis, com o valor de cada traço naquele instante e o mínimo, a média e o máximo da janela visível.
- **Modos de voo** pintados no fundo, com o nome de cada trecho.
- **Presets**: atitude, altitude, bateria, vibração, motores, GPS e RC, montados só com os campos que o log realmente tem.
- **Parâmetros**: lista os valores do log, marca os que mudaram durante o voo e mostra o significado do valor (nome da opção ou bits ativos).
- **Mensagens**: `MSG`, `ERR` e `EV`, com os erros e eventos traduzidos para texto. Clicar numa linha marca aquele instante em todos os gráficos.
- **Descrições** de cada mensagem, campo e parâmetro, em inglês e português (botão EN/PT), direto da documentação do ArduPilot. Funciona offline.
- Atalhos: `/` busca, `P` novo painel, `M` modos de voo, `R` reset do zoom, `S` barra lateral.

### Logs do drone

- Lista os logs do drone por **MAVFTP** (`/APM/LOGS`) quando disponível e, senão, pelo protocolo de log do MAVLink (`LOG_REQUEST_LIST`).
- **Open** baixa e abre o log no SpeedLOG. **Save…** salva uma cópia onde você escolher.
- Barra de progresso com velocidade e botão para cancelar. Logs já baixados ficam em cache.
- No modo Replay não há logs a baixar.

## Outras mudanças

- O leitor de logs `.BIN` ganhou índice por tipo de mensagem, leitura de uma coluna só (mais rápido em logs grandes), unidades e multiplicadores e a separação de instâncias.
- Documentação do projeto atualizada (`CLAUDE.md`, `HANDOFF.md`, `docs/ARCHITECTURE.md`).

## Limitações conhecidas

- **Ainda não testado com um `.BIN` real no app nem com download de um drone de verdade.** Foi verificado com testes automáticos, logs sintéticos, um drone simulado e a interface em modo simulado. Se você testar, por favor reporte o que encontrar.
- Campos inteiros com multiplicador aparecem na unidade base (por exemplo `m` em vez de `cm`), diferente do SpeedLOG web.
- O log inteiro é mantido na memória, então logs muito grandes podem pesar.
- Hardware Report e PID Review (FFT, resposta ao degrau) do SpeedLOG original ainda não foram portados.
- Os instaladores não são assinados: o Windows SmartScreen avisa na instalação.

## Instaladores

| Sistema | Arquivos |
|---|---|
| Windows x64 | `Rust.Control.Station_0.4.0_x64-setup.exe`, `Rust.Control.Station_0.4.0_x64_en-US.msi` |
| Windows ARM64 | `Rust.Control.Station_0.4.0_arm64-setup.exe`, `Rust.Control.Station_0.4.0_arm64_en-US.msi` |
| Linux x86_64 | `.deb`, `.rpm`, `.AppImage` (`amd64` / `x86_64`) |
| Linux ARM64 | `.deb`, `.rpm`, `.AppImage` (`arm64` / `aarch64`) |

Instruções de instalação e de reporte de problemas em `Beta/BETA.md`.

**Full Changelog**: https://github.com/Humerlok/Rust-Control-Station/compare/beta-0.3.0...beta-0.4.0
