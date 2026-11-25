# Plano de melhorias de interface (revisão de design, 2026-11-16)

Resultado de uma revisão de todas as telas do Rust Control Station feita no navegador com o backend
simulado (`npm run dev` em `rust/desktop/ui`, URL `/?mock=1`), em 1440x860 e no tamanho mínimo da
janela (1080x680, definido em `rust/desktop/src-tauri/tauri.conf.json`).

Depois de escrito, cada pacote foi conferido contra o código por um agente de leitura (arquivos,
seletores, trechos citados, APIs das bibliotecas instaladas). As instruções abaixo já incluem essas
correções. Os números de linha valem para o commit `ffd15f815`; se o arquivo mudou, procure pelo
trecho citado.

Este documento é escrito para ser executado por agentes. Cada **pacote** é independente e toca um
conjunto próprio de arquivos, então pacotes diferentes podem ser feitos em paralelo. Dentro de um
pacote, faça as tarefas na ordem.

## Regras para quem executa

1. Leia `CLAUDE.md` na raiz antes de começar. Todo o trabalho é em `rust/desktop/ui/src`, exceto a
   tarefa J4, que também toca Rust.
2. Textos da interface ficam em **inglês** (o app inteiro é em inglês). Não traduza.
3. Use as variáveis de cor do tema (`var(--nord*)`, `var(--text)`, `var(--danger)` etc., definidas
   em `rust/desktop/ui/src/theme.css`). Não invente cores novas, salvo onde o plano der o valor.
4. `src/api.ts` é o único lugar que chama `invoke`/`listen`. React não ganha lógica de protocolo.
5. Não mude comportamento que o plano não pede. Não refatore arquivos vizinhos.
6. Edite arquivos com a ferramenta de edição, não com `Set-Content` do PowerShell (ele grava em
   ANSI e corrompe caracteres como `·` e `≈`).
7. Depois de cada pacote, rode em `rust/desktop/ui`:
   ```powershell
   npm run build
   npm run lint
   ```
   Os dois devem passar sem erro. Se tocou em Rust (J4), rode também em `rust/`:
   ```powershell
   cargo fmt --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   ```
8. Verifique visualmente com `npm run dev` e `http://localhost:5173/?mock=1`. O mock já abre
   conectado, armado e voando a cerca de 42 m. Cada tarefa tem um item **Aceite** que diz o que
   conferir. Se o aceite não passar, a tarefa não está pronta.
9. Um commit por pacote, no formato do repositório: `desktop: <assunto no imperativo>`.
10. Se algo no código não bater com o que o plano descreve, procure pelo trecho citado com busca de
    texto. Se ainda assim não achar, pare e relate em vez de adivinhar.

## Prioridades

- **P1**: segurança de voo ou controle inacessível. Fazer primeiro.
- **P2**: clareza e precisão no uso diário.
- **P3**: acabamento.

| Pacote | Área | Arquivos | Prioridade |
|---|---|---|---|
| A | HUD | `components/Hud.tsx`, `app.css` (regras `.hud*`) | P1/P2 |
| B | Barra superior | `app.css` (regras novas logo após a linha 20) | P1 |
| C | Ações e diálogos | `components/ActionsPanel.tsx`, `components/ConfirmDialog.tsx` | P1 |
| D | Quick View | `components/QuickView.tsx`, `components/QuickPicker.tsx`, `app.css` (regras `.quick-*`) | P2 |
| E | Mapa de voo | `components/FlightMap.tsx`, `app.css` (regra `.message-banner`) | P2 |
| F | Flight Plan | `components/plan/PlanGrid.tsx`, `components/plan/missionModel.ts`, `views/FlightPlan.tsx`, `mockMission.ts` | P2/P3 |
| G | DroneCAN | `dronecan.css` | P3 |
| J | SpeedLOG (precisão) | `components/speedlog/PlotPanel.tsx`, `views/SpeedLog.tsx`, `speedlog.css`, `types.ts`, `mockSpeedLog.ts`, e para J4 `rust/crates/speedlog/src/series.rs` e `types.rs` | P2 |

Os pacotes A, B, D e E editam `app.css`, mas em regras diferentes. Se rodarem em paralelo, cada
agente só mexe nas regras listadas para o seu pacote.

---

## Pacote A: HUD

Arquivos: `rust/desktop/ui/src/components/Hud.tsx` e as regras `.hud`, `.hud-type`, `.hud-mode`,
`.hud-readouts` em `rust/desktop/ui/src/app.css`. Atenção: essas regras não estão juntas. `.hud`
está na linha 72, `.hud-type` na 74, `.hud-mode` na 75 e `.hud-readouts` mais abaixo, na 88, depois
das regras `.banner-*`.

O que já funciona e deve continuar funcionando: arrastar pela barra do topo, redimensionar pelo
canto, duplo clique na barra para voltar ao padrão, posição salva em `localStorage["hud.layout"]`.

### A1 (P1). Sem link, o HUD mostra dados que parecem reais

Problema: desconectado, o HUD mostra `HDG 0° N` e um horizonte nivelado, como se fosse leitura
válida. Os outros campos já mostram `—`.

Mudança em `Hud.tsx`:
- No readout de heading, quando `live` for `false`, mostrar `HDG —` (sem grau, sem cardeal).
- No `<div className="hud" ...>`, usar `className={live ? "hud" : "hud offline"}`.

Mudança em `app.css` (regra nova, logo depois da regra `.hud`):
```css
.hud.offline svg { opacity: 0.35; }
```

Aceite: clicar em Disconnect na barra superior. O HUD mostra `NO LINK`, `HDG —`, e a fita de rumo e
o horizonte ficam esmaecidos. Clicar em Connect volta ao normal.

### A2 (P2). Texto não acompanha o tamanho do HUD

Problema: ao aumentar o HUD (por exemplo para 400 px de largura), o horizonte cresce mas o modo de
voo continua em 20 px e os readouts em 11 px.

Mudança em `app.css`. **Edite as regras existentes no lugar; não cole um bloco novo** (uma regra nova
colada perto da linha 72 perderia para a `.hud-readouts` original da linha 88):
1. Na regra `.hud { position: absolute; ...` adicionar `container-type: inline-size;`.
2. Em `.hud-type`, trocar `font-size: 11px` por `font-size: clamp(11px, 5.2cqw, 20px)`.
3. Em `.hud-mode`, trocar `font-size: 20px` por `font-size: clamp(20px, 9.5cqw, 44px)`.
4. Em `.hud-readouts`, trocar `font-size: 11px` por `font-size: clamp(11px, 5.2cqw, 22px)`.
5. Adicionar uma regra nova na linha logo depois de `.hud-type`:
   `.hud .badge { font-size: clamp(11px, 5.2cqw, 20px); }`
