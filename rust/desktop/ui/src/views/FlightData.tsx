import { ActionsPanel } from "../components/ActionsPanel";
import { FlightMap } from "../components/FlightMap";
import { Hud } from "../components/Hud";
import { MessageBanner } from "../components/MessageBanner";
import { MessagesPanel } from "../components/MessagesPanel";
import { QuickView } from "../components/QuickView";
import type { StatusText, Telemetry } from "../types";

type Props = {
  telemetry: Telemetry;
  messages: StatusText[];
  onClearMessages: () => void;
  notify: (text: string, ok: boolean) => void;
};

export function FlightData({ telemetry, messages, onClearMessages, notify }: Props) {
  return (
    <div className="flight-data">
      <div className="fd-main">
        <div className="panel map-panel">
          <FlightMap telemetry={telemetry} notify={notify} />
          <Hud telemetry={telemetry} />
          <MessageBanner messages={messages} systemId={telemetry.vehicle?.systemId ?? null} />
        </div>
        <QuickView fields={telemetry.fields} />
      </div>
      <div className="fd-side">
        <MessagesPanel messages={messages} onClear={onClearMessages} />
        <ActionsPanel telemetry={telemetry} />
      </div>
    </div>
  );
}
