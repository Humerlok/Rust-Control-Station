import { useState } from "react";
import { api } from "../api";
import { ConfirmDialog } from "./ConfirmDialog";

type Props = {
  connected: boolean;
  armed: boolean;
  notify: (text: string, ok: boolean) => void;
};

export function RebootButton({ connected, armed, notify }: Props) {
  const [open, setOpen] = useState(false);

  async function reboot() {
    setOpen(false);
    try {
      notify(`Reboot: ${await api.reboot()}`, true);
    } catch (error) {
      notify(`Reboot failed: ${String(error)}`, false);
    }
  }

  return (
    <>
      <button className="danger" disabled={!connected} onClick={() => setOpen(true)} title="Reboot the autopilot">
        Reboot
      </button>
      {open && (
        <ConfirmDialog
          title="Reboot autopilot?"
          tone={armed ? "danger" : "warn"}
          confirmLabel="Reboot now"
          message={
            armed
              ? "Vehicle is ARMED. Rebooting now may crash it."
              : "The autopilot will restart and the link will drop for a few seconds."
          }
          onConfirm={reboot}
          onCancel={() => setOpen(false)}
        />
      )}
    </>
  );
}