6. Não tocar na regra global `.badge` (linha 43).

Aceite: com o HUD em 210 px (padrão) nada muda visualmente. O texto só começa a crescer acima de
cerca de 230 px de largura. Arrastando o canto até cerca de 400 px, modo, readouts e o selo ARMED
ficam bem maiores (modo perto de 36 px). Arrastar e redimensionar continuam funcionando.

### A3 (P2). Linha da bateria quebra de forma estranha

Problema: na largura padrão, a bateria aparece como `BAT 22.4 V` numa linha e `18.5 A 78%` na
seguinte, desalinhada da grade de duas colunas.

Mudança em `Hud.tsx`: trocar o `<span>` da bateria por
```tsx
<span className="hud-bat">
  BAT {fmt(f.battery_voltage, 1)} V · {fmt(f.battery_current, 1)} A
  {f.battery_remaining !== undefined ? ` · ${fmt(f.battery_remaining)}%` : ""}
</span>
```

Mudança em `app.css` (regra nova, logo depois de `.hud-readouts`):
```css
.hud-bat { grid-column: 1 / -1; }
```
Não use `white-space: nowrap` aqui: na largura mínima do HUD (160 px) o texto sairia da caixa.

Aceite: em 210 px a bateria ocupa uma linha inteira abaixo de HDG/GS/ALT, sem quebra. Encolhendo o
HUD até o mínimo, nenhum texto sai da caixa (a linha pode quebrar num espaço).

### A4 (P2). Ponteiro da fita de rumo aponta para o lado errado

Problema: em `HeadingTape`, o triângulo amarelo (`M100 0 L95 6 L105 6 Z`) tem a ponta para cima,
apontando para fora da escala.

Mudança: trocar o `d` desse `<path>` por `M100 7 L95 0 L105 0 Z`.

Aceite: o triângulo amarelo no topo da fita aponta para baixo, para o tick central.

### A5 (P2). Não há escala de inclinação (roll)

Problema: no `Horizon` existe um ponteiro amarelo que gira com o roll, mas não há marcas fixas, então
não dá para ler o ângulo.

Mudança em `Hud.tsx`. Acima de `function Horizon`, adicionar:
```tsx
const ROLL_TICKS = [-60, -45, -30, -20, -10, 0, 10, 20, 30, 45, 60];
```
Dentro de `Horizon`, antes do `return`, adicionar:
```tsx
  const rollTicks = ROLL_TICKS.map((deg) => {
    const rad = (deg * Math.PI) / 180;
    const outer = deg % 30 === 0 ? 99 : 96;
    return (
      <line
        key={deg}
        x1={100 + 92 * Math.sin(rad)}
        y1={100 - 92 * Math.cos(rad)}
        x2={100 + outer * Math.sin(rad)}
        y2={100 - outer * Math.cos(rad)}
        stroke="var(--nord4)"
        strokeWidth={1.5}
      />
    );
  });
```
No JSX, inserir `{rollTicks}` numa linha própria logo depois do anel
`<circle cx={100} cy={100} r={92} fill="none" stroke="var(--nord4)" strokeWidth={2} />`, como filho
direto do `<svg>`. **Não** dentro de `<g clipPath="url(#hud-clip)">` (seria cortado) e **não** dentro
de um `<g transform={`rotate(...)`}>` (giraria junto).

Depois, trocar o `<path>` do ponteiro que gira (o que tem `transform="translate(0 6)"`) por
`<path d="M100 10 L95 20 L105 20 Z" fill="var(--nord13)" />`, mantendo em volta o
`<g transform={`rotate(${-roll} 100 100)`}>`.

Aceite: aparecem 11 marcas fixas por fora do anel, na metade de cima. O ponteiro amarelo fica por
dentro do anel e percorre as marcas. O veículo simulado só inclina cerca de ±18°, então o ponteiro
oscila entre a marca do topo e um pouco antes das marcas de ±20°.

---

## Pacote B: barra superior

Arquivo: `rust/desktop/ui/src/app.css`. Só regras novas; não edite as regras existentes
`.link-pill`, `.badge`, `.stats` nem `.topbar`.

### B1 (P1). Botão Reboot fica fora da tela na janela mínima

Problema medido em 1080x680: o conteúdo da barra soma 1143 px numa barra de 1080 px. O botão
**Reboot** é cortado e o seletor de veículo encolhe para 18 px (só a seta). A causa é o seletor de
conexão, que cresce até a largura da opção mais longa (nomes de porta serial), e a falta de regras de
encolhimento.

Mudança: inserir estas regras logo depois da linha 20
(`.connect select, .connect input { height: 28px; }`):
```css
.connect { min-width: 0; }
.connect select:first-child { width: 190px; min-width: 110px; flex-shrink: 1; text-overflow: ellipsis; }
.connect input.port, .connect .baud, .connect button { flex: none; }
.topbar > select { min-width: 150px; flex: none; }
.topbar > .link-pill, .topbar > .badge, .topbar > button { flex: none; }
.topbar > .stats { flex-shrink: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; }
@media (max-width: 1240px) { .topbar > .stats { display: none; } }
```
Os seletores `.topbar > ...` só pegam filhos diretos da barra, então selos e botões de outras telas
não mudam. O tooltip do `.link-pill` já traz a descrição do link, então esconder as estatísticas em
janela estreita não perde informação essencial.

Aceite:
- Em 1080x680 (DevTools), conectado ao mock, rodar no console
  `document.querySelector('.topbar').scrollWidth <= document.querySelector('.topbar').clientWidth`.
  Deve dar `true`. O botão Reboot aparece inteiro e o seletor de veículo mostra `1-1 QUADROTOR`.
- Repetir para cada tipo de conexão. Os controles de conexão ficam desabilitados enquanto conectado,
  então para cada tipo: clicar em Disconnect, escolher o tipo no primeiro seletor, clicar em Connect,
  esperar cerca de 1 s até o selo mostrar ARMED e o seletor de veículo reaparecer, e rodar a expressão.
  Tipos: UDP, TCP, UDPCl, Replay (log), WS (digitar `ws://127.0.0.1:5760` no campo de URL antes, senão
  Connect só mostra erro) e a porta serial `COM3 — USB Serial Device (CubeOrange+)` (aparece o seletor
  de baud). Todos devem dar `true`. Em TCP e UDPCl o campo de porta continua mostrando 5 dígitos
  (`14550`); o campo de host/URL pode ficar mais estreito, o que é esperado. No fim, voltar o tipo
  para UDP.
