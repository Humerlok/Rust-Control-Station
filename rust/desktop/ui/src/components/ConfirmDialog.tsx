import { useEffect, type ReactNode } from "react";

type Props = {
  title: string;
  message?: string;
  tone?: "default" | "warn" | "danger";
  confirmLabel?: string;
  cancelLabel?: string;
  children?: ReactNode;
  onConfirm: () => void;
  onCancel: () => void;
};

export function ConfirmDialog({
  title,
  message,
  tone = "default",
  confirmLabel = "Confirm",
  cancelLabel = "Cancel",
  children,
  onConfirm,
  onCancel,
}: Props) {
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onCancel();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onCancel]);

  const confirmClass = tone === "danger" ? "danger" : "primary";
  return (
    <div className="modal-back" onMouseDown={onCancel}>
      <div
        className={`modal ${tone === "default" ? "" : tone}`}
        role="dialog"
        aria-modal="true"
        aria-label={title}
        onMouseDown={(event) => event.stopPropagation()}
      >
        <h3>{title}</h3>
        {message && <p>{message}</p>}
        {children}
        <div className="buttons">
          <button onClick={onCancel}>{cancelLabel}</button>
          <button className={confirmClass} onClick={onConfirm} autoFocus>
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
