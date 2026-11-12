//! Texts for the `ERR` and `EV` log messages, ported from SpeedLOG's `i18n.js`.
//!
//! `ERR` carries a subsystem number and an error code (numbers from `AP_Logger.h`, names from the
//! `LogErrorSubsystem` enum of the documentation); `EV` carries an event id (`LogEvent`).
use crate::{docs, types::Lang};

fn pick(lang: Lang, pair: (&'static str, &'static str)) -> &'static str {
    match lang {
        Lang::En => pair.0,
        Lang::Pt => pair.1,
    }
}

fn subsystem(name: &str) -> Option<(&'static str, &'static str)> {
    Some(match name {
        "MAIN" => ("Main loop", "Laço principal"),
        "RADIO" => ("RC radio", "Rádio (RC)"),
        "COMPASS" => ("Compass", "Bússola"),
        "OPTFLOW" => ("Optical flow", "Fluxo óptico"),
        "FAILSAFE_RADIO" => ("Radio failsafe", "Failsafe de rádio"),
        "FAILSAFE_BATT" => ("Battery failsafe", "Failsafe de bateria"),
        "FAILSAFE_GPS" => ("GPS failsafe", "Failsafe de GPS"),
        "FAILSAFE_GCS" => (
            "Ground station (telemetry link) failsafe",
            "Failsafe da estação de solo (link de telemetria)",
        ),
        "FAILSAFE_FENCE" => ("Geofence failsafe", "Failsafe de geofence"),
        "FLIGHT_MODE" => ("Flight mode change", "Troca de modo de voo"),
        "GPS" => ("GPS", "GPS"),
        "CRASH_CHECK" => ("Crash check", "Detecção de queda"),
        "FLIP" => ("Flip", "Flip (acrobacia)"),
        "AUTOTUNE" => ("AutoTune", "AutoTune"),
        "PARACHUTES" => ("Parachute", "Paraquedas"),
        "EKFCHECK" => ("EKF check", "Verificação do EKF"),
        "FAILSAFE_EKFINAV" => (
            "EKF / inertial navigation failsafe",
            "Failsafe do EKF / navegação inercial",
        ),
        "BARO" => ("Barometer", "Barômetro"),
        "CPU" => ("CPU", "CPU"),
        "FAILSAFE_ADSB" => ("ADS-B failsafe", "Failsafe de ADS-B"),
        "TERRAIN" => ("Terrain", "Terreno"),
        "NAVIGATION" => ("Navigation", "Navegação"),
        "FAILSAFE_TERRAIN" => ("Terrain failsafe", "Failsafe de terreno"),
        "EKF_PRIMARY" => ("Primary EKF core", "Núcleo primário do EKF"),
        "THRUST_LOSS_CHECK" => ("Thrust loss check", "Verificação de perda de empuxo"),
        "FAILSAFE_SENSORS" => ("Sensors failsafe", "Failsafe de sensores"),
        "FAILSAFE_LEAK" => ("Leak failsafe", "Failsafe de vazamento"),
        "PILOT_INPUT" => ("Pilot input", "Comando do piloto"),
        "FAILSAFE_VIBE" => ("Vibration failsafe", "Failsafe de vibração"),
        "INTERNAL_ERROR" => ("Internal error", "Erro interno"),
        "FAILSAFE_DEADRECKON" => (
            "Dead reckoning failsafe",
            "Failsafe de navegação por estimativa",
        ),
        _ => return None,
    })
}