- Em 1440 px as estatísticas (`62 pkt/s · lost N · 99%`) continuam visíveis e nada é cortado. O
  seletor de conexão passa a ter 190 px em qualquer tamanho de janela (nomes longos terminam em `…`;
  o nome completo aparece na lista aberta) e o seletor de veículo 150 px. Essas duas mudanças de
  largura são esperadas.

---

## Pacote C: ações e diálogos de confirmação

Arquivos: `rust/desktop/ui/src/components/ConfirmDialog.tsx` e
`rust/desktop/ui/src/components/ActionsPanel.tsx`.

### C1 (P1). Enter confirma ações perigosas sem querer

Problema: `ConfirmDialog` dá `autoFocus` ao botão de confirmação. Em diálogos como **Terminate
Flight?**, **Reboot autopilot?** com veículo armado ou **Force disarm?**, basta apertar Enter ou
Espaço logo depois de abrir para executar.

Mudança em `ConfirmDialog.tsx`. Logo abaixo de `const confirmClass = ...`, adicionar
`const focusCancel = tone === "danger";`. Trocar os dois botões por:
```tsx
<button onClick={onCancel} autoFocus={focusCancel}>{cancelLabel}</button>
<button className={confirmClass} onClick={onConfirm} autoFocus={!focusCancel}>
  {confirmLabel}
</button>
```
No comentário do topo do arquivo, trocar a frase sobre o foco por:
`Escape or a click on the backdrop cancels; the confirm button has focus, except for tone "danger", where Cancel has focus so Enter does not run the command.`

Isso vale para todos os diálogos `danger` do app (Force arm/disarm, ações perigosas, Reboot armado,
Calibration, DroneCAN update/restart armado, Firmware flash, Flight Plan > Clear on vehicle, Motor
Test, MAVFTP delete/replace). É o esperado e nenhum outro arquivo precisa mudar: nenhum desses
diálogos tem campo com foco próprio.

Aceite: em Flight Data, escolher `Terminate Flight` na lista e clicar em Do Action. Sem clicar em
mais nada, apertar Enter: o diálogo fecha e a linha de resultado do painel não mostra
`Terminate Flight: ...`. Para conferir o foco diretamente, com o diálogo aberto rodar no console
`document.activeElement?.textContent`: retorna `Cancel`. (O contorno azul de foco só aparece quando o
diálogo é aberto pelo teclado; aberto com o mouse, não aparece, e isso é normal.) Num diálogo não
perigoso (Flight Plan > Write com pelo menos um waypoint), a mesma expressão retorna `Write`.

### C2 (P1). Desarmar em voo aparece como aviso comum

Problema: o diálogo **Disarm motors?** usa `tone="warn"` e botão azul mesmo quando a mensagem diz
"Disarming in flight will drop it".

Mudança em `ActionsPanel.tsx`, no `ConfirmDialog` de `pending?.kind === "disarm"`: trocar
`tone="warn"` por `tone={telemetry.fields.alt > 2 ? "danger" : "warn"}`.

Aceite: com o mock recém-carregado, clicar em Disarm mostra diálogo com borda vermelha e botão
Disarm com borda e texto vermelhos (o fundo só fica vermelho ao passar o mouse). Enter fecha o
diálogo sem mostrar `Disarm: ...` na linha de resultado (por causa de C1).

### C3 (P2). Resultado antigo fica na tela depois de desconectar

Problema: a linha de resultado (por exemplo `Disarm: Denied: vehicle is flying`) continua visível
depois de desconectar e reconectar, parecendo um erro atual.

Mudança em `ActionsPanel.tsx`, logo depois do `useEffect` que chama `api.listActions()`:
```tsx
// A result from a previous connection is stale: clear it when the link goes up or down.
useEffect(() => {
  setResult(null);
}, [telemetry.connected]);
```

Aceite: clicar em Arm (a linha mostra `Arm: Accepted`). Clicar em Disconnect: a linha mostra
`Connect to a vehicle to use actions.`. Conectar de novo: mostra `Ready.`.

### C4 (P3). "Are you sure?" não diz o que vai acontecer

Mudança em `ActionsPanel.tsx`, no `ConfirmDialog` de `pending?.kind === "action"`: trocar
`message="Are you sure?"` por
`` message={`"${pending.action.label}" will be sent to the vehicle now.${pending.action.danger ? " This cannot be undone." : ""}`} ``
e `confirmLabel="Do it"` por `confirmLabel={pending.action.label}`.

Aceite: o diálogo de Terminate Flight mostra a frase nova e o botão de confirmação diz
`Terminate Flight`.

---

## Pacote D: Quick View

Arquivos: `rust/desktop/ui/src/components/QuickView.tsx`,
`rust/desktop/ui/src/components/QuickPicker.tsx` e as regras `.quick-tile` e `.quick-label` em
`app.css` (linhas 94 e 95).

### D1 (P2). Não dá para descobrir que os blocos são editáveis

Problema: trocar o valor de um bloco só funciona com duplo clique, e a única pista é um tooltip. Não
há mudança de cursor, realce ao passar o mouse nem acesso por teclado.

Mudança em `QuickView.tsx`:
- Importar `useRef` junto de `useEffect, useState`.
- Adicionar `const tiles = useRef<(HTMLDivElement | null)[]>([]);`.
- Adicionar, antes do `return`:
  ```tsx
  // Closing the picker returns keyboard focus to the tile it was opened from.
  const close = () => {
    if (editing !== null) tiles.current[editing]?.focus();
    setEditing(null);
  };
  ```
- No `<div className="quick-tile" ...>` adicionar:
  `ref={(el) => { tiles.current[index] = el; }}`, `role="button"`, `tabIndex={0}` e
  `onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); setEditing(index); } }}`.
  Manter o `onDoubleClick` e o `title`.
- No `<QuickPicker ...>`: `onCancel={close}` e, dentro de `onPick`, trocar `setEditing(null)` por
  `close()`.
- No comentário do topo do arquivo, trocar "double-click a tile to choose which field it shows" por
  "double-click a tile (or focus it and press Enter/Space) to choose which field it shows".

Mudança em `app.css`:
- Na regra `.quick-tile` existente, acrescentar `cursor: pointer; position: relative;`.
- Na regra `.quick-label` existente, acrescentar `padding-right: 14px;` (reserva espaço para o lápis).
- Regras novas, logo depois de `.quick-tile`:
  ```css
  .quick-tile:hover, .quick-tile:focus-visible { background: var(--panel-raised); }
  .quick-tile:focus-visible { outline: 1px solid var(--accent); outline-offset: -1px; }
  .quick-tile:hover::after, .quick-tile:focus-visible::after { content: "\270E"; position: absolute; top: 4px; right: 8px; font-size: 12px; color: var(--text-dim); }
  ```
  Use o escape `\270E` exatamente assim (é o lápis); não cole o caractere.

