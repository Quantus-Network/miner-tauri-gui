declare module "canvas-confetti" {
  // Minimal ambient types to satisfy TS without installing @types
  // This matches the common usage pattern: default export function with .create and .reset.

  export interface ConfettiOptions {
    // Commonly used options (non-exhaustive)
    particleCount?: number;
    angle?: number;
    spread?: number;
    startVelocity?: number;
    decay?: number;
    gravity?: number;
    drift?: number;
    scalar?: number;
    ticks?: number;
    colors?: string[];
    shapes?: Array<"square" | "circle" | string>;
    origin?: { x?: number; y?: number };
    zIndex?: number;
    disableForReducedMotion?: boolean;
  }

  export interface CreateOptions {
    resize?: boolean;
    useWorker?: boolean;
  }

  export interface ConfettiInstance {
    (options?: ConfettiOptions): Promise<null>;
    reset: () => void;
  }

  // Default export callable function with static methods.
  // Usage:
  //  import confetti from "canvas-confetti";
  //  await confetti({ particleCount: 100 });
  //  const myConfetti = confetti.create(canvas, { resize: true });
  //  await myConfetti({ particleCount: 50 });
  //  confetti.reset();
  const confetti: {
    (options?: ConfettiOptions): Promise<null>;
    create: (canvas: HTMLCanvasElement, options?: CreateOptions) => ConfettiInstance;
    reset: () => void;
  };

  export default confetti;
}