/// Error codes that mean something specific to one subsystem.
fn specific_code(sub: &str, code: i64) -> Option<(&'static str, &'static str)> {
    Some(match (sub, code) {
        ("RADIO", 2) => (
            "RC frames arriving late",
            "Quadros do rádio chegando atrasados",
        ),
        ("MAIN", 1) => (
            "Main loop delayed waiting for the IMU (INS)",
            "Laço principal atrasado esperando a IMU (INS)",
        ),
        ("CRASH_CHECK", 1) => ("Crash detected", "Queda detectada"),
        ("CRASH_CHECK", 2) => ("Loss of control detected", "Perda de controle detectada"),
        ("FLIP", 2) => ("Flip abandoned", "Flip abandonado"),
        ("TERRAIN", 2) => ("Terrain data missing", "Dados de terreno ausentes"),
        ("NAVIGATION", 2) => (
            "Failed to set the destination",
            "Falha ao definir o destino",
        ),
        ("NAVIGATION", 3) => ("RTL restarted", "RTL reiniciado"),
        ("NAVIGATION", 4) => ("Failed to start the circle", "Falha ao iniciar o círculo"),
        ("NAVIGATION", 5) => ("Destination outside the fence", "Destino fora do geofence"),
        ("NAVIGATION", 6) => (
            "RTL is missing a rangefinder",
            "RTL sem telêmetro (rangefinder)",
        ),
        ("INTERNAL_ERROR", 1) => ("Internal errors detected", "Erros internos detectados"),
        ("THRUST_LOSS_CHECK", 1) => ("Thrust loss detected", "Perda de empuxo detectada"),
        ("PARACHUTES", 2) => (
            "Parachute not released: too low",
            "Paraquedas não liberado: altitude muito baixa",
        ),
        ("PARACHUTES", 3) => (
            "Parachute not released: already landed",
            "Paraquedas não liberado: já pousou",
        ),
        ("EKFCHECK", 2) => ("EKF variance too high", "Variância do EKF alta demais"),
        ("BARO", 2) => ("Barometer glitch", "Falha momentânea (glitch) do barômetro"),
        ("BARO", 3) => ("Bad depth reading", "Leitura de profundidade inválida"),
        ("GPS", 2) => ("GPS glitch", "Falha momentânea (glitch) do GPS"),
        _ => return None,
    })
}

fn error_code_text(lang: Lang, sub: &str, code: i64, mode_label: &str) -> String {
    match sub {
        "FLIGHT_MODE" => {
            return match lang {
                Lang::En => format!("Failed to change to flight mode {mode_label}"),
                Lang::Pt => format!("Falha ao mudar para o modo de voo {mode_label}"),
            }
        }
        "EKF_PRIMARY" => {
            return match lang {
                Lang::En => format!("Primary EKF core is now {code}"),
                Lang::Pt => format!("O núcleo primário do EKF agora é {code}"),
            }
        }
        _ => {}
    }
    if let Some(text) = specific_code(sub, code) {
        return pick(lang, text).to_string();
    }
    let text = match code {
        0 => ("Resolved (back to normal)", "Resolvido (voltou ao normal)"),
        1 if sub.starts_with("FAILSAFE_") => ("Failsafe triggered", "Failsafe acionado"),
        1 => ("Failed to initialise", "Falha ao inicializar"),
        4 => ("Unhealthy", "Não saudável (unhealthy)"),
        _ => ("unknown code", "código desconhecido"),
    };
    pick(lang, text).to_string()
}

/// One `ERR` record as text: `Battery failsafe (6): Failsafe triggered [code 1]`.
/// `mode_name` names a flight mode number, for `FLIGHT_MODE` errors.
pub fn err_text(
    lang: Lang,
    subsys: i64,
    code: i64,
    mode_name: impl Fn(i64) -> Option<String>,
) -> String {
    let code_word = pick(lang, ("code", "código"));
    match docs::enum_name("LogErrorSubsystem", subsys) {
        Some(name) => {
            let label = subsystem(name).map_or(name, |s| pick(lang, s));
            let mode =
                mode_name(code).map_or_else(|| code.to_string(), |m| format!("{m} ({code})"));
            let meaning = error_code_text(lang, name, code, &mode);
            format!("{label} ({subsys}): {meaning} [{code_word} {code}]")
        }
        None => {
            let label = pick(lang, ("Subsystem", "Subsistema"));
            let unknown = pick(lang, ("unknown code", "código desconhecido"));
            format!("{label} {subsys}: {unknown} [{code_word} {code}]")
        }
    }
}