Mudança em `QuickPicker.tsx` (três lugares, o segundo com capitalização diferente):
- linha 2, comentário: `"Display This" dialog` vira `"Choose value to display" dialog`;
- linha 36: `aria-label="Display this"` vira `aria-label="Choose value to display"`;
- linha 37: `<h3>Display This</h3>` vira `<h3>Choose value to display</h3>`.

Aceite: passar o mouse num bloco realça o fundo e mostra um lápis (não um quadrado nem caracteres
estranhos). Com Cols = 8, o lápis fica ao lado do rótulo cortado, não por cima. Tab leva o foco aos
blocos, com contorno na cor de destaque, diferente do realce do mouse. Enter abre o seletor; Escape
fecha e o foco volta ao mesmo bloco. Duplo clique continua abrindo. Escolher um campo e Apply troca
o bloco.

---

## Pacote E: mapa de voo

Arquivos: `rust/desktop/ui/src/components/FlightMap.tsx` e a regra `.message-banner` em `app.css`
(linha 77, logo acima de `.banner-msg`).

Atenção para os aceites: o mapa do Leaflet é criado uma vez. Depois de editar `FlightMap.tsx`,
**recarregue a página (F5)**; o hot reload do Vite não reconstrói o mapa.

### E1 (P2). Trilha quase invisível no mapa Streets

Problema: a trilha é `#ebcb8b` com 3 px. No satélite funciona; no mapa Streets (fundo bege claro)
some.

Mudança em `FlightMap.tsx`: trocar a linha
`{track.length > 1 && <Polyline positions={track} pathOptions={{ color: "#ebcb8b", weight: 3 }} />}`
por
```tsx
        {track.length > 1 && (
          <>
            <Polyline positions={track} pathOptions={{ color: "#2e3440", weight: 6, opacity: 0.55 }} />
            <Polyline positions={track} pathOptions={{ color: "#ebcb8b", weight: 3 }} />
          </>
        )}
```
Não precisa de import novo.

Aceite: esperar alguns segundos para a trilha crescer e clicar em Streets. A trilha amarela tem borda
escura e é legível sobre áreas claras e verdes. Em Satellite continua boa.

### E2 (P2). HUD cobre o controle de zoom

Problema: o controle de zoom do Leaflet fica no canto superior esquerdo e o HUD, por padrão, no
inferior esquerdo. Em 1080x680 o HUD (361 px de altura num mapa de 443 px) fica por cima do botão
`−`.

Mudança em `FlightMap.tsx` (somente este mapa, não o `PlanMap`):
- Na linha de import do `react-leaflet`, acrescentar `ZoomControl`:
  `import { MapContainer, Marker, Polyline, TileLayer, useMap, useMapEvents, ZoomControl } from "react-leaflet";`
- No `<MapContainer ...>`, trocar o atributo solto `zoomControl` por `zoomControl={false}`.
- Adicionar `<ZoomControl position="bottomright" />` como primeiro filho do `<MapContainer>`, antes
  do `<TileLayer ...>`.

Mudança em `app.css`, na regra `.message-banner`: trocar `right: 12px;` por `right: 54px;`, para que
um banner largo não cubra os botões de zoom. Não mexer em `left` nem em `bottom`.

Aceite (depois de F5): os botões `+`/`−` aparecem uma única vez, no canto inferior direito, acima da
atribuição do mapa, e funcionam. A atribuição continua visível (é obrigatória). Dar duplo clique na
barra do HUD para colocá-lo na posição padrão: em 1080x680 ele não cobre nenhum controle. Os banners
de PreArm/Battery não ficam por cima dos botões de zoom.

### E3 (P2). Alertas ficam atrás do HUD

Problema: os banners de mensagem ficam abaixo do HUD (`z-index: 650`) quando o usuário move ou
aumenta o HUD para o centro. Um alerta crítico pode ficar escondido.

Mudança em `app.css`, na mesma regra `.message-banner`: trocar `z-index: 600` por `z-index: 700`.

Aceite: arrastar o HUD para cima dos banners no centro inferior do mapa. Os banners aparecem por cima
do HUD e o botão `×` deles continua clicável.

---

## Pacote F: Flight Plan

Arquivos: `rust/desktop/ui/src/components/plan/PlanGrid.tsx`,
`rust/desktop/ui/src/components/plan/missionModel.ts`, `rust/desktop/ui/src/views/FlightPlan.tsx` e
`rust/desktop/ui/src/mockMission.ts`.

### F1 (P2). Sem dica de como adicionar pontos quando só existe o Home

Problema: a dica "No items. Click the map to add a point…" só aparece com zero itens. Numa missão, a
linha Home sempre existe, então quem abre a tela vê só o Home e nenhuma instrução.

Mudança em `PlanGrid.tsx`: o JSX atual é um ternário
`items.length === 0 ? <p className="plan-empty dim">… : <VirtualList … />`. Envolver o ramo do
`VirtualList` num fragmento e acrescentar a dica depois dele:
```tsx
      ) : (
        <>
          <VirtualList ... sem alterações ... />
          {hasHome(kind) && items.length === 1 && (
            <p className="plan-empty dim">Click the map to add waypoints, or Read the mission from the vehicle.</p>
          )}
        </>
      )}
```
`hasHome` já está importado em `PlanGrid.tsx`; não adicione import.

Aceite: abrir Flight Plan no mock: a linha Home aparece e a dica aparece na parte de baixo do painel
da grade (a lista ocupa o espaço acima; essa posição é a esperada). Clicar no mapa adiciona o
waypoint 1 e a dica some.

### F2 (P3). Campos da grade sem nome acessível

Problema: os campos numéricos da grade são lidos como `textbox "0"`. Leitores de tela e automação
não distinguem P1 de LAT.

Mudança em `PlanGrid.tsx`:
1. `NumCell`: adicionar a prop obrigatória `label: string` (na desestruturação e no tipo das props) e
   `aria-label={label}` no `<input>`.
2. Em `PlanRow`, depois de `const frameKnown = …`, adicionar:
   ```tsx
   const who = home ? "home" : `item ${index}`;
   const aria = (label: string | null | undefined, fallback: string) => `${label ?? fallback} of ${who}`;
   ```
