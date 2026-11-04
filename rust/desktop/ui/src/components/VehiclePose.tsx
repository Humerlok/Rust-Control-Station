const TRANSFORMS: Record<string, string> = {
  Level: "rotateX(0deg)",
  Left: "rotateY(-68deg)",
  Right: "rotateY(68deg)",
  "Nose down": "rotateX(68deg)",
  "Nose up": "rotateX(-68deg)",
  Back: "rotateX(180deg)",
};

const CAPTIONS: Record<string, string> = {
  Level: "Place the vehicle level, on its landing gear",
  Left: "Place the vehicle on its LEFT side",
  Right: "Place the vehicle on its RIGHT side",
  "Nose down": "Place the vehicle NOSE DOWN",
  "Nose up": "Place the vehicle NOSE UP",
  Back: "Place the vehicle upside down (on its back)",
};

/** A top-view quadcopter tilted the way the vehicle must be placed. */
export function VehiclePose({ name }: { name: string }) {
  return (
    <figure className="pose">
      <div className="pose-stage">
        <svg viewBox="0 0 120 120" width="120" height="120" style={{ transform: TRANSFORMS[name] ?? "none" }} aria-hidden="true">
          <g stroke="var(--nord9)" strokeWidth="5" strokeLinecap="round">
            <line x1="30" y1="30" x2="90" y2="90" />
            <line x1="90" y1="30" x2="30" y2="90" />
          </g>
          {[
            [30, 30],
            [90, 30],
            [30, 90],
            [90, 90],
          ].map(([cx, cy]) => (
            <circle key={`${cx}-${cy}`} cx={cx} cy={cy} r="14" fill="none" stroke="var(--nord8)" strokeWidth="3" />
          ))}
          <rect x="45" y="45" width="30" height="30" rx="5" fill="var(--nord2)" stroke="var(--nord4)" strokeWidth="2" />
          <polygon points="60,10 52,26 68,26" fill="var(--nord13)" />
          <text x="60" y="64" textAnchor="middle" fontSize="9" fill="var(--nord4)">
            NOSE
          </text>
        </svg>
      </div>
      <figcaption>{CAPTIONS[name] ?? name}</figcaption>
    </figure>
  );
}