fn event(name: &str) -> Option<(&'static str, &'static str)> {
    Some(match name {
        "ARMED" => ("Armed", "Armado"),
        "DISARMED" => ("Disarmed", "Desarmado"),
        "AUTO_ARMED" => (
            "Auto-armed (ready for automatic takeoff)",
            "Armado automaticamente (pronto para decolagem automática)",
        ),
        "LAND_COMPLETE_MAYBE" => ("Landing possibly complete", "Pouso possivelmente concluído"),
        "LAND_COMPLETE" => ("Landing complete", "Pouso concluído"),
        "LOST_GPS" => ("GPS lost", "GPS perdido"),
        "FLIP_START" => ("Flip started", "Flip iniciado"),
        "FLIP_END" => ("Flip ended", "Flip finalizado"),
        "SET_HOME" => ("Home position set", "Posição Home definida"),
        "SET_SIMPLE_ON" => ("Simple mode on", "Modo Simple ligado"),
        "SET_SIMPLE_OFF" => ("Simple mode off", "Modo Simple desligado"),
        "NOT_LANDED" => (
            "Left the ground (not landed)",
            "Saiu do solo (não está pousado)",
        ),
        "SET_SUPERSIMPLE_ON" => ("Super Simple mode on", "Modo Super Simple ligado"),
        "AUTOTUNE_INITIALISED" => ("AutoTune started", "AutoTune iniciado"),
        "AUTOTUNE_OFF" => ("AutoTune off", "AutoTune desligado"),
        "AUTOTUNE_RESTART" => ("AutoTune restarted", "AutoTune reiniciado"),
        "AUTOTUNE_SUCCESS" => ("AutoTune succeeded", "AutoTune concluído com sucesso"),
        "AUTOTUNE_FAILED" => ("AutoTune failed", "AutoTune falhou"),
        "AUTOTUNE_REACHED_LIMIT" => ("AutoTune reached a limit", "AutoTune atingiu um limite"),
        "AUTOTUNE_PILOT_TESTING" => (
            "AutoTune: pilot testing the new gains",
            "AutoTune: piloto testando os novos ganhos",
        ),
        "AUTOTUNE_SAVEDGAINS" => ("AutoTune gains saved", "Ganhos do AutoTune salvos"),
        "SAVE_TRIM" => ("Trim saved", "Trim salvo"),
        "SAVEWP_ADD_WP" => ("Waypoint saved (SaveWP)", "Waypoint salvo (SaveWP)"),
        "FENCE_ENABLE" => ("Geofence enabled", "Geofence ativado"),
        "FENCE_DISABLE" => ("Geofence disabled", "Geofence desativado"),
        "ACRO_TRAINER_OFF" => ("Acro trainer off", "Acro trainer desligado"),
        "ACRO_TRAINER_LEVELING" => ("Acro trainer: leveling", "Acro trainer: nivelamento"),
        "ACRO_TRAINER_LIMITED" => ("Acro trainer: limited", "Acro trainer: limitado"),
        "GRIPPER_GRAB" => ("Gripper closed (grab)", "Garra fechada (agarrar)"),
        "GRIPPER_RELEASE" => ("Gripper opened (release)", "Garra aberta (soltar)"),
        "PARACHUTE_DISABLED" => ("Parachute disabled", "Paraquedas desativado"),
        "PARACHUTE_ENABLED" => ("Parachute enabled", "Paraquedas ativado"),
        "PARACHUTE_RELEASED" => ("Parachute released", "Paraquedas liberado"),
        "LANDING_GEAR_DEPLOYED" => ("Landing gear deployed", "Trem de pouso baixado"),
        "LANDING_GEAR_RETRACTED" => ("Landing gear retracted", "Trem de pouso recolhido"),
        "MOTORS_EMERGENCY_STOPPED" => ("Motors emergency stopped", "Motores parados em emergência"),
        "MOTORS_EMERGENCY_STOP_CLEARED" => (
            "Motor emergency stop cleared",
            "Parada de emergência dos motores liberada",
        ),
        "MOTORS_INTERLOCK_DISABLED" => (
            "Motor interlock disabled",
            "Interlock dos motores desativado",
        ),
        "MOTORS_INTERLOCK_ENABLED" => ("Motor interlock enabled", "Interlock dos motores ativado"),
        "ROTOR_RUNUP_COMPLETE" => ("Rotor run-up complete", "Aceleração do rotor concluída"),
        "ROTOR_SPEED_BELOW_CRITICAL" => (
            "Rotor speed below critical",
            "Rotação do rotor abaixo do crítico",
        ),
        "EKF_ALT_RESET" => ("EKF altitude reset", "Reinício da altitude do EKF"),
        "LAND_CANCELLED_BY_PILOT" => (
            "Landing cancelled by the pilot",
            "Pouso cancelado pelo piloto",
        ),
        "EKF_YAW_RESET" => ("EKF yaw reset", "Reinício do yaw do EKF"),
        "AVOIDANCE_ADSB_ENABLE" => ("ADS-B avoidance enabled", "Desvio por ADS-B ativado"),
        "AVOIDANCE_ADSB_DISABLE" => ("ADS-B avoidance disabled", "Desvio por ADS-B desativado"),
        "AVOIDANCE_PROXIMITY_ENABLE" => (
            "Proximity avoidance enabled",
            "Desvio por proximidade ativado",
        ),
        "AVOIDANCE_PROXIMITY_DISABLE" => (
            "Proximity avoidance disabled",
            "Desvio por proximidade desativado",
        ),
        "GPS_PRIMARY_CHANGED" => ("Primary GPS changed", "GPS primário trocado"),
        "ZIGZAG_STORE_A" => ("ZigZag: point A stored", "ZigZag: ponto A armazenado"),
        "ZIGZAG_STORE_B" => ("ZigZag: point B stored", "ZigZag: ponto B armazenado"),
        "LAND_REPO_ACTIVE" => (
            "Landing repositioning active",
            "Reposicionamento de pouso ativo",
        ),
        "STANDBY_ENABLE" => ("Standby enabled", "Standby ativado"),
        "STANDBY_DISABLE" => ("Standby disabled", "Standby desativado"),
        "FENCE_ALT_MAX_ENABLE" => (
            "Max altitude fence enabled",
            "Limite de altitude máxima ativado",
        ),
        "FENCE_ALT_MAX_DISABLE" => (
            "Max altitude fence disabled",
            "Limite de altitude máxima desativado",
        ),
        "FENCE_CIRCLE_ENABLE" => ("Circle fence enabled", "Fence circular ativado"),
        "FENCE_CIRCLE_DISABLE" => ("Circle fence disabled", "Fence circular desativado"),
        "FENCE_ALT_MIN_ENABLE" => (
            "Min altitude fence enabled",
            "Limite de altitude mínima ativado",
        ),
        "FENCE_ALT_MIN_DISABLE" => (
            "Min altitude fence disabled",
            "Limite de altitude mínima desativado",
        ),
        "FENCE_POLYGON_ENABLE" => ("Polygon fence enabled", "Fence poligonal ativado"),
        "FENCE_POLYGON_DISABLE" => ("Polygon fence disabled", "Fence poligonal desativado"),
        "EK3_SOURCES_SET_TO_PRIMARY" => (
            "EKF3 sources: primary set",
            "Fontes do EKF3: conjunto primário",
        ),
        "EK3_SOURCES_SET_TO_SECONDARY" => (
            "EKF3 sources: secondary set",
            "Fontes do EKF3: conjunto secundário",
        ),
        "EK3_SOURCES_SET_TO_TERTIARY" => (
            "EKF3 sources: tertiary set",
            "Fontes do EKF3: conjunto terciário",
        ),
        "AIRSPEED_PRIMARY_CHANGED" => (
            "Primary airspeed sensor changed",
            "Sensor de velocidade do ar primário trocado",
        ),
        "SURFACED" => ("Surfaced", "Na superfície"),
        "NOT_SURFACED" => ("No longer at the surface", "Fora da superfície"),
        "BOTTOMED" => ("Reached the bottom", "Chegou ao fundo"),
        "NOT_BOTTOMED" => ("No longer at the bottom", "Fora do fundo"),
        "EKF_MAG_OFFSETS_SAVED" => (
            "EKF magnetometer offsets saved",
            "Offsets do magnetômetro do EKF salvos",
        ),
        _ => return None,
    })
}