3. Mudar o helper `param` para receber o rótulo:
   `(field: "param1" | "param2" | "param3" | "param4", label: string | null | undefined, fallback: string) => <NumCell label={aria(label, fallback)} … />`
   e chamá-lo como `param("param1", info?.p1, "P1")` até `param("param4", info?.p4, "P4")`.
4. As três células de posição recebem `label={aria(info?.x, "Lat")}`, `label={aria(info?.y, "Lon")}`
   e `label={aria(info?.z, "Alt")}`.
5. O `<select>` de comando recebe `` aria-label={`Command of ${who}`} `` e o de frame
   `` aria-label={`Frame of ${who}`} ``. A linha Home não tem selects; nada a fazer nela.

O rótulo usa os nomes do comando **da própria linha** (`info`), não os do cabeçalho, que segue a
linha selecionada.

Aceite: no mock, clicar duas vezes no mapa e inspecionar o campo de latitude da linha 2:
`aria-label="Lat of item 2"`. O primeiro campo numérico da linha Home tem `aria-label` terminando em
`of home`.

### F3 (P2). Comandos de cerca e rally na lista de missão, e ponto de cerca criado como waypoint

Situação encontrada lendo o código (não testado em veículo real):
- O catálogo real (`rust/crates/ardupilot-meta/data/mavcmd.xml`, servido por `mission_commands` em
  `rust/desktop/src-tauri/src/mission_cmds.rs`) **não** tem comandos de item de cerca nem de rally.
  Só o mock os adiciona, em `rust/desktop/ui/src/mockMission.ts` (constante `CATALOG`, entradas 5001
  e 5100). Por isso eles aparecem na lista de comandos de uma missão no mock.
- Isso escondia um defeito real: `defaultCommand` em `missionModel.ts` devolve
  `catalog[0]?.id ?? wanted` para cerca/rally quando o id desejado não está no catálogo. Com o
  catálogo real, `catalog[0]` é WAYPOINT (16), então cada ponto novo de Fence ou Rally seria criado
  como NAV_WAYPOINT em vez de 5001/5100.

Mudança:
1. Em `mockMission.ts`, apagar as duas entradas de `CATALOG`:
   `cmd(5001, "FENCE_POLYGON_VERTEX_INCLUSION", …)` e `cmd(5100, "RALLY_POINT", …)`.
2. Em `missionModel.ts`, trocar o corpo de `defaultCommand` por uma linha:
   `return kind === "fence" ? CMD_FENCE_VERTEX : kind === "rally" ? CMD_RALLY_POINT : CMD_WAYPOINT;`
   (use os nomes reais das constantes já definidas no arquivo para 5001 e 5100). Remover o parâmetro
   `catalog` da função e ajustar o único chamador em `FlightPlan.tsx` (perto da linha 135) para
   `defaultCommand(k)`, removendo dali a variável que ficar sem uso.
3. Não adicionar nenhum filtro de ids na UI.

Aceite: no mock, Type = Mission, abrir a lista Command de uma linha: não há
`FENCE_POLYGON_VERTEX_INCLUSION` nem `RALLY_POINT`. Type = Fence, clicar no mapa: o Command da linha
mostra `CMD 5001`. Type = Rally: `CMD 5100`.

Limitação que fica (registrar no `HANDOFF.md`): em Fence/Rally a lista ainda oferece os comandos de
missão e os cabeçalhos caem em P1..P4/Lat/Lon/Alt, porque o backend não tem catálogo por tipo. A
correção certa é em `ardupilot-meta` (um parâmetro de tipo em `mission_commands`), não em React.

---

## Pacote G: DroneCAN

Arquivo: `rust/desktop/ui/src/dronecan.css`. Não edite `views/DroneCan.tsx` nem `app.css`.

### G1 (P3). Caixa de seleção solta acima do texto

Problema: `.dc-connect label { flex-direction: column; }` (linha 4) também pega o
`<label className="toggle">` de "Assign node IDs to new devices", empilhando a caixa acima do texto.

Mudança: na linha 22, substituir a regra inteira `.dc-connect .toggle { align-self: center; }` por
```css
.dc-connect label.toggle { flex-direction: row; align-items: center; gap: 6px; align-self: center; }
```
Não altere a linha 4: o rótulo "CAN bus" acima do seletor depende dela.

Aceite: na tela DroneCAN (não precisa conectar; a caixa aparece desabilitada), a caixa fica à
esquerda do texto, na mesma linha, na mesma altura do texto de status à direita
(`Not connected to a CAN bus`). O rótulo "CAN bus" continua acima do seletor. A caixa fica um pouco
acima do centro do botão Connect; isso é esperado, não ajuste.

---

## Pacote J: SpeedLOG, análise com precisão

Objetivo dado pelo usuário: **no SpeedLOG a pessoa precisa conseguir analisar logs com precisão.**
Hoje o gráfico funciona (zoom por arraste, cursor sincronizado, min/média/máx por traço), mas a
leitura é imprecisa em vários pontos. J1 a J3 são pequenas; J4 a J8 são funcionalidades. Faça na
ordem: J6 usa o `rangeRef` e J5 usa partes de J6 no handler de teclado, então faça **J6 antes de J5**.

Arquivos: `rust/desktop/ui/src/components/speedlog/PlotPanel.tsx`,
`rust/desktop/ui/src/views/SpeedLog.tsx`, `rust/desktop/ui/src/speedlog.css`. J4 toca também
`rust/crates/speedlog/src/series.rs`, `rust/crates/speedlog/src/types.rs`,
`rust/desktop/ui/src/types.ts` e `rust/desktop/ui/src/mockSpeedLog.ts` (o mock do SpeedLOG **não**
está em `mockBackend.ts`).

Para testar: `/?mock=1` > SpeedLOG > Open log… > expandir ATT > marcar Roll e Pitch. Os atalhos de
teclado são ignorados enquanto o foco está num campo, e as caixas da barra lateral são campos: depois
de marcar um traço, clique uma vez no gráfico antes de testar teclas.

### J1 (P2). Tempo com resolução de só 0,1 s

Problema: `formatTime` em `PlotPanel.tsx` sempre mostra `m:ss.s`. Logs do ArduPilot chegam a 400 Hz
(2,5 ms entre amostras); com zoom em 1 s, todos os ticks e o cursor mostram o mesmo décimo.

Mudança:
- `formatTime(seconds: number, digits = 1)`: usar `toFixed(digits)` e `padStart(3 + digits, "0")`.
- Eixo X (`axes[0].values`): escolher `digits` pela janela visível
  (`xRange.current[1] - xRange.current[0]`): maior que 60 s usa 1; até 60 s usa 2; até 5 s usa 3.
