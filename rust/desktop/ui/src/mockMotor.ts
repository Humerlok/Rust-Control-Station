import type { Backend, Unlisten } from "./api";
import { idleMotorTest, type EscReading, type MotorFrame, type MotorSlot, type MotorTestRequest, type MotorTestState } from "./types";

type Context = {
  isConnected: () => boolean;
  isArmed: () => boolean;
  emitStatus: (severity: number, text: string) => void;
};

type MotorMock = Pick<
  Backend,
  | "motorFrame"
  | "motorStart"
  | "motorStartAll"
  | "motorStartSequence"
  | "motorStopAll"
  | "motorSetSpin"
  | "motorWatchEsc"
  | "motorStatus"
  | "onMotorTest"
>;

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
const HIGH_PERCENT = 20;

function slots(rows: Array<[number, number, string]>): MotorSlot[] {
  return rows
    .map(([testOrder, output, rotation]) => ({ label: String.fromCharCode(64 + testOrder), testOrder, output, rotation }))
    .sort((a, b) => a.testOrder - b.testOrder);
}

const QUAD_X = slots([[1, 1, "CCW"], [3, 2, "CCW"], [4, 3, "CW"], [2, 4, "CW"]]);
const HEXA_X = slots([[1, 1, "CW"], [2, 4, "CCW"], [3, 6, "CW"], [4, 2, "CCW"], [5, 3, "CW"], [6, 5, "CCW"]]);

function label(order: number) {
  return String.fromCharCode(64 + order);
}

export function createMotorMock({ isConnected, isArmed, emitStatus }: Context): MotorMock {
  const listeners = new Set<(s: MotorTestState) => void>();
  let state: MotorTestState = idleMotorTest;
  let spinning: Record<number, number> = {};
  let endsAt = 0;
  let timer: ReturnType<typeof setInterval> | null = null;
  let watching = false;

  const requireVehicle = () => {
    if (!isConnected()) throw "no vehicle connected; nothing was sent";
  };

  function escs(): EscReading[] {
    return Object.entries(spinning).map(([motor, percent]) => ({
      index: Number(motor),
      rpm: Math.round(percent * 95 + Math.random() * 20),
      voltage: 22.1 - percent * 0.02,
      current: percent * 0.35,
      temperature: 31 + Math.round(percent / 4),
    }));
  }

  function push(message?: string) {
    const running = Date.now() < endsAt;
    state = { escs: running || watching ? escs() : [], running, message: message ?? state.message };
    listeners.forEach((cb) => cb(state));
  }

  function ensureTimer() {
    if (timer) return;
    timer = setInterval(() => {
      if (Date.now() >= endsAt && Object.keys(spinning).length > 0) {
        spinning = {};
        push("Test finished");
      } else if (Object.keys(spinning).length > 0) {
        push();
      }
    }, 250);
  }

  function check(request: MotorTestRequest) {
    if (request.seconds <= 0 || request.seconds > 120) throw "the duration must be between 0 and 120 seconds";
    if (request.throttleType === "percent") {
      if (request.throttle < 0 || request.throttle > 100) throw "the throttle must be between 0 and 100 %";
      if (request.throttle > HIGH_PERCENT && !request.allowHigh) throw `${request.throttle.toFixed(0)} % is above 20 %: confirm the high throttle first`;
    }
    if (request.throttleType === "pwm" && (request.throttle < 800 || request.throttle > 2200)) throw "the PWM must be between 800 and 2200 µs";
  }

  async function begin(request: MotorTestRequest) {
    await sleep(120);
    requireVehicle();
    check(request);
    if (isArmed()) throw "the vehicle is armed; disarm it before testing motors";
    if (Date.now() < endsAt) throw "a motor test is already running; press Stop first";
  }

  const percentOf = (request: MotorTestRequest) => (request.throttleType === "pwm" ? (request.throttle - 1000) / 10 : request.throttle);

  return {
    async motorFrame(frameClass, frameType, genericCount): Promise<MotorFrame> {
      requireVehicle();
      if (frameClass === 1 && frameType === 1) return { known: true, className: "QUAD", typeName: "X", motors: QUAD_X };
      if (frameClass === 2 && frameType === 1) return { known: true, className: "HEXA", typeName: "X", motors: HEXA_X };
      const count = Math.min(12, Math.max(1, genericCount ?? 4));
      return {
        known: false,
        className: null,
        typeName: null,
        motors: Array.from({ length: count }, (_, i) => ({ label: label(i + 1), testOrder: i + 1, output: null, rotation: null })),
      };
    },
    async motorStart(request) {
      await begin(request);
      const frame = QUAD_X.find((m) => m.testOrder === request.motor);
      spinning = { [frame?.output ?? request.motor]: percentOf(request) };
      endsAt = Date.now() + request.seconds * 1000;
      ensureTimer();
      const message = `Motor ${label(request.motor)} spinning for ${request.seconds} s`;
      emitStatus(6, message);
      push(message);
      return message;
    },
    async motorStartAll(request, count) {
      await begin(request);
      spinning = Object.fromEntries(Array.from({ length: count }, (_, i) => [i + 1, percentOf(request)]));
      endsAt = Date.now() + request.seconds * 1000;
      ensureTimer();
      const message = `All ${count} motors spinning for ${request.seconds} s`;
      push(message);
      return message;
    },
    async motorStartSequence(request, count) {
      await begin(request);
      spinning = { 1: percentOf(request) };
      endsAt = Date.now() + request.seconds * count * 1000;
      ensureTimer();
      const message = `Testing ${count} motors in sequence, ${request.seconds} s each`;
      push(message);
      return message;
    },
    async motorStopAll() {
      await sleep(80);
      requireVehicle();
      spinning = {};
      endsAt = 0;
      push("Motors stopped");
      return "Motors stopped";
    },
    async motorSetSpin(which, percent) {
      await sleep(120);
      requireVehicle();
      if (isArmed()) throw "the vehicle is armed; disarm it first";
      if (percent < 0 || percent > 30) throw "the spin value must be between 0 and 30 %";
      return `MOT_SPIN_${which === "arm" ? "ARM" : "MIN"} set to ${percent.toFixed(1)} %`;
    },
    async motorWatchEsc(enabled) {
      watching = enabled;
    },
    motorStatus: async () => state,
    async onMotorTest(cb: (s: MotorTestState) => void): Promise<Unlisten> {
      listeners.add(cb);
      return () => {
        listeners.delete(cb);
      };
    },
  };
}