/// One `EV` record as text: `Armed (10)`.
pub fn ev_text(lang: Lang, id: i64) -> String {
    match docs::enum_name("LogEvent", id) {
        Some(name) => {
            let text = event(name).map_or_else(
                || name.to_lowercase().replace('_', " "),
                |e| pick(lang, e).to_string(),
            );
            format!("{text} ({id})")
        }
        None => format!("{} {id}", pick(lang, ("Event", "Evento"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn err_texts() {
        let none = |_| None;
        assert_eq!(
            err_text(Lang::En, 6, 1, none),
            "Battery failsafe (6): Failsafe triggered [code 1]"
        );
        assert_eq!(
            err_text(Lang::Pt, 6, 0, none),
            "Failsafe de bateria (6): Resolvido (voltou ao normal) [código 0]"
        );
        assert_eq!(
            err_text(Lang::En, 12, 1, none),
            "Crash check (12): Crash detected [code 1]"
        );
        let modes = |n| (n == 6).then(|| "RTL".to_string());
        assert_eq!(
            err_text(Lang::En, 10, 6, modes),
            "Flight mode change (10): Failed to change to flight mode RTL (6) [code 6]"
        );
        assert_eq!(
            err_text(Lang::En, 250, 3, none),
            "Subsystem 250: unknown code [code 3]"
        );
    }

    #[test]
    fn ev_texts() {
        assert_eq!(ev_text(Lang::En, 10), "Armed (10)");
        assert_eq!(ev_text(Lang::Pt, 11), "Desarmado (11)");
        assert_eq!(ev_text(Lang::En, 9999), "Event 9999");
    }
}