- Leitura do cursor (`setCursor`, `timeRef`): sempre 3 casas (`formatTime(time, 3)`).
- Os outros usos de `formatTime` (busque em `components/speedlog/` e em `SpeedLog.tsx`) não passam
  `digits` e continuam como hoje.

Aceite: com o log inteiro visível, os ticks são como `3:20.0`. Com zoom numa janela de 2 s, os ticks
têm 3 casas (`3:19.250`) e são todos diferentes. O tempo do cursor sempre tem milissegundos.

### J2 (P2). Gráfico usa só 240 px de altura

Problema: `.sl-plot { height: 240px; }` fixo. Com um painel numa tela de 860 px, sobram cerca de
500 px vazios e a resolução vertical é baixa.

Mudança:
- Em `SpeedLog.tsx`, no `<div className="sl-panels" ref={panelsRef}>`, adicionar
  `style={{ "--sl-count": panels.length } as React.CSSProperties}` (use o nome real do estado que
  guarda a lista de painéis).
- Em `speedlog.css`, na regra `.sl-plot`, trocar `height: 240px` por
  `height: clamp(240px, calc((100vh - 230px) / var(--sl-count, 1) - 70px), 75vh)`. Manter
  `min-height`, `resize: vertical` e o resto.

Aceite: com 1 painel, o gráfico ocupa quase toda a altura livre. Com 2 painéis (botão + Panel), os
dois cabem sem rolagem em 1440x860. Com 4 ou mais, cada um tem pelo menos 240 px e a lista rola. A
alça de redimensionar no canto do gráfico continua funcionando.

### J3 (P2). Leitura do cursor pequena e sem rótulo

Problema: o tempo do cursor aparece em 11,5 px, cinza, colado ao nome do painel; as estatísticas
aparecem como três números soltos (`-12.893 / 0.23308 / 12.957`) explicados só por tooltip.

Mudança:
- `PlotPanel.tsx`, no span do tempo: remover a classe `dim`, ficando
  `<span className="mono sl-panel-time" ref={timeRef} />`. (Sem isso a cor nova não vale: `.dim` em
  `theme.css` vem depois na cascata.)
- `PlotPanel.tsx`, em `setCursor`: escrever `t = ` antes do tempo
  (`` timeRef.current.textContent = time == null ? "" : `t = ${formatTime(time, 3)}` ``).
- `PlotPanel.tsx`, no chip: trocar o texto das estatísticas por
  `min {formatValue(st.min)} · avg {formatValue(st.mean)} · max {formatValue(st.max)}`.
- `speedlog.css`: a regra agrupada da linha 73 vira
  `.sl-panel-units, .sl-panel-loading { font-size: 11.5px; }` e entra uma regra nova
  `.sl-panel-time { font-size: 12.5px; color: var(--text-strong); }`. Em `.sl-chip-value` acrescentar
  `font-weight: 600;`. Em `.sl-chip-stats` trocar o tamanho para `11.5px`.

Aceite: passando o mouse no gráfico, o cabeçalho mostra `t = 3:19.125` em texto claro e cada chip
mostra o valor em negrito e `min … · avg … · max …`.

### J4 (P2). O valor no cursor pode esconder amostras

Problema: para janelas grandes, o backend reduz cada traço ao mínimo e ao máximo de cada balde
(`rust/crates/speedlog/src/series.rs`, função `view`; quando `t.len() <= buckets * 3` devolve as
amostras cruas). Os pontos devolvidos são amostras reais, mas há outras escondidas entre eles, e a
leitura do cursor não avisa.

Mudança:
- Rust, `series.rs`: antes de `let (out_t, out_v) = if ...`, calcular
  `let reduced = t.len() > buckets * 3;` e incluir `reduced` no `SeriesView` devolvido.
- Rust, `types.rs`: adicionar `pub reduced: bool` em `SeriesView` (o struct já usa
  `#[serde(rename_all = "camelCase")]`). No teste de contrato perto da linha 186, adicionar
  `reduced: false` ao literal e `assert_eq!(v["reduced"], false);`.
- UI, `types.ts`: adicionar `reduced: boolean` em `SpeedLogSeries`.
- UI, `mockSpeedLog.ts`, função `makeSeries`: no `return`, adicionar `reduced: step > 1` (o mock
  decima por passo; `step` já existe ali).
- `PlotPanel.tsx`: guardar os flags num `useRef<boolean[]>([])`, preenchido junto de `setStats`
  (`result.map((s) => s?.reduced ?? false)`). Em `setCursor`, quando o traço está reduzido, prefixar
  o valor com `≈ `. No `<li>` do chip, quando reduzido, usar
  `title="Reduced view (min/max per bucket). Zoom in to read every sample."`. Para o `title`
  atualizar, guarde os flags também em estado (`useState`) ou derive de `stats`.

Aceite: com o log inteiro visível, o valor no chip aparece como `≈ -10.27`. Depois de dar zoom até a
janela ter poucas amostras, o `≈` some. `cargo test -p speedlog` passa.

### J6 (P2). Navegação: só zoom por arraste e reset total

Problema: depois de dar zoom, não há como deslocar a janela nem voltar um passo; o texto inicial diz
"double click to go back", mas o duplo clique volta ao log inteiro.

Mudança em `SpeedLog.tsx`:
- Guardar a janela atual e o histórico em refs:
  ```tsx
  const rangeRef = useRef<[number, number] | null>(null);
  rangeRef.current = range;
  const history = useRef<Array<[number, number] | null>>([]);
  const zoom = (next: [number, number]) => { history.current.push(rangeRef.current); setRange(next); };
  const resetZoom = () => { history.current = []; setRange(null); };
  ```
- Em `loaded()`, junto de `setRange(null)`, fazer `history.current = [];`.
- No `switch` do handler de teclado, adicionar:
  ```tsx
  case "Backspace":
    if (history.current.length > 0) { e.preventDefault(); setRange(history.current.pop() ?? null); }
    break;
  case "ArrowLeft":
  case "ArrowRight": {
    const r = rangeRef.current;
    if (!r) break;
    e.preventDefault();
    const w = r[1] - r[0];
    let d = (e.key === "ArrowLeft" ? -1 : 1) * w * (e.shiftKey ? 0.5 : 0.1);
    d = Math.max(span[0] - r[0], Math.min(span[1] - r[1], d));
    setRange([r[0] + d, r[1] + d]);
    break;
  }
  ```
  e incluir `span` nas dependências do efeito (`[active, info, addPanel, span]`; use o nome real da
  variável que guarda o intervalo do log e que é passada como `span` aos painéis).
