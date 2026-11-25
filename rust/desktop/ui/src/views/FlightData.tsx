/** Flight Data screen: map with HUD overlays, Quick View, status messages and action buttons; purely presentational over `telemetry`. */
import { ActionsPanel } from "../components/ActionsPanel";
import { FlightMap } from "../components/FlightMap";
import { GpsBadge } from "../components/GpsBadge";
import { Hud } from "../components/Hud";
import { MessageBanner } from "../components/MessageBanner";
import { MessagesPanel } from "../components/MessagesPanel";
import { QuickView } from "../components/QuickView";
import { ReplayBar } from "../components/ReplayBar";
import type { ReplayStatus, StatusText, Telemetry } from "../types";

type Props = {
  telemetry: Telemetry;
  messages: StatusText[];
  onClearMessages: () => void;
  notify: (text: string, ok: boolean) => void;
  /** Set while the link is a log replay (see `useReplay`): shows the playback bar and rebuilds the track after a jump. */
  replay: { status: ReplayStatus | null; resetKey: string };
};

export function FlightData({ telemetry, messages, onClearMessages, notify, replay }: Props) {
  const status = replay.status;
  return (
    <div className="flight-data">
      <div className="fd-main">
        <div className="panel map-panel">
          <FlightMap
            telemetry={telemetry}
            notify={notify}
            replay={status ? { resetKey: replay.resetKey, positionS: status.positionS } : null}
          />
          <GpsBadge telemetry={telemetry} />
          <Hud telemetry={telemetry} />
          <MessageBanner messages={messages} systemId={telemetry.vehicle?.systemId ?? null} />
        </div>
        {/* Outside the map panel: over the map it would cover the HUD, the alert banner and the zoom buttons. */}
        {status && <ReplayBar status={status} notify={notify} />}
        <QuickView fields={telemetry.fields} />
      </div>
      <div className="fd-side">
        <MessagesPanel messages={messages} onClear={onClearMessages} />
        <ActionsPanel telemetry={telemetry} />
      </div>
    </div>
  );
}
