# Rust Control Station beta 0.6.1

## Replay de logs de Plane
- O Replay agora identifica o tipo de veículo do log (Plane, Rover, Sub, Tracker, Blimp). Antes todo log aparecia como Copter, e o modo de um Plane virava "MODE 12".
- Nomes de modo corretos para Plane (inclui `AutoLand`).
- HUD de Plane com velocidade no ar (`AS`), acelerador (`THR`) e distância ao waypoint (`WP`).
- Dados de navegação do Plane (roll/pitch alvo, rumos, erros de altitude, cross-track e velocidade) chegam aos campos do Quick View.

## Flight Plan
- Mapa maior; a lista de waypoints ficou mais baixa.

## Limitações conhecidas
- Os campos de Plane seguem o formato de log do ArduPlane e ainda não foram testados com um log real nem com veículo real.
- Demais limitações da 0.5.0.