- Passar aos painéis a prop nova `onWheelZoom={setRange}` (zoom por roda não entra no histórico).
- Corrigir o texto de ajuda da tela inicial: trocar `double click to go back` por
  `double click to reset, Backspace to go back one zoom, arrows to pan, wheel (or Ctrl+wheel) to zoom`.

Mudança em `PlotPanel.tsx`:
- Em `Props`, adicionar `onWheelZoom: (range: [number, number] | null) => void;`.
- No hook `setSelect`, só o gráfico que o usuário arrastou pode reportar o zoom (com vários painéis,
  o uPlot dispara o hook em todos os gráficos sincronizados e o histórico ganharia entradas
  repetidas). Trocar a condição `if (u.select.width > 4) {` por
  `if (u.cursor.event != null && u.select.width > 4) {`. A linha
  `u.setSelect({ left: 0, top: 0, width: 0, height: 0 }, false);` continua fora do `if`.
- No efeito que cria o gráfico, junto do listener de `dblclick`, registrar:
  ```tsx
  const onWheel = (e: WheelEvent) => {
    const list = host.closest(".sl-panels");
    const scrollable = !!list && list.scrollHeight > list.clientHeight + 1;
    if (!e.ctrlKey && scrollable) return; // let the panel list scroll
    e.preventDefault();
    const rect = plot.over.getBoundingClientRect();
    const t = plot.posToVal(e.clientX - rect.left, "x");
    const [x0, x1] = xRange.current;
    const f = e.deltaY < 0 ? 0.8 : 1.25;
    const [s0, s1] = live.current.span;
    const a = Math.max(s0, t - (t - x0) * f);
    const b = Math.min(s1, t + (x1 - t) * f);
    live.current.onWheelZoom(a <= s0 && b >= s1 ? null : [a, b]);
  };
  plot.over.addEventListener("wheel", onWheel, { passive: false });
  ```
  e `plot.over.removeEventListener("wheel", onWheel);` na limpeza do efeito.

Aceite: com um painel, zoom por arraste duas vezes; Backspace volta à primeira janela; Backspace de
novo volta ao log inteiro. Com dois painéis plotados, um arraste e um Backspace bastam para voltar
(não precisa apertar duas vezes). Setas deslocam a janela sem passar do início/fim. Com um ou dois
painéis a roda aproxima no ponto do cursor; com painéis suficientes para a lista rolar, a roda rola
a lista e Ctrl+roda dá zoom. Os atalhos não disparam com o foco no campo de busca.

### J5 (P2). Não há como medir entre dois pontos

Problema: para medir período de oscilação, atraso entre desejado e obtido ou variação de um valor, o
usuário precisa anotar dois valores à mão.

Mudança em `SpeedLog.tsx`:
- Estado `const [measure, setMeasure] = useState<{ a: number | null; b: number | null }>({ a: null, b: null });`.
- Passar a todos os painéis `measure={measure}` e
  `onMeasure={(t, which) => setMeasure((m) => ({ ...m, [which]: t }))}`.
- Em `loaded()`, junto de `setMarker(null)`, chamar `setMeasure({ a: null, b: null })`.
- No `switch` do teclado: `case "Escape": setMeasure({ a: null, b: null }); break;`.
- Logo depois do bloco `{marker !== null && (...)}` da barra de ferramentas, um botão `Clear A/B`
  que faz o mesmo, renderizado quando `measure.a !== null || measure.b !== null`.
- No texto de ajuda da tela inicial, acrescentar `click sets A, Shift+click sets B, Esc clears`.

Mudança em `PlotPanel.tsx`:
- Em `Props`: `measure: { a: number | null; b: number | null };` e
  `onMeasure: (t: number, which: "a" | "b") => void;`. Incluir `measure` na desestruturação.
- Clique: no efeito que cria o gráfico, registrar `mousedown` e `mouseup` em `plot.over` (o uPlot
  engole o evento `click` quando há qualquer movimento, então não use `click`):
  ```tsx
  let downX: number | null = null;
  let clickTimer: number | undefined;
  const onDown = (e: MouseEvent) => { if (e.button === 0) downX = e.clientX; };
  const onUp = (e: MouseEvent) => {
    if (e.button !== 0 || downX === null || Math.abs(e.clientX - downX) >= 4) return;
    const idx = plot.cursor.idx;
    if (idx == null) return;
    const t = plot.data[0][idx];
    if (t == null) return;
    const which = e.shiftKey ? "b" : "a";
    window.clearTimeout(clickTimer);
    // Deferred so a double click (reset zoom) does not also set A.
    clickTimer = window.setTimeout(() => live.current.onMeasure(t, which), 250);
  };
  ```
  Em `onDouble` e na limpeza do efeito, chamar `window.clearTimeout(clickTimer)`. Remover os dois
  listeners na limpeza. Não altere as opções `cursor.drag` do uPlot.
- Desenho: no hook `draw`, depois do bloco do `marker`, desenhar A e B do mesmo jeito, lendo
  `live.current.measure`: linha vertical contínua (`ctx.setLineDash([])`), A com
  `cssColor("--nord8", "#88c0d0")` e B com `cssColor("--nord15", "#b48ead")`, e a letra `A`/`B` no
  topo da linha. (Canvas não aceita `var(...)`; use o helper `cssColor`.)
- Redesenho: no efeito de overlays, trocar as dependências `[showModes, modes, marker]` por
  `[showModes, modes, marker, measure.a, measure.b]`. Sem isso as linhas não aparecem nos outros
  painéis.
- Valores: `const [deltas, setDeltas] = useState<Array<number | null>>([]);` e uma função
  `computeDeltas()` que lê `plotRef.current` e `live.current.measure`: devolve `[]` se não há gráfico
  ou se A ou B é `null`; senão, com `const x = u.data[0]` e
  `const inside = (t: number) => x.length > 0 && t >= x[0] && t <= x[x.length - 1]`, para cada série
  `i >= 1` devolve `valueNear(u.data[i], u.valToIdx(b)) - valueNear(u.data[i], u.valToIdx(a))` quando
  A e B estão dentro da janela carregada e os dois valores existem, e `null` caso contrário. Chamar
  `setDeltas(computeDeltas())` logo depois de `plot?.setData(data)` e num
  `useEffect(..., [measure.a, measure.b])`.
- Exibição: no chip, quando A e B existem, mostrar `Δ = {formatValue(d)}` se `d` for número, ou
  `Δ = n/a` com `title="A or B is outside the visible window"` se for `null`. No cabeçalho do painel,
  quando A e B existem, mostrar `` `Δt = ${(b - a).toFixed(3)} s` `` e, se `b !== a`, acrescentar
  `` ` (${(1 / Math.abs(b - a)).toFixed(3)} Hz)` ``.

