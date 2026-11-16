# Rust Control Station beta 0.5.0

Versão focada em **melhorias de interface** (revisão de todas as telas) e em **precisão no SpeedLOG**.

## Flight Data
- **HUD**: sem conexão mostra `HDG —` e fica esmaecido. O texto cresce junto com o HUD, a bateria (V, A, %) tem linha própria e há marcas fixas de roll. Continua arrastável e redimensionável.
- **Mapa**: botões de zoom no canto inferior direito, trilha com contorno (legível no mapa Streets) e alertas sempre acima do HUD.
- **Quick View**: os blocos mostram um lápis ao passar o mouse e abrem o seletor com Enter/Espaço.
- **Ações**: nos diálogos perigosos o foco começa em **Cancel**, então Enter não executa Terminate, force disarm ou reboot por engano. Desarmar em voo é tratado como perigoso. O resultado antigo some ao desconectar.

## Barra superior
- O botão **Reboot** não é mais cortado na janela mínima (1080 px). As estatísticas do link somem em janelas estreitas.

## Flight Plan e DroneCAN
- Dica de como adicionar pontos quando só existe o Home, campos da grade com rótulo e pontos de cerca/rally criados com o comando certo.
- Caixa "Assign node IDs" alinhada ao texto.

## SpeedLOG
- Tempo com milissegundos, gráficos mais altos e cursor com rótulo `t = …` e min/média/máx.
- `≈` marca valores de visões reduzidas (zoom para ver todas as amostras).
- **Navegação**: Backspace volta um zoom, setas deslocam a janela, a roda dá zoom (Ctrl+roda quando a lista rola).
- **Medição**: clique marca A, Shift+clique marca B, com Δ de cada traço, Δt e frequência. Esc limpa.
- Dois eixos Y quando há duas unidades no mesmo painel, pontos nas amostras com zoom alto e abas que não mudam de lugar.

## Limitações conhecidas
- Em Fence/Rally a lista de comandos ainda mostra os comandos de missão.
- SpeedLOG: quedas de log aparecem como linha reta; o valor no cursor de um traço lento pode estar até ~1 s defasado; sem zoom manual no eixo Y nem exportação CSV.
- Testado no navegador com backend simulado e na janela do app; ainda não validado com veículo real.