Aceite: clicar num pico de ATT.Roll e Shift+clicar no pico seguinte mostra Δt de cerca de um período
e a frequência correspondente; os chips mostram Δ de cada traço. As linhas A e B aparecem em todos os
painéis. Dar zoom até A sair da janela mostra `Δ = n/a`. Esc e o botão `Clear A/B` removem a medida.
Arrastar para dar zoom continua funcionando e o duplo clique reseta o zoom sem definir A.

### J7 (P2). Traços com unidades diferentes dividem o mesmo eixo Y

Problema: cada painel tem um único eixo Y. Ao juntar, por exemplo, `ATT.Roll` (deg) e uma tensão (V),
o traço de menor amplitude vira uma linha reta. O cabeçalho só lista `deg, V`.

Mudança em `PlotPanel.tsx`:
- Unidades distintas, na ordem em que aparecem (traço sem unidade conta como `""`):
  `const unitList = useMemo(() => [...new Set(panel.traces.map((t) => units[traceKey(t)] ?? ""))], [panel.traces, units]);`
- Trocar a `signature` (uma só, usada pelo efeito que cria o gráfico e pelo que busca os dados) por
  `` const signature = panel.traces.map((t) => `${traceKey(t)}@${units[traceKey(t)] ?? ""}`).join("|"); ``
  Não crie uma segunda dependência só para o efeito de criação: o gráfico seria recriado vazio.
- Dentro do efeito de criação: `const dual = unitList.length === 2;`.
  - `axes[1]` ganha `label: dual && unitList[0] ? unitList[0] : undefined`.
  - Quando `dual`, acrescentar um terceiro eixo com o mesmo estilo do segundo:
    `{ scale: "y2", side: 1, stroke: text, grid: { show: false }, ticks: { stroke: grid, width: 1 }, size: 60, label: unitList[1] || undefined, values: (_u, splits) => splits.map((v) => formatValue(v)) }`.
  - Em cada série: `scale: dual && (units[traceKey(trace)] ?? "") === unitList[1] ? "y2" : "y"`.
  - Não é preciso declarar `y2` em `scales`. Passe unidade vazia como `undefined`, nunca `""` (o
    uPlot reserva espaço para rótulo quando ele não é nulo).
- Cabeçalho: quando `unitList.length >= 3`, no lugar do span de `yUnits`, renderizar
  `<span className="warn-text sl-panel-units">mixed units: use + Panel</span>`.

Aceite: marcar ATT.Roll e um campo de outra unidade (por exemplo em BAT[0]) no mesmo painel: aparecem
dois eixos, esquerdo e direito, cada um com sua unidade, e os dois traços usam a altura toda. Com
três unidades diferentes aparece o aviso no cabeçalho.

### J8 (P3). Amostras individuais não aparecem com zoom alto

Problema: `points: { show: false }` sempre. Com zoom em poucas amostras, não dá para ver onde estão
os instantes reais de amostragem (importante para avaliar taxa de log e atrasos).

Mudança em `PlotPanel.tsx`, nas séries: trocar por
`points: { show: (u, _sidx, i0, i1) => i1 - i0 < u.bbox.width / (10 * (window.devicePixelRatio || 1)), size: 4 }`.

Aceite: com o log inteiro, só linhas. Com zoom até restarem poucas dezenas de amostras na tela,
aparecem pontos em cada amostra.

### J9 (P3). Abas mudam de lugar ao trocar de aba

Problema: as abas Plot / Parameters / Messages ficam à esquerda dos botões que só existem em Plot
(+ Panel, Presets, Modes, Reset zoom). Ao ir para Parameters esses botões somem e as abas pulam
cerca de 190 px para a direita, saindo de baixo do cursor.

Mudança em `SpeedLog.tsx`: os botões só de Plot hoje são um fragmento,
`{tab === "plot" && (<> ... </>)}`. Trocar por um `div` sempre renderizado, com os mesmos filhos:
`<div className={tab === "plot" ? "sl-plot-tools" : "sl-plot-tools hidden"}> ... </div>`.

Mudança em `speedlog.css`, depois das regras `.sl-tabs`:
```css
.sl-plot-tools { display: contents; }
.sl-plot-tools.hidden { visibility: hidden; }
```
`display: contents` mantém os filhos como itens diretos da barra (mesmos espaçamentos de hoje).

Aceite: alternar entre Plot, Parameters e Messages sem mover o mouse: as três abas ficam na mesma
posição horizontal. Em Parameters e Messages os botões de Plot não aparecem nem recebem foco por Tab.

### Limitações conhecidas do SpeedLOG que este plano não resolve

Registre no `HANDOFF.md` ao terminar o pacote J:
- `spanGaps: true` liga com uma reta os trechos sem dados; quedas de log não ficam visíveis.
- `valueNear` procura até 400 linhas de distância na tabela unida, então um traço lento pode mostrar
  no cursor um valor de até cerca de 1 s antes ou depois.
- Não há zoom manual no eixo Y nem exportação da janela visível para CSV.

---

## O que foi verificado e o que não foi

Verificado no navegador com backend simulado: as 11 telas abrem sem erro no console; HUD arrasta,
redimensiona e reseta; Arm, Disarm (com confirmação), Do Action (com confirmação), Reboot (com
confirmação), Escape e clique fora fecham diálogos; troca Satellite/Streets; seletor do Quick View;
inclusão de waypoints por clique no mapa e diálogo de Write; edição de parâmetro com destaque de
modificado; conexão DroneCAN simulada; abrir log no SpeedLOG, plotar, cursor, zoom por arraste e aba
Parameters; estado desconectado.

As instruções deste plano foram conferidas por leitura do código, não por execução: nenhuma das
mudanças foi aplicada nem testada.

Não verificado (quem executar deve conferir se cruzar com a sua tarefa):
- A janela real do Tauri (`npm run tauri`); tudo foi visto só no navegador.
- Calibration e Motor Test com o veículo desarmado: o veículo simulado recusa desarmar em voo, então
  os botões dessas telas ficaram desabilitados.
- Parameters > Compare, Load/Save file e Firmware: dependem de diálogos de arquivo do sistema.
- Aba Messages do SpeedLOG e o diálogo Vehicle logs.
- Em Motor Test, o campo `MOT_SPIN_ARM` aparece vazio no mock; provável lacuna de dados do mock, não
  investigada.
- O defeito descrito em F3 (ponto de cerca/rally criado como waypoint no app real) vem da leitura do
  código; não foi reproduzido em veículo nem em Replay.
